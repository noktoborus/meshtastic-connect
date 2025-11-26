use std::fmt;

use meshtastic_connect::keyring::node_id::NodeId;

// Last byte of NodeId
#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteNodeId(u8);

impl ByteNodeId {
    pub fn zero() -> Self {
        ByteNodeId(0)
    }
}

impl From<u32> for ByteNodeId {
    fn from(value: u32) -> Self {
        ByteNodeId(value.to_ne_bytes()[3])
    }
}

impl From<NodeId> for ByteNodeId {
    fn from(node_id: NodeId) -> Self {
        ByteNodeId(node_id.to_bytes()[3])
    }
}

impl fmt::Display for ByteNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "!~~~~~~{:02x}", self.0)
    }
}

impl PartialEq<NodeId> for ByteNodeId {
    fn eq(&self, other: &NodeId) -> bool {
        self.0 == other.to_bytes()[3]
    }
}
