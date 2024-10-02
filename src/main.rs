mod api;
mod config;
mod consensus;
mod crypto;
mod gossipper;
mod handlers;
mod state;
mod types;
use api::{
    commit, get_block, get_commitments, get_height, get_pool, get_state_root_hash, merkle_proof,
    propose, schedule,
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
use consensus::logic::{current_round, evaluate_commitment, get_committing_validator};
use gossipper::{docker_skip_self, Gossipper};
use handlers::handle_synchronization_response;
use k256::ecdsa::{signature::SignerMut, Signature};

#[cfg(not(feature = "sqlite"))]
use patricia_trie::store::{db::TrieDB as MerkleTrieDB, types::Root};
#[cfg(feature = "sqlite")]
use patricia_trie::{
    insert_leaf,
    store::{
        db::sql::TrieDB as MerkleTrieDB,
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
use tokio::sync::RwLock;
use types::{Block, ConsensusCommitment};
struct ServerState {
    block_state: BlockStore,
    pool_state: TransactionPool,
    consensus_state: InMemoryConsensus,
    merkle_trie_state: MerkleTrieDB,
    merkle_trie_root: Root,
    local_gossipper: Gossipper,
}

#[allow(unused)]
async fn synchronization_loop_with_finality(database: Arc<RwLock<ServerState>>) {
    let mut state_lock = database.write().await;
    #[cfg(not(feature = "sqlite"))]
    let next_height = state_lock.block_state.height;

    #[cfg(feature = "sqlite")]
    let next_height = state_lock.block_state.current_block_height();

    let gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };

    for peer in gossipper.peers {
        // todo: make this generic for n amount of nodes
        let this_node = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
        if docker_skip_self(&this_node, &peer) {
            continue;
        }
        let response: Option<Response> = match gossipper
            .client
            .get(format!("http://{}{}", &peer, "/get/height"))
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(response) => Some(response),
            Err(_) => None,
        };
        match response {
            Some(response) => {
                let peer_height: Option<u32> = match response.text().await {
                    Ok(height) => Some(serde_json::from_str(&height).unwrap()),
                    Err(_) => None,
                };
                if peer_height.is_none() {
                    continue;
                } else {
                    let mut peer_height_unwrapped = peer_height.unwrap();
                    while peer_height_unwrapped >= next_height {
                        // get & store peer block
                        // todo: implement a finality threshold
                        peer_height_unwrapped -= 1;
                    }
                }
            }
            _ => {}
        }
    }
}

#[deprecated]
async fn synchronization_loop(database: Arc<RwLock<ServerState>>) {
    let mut state_lock = database.write().await;
    #[cfg(not(feature = "sqlite"))]
    let next_height = state_lock.block_state.height - 1;

    #[cfg(feature = "sqlite")]
    let next_height = state_lock.block_state.current_block_height();

    let gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };

    for peer in gossipper.peers {
        // todo: make this generic for n amount of nodes
        let this_node = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
        if docker_skip_self(&this_node, &peer) {
            continue;
        }
        let response: Option<Response> = match gossipper
            .client
            .get(format!("http://{}{}{}", &peer, "/get/block/", next_height))
            .timeout(Duration::from_secs(15))
            .send()
            .await
        {
            Ok(response) => Some(response),
            Err(_) => None,
        };
        match response {
            Some(response) => {
                handle_synchronization_response(&mut state_lock, response, next_height).await;
            }
            _ => {}
        }
    }
}

async fn consensus_loop(state: Arc<RwLock<ServerState>>) {
    let unix_timestamp = get_current_time();
    let mut state_lock = state.write().await;

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
        return;
    }

    let committing_validator = get_committing_validator(
        last_block_unix_timestamp,
        state_lock.consensus_state.validators.clone(),
    );

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
        let proposing_validator =
            evaluate_commitment(commitment, state_lock.consensus_state.validators.clone());
        state_lock.consensus_state.round_winner = Some(proposing_validator);
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
    let shared_state: Arc<RwLock<ServerState>> = Arc::new(RwLock::new(ServerState {
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
                tokio::time::sleep(Duration::from_secs(120)).await;
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
                .route("/get/height", get(get_height))
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
