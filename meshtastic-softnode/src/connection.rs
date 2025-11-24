use crate::{SoftNodeConfig, config};
use futures::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use meshtastic_connect::{
    keyring::node_id::NodeId,
    meshtastic::{self, to_radio},
    transport::{
        if_index_by_addr, mqtt, mqtt_stream,
        stream::{self, codec::BytesSequence},
        udp,
    },
};
use std::process::exit;

pub enum Sender {
    UDP(SplitSink<udp::Udp, meshtastic::MeshPacket>),
    Stream(SplitSink<mqtt_stream::MqttStream, mqtt_stream::MqttStreamSendData>),
    MQTT(mqtt::MqttSender),
}

pub enum Receiver {
    UDP(SplitStream<udp::Udp>),
    Stream(SplitStream<mqtt_stream::MqttStream>),
    MQTT(mqtt::MqttReceiver),
}

pub enum DataVariant {
    MeshPacket(meshtastic::MeshPacket),
    Unstructured(Vec<u8>),
}

pub struct Incoming {
    pub channel_id: Option<mqtt::ChannelId>,
    pub gateway_id: Option<NodeId>,
    pub data: DataVariant,
}

type SendData = (mqtt::ChannelId, meshtastic::MeshPacket);

impl Sender {
    pub async fn send(&mut self, send_data: SendData) -> Result<(), std::io::Error> {
        let (channel_id, mesh_packet) = send_data;
        match self {
            Sender::UDP(udp) => {
                println!("UDP: Sending...");
                udp.send(mesh_packet).await
            }
            Sender::Stream(stream) => {
                println!("STREAM MQTT: Sending to {}...", channel_id);
                stream
                    .send(mqtt_stream::MqttStreamSendData::MeshPacket(
                        channel_id,
                        mesh_packet,
                    ))
                    .await
            }
            Sender::MQTT(mqtt) => {
                println!("MQTT: Sending to {}...", channel_id);
                mqtt.send((channel_id, mesh_packet)).await
            }
        }
    }
}

async fn udp_next(udp: &mut SplitStream<udp::Udp>) -> Result<Incoming, std::io::Error> {
    let (mesh_packet, _) = udp.next().await.ok_or(std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "UDP connection lost",
    ))??;

    Ok(Incoming {
        channel_id: None,
        gateway_id: None,
        data: DataVariant::MeshPacket(mesh_packet),
    })
}

async fn stream_next(
    // Need to add struct StreamContext to store: nodeid from `FromRadio(MyNodeInfo)` message
    stream_connection: &mut SplitStream<mqtt_stream::MqttStream>,
) -> Result<Incoming, std::io::Error> {
    let recv_data = stream_connection.next().await.ok_or(std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "Stream connection lost",
    ))??;

    let incoming = match recv_data {
        mqtt_stream::MqttStreamRecvData::MeshPacket(packet_id, mesh_packet) => {
            let message = format!(
                "\nStreamAPI: Receive transport's MeshPacket: [{}] {:?}\n",
                packet_id, mesh_packet
            );

            Incoming {
                channel_id: None,
                gateway_id: None,
                data: DataVariant::Unstructured(message.into()),
            }
        }
        mqtt_stream::MqttStreamRecvData::MQTTMeshPacket(
            _packet_id,
            mesh_packet,
            channel_id,
            gateway_id,
        ) => Incoming {
            channel_id: Some(channel_id),
            gateway_id: Some(gateway_id),
            data: DataVariant::MeshPacket(mesh_packet),
        },
        mqtt_stream::MqttStreamRecvData::FromRadio(_, from_radio) => {
            let message = format!(
                "\nStreamAPI: Receive transport's radio packet: {:?}\n",
                from_radio
            );

            Incoming {
                channel_id: None,
                // TODO: put stream's node id
                gateway_id: None,
                data: DataVariant::Unstructured(message.into()),
            }
        }

        mqtt_stream::MqttStreamRecvData::Unstructured(bytes_mut) => Incoming {
            channel_id: None,
            // TODO: put stream's node id
            gateway_id: None,
            data: DataVariant::Unstructured(bytes_mut.to_vec()),
        },
    };
    Ok(incoming)
}

async fn mqtt_next(mqtt: &mut mqtt::MqttReceiver) -> Result<Incoming, std::io::Error> {
    let (mesh_packet, channel_id, gateway_id) = mqtt.next().await?;

    Ok(Incoming {
        channel_id: Some(channel_id),
        gateway_id: Some(gateway_id),
        data: DataVariant::MeshPacket(mesh_packet),
    })
}

impl Receiver {
    pub async fn next(&mut self) -> Result<Incoming, std::io::Error> {
        match self {
            Receiver::UDP(udp) => udp_next(udp).await,
            Receiver::Stream(stream_connection) => stream_next(stream_connection).await,
            Receiver::MQTT(mqtt) => mqtt_next(mqtt).await,
        }
    }
}

