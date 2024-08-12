use crate::types::{Commitment, GenericPublicKey};

pub fn evaluate_commitments(commitments: Vec<Commitment>) -> GenericPublicKey {
    let mut commitment_timestamps = {
        let mut commitment_timestamps: Vec<u32> = Vec::new();
        for commitment in &commitments {
            commitment_timestamps.push(commitment.timestamp);
        }
        commitment_timestamps
    };
    let mean_commitment = {
        let mut commitment_sum = 0;
        for timestamp in &commitment_timestamps {
            commitment_sum += timestamp;
        }
        commitment_sum / commitment_timestamps.len() as u32
    };
    choose_winner(mean_commitment, commitments)
}

fn choose_winner(mean_commitment: u32, commitments: Vec<Commitment>) -> GenericPublicKey {
    let winner = commitments
        .iter()
        .min_by_key(|commitment| (commitment.timestamp as i32 - mean_commitment as i32).abs());
    winner.unwrap().validator.clone()
}
