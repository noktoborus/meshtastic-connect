use std::{
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use bytes::BytesMut;
use prost::Message;
use tokio::net::UdpSocket;
const STREAM_PACKET_SIZE_MAX: u16 = 512;
use crate::meshtastic::{self, MeshPacket};

#[derive(Debug)]
pub struct Multicast {
    pub address: SocketAddr,
    connection: Option<UdpSocket>,
}

impl Multicast {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            connection: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), std::io::Error> {
        let bind_addr = match self.address {
            SocketAddr::V4(_) => {
                SocketAddr::new(IpAddr::from(Ipv4Addr::UNSPECIFIED), self.address.port())
            }
            SocketAddr::V6(_) => {
                SocketAddr::new(IpAddr::from(Ipv6Addr::UNSPECIFIED), self.address.port())
            }
        };

        let socket = UdpSocket::bind(&[bind_addr][..]).await?;

        match self.address {
            SocketAddr::V4(socket_addr_v4) => {
                socket.join_multicast_v4(*socket_addr_v4.ip(), Ipv4Addr::UNSPECIFIED)?;
            }
            SocketAddr::V6(socket_addr_v6) => {
                socket.join_multicast_v6(socket_addr_v6.ip(), 0)?;
            }
        };

        self.connection = Some(socket);

        Ok(())
    }

    pub async fn recv(&mut self) -> Result<(meshtastic::MeshPacket, SocketAddr), std::io::Error> {
        match self.connection {
            None => Err(std::io::Error::new(
                ErrorKind::NotConnected,
                "Not joined to multicast",
            )),
            Some(ref mut socket) => {
                static PACKET_BUFFER: usize = STREAM_PACKET_SIZE_MAX as usize * 2;
                let mut buf = [0u8; PACKET_BUFFER];

                let (size, addr) = socket.recv_from(&mut buf).await?;
                let mesh_packet = meshtastic::MeshPacket::decode(&buf[0..size])
                    .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
                Ok((mesh_packet, addr))
            }
        }
    }

    pub async fn send(&mut self, mesh_packet: MeshPacket) -> Result<(), std::io::Error> {
        match self.connection {
            None => Err(std::io::Error::new(
                ErrorKind::NotConnected,
                "Not joined to multicast",
            )),
            Some(ref mut socket) => {
                let mut buf = BytesMut::new();

                mesh_packet
                    .encode(&mut buf)
                    .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e.to_string()))?;
                socket.send_to(&buf, self.address).await?;
                Ok(())
            }
        }
    }

    async fn disconnect(&mut self) {
        self.connection = None;
    }
}
