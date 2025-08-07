use super::key::Key;
use std::fmt;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Channel {
    pub name: String,
    pub key: Key,
    pub channel_hash: u32,
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Channel([{:#x}] {})", self.channel_hash, self.name)
    }
}

impl Channel {
    pub fn new(name: &str, key: Key) -> Result<Self, String> {
        let chan_no = Self::generate_hash(name, key.as_bytes()) as u32;

        Ok(Self {
            name: name.to_string(),
            key,
            channel_hash: chan_no,
        })
    }

    fn xor_hash(data: &[u8]) -> u8 {
        data.iter().fold(0u8, |acc, &b| acc ^ b)
    }

    fn generate_hash(name: &str, key: &[u8]) -> i16 {
        if key.is_empty() {
            -1
        } else {
            let mut h = Self::xor_hash(name.as_bytes());
            h ^= Self::xor_hash(key);
            h as i16
        }
    }
}
