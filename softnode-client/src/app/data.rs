use chrono::{DateTime, Utc};
use meshtastic_connect::{
    keyring::{Keyring, cryptor::Decrypt, key::Key, node_id::NodeId},
    meshtastic,
};
use prost::Message;
use std::{collections::HashMap, fmt::Display};

use super::byte_node_id::ByteNodeId;

pub struct JournalData {
    port_num: meshtastic::PortNum,
    hint: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct JournalDataSerdeHelper<'a> {
    port_num: &'a str,
    hint: &'a str,
}

impl serde::Serialize for JournalData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let helper = JournalDataSerdeHelper {
            port_num: self.port_num.as_str_name(),
            hint: self.hint.as_str(),
        };
        helper.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for JournalData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = JournalDataSerdeHelper::deserialize(deserializer)?;
        Ok(JournalData {
            port_num: meshtastic::PortNum::from_str_name(&helper.port_num).ok_or_else(|| {
                serde::de::Error::custom(format!("Unknown port number: {}", helper.port_num))
            })?,
            hint: helper.hint.to_string(),
        })
    }
}

pub enum DataVariant {
    Encrypted(Vec<u8>),
    Decrypted(meshtastic::Data),
    DecryptError(DecryptError, Vec<u8>),
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub enum DecryptError {
    DecryptorNotFound,
    DecryptFailed,
    ConstructFailed,
}

#[derive(serde::Deserialize, serde::Serialize)]
enum DataVariantSerdeHelper {
    Encrypted(Vec<u8>),
    Decrypted(Vec<u8>),
    DecryptError(DecryptError, Vec<u8>),
}

impl serde::Serialize for DataVariant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            DataVariant::Encrypted(data) => DataVariantSerdeHelper::Encrypted(data.clone()),
            DataVariant::Decrypted(data) => DataVariantSerdeHelper::Decrypted(data.encode_to_vec()),
            DataVariant::DecryptError(reason, data) => {
                DataVariantSerdeHelper::DecryptError(reason.clone(), data.clone())
            }
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for DataVariant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = DataVariantSerdeHelper::deserialize(deserializer)?;

        match helper {
            DataVariantSerdeHelper::Encrypted(items) => Ok(DataVariant::Encrypted(items)),
            DataVariantSerdeHelper::Decrypted(items) => {
                let data =
                    meshtastic::Data::decode(items.as_slice()).map_err(serde::de::Error::custom)?;

                Ok(DataVariant::Decrypted(data))
            }
            DataVariantSerdeHelper::DecryptError(reason, data) => {
                Ok(DataVariant::DecryptError(reason, data))
            }
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, PartialOrd)]
pub struct StoreMeshRxInfo {
    pub rx_time: DateTime<Utc>,
    pub rx_snr: f32,
    pub rx_rssi: i32,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct StoredMeshHeader {
    pub from: NodeId,
    pub to: NodeId,
    pub channel: u32,
    pub id: u32,
    pub priority: String,
    pub via_mqtt: bool,
    pub rx: Option<StoreMeshRxInfo>,
    pub hop_limit: u32,
    pub hop_start: u32,
    pub pki_encrypted: bool,
    pub next_hop: ByteNodeId,
    pub relay_node: ByteNodeId,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct StoredMeshPacket {
    pub sequence_number: u64,
    pub store_timestamp: DateTime<chrono::Utc>,
    pub gateway: Option<NodeId>,
    pub connection_name: String,
    pub header: StoredMeshHeader,
    pub data: Option<DataVariant>,
}

impl StoredMeshPacket {
    // Decrypt data if possible or return error
    pub fn decrypt(mut self, keyring: &Keyring) -> Self {
        if let Some(data) = self.data {
            let data = match data {
                DataVariant::Encrypted(items) | DataVariant::DecryptError(_, items) => {
                    if let Some(cryptor) =
                        keyring.cryptor_for(self.header.from, self.header.to, self.header.channel)
                    {
                        if let Ok(decrypted) = cryptor.decrypt(self.header.id, items.clone()) {
                            if let Ok(data) = meshtastic::Data::decode(decrypted.as_slice()) {
                                DataVariant::Decrypted(data)
                            } else {
                                DataVariant::DecryptError(DecryptError::ConstructFailed, items)
                            }
                        } else {
                            DataVariant::DecryptError(DecryptError::DecryptFailed, items)
                        }
                    } else {
                        DataVariant::DecryptError(DecryptError::DecryptorNotFound, items)
                    }
                }
                DataVariant::Decrypted(items) => DataVariant::Decrypted(items),
            };

            self.data = Some(data);
        }
        self
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct PowerMetrics {
    voltage: f32,
    current: f32,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash, Clone, Copy)]
pub enum TelemetryVariant {
    BarometricPressure,
    Temperature,
    Lux,
    Iaq,
    Humidity,
    GasResistance,
    Radiation,
    // power metric with channel no (1-3)
    PowerMetricVoltage(usize),
    // power metric with channel no (1-3)
    PowerMetricCurrent(usize),
}

impl Display for TelemetryVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryVariant::BarometricPressure => write!(f, "Pressure"),
            TelemetryVariant::Temperature => write!(f, "Temperature"),
            TelemetryVariant::Lux => write!(f, "Lux"),
            TelemetryVariant::Iaq => write!(f, "Iaq"),
            TelemetryVariant::Humidity => write!(f, "Humidity"),
            TelemetryVariant::GasResistance => write!(f, "Gas Resistance"),
            TelemetryVariant::Radiation => write!(f, "Radiation"),
            TelemetryVariant::PowerMetricVoltage(channel) => {
                write!(f, "Voltage ch. {}", channel)
            }
            TelemetryVariant::PowerMetricCurrent(channel) => {
                write!(f, "Current ch. {}", channel)
            }
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, PartialOrd)]
pub struct NodeTelemetry {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Position {
    pub seq_number: u32,
    pub timestamp: DateTime<Utc>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: i32,
    pub speed: u32,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, PartialOrd)]
pub enum NodePacketType {
    Normal(String),
    CannotDecrypt,
    Error,
    Empty,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, PartialOrd)]
