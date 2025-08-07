use bytes::{BufMut, BytesMut};
use futures::SinkExt;
use prost::Message;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};
use tokio_stream::StreamExt;
use zerocopy::U16;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

const STREAM_PACKET_SIZE_MAX: u16 = 512;
const STREAM_MAGIC_START1: u8 = 0x94;
const STREAM_MAGIC_START2: u8 = 0xc3;
const STREAM_HEADER_MAGIC: [u8; 2] = [STREAM_MAGIC_START1, STREAM_MAGIC_START2];
const STREAM_WAKEUP_MAGIC: [u8; 4] = [
    STREAM_MAGIC_START1,
    STREAM_MAGIC_START1,
    STREAM_MAGIC_START1,
    STREAM_MAGIC_START1,
];
#[repr(C)]
#[derive(Debug, Immutable, FromBytes, KnownLayout, IntoBytes)]
pub struct MeshtasticStreamHeader {
    magic: [u8; 2],
    pub length: U16<zerocopy::byteorder::BE>,
}

impl Default for MeshtasticStreamHeader {
    fn default() -> Self {
        Self {
            magic: STREAM_HEADER_MAGIC,
            length: U16::new(0),
        }
    }
}

impl MeshtasticStreamHeader {
    pub fn new(length: u16) -> Self {
        Self {
            magic: STREAM_HEADER_MAGIC,
            length: U16::new(length),
        }
    }
}
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::meshtastic;

#[derive(Debug)]
struct RadioCodec;

pub enum StreamData {
    Packet(meshtastic::FromRadio),
    Unstructured(BytesMut),
}

impl Decoder for RadioCodec {
    type Item = StreamData;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        static HEADER_LEN: usize = size_of::<MeshtasticStreamHeader>();

        let dropoff_len = if let Some(pos) = src
            .windows(STREAM_HEADER_MAGIC.len())
            .position(|window| window == STREAM_HEADER_MAGIC)
        {
            pos
        } else if src.last() == Some(&STREAM_MAGIC_START1) {
            src.len() - 1
        } else {
            src.len()
        };

        if dropoff_len > 0 {
            return Ok(Some(StreamData::Unstructured(src.split_to(dropoff_len))));
        } else if src.len() < HEADER_LEN {
            return Ok(None);
        }

        let header_bytes = &src[..HEADER_LEN];
        let header: &MeshtasticStreamHeader;
        match MeshtasticStreamHeader::ref_from_bytes(header_bytes) {
            Ok(result) => header = result,
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ));
            }
        }

        if header.magic != STREAM_HEADER_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Invalid magic: {:#x?} (expected {:#x?})",
                    header.magic, STREAM_HEADER_MAGIC
                ),
            ));
        }

        let length = header.length.get();
        if length >= STREAM_PACKET_SIZE_MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Invalid packet length: {} (expected less {})",
                    length, STREAM_PACKET_SIZE_MAX
                ),
            ));
        }

        let frame_len = length as usize + HEADER_LEN;

        if src.len() >= frame_len {
            let pbuf = src.split_to(frame_len);
            match meshtastic::FromRadio::decode(&pbuf[HEADER_LEN..]) {
                Ok(from_radio) => Ok(Some(StreamData::Packet(from_radio))),
                Err(e) => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )),
            }
        } else {
            Ok(None)
        }
    }
}

impl Encoder<meshtastic::to_radio::PayloadVariant> for RadioCodec {
    type Error = std::io::Error;

    fn encode(
        &mut self,
        item: meshtastic::to_radio::PayloadVariant,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        let to_radio = meshtastic::ToRadio {
            payload_variant: Some(item),
        };
        let header = MeshtasticStreamHeader::new(to_radio.encoded_len() as u16);
        dst.put_slice(header.as_bytes());
        to_radio
            .encode(dst)
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e.to_string()))?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum StreamAddress {
    TCPSocket(SocketAddr),
    Serial(String),
}

#[derive(Debug)]
enum StreamCodec {
    No,
    Socket(Framed<TcpStream, RadioCodec>),
    Serial(Framed<SerialStream, RadioCodec>),
}

