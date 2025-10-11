mod channel;
pub mod cryptor;
pub mod key;
pub mod node_id;
mod peer;

use std::collections::HashMap;

use channel::Channel;
use cryptor::{Cryptor, pki::PKI, symmetric::Symmetric};
use key::{K256, Key};
use node_id::NodeId;
use peer::Peer;
use serde::{Deserialize, Serialize};

fn serialize_peers<S>(peers: &HashMap<NodeId, Peer>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut list = peers.values().collect::<Vec<_>>();
    list.sort_by_key(|peer| peer.node_id);
    Vec::serialize(&list, serializer)
}

fn deserialize_peers<'de, D>(deserializer: D) -> Result<HashMap<NodeId, Peer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let list: Vec<Peer> = Vec::deserialize(deserializer)?;
    let mut peers = HashMap::new();
    for peer in list {
        peers.insert(peer.node_id, peer);
    }
    Ok(peers)
}

#[derive(Default, Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct Keyring {
    #[serde(rename = "Channels")]
    channels: Vec<Channel>,
    #[serde(
        rename = "Peers",
        serialize_with = "serialize_peers",
        deserialize_with = "deserialize_peers"
    )]
    peers: HashMap<NodeId, Peer>,
}

impl Keyring {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_channel(&mut self, name: &str, key: Key) -> Result<(), String> {
        let channel = Channel::new(name, key);
        self.channels.push(channel);
        Ok(())
    }

    pub fn add_peer(&mut self, node_id: NodeId, secret_key: K256) -> Result<(), String> {
        let peer = Peer::new(node_id, secret_key)?;
        self.peers.entry(node_id).or_insert(peer);
        Ok(())
    }

    pub fn add_remote_peer(&mut self, node_id: NodeId, public_key: K256) -> Result<(), String> {
        let peer = Peer::new_remote_peer(node_id, public_key)?;
        self.peers.entry(node_id).or_insert(peer);
        Ok(())
    }

    // Get cryptographic API for channel name
    // Returns a tuple containing the cryptographic API and the channel's hash
    pub fn cryptor_for_channel_name(
        &self,
        from: NodeId,
        channel_name: &String,
    ) -> Option<(Cryptor, u32)> {
        if let Some(channel) = self.channels.iter().find(|chan| chan.name == *channel_name) {
            Some((
                Cryptor::Symmetric(
                    channel.name.clone(),
                    Symmetric {
                        from,
                        key: channel.key.clone(),
                    },
                ),
                channel.channel_hash,
            ))
        } else {
            None
        }
    }

    // Get cryptographic API for pair of nodes
    pub fn cryptor_for_pki(&self, from: NodeId, to: NodeId) -> Option<Cryptor> {
        if let (Some(remote_peer), Some(local_peer)) = (self.peers.get(&from), self.peers.get(&to))
        {
            if let Some(private_key) = local_peer.private_key {
                Some(Cryptor::PKI(PKI::new(
                    from,
                    remote_peer.public_key,
                    private_key,
                )))
            } else {
                None
            }
        } else {
            None
        }
    }

    // Get cryptographic API for channel from `MeshPacket::channel` field
    pub fn cryptor_for_channel(&self, from: NodeId, channel: u32) -> Option<Cryptor> {
        if let Some(channel) = self
            .channels
            .iter()
            .find(|chan| chan.channel_hash == channel)
        {
            Some(Cryptor::Symmetric(
                channel.name.clone(),
                Symmetric {
                    from,
                    key: channel.key.clone(),
                },
            ))
        } else {
            None
        }
    }

    // Get cryptographic API for `MeshPacket::channel` field
    pub fn cryptor_for(&self, from: NodeId, to: NodeId, channel: u32) -> Option<Cryptor> {
        if channel == 0x0 {
            self.cryptor_for_pki(from, to)
        } else {
            self.cryptor_for_channel(from, channel)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Keyring, key::Key};
    use pretty_assertions::assert_eq;

    fn build_test_keyring() -> Keyring {
        let mut keyring = Keyring::new();

        keyring
            .add_channel("Channel1", Key::K128(Default::default()))
            .unwrap();
        keyring
            .add_channel("Channel2", Key::K256(Default::default()))
            .unwrap();

        keyring
            .add_peer(0xdeadbeef.into(), Default::default())
            .unwrap();
        keyring
            .add_remote_peer(0xbbbbaaaa.into(), Default::default())
            .unwrap();
        keyring
    }

    #[test]
    fn yaml_serialize_and_deserialize() {
        let se_keyring = build_test_keyring();
        let yaml = serde_yaml_ng::to_string(&se_keyring).unwrap();
        let de_keyring = serde_yaml_ng::from_str(&yaml).unwrap();

        assert_eq!(se_keyring, de_keyring);
    }

    #[test]
    fn ron_serialize_and_deserialize() {
        let se_keyring = build_test_keyring();
        let ron_data = ron::ser::to_string(&se_keyring).unwrap();
        let de_keyring = ron::de::from_str(&ron_data).unwrap();

        assert_eq!(se_keyring, de_keyring);
    }
}
