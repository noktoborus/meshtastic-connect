use crate::{keyring::node_id::NodeId, meshtastic};
use bytes::BytesMut;
use prost::Message;

use super::{
    mqtt::{ChannelId, Topic},
    stream::{self, PacketId},
};

pub enum MQTTStreamData {
    // MeshPacket from Radio
    MeshPacket(PacketId, meshtastic::MeshPacket),
    // MeshPacket from MQTT
    MQTTMeshPacket(PacketId, meshtastic::MeshPacket, ChannelId, NodeId),
    // Any FromRadio message, except MeshPacket and MqttClientProxyMessage
    FromRadio(PacketId, meshtastic::from_radio::PayloadVariant),
    // Raw, journal or other unrecognized data
    Unstructured(BytesMut),
}

// MQTT using Stream if MQTT Proxy enabled in node's configuration
pub struct MQTTStream {
    pub stream: stream::Stream,
    // Gateway ID to publish messages from
    pub gateway: NodeId,
    pub topic: Topic,
}

impl MQTTStream {
    pub fn new(stream: stream::Stream, gateway: NodeId, topic: Topic) -> Self {
        Self {
            stream,
            gateway,
            topic,
        }
    }

    pub async fn recv(&mut self) -> Result<MQTTStreamData, std::io::Error> {
        match self.stream.recv().await? {
            stream::StreamData::FromRadio(packet_id, from_radio) => match from_radio {
                meshtastic::from_radio::PayloadVariant::Packet(mesh_packet) => {
                    Ok(MQTTStreamData::MeshPacket(packet_id, mesh_packet))
                }
                meshtastic::from_radio::PayloadVariant::MqttClientProxyMessage(mqtt_proxy_msg) => {
                    if let Some(ref payload) = mqtt_proxy_msg.payload_variant {
                        match payload {
                            meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(items) => {
                                let service_envelope = meshtastic::ServiceEnvelope::decode(
                                    items.as_slice(),
                                )
                                .map_err(|e| {
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!(
                                            "MQTT proxy ServiceEnvelope::decode() failed: {:?}",
                                            e
                                        ),
                                    )
                                })?;
                                if let Some(packet) = service_envelope.packet {
                                    let gateway = NodeId::try_from(service_envelope.gateway_id)
                                        .map_err(|e| {
                                            std::io::Error::new(
                                                std::io::ErrorKind::InvalidData,
                                                format!("MQTT proxy gateway id malformed: {:?}", e),
                                            )
                                        })?;

                                    Ok(MQTTStreamData::MQTTMeshPacket(
                                        packet_id,
                                        packet,
                                        service_envelope.channel_id,
                                        gateway,
                                    ))
                                } else {
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!(
                                            "MQTT proxy ServiceEnvelope has no packet: {:?}",
                                            service_envelope
                                        ),
                                    ))
                                }
                            }
                            meshtastic::mqtt_client_proxy_message::PayloadVariant::Text(text) => {
                                Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    format!(
                                        "MQTT proxy message has text payload (unsupported): {:?}",
                                        text
                                    ),
                                ))
                            }
                        }
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("MQTT proxy message has no payload: {:?}", mqtt_proxy_msg),
                        ))
                    }
                }
                _ => Ok(MQTTStreamData::FromRadio(packet_id, from_radio)),
            },
            stream::StreamData::Unstructured(bytes_mut) => {
                Ok(MQTTStreamData::Unstructured(bytes_mut))
            }
        }
    }

    pub async fn send(
        &mut self,
        channel_id: ChannelId,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), std::io::Error> {
        let topic = format!("{}/2/e/{}/{}", self.topic, channel_id, self.gateway);
        let service_envelope = meshtastic::ServiceEnvelope {
            packet: Some(mesh_packet),
            channel_id,
            gateway_id: self.gateway.into(),
        };
        let mqtt_proxy = meshtastic::MqttClientProxyMessage {
            topic: topic.into(),
            retained: false,
            payload_variant: Some(meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(
                service_envelope.encode_to_vec(),
            )),
        };
        let to_radio = meshtastic::to_radio::PayloadVariant::MqttClientProxyMessage(mqtt_proxy);

        self.stream.send(to_radio).await
    }

    // Get stream to send ToRadio, disconnect or perform other actions
    pub fn stream(&mut self) -> &mut stream::Stream {
        &mut self.stream
    }
}
