mod channel;
pub mod decryptor;
pub mod key;
pub mod node_id;
mod peer;

use std::collections::HashMap;

use channel::Channel;
use decryptor::{Decryptor, pki::PKI, symmetric::Symmetric};
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

    pub fn decryptor_for(&self, from: NodeId, to: NodeId, chan_no: u32) -> Option<Decryptor> {
        if chan_no == 0x0 {
            if let (Some(remote_peer), Some(local_peer)) =
                (self.peers.get(&from), self.peers.get(&to))
            {
                if let Some(private_key) = local_peer.private_key {
                    Some(Decryptor::PKI(
                        format!("{} → {}", from, to),
                        PKI::new(from, remote_peer.public_key, private_key),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            if let Some(channel) = self
                .channels
                .iter()
                .find(|chan| chan.channel_hash == chan_no)
            {
                Some(Decryptor::Symmetric(
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
    }
}
