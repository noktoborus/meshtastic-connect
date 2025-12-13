use std::{
    collections::{HashMap, HashSet, hash_map::Values},
    sync::Arc,
};

use base64::{Engine, engine::general_purpose};
use chrono::Duration;
use egui::{Color32, RichText};
use meshtastic_connect::keyring::{key::Key, node_id::NodeId};
use walkers::lon_lat;

use crate::app::{
    byte_node_id::ByteNodeId,
    data::{NodeInfo, PublicKey, TelemetryVariant},
    node_book::{NodeAnnotation, NodeBook},
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum FilterVariant {
    Generic(String),
    PublicPkey(Key),
    ByteNodeId(ByteNodeId),
    NodeId(NodeId),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize, Hash)]
enum PublicKeyVariant {
    None,
    Compromised,
    Valid,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize, Hash)]
enum StaticFilterVariant {
    PublicKey(PublicKeyVariant),
    IsLicensed(bool),
    IsUnmessagable,
    HasEnvironmentTelemetry,
    HasDeviceTelemetry,
    HasTracks,
    HasPosition,
    HasNoPosition,
    BoundingBox,
    IsGateway,
    LastSeen(Duration),
}

impl StaticFilterVariant {
    pub fn matches(
        &self,
        bbox: &Option<[walkers::Position; 2]>,
        node_info: &NodeInfo,
        node_annotation: Option<&NodeAnnotation>,
        ignore_extended: bool,
    ) -> bool {
        let device_telemetry = [
            TelemetryVariant::BatteryLevel,
            TelemetryVariant::AirUtilTx,
            TelemetryVariant::ChannelUtilization,
            TelemetryVariant::Voltage,
        ];
        match self {
            StaticFilterVariant::PublicKey(_) => {}
            StaticFilterVariant::IsLicensed(_) => {}
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
                return node_info.position.len() == 1;
            }
            StaticFilterVariant::HasNoPosition => {
                return node_info.position.len() == 0;
            }
            StaticFilterVariant::BoundingBox => {
                if let Some(bbox) = bbox {
                    if let Some(position) =
                        node_annotation.map(|a| a.position).flatten().or(node_info
                            .assumed_position
                            .or(node_info
                                .position
                                .last()
                                .map(|v| lon_lat(v.longitude, v.latitude))))
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
                } else {
                    /* no bbox info is passed */
                    return true;
                }
            }
            StaticFilterVariant::HasDeviceTelemetry => {
                for (variant, telemetry) in node_info.telemetry.iter() {
                    if device_telemetry.contains(variant) && telemetry.values.len() > 0 {
                        return true;
                    }
                }
                return false;
            }
            StaticFilterVariant::LastSeen(duration) => {
                let now = chrono::Utc::now();
                if let Some(last) = node_info.packet_statistics.last() {
                    return now - last.timestamp < *duration;
                }
                return false;
            }
            StaticFilterVariant::IsGateway => {
                return node_info.gateway_for.len() != 0;
            }
        }

        if ignore_extended {
            return true;
        }

