use std::fmt;

use pki::PKI;
use symmetric::Symmetric;

pub mod pki;
pub mod symmetric;

pub trait Decrypt {
    fn decrypt(&self, packet_id: u32, data: Vec<u8>) -> Result<Vec<u8>, String>;
}

pub enum Cryptor {
    Symmetric(String, Symmetric),
    PKI(PKI),
}

impl fmt::Display for Cryptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cryptor::Symmetric(name, _) => write!(f, "{}", name),
            Cryptor::PKI(_) => write!(f, "PKI"),
        }
    }
}

impl Decrypt for Cryptor {
    fn decrypt(&self, packet_id: u32, data: Vec<u8>) -> Result<Vec<u8>, String> {
        match self {
            Cryptor::Symmetric(_, symmetric) => symmetric.decrypt(packet_id, data),
            Cryptor::PKI(pki) => pki.decrypt(packet_id, data),
        }
    }
}

pub trait Encrypt {
    fn encrypt(&self, packet_id: u32, data: Vec<u8>) -> Result<Vec<u8>, String>;
}

impl Encrypt for Cryptor {
    fn encrypt(&self, packet_id: u32, buffer: Vec<u8>) -> Result<Vec<u8>, String> {
        match self {
            Cryptor::Symmetric(_, symmetric) => symmetric.encrypt(packet_id, buffer),
            Cryptor::PKI(pki) => pki.encrypt(packet_id, buffer),
        }
    }
}
