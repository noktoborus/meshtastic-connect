use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    keyring::node_id::NodeId,
    meshtastic::{self, to_radio},
};
use bytes::BytesMut;
use futures::StreamExt;
use prost::Message;

use super::{
    mqtt::{ConnectionHint, Topic},
    stream::{self, PacketId},
};

#[derive(Debug)]
pub enum MqttStreamRecvData {
    // MeshPacket from Radio
    MeshPacket(PacketId, meshtastic::MeshPacket),
    // MeshPacket from MQTT
    MQTTMeshPacket(PacketId, meshtastic::MeshPacket, ConnectionHint, NodeId),
    // Any FromRadio message, except MeshPacket and MqttClientProxyMessage
    FromRadio(PacketId, meshtastic::from_radio::PayloadVariant),
    // Raw, journal or other unrecognized data
    Unstructured(BytesMut),
}

pub enum MqttStreamSendData {
    // MeshPacket to Radio for MQTT layer
    MeshPacket(ConnectionHint, meshtastic::MeshPacket),
    // ToRadio message, for Stream layer
    ToRadio(to_radio::PayloadVariant),
    // Raw bytes for Stream layer
    BytesSequence(stream::BytesSequence),
}

// MQTT using Stream if MQTT Proxy enabled in node's configuration
pub struct MqttStream {
    stream: stream::Stream,
    // Gateway ID to publish messages from
    gateway: NodeId,
    topic: Topic,
}

impl MqttStream {
    pub fn new(stream: stream::Stream, gateway: NodeId, topic: Topic) -> Self {
        Self {
            stream,
            gateway,
            topic,
        }
    }

    pub fn stream(&mut self) -> &mut stream::Stream {
        &mut self.stream
    }
}

impl futures::Sink<MqttStreamSendData> for MqttStream {
    type Error = std::io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_ready(
            Pin::new(&mut self.get_mut().stream),
            cx,
        )
    }

    fn start_send(self: Pin<&mut Self>, send_data: MqttStreamSendData) -> Result<(), Self::Error> {
        match send_data {
            MqttStreamSendData::MeshPacket(channel_id, mesh_packet) => {
                let topic = format!("{}/2/e/{}/{}", self.topic, channel_id, self.gateway);
                let service_envelope = meshtastic::ServiceEnvelope {
                    packet: Some(mesh_packet),
                    channel_id,
                    gateway_id: self.gateway.into(),
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
                let to_radio =
                    meshtastic::to_radio::PayloadVariant::MqttClientProxyMessage(mqtt_proxy);

                futures::Sink::<meshtastic::to_radio::PayloadVariant>::start_send(
                    Pin::new(&mut self.get_mut().stream),
                    to_radio,
                )
            }
            MqttStreamSendData::ToRadio(payload_variant) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::start_send(
                    Pin::new(&mut self.get_mut().stream),
                    payload_variant,
                )
            }
            MqttStreamSendData::BytesSequence(bytes_sequence) => {
                futures::Sink::<stream::BytesSequence>::start_send(
                    Pin::new(&mut self.get_mut().stream),
                    bytes_sequence,
                )
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_flush(
            Pin::new(&mut self.get_mut().stream),
            cx,
        )
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_close(
            Pin::new(&mut self.get_mut().stream),
            cx,
        )
    }
}

impl futures::Stream for MqttStream {
    type Item = Result<MqttStreamRecvData, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut().stream.poll_next_unpin(cx) {
            Poll::Ready(item) => match item {
                Some(item) => {
                    let data = match item? {
                        stream::codec::StreamRecvData::FromRadio(packet_id, from_radio) => {
                            match from_radio {
                                meshtastic::from_radio::PayloadVariant::Packet(mesh_packet) => {
                                    Ok(MqttStreamRecvData::MeshPacket(packet_id, mesh_packet))
                                }
                                meshtastic::from_radio::PayloadVariant::MqttClientProxyMessage(
                                    mqtt_proxy_msg,
                                ) => {
                                    if let Some(ref payload) = mqtt_proxy_msg.payload_variant {
                                        match payload {
                                    meshtastic::mqtt_client_proxy_message::PayloadVariant::Data(
                                        items,
                                    ) => {
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
                                            let gateway = NodeId::try_from(service_envelope.gateway_id.as_str())
                                                .map_err(|e| {
                                                    std::io::Error::new(
                                                        std::io::ErrorKind::InvalidData,
                                                        format!(
                                                            "MQTT proxy gateway id malformed {:?}: {:?}",
                                                            service_envelope.gateway_id,
                                                            e
                                                        ),
                                                    )
                                                })?;

                                            Ok(MqttStreamRecvData::MQTTMeshPacket(
                                                packet_id,
                                                packet,
                                                mqtt_proxy_msg.topic,
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
                                    meshtastic::mqtt_client_proxy_message::PayloadVariant::Text(
                                        text,
                                    ) => Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        format!(
                                            "MQTT proxy message has text payload (unsupported): {:?}",
                                            text
                                        ),
                                    )),
                                }
                                    } else {
                                        Err(std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            format!(
                                                "MQTT proxy message has no payload: {:?}",
                                                mqtt_proxy_msg
                                            ),
                                        ))
                                    }
                                }
                                _ => Ok(MqttStreamRecvData::FromRadio(packet_id, from_radio)),
                            }
                        }
                        stream::codec::StreamRecvData::Unstructured(bytes_mut) => {
                            Ok(MqttStreamRecvData::Unstructured(bytes_mut))
                        }
                    };

                    Poll::Ready(Some(data))
                }
                None => Poll::Ready(None),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}
