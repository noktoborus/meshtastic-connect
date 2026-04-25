use super::key::Key;
use std::fmt;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Hash, Copy)]
pub struct ChannelHash(u32);

impl serde::Serialize for ChannelHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for ChannelHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.try_into().map_err(serde::de::Error::custom)
    }
}

impl TryFrom<&str> for ChannelHash {
    type Error = std::num::ParseIntError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.ends_with("h") {
            Ok(u32::from_str_radix(&s[..s.len() - 1], 16)?.into())
        } else {
            Ok(u32::from_str_radix(s, 16)?.into())
        }
    }
}

impl TryFrom<String> for ChannelHash {
    type Error = std::num::ParseIntError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.as_str().try_into()
    }
}

impl ChannelHash {
    pub fn new(value: u32) -> Self {
        ChannelHash(value)
    }
}

impl PartialEq<u32> for ChannelHash {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl fmt::Display for ChannelHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X}h", self.0)
    }
}

impl From<u32> for ChannelHash {
    fn from(value: u32) -> Self {
        ChannelHash(value)
    }
}

impl From<ChannelHash> for u32 {
    fn from(value: ChannelHash) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Hash)]
pub struct Channel {
    pub name: Option<String>,
    pub key: Key,
    pub channel_hash: ChannelHash,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct SerdeChannelHelper {
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(rename = "ChannelHash", skip_serializing_if = "Option::is_none")]
    pub channel_hash: Option<String>,
    #[serde(rename = "SharedKey")]
    key: Key,
}

impl serde::Serialize for Channel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (name, channel_hash) = if let Some(ref name) = self.name {
            let chan_hash = Self::generate_hash(name.as_str(), self.key.as_bytes()) as u32;
            if chan_hash == self.channel_hash.into() {
                (Some(name.clone()), None)
            } else {
                (Some(name.clone()), Some(chan_hash.to_string()))
            }
        } else {
            (None, Some(self.channel_hash.to_string()))
        };

        let data = SerdeChannelHelper {
            name,
            key: self.key.clone(),
            channel_hash,
        };
        data.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Channel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let data = SerdeChannelHelper::deserialize(deserializer)?;

        if let Some(channel_hash) = data.channel_hash {
            let channel_hash = channel_hash.try_into().map_err(serde::de::Error::custom)?;
            Ok(Channel {
                name: data.name,
                key: data.key,
                channel_hash: channel_hash,
            })
        } else if let Some(name) = data.name {
            Ok(Channel::new_with_name(&name, data.key))
        } else {
            Err(serde::de::Error::missing_field(
                "ChannelHash or Name is required",
            ))
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref name) = self.name {
            write!(f, "Channel([{}] {})", self.channel_hash, name)
        } else {
            write!(f, "Channel([{}])", self.channel_hash)
        }
    }
}

impl Channel {
    pub fn new_with_name(name: &str, key: Key) -> Self {
        let chan_no = Self::generate_hash(name, key.as_bytes()) as u32;

        Self {
            name: Some(name.to_string()),
            key,
            channel_hash: chan_no.into(),
        }
    }

    pub fn new(channel_hash: ChannelHash, key: Key) -> Self {
        Self {
            name: None,
            key,
            channel_hash,
        }
    }

    fn xor_hash(data: &[u8]) -> u8 {
        data.iter().fold(0u8, |acc, &b| acc ^ b)
    }

    fn generate_hash(name: &str, key: &[u8]) -> i16 {
        if key.is_empty() {
            -1
        } else {
            let mut h = Self::xor_hash(name.as_bytes());
            h ^= Self::xor_hash(key);
            h as i16
        }
    }
}
