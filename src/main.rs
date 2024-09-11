mod api;
mod config;
mod consensus;
mod crypto;
mod gossipper;
mod state;
mod types;
use api::{
    commit, get_block, get_commitments, get_pool, get_state_root_hash, merkle_proof, propose,
    schedule,
};
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Extension, Router,
};
use colored::*;
use config::{
    consensus::{
        ACCUMULATION_PHASE_DURATION, COMMITMENT_PHASE_DURATION, CONSENSUS_THRESHOLD, ROUND_DURATION,
    },
    network::PEERS,
};
use consensus::logic::evaluate_commitments;
use crypto::ecdsa::deserialize_vk;
use gossipper::Gossipper;
use k256::ecdsa::{signature::SignerMut, Signature};

#[cfg(feature = "sqlite")]
use patricia_trie::{
    insert_leaf,
    store::{
        db::sql::TrieDB as MerkleTrieDB,
        types::{Hashable, Leaf, Node, Root},
    },
};
#[cfg(not(feature = "sqlite"))]
use patricia_trie::{
    insert_leaf,
    store::{
        db::TrieDB as MerkleTrieDB,
        types::{Hashable, Leaf, Node, Root},
    },
};

use prover::generate_random_number;
use reqwest::{Client, Response};
#[cfg(not(feature = "sqlite"))]
use state::server::{InMemoryBlockStore, InMemoryTransactionPool};

#[cfg(feature = "sqlite")]
use patricia_trie::store::db::{sql, Database};
use state::server::{BlockStore, InMemoryConsensus, TransactionPool};
#[cfg(feature = "sqlite")]
use state::server::{SqLiteBlockStore, SqLiteTransactionPool};
use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use types::{Block, ConsensusCommitment, GenericPublicKey};
struct ServerState {
    block_state: BlockStore,
    pool_state: TransactionPool,
    consensus_state: InMemoryConsensus,
    merkle_trie_state: MerkleTrieDB,
    merkle_trie_root: Root,
    local_gossipper: Gossipper,
}

