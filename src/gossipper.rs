use std::env;

use crate::types::Block;
use reqwest::Client;

use crate::types::ConsensusCommitment;

// gossip commitments to other nodes
pub type PEER = &'static str;
pub struct Gossipper {
    pub peers: Vec<PEER>,
    pub client: Client,
}
impl Gossipper {
    pub async fn gossip_pending_block(&self, block: Block) -> Vec<String> {
        let mut responses: Vec<String> = Vec::new();
        let json_block: String = serde_json::to_string(&block).unwrap();
        for peer in &self.peers {
            if peer == &env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                continue;
            }
            let response = self
                .client
                .post(format!("http://{}{}", &peer, "/propose"))
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
            if peer == &env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                continue;
            }
            let response = self
                .client
                .post(format!("http://{}{}", &peer, "/commit"))
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
