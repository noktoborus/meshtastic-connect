mod config;

use clap::Parser;
use config::MQTTConfig;
use meshtastic_connect::{
    keyring::{
        Keyring,
        cryptor::{Decrypt, Encrypt},
        node_id::NodeId,
    },
    meshtastic::{self, ServiceEnvelope, mesh_packet},
    transport::{self, if_index_by_addr, multicast::Multicast, stream::Stream},
};
use rand::Rng;
use tokio::io::AsyncWriteExt;

use crate::config::Args;
use crate::config::load_config;
use prost::Message;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Publish, QoS};
use std::{process, time::Duration};

enum Connection {
    Multicast(Multicast),
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
        config::SoftNodeTransport::Multicast(multicast_bind) => {
            let if_index = if_index_by_addr(&multicast_bind.interface).unwrap();
            println!(
                "Listen multicast on {} (if {} index {})",
                multicast_bind.address, multicast_bind.interface, if_index,
            );
            Connection::Multicast(Multicast::new(multicast_bind.address.into(), if_index))
        }
        config::SoftNodeTransport::TCP(stream_address) => Connection::Stream(Stream::new(
            transport::stream::StreamAddress::TCPSocket(stream_address),
            Duration::from_secs(10),
        )),
        config::SoftNodeTransport::Serial(serial) => Connection::Stream(Stream::new(
            transport::stream::StreamAddress::Serial(serial),
            Duration::from_secs(10),
        )),
    };

    connection.connect().await.unwrap();

    for channel in &soft_node.channels {
        if channel.node_info.is_some() {
            println!("Send initial nodeinfo to {}", channel.name);
            let dest_node: NodeId = 0xffffffff.into();
            let packet_id = rand::rng().random();
            let node_info = meshtastic::User {
                id: soft_node.node_id.into(),
                long_name: soft_node.name.clone(),
                short_name: soft_node.short_name.clone(),
                hw_model: meshtastic::HardwareModel::AndroidSim.into(),
                is_licensed: false,
                role: meshtastic::config::device_config::Role::Client.into(),
                public_key: vec![],
                ..Default::default()
            };
            let data = meshtastic::Data {
                portnum: meshtastic::PortNum::NodeinfoApp.into(),
                payload: node_info.encode_to_vec(),
                ..Default::default()
            };
            let (cryptor, channel_hash) = keyring
                .cryptor_for_channel_name(soft_node.node_id, &channel.name)
                .unwrap();
            let data = cryptor
                .encrypt(packet_id, data.encode_to_vec())
                .await
                .unwrap();

            let mesh_packet = meshtastic::MeshPacket {
                from: soft_node.node_id.into(),
                to: dest_node.into(),
                channel: channel_hash,
                id: packet_id,
                rx_time: 1755713559,
                rx_snr: 3.0,
                hop_limit: channel.hop_start.into(),
                want_ack: true,
                priority: meshtastic::mesh_packet::Priority::Default.into(),
                rx_rssi: 0,
                via_mqtt: false,
                hop_start: channel.hop_start.into(),
                public_key: vec![],
                pki_encrypted: false,
                next_hop: 0,
                relay_node: 0,
                tx_after: 0,
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

async fn mesh_packet_get_portnum(
    mesh_packet: meshtastic::MeshPacket,
    keyring: &Keyring,
) -> Option<meshtastic::PortNum> {
    if let Some(payload_variant) = mesh_packet.payload_variant {
        match payload_variant {
            meshtastic::mesh_packet::PayloadVariant::Decoded(data) => Some(data.portnum()),
            meshtastic::mesh_packet::PayloadVariant::Encrypted(items) => {
                let from = mesh_packet.from.into();
                let to = mesh_packet.to.into();
                if let Some(decryptor) = keyring.cryptor_for(from, to, mesh_packet.channel) {
                    match decryptor.decrypt(mesh_packet.id, items).await {
                        Ok(buffer) => {
                            if let Ok(data) = meshtastic::Data::decode(buffer.as_slice()) {
                                Some(data.portnum())
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
        }
    } else {
        None
    }
}

async fn process_service_envelope(
    publish: Publish,
    keyring: &Keyring,
    opposite_client: &(AsyncClient, MQTTConfig),
) {
    if let Ok(service) = meshtastic::ServiceEnvelope::decode(publish.payload.clone()) {
        if let Some(mesh_packet) = service.packet {
            let client = &opposite_client.0;
            let from = NodeId::from(mesh_packet.from);
            let to = NodeId::from(mesh_packet.to);
            let mqtt = &opposite_client.1;
            let pass = (mqtt.filter.to.is_empty() || mqtt.filter.to.contains(&to))
                && (mqtt.filter.from.is_empty() || mqtt.filter.from.contains(&from));

            let portnum = mesh_packet_get_portnum(mesh_packet.clone(), keyring).await;

            if pass {
                println!(
                    "[{}:{}] xfer ({} gw {}) {} -> {} {:?}",
                    mqtt.server_addr,
                    mqtt.server_port,
                    publish.topic,
                    service.gateway_id,
                    from,
                    to,
                    portnum.ok_or("<undecrypted>")
                );

                // if mesh_packet.hop_limit == 0 {
                //     mesh_packet.hop_limit = 1
                // }

                let new_service = ServiceEnvelope {
                    packet: Some(mesh_packet),
                    channel_id: service.channel_id,
                    gateway_id: service.gateway_id,
                };

                client
                    .publish(
                        publish.topic,
                        publish.qos,
                        publish.retain,
                        new_service.encode_to_vec(),
                    )
                    .await
                    .unwrap();
            } else {
                println!(
                    "[{}:{}] ignore ({} gw {}) {} -> {} hops: {}/{} decrypt: {:?}",
                    mqtt.server_addr,
                    mqtt.server_port,
                    publish.topic,
                    service.gateway_id,
                    from,
                    to,
                    mesh_packet.hop_limit,
                    mesh_packet.hop_start,
                    portnum.ok_or("<undecrypted>")
                );
            }
        }
    }
}

async fn handle_notifications(
    notification: Event,
    keyring: &Keyring,
    opposite_client: &(AsyncClient, MQTTConfig),
) {
    if let rumqttc::Event::Incoming(packet) = notification {
        match packet {
            rumqttc::Packet::Publish(publish) => {
                process_service_envelope(publish, &keyring, opposite_client).await;
            }
            rumqttc::Packet::PingReq => {}
            rumqttc::Packet::PingResp => {}
            _ => {}
        }
    }
}

async fn build_connection(mqtt: MQTTConfig) -> ((AsyncClient, MQTTConfig), EventLoop) {
    println!(
        "Connect to MQTT {} port {}: {:?}",
        mqtt.server_addr, mqtt.server_port, mqtt.subscribe
    );

    let mut mqttoptions = MqttOptions::new(
        "rumqtt-async",
        mqtt.server_addr.clone(),
        mqtt.server_port.clone(),
    );
    mqttoptions.set_keep_alive(Duration::from_secs(5));
    mqttoptions.set_credentials(mqtt.username.clone(), mqtt.password.clone());

    let (client, connection) = AsyncClient::new(mqttoptions, 10);
    for topic in &mqtt.subscribe {
        client.subscribe(topic, QoS::AtMostOnce).await.unwrap();
    }

    ((client, mqtt), connection)
}
