use std::sync::Arc;

use axum::{extract::Path, Extension, Json};
use colored::Colorize;
use k256::ecdsa::{
    signature::{SignerMut, Verifier},
    Signature,
};
use tokio::sync::Mutex;

use crate::{
    config::consensus::CONSENSUS_THRESHOLD,
    crypto::ecdsa::deserialize_vk,
    get_current_time,
    types::{Block, BlockCommitment, ConsensusCommitment, GenericSignature, Transaction},
    InMemoryServerState,
};

pub async fn schedule(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(transaction): Json<Transaction>,
) -> String {
    let mut state = shared_state.lock().await;
    let success_response =
        format!("[Ok] Transaction is being sequenced: {:?}", &transaction).to_string();
    state.pool_state.insert_transaction(transaction);
    success_response
}

pub async fn commit(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(commitment): Json<ConsensusCommitment>,
) -> String {
    let mut state = shared_state.lock().await;
    let success_response = format!("[Ok] Commitment was accepted: {:?}", &commitment).to_string();
    state.consensus_state.insert_commitment(commitment);
    success_response
}

pub async fn propose(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
    Json(mut proposal): Json<Block>,
) -> String {
    println!("{}", format!("{} Proposal was received", "[Info]".green()));
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
                                println!(
                                    "{}",
                                    format!(
                                        "{} Invalid Commitment was Ignored",
                                        "[Warning]".yellow()
                                    )
                                )
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
                    // todo: insert block transations into trie
                    state_lock
                        .consensus_state
                        .reinitialize(previous_block_height + 1);
                } else if !is_signed {
                    let mut local_sk = state_lock.consensus_state.local_signing_key.clone();
                    let block_bytes = proposal.to_bytes();
                    let signature: Signature = local_sk.sign(&block_bytes);
                    let signature_serialized: GenericSignature = signature.to_bytes().to_vec();
                    let unix_timestamp = get_current_time();
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
                } else {
                    println!(
                        "{}",
                        format!(
                            "{} Block is signed but lacks commitments",
                            "[Warning]".yellow()
                        )
                    );
                }
            }
            Err(_) => {
                println!(
                    "{}",
                    format!(
                        "{} Invalid Signature for Round Winner, Proposal rejected",
                        "[Warning]".yellow()
                    )
                );
                return error_response;
            }
        }
        "[Ok] Block was processed".to_string()
    } else {
        "[Err] Awaiting consensus evaluation".to_string()
    }
}

pub async fn get_pool(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
) -> String {
    let state = shared_state.lock().await;
    format!("{:?}", state.pool_state.transactions)
}

pub async fn get_commitments(
    Extension(shared_state): Extension<Arc<Mutex<InMemoryServerState>>>,
) -> String {
    let state_lock = shared_state.lock().await;
    format!("{:?}", state_lock.consensus_state.commitments)
}

pub async fn get_block(
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
