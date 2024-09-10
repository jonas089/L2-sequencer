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
use patricia_trie::{
    insert_leaf,
    store::types::{Hashable, Leaf, Node},
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    config::consensus::{COMMITMENT_PHASE_DURATION, CONSENSUS_THRESHOLD},
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
    let mut state = shared_state.lock().await;
    let success_response = format!("[Ok] Commitment was accepted: {:?}", &commitment).to_string();
    state.consensus_state.insert_commitment(commitment);
    success_response
}

pub async fn propose(
    Extension(shared_state): Extension<Arc<Mutex<ServerState>>>,
    Json(mut proposal): Json<Block>,
) -> String {
    println!(
        "{}",
        format_args!("{} Proposal was Proposal was received", "[Info]".green())
    );
    let mut state_lock: tokio::sync::MutexGuard<ServerState> = shared_state.lock().await;
    let last_block_unix_timestamp = state_lock
        .block_state
        .get_block_by_height(state_lock.block_state.height)
        .timestamp;
    let error_response = format!("Block was rejected: {:?}", &proposal).to_string();

    if proposal.timestamp < last_block_unix_timestamp + COMMITMENT_PHASE_DURATION {
        return error_response;
    };
    if state_lock.block_state.height >= proposal.height {
        return error_response;
    }

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
                                    format_args!(
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
                    println!(
                        "{}",
                        format_args!("{} Received Valid Block", "[Info]".green())
                    );
                    let previous_block_height = state_lock.block_state.height;
                    // todo: verify Block height
                    state_lock
                        .block_state
                        .insert_block(previous_block_height, proposal.clone());
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
                    println!("{}", format_args!("{} Block was stored", "[Info]".green()));
                    println!(
                        "{}",
                        format_args!(
                            "{} New Trie Root: {:?}",
                            "[Info]".green(),
                            state_lock.merkle_trie_root.hash
                        )
                    );
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
                        .gossip_pending_block(proposal, state_lock.block_state.height)
                        .await;
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
        "[Err] Awaiting consensus evaluation".to_string()
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
        format_args!("{} Trying to get Block #{}", "[Info]".green(), height)
    );
    if state_lock.block_state.height < height {
        "[Warning] Requested Block that does not exist".to_string()
    } else {
        match serde_json::to_string(&state_lock.block_state.get_block_by_height(height)) {
            Ok(height_json) => height_json,
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
