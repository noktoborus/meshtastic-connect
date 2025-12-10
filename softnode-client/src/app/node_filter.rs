use std::collections::{HashMap, HashSet, hash_map::Values};

use egui::{Color32, RichText};
use meshtastic_connect::keyring::{key::Key, node_id::NodeId};
use walkers::{lat_lon, lon_lat};

use crate::app::{
    byte_node_id::ByteNodeId,
    data::{NodeInfo, PublicKey, TelemetryVariant},
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum FilterVariant {
    Generic(String),
    PublicPkey(Key),
    ByteNodeId(ByteNodeId),
    NodeId(NodeId),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize, Hash)]
enum StaticFilterVariant {
    CompromisedPkey,
    IsLicensed,
    IsUnmessagable,
    HasEnvironmentTelemetry,
    HasDeviceTelemetry,
    HasTracks,
    HasPosition,
    BoundingBox,
}

impl StaticFilterVariant {
    pub fn matches(&self, bbox: &[walkers::Position; 2], node_info: &NodeInfo) -> bool {
        let device_telemetry = [
            TelemetryVariant::BatteryLevel,
            TelemetryVariant::AirUtilTx,
            TelemetryVariant::ChannelUtilization,
            TelemetryVariant::Voltage,
        ];
        match self {
            StaticFilterVariant::CompromisedPkey => {}
            StaticFilterVariant::IsLicensed => {}
            StaticFilterVariant::IsUnmessagable => {}
            StaticFilterVariant::HasEnvironmentTelemetry => {
                for (variant, telemetry) in node_info.telemetry.iter() {
                    if device_telemetry.contains(variant) {
                        continue;
                    }
                    if telemetry.values.len() > 0 {
                        return true;
                    }
                }
                return false;
            }
            StaticFilterVariant::HasTracks => {
                return node_info.position.len() > 1;
            }
            StaticFilterVariant::HasPosition => {
                return node_info.position.len() > 0;
            }
            StaticFilterVariant::BoundingBox => {
                if let Some(position) = node_info.assumed_position.or(node_info
                    .position
                    .last()
                    .map(|v| lon_lat(v.longitude, v.latitude)))
                {
                    let p1 = bbox[0];
                    let p2 = bbox[1];

                    if position.x() < p1.x()
                        && position.y() > p1.y()
                        && position.x() > p2.x()
                        && position.y() < p2.y()
                    {
                        return true;
                    }
                }
                return false;
            }
            StaticFilterVariant::HasDeviceTelemetry => {
                for (variant, telemetry) in node_info.telemetry.iter() {
                    if device_telemetry.contains(variant) && telemetry.values.len() > 0 {
                        return true;
                    }
                }
                return false;
            }
        }

        if let Some(extended) = node_info.extended_info_history.last() {
            match self {
                StaticFilterVariant::CompromisedPkey => {
                    return matches!(extended.pkey, PublicKey::Compromised(_));
                }
                StaticFilterVariant::IsLicensed => return extended.is_licensed,
                StaticFilterVariant::IsUnmessagable => {
                    return Some(true) == extended.is_unmessagable;
                }
                StaticFilterVariant::HasEnvironmentTelemetry => {}
                StaticFilterVariant::HasTracks => {}
                StaticFilterVariant::HasPosition => {}
                StaticFilterVariant::BoundingBox => {}
                StaticFilterVariant::HasDeviceTelemetry => {}
            }
        }

        return false;
    }
}

impl FilterVariant {
    pub fn matches(&self, node_info: &NodeInfo) -> bool {
        match self {
            FilterVariant::Generic(string) => {
                if node_info
                    .node_id
                    .to_string()
                    .to_lowercase()
                    .contains(string)
                {
                    return true;
                }
                /* drop down to check extended info */
            }
            FilterVariant::PublicPkey(_key) => {}
            FilterVariant::ByteNodeId(byte_node_id) => return *byte_node_id == node_info.node_id,
            FilterVariant::NodeId(node_id) => return *node_id == node_info.node_id,
        }

        if let Some(extended) = node_info.extended_info_history.last() {
            match self {
                FilterVariant::Generic(string) => {
                    return extended.short_name.to_lowercase().contains(string)
                        || extended.long_name.to_lowercase().contains(string);
                }
                FilterVariant::PublicPkey(key) => match extended.pkey {
                    PublicKey::None => return false,
                    PublicKey::Key(node_key) => return *key == node_key,
                    PublicKey::Compromised(node_key) => return *key == node_key,
                },
                FilterVariant::ByteNodeId(_byte_node_id) => {}
                FilterVariant::NodeId(_node_id) => {}
            }
        }

        return false;
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct NodeFilter {
    filter_parts: Vec<(FilterVariant, bool)>,
    static_filter: HashSet<StaticFilterVariant>,
    filter_origin: Option<String>,
    // Bounding box for filtering nodes based on their positions
    bbox: [walkers::Position; 2],
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            filter_parts: Vec::new(),
            static_filter: HashSet::new(),
            filter_origin: None,
            bbox: [lat_lon(86.0, -180.0), lat_lon(-86.0, 180.0)],
        }
    }
}

impl NodeFilter {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn matches(&self, node: &NodeInfo) -> bool {
        for (filter_part, enabled) in &self.filter_parts {
            if *enabled {
                if !filter_part.matches(node) {
                    return false;
                }
            }
        }

