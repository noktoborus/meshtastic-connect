use tokio_serial::{SerialPort, SerialPortBuilderExt};
use tokio_util::codec::Decoder;

use super::{Stream, codec::MeshtasticStreamCodec};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SerialBuilder {
    pub tty: String,
    pub baudrate: u32,
}

impl SerialBuilder {
    pub fn new(tty: String, baudrate: u32) -> Self {
        SerialBuilder { tty, baudrate }
    }

    pub async fn connect(&self) -> Result<Stream, std::io::Error> {
        let mut serial = tokio_serial::new(self.tty.clone(), self.baudrate)
            .data_bits(tokio_serial::DataBits::Eight)
            .parity(tokio_serial::Parity::None)
            .stop_bits(tokio_serial::StopBits::One)
            .flow_control(tokio_serial::FlowControl::None)
            .open_native_async()?;
        serial.write_request_to_send(true)?;
        serial.write_data_terminal_ready(true)?;
        let codec = MeshtasticStreamCodec {}.framed(serial);
        Ok(Stream::Serial(codec))
    }
}
