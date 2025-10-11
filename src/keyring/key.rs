use base64::{Engine, engine::general_purpose};
use rand::Rng;
use serde::de::{self, Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use std::fmt;
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub struct K128(pub [u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub struct K256(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub enum Key {
    K128(K128),
    K256(K256),
}

impl Default for K128 {
    fn default() -> Self {
        let mut rng = rand::rng();

        K128(rng.random())
    }
}

impl Default for K256 {
    fn default() -> Self {
        let mut rng = rand::rng();

        K256(rng.random())
    }
}

impl K256 {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn public_key(&self) -> K256 {
        let secret = StaticSecret::from(*self.as_bytes());

        K256(PublicKey::from(&secret).to_bytes())
    }
}

impl K128 {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl Key {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Key::K128(k128) => k128.as_bytes(),
            Key::K256(k256) => k256.as_bytes(),
        }
    }
}

impl From<[u8; 16]> for K128 {
    fn from(value: [u8; 16]) -> Self {
        K128(value)
    }
}

impl From<[u8; 32]> for K256 {
    fn from(value: [u8; 32]) -> Self {
        K256(value)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::K128(key) => write!(f, "{}", key),
            Key::K256(key) => write!(f, "{}", key),
        }
    }
}

impl fmt::Display for K256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", general_purpose::STANDARD.encode(self.0))
    }
}

impl fmt::Display for K128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", general_purpose::STANDARD.encode(self.0))
    }
}

impl TryFrom<&str> for K256 {
    type Error = String;

    // From Base64 string
    fn try_from(base64_key: &str) -> Result<Self, Self::Error> {
        let bytes = general_purpose::STANDARD
            .decode(base64_key)
            .map_err(|e| e.to_string())?;

        match bytes.len() {
            32 => Ok(K256(bytes.try_into().unwrap())),
            unsupported_size => Err(format!("Unsupported key size: {} bytes", unsupported_size)),
        }
    }
}

impl TryFrom<String> for K256 {
    type Error = String;

    // From Base64 string
    fn try_from(base64_key: String) -> Result<Self, Self::Error> {
        base64_key.as_str().try_into()
    }
}

impl TryFrom<String> for Key {
    type Error = String;

    // From Base64 string
    fn try_from(base64_key: String) -> Result<Self, Self::Error> {
        Key::try_from(base64_key.as_str())
    }
}

impl TryFrom<&str> for Key {
    type Error = String;

    // From Base64 string
    fn try_from(base64_key: &str) -> Result<Self, Self::Error> {
        let bytes = general_purpose::STANDARD
            .decode(base64_key)
            .map_err(|e| e.to_string())?;

        Key::try_from(bytes)
    }
}

impl TryFrom<Vec<u8>> for Key {
    type Error = String;

    // From Base64 string
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        match bytes.len() {
            32 => Ok(Key::K256(K256(bytes.try_into().unwrap()))),
            16 => Ok(Key::K128(K128(bytes.try_into().unwrap()))),
            unsupported_size => Err(format!("Unsupported key size: {} bytes", unsupported_size)),
        }
    }
}

impl Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.try_into().map_err(de::Error::custom)
    }
}

impl Serialize for K256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for K256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.try_into().map_err(de::Error::custom)
    }
}
