use crate::config;
use meshtastic_connect::{
    keyring::node_id::NodeId,
    meshtastic::{self, ServiceEnvelope},
    transport::{
        self, if_index_by_addr,
        stream::{Serial, Stream},
        udp::{Interface, Multicast, UDP},
    },
};
use prost::Message;
use std::time::Duration;

enum ConnectionType {
    UDP(UDP),
    Stream(Stream),
}

pub struct Connection {
    stream_api_method: config::StreamAPIMethod,
    connection_type: ConnectionType,
}

pub enum RecvData {
    MeshPacket(meshtastic::MeshPacket),
    Unstructured(Vec<u8>),
}

pub trait ConnectionAPI {
    async fn connect(&mut self) -> Result<(), std::io::Error>;
    async fn disconnect(&mut self);
    async fn send_mesh(
        &mut self,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error>;
    async fn recv_mesh(&mut self) -> Result<RecvData, std::io::Error>;
}

impl ConnectionAPI for Connection {
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

    async fn recv_mesh(&mut self) -> Result<RecvData, std::io::Error> {
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
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("No payload data in FromRadio message"),
                        ))
                    }
                }
                transport::stream::StreamData::Unstructured(bytes_mut) => {
                    Ok(RecvData::Unstructured(bytes_mut.to_vec()))
                }
            },
        }
    }
}

pub fn build(transport_config: config::SoftNodeTransport) -> Connection {
    match transport_config {
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

            let udp = UDP::new(
                udp.bind_address.into(),
                udp.remote_address.into(),
                multicast_description,
            );

            Connection {
                stream_api_method: config::StreamAPIMethod::Direct,
                connection_type: ConnectionType::UDP(udp),
            }
        }
        config::SoftNodeTransport::TCP(ref tcp_config) => Connection {
            stream_api_method: tcp_config.stream_api_method,
            connection_type: ConnectionType::Stream(Stream::new(
                transport::stream::StreamAddress::TCPSocket(tcp_config.address),
                Duration::from_secs(10),
            )),
        },
        config::SoftNodeTransport::Serial(ref serial_config) => {
            let serial = Serial {
                tty: serial_config.port.clone(),
                baudrate: serial_config.baudrate,
            };

            Connection {
                stream_api_method: serial_config.stream_api_method,
                connection_type: ConnectionType::Stream(Stream::new(
                    transport::stream::StreamAddress::Serial(serial),
                    Duration::from_secs(10),
                )),
            }
        }
    }
}
