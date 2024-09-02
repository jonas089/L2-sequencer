mod api;
mod config;
mod consensus;
mod crypto;
mod gossipper;
mod state;
mod types;
use api::{commit, get_block, get_commitments, get_pool, propose, schedule};
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Extension, Router,
};
use colored::*;
use config::{consensus::CONSENSUS_THRESHOLD, network::PEERS};
use consensus::logic::evaluate_commitments;
use crypto::ecdsa::deserialize_vk;
use gossipper::Gossipper;
use k256::ecdsa::{signature::SignerMut, Signature};
use patricia_trie::{
    insert_leaf,
    store::{
        db::InMemoryDB as InMemoryMerkleTrie,
        types::{Hashable, Key, Leaf, Node, Root},
    },
};
use prover::generate_random_number;
use rand::Rng;
use reqwest::{Client, Response};
use state::server::{InMemoryBlockStore, InMemoryConsensus, InMemoryTransactionPool};
use std::{
    collections::HashMap,
    env,
    hash::Hash,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use types::{Block, ConsensusCommitment, GenericPublicKey};
struct InMemoryServerState {
    block_state: InMemoryBlockStore,
    pool_state: InMemoryTransactionPool,
    consensus_state: InMemoryConsensus,
    merkle_trie_state: InMemoryMerkleTrie,
    merkle_trie_root: Root,
    local_gossipper: Gossipper,
}

async fn synchronization_loop(database: Arc<Mutex<InMemoryServerState>>) {
    let mut state_lock = database.lock().await;
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.height)
        .timestamp;
    if get_current_time()
        > last_block_unix_timestamp
            + config::consensus::ACCUMULATION_PHASE_DURATION
            + config::consensus::COMMITMENT_PHASE_DURATION
    {
        let next_height = state_lock.consensus_state.height + 1;
        let gossipper = Gossipper {
            peers: PEERS.to_vec(),
            client: Client::new(),
        };
        for peer in gossipper.peers {
            if peer == env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                continue;
            }
            let response: Option<Response> = match gossipper
                .client
                .get(format!("http://{}{}{}", &peer, "/get/block/", next_height))
                .timeout(Duration::from_secs(3))
                .send()
                .await
            {
                Ok(response) => Some(response),
                Err(_) => None,
            };
            match response {
                Some(response) => {
                    let block_serialized = response.text().await.unwrap();
                    if block_serialized != "[Warning] Requested Block that does not exist" {
                        let block: Block = serde_json::from_str(&block_serialized).unwrap();
                        let block_height = block.height;
                        state_lock
                            .block_state
                            .insert_block(next_height - 1, block.clone());
                        // insert transactions into the trie
                        let mut root_node = Node::Root(state_lock.merkle_trie_root.clone());
                        for transaction in &block.transactions {
                            let mut leaf = Leaf::new(Vec::new(), Some(transaction.data.clone()));
                            leaf.hash();
                            leaf.key = leaf
                                .hash
                                .clone()
                                .unwrap()
                                .iter()
                                .flat_map(|&byte| (0..8).rev().map(move |i| (byte >> i) & 1))
                                .collect();
                            let new_root = insert_leaf(
                                &mut state_lock.merkle_trie_state,
                                &mut leaf,
                                root_node,
                            );
                            root_node = Node::Root(new_root);
                        }
                        // update in-memory trie root
                        state_lock.merkle_trie_root = root_node.unwrap_as_root();
                        state_lock.consensus_state.reinitialize(block_height + 1);
                        println!(
                            "{}",
                            format_args!("{} Synchronized Block", "[Info]".green())
                        );
                        println!(
                            "{}",
                            format_args!(
                                "{} New Trie Root: {:?}",
                                "[Info]".green(),
                                state_lock.merkle_trie_root.hash
                            )
                        );
                    }
                }
                None => {
                    println!(
                        "{}",
                        format_args!("{} Resource is Busy", "[Warning]".yellow())
                    )
                }
            }
        }
    }
}

