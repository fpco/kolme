use ed25519_dalek::SigningKey;
use kolme::SecretKey;
use libp2p::{identity::Keypair, PeerId};
use sha2::{Digest, Sha256};

pub fn secret_from_mnemonic(mnemonic: &str) -> SecretKey {
    // long hex string is boring, its better to use human-readable one!
    let mut hasher = Sha256::new();
    hasher.update(mnemonic);
    let hashed = hex::encode(hasher.finalize());
    SecretKey::from_hex(&hashed).unwrap()
}

pub fn keypair_from_mnemonic(mnemonic: &str) -> Keypair {
    let mut hasher = Sha256::new();
    hasher.update(mnemonic);
    let hashed = hasher.finalize();
    let signing_key = SigningKey::from_bytes(&hashed[..32].try_into().expect("Invalid seed"));
    let secret_part = &mut signing_key.to_keypair_bytes()[..32];
    Keypair::ed25519_from_bytes(secret_part)
        .expect("Failed to create libp2p keypair from ed25519 signing key")
}

pub fn peer_id_from_mnemonic(mnemonic: &str) -> PeerId {
    PeerId::from(keypair_from_mnemonic(mnemonic).public())
}

pub fn application_secret() -> SecretKey {
    secret_from_mnemonic("version upgrade test app")
}

const PROCESSOR_SECRETS_MNEMONIC: &str = "version upgrade test processor";

pub fn processor_keypair() -> Keypair {
    keypair_from_mnemonic(PROCESSOR_SECRETS_MNEMONIC)
}

pub fn processor_peer_id() -> PeerId {
    peer_id_from_mnemonic(PROCESSOR_SECRETS_MNEMONIC)
}
