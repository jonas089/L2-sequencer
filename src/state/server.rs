use k256::ecdsa::{SigningKey, VerifyingKey};
#[cfg(feature = "sqlite")]
use rusqlite::{params, Connection};

use crate::{
    config::consensus::{v1_sk_deserialized, v1_vk_deserialized, v2_sk_deserialized},
    types::{Block, ConsensusCommitment, Timestamp, Transaction},
};
use std::{collections::HashMap, env};

pub trait InMemoryBlockStore {
    fn empty() -> Self;
    fn trigger_genesis(&mut self, timestamp: Timestamp);
    fn insert_block(&mut self, previous_height: u32, block: Block);
    fn get_block_by_height(&self, height: u32) -> Block;
}

#[cfg(feature = "sqlite")]
pub trait SqLiteBlockStore {
    fn setup(&self);
    fn trigger_genesis(&mut self, timestamp: Timestamp);
    fn insert_block(&mut self, previous_height: u32, block: Block);
    fn get_block_by_height(&self, height: u32) -> Block;
}

#[cfg(feature = "sqlite")]
pub struct BlockStore {
    pub height: u32,
    pub db_path: String,
}

#[cfg(not(feature = "sqlite"))]
pub struct BlockStore {
    pub height: u32,
    pub blocks: HashMap<u32, Block>,
}

#[cfg(feature = "sqlite")]
impl SqLiteBlockStore for BlockStore {
    fn setup(&self) {
        let conn = Connection::open(&self.db_path).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (
            height BLOB PRIMARY KEY,
            block BLOB NOT NULL
            )",
            [],
        )
        .unwrap();
    }
    fn get_block_by_height(&self, height: u32) -> Block {
        let conn = Connection::open(&self.db_path).unwrap();
        let mut stmt = conn
            .prepare("SELECT block FROM blocks WHERE height = ?1 LIMIT 1")
            .unwrap();

        let block_serialized: Option<Vec<u8>> = stmt
            .query_row([&height], |row| {
                let block_serialized: Vec<u8> = row.get(0).unwrap();
                Ok(Some(block_serialized))
            })
            .unwrap_or(None);

        bincode::deserialize(
            &block_serialized.expect(&format!("[Error] Block not found: {}", &height)),
        )
        .unwrap()
    }
    fn insert_block(&mut self, previous_height: u32, block: Block) {
        let conn = Connection::open(&self.db_path).unwrap();
        let mut height = previous_height + 1;
        // todo: read height from db
        self.height += 1;
        if block.height == 0 {
            self.height -= 1;
            height = 0;
        }
        conn.execute(
            "INSERT OR REPLACE INTO blocks (height, block) VALUES (?1, ?2)",
            params![height, bincode::serialize(&block).unwrap()],
        )
        .unwrap();
    }
    fn trigger_genesis(&mut self, timestamp: Timestamp) {
        self.insert_block(
            0u32,
            Block {
                timestamp,
                height: 0,
                signature: Some(vec![]),
                transactions: vec![],
                commitments: None,
            },
        )
    }
}

#[cfg(not(feature = "sqlite"))]
impl InMemoryBlockStore for BlockStore {
    fn empty() -> Self {
        Self {
            height: 1,
            blocks: HashMap::new(),
        }
    }
    fn trigger_genesis(&mut self, timestamp: Timestamp) {
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
    fn insert_block(&mut self, previous_height: u32, block: Block) {
        self.blocks.insert(previous_height + 1, block);
        self.height += 1;
    }
    fn get_block_by_height(&self, height: u32) -> Block {
        self.blocks
            .get(&height)
            .expect("Failed to get Block")
            .clone()
    }
}

#[cfg(not(feature = "sqlite"))]
pub trait InMemoryTransactionPool {
    fn empty() -> Self;
    fn insert_transaction(&mut self, transaction: Transaction);
    #[allow(unused)]
    fn get_transaction_by_index(&self, index: u32) -> &Transaction;
    fn reinitialize(&mut self);
}

#[cfg(feature = "sqlite")]
pub trait SqLiteTransactionPool {
    fn setup(&self);
    fn insert_transaction(&mut self, transaction: Transaction);
    fn get_transaction_by_index(&self, index: u32) -> Transaction;
    fn get_all_transactions(&self) -> Vec<Transaction>;
    fn reinitialize(&mut self);
}

#[cfg(not(feature = "sqlite"))]
pub struct TransactionPool {
    pub size: u32,
    pub transactions: HashMap<u32, Transaction>,
}

#[cfg(feature = "sqlite")]
pub struct TransactionPool {
    pub size: u32,
    pub db_path: String,
}
#[cfg(feature = "sqlite")]
impl SqLiteTransactionPool for TransactionPool {
    fn setup(&self) {
        let conn = Connection::open(&self.db_path).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS txns (
                uid BLOB PRIMARY KEY,
                tx BLOB NOT NULL
            )",
            [],
        )
        .unwrap();
    }
    fn get_transaction_by_index(&self, index: u32) -> Transaction {
        let conn = Connection::open(&self.db_path).unwrap();
        let mut stmt = conn
            .prepare("SELECT tx FROM txns WHERE uid = ?1 LIMIT 1")
            .unwrap();

        let transaction_serialized: Option<Vec<u8>> = stmt
            .query_row([&index], |row| {
                let node_serialized: Vec<u8> = row.get(0).unwrap();
                Ok(Some(node_serialized))
            })
            .unwrap_or(None);

        bincode::deserialize(&transaction_serialized.expect("[Error] Block not found")).unwrap()
    }
    fn get_all_transactions(&self) -> Vec<Transaction> {
        let conn = Connection::open(&self.db_path).unwrap();
        let mut stmt = conn.prepare("SELECT tx FROM txns").unwrap();
        let transaction_iter = stmt
            .query_map([], |row| {
                let transaction_blob: Vec<u8> = row.get(0)?;
                // Deserialize the BLOB back into a Transaction
                let transaction: Transaction = bincode::deserialize(&transaction_blob).unwrap();
                Ok(transaction)
            })
            .unwrap();

        let mut transactions = Vec::new();
        for transaction in transaction_iter {
            transactions.push(transaction.unwrap());
        }

        transactions
    }
    fn insert_transaction(&mut self, transaction: Transaction) {
        let conn = Connection::open(&self.db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO txns (tx) VALUES (?1)",
            params![bincode::serialize(&transaction).unwrap()],
        )
        .unwrap();
        // todo: read size from db
        self.size += 1;
    }
    fn reinitialize(&mut self) {
        // todo: remove when reading size from db
        self.size = 0;
        let conn = Connection::open(&self.db_path).unwrap();
        conn.execute("DROP TABLE IF EXISTS txns", []).unwrap();
        self.setup();
    }
}

