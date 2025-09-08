use crate::{SoftNodeConfig, config};
use meshtastic_connect::{
    keyring::node_id::NodeId,
    meshtastic::{self, ServiceEnvelope},
    transport::{if_index_by_addr, mqtt, mqtt_stream, stream, udp},
};
use prost::Message;
use std::time::Duration;

pub struct StreamConnection {
    method: config::StreamMethod,
    stream: mqtt_stream::MQTTStream,
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
            Connection::Stream(stream) => stream.stream.stream().connect().await,
            Connection::MQTT(mqtt) => mqtt.connect().await,
        }
    }

    pub async fn disconnect(&mut self) {
        match self {
            Connection::UDP(multicast) => multicast.disconnect().await,
            Connection::Stream(stream) => stream.stream.stream().disconnect().await,
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
            Connection::Stream(stream) => {
                if let Some(channel_id) = channel_id {
                    println!("STREAM MQTT: Sending to {}...", channel_id);
                    stream.stream.send(channel_id, mesh_packet).await
                } else {
                    println!("STREAM MQTT SKIP: No channel ID provided");
                    Ok(())
                }
            }
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
                mqtt_stream::MQTTStreamData::MeshPacket(_packet_id, _mesh_packet) => Ok(Incoming {
                    channel_id: None,
                    data: DataVariant::Unstructured(
                        format!(
                            "StreamAPI({:?}): Receive transport's mesh packet",
                            stream.method
                        )
                        .into(),
                    ),
                }),
                mqtt_stream::MQTTStreamData::MQTTMeshPacket(
                    _packet_id,
                    mesh_packet,
                    channel_id,
                    _gateway_id,
                ) => Ok(Incoming {
                    channel_id: Some(channel_id),
                    data: DataVariant::MeshPacket(mesh_packet),
                }),
                mqtt_stream::MQTTStreamData::FromRadio(_, _) => Ok(Incoming {
                    channel_id: None,
                    data: DataVariant::Unstructured(
                        format!(
                            "StreamAPI({:?}): Receive transport's radio packet",
                            stream.method
                        )
                        .into(),
                    ),
                }),
                mqtt_stream::MQTTStreamData::Unstructured(bytes_mut) => Ok(Incoming {
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

            let connection = stream::Stream::new(
                stream::StreamAddress::TCPSocket(tcp_config.address),
                Duration::from_secs(10),
            );

            let connection =
                build_mqtt_stream_for_method(soft_node, connection, &tcp_config.method);

            Connection::Stream(StreamConnection {
                method: tcp_config.method.clone(),
                stream: connection,
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
            let connection = stream::Stream::new(
                stream::StreamAddress::Serial(serial),
                Duration::from_secs(10),
            );
            let connection =
                build_mqtt_stream_for_method(soft_node, connection, &serial_config.method);

            Connection::Stream(StreamConnection {
                method: serial_config.method.clone(),
                stream: connection,
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

fn build_mqtt_stream_for_method(
    soft_node: &SoftNodeConfig,
    stream: stream::Stream,
    method: &config::StreamMethod,
) -> mqtt_stream::MQTTStream {
    match method {
        config::StreamMethod::AUTO => todo!(),
        config::StreamMethod::Direct => todo!(),
        config::StreamMethod::FORCE(topic) => {
            mqtt_stream::MQTTStream::new(stream, soft_node.node_id, topic.clone())
        }
    }
}
