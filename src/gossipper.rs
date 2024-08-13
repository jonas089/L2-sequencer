use crate::types::Block;
use reqwest::{Client, Response};

use crate::{config::network::DEFAULT_RPC_PORT, types::ConsensusCommitment};

// gossip commitments to other nodes
pub type peer = &'static str;
pub struct Gossipper {
    pub peers: Vec<peer>,
    pub client: Client,
}
impl Gossipper {
    pub async fn gossip_pending_block(&self, block: Block) -> Vec<String> {
        let mut responses: Vec<String> = Vec::new();
        let json_block: String = serde_json::to_string(&block).unwrap();
        for peer in &self.peers {
            let response = self
                .client
                .post(format!("{}{}", &peer, "/propose"))
                .header("Content-Type", "application/json")
                .body(json_block.clone())
                .send()
                .await
                .unwrap();
            responses.push(response.text().await.unwrap());
        }
        responses
    }
    pub async fn gossip_consensus_commitment(
        &self,
        commitment: ConsensusCommitment,
    ) -> Vec<String> {
        let mut responses: Vec<String> = Vec::new();
        let json_commitment: String = serde_json::to_string(&commitment).unwrap();
        for peer in &self.peers {
            let response = self
                .client
                .post(format!("{}{}", &peer, "/commit"))
                .header("Content-Type", "application/json")
                .body(json_commitment.clone())
                .send()
                .await
                .unwrap();
            responses.push(response.text().await.unwrap());
        }
        responses
    }
}
