use crate::{SoftNodeConfig, config};
use meshtastic_connect::{
    keyring::node_id::NodeId,
    meshtastic::{self, ServiceEnvelope},
    transport::{self, if_index_by_addr, mqtt, stream, udp},
};
use prost::Message;
use std::time::Duration;

pub struct StreamConnection {
    method: config::StreamMethod,
    stream: stream::Stream,
}

pub enum Connection {
    UDP(udp::UDP),
    Stream(StreamConnection),
    MQTT(mqtt::MQTT),
}

pub enum DataVariant {
    MeshPacket(meshtastic::MeshPacket),
    Unstructured(Vec<u8>),
}

pub struct Incoming {
    pub channel_id: Option<mqtt::ChannelId>,
    pub data: DataVariant,
}

impl Connection {
    pub async fn connect(&mut self) -> Result<(), std::io::Error> {
        match self {
            Connection::UDP(multicast) => multicast.connect().await,
            Connection::Stream(stream) => stream.stream.connect().await,
            Connection::MQTT(mqtt) => mqtt.connect().await,
        }
    }

    pub async fn disconnect(&mut self) {
        match self {
            Connection::UDP(multicast) => multicast.disconnect().await,
            Connection::Stream(stream) => stream.stream.disconnect().await,
            Connection::MQTT(mqtt) => mqtt.disconnect().await,
        }
    }

    pub async fn send_mesh(
        &mut self,
        channel_id: Option<mqtt::ChannelId>,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        match self {
            Connection::UDP(multicast) => {
                println!("UDP: Sending...");
                multicast.send(mesh_packet).await
            }
            Connection::Stream(stream) => match stream.method {
                config::StreamMethod::Direct => {
                    // let to_radio = meshtastic::to_radio::PayloadVariant::Packet(mesh_packet);
                    println!("STREAM DIRECT: Sending...");
                    // TODO: drop if MyInfo.NodeId != mesh_packet.from else there may be unforeseen consequences
                    todo!()
                    // stream.stream.send(to_radio).await
                }
                config::StreamMethod::AUTO => {
                    // Direct or MQTT
                    todo!()
                }
                config::StreamMethod::FORCE(ref topic) => {
                    if let Some(channel_id) = channel_id {
                        println!("STREAM MQTT: Sending to {}...", channel_id);
                        let gateway_id = NodeId::from(mesh_packet.id);
                        let topic = format!("{}/2/e/{}/{}", topic, channel_id, gateway_id);
                        let service_envelope = ServiceEnvelope {
                            packet: Some(mesh_packet),
                            channel_id,
                            gateway_id: gateway_id.into(),
                        };
                        let mqtt_proxy = meshtastic::MqttClientProxyMessage {
                            topic: topic.into(),
                            retained: false,
                            payload_variant: Some(
                                meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(
                                    service_envelope.encode_to_vec(),
                                ),
                            ),
                        };
                        let to_radio = meshtastic::to_radio::PayloadVariant::MqttClientProxyMessage(
                            mqtt_proxy,
                        );
                        stream.stream.send(to_radio).await
                    } else {
                        println!("STREAM MQTT SKIP: No channel ID provided");
                        Ok(())
                    }
                }
            },
            Connection::MQTT(mqtt) => {
                if let Some(channel_id) = channel_id {
                    println!("MQTT: Sending to {}...", channel_id);
                    mqtt.send(channel_id, mesh_packet).await
                } else {
                    println!("MQTT SKIP: No channel ID provided");
                    Ok(())
                }
            }
        }
    }

