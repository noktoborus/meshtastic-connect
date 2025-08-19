use crate::keyring::key::Key;
use crate::keyring::node_id::NodeId;
use aes::cipher::StreamCipherError;
use aes::{Aes128, Aes256};
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use zerocopy::IntoBytes;

use super::{Decrypt, Encrypt};

// Data to decrypt using symmetric AES
pub struct Symmetric {
    // Part of nonce
    pub from: NodeId,

    // Key of channel
    pub key: Key,
}

fn prepare_nonce(packet_id: u32, from: NodeId) -> [u8; 16] {
    let mut nonce = [0u8; 16];

    nonce[..4].copy_from_slice(&packet_id.to_le_bytes());
    nonce[8..12].copy_from_slice(&from.to_bytes());

    nonce
}

fn crypt(
    key: &Key,
    packet_id: u32,
    from: NodeId,
    mut buffer: Vec<u8>,
) -> Result<Vec<u8>, StreamCipherError> {
    let nonce = prepare_nonce(packet_id, from);

    match key {
        Key::K128(key) => Ctr128BE::<Aes128>::new(key.as_bytes().into(), &nonce.into())
            .try_apply_keystream(buffer.as_mut_bytes()),
        Key::K256(key) => Ctr128BE::<Aes256>::new(key.as_bytes().into(), &nonce.into())
            .try_apply_keystream(buffer.as_mut_bytes()),
    }?;
    Ok(buffer)
}

impl Decrypt for Symmetric {
    async fn decrypt(&self, packet_id: u32, buffer: Vec<u8>) -> Result<Vec<u8>, String> {
        crypt(&self.key, packet_id, self.from, buffer)
            .map_err(|e| format!("Unable to decrypt: {:?}", e))
    }
}

impl Encrypt for Symmetric {
    async fn encrypt(&self, packet_id: u32, buffer: Vec<u8>) -> Result<Vec<u8>, String> {
        crypt(&self.key, packet_id, self.from, buffer)
            .map_err(|e| format!("Unable to encrypt: {:?}", e))
    }
}
