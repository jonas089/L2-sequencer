#[cfg(not(feature = "sqlite"))]
use crate::state::server::{InMemoryBlockStore, InMemoryTransactionPool};
#[cfg(feature = "sqlite")]
use crate::state::server::{SqLiteBlockStore, SqLiteTransactionPool};
use axum::{extract::Path, Extension, Json};
use colored::Colorize;
use k256::ecdsa::{signature::Verifier, Signature};
use l2_sequencer::config::consensus::ROUND_DURATION;
use patricia_trie::store::types::Node;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    consensus::logic::{current_round, evaluate_commitment, get_committing_validator},
    crypto::ecdsa::deserialize_vk,
    handlers::handle_block_proposal,
    types::{Block, ConsensusCommitment, Transaction},
    ServerState,
};

pub async fn schedule(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
    Json(transaction): Json<Transaction>,
) -> String {
    let mut state = shared_state.write().await;
    let success_response =
        format!("[Ok] Transaction is being sequenced: {:?}", &transaction).to_string();
    state.pool_state.insert_transaction(transaction);
    success_response
}

pub async fn commit(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
    Json(commitment): Json<ConsensusCommitment>,
) -> String {
    let mut state_lock = shared_state.write().await;
    let success_response = format!("[Ok] Commitment was accepted: {:?}", &commitment).to_string();
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
    if !state_lock.consensus_state.round_winner.is_some() {
        // no round winner found, commitment might be valid
        let validator = get_committing_validator(
            last_block_unix_timestamp,
            state_lock.consensus_state.validators.clone(),
        );
        // todo: check if commitment signature is valid for validator
        if deserialize_vk(&commitment.validator) == validator {
            let winner =
                evaluate_commitment(commitment, state_lock.consensus_state.validators.clone());
            state_lock.consensus_state.round_winner = Some(winner);
        }
    }
    success_response
}

pub async fn propose(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
    Json(mut proposal): Json<Block>,
) -> String {
    let mut state_lock = shared_state.write().await;
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
    let error_response = format!("Block was rejected: {:?}", &proposal).to_string();

    let round = current_round(last_block_unix_timestamp);
    if proposal.timestamp < last_block_unix_timestamp + ((round - 1) * (ROUND_DURATION)) {
        println!(
            "[Warning] Invalid Proposal Timestamp: {}",
            proposal.timestamp
        );
        return error_response;
    };

    let block_signature = proposal
        .signature
        .clone()
        .expect("Block has not been signed!");

    if let Some(round_winner) = state_lock.consensus_state.round_winner {
        let signature_deserialized = Signature::from_slice(&block_signature).unwrap();
        match round_winner.verify(&proposal.to_bytes(), &signature_deserialized) {
            Ok(_) => {
                let res =
                    handle_block_proposal(&mut state_lock, &mut proposal, error_response).await;
                match res {
                    Some(e) => return e,
                    None => {}
                }
            }
            Err(_) => {
                println!(
                    "{}",
                    format_args!(
                        "{} Invalid Signature for Round Winner, Proposal rejected",
                        "[Warning]".yellow(),
                    )
                );
                return error_response;
            }
        }
        "[Ok] Block was processed".to_string()
    } else {
        "[Warning] Awaiting consensus evaluation".to_string()
    }
}

pub async fn merkle_proof(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
    Json(key): Json<Vec<u8>>,
) -> String {
    let mut state_lock = shared_state.write().await;
    let trie_root = state_lock.merkle_trie_root.clone();
    let merkle_proof = patricia_trie::merkle::merkle_proof(
        &mut state_lock.merkle_trie_state,
        key,
        Node::Root(trie_root),
    );
    match merkle_proof {
        Some(merkle_proof) => serde_json::to_string(&merkle_proof).unwrap(),
        None => "[Err] Failed to generate Merkle Proof for Transaction".to_string(),
    }
}

pub async fn get_pool(Extension(shared_state): Extension<Arc<RwLock<ServerState>>>) -> String {
    let state = shared_state.read().await;

    #[cfg(not(feature = "sqlite"))]
    {
        format!("{:?}", state.pool_state.transactions)
    }

    #[cfg(feature = "sqlite")]
    {
        format!("{:?}", state.pool_state.get_all_transactions())
    }
}

pub async fn get_commitments(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
) -> String {
    let state_lock = shared_state.read().await;
    format!("{:?}", state_lock.consensus_state.commitments)
}

pub async fn get_block(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
    Path(height): Path<u32>,
) -> String {
    let state_lock = shared_state.read().await;
    println!(
        "{}",
        format_args!("{} Peer Requested Block #{}", "[Info]".green(), height)
    );
    #[cfg(not(feature = "sqlite"))]
    let previous_block_height = state_lock.block_state.height - 1;

    #[cfg(feature = "sqlite")]
    let previous_block_height = state_lock.block_state.current_block_height();

    if previous_block_height < height + 1 {
        "[Warning] Requested Block that does not exist".to_string()
    } else {
        match serde_json::to_string(&state_lock.block_state.get_block_by_height(height)) {
            Ok(block_json) => block_json,
            Err(e) => e.to_string(),
        }
    }
}

pub async fn get_state_root_hash(
    Extension(shared_state): Extension<Arc<RwLock<ServerState>>>,
) -> String {
    let state_lock = shared_state.read().await;
    match serde_json::to_string(&state_lock.merkle_trie_root) {
        Ok(trie_root_json) => trie_root_json,
        Err(e) => e.to_string(),
    }
}

pub async fn get_height(Extension(shared_state): Extension<Arc<RwLock<ServerState>>>) -> String {
    let state_lock = shared_state.read().await;
    #[cfg(not(feature = "sqlite"))]
    let previous_block_height = state_lock.block_state.height - 1;

    #[cfg(feature = "sqlite")]
    let previous_block_height = state_lock.block_state.current_block_height();
    serde_json::to_string(&previous_block_height).unwrap()
}
