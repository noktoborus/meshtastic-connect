use chrono::{DateTime, Utc};
use meshtastic_connect::{
    keyring::{Keyring, cryptor::Decrypt, key::Key, node_id::NodeId},
    meshtastic,
};
use prost::Message;
use std::{collections::HashMap, fmt::Display};

use crate::app::node_book::NodeBook;

use super::byte_node_id::ByteNodeId;

pub struct JournalData {
    pub timestamp: DateTime<Utc>,
    pub hop_start: u32,
    pub hop_limit: u32,
    pub id: u32,
    pub from: NodeId,
    pub to: NodeId,
    pub channel: u32,
    pub via_mqtt: bool,
    pub is_pki: bool,
    pub is_encrypted: bool,
    pub gateway: Option<NodeId>,
    pub relay: ByteNodeId,
    pub message_type: String,
    pub message_hint: String,
}

impl From<StoredMeshPacket> for JournalData {
    fn from(stored_mesh_packet: StoredMeshPacket) -> Self {
        let message_type;
        let is_encrypted;
        let message_hint;

        if let Some(data) = stored_mesh_packet.data {
            match data {
                DataVariant::Encrypted(_) => {
                    message_type = "<encrypted>".into();
                    message_hint = "".into();
                    is_encrypted = true;
                }
                DataVariant::Decrypted(decrypt_target, data) => {
                    match decrypt_target {
                        DecryptTarget::Direct(_) => is_encrypted = false,
                        DecryptTarget::PKI => is_encrypted = true,
                        DecryptTarget::Channel(_) => is_encrypted = true,
                    }
                    message_type = data.portnum().as_str_name().into();
                    message_hint = match data.portnum() {
                        meshtastic::PortNum::TextMessageApp => {
                            String::from_utf8_lossy(data.payload.as_slice()).into()
                        }
                        // meshtastic::PortNum::PositionApp => todo!(),
                        // meshtastic::PortNum::NodeinfoApp => todo!(),
                        // meshtastic::PortNum::WaypointApp => todo!(),
                        meshtastic::PortNum::TelemetryApp => {
                            match meshtastic::Telemetry::decode(data.payload.as_slice()) {
                                Ok(telemetry) => telemetry
                                    .variant
                                    .map_or("<empty>".to_string(), |v| match v {
                                        meshtastic::telemetry::Variant::DeviceMetrics(
                                            _device_metrics,
                                        ) => "DeviceMetrics".to_string(),
                                        meshtastic::telemetry::Variant::EnvironmentMetrics(
                                            _environment_metrics,
                                        ) => "EnvironmentMetrics".to_string(),
                                        meshtastic::telemetry::Variant::AirQualityMetrics(
                                            _air_quality_metrics,
                                        ) => "AirQualityMetrics".to_string(),
                                        meshtastic::telemetry::Variant::PowerMetrics(
                                            _power_metrics,
                                        ) => "PowerMetrics".to_string(),
                                        meshtastic::telemetry::Variant::LocalStats(
                                            _local_stats,
                                        ) => "LocalStats".to_string(),
                                        meshtastic::telemetry::Variant::HealthMetrics(
                                            health_metrics,
                                        ) => format!(
                                            "HealthMetrics: {}{}{}",
                                            health_metrics
                                                .temperature
                                                .map(|temp| format!("{} °C ", temp))
                                                .unwrap_or("".to_string()),
                                            health_metrics
                                                .heart_bpm
                                                .map(|bpm| format!("{} BPM ", bpm))
                                                .unwrap_or("".to_string()),
                                            health_metrics
                                                .sp_o2
                                                .map(|spo2| format!("SpO2 {}%", spo2))
                                                .unwrap_or("".to_string()),
                                        ),
                                        meshtastic::telemetry::Variant::HostMetrics(
                                            _host_metrics,
                                        ) => "HostMetrics".to_string(),
                                    })
                                    .into(),
                                Err(e) => format!("<decoding error: {}>", e),
                            }
                        }
                        _ => "".into(),
                    };
                }
                DataVariant::DecryptError(decrypt_error, _) => {
                    is_encrypted = true;
                    match decrypt_error {
                        DecryptError::DecryptorNotFound => {
                            message_type = "<encrypted>".into();
                            message_hint = "".into();
                        }
                        DecryptError::DecryptFailed => {
                            message_type = "<decrypt error>".into();
                            message_hint = "error while decrypting".into();
                        }
                        DecryptError::ConstructFailed => {
                            message_type = "<decrypt error>".into();
                            message_hint = "protobuf error".into();
                        }
                    }
                }
            }
        } else {
            is_encrypted = false;
            message_type = "<empty>".into();
            message_hint = "".into();
        };

        JournalData {
            id: stored_mesh_packet.header.id,
            timestamp: stored_mesh_packet.store_timestamp,
            hop_start: stored_mesh_packet.header.hop_start,
            hop_limit: stored_mesh_packet.header.hop_limit,
            from: stored_mesh_packet.header.from,
            to: stored_mesh_packet.header.to,
            channel: stored_mesh_packet.header.channel,
            via_mqtt: stored_mesh_packet.header.via_mqtt,
            is_pki: stored_mesh_packet.header.pki_encrypted,
            is_encrypted,
            gateway: stored_mesh_packet.gateway,
            relay: stored_mesh_packet.header.relay_node,
            message_type,
            message_hint,
        }
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub enum DecryptTarget {
    Direct(u32),
    PKI,
    Channel(String),
}

#[derive(Clone)]
pub enum DataVariant {
    Encrypted(Vec<u8>),
    Decrypted(DecryptTarget, meshtastic::Data),
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
    Decrypted(DecryptTarget, Vec<u8>),
    DecryptError(DecryptError, Vec<u8>),
}

impl serde::Serialize for DataVariant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            DataVariant::Encrypted(data) => DataVariantSerdeHelper::Encrypted(data.clone()),
            DataVariant::Decrypted(target, data) => {
                DataVariantSerdeHelper::Decrypted(target.clone(), data.encode_to_vec())
            }
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
            DataVariantSerdeHelper::Decrypted(target, items) => {
                let data =
                    meshtastic::Data::decode(items.as_slice()).map_err(serde::de::Error::custom)?;

                Ok(DataVariant::Decrypted(target, data))
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

#[derive(serde::Deserialize, serde::Serialize, Clone)]
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

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct StoredMeshPacket {
    pub sequence_number: u64,
    pub store_timestamp: DateTime<chrono::Utc>,
    pub gateway: Option<NodeId>,
    pub connection_name: String,
    pub connection_hint: Option<String>,
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
                                match cryptor {
                                    meshtastic_connect::keyring::cryptor::Cryptor::Symmetric(
                                        name,
                                        _,
                                    ) => DataVariant::Decrypted(DecryptTarget::Channel(name), data),
                                    meshtastic_connect::keyring::cryptor::Cryptor::PKI(_) => {
                                        DataVariant::Decrypted(DecryptTarget::PKI, data)
                                    }
                                }
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
                DataVariant::Decrypted(target, items) => DataVariant::Decrypted(target, items),
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

#[derive(
    Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord,
)]
pub enum TelemetryVariant {
    // Host Metrics
    UptimeSeconds,

    // Environment
    BarometricPressure,
    EnvironmentTemperature,
    Lux,
    Iaq,
    Humidity,
    GasResistance,
    Radiation,
    // power metric with channel no (1-3)
    PowerMetricVoltage(usize),
    // power metric with channel no (1-3)
    PowerMetricCurrent(usize),
    //
    AirUtilTx,
    ChannelUtilization,
    Voltage,
    BatteryLevel,

    // Health metrics
    HeartRate,
    SpO2,
    HealthTemperature,
    // AirQuality
    AirPM10Standard,
    AirPM25Standard,
    AirPM100Standard,
    AirPM10Environmental,
    AirPM25Environmental,
    AirPM100Environmental,
    AirParticles03um,
    AirParticles05um,
    AirParticles10um,
    AirParticles25um,
    AirParticles50um,
    AirParticles100um,
    AirCo2,
    AirCo2Temperature,
    AirCo2Humidity,
}

impl Display for TelemetryVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryVariant::BarometricPressure => write!(f, "Pressure"),
            TelemetryVariant::EnvironmentTemperature => write!(f, "Environment Temperature"),
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
            TelemetryVariant::AirUtilTx => write!(f, "Device Air Util Tx"),
            TelemetryVariant::ChannelUtilization => write!(f, "Device Channel Utilization"),
            TelemetryVariant::Voltage => write!(f, "Device Voltage"),
            TelemetryVariant::BatteryLevel => write!(f, "Device Battery Level"),
            TelemetryVariant::HeartRate => write!(f, "Heart Rate"),
            TelemetryVariant::SpO2 => write!(f, "SpO2"),
            TelemetryVariant::HealthTemperature => write!(f, "Health Temperature"),
            TelemetryVariant::UptimeSeconds => write!(f, "Uptime Seconds"),
            TelemetryVariant::AirPM10Standard => write!(f, "Air PM10 Standard"),
            TelemetryVariant::AirPM25Standard => write!(f, "Air PM25 Standard"),
            TelemetryVariant::AirPM100Standard => write!(f, "Air PM100 Standard"),
            TelemetryVariant::AirPM10Environmental => write!(f, "Air PM10 Environmental"),
            TelemetryVariant::AirPM25Environmental => write!(f, "Air PM25 Environmental"),
            TelemetryVariant::AirPM100Environmental => write!(f, "Air PM100 Environmental"),
            TelemetryVariant::AirParticles03um => write!(f, "Air Particles 0.3 um"),
            TelemetryVariant::AirParticles05um => write!(f, "Air Particles 0.5 um"),
            TelemetryVariant::AirParticles10um => write!(f, "Air Particles 10 um"),
            TelemetryVariant::AirParticles25um => write!(f, "Air Particles 25 um"),
            TelemetryVariant::AirParticles50um => write!(f, "Air Particles 50 um"),
            TelemetryVariant::AirParticles100um => write!(f, "Air Particles 100 um"),
            TelemetryVariant::AirCo2 => write!(f, "Air CO2"),
            TelemetryVariant::AirCo2Temperature => write!(f, "Air CO2 Temperature"),
            TelemetryVariant::AirCo2Humidity => write!(f, "Air CO2 Humidity"),
        }
    }
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

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, PartialOrd)]
pub struct TelemetryValue {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize, PartialEq, PartialOrd)]
pub struct NodeTelemetry {
    pub values: Vec<TelemetryValue>,
    pub min_peaks: Vec<TelemetryValue>,
    pub max_peaks: Vec<TelemetryValue>,
}

