mod config;
mod consensus;
mod crypto;
mod gossipper;
mod state;
mod types;
use axum::extract::Path;
use axum::routing::post;
use axum::Json;
use axum::{extract::DefaultBodyLimit, routing::get, Extension, Router};
use colored::*;
use config::consensus::CONSENSUS_THRESHOLD;
use config::network::PEERS;
use consensus::logic::evaluate_commitments;
use crypto::ecdsa::deserialize_vk;
use gossipper::Gossipper;
use k256::ecdsa::signature::{SignerMut, Verifier};
use k256::ecdsa::Signature;
use prover::generate_random_number;
use reqwest::Client;
use state::server::{InMemoryBlockStore, InMemoryConsensus, InMemoryTransactionPool};
use std::env;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::timeout;
use types::{
    Block, BlockCommitment, ConsensusCommitment, GenericPublicKey, GenericSignature, Transaction,
};
struct InMemoryServerState {
    block_state: InMemoryBlockStore,
    pool_state: InMemoryTransactionPool,
    consensus_state: InMemoryConsensus,
    local_gossipper: Gossipper,
}

async fn synchronization_loop(database: Arc<Mutex<InMemoryServerState>>) {
    tokio::spawn(async move {
        if let Ok(mut state_lock) = timeout(Duration::from_secs(5), database.lock()).await {
            let next_height = state_lock.consensus_state.height + 1;
            let gossipper = Gossipper {
                peers: PEERS.to_vec(),
                client: Client::new(),
            };
            for peer in gossipper.peers {
                if peer == &env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                    continue;
                }
                let response = gossipper
                    .client
                    .get(format!("http://{}{}{}", &peer, "/get/block/", next_height))
                    .send()
                    .await
                    .unwrap();
                let block_serialized = response.text().await.unwrap();
                if block_serialized != "[Warning] Requested Block that does not exist".to_string() {
                    let block: Block = serde_json::from_str(&block_serialized).unwrap();
                    state_lock.block_state.insert_block(next_height - 1, block);
                    state_lock.consensus_state.height += 1;
                    println!("{}", format!("{} Synchronized Block", "[Info]".green()));
                }
            }
        } else {
            println!("{}", format!("{} Synchronization Timeout", "[Error]".red()));
        }
    });
}

