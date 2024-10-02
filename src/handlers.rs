use crate::config::consensus::CONSENSUS_THRESHOLD;
#[cfg(not(feature = "sqlite"))]
use crate::state::server::InMemoryBlockStore;
use crate::types::BlockCommitment;
use crate::types::GenericSignature;
use crate::{crypto::ecdsa::deserialize_vk, types::Block};
use crate::{get_current_time, ServerState};
use colored::Colorize;
use k256::ecdsa::signature::{SignerMut, Verifier};
use k256::ecdsa::Signature;
use patricia_trie::{
    check_leaf, insert_leaf,
    store::types::{Hashable, Leaf, Node},
};
use reqwest::Response;

#[cfg(feature = "sqlite")]
use crate::state::server::{BlockStore, SqLiteBlockStore, SqLiteTransactionPool};
#[cfg(feature = "sqlite")]
use patricia_trie::store::db::{sql, Database};

pub async fn handle_synchronization_response(
    state_lock: &mut tokio::sync::RwLockWriteGuard<'_, ServerState>,
    response: Response,
    next_height: u32,
) {
    println!("[Info] Querying Block: {}", &next_height);
    let block_serialized = response.text().await.unwrap();
    if block_serialized != "[Warning] Requested Block that does not exist" {
        let mut block: Block = serde_json::from_str(&block_serialized).unwrap();
        #[cfg(not(feature = "sqlite"))]
        state_lock
            .block_state
            .insert_block(next_height, block.clone());

        #[cfg(feature = "sqlite")]
        state_lock
            .block_state
            .insert_block(next_height, block.clone());

        // insert transactions into the trie
        let root_node = Node::Root(state_lock.merkle_trie_root.clone());
        let root_node = handle_transaction_batch(state_lock, &mut block, root_node);
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

pub async fn handle_block_proposal(
    state_lock: &mut tokio::sync::RwLockWriteGuard<'_, ServerState>,
    proposal: &mut Block,
    error_response: String,
) -> Option<String> {
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
        return Some(error_response);
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
                        format_args!("{} Invalid Commitment was Ignored", "[Warning]".yellow())
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
        let root_node = Node::Root(state_lock.merkle_trie_root.clone());
        let root_node = handle_transaction_batch(state_lock, proposal, root_node);
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
        // && !state_lock.consensus_state.signed
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
    } else {
        println!(
            "{}",
            format_args!(
                "{} Block is signed but lacks commitments",
                "[Warning]".yellow()
            )
        );
    }
    None
}

fn handle_transaction_batch(
    state_lock: &mut tokio::sync::RwLockWriteGuard<'_, ServerState>,
    block: &mut Block,
    mut root_node: Node,
) -> Node {
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
        leaf.hash();
        if !check_leaf(
            &mut state_lock.merkle_trie_state,
            leaf.clone(),
            root_node.clone(),
        ) {
            let new_root = insert_leaf(&mut state_lock.merkle_trie_state, &mut leaf, root_node);
            root_node = Node::Root(new_root);
        }
    }
    root_node
}