pub struct Heartbeat {
    interval: tokio::time::Interval,
}

impl Heartbeat {
    pub async fn next(&mut self) {
        let _ = self.interval.tick().await;
    }

    pub async fn send(&self, sender: &mut Sender) -> Result<(), std::io::Error> {
        if let Sender::Stream(split_sink) = sender {
            split_sink
                .send(mqtt_stream::MqttStreamSendData::ToRadio(
                    to_radio::PayloadVariant::Heartbeat(meshtastic::Heartbeat {}),
                ))
                .await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Heartbeat can only be sent over a `Stream` sender",
            ))
        }
    }
}

pub async fn build(
    transport_config: config::SoftNodeTransport,
    soft_node: &SoftNodeConfig,
) -> (Sender, Receiver, Option<Heartbeat>) {
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

            let udp = udp::UdpBuilder::new(
                udp.bind_address.into(),
                udp.remote_address.into(),
                multicast_description,
            );
            let udp = udp.connect().await.unwrap();
            let (sender, receiver) = udp.split();

            (Sender::UDP(sender), Receiver::UDP(receiver), None)
        }
        config::SoftNodeVariant::TCP(ref tcp_config) => {
            println!("Connect TCP to {}", tcp_config.address);

            let mut connection = stream::tcp::TcpBuilder::new(tcp_config.address)
                .connect()
                .await
                .inspect_err(|e| {
                    println!("TCP connect failed: {e}");
                    exit(1);
                })
                .unwrap();

            connection.send(BytesSequence::Wakeup).await.unwrap();
            connection
                .send(to_radio::PayloadVariant::WantConfigId(0))
                .await
                .unwrap();

            let connection =
                build_mqtt_stream_for_method(soft_node, connection, &tcp_config.method);

            let (sender, receiver) = connection.split();
            let heartbeat = if tcp_config.heartbeat_interval.is_zero() {
                None
            } else {
                Some(Heartbeat {
                    interval: tokio::time::interval_at(
                        tokio::time::Instant::now() + tcp_config.heartbeat_interval.into(),
                        tcp_config.heartbeat_interval.into(),
                    ),
                })
            };

            (
                Sender::Stream(sender),
                Receiver::Stream(receiver),
                heartbeat,
            )
        }
        config::SoftNodeVariant::SERIAL(ref serial_config) => {
            println!(
                "Connect SERIAL to {} baudrate {}",
                serial_config.port, serial_config.baudrate
            );

            let mut connection = stream::serial::SerialBuilder::new(
                serial_config.port.clone(),
                serial_config.baudrate,
            )
            .connect()
            .await
            .unwrap();

            connection.send(BytesSequence::Wakeup).await.unwrap();
            connection
                .send(to_radio::PayloadVariant::WantConfigId(0))
                .await
                .unwrap();

            let connection =
                build_mqtt_stream_for_method(soft_node, connection, &serial_config.method);

            let (sender, receiver) = connection.split();
            let heartbeat = if serial_config.heartbeat_interval.is_zero() {
                None
            } else {
                Some(Heartbeat {
                    interval: tokio::time::interval_at(
                        tokio::time::Instant::now() + serial_config.heartbeat_interval.into(),
                        serial_config.heartbeat_interval.into(),
                    ),
                })
            };

            (
                Sender::Stream(sender),
                Receiver::Stream(receiver),
                heartbeat,
            )
        }
        config::SoftNodeVariant::MQTT(mqttconfig) => {
            println!(
                "Connect MQTT to {}@{} {:?}",
                mqttconfig.username, mqttconfig.server, mqttconfig.topic
            );

            let mqtt = mqtt::MqttBuilder::new(
                mqttconfig.server,
                mqttconfig.username.clone(),
                mqttconfig.password.clone(),
                soft_node.node_id,
                mqttconfig.topic.clone(),
            );

            let connection = mqtt
                .connect()
                .await
                .inspect_err(|e| {
                    println!("MQTT connect failed: {e}");
                    exit(1);
                })
                .unwrap();
            let (sender, receiver) = connection.split();

            (Sender::MQTT(sender), Receiver::MQTT(receiver), None)
        }
    }
}

fn build_mqtt_stream_for_method(
    soft_node: &SoftNodeConfig,
    stream: stream::Stream,
    method: &config::StreamMethod,
) -> mqtt_stream::MqttStream {
    match method {
        config::StreamMethod::AUTO => todo!(),
        config::StreamMethod::Direct => todo!(),
        config::StreamMethod::FORCE(topic) => {
            mqtt_stream::MqttStream::new(stream, soft_node.node_id, topic.clone())
        }
    }
}
