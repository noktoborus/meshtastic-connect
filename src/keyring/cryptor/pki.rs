use super::{Decrypt, Encrypt};
use crate::keyring::{key::K256, node_id::NodeId};
use aes::Aes256;
use ccm::{
    Ccm, KeyInit,
    aead::{self, Aead},
};
use rand::Rng;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

// Data to decrypt using `Curve25519`
#[derive(Debug)]
pub struct PKI {
    // Part of nonce
    from: NodeId,

    // Shared key to decrypt message
    shared_key: K256,
}

impl PKI {
    pub fn new(remote: NodeId, remote_pubkey: K256, local_privkey: K256) -> Self {
        let remote_public = PublicKey::from(*remote_pubkey.as_bytes());
        let local_secret = StaticSecret::from(*local_privkey.as_bytes());
        let shared_secret = local_secret.diffie_hellman(&remote_public);
        let digest: [u8; 32] = Sha256::digest(shared_secret.as_bytes()).into();

        Self {
            from: remote,
            shared_key: digest.into(),
        }
    }
}

fn prepare_nonce(packet_id: u32, from: NodeId, extra_nonce: &[u8; 4]) -> [u8; 16] {
    let mut nonce = [0u8; 16];

    nonce[..4].copy_from_slice(&packet_id.to_le_bytes());
    nonce[4..8].copy_from_slice(extra_nonce);
    nonce[8..12].copy_from_slice(&from.to_bytes());

    nonce
}

const AUTH_LEN: usize = 8;
const EXTRA_NONCE_LEN: usize = 4;

impl Decrypt for PKI {
    async fn decrypt(&self, packet_id: u32, buffer: Vec<u8>) -> Result<Vec<u8>, String> {
        if buffer.len() < AUTH_LEN + EXTRA_NONCE_LEN {
            return Err(format!(
                "PKI: {} bytes is not enough to decode",
                buffer.len()
            ));
        }

        let (ciphertext_with_auth, tail) = buffer.split_at(buffer.len() - EXTRA_NONCE_LEN);
        let nonce = prepare_nonce(packet_id, self.from, tail.try_into().unwrap());

        let cipher = Ccm::<Aes256, ccm::consts::U8, ccm::consts::U13>::new_from_slice(
            self.shared_key.as_bytes(),
        )
        .map_err(|e| format!("PKI cipher init failed: {}", e))?;

        cipher
            .decrypt(
                nonce[0..13].into(),
                aead::Payload {
                    msg: ciphertext_with_auth,
                    aad: &[],
                },
            )
            .map_err(|e| format!("PKI decrypt failed: {}", e))
    }
}

fn generate_extra_nonce() -> [u8; EXTRA_NONCE_LEN] {
    rand::rng().random()
}

impl Encrypt for PKI {
    async fn encrypt(&self, packet_id: u32, buffer: Vec<u8>) -> Result<Vec<u8>, String> {
        let extra_nonce = generate_extra_nonce();
        let nonce = prepare_nonce(packet_id, self.from, &extra_nonce);

        let cipher = Ccm::<Aes256, ccm::consts::U8, ccm::consts::U13>::new_from_slice(
            self.shared_key.as_bytes(),
        )
        .map_err(|e| format!("PKI cipher init failed: {}", e))?;

        let mut ciphertext_with_auth = cipher
            .encrypt(
                nonce[0..13].into(),
                aead::Payload {
                    msg: &buffer,
                    aad: &[],
                },
            )
            .map_err(|e| format!("PKI encrypt failed: {}", e))?;

        ciphertext_with_auth.extend_from_slice(&extra_nonce);
        Ok(ciphertext_with_auth)
    }
}
