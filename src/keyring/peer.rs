use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, Visitor},
    ser::SerializeStruct,
};

use super::{key::K256, node_id::NodeId};
use std::fmt;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Hash)]
pub struct Peer {
    pub node_id: NodeId,
    pub public_key: K256,
    pub private_key: Option<K256>,
}

const PEER_NAME: &str = "Peer";
const NODE_ID_NAME: &str = "NodeId";
const PUBLIC_KEY_NAME: &str = "PublicKey";
const PRIVATE_KEY_NAME: &str = "PrivateKey";

impl Serialize for Peer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct(PEER_NAME, 2)?;
        state.serialize_field(NODE_ID_NAME, &self.node_id)?;
        if let Some(private_key) = self.private_key {
            state.serialize_field(PRIVATE_KEY_NAME, &Some(private_key))?;
        } else {
            state.serialize_field(PUBLIC_KEY_NAME, &self.public_key)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for Peer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PeerVisitor;

        impl<'de> Visitor<'de> for PeerVisitor {
            type Value = Peer;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(format!("struct {}", PEER_NAME).as_str())
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut node_id = None;
                let mut public_key = None;
                let mut private_key: Option<Option<K256>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        NODE_ID_NAME => {
                            if node_id.is_some() {
                                Err(de::Error::duplicate_field(NODE_ID_NAME))?
                            } else {
                                node_id = Some(map.next_value()?);
                                Ok(())
                            }
                        }
                        PUBLIC_KEY_NAME => {
                            if public_key.is_some() {
                                Err(de::Error::duplicate_field(PUBLIC_KEY_NAME))?
                            } else {
                                public_key = Some(map.next_value()?);
                                Ok(())
                            }
                        }
                        PRIVATE_KEY_NAME => {
                            if private_key.is_some() {
                                Err(de::Error::duplicate_field(PRIVATE_KEY_NAME))
                            } else {
                                private_key = Some(map.next_value()?);
                                Ok(())
                            }
                        }
                        v => Err(de::Error::unknown_field(
                            v,
                            &[NODE_ID_NAME, PUBLIC_KEY_NAME, PRIVATE_KEY_NAME],
                        )),
                    }?
                }
                let node_id = node_id.ok_or_else(|| de::Error::missing_field(NODE_ID_NAME))?;
                let private_key = private_key.unwrap_or(None);
                let public_key = if public_key.is_none() && private_key.is_some() {
                    private_key.unwrap().public_key()
                } else {
                    public_key.ok_or_else(|| de::Error::missing_field(PUBLIC_KEY_NAME))?
                };

                Ok(Peer {
                    node_id,
                    public_key,
                    private_key,
                })
            }
        }

        deserializer.deserialize_struct(
            PEER_NAME,
            &[NODE_ID_NAME, PUBLIC_KEY_NAME, PRIVATE_KEY_NAME],
            PeerVisitor,
        )
    }
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