async fn synchronization_loop(database: Arc<Mutex<ServerState>>) {
    let mut state_lock = database.lock().await;
    let next_height = state_lock.block_state.height;
    let gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };
    for peer in gossipper.peers {
        // todo: make this generic for n amount of nodes
        let this_node = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
        if this_node == "0.0.0.0:8080" && peer == "rust-node-1:8080" {
            continue;
        } else if this_node == "0.0.0.0:8081" && peer == "rust-node-2:8080" {
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
                    state_lock
                        .block_state
                        .insert_block(next_height - 1, block.clone());
                    // insert transactions into the trie
                    let mut root_node = Node::Root(state_lock.merkle_trie_root.clone());
                    #[cfg(not(feature = "sqlite"))]
                    let transactions = &block.transactions;
                    #[cfg(feature = "sqlite")]
                    let transactions = &state_lock.pool_state.get_all_transactions();
                    for transaction in transactions {
                        let mut leaf = Leaf::new(Vec::new(), Some(transaction.data.clone()));
                        leaf.hash();
                        leaf.key = leaf
                            .hash
                            .clone()
                            .unwrap()
                            .iter()
                            .flat_map(|&byte| (0..8).rev().map(move |i| (byte >> i) & 1))
                            .collect();
                        leaf.hash();
                        let new_root =
                            insert_leaf(&mut state_lock.merkle_trie_state, &mut leaf, root_node);
                        root_node = Node::Root(new_root);
                    }
                    // update trie root
                    state_lock.merkle_trie_root = root_node.unwrap_as_root();
                    state_lock.consensus_state.reinitialize();
                    println!(
                        "{}",
                        format_args!("{} Synchronized Block: {}", "[Info]".green(), next_height)
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

async fn consensus_loop(state: Arc<Mutex<ServerState>>) {
    let unix_timestamp = get_current_time();
    let mut state_lock = state.lock().await;
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.height - 1)
        .timestamp;
    let round = (get_current_time() - last_block_unix_timestamp)
        / (COMMITMENT_PHASE_DURATION + ACCUMULATION_PHASE_DURATION + ROUND_DURATION)
        + 1;
    println!(
        "Round winner length: {:?}",
        &state_lock.consensus_state.round_winners.len()
    );
    let comm_size = match state_lock
        .consensus_state
        .commitments
        .get(round as usize - 1)
    {
        Some(c) => c.len(),
        None => 0,
    };
    println!("Commitments: {}", comm_size);
    println!("[Info] Commitment round: {} / 10", &round);
    if unix_timestamp
        > (last_block_unix_timestamp
            + (COMMITMENT_PHASE_DURATION)
            + ((round - 1) * (COMMITMENT_PHASE_DURATION + ACCUMULATION_PHASE_DURATION)))
        && !state_lock.consensus_state.committed[round as usize - 1]
    {
        println!(
            "{}",
            format_args!("{} Generating ZK Random Number", "[Info]".green())
        );
        let random_zk_commitment = generate_random_number(
            state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec(),
            state_lock.block_state.height.to_be_bytes().to_vec(),
        );
        let commitment = ConsensusCommitment {
            validator: state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec(),
            receipt: random_zk_commitment,
        };
        match state_lock
            .consensus_state
            .commitments
            .get_mut(round as usize - 1)
        {
            Some(r) => r.push(commitment.clone()),
            None => state_lock
                .consensus_state
                .commitments
                .push(vec![commitment.clone()]),
        }
        println!(
            "{}",
            format_args!("{} Gossipping Consensus Commitment", "[Info]".green())
        );
        let _ = state_lock
            .local_gossipper
            .gossip_consensus_commitment(commitment)
            .await;
        if round > state_lock.consensus_state.committed.len() as u32 {
            panic!("Exceeded the consensus round limit for this block, the network got stuck!");
        }
        state_lock.consensus_state.committed[round as usize - 1] = true;
    }

    let round_commitment_count: u32 = match state_lock
        .consensus_state
        .commitments
        .get(round as usize - 1)
    {
        Some(c) => c.len() as u32,
        None => 0,
    };

    if unix_timestamp
        > (last_block_unix_timestamp
            + ACCUMULATION_PHASE_DURATION
            + COMMITMENT_PHASE_DURATION
            + ((round - 1) * (ACCUMULATION_PHASE_DURATION + COMMITMENT_PHASE_DURATION)))
        && round_commitment_count >= CONSENSUS_THRESHOLD
        && !state_lock.consensus_state.proposed[round as usize - 1]
    {
        println!("[Info] Choosing new round winner: {}", &round);
        let round_winner: GenericPublicKey = evaluate_commitments(
            state_lock
                .consensus_state
                .commitments
                .get(round as usize - 1)
                .unwrap()
                .clone(),
        );
        /*match state_lock
            .consensus_state
            .round_winners
            .get_mut(round as usize - 1)
        {
            Some(_) => println!("[Err] Round winner exists prior to consensus evaluation phase"),
            None => {
                state_lock
                    .consensus_state
                    .round_winners
                    .push(deserialize_vk(&round_winner));
            }
        }*/

        state_lock
            .consensus_state
            .round_winners
            .push(deserialize_vk(&round_winner));

        // if this node won the round it will propose the new Block
        let unix_timestamp = get_current_time();
        if round_winner
            == state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec()
        {
            #[cfg(not(feature = "sqlite"))]
            let transactions = state_lock
                .pool_state
                .transactions
                .values()
                .cloned()
                .collect();
            #[cfg(feature = "sqlite")]
            let transactions = state_lock.pool_state.get_all_transactions();
            let mut proposed_block = Block {
                height: state_lock.block_state.height,
                signature: None,
                transactions,
                commitments: None,
                timestamp: unix_timestamp,
            };
            let mut signing_key = state_lock.consensus_state.local_signing_key.clone();
            let signature: Signature = signing_key.sign(&proposed_block.to_bytes());
            proposed_block.signature = Some(signature.to_bytes().to_vec());
            println!(
                "{}",
                format_args!("{} Gossipping proposed Block", "[Info]".green())
            );
            let _ = state_lock
                .local_gossipper
                .gossip_pending_block(proposed_block, last_block_unix_timestamp)
                .await;
            println!(
                "{}",
                format_args!("{} Block was proposed successfully", "[Info]".green())
            );
            state_lock.pool_state.reinitialize();
        }
        if round > state_lock.consensus_state.committed.len() as u32 {
            panic!("Exceeded the consensus round limit for this block, the network got stuck!");
        }
        state_lock.consensus_state.proposed[round as usize - 1] = true;
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
    #[cfg(feature = "sqlite")]
    let mut block_state = {
        let block_state: BlockStore = BlockStore {
            height: 1,
            db_path: env::var("PATH_TO_DB").unwrap_or("database.sqlite".to_string()),
        };
        block_state.setup();
        block_state
    };
    #[cfg(not(feature = "sqlite"))]
    let mut block_state = {
        let block_state: BlockStore = BlockStore::empty();
        block_state
    };
    block_state.trigger_genesis(get_current_time());
    #[cfg(not(feature = "sqlite"))]
    let pool_state: TransactionPool = TransactionPool::empty();
    #[cfg(feature = "sqlite")]
    let pool_state: TransactionPool = {
        let pool_state: TransactionPool = TransactionPool {
            size: 0,
            db_path: env::var("PATH_TO_DB").unwrap_or("database.sqlite".to_string()),
        };
        pool_state.setup();
        pool_state
    };
    let consensus_state: InMemoryConsensus = InMemoryConsensus::empty_with_default_validators();
    #[cfg(not(feature = "sqlite"))]
    let merkle_trie_state: MerkleTrieDB = MerkleTrieDB {
        nodes: HashMap::new(),
    };
    #[cfg(feature = "sqlite")]
    let merkle_trie_state: MerkleTrieDB = MerkleTrieDB {
        path: env::var("PATH_TO_DB").unwrap_or("database.sqlite".to_string()),
        cache: None,
    };
    #[cfg(feature = "sqlite")]
    merkle_trie_state.setup();
    let merkle_trie_root: Root = Root::empty();
    let local_gossipper: Gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };
    let shared_state: Arc<Mutex<ServerState>> = Arc::new(Mutex::new(ServerState {
        block_state,
        pool_state,
        consensus_state,
        merkle_trie_state,
        merkle_trie_root,
        local_gossipper,
    }));
    let host_with_port = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
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
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    });
    let consensus_task = tokio::spawn({
        let shared_state = Arc::clone(&shared_state);
        async move {
            loop {
                consensus_loop(Arc::clone(&shared_state)).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });

    let api_task = tokio::spawn({
        async move {
            let api = Router::new()
                .route("/get/pool", get(get_pool))
                .route("/get/commitments", get(get_commitments))
                .route("/get/block/:height", get(get_block))
                .route("/get/state_root_hash", get(get_state_root_hash))
                .route("/schedule", post(schedule))
                .route("/commit", post(commit))
                .route("/propose", post(propose))
                .route("/merkle_proof", post(merkle_proof))
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
