use std::fmt;

use meshtastic_connect::keyring::node_id::NodeId;

// Last byte of NodeId
#[derive(
    Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct ByteNodeId(u8);

impl ByteNodeId {
    pub fn zero() -> Self {
        ByteNodeId(0)
    }
}

impl From<u32> for ByteNodeId {
    fn from(value: u32) -> Self {
        ByteNodeId(value.to_ne_bytes()[0])
    }
}

impl From<NodeId> for ByteNodeId {
    fn from(node_id: NodeId) -> Self {
        ByteNodeId(node_id.to_bytes()[0])
    }
}

impl TryFrom<&str> for ByteNodeId {
    type Error = std::num::ParseIntError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() == 3 {
            Ok(ByteNodeId(u8::from_str_radix(&value[1..], 16)?))
        } else {
            Ok(ByteNodeId(u8::from_str_radix(value, 16)?))
        }
    }
}

impl fmt::Display for ByteNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "~{:02x}", self.0)
    }
}

impl PartialEq<NodeId> for ByteNodeId {
    fn eq(&self, other: &NodeId) -> bool {
        self.0 == other.to_bytes()[0]
    }
}