#[derive(Debug)]
pub struct Stream {
    pub address: StreamAddress,
    pub heartbeat_interval: Duration,
    codec: StreamCodec,
}

async fn recv_for_codec<T>(
    codec: &mut Framed<T, RadioCodec>,
    heartbeat_interval: Duration,
) -> Result<StreamData, std::io::Error>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut hb_interval =
        tokio::time::interval_at(Instant::now() + heartbeat_interval, heartbeat_interval);

    loop {
        tokio::select! {
            _ = hb_interval.tick() => {
                codec.send(meshtastic::to_radio::PayloadVariant::Heartbeat(meshtastic::Heartbeat{})).await?;
            }
            Some(result) = codec.next() => {
                return Ok(result?)
            }
        }
    }
}

impl Stream {
    pub async fn recv(&mut self) -> Result<StreamData, std::io::Error> {
        match self.codec {
            StreamCodec::No => Err(std::io::Error::new(
                ErrorKind::NotConnected,
                "Device not connected",
            )),
            StreamCodec::Socket(ref mut framed) => {
                recv_for_codec::<TcpStream>(framed, self.heartbeat_interval).await
            }
            StreamCodec::Serial(ref mut framed) => {
                recv_for_codec(framed, self.heartbeat_interval).await
            }
        }
    }

    pub async fn send(
        &mut self,
        to_radio: meshtastic::to_radio::PayloadVariant,
    ) -> Result<(), std::io::Error> {
        match self.codec {
            StreamCodec::No => {
                return Err(std::io::Error::new(
                    ErrorKind::NotConnected,
                    "Device not connected",
                ));
            }
            StreamCodec::Socket(ref mut framed) => framed.send(to_radio).await?,
            StreamCodec::Serial(ref mut framed) => framed.send(to_radio).await?,
        };

        Ok(())
    }

    pub async fn connect(&mut self) -> Result<(), std::io::Error> {
        match &self.address {
            StreamAddress::TCPSocket(socket_addr) => {
                let mut tcp = TcpStream::connect(socket_addr).await?;
                tcp.write(&STREAM_WAKEUP_MAGIC).await?;
                let mut codec = RadioCodec.framed(tcp);
                codec
                    .send(meshtastic::to_radio::PayloadVariant::WantConfigId(
                        u32::to_be(0x0),
                    ))
                    .await?;
                self.codec = StreamCodec::Socket(codec);
            }
            StreamAddress::Serial(port) => {
                let mut serial = tokio_serial::new(port.clone(), 115200)
                    .data_bits(tokio_serial::DataBits::Eight)
                    .parity(tokio_serial::Parity::None)
                    .stop_bits(tokio_serial::StopBits::One)
                    .flow_control(tokio_serial::FlowControl::None)
                    .open_native_async()?;
                serial.write_request_to_send(true)?;
                serial.write_data_terminal_ready(true)?;
                serial.write(&STREAM_WAKEUP_MAGIC).await?;
                let mut codec = RadioCodec.framed(serial);
                codec
                    .send(meshtastic::to_radio::PayloadVariant::WantConfigId(
                        u32::to_be(0x0),
                    ))
                    .await?;
                self.codec = StreamCodec::Serial(codec);
            }
        }
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        let _ = self
            .send(meshtastic::to_radio::PayloadVariant::Disconnect(true))
            .await;
        match self.codec {
            StreamCodec::No => {}
            StreamCodec::Socket(ref mut framed) => {
                let _ = framed.close().await;
            }
            StreamCodec::Serial(ref mut framed) => {
                let _ = framed.close().await;
            }
        }
        self.codec = StreamCodec::No;
    }

    pub fn new(address: StreamAddress, heartbeat_interval: Duration) -> Self {
        Self {
            address,
            heartbeat_interval,
            codec: StreamCodec::No,
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        match self.codec {
            StreamCodec::No => {
                return;
            }
            StreamCodec::Socket(_) => {}
            StreamCodec::Serial(_) => {}
        }

        panic!("`Disconnect` message is not send before socket closing!")
    }
}
