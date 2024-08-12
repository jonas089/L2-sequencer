use crate::types::{Commitment, GenericTransactionData, Proposal, Timestamp};
use std::collections::HashMap;
type Block = Proposal;

pub struct InMemoryBlockStore {
    pub blocks: HashMap<u32, Block>,
}

impl InMemoryBlockStore {
    pub fn empty() -> Self {
        Self {
            blocks: HashMap::new(),
        }
    }
    pub fn trigger_genesis(&mut self, timestamp: Timestamp) {
        self.insert_block(
            0u32,
            Block {
                timestamp,
                height: 0,
                transactions: vec![],
                commitments: vec![],
            },
        )
    }
    pub fn insert_block(&mut self, height: u32, block: Block) {
        self.blocks.insert(height, block);
    }
    pub fn get_block_by_height(&self, height: u32) -> &Block {
        self.blocks.get(&height).expect("Failed to get Block")
    }
}

pub struct InMemoryTransactionPool {
    pub size: u32,
    pub transactions: HashMap<u32, GenericTransactionData>,
}

impl InMemoryTransactionPool {
    pub fn empty() -> Self {
        Self {
            size: 0,
            transactions: HashMap::new(),
        }
    }
    pub fn insert_transaction(&mut self, transaction: GenericTransactionData) {
        self.transactions.insert(self.size, transaction);
        self.size += 1;
    }
    pub fn get_transaction_by_index(&self, index: u32) -> &GenericTransactionData {
        self.transactions
            .get(&index)
            .expect("Failed to get Transaction")
    }
}

pub struct InMemoryConsensus {
    pub commitments: Vec<Commitment>,
}

impl InMemoryConsensus {
    pub fn empty() -> Self {
        Self {
            commitments: Vec::new(),
        }
    }
}