#[cfg(not(feature = "sqlite"))]
impl InMemoryTransactionPool for TransactionPool {
    fn empty() -> Self {
        Self {
            size: 0,
            transactions: HashMap::new(),
        }
    }
    fn insert_transaction(&mut self, transaction: Transaction) {
        self.transactions.insert(self.size, transaction);
        self.size += 1;
    }
    fn get_transaction_by_index(&self, index: u32) -> &Transaction {
        self.transactions
            .get(&index)
            .expect("Failed to get Transaction")
    }
    fn reinitialize(&mut self) {
        self.size = 0;
        self.transactions = HashMap::new();
    }
}

pub struct InMemoryConsensus {
    pub validators: Vec<VerifyingKey>,
    pub local_validator: VerifyingKey,
    pub local_signing_key: SigningKey,
    pub commitments: Vec<Vec<ConsensusCommitment>>,
    pub current_round: u32,
    pub round_winners: Vec<VerifyingKey>,
    pub proposed: [bool; 10],
    pub committed: [bool; 10],
}

impl InMemoryConsensus {
    #[allow(unused)]
    pub fn empty() -> Self {
        Self {
            validators: Vec::new(),
            local_validator: v1_vk_deserialized(),
            local_signing_key: v2_sk_deserialized(),
            commitments: Vec::new(),
            current_round: 0,
            round_winners: Vec::new(),
            proposed: [false; 10],
            committed: [false; 10],
        }
    }
    pub fn empty_with_default_validators() -> InMemoryConsensus {
        use crate::config::consensus::v2_vk_deserialized;
        let local_validator_test_id = env::var("LOCAL_VALIDATOR").unwrap_or(0.to_string());

        let local_validator = if local_validator_test_id == "0" {
            (v1_sk_deserialized(), v1_vk_deserialized())
        } else {
            (v2_sk_deserialized(), v2_vk_deserialized())
        };

        Self {
            validators: vec![v1_vk_deserialized(), v2_vk_deserialized()],
            local_validator: local_validator.1,
            local_signing_key: local_validator.0,
            commitments: Vec::new(),
            current_round: 0,
            round_winners: Vec::new(),
            proposed: [false; 10],
            committed: [false; 10],
        }
    }
    pub fn insert_commitment(&mut self, commitment: ConsensusCommitment, round: u32) {
        // this is an inconvenient check, Receipt does not implement ParitalEq
        // come up with a better solution in the future
        // luckily the list of consensus commitments will be relatively small in memory
        let commitments = self.commitments.get_mut(round as usize);
        // iterate over all of them to see if the validator exists
        match commitments {
            Some(v) => {
                for c in v {
                    if c.validator == commitment.validator {
                        return;
                    }
                }
            }
            None => {}
        }

        match self.commitments.get_mut(round as usize) {
            Some(v) => {
                v.push(commitment);
            }
            None => {
                self.commitments.insert(round as usize, vec![commitment]);
            }
        }
    }
    pub fn reinitialize(&mut self) {
        self.commitments = Vec::new();
        self.round_winners = Vec::new();
        self.current_round = 0;
        self.proposed = [false; 10];
        self.committed = [false; 10];
    }
}
