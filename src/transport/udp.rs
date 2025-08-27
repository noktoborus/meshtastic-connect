use std::{
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use bytes::BytesMut;
use prost::Message;
use socket2::SockRef;
use tokio::net::UdpSocket;
const STREAM_PACKET_SIZE_MAX: u16 = 512;
use crate::meshtastic::{self, MeshPacket};

#[derive(Debug, Clone, Copy)]
pub struct Interface {
    pub if_addr: IpAddr,
    pub if_index: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Multicast {
    pub address: IpAddr,
    pub interface: Interface,
}

impl Interface {
    pub fn unspecified() -> Self {
        Self {
            if_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            if_index: 0,
        }
    }
}

#[derive(Debug)]
pub struct UDP {
    pub bind_address: SocketAddr,
    pub remote_address: SocketAddr,
    pub join_multicast: Option<Multicast>,
    connection: Option<UdpSocket>,
}

impl UDP {
    pub fn new(
        bind_address: SocketAddr,
        remote_address: SocketAddr,
        join_multicast: Option<Multicast>,
    ) -> Self {
        Self {
            bind_address,
            remote_address,
            join_multicast,
            connection: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), std::io::Error> {
        let socket = UdpSocket::bind(&[self.bind_address][..]).await?;
        let sock_ref = SockRef::from(&socket);
        sock_ref.set_reuse_address(true)?;

        if let Some(multicast) = self.join_multicast {
            match multicast.address {
                IpAddr::V4(join_ipv4_addr) => {
                    sock_ref.set_multicast_loop_v4(false)?;
                    sock_ref.set_multicast_ttl_v4(1)?;

                    match multicast.interface.if_addr {
                        IpAddr::V4(if_ipv4_addr) => {
                            sock_ref.join_multicast_v4(&join_ipv4_addr, &if_ipv4_addr)?;
                            sock_ref.set_multicast_if_v4(&if_ipv4_addr)?;
                        }
                        IpAddr::V6(if_ipv6_addr) => {
                            if if_ipv6_addr.is_unspecified() {
                                sock_ref
                                    .join_multicast_v4(&join_ipv4_addr, &Ipv4Addr::UNSPECIFIED)?;
                            } else {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "IPv6 Address is not suitable for IPv4 multicast",
                                ));
                            }
                        }
                    }
                }
                IpAddr::V6(ipv6_addr) => {
                    sock_ref.set_multicast_loop_v6(false)?;
                    sock_ref.set_multicast_hops_v6(1)?;

                    sock_ref.join_multicast_v6(&ipv6_addr, multicast.interface.if_index)?;
                    sock_ref.set_multicast_if_v6(multicast.interface.if_index)?;
                }
            }
        }

        drop(sock_ref);
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
                socket.send_to(&buf, self.remote_address).await?;
                Ok(())
            }
        }
    }

    pub async fn disconnect(&mut self) {
        if let Some(connection) = self.connection.take() {
            drop(connection);
        }
    }
}
