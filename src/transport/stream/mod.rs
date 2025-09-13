use std::{pin::Pin, task::Context};

use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio_serial::SerialStream;
use tokio_util::codec::Framed;

use crate::meshtastic;
pub use codec::BytesSequence;
pub use codec::StreamRecvData;
pub mod codec;
pub mod serial;
pub mod tcp;

pub enum Stream {
    Serial(Framed<SerialStream, codec::MeshtasticStreamCodec>),
    Tcp(Framed<TcpStream, codec::MeshtasticStreamCodec>),
}

pub type PacketId = u32;

impl futures::Sink<meshtastic::to_radio::PayloadVariant> for Stream {
    type Error = std::io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Stream::Serial(s) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_ready(Pin::new(s), cx)
            }
            Stream::Tcp(t) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_ready(Pin::new(t), cx)
            }
        }
    }

    fn start_send(
        self: Pin<&mut Self>,
        item: meshtastic::to_radio::PayloadVariant,
    ) -> Result<(), Self::Error> {
        match self.get_mut() {
            Stream::Serial(s) => futures::Sink::start_send(Pin::new(s), item),
            Stream::Tcp(t) => futures::Sink::start_send(Pin::new(t), item),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Stream::Serial(s) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_flush(Pin::new(s), cx)
            }
            Stream::Tcp(t) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_flush(Pin::new(t), cx)
            }
        }
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Stream::Serial(s) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_close(Pin::new(s), cx)
            }
            Stream::Tcp(t) => {
                futures::Sink::<meshtastic::to_radio::PayloadVariant>::poll_close(Pin::new(t), cx)
            }
        }
    }
}

impl futures::Sink<codec::BytesSequence> for Stream {
    type Error = std::io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Stream::Serial(s) => futures::Sink::<codec::BytesSequence>::poll_ready(Pin::new(s), cx),
            Stream::Tcp(t) => futures::Sink::<codec::BytesSequence>::poll_ready(Pin::new(t), cx),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: codec::BytesSequence) -> Result<(), Self::Error> {
        match self.get_mut() {
            Stream::Serial(s) => futures::Sink::start_send(Pin::new(s), item),
            Stream::Tcp(t) => futures::Sink::start_send(Pin::new(t), item),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Stream::Serial(s) => futures::Sink::<codec::BytesSequence>::poll_flush(Pin::new(s), cx),
            Stream::Tcp(t) => futures::Sink::<codec::BytesSequence>::poll_flush(Pin::new(t), cx),
        }
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Stream::Serial(s) => futures::Sink::<codec::BytesSequence>::poll_close(Pin::new(s), cx),
            Stream::Tcp(t) => futures::Sink::<codec::BytesSequence>::poll_close(Pin::new(t), cx),
        }
    }
}

impl futures::Stream for Stream {
    type Item = Result<codec::StreamRecvData, std::io::Error>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.get_mut() {
            Stream::Serial(s) => s.try_poll_next_unpin(cx),
            Stream::Tcp(t) => t.try_poll_next_unpin(cx),
        }
    }
}
