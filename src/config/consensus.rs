use crate::crypto::ecdsa::{deserialize_sk, deserialize_vk};
use k256::ecdsa::{SigningKey, VerifyingKey};

// time before a new block is created, from a block's timestamp onwards
// specified in seconds
pub const accumulation_phase_duration: u32 = 600; // 10 minutes
pub const consensus_threshold: u32 = 2; // for the first iteration of development,
                                        // 2 nodes will be run and both must commit to the Consensus round
pub const validator_count: u32 = 2;

pub const test_v1_sk: &'static [u8] = &[
    197, 131, 252, 199, 111, 171, 195, 194, 6, 111, 156, 165, 24, 173, 168, 49, 220, 204, 234, 73,
    99, 125, 215, 189, 192, 254, 218, 47, 55, 40, 214, 117,
];

pub const test_v1_vk: &'static [u8] = &[
    2, 145, 6, 132, 63, 12, 220, 31, 107, 229, 80, 59, 38, 153, 140, 235, 182, 43, 206, 83, 189, 7,
    223, 91, 52, 126, 122, 10, 55, 62, 238, 7, 219,
];

pub const test_v2_sk: &'static [u8] = &[
    31, 133, 86, 165, 209, 28, 9, 200, 44, 211, 32, 243, 68, 35, 181, 101, 112, 158, 112, 89, 132,
    37, 223, 101, 46, 64, 204, 23, 247, 13, 207, 129,
];

pub const test_v2_vk: &'static [u8] = &[
    2, 117, 224, 184, 15, 207, 177, 48, 93, 85, 52, 230, 31, 123, 67, 27, 47, 58, 245, 197, 215,
    189, 128, 192, 14, 203, 86, 139, 85, 234, 68, 42, 148,
];

pub fn v1_sk_deserialized() -> SigningKey {
    deserialize_sk(&test_v1_sk)
}

pub fn v2_sk_deserialized() -> SigningKey {
    deserialize_sk(&test_v2_sk)
}

pub fn v1_vk_deserialized() -> VerifyingKey {
    deserialize_vk(&test_v1_vk)
}

pub fn v2_vk_deserialized() -> VerifyingKey {
    deserialize_vk(&test_v2_vk)
}
