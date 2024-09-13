#[cfg(not(feature = "sqlite"))]
use crate::state::server::{InMemoryBlockStore, InMemoryTransactionPool};
#[cfg(feature = "sqlite")]
use crate::state::server::{SqLiteBlockStore, SqLiteTransactionPool};
use axum::{extract::Path, Extension, Json};
use colored::Colorize;
use k256::ecdsa::{
    signature::{SignerMut, Verifier},
    Signature,
};
use l2_sequencer::config::consensus::ROUND_DURATION;
use patricia_trie::{
    insert_leaf,
    store::types::{Hashable, Leaf, Node},
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    config::consensus::CONSENSUS_THRESHOLD,
    consensus::logic::{current_round, evaluate_commitment, get_committing_validator},
    crypto::ecdsa::deserialize_vk,
    get_current_time,
    types::{Block, BlockCommitment, ConsensusCommitment, GenericSignature, Transaction},
    ServerState,
};

pub async fn schedule(
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
    Json(transaction): Json<Transaction>,
) -> String {
    let mut state = shared_state.lock().await;
    let success_response =
        format!("[Ok] Transaction is being sequenced: {:?}", &transaction).to_string();
    state.pool_state.insert_transaction(transaction);
    success_response
}

pub async fn commit(
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
    Json(commitment): Json<ConsensusCommitment>,
) -> String {
    let mut state_lock = shared_state.lock().await;
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
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
    Json(mut proposal): Json<Block>,
) -> String {
    let mut state_lock: tokio::sync::MutexGuard<ServerState> = shared_state.lock().await;
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
                let early_revert: bool = match &state_lock.consensus_state.lowest_block {
                    Some(v) => {
                        if proposal.to_bytes() < v.clone() {
                            state_lock.consensus_state.lowest_block = Some(proposal.to_bytes());
                            false
                        } else if proposal.to_bytes() == v.clone() {
                            false
                        } else {
                            true
                        }
                    }
                    None => {
                        state_lock.consensus_state.lowest_block = Some(proposal.to_bytes());
                        false
                    }
                };
                if early_revert {
                    return error_response;
                }
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
                                    format_args!(
                                        "{} Invalid Commitment was Ignored",
                                        "[Warning]".yellow()
                                    )
                                )
                            }
                        }
                    } else {
                        println!("[Err] Invalid Proposal found with invalid VK")
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
                println!(
                    "[Info] Commitment count for proposal: {}",
                    &commitment_count
                );

                #[cfg(not(feature = "sqlite"))]
                let previous_block_height = state_lock.block_state.height - 1;
                #[cfg(feature = "sqlite")]
                let previous_block_height = state_lock.block_state.current_block_height() - 1;

                if commitment_count >= CONSENSUS_THRESHOLD {
                    println!(
                        "{}",
                        format_args!("{} Received Valid Block", "[Info]".green())
                    );

                    #[cfg(not(feature = "sqlite"))]
                    state_lock
                        .block_state
                        .insert_block(proposal.height - 1, proposal.clone());

                    #[cfg(feature = "sqlite")]
                    state_lock
                        .block_state
                        .insert_block(proposal.height, proposal.clone());
                    // insert transactions into the trie
                    let mut root_node = Node::Root(state_lock.merkle_trie_root.clone());
                    for transaction in &proposal.transactions {
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
                    // update in-memory trie root
                    state_lock.merkle_trie_root = root_node.unwrap_as_root();
                    println!(
                        "{}",
                        format_args!("{} Block was stored: {}", "[Info]".green(), proposal.height)
                    );
                    println!(
                        "{}",
                        format_args!(
                            "{} New Trie Root: {:?}",
                            "[Info]".green(),
                            state_lock.merkle_trie_root.hash
                        )
                    );
                    //state_lock.consensus_state.reinitialize();
                } else if !is_signed
                    && !state_lock.consensus_state.signed
                    // only signing proposals for the current height
                    && (previous_block_height + 1 == proposal.height)
                {
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
                    println!("[Info] Signed Block is being gossipped");
                    let last_block_unix_timestamp = state_lock
                        .block_state
                        .get_block_by_height(previous_block_height)
                        .timestamp;

                    let _ = state_lock
                        .local_gossipper
                        .gossip_pending_block(proposal.clone(), last_block_unix_timestamp)
                        .await;
                    // allow signing of infinite lower blocks
                    // state_lock.consensus_state.signed = true;
                } else {
                    println!(
                        "{}",
                        format_args!(
                            "{} Block is signed but lacks commitments",
                            "[Warning]".yellow()
                        )
                    );
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
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
    Json(key): Json<Vec<u8>>,
) -> String {
    let mut state_lock: tokio::sync::MutexGuard<ServerState> = shared_state.lock().await;
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

pub async fn get_pool(Extension(shared_state): Extension<Arc<Mutex<ServerState>>>) -> String {
    let state = shared_state.lock().await;

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
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
) -> String {
    let state_lock = shared_state.lock().await;
    format!("{:?}", state_lock.consensus_state.commitments)
}

pub async fn get_block(
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
    Path(height): Path<u32>,
) -> String {
    let state_lock = shared_state.lock().await;
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
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
) -> String {
    let state_lock = shared_state.lock().await;
    match serde_json::to_string(&state_lock.merkle_trie_root) {
        Ok(trie_root_json) => trie_root_json,
        Err(e) => e.to_string(),
    }
}
