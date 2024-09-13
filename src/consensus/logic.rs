use crate::types::{ConsensusCommitment, GenericPublicKey};
use k256::ecdsa::VerifyingKey;
use l2_sequencer::{
    config::consensus::{ACCUMULATION_PHASE_DURATION, COMMITMENT_PHASE_DURATION, ROUND_DURATION},
    get_current_time,
};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};
use zk_logic::{random_bytes_to_int, types::CircuitOutputs};

pub fn evaluate_commitments(commitments: Vec<ConsensusCommitment>) -> GenericPublicKey {
    // todo: verify the associated signatures
    let commitment_values = {
        let mut commitment_values: Vec<BigInt> = Vec::new();
        for commitment in &commitments {
            let commitment_value: CircuitOutputs = commitment.receipt.journal.decode().unwrap();
            commitment_values.push(random_bytes_to_int(&commitment_value.random_bytes));
        }
        println!("Commitment Values: {:?}", &commitment_values);
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

#[allow(clippy::comparison_chain)]
fn choose_winner(
    mean_commitment: BigInt,
    commitments: Vec<ConsensusCommitment>,
) -> GenericPublicKey {
    let mut winner: Option<GenericPublicKey> = None;
    let mut lowest_distance: Option<BigInt> = None;
    if commitments.len() > 2 {
        for (index, commitment) in commitments.into_iter().enumerate() {
            let value = random_bytes_to_int(
                &commitment
                    .receipt
                    .journal
                    .decode::<CircuitOutputs>()
                    .unwrap()
                    .random_bytes,
            );
            if index == 0 {
                lowest_distance = Some((value - mean_commitment.clone()).abs());
                winner = Some(commitment.validator.clone())
            } else {
                let distance = Some((value - mean_commitment.clone()).abs());
                if distance < lowest_distance {
                    lowest_distance = distance;
                    winner = Some(commitment.validator.clone());
                }
            }
        }
    }
    // edge case for just 2 values (absolute value is the same so take the lowest) - only for test setup!
    else if commitments.len() == 2 {
        for (index, commitment) in commitments.into_iter().enumerate() {
            let value = random_bytes_to_int(
                &commitment
                    .receipt
                    .journal
                    .decode::<CircuitOutputs>()
                    .unwrap()
                    .random_bytes,
            );
            if index == 0 {
                lowest_distance = Some(value);
                winner = Some(commitment.validator.clone())
            } else {
                // choose the lowest of the two by value e.g. (x, -x) => -x
                if value < lowest_distance.clone().unwrap() {
                    lowest_distance = Some(value);
                    winner = Some(commitment.validator.clone());
                }
            }
        }
    }
    winner.unwrap()
}

pub fn get_committing_validator(
    last_block_unix_timestamp: u32,
    validators: Vec<VerifyingKey>,
) -> VerifyingKey {
    let round = (get_current_time() - last_block_unix_timestamp)
        / (COMMITMENT_PHASE_DURATION + ACCUMULATION_PHASE_DURATION + ROUND_DURATION);
    // returns the current validator
    validators[round as usize % (validators.len() - 1) as usize]
}

pub fn choose_winner_v2(random_commitment: BigInt, validators: Vec<VerifyingKey>) -> VerifyingKey {
    let index = (random_commitment % (validators.len() - 1))
        .to_u32()
        .unwrap();
    validators[index as usize]
}
