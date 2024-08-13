use std::{env, time::Duration};

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
    pub async fn gossip_pending_block(&self, block: Block) {
        let mut responses: Vec<String> = Vec::new();
        for peer in &self.peers {
            let client_clone = self.client.clone();
            let peer_clone = peer.clone();
            let json_block: String = serde_json::to_string(&block).unwrap();
            if peer == &env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                continue;
            }
            tokio::spawn(async move {
                let _ = client_clone
                    .post(format!("http://{}{}", &peer_clone, "/propose"))
                    .header("Content-Type", "application/json")
                    .body(json_block.clone())
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await
                    .unwrap();
            });
        }
    }
    pub async fn gossip_consensus_commitment(&self, commitment: ConsensusCommitment) {
        let json_commitment: String = serde_json::to_string(&commitment).unwrap();
        for peer in &self.peers {
            let client_clone = self.client.clone();
            let peer_clone = peer.clone();
            let json_commitment_clone: String = json_commitment.clone();
            if peer == &env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                continue;
            }
            println!(
                "[Info] Sending Commitment to: {:?}",
                format!("http://{}{}", &peer, "/commit")
            );
            tokio::spawn(async move {
                let _ = client_clone
                    .post(format!("http://{}{}", &peer_clone, "/commit"))
                    .header("Content-Type", "application/json")
                    .body(json_commitment_clone)
                    .send()
                    .await
                    .unwrap();
            });
        }
    }
}
