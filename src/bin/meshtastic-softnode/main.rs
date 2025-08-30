mod config;
mod publish;
mod sqlite;

use clap::Parser;
use meshtastic_connect::{
    keyring::{
        Keyring,
        cryptor::{Decrypt, Encrypt},
        node_id::NodeId,
    },
    meshtastic::{self, ServiceEnvelope, mesh_packet},
    transport::{
        self, if_index_by_addr,
        stream::{Serial, Stream},
        udp::{Interface, Multicast, UDP},
    },
};
use prost::Message;
use publish::Publishable;
use rand::Rng;
use std::{collections::VecDeque, process, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    time::{Instant, sleep_until},
};

use crate::config::{Args, SoftNodeChannel, SoftNodeConfig, load_config};

enum ConnectionType {
    UDP(UDP),
    Stream(Stream),
}

struct Connection {
    stream_api_method: config::StreamAPIMethod,
    connection_type: ConnectionType,
}

enum RecvData {
    MeshPacket(meshtastic::MeshPacket),
    Unstructured(Vec<u8>),
}

struct Schedule {
    items: VecDeque<(Instant, (usize, usize))>,
}

impl Schedule {
    fn new(channels: &[SoftNodeChannel]) -> Self {
        let mut items = Vec::new();
        let now = Instant::now();

        for (channel_idx, channel) in channels.iter().enumerate() {
            for (publish_idx, _) in channel.publish.iter().enumerate() {
                items.push((now, (channel_idx, publish_idx)));
            }
        }

        items.sort_by_key(|(inst, _)| *inst);

        Self {
            items: VecDeque::from(items),
        }
    }

    fn add(&mut self, event_time: Instant, event_data: (usize, usize)) {
        let pos = self
            .items
            .binary_search_by_key(&event_time, |(t, _)| *t)
            .unwrap_or_else(|e| e);
        self.items.insert(pos, (event_time, event_data));
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.items.front().map(|(inst, _)| *inst)
    }

    fn pop_if_completed(&mut self) -> Option<(Instant, (usize, usize))> {
        let now = Instant::now();
        if let Some((event_time, _)) = self.items.front() {
            if *event_time <= now {
                return self.items.pop_front();
            }
        }
        None
    }
}

impl Connection {
    async fn connect(&mut self) -> Result<(), std::io::Error> {
        match &mut self.connection_type {
            ConnectionType::UDP(multicast) => multicast.connect().await,
            ConnectionType::Stream(stream) => stream.connect().await,
        }
    }

    async fn disconnect(&mut self) {
        match &mut self.connection_type {
            ConnectionType::UDP(multicast) => multicast.disconnect().await,
            ConnectionType::Stream(stream) => stream.disconnect().await,
        }
    }

    async fn send_mesh(
        &mut self,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        match &mut self.connection_type {
            ConnectionType::UDP(multicast) => multicast.send(mesh_packet).await,
            ConnectionType::Stream(stream) => match self.stream_api_method {
                config::StreamAPIMethod::Direct => {
                    let to_radio = meshtastic::to_radio::PayloadVariant::Packet(mesh_packet);
                    stream.send(to_radio).await
                }
                config::StreamAPIMethod::MQTTProxy => {
                    let gateway_id = NodeId::from(mesh_packet.id);
                    let service_envelope = ServiceEnvelope {
                        packet: Some(mesh_packet),
                        channel_id: "devel".into(),
                        gateway_id: gateway_id.into(),
                    };
                    let mqtt_proxy = meshtastic::MqttClientProxyMessage {
                        topic: "devel".into(),
                        retained: false,
                        payload_variant: Some(
                            meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(
                                service_envelope.encode_to_vec(),
                            ),
                        ),
                    };
                    let to_radio =
                        meshtastic::to_radio::PayloadVariant::MqttClientProxyMessage(mqtt_proxy);
                    stream.send(to_radio).await
                }
            },
        }
    }