impl NodeTelemetry {
    pub fn push(&mut self, value: TelemetryValue) {
        push_statistic!(self.values, value);
        self.min_peaks.clear();
        self.max_peaks.clear();

        // Remove consecutive duplicate values to simplify peak detection
        let mut compressed = Vec::new();
        for value in &self.values {
            if compressed
                .last()
                .map_or(true, |last: &TelemetryValue| last.value != value.value)
            {
                compressed.push(value.clone());
            }
        }

        let delta_avg = compressed
            .windows(2)
            .map(|w| (w[1].value - w[0].value).abs())
            .sum::<f64>()
            / (self.values.len().saturating_sub(1) as f64);

        // recalc min/max
        for w in compressed.windows(3) {
            let [prev, current, next] = w else {
                continue;
            };
            let da = (current.value - prev.value).abs();
            let db = (current.value - next.value).abs();
            if da < delta_avg || db < delta_avg {
                continue;
            }
            if current.value < prev.value && current.value < next.value {
                self.min_peaks.push(current.clone());
            } else if current.value > prev.value && current.value > next.value {
                self.max_peaks.push(current.clone());
            }
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq)]
pub struct Position {
    pub seq_number: u32,
    pub timestamp: DateTime<Utc>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: i32,
    pub speed: u32,
    pub precision_bits: u32,
    pub precision_bounds: Vec<geo::Point>,
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
    pub hop_distance: Option<u32>,
    pub via_mqtt: bool,
    pub is_duplicate: bool,
}

// Public key variant
#[derive(Default, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub enum PublicKey {
    #[default]
    None,
    // Normal key: set while message's decoding
    Key(Key),
    // Key used by another node, set on nodes' list processing stage
    Compromised(Key),
}

