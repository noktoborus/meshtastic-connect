use rand::Rng;
use serde::de::{self, Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use std::fmt::{self, LowerHex, UpperHex};
use std::num;
use std::str;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn to_bytes(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    pub fn broadcast() -> Self {
        NodeId(0xffffffff)
    }
}

impl Default for NodeId {
    fn default() -> Self {
        let mut rng = rand::rng();

        NodeId(rng.random())
    }
}

impl LowerHex for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

impl UpperHex for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}", self.0)
    }
}

impl Serialize for NodeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.try_into().map_err(de::Error::custom)
    }
}

impl TryFrom<&str> for NodeId {
    type Error = num::ParseIntError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let hex_part = s.strip_prefix('!').map_or(s, |v| v);

        let value = u32::from_str_radix(hex_part, 16)?;
        Ok(NodeId(value))
    }
}

impl TryFrom<String> for NodeId {
    type Error = num::ParseIntError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.as_str().try_into()
    }
}

impl From<u32> for NodeId {
    fn from(value: u32) -> Self {
        NodeId(value)
    }
}

impl From<NodeId> for u32 {
    fn from(value: NodeId) -> Self {
        value.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "!{:08x}", self.0)
    }
}

impl From<NodeId> for String {
    fn from(value: NodeId) -> Self {
        value.to_string()
    }
}
