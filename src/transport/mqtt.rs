use crate::{keyring::node_id::NodeId, meshtastic};
use prost::Message;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use std::{net::SocketAddr, time::Duration};

// Root topic
pub type Topic = String;

// Channel identifier (name)
pub type ChannelId = String;

pub struct MqttMeta {
    gateway: NodeId,
    root_topic: Topic,
}

pub struct Mqtt {
    receiver: MqttReceiver,
    sender: MqttSender,
}

pub struct MqttReceiver {
    event_loop: EventLoop,
}

pub struct MqttSender {
    mqtt: MqttMeta,
    client: AsyncClient,
}

#[derive(Debug)]
pub struct MqttBuilder {
    pub server: SocketAddr,
    pub username: String,
    pub password: String,
    // Gateway ID to publish messages from
    pub gateway: NodeId,
    pub root_topic: Topic,
}

impl MqttBuilder {
    pub fn new(
        server: SocketAddr,
        username: String,
        password: String,
        gateway: NodeId,
        root_topic: Topic,
    ) -> Self {
        Self {
            server,
            username,
            password,
            gateway,
            root_topic,
        }
    }

    pub async fn connect(&self) -> Result<Mqtt, std::io::Error> {
        let mut mqttoptions = MqttOptions::new(
            self.gateway.to_string(),
            self.server.ip().to_string(),
            self.server.port(),
        );
        mqttoptions.set_keep_alive(Duration::from_secs(10));
        mqttoptions.set_credentials(self.username.clone(), self.password.clone());

        let topic = format!("{}/2/e/+/+", self.root_topic);
        let (client, event_loop) = AsyncClient::new(mqttoptions, 30);
        client
            .subscribe(topic, QoS::AtMostOnce)
            .await
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("MQTT subscription failed: {}", e),
                )
            })?;

        let data = MqttMeta {
            gateway: self.gateway,
            root_topic: self.root_topic.clone(),
        };
        let reader = MqttReceiver { event_loop };
        let writer = MqttSender { mqtt: data, client };

        Ok(Mqtt {
            receiver: reader,
            sender: writer,
        })
    }
}

impl MqttReceiver {
    pub async fn next(
        &mut self,
    ) -> Result<(meshtastic::MeshPacket, ChannelId, NodeId), std::io::Error> {
        loop {
            let event = self.event_loop.poll().await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("Recv error: {:?}", e),
                )
            })?;

            if let rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish)) = event {
                let service_envelope = meshtastic::ServiceEnvelope::decode(publish.payload.clone())
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Decode error: {:?}", e),
                        )
                    })?;
                let gateway_id = NodeId::try_from(service_envelope.gateway_id).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Received invalid gateway ID: {:?}", e),
                    )
                })?;

                if let Some(packet) = service_envelope.packet {
                    return Ok((packet, service_envelope.channel_id, gateway_id));
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Envelope has no packet"),
                    ));
                }
            }
        }
    }
}
type MqttSendData = (ChannelId, meshtastic::MeshPacket);

impl MqttSender {
    pub async fn send(&mut self, send_data: MqttSendData) -> Result<(), std::io::Error> {
        let (channel_id, mesh_packet) = send_data;
        let topic = format!(
            "{}/2/e/{}/{}",
            self.mqtt.root_topic, channel_id, self.mqtt.gateway
        );
        let service_envelope = meshtastic::ServiceEnvelope {
            packet: Some(mesh_packet),
            channel_id,
            gateway_id: self.mqtt.gateway.into(),
        };

        self.client
            .publish(
                topic,
                QoS::AtLeastOnce,
                false,
                service_envelope.encode_to_vec(),
            )
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, e))?;

        Ok(())
    }
}

impl Mqtt {
    pub async fn send(&mut self, send_data: MqttSendData) -> Result<(), std::io::Error> {
        self.sender.send(send_data).await
    }

    pub async fn next(
        &mut self,
    ) -> Result<(meshtastic::MeshPacket, ChannelId, NodeId), std::io::Error> {
        self.receiver.next().await
    }

    pub fn split(self) -> (MqttSender, MqttReceiver) {
        (self.sender, self.receiver)
    }
}
