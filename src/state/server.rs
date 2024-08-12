use crate::types::Proposal;
use std::collections::HashMap;
type Block = Proposal;

pub struct MemoryDB {
    pub blocks: HashMap<u32, Block>,
}

impl MemoryDB {
    pub fn insert_block(&mut self, height: u32, block: Block) {
        self.blocks.insert(height, block);
    }
    pub fn get_block_by_height(&self, height: u32) -> &Block {
        self.blocks.get(&height).expect("Failed to get Block")
    }
}
