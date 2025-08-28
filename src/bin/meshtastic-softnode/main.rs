mod config;
mod publish;

use clap::Parser;
use meshtastic_connect::{
    keyring::{Keyring, cryptor::Encrypt, node_id::NodeId},
    meshtastic::{self, ServiceEnvelope, mesh_packet},
    transport::{
        self, if_index_by_addr,
        stream::Stream,
        udp::{Interface, Multicast, UDP},
    },
};
use publish::Publishable;
use rand::Rng;
use tokio::io::AsyncWriteExt;

use crate::config::Args;
use crate::config::load_config;
use prost::Message;
use std::{any::type_name_of_val, process, time::Duration};

enum Connection {
    Multicast(UDP),
    Stream(Stream),
}

enum RecvData {
    MeshPacket(meshtastic::MeshPacket),
    Unstructured(Vec<u8>),
}

impl Connection {
    async fn connect(&mut self) -> Result<(), std::io::Error> {
        match self {
            Connection::Multicast(multicast) => multicast.connect().await,
            Connection::Stream(stream) => stream.connect().await,
        }
    }

    async fn disconnect(&mut self) {
        match self {
            Connection::Multicast(multicast) => multicast.disconnect().await,
            Connection::Stream(stream) => stream.disconnect().await,
        }
    }

    async fn send_mesh(
        &mut self,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        match self {
            Connection::Multicast(multicast) => multicast.send(mesh_packet).await,
            Connection::Stream(stream) => {
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
        }
    }

    async fn recv_mesh(&mut self) -> Result<RecvData, Box<dyn std::error::Error>> {
        match self {
            Connection::Multicast(multicast) => {
                let (mesh_packet, _) = multicast.recv().await?;
                Ok(RecvData::MeshPacket(mesh_packet))
            }
            Connection::Stream(stream) => match stream.recv().await? {
                transport::stream::StreamData::Packet(from_radio) => {
                    if let Some(payload_variant) = from_radio.payload_variant {
                        match payload_variant {
                            meshtastic::from_radio::PayloadVariant::Packet(mesh_packet) => {
                                // Ok(RecvData::MeshPacket(mesh_packet))
                                Ok(RecvData::Unstructured(
                                    format!("Receive transport's mesh packet").into(),
                                ))
                            }
                            meshtastic::from_radio::PayloadVariant::MqttClientProxyMessage(
                                mqtt_proxy_msg,
                            ) => {
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
                                    Ok(RecvData::Unstructured("MQTT proto: no payload data".into()))
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
        if peer.highlight {
            filter_by_nodeid.push(peer.node_id);
        }
    }

    println!();
    let soft_node = config.soft_node;
    let mut connection = match soft_node.transport {
        config::SoftNodeTransport::UDP(udp) => {
            if let Some(multicast) = udp.join_multicast {
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
                Connection::Multicast(UDP::new(
                    udp.bind_address.into(),
                    udp.remote_address.into(),
                    Some(multicast_description),
                ))
            } else {
                println!(
                    "Listen UDP on {} remote is {}",
                    udp.bind_address, udp.remote_address
                );
                Connection::Multicast(UDP::new(
                    udp.bind_address.into(),
                    udp.remote_address.into(),
                    None,
                ))
            }
        }
        config::SoftNodeTransport::TCP(stream_address) => Connection::Stream(Stream::new(
            transport::stream::StreamAddress::TCPSocket(stream_address),
            Duration::from_secs(10),
        )),
        config::SoftNodeTransport::Serial(ref serial) => Connection::Stream(Stream::new(
            transport::stream::StreamAddress::Serial(serial.clone()),
            Duration::from_secs(10),
        )),
    };

    connection.connect().await.unwrap();

    for channel in &soft_node.channels {
        let (cryptor, channel_hash) = keyring
            .cryptor_for_channel_name(soft_node.node_id, &channel.name)
            .unwrap();

        for publish_descriptor in &channel.publish {
            println!("Publishing {}", type_name_of_val(publish_descriptor));
            let (port_num, data_payload) = publish_descriptor.pack_to_data(&soft_node);
            let packet_id = rand::rng().random();
            let dest_node: u32 = 0xffffffff;
            let data = cryptor
                .encrypt(
                    packet_id,
                    meshtastic::Data {
                        portnum: port_num.into(),
                        payload: data_payload,
                        ..Default::default()
                    }
                    .encode_to_vec(),
                )
                .await
                .unwrap();

            let mesh_packet = meshtastic::MeshPacket {
                from: soft_node.node_id.into(),
                to: dest_node.into(),
                channel: channel_hash,
                id: packet_id,
                hop_limit: channel.hop_start.into(),
                priority: meshtastic::mesh_packet::Priority::Default.into(),
                hop_start: channel.hop_start.into(),
                public_key: vec![],
                payload_variant: Some(mesh_packet::PayloadVariant::Encrypted(data)),
                ..Default::default()
            };

            println!("send mesh: {:?}", mesh_packet);
            connection.send_mesh(mesh_packet).await.unwrap();
        }
    }

    loop {
        match connection.recv_mesh().await {
            Ok(recv_data) => match recv_data {
                RecvData::MeshPacket(mesh_packet) => {
                    println!("received mesh packet: {:?}", mesh_packet);
                    println!();
                }
                RecvData::Unstructured(items) => {
                    tokio::io::stderr().write_all(&items).await.unwrap()
                }
            },
            Err(err) => {
                println!("handle error: {}", err);
                println!();
            }
        }

        // print_mesh_packet(mesh_packet, &keyring, &filter_by_nodeid).await;
    }
}