#[derive(Default, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct NodeInfoExtended {
    pub timestamp: DateTime<Utc>,
    pub announced_node_id: String,
    pub long_name: String,
    pub short_name: String,
    pub pkey: PublicKey,
    pub is_licensed: bool,
    pub is_unmessagable: Option<bool>,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq)]
pub struct GatewayInfo {
    pub timestamp: DateTime<Utc>,
    pub rx_info: Option<StoreMeshRxInfo>,
    pub hop_limit: u32,
    pub hop_distance: Option<u32>,
    pub via_mqtt: bool,
    pub packet_id: u32,
}

impl From<&StoredMeshPacket> for GatewayInfo {
    fn from(stored_mesh_packet: &StoredMeshPacket) -> Self {
        let rx_info = if let Some(rx_info) = &stored_mesh_packet.header.rx {
            if rx_info.rx_rssi > RSSI_UPPER_THRESHOLD || rx_info.rx_rssi < RSSI_LOWER_THRESHOLD {
                None
            } else if rx_info.rx_snr > SNR_UPPER_THRESHOLD || rx_info.rx_snr < SNR_LOWER_THRESHOLD {
                None
            } else {
                Some(rx_info.clone())
            }
        } else {
            None
        };

        let hop_distance =
            if stored_mesh_packet.header.hop_start >= stored_mesh_packet.header.hop_limit {
                Some(stored_mesh_packet.header.hop_start - stored_mesh_packet.header.hop_limit)
            } else {
                None
            };

        Self {
            timestamp: stored_mesh_packet.store_timestamp,
            rx_info,
            hop_limit: stored_mesh_packet.header.hop_limit,
            hop_distance,
            via_mqtt: stored_mesh_packet.header.via_mqtt,
            packet_id: stored_mesh_packet.header.id,
        }
    }
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub extended_info_history: Vec<NodeInfoExtended>,
    pub position: Vec<Position>,
    pub assumed_position: Option<geo::Point>,
    pub telemetry: HashMap<TelemetryVariant, NodeTelemetry>,
    pub packet_statistics: Vec<NodePacket>,
    pub gateway_for: HashMap<NodeId, Vec<GatewayInfo>>,
    pub gatewayed_by: HashMap<NodeId, GatewayInfo>,
}

