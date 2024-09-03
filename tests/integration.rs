#[cfg(test)]
mod tests {
    use patricia_trie::{
        merkle::{verify_merkle_proof, MerkleProof},
        store::types::{Hashable, Leaf, Root},
    };
    use pord_sequencer::types::Transaction;
    use prover::generate_random_number;
    use reqwest::Client;
    use std::env;
    use {
        pord_sequencer::config::network::PEERS, pord_sequencer::gossipper::Gossipper,
        pord_sequencer::types::ConsensusCommitment,
    };

    #[tokio::test]
    async fn test_schedule_transaction() {
        let client = Client::new();
        let transaction: Transaction = Transaction {
            data: vec![1, 2, 3, 4, 5],
            timestamp: 0,
        };
        let transaction_json: String = serde_json::to_string(&transaction).unwrap();
        // note that currently a transaction may only be submitted to one node
        // mishandling this can cause the network to crash
        let response = client
            .post("http://127.0.0.1:8080/schedule")
            .header("Content-Type", "application/json")
            .body(transaction_json.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.text().await.unwrap(),
            "[Ok] Transaction is being sequenced: Transaction { data: [1, 2, 3, 4, 5], timestamp: 0 }"
        );
    }

    #[tokio::test]
    async fn test_request_merkle_proof() {
        let client = Client::new();
        let transaction: Transaction = Transaction {
            data: vec![1, 2, 3, 4, 5],
            timestamp: 0,
        };
        let mut leaf = Leaf::new(Vec::new(), Some(transaction.data.clone()));
        leaf.hash();
        leaf.key = leaf
            .hash
            .clone()
            .unwrap()
            .iter()
            .flat_map(|&byte| (0..8).rev().map(move |i| (byte >> i) & 1))
            .collect();
        leaf.hash();
        let transaction_key_json = serde_json::to_string(&leaf.key).unwrap();
        let merkle_proof_response = client
            .post("http://127.0.0.1:8080/merkle_proof")
            .header("Content-Type", "application/json")
            .body(transaction_key_json)
            .send()
            .await
            .unwrap();
        let merkle_proof_json = merkle_proof_response.text().await.unwrap();
        let merkle_proof: MerkleProof = serde_json::from_str(&merkle_proof_json).unwrap();
        let state_root_hash_response = client
            .get("http://127.0.0.1:8080/get/state_root_hash")
            .send()
            .await
            .unwrap();
        let state_root_hash: Root =
            serde_json::from_str(&state_root_hash_response.text().await.unwrap()).unwrap();
        let mut inner_proof = merkle_proof.nodes;
        inner_proof.reverse();
        println!("Inner Proof: {:?}", &inner_proof);
        verify_merkle_proof(inner_proof, state_root_hash.hash.unwrap());
    }

    #[tokio::test]
    async fn test_commit() {
        let receipt = generate_random_number(vec![0; 32], vec![0; 32]);
        let consensus_commitment: ConsensusCommitment = ConsensusCommitment {
            validator: vec![0; 32],
            receipt,
        };
        let gossipper = Gossipper {
            peers: PEERS.to_vec(),
            client: Client::new(),
        };
        env::set_var("API_HOST_WITH_PORT", "127.0.0.1:8081");
        gossipper
            .gossip_consensus_commitment(consensus_commitment)
            .await;
    }
}
