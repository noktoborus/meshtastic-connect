use std::{
    fmt,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};

use crate::meshtastic;
use prost::Message;
use socket2::SockRef;
use tokio::{io::ReadBuf, net::UdpSocket};

const UDP_PACKET_SIZE_MAX: u16 = 512;

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
pub struct UdpBuilder {
    pub bind_address: SocketAddr,
    pub remote_address: SocketAddr,
    pub join_multicast: Option<Multicast>,
}

impl fmt::Display for UdpBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(join_multicast) = self.join_multicast {
            write!(
                f,
                "{}->{} [{}:{}]",
                self.bind_address,
                self.remote_address,
                join_multicast.address,
                join_multicast.interface.if_addr,
            )
        } else {
            write!(f, "{}->{}", self.bind_address, self.remote_address)
        }
    }
}

pub struct Udp {
    socket: UdpSocket,
    remote_address: SocketAddr,
}

impl UdpBuilder {
    pub fn new(
        bind_address: SocketAddr,
        remote_address: SocketAddr,
        join_multicast: Option<Multicast>,
    ) -> Self {
        Self {
            bind_address,
            remote_address,
            join_multicast,
        }
    }

    pub async fn connect(&self) -> Result<Udp, std::io::Error> {
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

        Ok(Udp {
            socket,
            remote_address: self.remote_address,
        })
    }
}

impl futures::Sink<meshtastic::MeshPacket> for Udp {
    type Error = std::io::Error;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(
        self: Pin<&mut Self>,
        mesh_packet: meshtastic::MeshPacket,
    ) -> Result<(), Self::Error> {
        // this.socket.try_send_to(&mesh_packet, this.target)?;
        let buf = mesh_packet.encode_to_vec();
        let remote = self.remote_address;
        self.socket.try_send_to(&buf, remote)?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl futures::Stream for Udp {
    type Item = Result<(meshtastic::MeshPacket, SocketAddr), std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        static PACKET_BUFFER: usize = UDP_PACKET_SIZE_MAX as usize * 2;
        let mut u8buf = [0u8; PACKET_BUFFER];
        let mut buf = ReadBuf::new(&mut u8buf);

        match self.socket.poll_recv_from(cx, &mut buf)? {
            Poll::Ready(addr) => {
                let mesh_packet = meshtastic::MeshPacket::decode(buf.filled())
                    .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
                Poll::Ready(Some(Ok((mesh_packet, addr))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
