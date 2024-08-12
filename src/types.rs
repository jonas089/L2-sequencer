pub type Signature = Vec<u8>;
pub type Timestamp = u32;
pub type TransactionData = Vec<u8>;
pub type PublicKey = Vec<u8>;
pub struct Proposal {
    pub height: u32,
    pub transactions: Vec<Transaction>,
    pub commitments: Vec<Commitment>,
    pub timestamp: Timestamp,
}

pub struct Transaction {
    pub data: TransactionData,
    pub timestamp: Timestamp,
}

pub struct Commitment {
    // a signature over the serialized
    // transactions in the Block
    signature: Signature,
    validator: PublicKey,
    timestamp: Timestamp,
}
