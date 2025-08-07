use std::net::ToSocketAddrs;
use std::time::Duration;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use crate::{keyring::Keyring, meshtastic};
use bytes::Bytes;
use parking_lot::RwLock;
use tokio::select;
use tokio_util::sync::CancellationToken;

pub mod multicast;
pub mod stream;

const HEARTBEAT_SECS: u64 = 5;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
struct MQTTAddr {
    address: String,
    username: String,
    password: String,
}

#[derive(Debug, Clone)]
pub enum Endpoint {
    // Connect to MQTT's hostname:port
    MQTT(MQTTAddr),
    // Listen UDP multicast
    UDPMulticast(SocketAddr),
    // Connect to TCP hostname:port
    TCP(String),
    // Connect to serial port
    Serial(String),
}

#[derive(Debug)]
enum TransportVariant {
    Multicast(multicast::Multicast),
    Stream(stream::Stream),
}

impl Endpoint {
    async fn to_transport_variant(self) -> Result<TransportVariant, String> {
        match self {
            Endpoint::MQTT(_mqttaddr) => todo!(),
            Endpoint::UDPMulticast(_endpoint) => todo!(),
            Endpoint::TCP(endpoint) => {
                let address = endpoint
                    .to_socket_addrs()
                    .map_err(|e| e.to_string())?
                    .next()
                    .ok_or("No addresses resolved")?;

                Ok(TransportVariant::Stream(stream::Stream::new(
                    stream::StreamAddress::TCPSocket(address),
                    Duration::from_secs(HEARTBEAT_SECS),
                )))
            }
            Endpoint::Serial(endpoint) => Ok(TransportVariant::Stream(stream::Stream::new(
                stream::StreamAddress::Serial(endpoint),
                Duration::from_secs(HEARTBEAT_SECS),
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub name: String,
    pub short_name: String,
    pub id: u32,
}

#[derive(Default, Debug)]
pub struct TransportPool {
    transports: HashMap<Endpoint, Transport>,
}

impl TransportPool {
    pub fn serve(&self, endpoint: &Endpoint) -> Transport {
        let transport = Transport::new();

        transport
    }

    // Stop all transports
    pub fn stop(&self) {
        todo!()
    }
}

#[derive(Default, Debug, Clone)]
pub struct EndpointMeta {
    pub node_info: Option<NodeInfo>,
    pub channels: Option<Keyring>,
    pub key: Option<Bytes>,
}

#[derive(Default, Debug)]
pub enum TransportState {
    #[default]
    Inited,
    Resolving,
    Connecting,
    Connected,
    Stopping,
    Stopped,
    Error(String),
}

#[derive(Debug)]
pub enum TransportMessage {
    // Raw string from Serial socket
    RawString(String),
    // Packet from UDP broadcast
    MeshPacket(meshtastic::MeshPacket),
    // MQTT message
    ServiceEnvelope(meshtastic::ServiceEnvelope),
    // TCP and Serial transport's message
    FromRadio(meshtastic::FromRadio),
}

#[derive(Default, Debug)]
pub struct Transport {
    received: Arc<RwLock<Vec<TransportMessage>>>,
    cancel_token: CancellationToken,
    meta: Arc<RwLock<EndpointMeta>>,
    state: Arc<RwLock<TransportState>>,
}

impl Transport {
    fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn close(&self) {
        self.cancel_token.cancel()
    }
}

async fn event_loop(transport: Transport, endpoint: Endpoint) {
    *transport.state.write() = TransportState::Resolving;
    let transport_variant = endpoint.to_transport_variant().await.unwrap();

    loop {
        select! {
            _ = transport.cancel_token.cancelled() => {
                *transport.state.write() = TransportState::Stopping;
                break;
            }
        }
    }

    *transport.state.write() = TransportState::Stopped;
}
