use crate::types::Block;
use crate::{state::server::InMemoryBlockStore, ServerState};
use colored::Colorize;
use patricia_trie::{
    insert_leaf,
    store::types::{Hashable, Leaf, Node},
};
use reqwest::Response;

pub async fn handle_synchronization_response(
    state_lock: &mut tokio::sync::MutexGuard<'_, ServerState>,
    response: Response,
    next_height: u32,
) {
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
            let new_root = insert_leaf(&mut state_lock.merkle_trie_state, &mut leaf, root_node);
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