        for static_filter in &self.static_filter {
            if !static_filter.matches(&self.bbox, node) {
                return false;
            }
        }

        true
    }

    // Set new filter's string and parse to filter parts
    pub fn update_filter(&mut self, filter: &str) {
        if let Some(ref origin) = self.filter_origin {
            if origin == filter {
                return;
            }
        }
        self.filter_origin = Some(filter.to_string());
        self.filter_parts.clear();
        for unparsed_part in filter.split_whitespace() {
            if let Ok(pkey) = Key::try_from(unparsed_part) {
                self.filter_parts
                    .push((FilterVariant::PublicPkey(pkey), true));
            } else if unparsed_part.starts_with("!*")
                && unparsed_part.len() <= 2 + 2 /* means: '!*' + '<2 bytes of node id's hex>' */
                && let Ok(byte_node_id) = ByteNodeId::try_from(&unparsed_part[2..])
            {
                self.filter_parts
                    .push((FilterVariant::ByteNodeId(byte_node_id), true));
            } else if unparsed_part.starts_with("!")
                && unparsed_part.len() <= 8 + 1 /* means: '!' + '<8 bytes of node id>' */
                && let Ok(node_id) = NodeId::try_from(&unparsed_part[1..])
            {
                self.filter_parts
                    .push((FilterVariant::NodeId(node_id), true));
            } else {
                self.filter_parts.push((
                    FilterVariant::Generic(unparsed_part.to_string().to_lowercase()),
                    true,
                ));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("🔍");
            for (filter_part, enabled) in self.filter_parts.iter_mut() {
                match filter_part {
                    FilterVariant::PublicPkey(pkey) => {
                        ui.selectable_label(*enabled, format!("pkey:{}", pkey))
                    }
                    FilterVariant::NodeId(node_id) => {
                        ui.selectable_label(*enabled, format!("nid:{}", node_id))
                    }
                    FilterVariant::ByteNodeId(byte_node_id) => {
                        ui.selectable_label(*enabled, format!("rnid:{}", byte_node_id))
                    }
                    FilterVariant::Generic(generic) => {
                        ui.selectable_label(*enabled, format!("{}", generic))
                    }
                }
                .clicked()
                .then(|| {
                    *enabled = !*enabled;
                });
            }
            let static_filter = [
                (
                    StaticFilterVariant::CompromisedPkey,
                    RichText::new("🔒").color(Color32::YELLOW),
                    "Filter by compromised public key",
                ),
                (
                    StaticFilterVariant::IsLicensed,
                    RichText::new("🖹").color(Color32::LIGHT_BLUE),
                    "Search with enabled `is_licensed` flag",
                ),
                (
                    StaticFilterVariant::IsUnmessagable,
                    RichText::new("🚫").color(Color32::LIGHT_RED),
                    "Filter by `unmessagable` flag",
                ),
                (
                    StaticFilterVariant::HasEnvironmentTelemetry,
                    RichText::new("Environment"),
                    "Node has environment telemetry",
                ),
                (
                    StaticFilterVariant::HasDeviceTelemetry,
                    RichText::new("Device's Telemetry"),
                    "Node has environment telemetry",
                ),
                (
                    StaticFilterVariant::HasTracks,
                    RichText::new("Tracks"),
                    "Node has tracks (number of positions > 1)",
                ),
                (
                    StaticFilterVariant::HasPosition,
                    RichText::new("Position"),
                    "Node has position (number of positions > 0)",
                ),
                (
                    StaticFilterVariant::BoundingBox,
                    RichText::new(format!(
                        "[{:.6}, {:.6}, {:.6}, {:.6}]",
                        self.bbox[0].y(),
                        self.bbox[0].x(),
                        self.bbox[1].y(),
                        self.bbox[1].x(),
                    )),
                    "Filter by bounding box",
                ),
            ];

            for (filter, label, hint) in static_filter {
                let enabled = self.static_filter.contains(&filter);
                if ui
                    .selectable_label(enabled, label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    if enabled {
                        self.static_filter.remove(&filter);
                    } else {
                        self.static_filter.insert(filter);
                    };
                }
            }
        });
    }

    // Get iterator &'a filterdes
    pub fn filter_for<'a>(
        &'a self,
        nodes: &'a HashMap<NodeId, NodeInfo>,
    ) -> NodeFilterIterator<'a> {
        NodeFilterIterator {
            nodes,
            nodes_iterator: nodes.values(),

            filter: self,
        }
    }

    // Set bounding box for filtering nodes based on position
    pub fn set_bbox(&mut self, bbox: [walkers::Position; 2]) {
        self.bbox = bbox;
    }
}

#[derive(Clone)]
pub struct NodeFilterIterator<'a> {
    // Direct access to NodeInfo by NodeId
    pub nodes: &'a HashMap<NodeId, NodeInfo>,
    nodes_iterator: Values<'a, NodeId, NodeInfo>,
    filter: &'a NodeFilter,
}

impl<'a> NodeFilterIterator<'a> {
    pub fn matches(&self, node: &NodeInfo) -> bool {
        self.filter.matches(node)
    }
}

impl<'a> Iterator for NodeFilterIterator<'a> {
    type Item = &'a NodeInfo;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.nodes_iterator.next() {
            if self.filter.matches(node) {
                return Some(node);
            }
        }
        None
    }
}