async fn consensus_loop(state: Arc<Mutex<InMemoryServerState>>) {
    let unix_timestamp = get_current_time();
    let mut state_lock = state.lock().await;
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.height)
        .timestamp;
    println!(
        "{}",
        format_args!(
            "{} Unix Timestamp: {} Target: {}",
            "[Info]".green(),
            unix_timestamp,
            (last_block_unix_timestamp + config::consensus::ACCUMULATION_PHASE_DURATION)
        )
    );
    if unix_timestamp > (last_block_unix_timestamp + config::consensus::ACCUMULATION_PHASE_DURATION)
        && !state_lock.consensus_state.proposed
    {
        println!(
            "{}",
            format_args!("{} Generating ZK Random Number", "[Info]".green())
        );
        // commit to consensus
        let random_zk_commitment = generate_random_number(
            state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec(),
            state_lock.consensus_state.height.to_be_bytes().to_vec(),
        );
        let commitment = ConsensusCommitment {
            validator: state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec(),
            receipt: random_zk_commitment, // to be added: Signature
        };
        println!(
            "{}",
            format_args!("{} Gossipping Consensus Commitment", "[Info]".green())
        );
        state_lock
            .consensus_state
            .commitments
            .push(commitment.clone());
        let _ = state_lock
            .local_gossipper
            .gossip_consensus_commitment(commitment)
            .await;
        state_lock.consensus_state.proposed = true;
    }
    if unix_timestamp
        > (last_block_unix_timestamp
            + config::consensus::ACCUMULATION_PHASE_DURATION
            + config::consensus::COMMITMENT_PHASE_DURATION)
            // this is an issue, since this can include invalid commitments, todo: check the commitments first!
        && state_lock.consensus_state.commitments.len() as u32 >= CONSENSUS_THRESHOLD && !state_lock.consensus_state.committed
    {
        let round_winner: GenericPublicKey =
            evaluate_commitments(state_lock.consensus_state.commitments.clone());
        state_lock.consensus_state.round_winner = Some(deserialize_vk(&round_winner));
        // if this node won the round it will propose the new Block
        // currently there is no fallback in case the selected validator fails to propose
        // this needs to be addressed to prevent the network from getting stuck
        let unix_timestamp = get_current_time();
        if round_winner
            == state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec()
        {
            let mut proposed_block = Block {
                height: state_lock.consensus_state.height,
                signature: None,
                transactions: state_lock
                    .pool_state
                    .transactions
                    .values()
                    .cloned()
                    .collect(),
                commitments: None,
                timestamp: unix_timestamp,
            };
            let mut signing_key = state_lock.consensus_state.local_signing_key.clone();
            let signature: Signature = signing_key.sign(&proposed_block.to_bytes());
            proposed_block.signature = Some(signature.to_bytes().to_vec());
            let _ = state_lock
                .local_gossipper
                .gossip_pending_block(proposed_block)
                .await;
            println!(
                "{}",
                format_args!("{} Block was proposed", "[Info]".green())
            );
            // since this node was selected as a validator and submitted a proposal,
            // the transaction pool must be reset
            state_lock.pool_state.reinitialize();
        }
        state_lock.consensus_state.committed = true;
    }
}

