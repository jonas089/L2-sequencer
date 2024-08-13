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
//use std::thread::sleep;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use types::{
    Block, BlockCommitment, ConsensusCommitment, GenericPublicKey, GenericSignature,
    GenericTransactionData,
};
struct InMemoryServerState {
    block_state: Arc<Mutex<InMemoryBlockStore>>,
    pool_state: Arc<Mutex<InMemoryTransactionPool>>,
    consensus_state: Arc<Mutex<InMemoryConsensus>>,
    local_gossipper: Arc<Mutex<Gossipper>>,
}

async fn synchronization_loop(database: Arc<Mutex<InMemoryServerState>>) {
    // todo: synchronize Blocks with other nodes
    // fetch if the height is > this node's
    // verify the signatures and threshold
    // store valid blocks
    // when a block is found and synchronized the pool_state and consensus_state are rest for height += 1
}

async fn consensus_loop(state: Arc<Mutex<InMemoryServerState>>) {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let unix_timestamp = since_the_epoch.as_secs() as u32;
    let state_lock = state.lock().await;
    let block_state_lock = state_lock.block_state.lock().await;
    let local_gossipper = state_lock.local_gossipper.lock().await;
    let last_block_unix_timestamp = block_state_lock
        .get_block_by_height(block_state_lock.height)
        .timestamp;
    let consensus_lock = state_lock.consensus_state.lock().await;
    if unix_timestamp > (last_block_unix_timestamp + config::consensus::ACCUMULATION_PHASE_DURATION)
    {
        // commit to consensus
        let random_zk_commitment = generate_random_number(
            consensus_lock.local_validator.to_sec1_bytes().to_vec(),
            consensus_lock.height.to_be_bytes().to_vec(),
        );
        let commitment = ConsensusCommitment {
            validator: consensus_lock.local_validator.to_sec1_bytes().to_vec(),
            receipt: random_zk_commitment, // to be added: Signature
        };
        let consensus_gossip = local_gossipper
            .gossip_consensus_commitment(commitment)
            .await;

        println!("Consensus Gossip Responses: {:?}", &consensus_gossip);
    }
    let mut consensus_state_lock = state_lock.consensus_state.lock().await;
    if unix_timestamp
        > (last_block_unix_timestamp
            + config::consensus::ACCUMULATION_PHASE_DURATION
            + config::consensus::COMMITMENT_PHASE_DURATION)
            // this is an issue, since this can include invalid commitments, todo: check the commitments first!
        && consensus_state_lock.commitments.len() as u32 > CONSENSUS_THRESHOLD
    {
        if consensus_state_lock.commitments.len() as u32 > CONSENSUS_THRESHOLD {
            let round_winner: GenericPublicKey =
                evaluate_commitments(consensus_state_lock.commitments.clone());
            consensus_state_lock.round_winner = Some(deserialize_vk(&round_winner));
        }
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
    let pool_state: InMemoryTransactionPool = InMemoryTransactionPool::empty(0);
    let consensus_state: InMemoryConsensus = InMemoryConsensus::empty_with_default_validators(0);
    let local_gossipper: Gossipper = Gossipper {
        peers: PEERS.to_vec(),
        client: Client::new(),
    };
    let shared_state: Arc<Mutex<InMemoryServerState>> = Arc::new(Mutex::new(InMemoryServerState {
        block_state: Arc::new(Mutex::new(block_state)),
        pool_state: Arc::new(Mutex::new(pool_state)),
        consensus_state: Arc::new(Mutex::new(consensus_state)),
        local_gossipper: Arc::new(Mutex::new(local_gossipper)),
    }));
    tokio::spawn(synchronization_loop(Arc::clone(&shared_state)));
    tokio::spawn(consensus_loop(Arc::clone(&shared_state)));
    let api = Router::new()
        .route("/get/pool", get(get_pool))
        .route("/get/commitments", get(get_commitments))
        .route("/get/block/:height", get(get_block))
        .route("/schedule", post(schedule))
        .route("/commit", post(commit))
        .route("/propose", post(propose))
        .layer(DefaultBodyLimit::max(10000000))
        .layer(Extension(shared_state));
    let listener = tokio::net::TcpListener::bind(
        env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()),
    )
    .await
    .unwrap();
    axum::serve(listener, api).await.unwrap();
}

async fn schedule(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(transaction): Json<GenericTransactionData>,
) -> String {
    let state = shared_state.lock().await;
    let success_response =
        format!("Transaction is being sequenced: {:?}", &transaction).to_string();
    state
        .pool_state
        .lock()
        .await
        .insert_transaction(transaction);
    success_response
}

async fn commit(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(commitment): Json<ConsensusCommitment>,
) -> String {
    let state = shared_state.lock().await;
    let success_response = format!("Commitment was accepted: {:?}", &commitment).to_string();
    state
        .consensus_state
        .lock()
        .await
        .insert_commitment(commitment);
    success_response
}

