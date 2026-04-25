use super::key::Key;
use std::fmt;
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Hash)]
pub struct Channel {
    pub name: Option<String>,
    pub key: Key,
    pub channel_hash: u32,
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
        let make_hex = |hash: u32| {
            let mut hex = String::with_capacity(10);
            write!(&mut hex, "{:02X}h", hash).unwrap();
            hex
        };

        let (name, channel_hash) = if let Some(ref name) = self.name {
            let chan_hash = Self::generate_hash(name.as_str(), self.key.as_bytes()) as u32;
            if chan_hash == self.channel_hash {
                (Some(name.clone()), None)
            } else {
                (Some(name.clone()), Some(make_hex(chan_hash)))
            }
        } else {
            (None, Some(make_hex(self.channel_hash)))
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
            let channel_hash = if channel_hash.ends_with("h") {
                u32::from_str_radix(&channel_hash[..channel_hash.len() - 1], 16)
                    .map_err(serde::de::Error::custom)?
            } else {
                u32::from_str_radix(&channel_hash, 16).map_err(serde::de::Error::custom)?
            };
            Ok(Channel {
                name: data.name,
                key: data.key,
                channel_hash,
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
            write!(f, "Channel([{:#x}] {})", self.channel_hash, name)
        } else {
            write!(f, "Channel([{:#x}])", self.channel_hash)
        }
    }
}

impl Channel {
    pub fn new_with_name(name: &str, key: Key) -> Self {
        let chan_no = Self::generate_hash(name, key.as_bytes()) as u32;

        Self {
            name: Some(name.to_string()),
            key,
            channel_hash: chan_no,
        }
    }

    pub fn new(channel_no: u32, key: Key) -> Self {
        Self {
            name: None,
            key,
            channel_hash: channel_no,
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