    async fn recv_mesh(&mut self) -> Result<RecvData, Box<dyn std::error::Error>> {
        match &mut self.connection_type {
            ConnectionType::UDP(multicast) => {
                let (mesh_packet, _) = multicast.recv().await?;
                Ok(RecvData::MeshPacket(mesh_packet))
            }
            ConnectionType::Stream(stream) => match stream.recv().await? {
                transport::stream::StreamData::FromRadio(from_radio) => {
                    if let Some(payload_variant) = from_radio.payload_variant {
                        match payload_variant {
                            meshtastic::from_radio::PayloadVariant::Packet(mesh_packet) => {
                                if self.stream_api_method != config::StreamAPIMethod::Direct {
                                    Ok(RecvData::Unstructured(
                                        format!(
                                            "StreamAPI({:?}): Receive transport's mesh packet",
                                            self.stream_api_method
                                        )
                                        .into(),
                                    ))
                                } else {
                                    Ok(RecvData::MeshPacket(mesh_packet))
                                }
                            }
                            meshtastic::from_radio::PayloadVariant::MqttClientProxyMessage(
                                mqtt_proxy_msg,
                            ) => {
                                if self.stream_api_method != config::StreamAPIMethod::MQTTProxy {
                                    Ok(RecvData::Unstructured(
                                        format!(
                                            "StreamAPI({:?}): Ignoring MQTT proxy message",
                                            self.stream_api_method
                                        )
                                        .into(),
                                    ))
                                } else {
                                    if let Some(payload_variant) = mqtt_proxy_msg.payload_variant {
                                        match payload_variant {
                                        meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(items) => {
                                            match meshtastic::ServiceEnvelope::decode(items.as_slice()) {
                                                Ok(service_envelope) => {
                                                    if let Some(mesh_packet) = service_envelope.packet {
                                                        Ok(RecvData::MeshPacket(mesh_packet))
                                                    } else {
                                                        Ok(RecvData::Unstructured(format!("MQTT ServiceEnvelope: no Packet").into()))
                                                    }
                                                },
                                                Err(e) =>  Ok(RecvData::Unstructured(format!("MQTT ServiceEnvelope::decode: {e}").into())),
                                            }


                                        },
                                        meshtastic::mqtt_client_proxy_message::PayloadVariant::Text(text) => Ok(RecvData::Unstructured(format!("MQTT proto: got text: {:?}", text).into())),
                                    }
                                    } else {
                                        Ok(RecvData::Unstructured(
                                            "MQTT proto: no payload data".into(),
                                        ))
                                    }
                                }
                            }
                            _ => Ok(RecvData::Unstructured("got not mesh packet".into())),
                        }
                    } else {
                        Err("No payload variant".into())
                    }
                }
                transport::stream::StreamData::Unstructured(bytes_mut) => {
                    Ok(RecvData::Unstructured(bytes_mut.to_vec()))
                }
            },
        }
    }
}

async fn handle_timer_event(
    sqlite: &sqlite::SQLite,
    schedule: &mut Schedule,
    soft_node: &SoftNodeConfig,
    keyring: &Keyring,
    connection: &mut Connection,
) {
    while let Some((_, (channel_idx, publish_idx))) = schedule.pop_if_completed() {
        let channel = &soft_node.channels[channel_idx];
        let publish_descriptor = &channel.publish[publish_idx];

        println!(
            "Publishing {:?} to channel {}",
            publish_descriptor, channel.name
        );
        let (port_num, data_payload) = publish_descriptor.pack_to_data(&soft_node);
        let packet_id: u32 = rand::rng().random();
        let dest_node: u32 = 0xffffffff;
        let data = meshtastic::Data {
            portnum: port_num.into(),
            payload: data_payload,
            ..Default::default()
        };

        let (channel_hash, payload_variant) = if channel.disable_encryption {
            (
                channel_idx as u32,
                mesh_packet::PayloadVariant::Decoded(data.clone()),
            )
        } else {
            let (cryptor, channel_hash) = keyring
                .cryptor_for_channel_name(soft_node.node_id, &channel.name)
                .unwrap();

            let encrypted_data = cryptor
                .encrypt(packet_id, data.encode_to_vec())
                .await
                .unwrap();
            (
                channel_hash,
                mesh_packet::PayloadVariant::Encrypted(encrypted_data),
            )
        };

        let mesh_packet = meshtastic::MeshPacket {
            from: soft_node.node_id.into(),
            to: dest_node.into(),
            channel: channel_hash,
            id: packet_id,
            hop_limit: channel.hop_start.into(),
            priority: meshtastic::mesh_packet::Priority::Default.into(),
            hop_start: channel.hop_start.into(),
            payload_variant: Some(payload_variant),
            ..Default::default()
        };

        println!("send mesh: {:?}", mesh_packet);
        sqlite
            .insert_packet(
                &mesh_packet,
                Some(channel.name.clone()),
                Some(port_num),
                Some(&data.encode_to_vec()),
            )
            .unwrap();
        connection.send_mesh(mesh_packet).await.unwrap();

        let interval = publish_descriptor.interval();
        if !interval.is_zero() {
            schedule.add(Instant::now() + interval, (channel_idx, publish_idx));
        }
    }
}

