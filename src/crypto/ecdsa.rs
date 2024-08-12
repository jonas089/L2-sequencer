use k256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    sha2::digest::Key,
};
use rand_core::OsRng;

pub struct Keypair {
    sk: SigningKey,
    vk: VerifyingKey,
}
impl Keypair {
    pub fn new() -> Self {
        let sk = SigningKey::random(&mut OsRng);
        let vk = VerifyingKey::from(&sk);
        Keypair { sk, vk }
    }
    pub fn sign_data(&self, data: &[u8]) -> Signature {
        self.sk.sign(data)
    }
    pub fn serialize_sk(&self) -> Vec<u8> {
        self.sk.to_bytes().to_vec()
    }
    pub fn serialize_vk(&self) -> Vec<u8> {
        self.vk.to_sec1_bytes().to_vec()
    }
}

pub fn deserialize_sk(sk_serialized: &[u8]) -> SigningKey {
    SigningKey::from_bytes(sk_serialized.into()).unwrap()
}

pub fn deserialize_vk(vk_serialized: &[u8]) -> VerifyingKey {
    VerifyingKey::from_sec1_bytes(vk_serialized).unwrap()
}

#[test]
fn test_serialize_and_deserialize_sk() {
    let keypair = Keypair::new();
    let sk_before = keypair.sk.clone();
    let sk_serialized = keypair.serialize_sk();
    let sk_deserialized = deserialize_sk(&sk_serialized);
    assert_eq!(sk_before, sk_deserialized);
}
#[test]
fn test_serialize_and_deserialize_vk() {
    let keypair = Keypair::new();
    let vk_before = keypair.vk.clone();
    let vk_serialized = keypair.serialize_vk();
    let vk_deserialized = deserialize_vk(&vk_serialized);
    assert_eq!(vk_before, vk_deserialized);
}
#[test]
fn test_generate_and_verify_ecdsa_signature_using_secp256k1_curve() {
    let keypair = Keypair::new();
    let arbitrary_data: Vec<u8> = vec![0; 32];
    let signature = keypair.sign_data(&arbitrary_data);
    keypair
        .vk
        .verify(&arbitrary_data, &signature)
        .expect("Failed to verify signature");
}
