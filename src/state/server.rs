use k256::ecdsa::{SigningKey, VerifyingKey};

use crate::{
    config::consensus::{v1_sk_deserialized, v1_vk_deserialized, v2_sk_deserialized},
    types::{
        Block, BlockCommitment, ConsensusCommitment, GenericTransactionData, Timestamp, Transaction,
    },
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
        self.blocks.insert(
            0u32,
            Block {
                timestamp,
                height: 0,
                signature: Some(vec![]),
                transactions: vec![],
                commitments: None,
            },
        );
    }
    pub fn insert_block(&mut self, previous_height: u32, block: Block) {
        self.blocks.insert(previous_height + 1, block);
        self.height += 1;
    }
    pub fn get_block_by_height(&self, height: u32) -> &Block {
        self.blocks.get(&height).expect("Failed to get Block")
    }
}

pub struct InMemoryTransactionPool {
    pub height: u32,
    pub size: u32,
    pub transactions: HashMap<u32, Transaction>,
}

impl InMemoryTransactionPool {
    pub fn empty(height: u32) -> Self {
        Self {
            height,
            size: 0,
            transactions: HashMap::new(),
        }
    }
    pub fn insert_transaction(&mut self, transaction: Transaction) {
        self.transactions.insert(self.size, transaction);
        self.size += 1;
    }
    pub fn get_transaction_by_index(&self, index: u32) -> &Transaction {
        self.transactions
            .get(&index)
            .expect("Failed to get Transaction")
    }
    pub fn reinitialize(&mut self, height: u32) {
        self.height = height;
        self.size = 0;
        self.transactions = HashMap::new();
    }
}

pub struct InMemoryConsensus {
    pub height: u32,
    pub validators: Vec<VerifyingKey>,
    pub local_validator: VerifyingKey,
    pub local_signing_key: SigningKey,
    pub commitments: Vec<ConsensusCommitment>,
    pub round_winner: Option<VerifyingKey>,
    pub proposed: bool,
    pub committed: bool,
}

impl InMemoryConsensus {
    pub fn empty(height: u32) -> Self {
        Self {
            height,
            validators: Vec::new(),
            local_validator: v1_vk_deserialized(),
            local_signing_key: v2_sk_deserialized(),
            commitments: Vec::new(),
            round_winner: None,
            proposed: false,
            committed: false,
        }
    }
    pub fn empty_with_default_validators(height: u32) -> InMemoryConsensus {
        use crate::config::consensus::{v1_vk_deserialized, v2_vk_deserialized};
        Self {
            height,
            validators: vec![v1_vk_deserialized(), v2_vk_deserialized()],
            local_validator: v1_vk_deserialized(),
            local_signing_key: v1_sk_deserialized(),
            commitments: Vec::new(),
            round_winner: None,
            proposed: false,
            committed: false,
        }
    }
    pub fn insert_commitment(&mut self, commitment: ConsensusCommitment) {
        self.commitments.push(commitment);
    }
    pub fn reinitialize(&mut self, height: u32) {
        self.height = height;
        self.commitments = Vec::new();
        self.round_winner = None;
    }
}
