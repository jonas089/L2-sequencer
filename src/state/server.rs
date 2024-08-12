use k256::ecdsa::VerifyingKey;

use crate::types::{
    Block, BlockCommitment, ConsensusCommitment, GenericTransactionData, Timestamp,
};
use std::collections::HashMap;

pub struct InMemoryBlockStore {
    pub height: u32,
    pub blocks: HashMap<u32, Block>,
}

impl InMemoryBlockStore {
    pub fn empty() -> Self {
        Self {
            height: 0,
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
        );
        self.height += 1;
    }
    pub fn insert_block(&mut self, height: u32, block: Block) {
        self.blocks.insert(height, block);
        self.height += 1;
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
    pub fn reset(&mut self) {
        self.size = 0;
        self.transactions = HashMap::new();
    }
}

pub struct InMemoryConsensus {
    pub validators: Vec<VerifyingKey>,
    pub commitments: Vec<ConsensusCommitment>,
}

impl InMemoryConsensus {
    pub fn empty() -> Self {
        Self {
            validators: Vec::new(),
            commitments: Vec::new(),
        }
    }
    pub fn empty_with_default_validators() -> InMemoryConsensus {
        use crate::config::consensus::{v1_vk_deserialized, v2_vk_deserialized};
        Self {
            validators: vec![v1_vk_deserialized(), v2_vk_deserialized()],
            commitments: Vec::new(),
        }
    }
    pub fn insert_commitment(&mut self, commitment: ConsensusCommitment) {
        self.commitments.push(commitment);
    }
    pub fn reset(&mut self) {
        self.commitments = Vec::new();
    }
}
