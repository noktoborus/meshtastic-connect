use aes::{Aes128, Aes256};
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use prost::Message;

use crate::keyring::key::Key;
use crate::keyring::node_id::NodeId;
use crate::meshtastic;

use super::Decrypt;

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

fn apply_symmetric_decryption<C>(mut cipher: C, data: Vec<u8>) -> Result<meshtastic::Data, String>
where
    C: StreamCipher,
{
    let mut buffer = data;
    cipher
        .try_apply_keystream(&mut buffer)
        .map_err(|e| format!("Unable to decrypt: {:?}", e))?;

    meshtastic::Data::decode(buffer.as_slice())
        .map_err(|e| format!("Unable to construct `Data`: {:?}", e))
}

impl Decrypt for Symmetric {
    async fn decrypt(&self, packet_id: u32, data: Vec<u8>) -> Result<meshtastic::Data, String> {
        let nonce = prepare_nonce(packet_id, self.from);

        match self.key {
            Key::K128(key) => {
                let cipher = Ctr128BE::<Aes128>::new(key.as_bytes().into(), &nonce.into());
                apply_symmetric_decryption(cipher, data)
            }
            Key::K256(key) => {
                let cipher = Ctr128BE::<Aes256>::new(key.as_bytes().into(), &nonce.into());
                apply_symmetric_decryption(cipher, data)
            }
        }
    }
}
