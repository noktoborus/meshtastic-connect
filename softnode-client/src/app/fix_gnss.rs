use std::collections::{HashMap, hash_map::Entry};

use meshtastic_connect::keyring::node_id::NodeId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FixGnss {
    #[serde(rename = "NodeId")]
    pub node_id: NodeId,
    #[serde(rename = "Lat")]
    pub latitude: f64,
    #[serde(rename = "Lon")]
    pub longitude: f64,
}

#[derive(Debug, Clone, Default)]
pub struct FixGnssLibrary(HashMap<NodeId, FixGnss>);

impl serde::Serialize for FixGnssLibrary {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.values().collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for FixGnssLibrary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let vec: Vec<FixGnss> = Vec::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for item in vec {
            map.insert(item.node_id, item);
        }
        Ok(FixGnssLibrary(map))
    }
}

impl FixGnssLibrary {
    pub fn entry(&mut self, key: NodeId) -> Entry<'_, NodeId, FixGnss> {
        self.0.entry(key)
    }

    pub fn get(&self, key: &NodeId) -> Option<&FixGnss> {
        self.0.get(key)
    }

    pub fn remove(&mut self, key: &NodeId) {
        self.0.remove(key);
    }
}