pub struct NodePacket {
    pub timestamp: DateTime<Utc>,
    pub packet_type: NodePacketType,
    pub to: NodeId,
    pub channel: u32,
    pub rx_info: Option<StoreMeshRxInfo>,
    pub gateway: Option<NodeId>,
    pub packet_id: u32,
    pub hop_limit: u32,
}

#[derive(Default, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct NodeInfoExtended {
    pub timestamp: DateTime<Utc>,
    pub announced_node_id: String,
    pub long_name: String,
    pub short_name: String,
    pub pkey: Option<Key>,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq)]
pub struct GatewayInfo {
    pub timestamp: DateTime<Utc>,
    pub rx_info: Option<StoreMeshRxInfo>,
    pub hop_limit: u32,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub extended_info_history: Vec<NodeInfoExtended>,
    pub position: Vec<Position>,
    pub telemetry: HashMap<TelemetryVariant, Vec<NodeTelemetry>>,
    pub packet_statistics: Vec<NodePacket>,
    pub gateway_for: HashMap<NodeId, Vec<GatewayInfo>>,
}

macro_rules! push_statistic {
    ($list:expr, $packet:expr) => {
        if !$list.is_empty() {
            for (i, v) in $list.iter().rev().enumerate() {
                if v == &$packet {
                    break;
                }

                if $packet.timestamp > v.timestamp {
                    $list.insert($list.len() - i, $packet);
                    break;
                }
            }
        } else {
            $list.push($packet);
        }
    };
}

impl NodeInfo {
    fn push_telemetry(
        &mut self,
        timestamp: DateTime<Utc>,
        telemetry_variant: TelemetryVariant,
        telemetry: f64,
    ) {
        let telemetry = NodeTelemetry {
            timestamp,
            value: telemetry,
        };
        let list = self.telemetry.entry(telemetry_variant).or_default();

        push_statistic!(list, telemetry);
    }

