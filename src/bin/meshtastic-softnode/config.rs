use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, DeserializeOwned},
};
use serde_yaml_ng::{from_reader, to_writer};

use meshtastic_connect::{
    keyring::{
        key::{K256, Key},
        node_id::NodeId,
    },
    transport::stream::{Serial, StreamAddress},
};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    net::{IpAddr, SocketAddr},
    time::Duration,
};

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
pub(crate) struct Args {
    // Path to config file
    // If file not exists, create it with default values
    #[arg(short, long, default_value_t = String::from("softnode.yaml"))]
    pub(crate) main_file: String,
    // Path to file with keys to decode Peers and Channels messages
    // This file is rewrite if new nodes are coming
    #[arg(short, long, default_value_t = String::from("keys.yaml"))]
    pub(crate) keys_file: String,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct TransitConfig {
    // Transit if this is destination. Transit all if empty.
    pub(crate) to: Vec<NodeId>,
    // Transit if this is source. Transit all if empty.
    pub(crate) from: Vec<NodeId>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct MQTTConfig {
    pub(crate) server_addr: String,
    pub(crate) server_port: u16,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) subscribe: Vec<String>,
    pub(crate) filter: TransitConfig,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct Channel {
    pub(crate) name: String,
    pub(crate) key: Key,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct Peer {
    pub(crate) name: String,
    pub(crate) node_id: NodeId,
    pub(crate) highlight: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) public_key: Option<K256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) private_key: Option<K256>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Hops(u16);

impl Default for Hops {
    fn default() -> Self {
        Self(3)
    }
}

impl From<Hops> for u32 {
    fn from(val: Hops) -> Self {
        val.0.into()
    }
}

impl<'de> Deserialize<'de> for Hops {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        if value <= 7 {
            Ok(Hops(value))
        } else {
            Err(de::Error::custom("Hops must be in range 0..=7"))
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct SoftNodeChannel {
    pub(crate) name: String,
    pub(crate) node_info: Option<Duration>,
    pub(crate) hop_start: Hops,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub(crate) struct MulticastBindAddr(SocketAddr);

impl TryFrom<String> for MulticastBindAddr {
    type Error = std::net::AddrParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Ok(Self(s.parse::<SocketAddr>()?))
    }
}

impl TryFrom<&str> for MulticastBindAddr {
    type Error = std::net::AddrParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(s.parse::<SocketAddr>()?))
    }
}

impl std::fmt::Display for MulticastBindAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Into<SocketAddr> for MulticastBindAddr {
    fn into(self) -> SocketAddr {
        self.0
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub(crate) struct MulticastBind {
    // Bind address for receiving multicast packets
    pub(crate) address: MulticastBindAddr,

    // Interface index to send multicast packets
    pub(crate) interface: IpAddr,
}

impl Default for MulticastBind {
    fn default() -> Self {
        Self {
            address: "224.0.0.69:4403".try_into().unwrap(),
            interface: "0.0.0.0".parse().unwrap(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) enum SoftNodeTransport {
    Multicast(MulticastBind),
    TCP(SocketAddr),
    Serial(Serial),
}

impl Default for SoftNodeTransport {
    fn default() -> Self {
        Self::Multicast(Default::default())
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct SoftNodeConfig {
    pub(crate) transport: SoftNodeTransport,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) short_name: String,
    #[serde(default)]
    pub(crate) node_id: NodeId,
    #[serde(default)]
    pub(crate) private_key: K256,
    #[serde(default)]
    pub(crate) channels: Vec<SoftNodeChannel>,
}

impl Default for SoftNodeConfig {
    fn default() -> Self {
        Self {
            transport: Default::default(),
            name: "SoftNode".to_string(),
            short_name: "SFTN".to_string(),
            node_id: NodeId::default(),
            private_key: K256::default(),
            channels: vec![SoftNodeChannel {
                name: "LongFast".into(),
                hop_start: Hops(7),
                node_info: Some(Duration::from_secs(900)),
            }],
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct KeyringConfig {
    pub(crate) channels: Vec<Channel>,
    pub(crate) peers: Vec<Peer>,
}

impl Default for KeyringConfig {
    fn default() -> Self {
        Self {
            channels: vec![Channel {
                name: "LongFast".into(),
                key: "1PG7OiApB1nwvP+rz05pAQ==".try_into().unwrap(),
            }],
            peers: vec![],
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct Config {
    pub(crate) soft_node: SoftNodeConfig,
    pub(crate) keys: KeyringConfig,
}

pub(crate) fn config_write<T>(path: &String, config: &T) -> Result<(), String>
where
    T: Serialize,
{
    println!("Try to write {}", path);

    match File::create(&path) {
        Ok(mut file) => {
            let writer = BufWriter::new(&mut file);

            match to_writer(writer, &config) {
                Ok(_) => {
                    println!("... ok");
                    Ok(())
                }
                Err(e) => {
                    println!("Config file `{}` not written: {}", path, e);
                    Err(format!("Config file `{}` not written: {}", path, e))
                }
            }
        }
        Err(e) => {
            println!("Config file `{}` is not accessible: {}", path, e);
            Err(format!("Config file `{}` is not accessible: {}", path, e))
        }
    }
}

pub(crate) fn config_read<T>(path: &String) -> Result<Option<T>, serde_yaml_ng::Error>
where
    T: DeserializeOwned,
{
    println!("Try to read {}", path);
    match File::open(&path) {
        Ok(file) => {
            let reader = BufReader::new(file);

            Ok(Some(from_reader::<_, T>(reader)?))
        }
        Err(e) => {
            println!("Config file `{}` is not accessible: {}", path, e);
            Ok(None)
        }
    }
}

pub(crate) fn load_config(args: &Args) -> Option<Config> {
    let soft_node = match config_read::<SoftNodeConfig>(&args.main_file) {
        Ok(soft_node_or_not) => {
            if let Some(soft_node) = soft_node_or_not {
                Some(soft_node)
            } else {
                println!("Connection config not found, write default");
                let soft_node = Default::default();
                if let Err(e) = config_write(&args.main_file, &soft_node) {
                    println!("Failed to write default connection config: {}", e);
                }
                Some(soft_node)
            }
        }
        Err(e) => {
            println!("Failed to parse {}: {}", args.main_file, e);
            None
        }
    };

    let keys = match config_read::<KeyringConfig>(&args.keys_file) {
        Ok(keys_or_not) => {
            if let Some(keys) = keys_or_not {
                println!("Keys config loaded");
                Some(keys)
            } else {
                println!("Key config not loaded, write default");
                let keys = Default::default();
                if let Err(e) = config_write(&args.keys_file, &keys) {
                    println!("Failed to write default key config: {}", e);
                }
                Some(keys)
            }
        }
        Err(e) => {
            println!("Failed to parse {}: {}", args.keys_file, e);
            None
        }
    };

    if !keys.is_some() || !soft_node.is_some() {
        println!("Soft node config not loaded");
        None
    } else {
        Some(Config {
            soft_node: soft_node.unwrap(),
            keys: keys.unwrap(),
        })
    }
}
