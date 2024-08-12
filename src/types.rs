use serde::{Deserialize, Serialize};

pub type GenericSignature = Vec<u8>;
pub type Timestamp = u32;
pub type GenericTransactionData = Vec<u8>;
pub type GenericPublicKey = Vec<u8>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Proposal {
    pub height: u32,
    pub transactions: Vec<Transaction>,
    pub commitments: Vec<Commitment>,
    pub timestamp: Timestamp,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transaction {
    pub data: GenericTransactionData,
    pub timestamp: Timestamp,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Commitment {
    // a signature over the serialized
    // transactions in the Block
    pub signature: GenericSignature,
    pub validator: GenericPublicKey,
    pub timestamp: Timestamp,
}
