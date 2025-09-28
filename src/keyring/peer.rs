use super::{key::K256, node_id::NodeId};
use std::fmt;

#[derive(Debug, Clone, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
pub struct Peer {
    #[serde(rename = "NodeId")]
    pub node_id: NodeId,
    #[serde(rename = "PublicKey")]
    pub public_key: K256,
    #[serde(rename = "PrivateKey")]
    pub private_key: Option<K256>,
}

impl Peer {
    pub fn new(node_id: NodeId, secret_key: K256) -> Result<Self, String> {
        Ok(Self {
            node_id,
            public_key: secret_key.public_key(),
            private_key: Some(secret_key),
        })
    }

    pub fn new_remote_peer(node_id: NodeId, public_key: K256) -> Result<Self, String> {
        Ok(Self {
            node_id,
            public_key,
            private_key: None,
        })
    }
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Peer({} pkey={})", self.node_id, self.public_key)
    }
}