        if let Some(extended) = node_info.extended_info_history.last() {
            match self {
                StaticFilterVariant::PublicKey(variant) => match variant {
                    PublicKeyVariant::None => return matches!(extended.pkey, PublicKey::None),
                    PublicKeyVariant::Compromised => {
                        return matches!(extended.pkey, PublicKey::Compromised(_));
                    }
                    PublicKeyVariant::Valid => return matches!(extended.pkey, PublicKey::Key(_)),
                },
                StaticFilterVariant::IsLicensed(variant) => {
                    return *variant == extended.is_licensed;
                }
                StaticFilterVariant::IsUnmessagable => {
                    return Some(true) == extended.is_unmessagable;
                }
                StaticFilterVariant::HasEnvironmentTelemetry => {}
                StaticFilterVariant::HasTracks => {}
                StaticFilterVariant::HasPosition => {}
                StaticFilterVariant::BoundingBox => {}
                StaticFilterVariant::HasDeviceTelemetry => {}
                StaticFilterVariant::LastSeen(_) => {}
                StaticFilterVariant::IsGateway => {}
                StaticFilterVariant::HasNoPosition => {}
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
enum KnownNodesFilter {
    Unspecified,
    Known,
    Unknown,
    // Favorite
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct NodeFilter {
    known_nodes_filter: KnownNodesFilter,
    filter_parts: Vec<(FilterVariant, bool)>,
    static_filter: HashSet<StaticFilterVariant>,
    filter_origin: Option<String>,
    // Bounding box for filtering nodes based on their positions
    bbox: Option<[walkers::Position; 2]>,
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            known_nodes_filter: KnownNodesFilter::Unspecified,
            filter_parts: Vec::new(),
            static_filter: HashSet::new(),
            filter_origin: None,
            bbox: None,
        }
    }
}

impl NodeFilter {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn matches(&self, node_info: &NodeInfo, node_annotation: Option<&NodeAnnotation>) -> bool {
        let ignore_extended = match self.known_nodes_filter {
            KnownNodesFilter::Unspecified => false,
            KnownNodesFilter::Known => {
                if node_info.extended_info_history.is_empty() {
                    return false;
                }
                false
            }
            KnownNodesFilter::Unknown => {
                if !node_info.extended_info_history.is_empty() {
                    return false;
                }
                true
            }
        };

        for (filter_part, enabled) in &self.filter_parts {
            if *enabled {
                if !filter_part.matches(node_info) {
                    return false;
                }
            }
        }

        for static_filter in &self.static_filter {
            if !static_filter.matches(&self.bbox, node_info, node_annotation, ignore_extended) {
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
            if let Ok(base64_decoded) = general_purpose::STANDARD.decode(unparsed_part) {
                if base64_decoded.len() == 32 || base64_decoded.len() == 16 {
                    if let Ok(pkey) = Key::try_from(base64_decoded) {
                        self.filter_parts
                            .push((FilterVariant::PublicPkey(pkey), true));
                        continue;
                    }
                }
            }
            if unparsed_part.starts_with("!*")
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
        });
        ui.horizontal_wrapped(|ui| {
            let show_extended = match self.known_nodes_filter {
                KnownNodesFilter::Unspecified => {
                    if ui
                        .selectable_label(false, RichText::new("☆"))
                        .on_hover_text("Switch on filter by known nodes")
                        .clicked()
                    {
                        self.known_nodes_filter = KnownNodesFilter::Known;
                    }
                    true
                }
                KnownNodesFilter::Known => {
                    if ui
                        .selectable_label(true, RichText::new("☆").color(Color32::LIGHT_GREEN))
                        .on_hover_text("Filter only by known nodes: node info is received")
                        .clicked()
                    {
                        self.known_nodes_filter = KnownNodesFilter::Unknown;
                    }
                    true
                }
                KnownNodesFilter::Unknown => {
                    if ui
                        .selectable_label(true, RichText::new("☆").color(Color32::LIGHT_RED))
                        .on_hover_text("Show only if no node info received")
                        .clicked()
                    {
                        self.known_nodes_filter = KnownNodesFilter::Unspecified;
                    }
                    false
                }
            };

            let mut static_filters_vary = vec![
                vec![
                    (
                        None,
                        Arc::new(RichText::new("🕒")),
                        "Switch on filter by last seen time".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::LastSeen(Duration::hours(2))),
                        Arc::new(RichText::new("🕒 2h")),
                        "Filter by last seen time: 2 hours".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::LastSeen(Duration::hours(1))),
                        Arc::new(RichText::new("🕒 1h")),
                        "Filter by last seen time: 1 hour".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::LastSeen(Duration::minutes(30))),
                        Arc::new(RichText::new("🕒 30m")),
                        "Filter by last seen time: 30 minutes".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::LastSeen(Duration::minutes(15))),
                        Arc::new(RichText::new("🕒 15m")),
                        "Filter by last seen time: 15 minutes".to_string(),
                    ),
                ],
                vec![
                    (
                        None,
                        Arc::new(RichText::new("📍")),
                        "Switch on filter by position".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::HasPosition),
                        Arc::new(RichText::new("📍")),
                        "Node has only one position".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::HasTracks),
                        Arc::new(RichText::new("🏁")),
                        "Node has tracks (number of positions > 1)".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::HasNoPosition),
                        Arc::new(RichText::new("📍").color(Color32::LIGHT_RED)),
                        "Node has no position".to_string(),
                    ),
                ],
            ];

            if show_extended {
                let mut extended_filters = vec![
                    vec![
                        (
                            None,
                            Arc::new(RichText::new("🔒")),
                            "Filter by public key".to_string(),
                        ),
                        (
                            Some(StaticFilterVariant::PublicKey(
                                PublicKeyVariant::Compromised,
                            )),
                            Arc::new(RichText::new("🔒").color(Color32::YELLOW)),
                            "Filtered by compromised public key".to_string(),
                        ),
                        (
                            Some(StaticFilterVariant::PublicKey(PublicKeyVariant::Valid)),
                            Arc::new(RichText::new("🔒").color(Color32::LIGHT_GREEN)),
                            "Filtered by valid public key".to_string(),
                        ),
                        (
                            Some(StaticFilterVariant::PublicKey(PublicKeyVariant::None)),
                            Arc::new(RichText::new("🔓").color(Color32::LIGHT_RED)),
                            "Show nodes with no public key".to_string(),
                        ),
                    ],
                    vec![
                        (
                            None,
                            Arc::new(RichText::new("🖹")),
                            "Switch on by `is_licensed` flag".to_string(),
                        ),
                        (
                            Some(StaticFilterVariant::IsLicensed(true)),
                            Arc::new(RichText::new("🖹").color(Color32::LIGHT_BLUE)),
                            "Search with enabled `is_licensed` flag".to_string(),
                        ),
                        (
                            Some(StaticFilterVariant::IsLicensed(false)),
                            Arc::new(RichText::new("🖹").color(Color32::LIGHT_RED)),
                            "Show only node without `is_licensed` flag".to_string(),
                        ),
                    ],
                    vec![
                        (
                            None,
                            Arc::new(RichText::new("🚫")),
                            "Enable filter by `is_unmessagable` flag".to_string(),
                        ),
                        (
                            Some(StaticFilterVariant::IsUnmessagable),
                            Arc::new(RichText::new("🚫").color(Color32::LIGHT_RED)),
                            "Show only if `is_unmessagable` is set".to_string(),
                        ),
                    ],
                ];
                static_filters_vary.append(&mut extended_filters);
            }

            if let Some(bbox) = self.bbox {
                static_filters_vary.push(vec![
                    (
                        None,
                        Arc::new(RichText::new("🌐")),
                        "Filter by map's view".to_string(),
                    ),
                    (
                        Some(StaticFilterVariant::BoundingBox),
                        Arc::new(RichText::new("🌐")),
                        format!(
                            "Filter by bounding box: [{:.6}, {:.6}, {:.6}, {:.6}]",
                            bbox[0].y(),
                            bbox[0].x(),
                            bbox[1].y(),
                            bbox[1].x(),
                        ),
                    ),
                ]);
            }

            for filters_vary in static_filters_vary {
                let enabled_index = filters_vary
                    .iter()
                    .enumerate()
                    .map(|(index, (filter_or_not, _, _))| {
                        if let Some(filter) = filter_or_not {
                            self.static_filter.contains(filter).then(|| Some(index))
                        } else {
                            None
                        }
                    })
                    .flatten()
                    .flatten()
                    .last();

                if let Some(enabled_index) = enabled_index {
                    if ui
                        .selectable_label(true, filters_vary[enabled_index].1.clone())
                        .on_hover_text(filters_vary[enabled_index].2.as_str())
                        .clicked()
                    {
                        if let Some(ref filter) = filters_vary[enabled_index].0 {
                            self.static_filter.remove(&filter);
                            if let Some(next_filter) = filters_vary
                                .get(enabled_index + 1)
                                .map(|v| v.0.clone())
                                .flatten()
                            {
                                self.static_filter.insert(next_filter);
                            }
                        }
                    }
                } else {
                    if let Some(filters) = filters_vary.windows(2).next() {
                        if let Some(ref next_filter) = filters[1].0 {
                            if ui
                                .selectable_label(false, filters[0].1.clone())
                                .on_hover_text(filters[0].2.as_str())
                                .clicked()
                            {
                                self.static_filter.insert(next_filter.clone());
                            }
                        }
                    }
                }
            }

            let static_filter = [
                (
                    StaticFilterVariant::HasEnvironmentTelemetry,
                    RichText::new("🌱"),
                    "Node has environment telemetry like temperature, humidity, etc.",
                ),
                (
                    StaticFilterVariant::HasDeviceTelemetry,
                    RichText::new("📟"),
                    "Node has device telemetry like channel util, battery level, etc.",
                ),
                (
                    StaticFilterVariant::IsGateway,
                    RichText::new("🖧"),
                    "Show if node is gateway",
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
    pub fn seeker_for<'a>(
        &'a self,
        nodes: &'a HashMap<NodeId, NodeInfo>,
        nodebook: &'a NodeBook,
    ) -> NodeSeeker<'a> {
        NodeSeeker {
            nodes,
            nodebook,
            iterator: nodes.values(),
            filter: self,
        }
    }

    // Set bounding box for filtering nodes based on position
    pub fn set_bbox(&mut self, bbox: [walkers::Position; 2]) {
        self.bbox = Some(bbox);
    }
}

#[derive(Clone)]
pub struct NodeSeeker<'a> {
    // Direct access to NodeInfo by NodeId
    pub nodes: &'a HashMap<NodeId, NodeInfo>,
    // Direct access to NodeBook
    pub nodebook: &'a NodeBook,
    iterator: Values<'a, NodeId, NodeInfo>,
    filter: &'a NodeFilter,
}

impl<'a> NodeSeeker<'a> {
    pub fn matches(&self, node_info: &NodeInfo, node_annotation: Option<&NodeAnnotation>) -> bool {
        self.filter.matches(node_info, node_annotation)
    }
}

impl<'a> Iterator for NodeSeeker<'a> {
    type Item = &'a NodeInfo;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node_info) = self.iterator.next() {
            let node_annotation = self.nodebook.node_get(&node_info.node_id);

            if self.matches(node_info, node_annotation) {
                return Some(node_info);
            }
        }
        None
    }
}
