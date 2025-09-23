use crate::{config::SoftNodeConfig, meshtastic};
use duration_string::DurationString;
use meshtastic_connect::keyring::{key::Key, node_id::NodeId};
use prost::Message;
use serde::{Deserialize, Serialize, de};
use std::time::Duration;

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct PublishPosition {
    pub(crate) interval: DurationString,
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    pub(crate) alt: i32,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct PublishNodeInfoOverride {
    pub(crate) node_id: Option<NodeId>,
    pub(crate) name: Option<String>,
    pub(crate) short_name: Option<String>,
    pub(crate) public_key: Option<Key>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct PublishNodeInfo {
    pub(crate) interval: DurationString,
    #[serde(default)]
    pub(crate) hardware: HardwareModel,
    #[serde(default)]
    pub(crate) role: Role,
    #[serde(default)]
    pub(crate) force: PublishNodeInfoOverride,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) struct PublishText {
    pub(crate) interval: DurationString,
    #[serde(default)]
    pub(crate) text: String,
}

#[derive(Debug, PartialEq, Clone, Copy, Eq, Ord, PartialOrd)]
pub(crate) struct Role(meshtastic::config::device_config::Role);

impl Into<meshtastic::config::device_config::Role> for Role {
    fn into(self) -> meshtastic::config::device_config::Role {
        self.0
    }
}

impl Into<i32> for Role {
    fn into(self) -> i32 {
        self.0.into()
    }
}

impl Default for Role {
    fn default() -> Self {
        Role(meshtastic::config::device_config::Role::Client)
    }
}

impl Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.as_str_name())
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match meshtastic::config::device_config::Role::from_str_name(&s) {
            Some(hwmodel) => Ok(Role(hwmodel)),
            None => Err(de::Error::custom(format!("Invalid role: {:?}", s))),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Eq, Ord, PartialOrd)]
pub(crate) struct HardwareModel(meshtastic::HardwareModel);

impl Into<meshtastic::HardwareModel> for HardwareModel {
    fn into(self) -> meshtastic::HardwareModel {
        self.0
    }
}

impl Into<i32> for HardwareModel {
    fn into(self) -> i32 {
        self.0.into()
    }
}

impl Default for HardwareModel {
    fn default() -> Self {
        HardwareModel(meshtastic::HardwareModel::AndroidSim)
    }
}

impl Serialize for HardwareModel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.as_str_name())
    }
}

impl<'de> Deserialize<'de> for HardwareModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match meshtastic::HardwareModel::from_str_name(&s) {
            Some(hwmodel) => Ok(HardwareModel(hwmodel)),
            None => Err(de::Error::custom(format!(
                "Invalid hardware model: {:?}",
                s
            ))),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub(crate) enum Publish {
    NodeInfo(PublishNodeInfo),
    Position(PublishPosition),
    Text(PublishText),
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
            Publish::Text(text) => text.interval(),
        }
    }

    fn pack_to_data(&self, soft_node: &SoftNodeConfig) -> (meshtastic::PortNum, Vec<u8>) {
        match self {
            Publish::NodeInfo(info) => info.pack_to_data(soft_node),
            Publish::Position(pos) => pos.pack_to_data(soft_node),
            Publish::Text(text) => text.pack_to_data(soft_node),
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
        let pkey = if let Some(pkey) = self.force.public_key {
            pkey.as_bytes().to_vec()
        } else {
            soft_node.public_key.as_bytes().to_vec()
        };

        let node_id = if let Some(node_id) = self.force.node_id {
            node_id
        } else {
            soft_node.node_id.into()
        };

        let long_name = if let Some(ref long_name) = self.force.name {
            long_name.clone()
        } else {
            soft_node.name.clone()
        };

        let short_name = if let Some(ref short_name) = self.force.short_name {
            short_name.clone()
        } else {
            soft_node.short_name.clone()
        };

        let node_info = meshtastic::User {
            id: node_id.into(),
            long_name,
            short_name,
            hw_model: self.hardware.into(),
            is_licensed: false,
            role: self.role.into(),
            public_key: pkey,
            is_unmessagable: Some(false),
            ..Default::default()
        };

        (meshtastic::PortNum::NodeinfoApp, node_info.encode_to_vec())
    }
}

impl Publishable for PublishText {
    fn interval(&self) -> Duration {
        self.interval.into()
    }

    fn pack_to_data(&self, _: &SoftNodeConfig) -> (meshtastic::PortNum, Vec<u8>) {
        (
            meshtastic::PortNum::TextMessageApp,
            self.text.encode_to_vec(),
        )
    }
}