    pub async fn recv_mesh(&mut self) -> Result<Incoming, std::io::Error> {
        match self {
            Connection::UDP(multicast) => {
                let (mesh_packet, _) = multicast.recv().await?;
                Ok(Incoming {
                    channel_id: None,
                    data: DataVariant::MeshPacket(mesh_packet),
                })
            }
            Connection::Stream(stream) => match stream.stream.recv().await? {
                transport::stream::StreamData::FromRadio(from_radio) => {
                    if let Some(payload_variant) = from_radio.payload_variant {
                        match payload_variant {
                            meshtastic::from_radio::PayloadVariant::Packet(mesh_packet) => {
                                if stream.method != config::StreamMethod::Direct {
                                    Ok(Incoming {
                                        channel_id: None,
                                        data: DataVariant::Unstructured(
                                            format!(
                                                "StreamAPI({:?}): Receive transport's mesh packet",
                                                stream.method
                                            )
                                            .into(),
                                        ),
                                    })
                                } else {
                                    Ok(Incoming {
                                        channel_id: None,
                                        data: DataVariant::MeshPacket(mesh_packet),
                                    })
                                }
                            }
                            meshtastic::from_radio::PayloadVariant::MqttClientProxyMessage(
                                mqtt_proxy_msg,
                            ) => {
                                match stream.method {
                                    config::StreamMethod::AUTO => todo!(),
                                    config::StreamMethod::Direct => Ok(Incoming {
                                        channel_id: None,
                                        data: DataVariant::Unstructured(
                                            format!(
                                                "StreamAPI({:?}): Ignoring MQTT proxy message",
                                                stream.method
                                            )
                                            .into(),
                                        ),
                                    }),
                                    config::StreamMethod::FORCE(_) => {
                                        if let Some(payload_variant) =
                                            mqtt_proxy_msg.payload_variant
                                        {
                                            match payload_variant {
                                                meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(items) => {
                                                    match meshtastic::ServiceEnvelope::decode(items.as_slice()) {
                                                        Ok(service_envelope) => {
                                                            if let Some(mesh_packet) = service_envelope.packet {
                                                                Ok(Incoming{ channel_id: Some(service_envelope.channel_id), data: DataVariant::MeshPacket(mesh_packet)})
                                                            } else {
                                                                Ok(Incoming{ channel_id: Some(service_envelope.channel_id), data: DataVariant::Unstructured(format!("MQTT ServiceEnvelope: no Packet").into())})
                                                            }
                                                        },
                                                        // TODO: map err as err, not as ok
                                                        Err(e) =>  Ok(Incoming{ channel_id: None, data: DataVariant::Unstructured(format!("MQTT ServiceEnvelope::decode: {e}").into())}),
                                                    }


                                                },
                                                meshtastic::mqtt_client_proxy_message::PayloadVariant::Text(text) => {
                                                    Ok(Incoming {channel_id: None, data: DataVariant::Unstructured(format!("MQTT proto: got text: {:?}", text).into())})
                                                },
                                            }
                                        } else {
                                            Ok(Incoming {
                                                channel_id: None,
                                                data: DataVariant::Unstructured(
                                                    "MQTT proto: no payload data".into(),
                                                ),
                                            })
                                        }
                                    }
                                }
                            }
                            _ => Ok(Incoming {
                                channel_id: None,
                                data: DataVariant::Unstructured("got not mesh packet".into()),
                            }),
                        }
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("No payload data in FromRadio message"),
                        ))
                    }
                }
                transport::stream::StreamData::Unstructured(bytes_mut) => Ok(Incoming {
                    channel_id: None,
                    data: DataVariant::Unstructured(bytes_mut.to_vec()),
                }),
            },
            Connection::MQTT(mqtt) => {
                let (mesh_packet_or_not, channel_id, node_id) = mqtt.recv().await?;

                if let Some(mesh_packet) = mesh_packet_or_not {
                    Ok(Incoming {
                        channel_id: Some(channel_id),
                        data: DataVariant::MeshPacket(mesh_packet),
                    })
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!(
                            "No MeshPacket from mqtt channel {:?} (node_id: {:?})",
                            channel_id, node_id
                        ),
                    ))
                }
            }
        }
    }
}

pub fn build(
    transport_config: config::SoftNodeTransport,
    soft_node: &SoftNodeConfig,
) -> Connection {
    match transport_config.variant {
        config::SoftNodeVariant::UDP(udp) => {
            let multicast_description = if let Some(multicast) = udp.join_multicast {
                let multicast_description = udp::Multicast {
                    address: multicast.multicast,
                    interface: udp::Interface {
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

            let udp = udp::UDP::new(
                udp.bind_address.into(),
                udp.remote_address.into(),
                multicast_description,
            );

            Connection::UDP(udp)
        }
        config::SoftNodeVariant::TCP(ref tcp_config) => {
            println!("Connect TCP to {}", tcp_config.address);

            Connection::Stream(StreamConnection {
                method: tcp_config.method.clone(),
                stream: stream::Stream::new(
                    transport::stream::StreamAddress::TCPSocket(tcp_config.address),
                    Duration::from_secs(10),
                ),
            })
        }
        config::SoftNodeVariant::SERIAL(ref serial_config) => {
            println!(
                "Connect SERIAL to {} baudrate {}",
                serial_config.port, serial_config.baudrate
            );

            let serial = stream::Serial {
                tty: serial_config.port.clone(),
                baudrate: serial_config.baudrate,
            };

            Connection::Stream(StreamConnection {
                method: serial_config.method.clone(),
                stream: stream::Stream::new(
                    transport::stream::StreamAddress::Serial(serial),
                    Duration::from_secs(10),
                ),
            })
        }
        config::SoftNodeVariant::MQTT(mqttconfig) => {
            println!(
                "Connect MQTT to {}@{} {}",
                mqttconfig.username, mqttconfig.server, mqttconfig.topic
            );

            let mqtt = mqtt::MQTT::new(
                mqttconfig.server,
                mqttconfig.username.clone(),
                mqttconfig.password.clone(),
                soft_node.node_id,
                mqttconfig.topic.clone(),
            );

            Connection::MQTT(mqtt)
        }
    }
}
