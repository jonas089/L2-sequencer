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
    consensus::{CLEARING_PHASE_DURATION, ROUND_DURATION},
    network::PEERS,
};
use consensus::logic::{current_round, get_committing_validator};
use crypto::ecdsa::Keypair;
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
use types::{Block, ConsensusCommitment};
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
    #[cfg(not(feature = "sqlite"))]
    let previous_block_height = state_lock.block_state.height - 1;

    #[cfg(feature = "sqlite")]
    let previous_block_height = state_lock.block_state.current_block_height() - 1;

    let next_height = previous_block_height + 1;
    println!("[Info] Starting Synchronisation, target: {}", &next_height);
    let gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };

    for peer in gossipper.peers {
        // todo: make this generic for n amount of nodes
        let this_node = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
        if this_node == "0.0.0.0:8080" && peer == "rust-node-1:8080" {
            continue;
        } else if this_node == "0.0.0.0:8081" && peer == "rust-node-2:8081" {
            continue;
        } else if this_node == "0.0.0.0:8082" && peer == "rust-node-3:8082" {
            continue;
        } else if this_node == "0.0.0.0:8083" && peer == "rust-node-4:8083" {
            continue;
        }
        let response: Option<Response> = match gossipper
            .client
            .get(format!("http://{}{}{}", &peer, "/get/block/", next_height))
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(response) => Some(response),
            Err(e) => {
                println!("[Warning] Synchronization failed with: {}", &e);
                None
            }
        };
        match response {
            Some(response) => {
                let block_serialized = response.text().await.unwrap();
                if block_serialized != "[Warning] Requested Block that does not exist" {
                    let block: Block = serde_json::from_str(&block_serialized).unwrap();
                    #[cfg(not(feature = "sqlite"))]
                    state_lock
                        .block_state
                        .insert_block(next_height - 1, block.clone());

                    #[cfg(feature = "sqlite")]
                    state_lock
                        .block_state
                        .insert_block(next_height, block.clone());

                    // insert transactions into the trie
                    let mut root_node = Node::Root(state_lock.merkle_trie_root.clone());
                    let transactions = &block.transactions;
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
            _ => {}
        }
    }
}

async fn consensus_loop(state: Arc<Mutex<ServerState>>) {
    let unix_timestamp = get_current_time();
    let mut state_lock = state.lock().await;

    #[cfg(not(feature = "sqlite"))]
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.height - 1)
        .timestamp;

    #[cfg(feature = "sqlite")]
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.current_block_height() - 1)
        .timestamp;

    // check if clearing phase of new consensus round
    if unix_timestamp
        <= last_block_unix_timestamp
            + ((((unix_timestamp - last_block_unix_timestamp) / (ROUND_DURATION)) * ROUND_DURATION)
                + CLEARING_PHASE_DURATION)
    {
        state_lock.consensus_state.reinitialize();
        // establish finality over the most recent block
        return;
    }

    let committing_validator = get_committing_validator(
        last_block_unix_timestamp,
        state_lock.consensus_state.validators.clone(),
    );

    // todo: remove
    let mut keypair = Keypair::new();
    keypair.vk = committing_validator.clone();
    println!("[Info] Committing Validator: {:?}", keypair.serialize_vk());

    println!(
        "[Info] Current round: {}",
        current_round(last_block_unix_timestamp)
    );

    #[cfg(not(feature = "sqlite"))]
    let previous_block_height = state_lock.block_state.height - 1;

    #[cfg(feature = "sqlite")]
    let previous_block_height = state_lock.block_state.current_block_height() - 1;

    if state_lock.consensus_state.local_validator == committing_validator
        && !state_lock.consensus_state.committed
    {
        let random_zk_number = generate_random_number(
            state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec(),
            (previous_block_height + 1).to_be_bytes().to_vec(),
        );
        let commitment = ConsensusCommitment {
            validator: state_lock
                .consensus_state
                .local_validator
                .to_sec1_bytes()
                .to_vec(),
            receipt: random_zk_number,
        };
        let _ = state_lock
            .local_gossipper
            .gossip_consensus_commitment(commitment.clone())
            .await;
        state_lock.consensus_state.committed = true;
    }

    if state_lock.consensus_state.round_winner.is_none() {
        return;
    }

    let proposing_validator = state_lock.consensus_state.round_winner.unwrap();
    #[cfg(not(feature = "sqlite"))]
    let transactions = state_lock
        .pool_state
        .transactions
        .values()
        .cloned()
        .collect();
    #[cfg(feature = "sqlite")]
    let transactions = state_lock.pool_state.get_all_transactions();
    if state_lock.consensus_state.local_validator == proposing_validator
        && !state_lock.consensus_state.proposed
    {
        let mut proposed_block = Block {
            height: previous_block_height + 1,
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
        state_lock.consensus_state.proposed = true;
        state_lock.pool_state.reinitialize()
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
                tokio::time::sleep(Duration::from_secs(20)).await;
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