impl NodeInfo {
    fn push_telemetry(
        &mut self,
        timestamp: DateTime<Utc>,
        telemetry_variant: TelemetryVariant,
        value: f64,
    ) {
        let telemetry = TelemetryValue { timestamp, value };
        let telemetry_store = self.telemetry.entry(telemetry_variant).or_default();
        telemetry_store.push(telemetry);
    }

    fn update_using_data(
        &mut self,
        stored_timestamp: DateTime<Utc>,
        data: &meshtastic::Data,
        nodebook: &NodeBook,
        is_duplicate: bool,
    ) -> Result<meshtastic::PortNum, String> {
        match data.portnum() {
            meshtastic::PortNum::PositionApp => {
                let mesh_position = meshtastic::Position::decode(data.payload.as_slice())
                    .map_err(|e| e.to_string())?;

                if !is_duplicate {
                    let altitude = if let Some(altitude) = mesh_position.altitude {
                        altitude
                    } else if let Some(altitude) = mesh_position.altitude_hae {
                        altitude
                    } else if let Some(altitude) = mesh_position.altitude_geoidal_separation {
                        altitude
                    } else {
                        0
                    };

                    let mut latitude = mesh_position.latitude_i() as f64 * 1e-7;
                    let mut longitude = mesh_position.longitude_i() as f64 * 1e-7;
                    let point = geo::Point::new(longitude, latitude);

                    if let Some(zone_name) = nodebook.point_in_zone(point) {
                        log::info!("Skip point in zone id: {:?}", zone_name);
                    } else {
                        let timestamp = DateTime::from_timestamp(mesh_position.timestamp as i64, 0)
                            .unwrap_or(Default::default());

                        let precision_bounds = if mesh_position.precision_bits < 32 {
                            let fix = (1_u64 << (32 - mesh_position.precision_bits)) as f64 * 1e-7;

                            let c1 = geo::Point::new(longitude, latitude);
                            let c2 = geo::Point::new(longitude + fix, latitude + fix);
                            let center = geo::Rect::new(c1, c2).center();
                            (longitude, latitude) = center.x_y();

                            vec![c1, c2]
                        } else {
                            vec![]
                        };

                        let position = Position {
                            seq_number: mesh_position.seq_number,
                            timestamp,
                            latitude,
                            longitude,
                            altitude,
                            speed: mesh_position.ground_speed(),
                            precision_bits: mesh_position.precision_bits,
                            precision_bounds,
                        };

                        let position_unchanged = |previous: &Position, current: &Position| {
                            previous.latitude == current.latitude
                                && previous.longitude == current.longitude
                                && previous.altitude == current.altitude
                        };

                        if self.position.is_empty() {
                            self.position.push(position);
                        } else if position.timestamp == DateTime::<Utc>::default() {
                            if let Some(previous_position) = self.position.last() {
                                if !position_unchanged(previous_position, &position) {
                                    self.position.push(position);
                                }
                            } else {
                                self.position.push(position);
                            }
                        } else {
                            for (i, previous_position) in self.position.iter().rev().enumerate() {
                                if previous_position == &position {
                                    break;
                                }
                                if position_unchanged(previous_position, &position) {
                                    break;
                                }
                                if position.timestamp > previous_position.timestamp {
                                    self.position.insert(self.position.len() - i, position);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            meshtastic::PortNum::NodeinfoApp => {
                let user =
                    meshtastic::User::decode(data.payload.as_slice()).map_err(|e| e.to_string())?;

                let mut pkey = if user.public_key.len() > 0 {
                    PublicKey::Key(Key::try_from(user.public_key)?)
                } else {
                    PublicKey::None
                };

                if !is_duplicate {
                    if let Some(last_extended) = self.extended_info_history.last() {
                        if let PublicKey::Compromised(previous_key) = last_extended.pkey {
                            if PublicKey::Key(previous_key) == pkey {
                                pkey = PublicKey::Compromised(previous_key);
                            }
                        }
                    }

                    let node_info_extended = NodeInfoExtended {
                        timestamp: stored_timestamp,
                        announced_node_id: user.id.clone(),
                        long_name: user.long_name,
                        short_name: user.short_name,
                        pkey,
                        is_licensed: user.is_licensed,
                        is_unmessagable: user.is_unmessagable,
                    };

                    push_statistic!(self.extended_info_history, node_info_extended);
                }
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

                if !is_duplicate {
                    match telemetry.variant.ok_or(format!("Telemetry is empty"))? {
                        meshtastic::telemetry::Variant::DeviceMetrics(device_metrics) => {
                            if let Some(air_util_tx) = device_metrics.air_util_tx {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirUtilTx,
                                    air_util_tx as f64,
                                );
                            }
                            if let Some(channel_utilization) = device_metrics.channel_utilization {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::ChannelUtilization,
                                    channel_utilization as f64,
                                );
                            }
                            if let Some(voltage) = device_metrics.voltage {
                                if voltage.abs() == 0.0 {
                                    self.push_telemetry(
                                        timestamp,
                                        TelemetryVariant::Voltage,
                                        voltage as f64,
                                    );
                                    if let Some(battery_level) = device_metrics.battery_level {
                                        self.push_telemetry(
                                            timestamp,
                                            TelemetryVariant::BatteryLevel,
                                            battery_level as f64,
                                        );
                                    }
                                }
                            }
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
                                    TelemetryVariant::EnvironmentTemperature,
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
                        meshtastic::telemetry::Variant::AirQualityMetrics(air_quality_metrics) => {
                            if let Some(pm10_standard) = air_quality_metrics.pm10_standard {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirPM10Standard,
                                    pm10_standard as f64,
                                );
                            }
                            if let Some(pm25_standard) = air_quality_metrics.pm25_standard {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirPM25Standard,
                                    pm25_standard as f64,
                                );
                            }
                            if let Some(pm100_standard) = air_quality_metrics.pm100_standard {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirPM100Standard,
                                    pm100_standard as f64,
                                );
                            }
                            if let Some(pm10_env) = air_quality_metrics.pm10_environmental {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirPM10Environmental,
                                    pm10_env as f64,
                                );
                            }
                            if let Some(pm25_env) = air_quality_metrics.pm25_environmental {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirPM25Environmental,
                                    pm25_env as f64,
                                );
                            }
                            if let Some(pm100_env) = air_quality_metrics.pm100_environmental {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirPM100Environmental,
                                    pm100_env as f64,
                                );
                            }
                            if let Some(part_03um) = air_quality_metrics.particles_03um {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirParticles03um,
                                    part_03um as f64,
                                );
                            }
                            if let Some(part_05um) = air_quality_metrics.particles_05um {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirParticles05um,
                                    part_05um as f64,
                                );
                            }
                            if let Some(part_10um) = air_quality_metrics.particles_10um {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirParticles10um,
                                    part_10um as f64,
                                );
                            }
                            if let Some(part_25um) = air_quality_metrics.particles_25um {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirParticles25um,
                                    part_25um as f64,
                                );
                            }
                            if let Some(part_50um) = air_quality_metrics.particles_50um {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirParticles50um,
                                    part_50um as f64,
                                );
                            }
                            if let Some(part_100um) = air_quality_metrics.particles_100um {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirParticles100um,
                                    part_100um as f64,
                                );
                            }
                            if let Some(co2) = air_quality_metrics.co2 {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirCo2,
                                    co2 as f64,
                                );
                            }
                            if let Some(co2_temperature) = air_quality_metrics.co2_temperature {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirCo2Temperature,
                                    co2_temperature as f64,
                                );
                            }
                            if let Some(co2_humidity) = air_quality_metrics.co2_humidity {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirCo2Humidity,
                                    co2_humidity as f64,
                                );
                            }
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
                        meshtastic::telemetry::Variant::LocalStats(local_stats) => {
                            if local_stats.air_util_tx != 0.0 {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::AirUtilTx,
                                    local_stats.air_util_tx as f64,
                                );
                            }
                            if local_stats.channel_utilization != 0.0 {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::ChannelUtilization,
                                    local_stats.channel_utilization as f64,
                                );
                            }
                            log::info!("Telemetry::LocalStats from {} ignored", self.node_id);
                        }
                        meshtastic::telemetry::Variant::HealthMetrics(health_metrics) => {
                            log::info!(
                                "Telemetry::HealthMetrics from {} not ignored",
                                self.node_id
                            );
                            if let Some(heart_rate) = health_metrics.heart_bpm {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::HeartRate,
                                    heart_rate as f64,
                                );
                            }
                            if let Some(spo2) = health_metrics.sp_o2 {
                                self.push_telemetry(timestamp, TelemetryVariant::SpO2, spo2 as f64);
                            }
                            if let Some(temperature) = health_metrics.temperature {
                                self.push_telemetry(
                                    timestamp,
                                    TelemetryVariant::HealthTemperature,
                                    temperature as f64,
                                );
                            }
                        }
                        meshtastic::telemetry::Variant::HostMetrics(host_metrics) => {
                            self.push_telemetry(
                                timestamp,
                                TelemetryVariant::UptimeSeconds,
                                host_metrics.uptime_seconds as f64,
                            );

                            log::info!("Telemetry::HostMetrics from {} ignored", self.node_id);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(data.portnum())
    }

    pub fn update_as_gateway(&mut self, stored_mesh_packet: &StoredMeshPacket) {
        if self.node_id != stored_mesh_packet.header.from {
            let gateway_info = stored_mesh_packet.into();

            let list = self
                .gateway_for
                .entry(stored_mesh_packet.header.from)
                .or_insert(Default::default());

            push_statistic!(list, gateway_info);
        }
    }

    pub fn update(&mut self, stored_mesh_packet: &StoredMeshPacket, nodebook: &NodeBook) {
        let current_time = chrono::Utc::now();
        let timestamp = stored_mesh_packet.store_timestamp;
        let is_duplicate = self
            .packet_statistics
            .iter()
            .rev()
            .take_while(|v|
                // I believe that 30 minutes is enough to consider it a duplicate
                // Duplicate can be received as retranslation from another node
                // or from very slow MQTT
                v.timestamp > current_time - chrono::Duration::minutes(30))
            .find(|v| v.packet_id == stored_mesh_packet.header.id)
            .is_some();
        // TODO: move to perday_telemetry
        // self.push_telemetry(timestamp, TelemetryVariant::MeshPacket, 1);
        let packet_type = if let Some(data) = &stored_mesh_packet.data {
            match data {
                DataVariant::Encrypted(_) => NodePacketType::CannotDecrypt,
                DataVariant::Decrypted(_, data) => {
                    match self.update_using_data(timestamp, data, nodebook, is_duplicate) {
                        Ok(portnum) => NodePacketType::Normal(format!("{}", portnum.as_str_name())),
                        Err(e) => {
                            log::error!("Failed to update using data: {}", e);
                            NodePacketType::Error
                            // TODO: move to perday_telemetry
                            // self.push_telemetry(timestamp, TelemetryVariant::CorruptedPacket, 1);
                        }
                    }
                }

                DataVariant::DecryptError(_, _) => NodePacketType::Error,
            }
        } else {
            NodePacketType::Empty
            // TODO: move to perday_telemetry
            // self.push_telemetry(timestamp, TelemetryVariant::EmptyPackets, 1);
        };

        if let Some(gateway) = stored_mesh_packet.gateway {
            if gateway != stored_mesh_packet.header.from {
                self.gatewayed_by
                    .entry(gateway)
                    .and_modify(|v| *v = stored_mesh_packet.into())
                    .or_insert_with(|| stored_mesh_packet.into());
            }
        }

        let hop_distance =
            if stored_mesh_packet.header.hop_start >= stored_mesh_packet.header.hop_limit {
                Some(stored_mesh_packet.header.hop_start - stored_mesh_packet.header.hop_limit)
            } else {
                None
            };

        let packet = NodePacket {
            timestamp,
            packet_type,
            to: stored_mesh_packet.header.to,
            channel: stored_mesh_packet.header.channel,
            rx_info: stored_mesh_packet.header.rx.clone(),
            gateway: stored_mesh_packet.gateway,
            packet_id: stored_mesh_packet.header.id,
            hop_limit: stored_mesh_packet.header.hop_limit,
            hop_distance,
            via_mqtt: stored_mesh_packet.header.via_mqtt,
            is_duplicate,
        };

        push_statistic!(self.packet_statistics, packet);
    }
}

const RSSI_UPPER_THRESHOLD: i32 = 50;
const RSSI_LOWER_THRESHOLD: i32 = -200;
const SNR_UPPER_THRESHOLD: f32 = 30.0;
const SNR_LOWER_THRESHOLD: f32 = -200.0;