async fn handle_network_event(
    sqlite: &sqlite::SQLite,
    keyring: &Keyring,
    result: Result<RecvData, Box<dyn std::error::Error>>,
) {
    match result {
        Ok(recv_data) => match recv_data {
            RecvData::MeshPacket(mesh_packet) => {
                println!("received mesh packet: {:?}", mesh_packet);
                println!();
                if let Some(ref payload_variant) = mesh_packet.payload_variant {
                    match payload_variant {
                        mesh_packet::PayloadVariant::Decoded(data) => {
                            sqlite
                                .insert_packet(
                                    &mesh_packet,
                                    None,
                                    Some(data.portnum()),
                                    Some(&data.encode_to_vec()),
                                )
                                .unwrap();
                        }
                        mesh_packet::PayloadVariant::Encrypted(encrypted_data) => {
                            if !match keyring.cryptor_for(
                                NodeId::from(mesh_packet.from),
                                NodeId::from(mesh_packet.to),
                                mesh_packet.channel,
                            ) {
                                Some(cryptor) => {
                                    match cryptor
                                        .decrypt(mesh_packet.id, encrypted_data.clone())
                                        .await
                                    {
                                        Ok(decrypted_data) => {
                                            match meshtastic::Data::decode(
                                                decrypted_data.as_slice(),
                                            ) {
                                                Ok(data) => {
                                                    sqlite
                                                        .insert_packet(
                                                            &mesh_packet,
                                                            Some(cryptor.to_string()),
                                                            Some(data.portnum()),
                                                            Some(&data.encode_to_vec()),
                                                        )
                                                        .unwrap();
                                                    true
                                                }
                                                Err(err) => {
                                                    println!("Failed to construct data: {}", err);
                                                    false
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            println!("Failed to decrypt data: {}", err);
                                            false
                                        }
                                    }
                                }
                                None => {
                                    println!("No cryptor found for packet: {:?}", mesh_packet);
                                    false
                                }
                            } {
                                sqlite
                                    .insert_packet(&mesh_packet, None, None, Some(encrypted_data))
                                    .unwrap();
                            };
                        }
                    }
                } else {
                    println!("No data received: {:?}", mesh_packet);
                    sqlite
                        .insert_packet(&mesh_packet, None, None, None)
                        .unwrap();
                }
            }
            RecvData::Unstructured(items) => tokio::io::stderr().write_all(&items).await.unwrap(),
        },
        Err(err) => {
            println!("handle error: {}", err);
            println!();
        }
    }
}

fn build_connection(soft_node: &SoftNodeConfig) -> Connection {
    match soft_node.transport {
        config::SoftNodeTransport::UDP(udp) => {
            let multicast_description = if let Some(multicast) = udp.join_multicast {
                let multicast_description = Multicast {
                    address: multicast.multicast,
                    interface: Interface {
                        if_addr: multicast.interface,
                        if_index: if_index_by_addr(&multicast.interface).unwrap(),
                    },
                };
                println!(
                    "Listen multicast on {} ({:?})",
                    udp.bind_address, multicast_description,
                );
                Some(multicast_description)
            } else {
                println!(
                    "Listen UDP on {} remote is {}",
                    udp.bind_address, udp.remote_address
                );
                None
            };

            Connection {
                stream_api_method: config::StreamAPIMethod::Direct,
                connection_type: ConnectionType::UDP(UDP::new(
                    udp.bind_address.into(),
                    udp.remote_address.into(),
                    multicast_description,
                )),
            }
        }
        config::SoftNodeTransport::TCP(ref tcp_config) => Connection {
            stream_api_method: tcp_config.stream_api_method,
            connection_type: ConnectionType::Stream(Stream::new(
                transport::stream::StreamAddress::TCPSocket(tcp_config.address),
                Duration::from_secs(10),
            )),
        },
        config::SoftNodeTransport::Serial(ref serial_config) => Connection {
            stream_api_method: serial_config.stream_api_method,
            connection_type: ConnectionType::Stream(Stream::new(
                transport::stream::StreamAddress::Serial(Serial {
                    tty: serial_config.port.clone(),
                    baudrate: serial_config.baudrate,
                }),
                Duration::from_secs(10),
            )),
        },
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config = load_config(&args).unwrap_or_else(|| {
        println!("Config file not loaded: try type `--help` to get help");
        process::exit(1)
    });

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
    let soft_node = config.soft_node;
    let mut connection = build_connection(&soft_node);
    let mut schedule = Schedule::new(&soft_node.channels);
    let sqlite_name = format!("journal-{:x}.sqlite", soft_node.node_id);
    let sqlite = sqlite::SQLite::new(sqlite_name.as_str()).unwrap();

    connection.connect().await.unwrap();

    loop {
        let next_wakeup = schedule.next_wakeup().unwrap_or_else(|| {
            Instant::now() + Duration::from_secs(60 * 60 * 24) // 1 day
        });

        tokio::select! {
            _ = sleep_until(next_wakeup) => {
                handle_timer_event(&sqlite, &mut schedule, &soft_node, &keyring, &mut connection).await;
            },
            result = connection.recv_mesh() => {
                handle_network_event(&sqlite, &keyring, result).await;
            }
        }
    }
}
