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

#[derive(Default, Debug, Clone)]
pub struct Keyring {
    channels: Vec<Channel>,
    peers: HashMap<NodeId, Peer>,
}

impl Keyring {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_channel(&mut self, name: &str, key: Key) -> Result<(), String> {
        let channel = Channel::new(name, key)?;
        println!("init channel {}", channel);
        self.channels.push(channel);
        Ok(())
    }

    pub fn add_peer(&mut self, node_id: NodeId, secret_key: K256) -> Result<(), String> {
        let peer = Peer::new(node_id, secret_key)?;
        println!("init peer {}", peer);
        self.peers.entry(node_id).or_insert(peer);
        Ok(())
    }

    pub fn add_remote_peer(&mut self, node_id: NodeId, public_key: K256) -> Result<(), String> {
        let peer = Peer::new_remote_peer(node_id, public_key)?;
        println!("init remote peer {}", peer);
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
