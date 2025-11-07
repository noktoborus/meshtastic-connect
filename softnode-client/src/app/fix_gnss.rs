use std::collections::{HashMap, hash_map::Entry};

use geo::{Distance, Haversine, Point};
use meshtastic_connect::keyring::node_id::NodeId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Copy, PartialEq)]
pub struct FixGnss {
    #[serde(rename = "Lat")]
    pub latitude: f64,
    #[serde(rename = "Lon")]
    pub longitude: f64,
}

impl FixGnss {
    pub fn from_lat_lon(latitude: f64, longitude: f64) -> Self {
        FixGnss {
            latitude,
            longitude,
        }
    }

    pub fn from_lon_lat(longitude: f64, latitude: f64) -> Self {
        FixGnss {
            latitude,
            longitude,
        }
    }
}

impl From<FixGnss> for geo::Point<f64> {
    fn from(fix: FixGnss) -> Self {
        Point::new(fix.longitude, fix.latitude)
    }
}

impl From<geo::Point<f64>> for FixGnss {
    fn from(point: geo::Point<f64>) -> Self {
        FixGnss {
            latitude: point.y(),
            longitude: point.x(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IgnoreZone {
    #[serde(rename = "Title")]
    pub name: String,
    #[serde(rename = "Center")]
    pub center: FixGnss,
    #[serde(rename = "Radius")]
    pub radius_meters: f32,
}

impl IgnoreZone {
    pub fn contains(&self, point: &FixGnss) -> bool {
        let distance = Haversine.distance(
            Point::new(self.center.longitude, self.center.latitude),
            Point::new(point.longitude, point.latitude),
        );
        distance <= self.radius_meters as f64
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Hash, PartialEq, Eq)]
pub struct ZoneId(u32);

impl ZoneId {
    pub fn next(&mut self) -> Self {
        let id = self.0;
        self.0 += 1;
        ZoneId(id)
    }
}

// FixGnssLibrary is stored separately from the map
// because it is simple way to load persistent data
// between app's updates
// e.g. map or other non-important data can change
// and changes should not affect the FixGnssLibrary
// like Keyring
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FixGnssLibrary {
    zone_id_generator: ZoneId,
    ignore_zones: HashMap<ZoneId, IgnoreZone>,
    fixed_nodes: HashMap<NodeId, FixGnss>,
}

impl Default for FixGnssLibrary {
    fn default() -> Self {
        let mut new = Self {
            zone_id_generator: ZoneId(0),
            ignore_zones: HashMap::new(),
            fixed_nodes: HashMap::new(),
        };
        new.zone_add(IgnoreZone {
            name: "Null Island".into(),
            center: FixGnss {
                latitude: 0.0,
                longitude: 0.0,
            },
            radius_meters: 500.0,
        });

        new
    }
}

impl FixGnssLibrary {
    pub fn point_in_zone(&self, point: &FixGnss) -> Option<ZoneId> {
        self.ignore_zones
            .iter()
            .find(|(_, zone)| zone.contains(point))
            .map_or(None, |(id, _)| Some(*id))
    }

    pub fn zones_list_mut(&mut self) -> Vec<(ZoneId, &mut IgnoreZone)> {
        self.ignore_zones
            .iter_mut()
            .map(|(id, zone)| (*id, zone))
            .collect()
    }

    pub fn zones_list(&self) -> Vec<(ZoneId, &IgnoreZone)> {
        self.ignore_zones
            .iter()
            .map(|(id, zone)| (*id, zone))
            .collect()
    }

    pub fn zone_add(&mut self, zone: IgnoreZone) -> ZoneId {
        let next_id = self.zone_id_generator.next();
        self.ignore_zones.insert(next_id, zone);
        next_id
    }

    pub fn zone_get_mut(&mut self, key: &ZoneId) -> Option<&mut IgnoreZone> {
        self.ignore_zones.get_mut(key)
    }

    pub fn zone(&mut self, key: ZoneId) -> Entry<'_, ZoneId, IgnoreZone> {
        self.ignore_zones.entry(key)
    }

    pub fn remove_zone(&mut self, id: ZoneId) {
        self.ignore_zones.remove(&id);
    }

    pub fn node(&mut self, key: NodeId) -> Entry<'_, NodeId, FixGnss> {
        self.fixed_nodes.entry(key)
    }

    pub fn node_get(&self, key: &NodeId) -> Option<&FixGnss> {
        self.fixed_nodes.get(key)
    }

    pub fn node_remove(&mut self, key: &NodeId) {
        self.fixed_nodes.remove(key);
    }
}