#[tokio::main]
async fn main() {
    println!(
        "{}\n{}",
        r#"
██████╗  ██████╗ ██████╗ ██████╗       ███████╗ ██████╗ 
██╔══██╗██╔═══██╗██╔══██╗██╔══██╗      ██╔════╝██╔═══██╗
██████╔╝██║   ██║██████╔╝██║  ██║█████╗███████╗██║   ██║
██╔═══╝ ██║   ██║██╔══██╗██║  ██║╚════╝╚════██║██║▄▄ ██║
██║     ╚██████╔╝██║  ██║██████╔╝      ███████║╚██████╔╝
╚═╝      ╚═════╝ ╚═╝  ╚═╝╚═════╝       ╚══════╝ ╚══▀▀═╝"#
            .blue()
            .bold(),
        "Compact, General Purpose, Semi-Decentralized, Sequencer"
            .bold()
            .italic()
            .magenta()
    );
    let mut block_state: InMemoryBlockStore = InMemoryBlockStore::empty();
    block_state.trigger_genesis(0u32);
    let pool_state: InMemoryTransactionPool = InMemoryTransactionPool::empty();
    let consensus_state: InMemoryConsensus = InMemoryConsensus::empty_with_default_validators(0);
    let merkle_trie_state: InMemoryMerkleTrie = InMemoryMerkleTrie {
        nodes: HashMap::new(),
    };
    let merkle_trie_root: Root = Root::empty();
    let local_gossipper: Gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };
    let shared_state: Arc<Mutex<InMemoryServerState>> = Arc::new(Mutex::new(InMemoryServerState {
        block_state,
        pool_state,
        consensus_state,
        merkle_trie_state,
        merkle_trie_root,
        local_gossipper,
    }));
    let host_with_port = env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string());
    let formatted_msg = format!(
        "{}{}",
        "Starting Node: ".green().italic(),
        &host_with_port.yellow().bold()
    );
    println!("{}", formatted_msg);

    let synchronization_task = tokio::spawn({
        let shared_state = Arc::clone(&shared_state);
        async move {
            loop {
                // for now the loop syncs one block at a time, this can be optimized
                synchronization_loop(Arc::clone(&shared_state)).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });
    let consensus_task = tokio::spawn({
        let shared_state = Arc::clone(&shared_state);
        async move {
            loop {
                consensus_loop(Arc::clone(&shared_state)).await;
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    });

    let api_task = tokio::spawn({
        async move {
            let api = Router::new()
                .route("/get/pool", get(get_pool))
                .route("/get/commitments", get(get_commitments))
                .route("/get/block/:height", get(get_block))
                .route("/schedule", post(schedule))
                .route("/commit", post(commit))
                .route("/propose", post(propose))
                .layer(DefaultBodyLimit::max(10000000))
                .layer(Extension(shared_state));

            let listener = tokio::net::TcpListener::bind(&host_with_port)
                .await
                .unwrap();
            axum::serve(listener, api).await.unwrap();
        }
    });

    tokio::select! {
        sync_task_res = synchronization_task => {
            match sync_task_res {
                Ok(_) => println!("{}", format_args!("{} Synchronization task concluded without error", "[Warning]".yellow())),
                Err(e) => println!("{}", format_args!("{} Synchronization task failed with error: {}", "[Error]".red(), e))
            }
        },
        consensus_task_res = consensus_task => {
            match consensus_task_res {
                Ok(_) => println!("{}", format_args!("{} Consensus task concluded without error", "[Warning]".yellow())),
                Err(e) => println!("{}", format_args!("{} Consensus task failed with error: {}", "[Error]".red(), e))
            }
        },
        api_task_res = api_task => {
            match api_task_res{
                Ok(_) => println!("{}", format_args!("{} API task concluded without error", "[Warning]".yellow())),
                Err(e) => println!("{}", format_args!("{} API task failed with error: {}", "[Error]".red(), e))
            }
        }
    }
}

pub fn get_current_time() -> u32 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_secs() as u32
}

#[tokio::test]
async fn test_schedule_transactions() {
    use crate::types::Transaction;
    let client = Client::new();
    let transaction: Transaction = Transaction {
        data: vec![1, 2, 3, 4, 5, 6, 7],
        timestamp: 0,
    };
    let transaction_json: String = serde_json::to_string(&transaction).unwrap();
    // note that currently a transaction may only be submitted to one node
    // mishandling this can cause the network to crash
    let _ = client
        .post("http://127.0.0.1:8080/schedule")
        .header("Content-Type", "application/json")
        .body(transaction_json.clone())
        .send()
        .await
        .unwrap();
    /*assert_eq!(
        response.text().await.unwrap(),
        "[Ok] Transaction is being sequenced: Transaction { data: [1, 2, 3, 4, 5], timestamp: 0 }"
    );*/
    // submit to other node aswell - since only the validator's pool will be included in the Block
    /*let _ = client
        .post("http://127.0.0.1:8081/schedule")
        .header("Content-Type", "application/json")
        .body(transaction_json)
        .send()
        .await
        .unwrap();*/
}

#[cfg(test)]
mod tests {
    use crate::{config::network::PEERS, gossipper::Gossipper, types::ConsensusCommitment};
    use prover::generate_random_number;
    use reqwest::Client;
    use std::env;
    #[tokio::test]
    async fn test_commit() {
        let receipt = generate_random_number(vec![0; 32], vec![0; 32]);
        let consensus_commitment: ConsensusCommitment = ConsensusCommitment {
            validator: vec![0; 32],
            receipt,
        };
        let gossipper = Gossipper {
            peers: PEERS.to_vec(),
            client: Client::new(),
        };
        env::set_var("API_HOST_WITH_PORT", "127.0.0.1:8081");
        gossipper
            .gossip_consensus_commitment(consensus_commitment)
            .await;
    }
}
