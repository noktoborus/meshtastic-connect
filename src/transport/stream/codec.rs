use bytes::{BufMut, Bytes, BytesMut};
use prost::Message;
use std::io::ErrorKind;
use tokio_util::codec::{Decoder, Encoder};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, U16};

use crate::meshtastic;

const STREAM_PACKET_SIZE_MAX: u16 = 512;
const STREAM_MAGIC_START1: u8 = 0x94;
const STREAM_MAGIC_START2: u8 = 0xc3;
const STREAM_HEADER_MAGIC: [u8; 2] = [STREAM_MAGIC_START1, STREAM_MAGIC_START2];
pub const STREAM_WAKEUP_MAGIC: [u8; 4] = [
    STREAM_MAGIC_START1,
    STREAM_MAGIC_START1,
    STREAM_MAGIC_START1,
    STREAM_MAGIC_START1,
];

pub enum BytesSequence {
    // Wakeup sequence
    Wakeup,
    // Raw bytes to send without `STREAM_HEADER_MAGIC`
    Unheaded(Bytes),
    // Raw bytes to send with `STREAM_HEADER_MAGIC` and lengths
    Headed(Bytes),
}

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

pub type PacketId = u32;

pub enum StreamRecvData {
    // FromRadio structured data
    FromRadio(PacketId, meshtastic::from_radio::PayloadVariant),
    // Raw, journal or other unrecognized data
    Unstructured(BytesMut),
}

pub struct MeshtasticStreamCodec;

impl Decoder for MeshtasticStreamCodec {
    type Item = StreamRecvData;
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
            return Ok(Some(StreamRecvData::Unstructured(
                src.split_to(dropoff_len),
            )));
        } else if src.len() < HEADER_LEN {
            return Ok(None);
        }

        let header_bytes = &src[..HEADER_LEN];
        let header: &MeshtasticStreamHeader;
        match MeshtasticStreamHeader::ref_from_bytes(header_bytes) {
            Ok(result) => header = result,
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ));
            }
        }

        if header.magic != STREAM_HEADER_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Invalid magic: {:#x?} (expected {:#x?})",
                    header.magic, STREAM_HEADER_MAGIC
                ),
            ));
        }

        let length = header.length.get();
        if length >= STREAM_PACKET_SIZE_MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
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
                Ok(from_radio) => {
                    if let Some(payload_variant) = from_radio.payload_variant {
                        Ok(Some(StreamRecvData::FromRadio(
                            from_radio.id,
                            payload_variant,
                        )))
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Radio send no payload: {:?}", from_radio),
                        ))
                    }
                }
                Err(e) => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            }
        } else {
            Ok(None)
        }
    }
}

impl Encoder<meshtastic::to_radio::PayloadVariant> for MeshtasticStreamCodec {
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

impl Encoder<BytesSequence> for MeshtasticStreamCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: BytesSequence, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            BytesSequence::Wakeup => dst.put_slice(STREAM_WAKEUP_MAGIC.as_bytes()),
            BytesSequence::Unheaded(bytes) => dst.put_slice(bytes.as_bytes()),
            BytesSequence::Headed(bytes) => {
                let header = MeshtasticStreamHeader::new(bytes.len() as u16);
                dst.put_slice(header.as_bytes());
                dst.put_slice(bytes.as_bytes());
            }
        }
        Ok(())
    }
}
