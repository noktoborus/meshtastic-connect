use crate::{keyring::node_id::NodeId, meshtastic};
use prost::Message;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use std::{net::SocketAddr, time::Duration};

// Root topic
type Topic = String;

type ChannelId = String;

struct MQTTConnection {
    client: AsyncClient,
    event_loop: EventLoop,
}

pub struct MQTT {
    pub server: SocketAddr,
    pub username: String,
    pub password: String,
    // Gateway ID to publish messages from
    pub gateway: NodeId,
    pub topic: Topic,
    connection: Option<MQTTConnection>,
}

impl MQTT {
    pub fn new(
        server: SocketAddr,
        username: String,
        password: String,
        gateway: NodeId,
        topic: Topic,
    ) -> Self {
        Self {
            server,
            username,
            password,
            gateway,
            topic,
            connection: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), std::io::Error> {
        let mut mqttoptions = MqttOptions::new(
            self.gateway.to_string(),
            self.server.ip().to_string(),
            self.server.port(),
        );
        mqttoptions.set_keep_alive(Duration::from_secs(10));
        mqttoptions.set_credentials(self.username.clone(), self.password.clone());

        let topic = format!("{}/2/e/+/+", self.topic);
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
        self.connection = Some(MQTTConnection { client, event_loop });
        Ok(())
    }

    pub async fn recv(
        &mut self,
    ) -> Result<(Option<meshtastic::MeshPacket>, ChannelId, NodeId), std::io::Error> {
        match self.connection {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Not connected",
            )),
            Some(ref mut connection) => loop {
                let event = connection.event_loop.poll().await.map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, format!("Recv error: {:?}", e))
                })?;

                if let rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish)) = event {
                    let service_envelope = meshtastic::ServiceEnvelope::decode(
                        publish.payload.clone(),
                    )
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Decode error: {:?}", e),
                        )
                    })?;
                    let gateway_id =
                        NodeId::try_from(service_envelope.gateway_id).map_err(|e| {
                            std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Received invalid gateway ID: {:?}", e),
                            )
                        })?;
                    return Ok((
                        service_envelope.packet,
                        service_envelope.channel_id,
                        gateway_id,
                    ));
                }
            },
        }
    }

    pub async fn send(
        &mut self,
        channel_name: Option<ChannelId>,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        match self.connection {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Not connected",
            )),
            Some(ref mut connection) => {
                let channel_name = channel_name.unwrap_or("PKI".into());
                let topic = format!("{}/2/e/{}/{}", self.topic, channel_name, mesh_packet.from);
                let service_envelope = meshtastic::ServiceEnvelope {
                    packet: Some(mesh_packet),
                    channel_id: channel_name,
                    gateway_id: self.gateway.into(),
                };

                connection
                    .client
                    .publish(
                        topic,
                        QoS::AtLeastOnce,
                        false,
                        service_envelope.encode_to_vec(),
                    )
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                Ok(())
            }
        }
    }

    pub async fn disconnect(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = connection.client.disconnect();
            drop(connection);
        }
    }
}
