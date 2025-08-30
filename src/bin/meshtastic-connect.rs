use meshtastic_connect::keyring;

use clap::Parser;
use meshtastic_connect::meshtastic_print::{
    print_from_radio_payload, print_mesh_packet, print_service_envelope,
};
use meshtastic_connect::transport::udp::{Interface, Multicast};
use meshtastic_connect::transport::{
    stream::{self, Serial, Stream, StreamAddress},
    udp::UDP,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_yaml_ng::from_reader;

use chrono::Local;
use keyring::{
    Keyring,
    key::{K256, Key},
    node_id::NodeId,
};
use rumqttc::{AsyncClient, MqttOptions, QoS};
use std::time::Duration;
use std::{fs::File, io::BufReader, net::SocketAddr};
use tokio::io::AsyncWriteExt;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    // Path to config file
    #[arg(short, long, default_value_t = String::from("connect.yaml"))]
    connection_file: String,
    // Path to file with keys to decode Peers and Channels messages
    #[arg(short, long, default_value_t = String::from("keys.yaml"))]
    keys_file: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct TCPConfig {
    connect_to: SocketAddr,
    heartbeat_seconds: u64,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
struct MQTTConfig {
    server_addr: String,
    server_port: u16,
    username: String,
    password: String,
    subscribe: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct MulticastConfig {
    listen_address: SocketAddr,
}

impl Default for MulticastConfig {
    fn default() -> Self {
        Self {
            listen_address: "224.0.0.69:4403".parse().unwrap(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct SerialConfig {
    tty: String,
    heartbeat_seconds: u64,
    baudrate: u32,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            tty: "/dev/ttyS0".into(),
            heartbeat_seconds: 5,
            baudrate: 115200,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
enum Mode {
    TCP(TCPConfig),
    Serial(SerialConfig),
    Multicast(MulticastConfig),
    MQTT(MQTTConfig),
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Multicast(Default::default())
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Channel {
    name: String,
    key: Key,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Peer {
    name: String,
    node_id: NodeId,
    #[serde(skip_serializing_if = "Option::is_none")]
    public_key: Option<K256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    private_key: Option<K256>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct ConnectionConfig {
    mode: Mode,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct KeysConfig {
    channels: Vec<Channel>,
    peers: Vec<Peer>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Config {
    connection: ConnectionConfig,
    keys: KeysConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            connection: ConnectionConfig {
                mode: Default::default(),
            },
            keys: KeysConfig {
                channels: vec![
                    Channel {
                        name: "LongFast".into(),
                        key: "1PG7OiApB1nwvP+rz05pAQ==".try_into().unwrap(),
                    },
                    Channel {
                        name: "ShortFast".into(),
                        key: "1PG7OiApB1nwvP+rz05pAQ==".try_into().unwrap(),
                    },
                ],
                peers: vec![],
            },
        }
    }
}

fn config_read<T>(path: &String) -> Option<T>
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

fn load_config(args: &Args) -> Option<Config> {
    let connection = config_read::<ConnectionConfig>(&args.connection_file);
    let keys = config_read::<KeysConfig>(&args.keys_file);

    if let (Some(connection), Some(keys)) = (connection, keys) {
        Some(Config { connection, keys })
    } else {
        None
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config = load_config(&args).expect("Config file not loaded: try type `--help` to get help");

    println!("=== loaded config ===");
    println!("{}", serde_yaml_ng::to_string(&config).unwrap());
    println!("=== ===");

    let mut keyring = Keyring::new();
    let mut filter_by_nodeid: Vec<NodeId> = Default::default();

    for channel in config.keys.channels {
        keyring
            .add_channel(channel.name.as_str(), channel.key)
            .unwrap();
    }

    for peer in config.keys.peers {
        if let Some(skey) = peer.private_key {
            keyring.add_peer(peer.node_id, skey).unwrap();
        } else if let Some(pkey) = peer.public_key {
            keyring.add_remote_peer(peer.node_id, pkey).unwrap();
        }
    }

    println!();
    match config.connection.mode {
        Mode::MQTT(mqtt) => {
            println!(
                "Connect to MQTT {} port {}: {:?}",
                mqtt.server_addr, mqtt.server_port, mqtt.subscribe
            );

            let mut mqttoptions =
                MqttOptions::new("rumqtt-async", mqtt.server_addr, mqtt.server_port);
            mqttoptions.set_keep_alive(Duration::from_secs(5));
            mqttoptions.set_credentials(mqtt.username, mqtt.password);

            let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);
            for topic in mqtt.subscribe {
                client.subscribe(topic, QoS::AtMostOnce).await.unwrap();
            }

            loop {
                let notification = eventloop.poll().await.unwrap();
                let system_time = Local::now().format("%H:%M:%S").to_string();

                if let rumqttc::Event::Incoming(packet) = notification {
                    match packet {
                        rumqttc::Packet::Publish(publish) => {
                            println!("> {} [size: {}] ", publish.topic, publish.payload.len());
                            print_service_envelope(publish.payload, &keyring, &filter_by_nodeid)
                                .await;
                        }
                        rumqttc::Packet::PingReq => {}
                        rumqttc::Packet::PingResp => {}
                        _ => {
                            println!("> [{}] {:?}", system_time, packet)
                        }
                    }
                }
            }
        }
        Mode::TCP(tcp) => {
            println!("Connect to TCP {}", tcp.connect_to);

            let connection = Stream::new(
                StreamAddress::TCPSocket(tcp.connect_to),
                Duration::from_secs(tcp.heartbeat_seconds),
            );

            connect_to_stream(connection, &keyring, &filter_by_nodeid).await;
        }
        Mode::Serial(serial) => {
            println!(
                "Connect to serial port {} with baudrate {}",
                serial.tty, serial.baudrate
            );

            let connection = Stream::new(
                StreamAddress::Serial(Serial {
                    tty: serial.tty,
                    baudrate: serial.baudrate,
                }),
                Duration::from_secs(serial.heartbeat_seconds),
            );

            connect_to_stream(connection, &keyring, &filter_by_nodeid).await;
        }
        Mode::Multicast(multicast) => {
            println!("Listen multicast on {}", multicast.listen_address);
            let mut connection = UDP::new(
                multicast.listen_address,
                multicast.listen_address,
                Some(Multicast {
                    address: multicast.listen_address.ip(),
                    interface: Interface::unspecified(),
                }),
            );

            connection.connect().await.unwrap();
            loop {
                let (mesh_packet, _) = connection.recv().await.unwrap();

                print_mesh_packet(mesh_packet, &keyring, &filter_by_nodeid).await;

                println!();
            }
        }
    }
}

async fn connect_to_stream(
    mut connection: Stream,
    keyring: &Keyring,
    filter_by_nodeid: &Vec<NodeId>,
) {
    connection.connect().await.unwrap();
    loop {
        let stream_data = connection.recv().await.unwrap();

        match stream_data {
            stream::StreamData::FromRadio(from_radio) => {
                if let Some(payload_variant) = from_radio.payload_variant {
                    println!("> message id: {:x}", from_radio.id);
                    print_from_radio_payload(payload_variant, keyring, filter_by_nodeid).await;
                } else {
                    println!("> message id: {:x} no payload", from_radio.id);
                }
                println!();
            }
            stream::StreamData::Unstructured(bytes) => {
                tokio::io::stderr().write_all(&bytes).await.unwrap();
            }
        }
    }
}