async fn consensus_loop(state: Arc<Mutex<InMemoryServerState>>) {
    let unix_timestamp = get_current_time();
    let mut state_lock = state.lock().await;
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.height)
        .timestamp;
    println!(
        "Unix Timestamp: {} Target: {}",
        unix_timestamp,
        (last_block_unix_timestamp + config::consensus::ACCUMULATION_PHASE_DURATION)
    );
    if unix_timestamp > (last_block_unix_timestamp + config::consensus::ACCUMULATION_PHASE_DURATION)
        && !state_lock.consensus_state.proposed
    {
        println!(
            "{}",
            format!("{} Generating ZK Random Number", "[Info]".green())
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
            format!("{} Gossipping Consensus Commitment", "[Info]".green())
        );
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
            println!("{}", format!("{} Block was proposed", "[Info]".green()));
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
    let local_gossipper: Gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };
    let shared_state: Arc<Mutex<InMemoryServerState>> = Arc::new(Mutex::new(InMemoryServerState {
        block_state,
        pool_state,
        consensus_state,
        local_gossipper,
    }));
    let host_with_port = env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string());
    let formatted_msg = format!(
        "{}{}",
        "Starting Node: ".green().italic(),
        &host_with_port.yellow().bold()
    );
    println!("{}", formatted_msg);

    tokio::spawn({
        let shared_state = Arc::clone(&shared_state);
        async move {
            loop {
                // for now the loop syncs one block at a time, this can be optimized
                synchronization_loop(Arc::clone(&shared_state)).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });
    tokio::spawn({
        let shared_state = Arc::clone(&shared_state);
        async move {
            loop {
                consensus_loop(Arc::clone(&shared_state)).await;
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    });

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

async fn schedule(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(transaction): Json<Transaction>,
) -> String {
    let mut state = shared_state.lock().await;
    let success_response =
        format!("Transaction is being sequenced: {:?}", &transaction).to_string();
    state.pool_state.insert_transaction(transaction);
    success_response
}

async fn commit(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(commitment): Json<ConsensusCommitment>,
) -> String {
    println!("Received Commitment: {:?}", &commitment.receipt.journal);
    let mut state = shared_state.lock().await;
    let success_response = format!("Commitment was accepted: {:?}", &commitment).to_string();
    state.consensus_state.insert_commitment(commitment);
    success_response
}

async fn propose(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(mut proposal): Json<Block>,
) -> String {
    let mut state_lock: tokio::sync::MutexGuard<InMemoryServerState> = shared_state.lock().await;
    let error_response = format!("Block was rejected: {:?}", &proposal).to_string();
    // if the block is complete, store it and reset memory db
    // if the block is incomplete, attest to it (in case this node hasn't yet done that)
    // and gossip it
    let block_signature = proposal
        .signature
        .clone()
        .expect("Block has not been signed!");
    if let Some(round_winner) = state_lock.consensus_state.round_winner {
        let signature_deserialized = Signature::from_slice(&block_signature).unwrap();
        match round_winner.verify(&proposal.to_bytes(), &signature_deserialized) {
            Ok(_) => {
                // sign the block if it has not been signed yet
                let mut is_signed = false;
                let block_commitments = proposal.commitments.clone().unwrap_or(Vec::new());
                let mut commitment_count: u32 = 0;
                for commitment in block_commitments {
                    let commitment_vk = deserialize_vk(&commitment.validator);
                    if state_lock
                        .consensus_state
                        .validators
                        .contains(&commitment_vk)
                    {
                        match commitment_vk.verify(
                            &proposal.to_bytes(),
                            &Signature::from_slice(&commitment.signature).unwrap(),
                        ) {
                            Ok(_) => commitment_count += 1,
                            Err(_) => {
                                eprintln!("[Warning] Invalid signature, skipping commitment!")
                            }
                        }
                    }
                    if commitment.validator
                        == state_lock
                            .consensus_state
                            .local_validator
                            .to_sec1_bytes()
                            .to_vec()
                    {
                        is_signed = true;
                    }
                }
                if commitment_count >= CONSENSUS_THRESHOLD {
                    println!("{}", format!("{} Received Valid Block", "[Info]".green()));
                    let previous_block_height = state_lock.block_state.height;
                    // todo: verify Block height
                    state_lock
                        .block_state
                        .insert_block(previous_block_height, proposal.clone());
                    println!("{}", format!("{} Block was stored", "[Info]".green()));
                    state_lock
                        .consensus_state
                        .reinitialize(previous_block_height + 1);
                } else if !is_signed {
                    // sign the proposal
                    let mut local_sk = state_lock.consensus_state.local_signing_key.clone();
                    let block_bytes = proposal.to_bytes();
                    let signature: Signature = local_sk.sign(&block_bytes);
                    let signature_serialized: GenericSignature = signature.to_bytes().to_vec();
                    //////////////////////////////////////////////////////
                    //                  Todo: factor this out           //
                    //////////////////////////////////////////////////////
                    let unix_timestamp = get_current_time();
                    //////////////////////////////////////////////////////
                    let commitment = BlockCommitment {
                        signature: signature_serialized,
                        validator: state_lock
                            .consensus_state
                            .local_validator
                            .to_sec1_bytes()
                            .to_vec()
                            .clone(),
                        timestamp: unix_timestamp,
                    };
                    match proposal.commitments.as_mut() {
                        Some(commitments) => commitments.push(commitment),
                        None => proposal.commitments = Some(vec![commitment]),
                    }
                    let _ = state_lock
                        .local_gossipper
                        .gossip_pending_block(proposal)
                        .await;
                }
            }
            Err(_) => {
                eprintln!("[Warning] Invalid Signature for Round Winner, Block Proposal Rejected!");
                return error_response;
            }
        }
    }
    "Ok".to_string()
}

async fn get_pool(Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>) -> String {
    let state = shared_state.lock().await;
    format!("{:?}", state.pool_state.transactions)
}

async fn get_commitments(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
) -> String {
    let state_lock = shared_state.lock().await;
    format!("{:?}", state_lock.consensus_state.commitments)
}

async fn get_block(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Path(height): Path<u32>,
) -> String {
    let state_lock = shared_state.lock().await;
    println!(
        "{}",
        format!("{} Trying to get Block #{}", "[Info]".green(), height)
    );
    if state_lock.block_state.height < height {
        "[Warning] Requested Block that does not exist".to_string()
    } else {
        serde_json::to_string(&state_lock.block_state.get_block_by_height(height)).unwrap()
    }
}

fn get_current_time() -> u32 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_secs() as u32
}

#[tokio::test]
async fn test_schedule_transaction() {
    let client = Client::new();
    let transaction: Transaction = Transaction {
        data: vec![1, 2, 3, 4, 5],
        timestamp: 0u32,
    };
    let transaction_json: String = serde_json::to_string(&transaction).unwrap();
    let response = client
        .post("http://127.0.0.1:8080/schedule")
        .header("Content-Type", "application/json")
        .body(transaction_json)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.text().await.unwrap(),
        "Transaction is being sequenced: Transaction { data: [1, 2, 3, 4, 5], timestamp: 0 }"
    )
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
            receipt: receipt,
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
