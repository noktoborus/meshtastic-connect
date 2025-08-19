use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeOwned},
};
use serde_yaml_ng::{from_reader, to_writer};

use meshtastic_connect::keyring::{
    key::{K256, Key},
    node_id::NodeId,
};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    net::SocketAddr,
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

impl Into<u32> for Hops {
    fn into(self) -> u32 {
        self.0.into()
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct SoftNodeConfig {
    pub(crate) bind_address: SocketAddr,
    pub(crate) name: String,
    pub(crate) node_id: NodeId,
    pub(crate) private_key: K256,
    pub(crate) channels: Vec<SoftNodeChannel>,
}

impl Default for SoftNodeConfig {
    fn default() -> Self {
        Self {
            bind_address: "224.0.0.69:4403".parse().unwrap(),
            name: "SoftNode".to_string(),
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

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct ConnectionConfig {
    pub(crate) soft_node: SoftNodeConfig,
    pub(crate) mqtt: MQTTConfig,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct KeyringConfig {
    pub(crate) channels: Vec<Channel>,
    pub(crate) peers: Vec<Peer>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct Config {
    pub(crate) connection: ConnectionConfig,
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

pub(crate) fn config_read<T>(path: &String) -> Option<T>
where
    T: DeserializeOwned,
{
    println!("Try to read {}", path);
    match File::open(&path) {
        Ok(file) => {
            let reader = BufReader::new(file);

            match from_reader::<_, T>(reader) {
                Ok(config) => {
                    println!("... ok");
                    Some(config)
                }
                Err(e) => {
                    println!("Config file `{}` not loaded: {}", path, e);
                    None
                }
            }
        }
        Err(e) => {
            println!("Config file `{}` is not accessible: {}", path, e);
            None
        }
    }
}

pub(crate) fn load_config(args: &Args) -> Option<Config> {
    let connection = if let Some(connection) = config_read::<ConnectionConfig>(&args.main_file) {
        connection
    } else {
        println!("Connection config not found, write default");
        let connection = Default::default();
        if let Err(e) = config_write(&args.main_file, &connection) {
            println!("Failed to write default connection config: {}", e);
        }
        connection
    };

    let keys = if let Some(keys) = config_read::<KeyringConfig>(&args.keys_file) {
        println!("Keys config loaded");
        keys
    } else {
        println!("Key config not loaded, write default");
        let keys = Default::default();
        if let Err(e) = config_write(&args.keys_file, &keys) {
            println!("Failed to write default key config: {}", e);
        }
        keys
    };

    Some(Config { connection, keys })
}
