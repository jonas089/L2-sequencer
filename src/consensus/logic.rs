use crate::types::{ConsensusCommitment, GenericPublicKey};
use zk_logic::{random_bytes_to_int, types::CircuitOutputs};

/*
    PoRD was changed to, instead of evaluating the closest commitment to the mean
    timestamp, find the closest commitment to the mean commitments
*/
pub fn evaluate_commitments(commitments: Vec<ConsensusCommitment>) -> GenericPublicKey {
    // todo: verify the associated signatures
    let commitment_values = {
        let mut commitment_values: Vec<u32> = Vec::new();
        for commitment in &commitments {
            let commitment_value: CircuitOutputs = commitment.receipt.journal.decode().unwrap();
            commitment_values.push(
                random_bytes_to_int(&commitment_value.random_bytes)
                    .to_u32_digits()
                    .1
                    .last()
                    .unwrap()
                    .clone(),
            );
        }
        commitment_values
    };
    let mean_commitment = {
        let mut commitment_sum: u64 = 0;
        for timestamp in &commitment_values {
            commitment_sum += *timestamp as u64;
        }
        commitment_sum / commitment_values.len() as u64
    };
    choose_winner(mean_commitment, commitments)
}

fn choose_winner(mean_commitment: u64, commitments: Vec<ConsensusCommitment>) -> GenericPublicKey {
    let winner: Option<&ConsensusCommitment> = commitments.iter().min_by_key(|commitment| {
        (random_bytes_to_int(
            &commitment
                .receipt
                .journal
                .decode::<CircuitOutputs>()
                .unwrap()
                .random_bytes,
        )
        .to_u32_digits()
        .1
        .last()
        .unwrap()
        .clone() as i32
            - mean_commitment as i32)
            .abs()
    });
    winner.unwrap().validator.clone()
}