async fn propose(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(mut proposal): Json<Block>,
) -> String {
    let state = shared_state.lock().await;
    let mut consensus_lock = state.consensus_state.lock().await;
    let error_response = format!("Block was rejected: {:?}", &proposal).to_string();
    // if the block is complete, store it and reset memory db
    // if the block is incomplete, attest to it (in case this node hasn't yet done that)
    // and gossip it
    let block_signature = proposal
        .signature
        .clone()
        .expect("Block has not been signed!");
    if let Some(round_winner) = consensus_lock.round_winner {
        let signature_deserialized = Signature::from_slice(&block_signature).unwrap();
        match round_winner.verify(&proposal.to_bytes(), &signature_deserialized) {
            Ok(_) => {
                // sign the block if it has not been signed yet
                let mut is_signed = false;
                let block_commitments = proposal.commitments.clone();
                let mut commitment_count: u32 = 0;
                for commitment in block_commitments {
                    let commitment_vk = deserialize_vk(&commitment.validator);
                    if consensus_lock.validators.contains(&commitment_vk) {
                        match commitment_vk.verify(
                            &proposal.to_bytes(),
                            &Signature::from_slice(&commitment.signature).unwrap(),
                        ) {
                            Ok(_) => commitment_count += 1,
                            Err(_) => eprintln!("Invalid signature, skipping commitment!"),
                        }
                    }
                    if commitment.validator
                        == consensus_lock.local_validator.to_sec1_bytes().to_vec()
                    {
                        is_signed = true;
                    }
                }
                if commitment_count >= CONSENSUS_THRESHOLD {
                    let mut block_state_lock = state.block_state.lock().await;
                    let previous_block_height = block_state_lock.height;
                    // todo: verify Block height
                    block_state_lock.insert_block(previous_block_height, proposal.clone());
                    consensus_lock.reinitialize(previous_block_height + 1);
                } else if !is_signed {
                    // sign the proposal
                    let mut local_sk = consensus_lock.local_signing_key.clone();
                    let block_bytes = proposal.to_bytes();
                    let signature: Signature = local_sk.sign(&block_bytes);
                    let signature_serialized: GenericSignature = signature.to_bytes().to_vec();
                    //////////////////////////////////////////////////////
                    //                  Todo: factor this out           //
                    //////////////////////////////////////////////////////
                    let start = SystemTime::now();
                    let since_the_epoch = start
                        .duration_since(UNIX_EPOCH)
                        .expect("Time went backwards");
                    let unix_timestamp = since_the_epoch.as_secs() as u32;
                    //////////////////////////////////////////////////////
                    let commitment = BlockCommitment {
                        signature: signature_serialized,
                        validator: consensus_lock
                            .local_validator
                            .to_sec1_bytes()
                            .to_vec()
                            .clone(),
                        timestamp: unix_timestamp,
                    };
                    proposal.commitments.push(commitment);
                }
            }
            Err(_) => {
                eprintln!("Invalid Signature for Round Winner, Block Proposal Rejected!");
                return error_response;
            }
        }
    }
    let gossiper_lock = state.local_gossipper.lock().await;
    let responses: Vec<String> = gossiper_lock.gossip_pending_block(proposal).await;
    serde_json::to_string(&responses).unwrap()
}

async fn get_pool(Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>) -> String {
    let state = shared_state.lock().await;
    let pool_state = state.pool_state.lock().await;
    format!("{:?}", pool_state.transactions)
}

async fn get_commitments(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
) -> String {
    let state = shared_state.lock().await;
    let consensus_state = state.consensus_state.lock().await;
    format!("{:?}", consensus_state.commitments)
}

async fn get_block(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Path(height): Path<u32>,
) -> String {
    let state = shared_state.lock().await;
    let block_state = state.block_state.lock().await;
    serde_json::to_string(&block_state.get_block_by_height(height)).unwrap()
}

#[tokio::test]
async fn test_schedule_transaction() {
    let client = Client::new();
    let raw_data: String = serde_json::to_string(&vec![1, 2, 3, 4, 5]).unwrap();
    let response = client
        .post("http://127.0.0.1:8080/schedule")
        .header("Content-Type", "application/json")
        .body(raw_data)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.text().await.unwrap(),
        "Transaction is being sequenced: [1, 2, 3, 4, 5]"
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        config::consensus::{v1_sk_deserialized, v1_vk_deserialized},
        crypto::ecdsa::Keypair,
        types::{GenericTransactionData, Transaction},
    };

    #[tokio::test]
    async fn test_commit() {
        let keypair: Keypair = Keypair {
            sk: v1_sk_deserialized(),
            vk: v1_vk_deserialized(),
        };
    }
}
