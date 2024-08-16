use std::{env, time::Duration};

use crate::types::Block;
use colored::Colorize;
use reqwest::{Client, Response};
use tokio::time::sleep;

use crate::types::ConsensusCommitment;

// gossip commitments to other nodes
pub type PEER = &'static str;

async fn send_proposal(client: Client, peer: PEER, json_block: String) -> Response {
    let response: Response = client
        .post(format!("http://{}{}", &peer, "/propose"))
        .header("Content-Type", "application/json")
        .body(json_block)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .unwrap();
    response
}

pub struct Gossipper {
    pub peers: Vec<PEER>,
    pub client: Client,
}
impl Gossipper {
    pub async fn gossip_pending_block(&self, block: Block) {
        for peer in &self.peers {
            let client_clone = self.client.clone();
            let peer_clone = peer.clone();
            let json_block: String = serde_json::to_string(&block).unwrap();
            if peer == &env::var("API_HOST_WITH_PORT").unwrap_or("127.0.0.1:8080".to_string()) {
                continue;
            }
            println!(
                "{}",
                format!("{} Sending Block to Peer: {}", "[Info]".green(), &peer)
            );
            tokio::spawn(async move {
                loop {
                    let response =
                        send_proposal(client_clone.clone(), peer_clone, json_block.clone())
                            .await
                            .text()
                            .await
                            .unwrap_or("[Err] Peer unresponsive".to_string());
                    if response == "[Ok] Block was processed" {
                        println!(
                            "{}",
                            format!(
                                "{} Block was successfully sent to peer: {}",
                                "[Info]".green(),
                                &peer_clone
                            )
                        );
                        break;
                    } else if response == "[Err] Awaiting consensus evaluation" {
                        println!(
                            "{}",
                            format!(
                                "{} Failed to send Block to peer: {}, {}",
                                "[Warning]".yellow(),
                                &peer_clone,
                                "Consensus has not concluded"
                            )
                        );
                    } else if response == "[Err] Peer unresponsive" {
                        println!(
                            "{}",
                            format!(
                                "{} Failed to send Block to peer: {}, {}",
                                "[Warning]".yellow(),
                                &peer_clone,
                                "Peer unresponsive"
                            )
                        );
                    }
                    sleep(Duration::from_secs(3)).await;
                }
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
            tokio::spawn(async move {
                match client_clone
                    .post(format!("http://{}{}", &peer_clone, "/commit"))
                    .header("Content-Type", "application/json")
                    .body(json_commitment_clone)
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await
                {
                    Ok(_) => {}
                    Err(_) => println!(
                        "{}",
                        format!(
                            "{} Failed to send Consensus Commitment to peer: {}, {}",
                            "[Warning]".yellow(),
                            &peer_clone,
                            "Proceeding with other peers"
                        )
                    ),
                }
            });
        }
    }
}