    fn update_using_data(
        &mut self,
        stored_timestamp: DateTime<Utc>,
        data: &meshtastic::Data,
    ) -> Result<meshtastic::PortNum, String> {
        match data.portnum() {
            meshtastic::PortNum::PositionApp => {
                let mesh_position = meshtastic::Position::decode(data.payload.as_slice())
                    .map_err(|e| e.to_string())?;

                let altitude = if let Some(altitude) = mesh_position.altitude {
                    altitude
                } else if let Some(altitude) = mesh_position.altitude_hae {
                    altitude
                } else if let Some(altitude) = mesh_position.altitude_geoidal_separation {
                    altitude
                } else {
                    0
                };

                let position = Position {
                    seq_number: mesh_position.seq_number,
                    timestamp: DateTime::from_timestamp(mesh_position.timestamp as i64, 0)
                        .unwrap_or(Default::default()),
                    latitude: mesh_position.latitude_i() as f64 * 1e-7,
                    longitude: mesh_position.longitude_i() as f64 * 1e-7,
                    altitude,
                    speed: mesh_position.ground_speed(),
                };

                self.position.push(position);
            }
            meshtastic::PortNum::NodeinfoApp => {
                let user =
                    meshtastic::User::decode(data.payload.as_slice()).map_err(|e| e.to_string())?;

                let pkey = if user.public_key.len() > 0 {
                    Some(Key::try_from(user.public_key)?)
                } else {
                    None
                };

                let node_info_extended = NodeInfoExtended {
                    timestamp: stored_timestamp,
                    announced_node_id: user.id.clone(),
                    long_name: user.long_name,
                    short_name: user.short_name,
                    pkey,
                };

                push_statistic!(self.extended_info_history, node_info_extended);
            }
            meshtastic::PortNum::TelemetryApp => {
                let telemetry = meshtastic::Telemetry::decode(data.payload.as_slice())
                    .map_err(|e| e.to_string())?;
                // let timestamp = DateTime::from_timestamp(telemetry.time as i64, 0)
                //     .map(|v| {
                //         if v == DateTime::<Utc>::default() {
                //             stored_timestamp
                //         } else {
                //             v
                //         }
                //     })
                //     .unwrap_or_else(|| stored_timestamp);
                //
                // Received timestamp may be buggy, so use the stored timestamp
                let timestamp = stored_timestamp;

                match telemetry.variant.ok_or(format!("Telemetry is empty"))? {
                    meshtastic::telemetry::Variant::DeviceMetrics(_device_metrics) => {
                        log::info!("Telemetry::DeviceMetrics ignored");
                    }
                    meshtastic::telemetry::Variant::EnvironmentMetrics(environment_metrics) => {
                        if let Some(barometric) = environment_metrics.barometric_pressure {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::BarometricPressure,
                                barometric as f64,
                            );
                        }
                        if let Some(temperature) = environment_metrics.temperature {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::Temperature,
                                temperature as f64,
                            );
                        }
                        if let Some(lux) = environment_metrics.lux {
                            self.push_telemetry(timestamp, TelemetryVariant::Lux, lux as f64);
                        }
                        if let Some(iaq) = environment_metrics.iaq {
                            self.push_telemetry(timestamp, TelemetryVariant::Iaq, iaq as f64);
                        }
                        if let Some(humidity) = environment_metrics.relative_humidity {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::Humidity,
                                humidity as f64,
                            );
                        }
                        if let Some(gas_resistance) = environment_metrics.gas_resistance {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::GasResistance,
                                gas_resistance as f64,
                            );
                        }
                        if let Some(radiation) = environment_metrics.radiation {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::Radiation,
                                radiation as f64,
                            );
                        }
                    }
                    meshtastic::telemetry::Variant::AirQualityMetrics(_air_quality_metrics) => {
                        log::info!("Telemetry::AirQualityMetrics ignored");
                    }
                    meshtastic::telemetry::Variant::PowerMetrics(power_metrics) => {
                        if let Some(current) = power_metrics.ch1_current {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::PowerMetricCurrent(1),
                                current as f64,
                            );
                        }
                        if let Some(current) = power_metrics.ch2_current {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::PowerMetricCurrent(2),
                                current as f64,
                            );
                        }
                        if let Some(current) = power_metrics.ch3_current {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::PowerMetricCurrent(3),
                                current as f64,
                            );
                        }
                        if let Some(voltage) = power_metrics.ch1_voltage {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::PowerMetricVoltage(1),
                                voltage as f64,
                            )
                        }
                        if let Some(voltage) = power_metrics.ch2_voltage {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::PowerMetricVoltage(2),
                                voltage as f64,
                            );
                        }
                        if let Some(voltage) = power_metrics.ch3_voltage {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::PowerMetricVoltage(3),
                                voltage as f64,
                            );
                        }
                    }
                    meshtastic::telemetry::Variant::LocalStats(_local_stats) => {
                        log::info!("Telemetry::LocalStats ignored");
                    }
                    meshtastic::telemetry::Variant::HealthMetrics(_health_metrics) => {
                        log::info!("Telemetry::HealthMetrics ignored");
                    }
                    meshtastic::telemetry::Variant::HostMetrics(_host_metrics) => {
                        log::info!("Telemetry::HostMetrics ignored");
                    }
                }
            }
            _ => {}
        }
        Ok(data.portnum())
    }

    pub fn update_as_gateway(&mut self, stored_mesh_packet: &StoredMeshPacket) {
        if self.node_id != stored_mesh_packet.header.from {
            let rx_info = if let Some(rx_info) = &stored_mesh_packet.header.rx {
                if rx_info.rx_rssi > RSSI_UPPER_THRESHOLD || rx_info.rx_rssi < RSSI_LOWER_THRESHOLD
                {
                    None
                } else if rx_info.rx_snr > SNR_UPPER_THRESHOLD
                    || rx_info.rx_snr < SNR_LOWER_THRESHOLD
                {
                    None
                } else {
                    Some(rx_info.clone())
                }
            } else {
                None
            };

            let gateway_info = GatewayInfo {
                timestamp: stored_mesh_packet.store_timestamp,
                rx_info,
                hop_limit: stored_mesh_packet.header.hop_limit,
            };

            let list = self
                .gateway_for
                .entry(stored_mesh_packet.header.from)
                .or_insert(Default::default());

            push_statistic!(list, gateway_info);
        }
    }

    pub fn update(&mut self, stored_mesh_packet: &StoredMeshPacket) {
        let timestamp = stored_mesh_packet.store_timestamp;
        // TODO: move to perday_telemetry
        // self.push_telemetry(timestamp, TelemetryVariant::MeshPacket, 1);
        let packet_type = if let Some(data) = &stored_mesh_packet.data {
            match data {
                DataVariant::Encrypted(_) => NodePacketType::CannotDecrypt,
                DataVariant::Decrypted(data) => match self.update_using_data(timestamp, data) {
                    Ok(portnum) => NodePacketType::Normal(format!("{}", portnum.as_str_name())),
                    Err(e) => {
                        log::error!("Failed to update using data: {}", e);
                        NodePacketType::Error
                        // TODO: move to perday_telemetry
                        // self.push_telemetry(timestamp, TelemetryVariant::CorruptedPacket, 1);
                    }
                },

                DataVariant::DecryptError(_, _) => NodePacketType::Error,
            }
        } else {
            NodePacketType::Empty
            // TODO: move to perday_telemetry
            // self.push_telemetry(timestamp, TelemetryVariant::EmptyPackets, 1);
        };

        if let Some(gateway) = stored_mesh_packet.gateway {
            if gateway == stored_mesh_packet.header.from {
                // RSSI and SNR is always 0 for node's messages if it gateway for himself
                return;
            }
        }

        let packet = NodePacket {
            timestamp,
            packet_type,
            to: stored_mesh_packet.header.to,
            channel: stored_mesh_packet.header.channel,
            rx_info: stored_mesh_packet.header.rx.clone(),
            gateway: stored_mesh_packet.gateway,
            packet_id: stored_mesh_packet.header.id,
            hop_limit: stored_mesh_packet.header.hop_limit,
        };

        push_statistic!(self.packet_statistics, packet);
    }
}

const RSSI_UPPER_THRESHOLD: i32 = 50;
const RSSI_LOWER_THRESHOLD: i32 = -200;
const SNR_UPPER_THRESHOLD: f32 = 30.0;
const SNR_LOWER_THRESHOLD: f32 = -200.0;
