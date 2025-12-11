use base64::{Engine, engine::general_purpose};
use rand::Rng;
use serde::de::{self, Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use std::fmt;
use x25519_dalek::{PublicKey, StaticSecret};

// Short key, non-secure key uses a static part
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub struct KIndex(pub [u8; 16]);

// AES-128
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub struct K128(pub [u8; 16]);

// AES-256
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub struct K256(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Hash)]
pub enum Key {
    KIndex(KIndex),
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

const DEFAULT_PSK: [u8; 16] = [
    0xd4, 0xf1, 0xbb, 0x3a, 0x20, 0x29, 0x07, 0x59, 0xf0, 0xbc, 0xff, 0xab, 0xcf, 0x4e, 0x69, 0x01,
];

impl Default for KIndex {
    fn default() -> Self {
        Self(DEFAULT_PSK)
    }
}

impl KIndex {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
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
            Key::KIndex(kindex) => kindex.as_bytes(),
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

impl From<[u8; 1]> for KIndex {
    fn from(value: [u8; 1]) -> Self {
        let index = value[0];
        let mut key = DEFAULT_PSK;
        // Reference: https://github.com/meshtastic/firmware/blob/0e3e8b7607ffdeeabc34a3a349e108e0c3a1363d/src/mesh/Channels.cpp#L236
        // Bump last byte to channel index where Index=0x01 means no default psk change
        key[15] = index;
        KIndex(key)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::KIndex(kindex) => write!(f, "{}", kindex),
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

impl fmt::Display for KIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", general_purpose::STANDARD.encode(&[self.0[15]]))
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
            unsupported_size => Err(format!(
                "Unsupported key size: {} bytes: {:?}",
                unsupported_size, base64_key
            )),
        }
    }
}

impl TryFrom<&str> for KIndex {
    type Error = String;

    // From Base64 string
    fn try_from(base64_key: &str) -> Result<Self, Self::Error> {
        let bytes = general_purpose::STANDARD
            .decode(base64_key)
            .map_err(|e| e.to_string())?;

        bytes.try_into()
    }
}

impl TryFrom<Vec<u8>> for KIndex {
    type Error = String;

    // From Base64 string
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        match bytes.len() {
            1 => Ok([bytes[0]]
                .try_into()
                .map_err(|e| format!("Unsupported input for indexed key: {:?}", e))?),
            unsupported_size => Err(format!(
                "Excepted 1-byte key, not {} bytes: {:x?}",
                unsupported_size, bytes
            )),
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

impl TryFrom<String> for KIndex {
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
        let len = bytes.len();

        if len == 1 {
            Ok(Key::KIndex(bytes.try_into().map_err(|e| {
                format!("KIndex unsupported data: {:#x?}", e)
            })?))
        } else if len <= 16 {
            let bytes = vec_to_array16_padded(bytes);
            Ok(Key::K128(K128(bytes.try_into().map_err(|e| {
                format!("K128 unsupported data: {:#x?}", e)
            })?)))
        } else if len <= 32 {
            let bytes = vec_to_array32_padded(bytes);
            Ok(Key::K256(K256(bytes.try_into().map_err(|e| {
                format!("K256 unsupported data: {:#x?}", e)
            })?)))
        } else {
            Err(format!(
                "Expected 1, 16, or 32 bytes, not {} bytes: {:#x?}",
                len, bytes
            ))
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

fn vec_to_array32_padded(vec: Vec<u8>) -> [u8; 32] {
    let mut array = [0u8; 32];
    let len = vec.len().min(32);
    array[..len].copy_from_slice(&vec[..len]);
    array
}

fn vec_to_array16_padded(vec: Vec<u8>) -> [u8; 16] {
    let mut array = [0u8; 16];
    let len = vec.len().min(16);
    array[..len].copy_from_slice(&vec[..len]);
    array
}
