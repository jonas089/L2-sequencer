use std::{env, time::Duration};

use crate::types::Block;
use colored::Colorize;
use reqwest::{Client, Response};
use tokio::time::sleep;

use crate::types::ConsensusCommitment;

// gossip commitments to other nodes
pub type Peer = &'static str;

async fn send_proposal(client: Client, peer: Peer, json_block: String) -> Response {
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
    pub peers: Vec<Peer>,
    pub client: Client,
}
impl Gossipper {
    pub async fn gossip_pending_block(&self, block: Block) {
        for peer in self.peers.clone() {
            let client_clone = self.client.clone();
            let peer_clone = peer;
            let json_block: String = serde_json::to_string(&block).unwrap();
            // todo: make this generic for n amount of nodes
            let this_node = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
            if this_node == "0.0.0.0:8080" && peer == "rust-node-1:8080" {
                continue;
            } else if this_node == "0.0.0.0:8081" && peer == "rust-node-2:8080" {
                continue;
            }
            println!(
                "{}",
                format_args!("{} Sending Block to Peer: {}", "[Info]".green(), &peer)
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
                            format_args!(
                                "{} Block was successfully sent to peer: {}",
                                "[Info]".green(),
                                &peer_clone
                            )
                        );
                        break;
                    } else if response == "[Err] Awaiting consensus evaluation" {
                        println!(
                            "{}",
                            format_args!(
                                "{} Failed to send Block to peer: {}, {}",
                                "[Warning]".yellow(),
                                &peer_clone,
                                "Consensus has not concluded"
                            )
                        );
                    } else if response == "[Err] Peer unresponsive" {
                        println!(
                            "{}",
                            format_args!(
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
        for peer in self.peers.clone() {
            let client_clone = self.client.clone();
            let peer_clone = peer;
            let json_commitment_clone: String = json_commitment.clone();

            // todo: make this generic for n amount of nodes
            let this_node = env::var("API_HOST_WITH_PORT").unwrap_or("0.0.0.0:8080".to_string());
            if this_node == "0.0.0.0:8080" && peer == "rust-node-1:8080" {
                continue;
            } else if this_node == "0.0.0.0:8081" && peer == "rust-node-2:8080" {
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
                        format_args!(
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
