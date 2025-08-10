use std::fmt;

use pki::PKI;
use symmetric::Symmetric;

use crate::meshtastic;

pub mod pki;
pub mod symmetric;

pub trait Decrypt {
    fn decrypt(
        &self,
        packet_id: u32,
        data: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<meshtastic::Data, String>> + Send;
}

pub enum Decryptor {
    Symmetric(String, Symmetric),
    PKI(String, PKI),
}

impl fmt::Display for Decryptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Decryptor::Symmetric(name, _) => write!(f, "{}", name),
            Decryptor::PKI(name, _) => write!(f, "{}", name),
        }
    }
}

impl Decrypt for Decryptor {
    async fn decrypt(&self, packet_id: u32, data: Vec<u8>) -> Result<meshtastic::Data, String> {
        match self {
            Decryptor::Symmetric(_, symmetric) => symmetric.decrypt(packet_id, data).await,
            Decryptor::PKI(_, pki) => pki.decrypt(packet_id, data).await,
        }
    }
}
