use std::{env, time::Duration};

use crate::{
    config::consensus::{ACCUMULATION_PHASE_DURATION, COMMITMENT_PHASE_DURATION, ROUND_DURATION},
    get_current_time,
    types::Block,
};
use colored::Colorize;
use reqwest::{Client, Response};
use tokio::time::sleep;

use crate::types::ConsensusCommitment;

pub type Peer = &'static str;

pub struct Gossipper {
    pub peers: Vec<Peer>,
    pub client: Client,
}

async fn send_proposal(client: Client, peer: Peer, json_block: String) -> Option<Response> {
    let response: Option<Response> = match client
        .post(format!("http://{}{}", &peer, "/propose"))
        .header("Content-Type", "application/json")
        .body(json_block)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    {
        Ok(r) => Some(r),
        Err(_) => None,
    };
    response
}

impl Gossipper {
    pub async fn gossip_pending_block(&self, block: Block, last_block_unix_timestamp: u32) {
        // try to collect attestations for the proposal and
        // store it eventually (if it reaches the threshold)
        // stop porposing before the new round begins
        // the new round will either introduce a new block,
        // or attempt to recreate this one
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
                let start_round = (get_current_time() - last_block_unix_timestamp)
                    / (COMMITMENT_PHASE_DURATION + ACCUMULATION_PHASE_DURATION + ROUND_DURATION)
                    + 1;
                loop {
                    let round = (get_current_time() - last_block_unix_timestamp)
                        / (COMMITMENT_PHASE_DURATION
                            + ACCUMULATION_PHASE_DURATION
                            + ROUND_DURATION)
                        + 1;
                    if start_round < round {
                        //println!("[Err] Refusing to gossip old Block");
                        break;
                    }
                    let response =
                        match send_proposal(client_clone.clone(), peer_clone, json_block.clone())
                            .await
                        {
                            Some(r) => r
                                .text()
                                .await
                                .unwrap_or("[Err] Peer unresponsive".to_string()),
                            None => "[Err] Failed to send request".to_string(),
                        };
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
                    Ok(_) => {
                        println!("[Info] Successfully sent Consensus Commitment to peer")
                    }
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
