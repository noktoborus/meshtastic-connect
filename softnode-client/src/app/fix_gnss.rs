use std::collections::{HashMap, hash_map::Entry};

use meshtastic_connect::keyring::node_id::NodeId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FixGnss {
    #[serde(rename = "Lat")]
    pub latitude: f64,
    #[serde(rename = "Lon")]
    pub longitude: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IgnoreZone {
    #[serde(rename = "Center")]
    pub center: FixGnss,
    #[serde(rename = "Radius")]
    pub radius: f64,
}

// FixGnssLibrary is stored separately from the map
// because it is simple way to load persistent data
// between app's updates
// e.g. map or other non-important data can change
// and changes should not affect the FixGnssLibrary
// like Keyring
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FixGnssLibrary {
    ignore_zones: HashMap<String, IgnoreZone>,
    fixed_nodes: HashMap<NodeId, FixGnss>,
}

impl Default for FixGnssLibrary {
    fn default() -> Self {
        let mut new = Self {
            ignore_zones: HashMap::new(),
            fixed_nodes: HashMap::new(),
        };
        new.add_zone(
            "Null Island",
            IgnoreZone {
                center: FixGnss {
                    latitude: 0.0,
                    longitude: 0.0,
                },
                radius: 1.0,
            },
        );

        new
    }
}

impl FixGnssLibrary {
    pub fn list_zones(&self) -> Vec<(String, &IgnoreZone)> {
        self.ignore_zones
            .iter()
            .map(|(name, zone)| (name.clone(), zone))
            .collect::<Vec<_>>()
    }

    pub fn add_zone(&mut self, name: &str, zone: IgnoreZone) {
        self.ignore_zones.insert(name.to_string(), zone);
    }

    pub fn remove_zone(&mut self, name: &str) {
        self.ignore_zones.remove(name);
    }

    pub fn entry(&mut self, key: NodeId) -> Entry<'_, NodeId, FixGnss> {
        self.fixed_nodes.entry(key)
    }

    pub fn get(&self, key: &NodeId) -> Option<&FixGnss> {
        self.fixed_nodes.get(key)
    }

    pub fn remove(&mut self, key: &NodeId) {
        self.fixed_nodes.remove(key);
    }
}
