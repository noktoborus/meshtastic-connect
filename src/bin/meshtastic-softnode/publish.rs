use crate::{config::SoftNodeConfig, meshtastic};
use duration_string::DurationString;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct PublishPosition {
    pub(crate) interval: DurationString,
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    pub(crate) alt: i32,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct PublishNodeInfo {
    pub(crate) interval: DurationString,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) enum Publish {
    NodeInfo(PublishNodeInfo),
    Position(PublishPosition),
}

pub(crate) trait Publishable {
    fn interval(&self) -> Duration;
    fn pack_to_data(&self, soft_node: &SoftNodeConfig) -> (meshtastic::PortNum, Vec<u8>);
}

impl Publishable for Publish {
    fn interval(&self) -> Duration {
        match self {
            Publish::NodeInfo(info) => info.interval(),
            Publish::Position(pos) => pos.interval(),
        }
    }

    fn pack_to_data(&self, soft_node: &SoftNodeConfig) -> (meshtastic::PortNum, Vec<u8>) {
        match self {
            Publish::NodeInfo(info) => info.pack_to_data(soft_node),
            Publish::Position(pos) => pos.pack_to_data(soft_node),
        }
    }
}

impl Publishable for PublishPosition {
    fn interval(&self) -> Duration {
        self.interval.into()
    }

    fn pack_to_data(&self, _soft_node: &SoftNodeConfig) -> (meshtastic::PortNum, Vec<u8>) {
        let position = meshtastic::Position {
            latitude_i: Some((self.lat / 1e-7).round() as i32),
            longitude_i: Some((self.lon / 1e-7).round() as i32),
            altitude_hae: Some(self.alt),
            location_source: meshtastic::position::LocSource::LocManual.into(),
            altitude_source: meshtastic::position::AltSource::AltManual.into(),
            timestamp: chrono::Utc::now().timestamp() as u32,
            next_update: self.interval.as_secs() as u32,
            ..Default::default()
        };

        (meshtastic::PortNum::PositionApp, position.encode_to_vec())
    }
}

impl Publishable for PublishNodeInfo {
    fn interval(&self) -> Duration {
        self.interval.into()
    }

    fn pack_to_data(&self, soft_node: &SoftNodeConfig) -> (meshtastic::PortNum, Vec<u8>) {
        let node_info = meshtastic::User {
            id: soft_node.node_id.into(),
            long_name: soft_node.name.clone(),
            short_name: soft_node.short_name.clone(),
            hw_model: meshtastic::HardwareModel::AndroidSim.into(),
            is_licensed: false,
            role: meshtastic::config::device_config::Role::ClientHidden.into(),
            public_key: soft_node.public_key.as_bytes().to_vec(),
            is_unmessagable: Some(false),
            ..Default::default()
        };

        (meshtastic::PortNum::NodeinfoApp, node_info.encode_to_vec())
    }
}
