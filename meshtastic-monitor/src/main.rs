mod meshtastic_print;

use clap::Parser;
use futures::{SinkExt, StreamExt};
use meshtastic_connect::keyring;
use meshtastic_connect::meshtastic::Heartbeat;
use meshtastic_connect::meshtastic::to_radio::PayloadVariant;
use meshtastic_connect::transport::stream::Stream;
use meshtastic_connect::transport::udp::{Interface, Multicast};
use meshtastic_connect::transport::{
    stream, stream::serial::SerialBuilder, stream::tcp::TcpBuilder, udp::UdpBuilder,
};
use meshtastic_print::{print_from_radio_payload, print_mesh_packet, print_service_envelope};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_yaml_ng::from_reader;

use chrono::Local;
use keyring::{
    Keyring,
    key::{K256, Key},
    node_id::NodeId,
};
use rumqttc::{AsyncClient, MqttOptions, QoS};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::exit;
use std::time::Duration;
use std::{fs::File, io::BufReader, net::SocketAddr};
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    // Path to config file
    #[arg(short, long, default_value_t = String::from("monitor.yaml"))]
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
                            print_service_envelope(publish.payload, &keyring).await;
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

            let connection = TcpBuilder::new(tcp.connect_to).connect().await.unwrap();

            connect_to_stream(
                connection,
                Duration::from_secs(tcp.heartbeat_seconds),
                &keyring,
            )
            .await;
        }
        Mode::Serial(serial) => {
            println!(
                "Connect to serial port {} with baudrate {}",
                serial.tty, serial.baudrate
            );

            let connection = SerialBuilder::new(serial.tty, serial.baudrate)
                .connect()
                .await
                .unwrap();

            connect_to_stream(
                connection,
                Duration::from_secs(serial.heartbeat_seconds),
                &keyring,
            )
            .await;
        }
        Mode::Multicast(multicast) => {
            println!("Listen multicast on {}", multicast.listen_address);
            let connection = UdpBuilder::new(
                SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::UNSPECIFIED,
                    multicast.listen_address.port(),
                )),
                multicast.listen_address,
                Some(Multicast {
                    address: multicast.listen_address.ip(),
                    interface: Interface::unspecified(),
                }),
            );

            let mut connection = connection.connect().await.unwrap();
            loop {
                match connection.next().await {
                    Some(result) => {
                        let (mesh_packet, _) = result.unwrap();
                        print_mesh_packet(mesh_packet, &keyring).await;
                    }
                    None => {
                        println!("Connection closed");
                        break;
                    }
                };
                println!();
            }
        }
    }
}

async fn connect_to_stream(
    mut connection: Stream,
    heartbeat_interval: Duration,
    keyring: &Keyring,
) -> ! {
    let _ = connection.send(PayloadVariant::WantConfigId(0)).await;
    let mut hb_interval =
        tokio::time::interval_at(Instant::now() + heartbeat_interval, heartbeat_interval);

    loop {
        tokio::select! {
            _ = hb_interval.tick() => {
                    connection.send(PayloadVariant::Heartbeat(Heartbeat{})).await.unwrap();
                }
            stream_data = connection.next() => {
                match  stream_data {
                    // TODO: heartbeat
                    Some(stream_data) => match stream_data.unwrap() {
                        stream::StreamRecvData::FromRadio(packet_id, from_radio) => {
                            println!("> message id: {:x}", packet_id);
                            print_from_radio_payload(from_radio, keyring).await;
                            println!();
                        }
                        stream::StreamRecvData::Unstructured(bytes) => {
                            tokio::io::stderr().write_all(&bytes).await.unwrap();
                        }
                    },
                    None => {
                        println!("Connection closed");
                        exit(0);
                    }
                }
            }
        }
    }
}
