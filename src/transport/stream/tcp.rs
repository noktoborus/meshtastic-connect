use std::net::SocketAddr;

use tokio::net::TcpStream;
use tokio_util::codec::Decoder;

use super::{Stream, codec::MeshtasticStreamCodec};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TcpBuilder {
    pub socket_addr: SocketAddr,
}

impl TcpBuilder {
    pub fn new(socket_addr: SocketAddr) -> Self {
        Self { socket_addr }
    }

    pub async fn connect(&self) -> Result<Stream, std::io::Error> {
        let tcp = TcpStream::connect(self.socket_addr).await?;
        let framed = MeshtasticStreamCodec {}.framed(tcp);
        Ok(Stream::Tcp(framed))
    }
}
