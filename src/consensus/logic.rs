use crate::types::{ConsensusCommitment, GenericPublicKey};
use num_bigint::BigInt;
use num_traits::Signed;
use zk_logic::{random_bytes_to_int, types::CircuitOutputs};

/*
    PoRD was changed to, instead of evaluating the closest commitment to the mean
    timestamp, find the closest commitment to the mean commitments
*/
pub fn evaluate_commitments(commitments: Vec<ConsensusCommitment>) -> GenericPublicKey {
    // todo: verify the associated signatures
    let commitment_values = {
        let mut commitment_values: Vec<BigInt> = Vec::new();
        for commitment in &commitments {
            let commitment_value: CircuitOutputs = commitment.receipt.journal.decode().unwrap();
            commitment_values.push(random_bytes_to_int(&commitment_value.random_bytes));
        }
        commitment_values
    };
    let mean_commitment = {
        let mut commitment_sum: BigInt = BigInt::ZERO;
        for random_bytes in &commitment_values {
            commitment_sum += random_bytes;
        }
        commitment_sum / commitment_values.len()
    };
    choose_winner(mean_commitment, commitments)
}

fn choose_winner(
    mean_commitment: BigInt,
    commitments: Vec<ConsensusCommitment>,
) -> GenericPublicKey {
    let winner: Option<&ConsensusCommitment> = commitments.iter().min_by_key(|commitment| {
        let value = random_bytes_to_int(
            &commitment
                .receipt
                .journal
                .decode::<CircuitOutputs>()
                .unwrap()
                .random_bytes,
        );
        let distance = (value - mean_commitment.clone()).abs();
        distance
    });

    winner.unwrap().validator.clone()
}
