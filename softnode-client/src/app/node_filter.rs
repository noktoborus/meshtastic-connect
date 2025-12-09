use std::collections::{HashMap, hash_map::Values};

use meshtastic_connect::keyring::{key::Key, node_id::NodeId};

use crate::app::{
    byte_node_id::ByteNodeId,
    data::{NodeInfo, PublicKey},
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum FilterVariant {
    Generic(String),
    PublicPkey(Key),
    ByteNodeId(ByteNodeId),
    NodeId(NodeId),
    CompromisedPkey,
    IsLicensed,
    IsUnmessagable,
    HasTelemetry,
    HasTracks,
    HasPosition,
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
            }
            FilterVariant::PublicPkey(_key) => {}
            FilterVariant::ByteNodeId(byte_node_id) => return *byte_node_id == node_info.node_id,
            FilterVariant::NodeId(node_id) => return *node_id == node_info.node_id,
            FilterVariant::CompromisedPkey => {}
            FilterVariant::IsLicensed => {}
            FilterVariant::IsUnmessagable => {}
            FilterVariant::HasTelemetry => {
                for telemetry in node_info.telemetry.values() {
                    if telemetry.values.len() > 0 {
                        return true;
                    }
                }
            }
            FilterVariant::HasTracks => {
                return node_info.position.len() > 1;
            }
            FilterVariant::HasPosition => {
                return node_info.position.len() > 0;
            }
        }

        if let Some(extended) = node_info.extended_info_history.last() {
            match self {
                FilterVariant::Generic(string) => {
                    if extended.short_name.to_lowercase().contains(string) {
                        return true;
                    }
                    if extended.long_name.to_lowercase().contains(string) {
                        return true;
                    }
                }
                FilterVariant::PublicPkey(key) => match extended.pkey {
                    PublicKey::None => return false,
                    PublicKey::Key(node_key) => return *key == node_key,
                    PublicKey::Compromised(node_key) => return *key == node_key,
                },
                FilterVariant::ByteNodeId(_byte_node_id) => {}
                FilterVariant::NodeId(_node_id) => {}
                FilterVariant::CompromisedPkey => {
                    return matches!(extended.pkey, PublicKey::Compromised(_));
                }
                FilterVariant::IsLicensed => return extended.is_licensed,
                FilterVariant::IsUnmessagable => {
                    if let Some(is_unmessagable) = extended.is_unmessagable {
                        return is_unmessagable;
                    }
                }
                FilterVariant::HasTelemetry => {}
                FilterVariant::HasTracks => {}
                FilterVariant::HasPosition => {}
            }
        }

        return false;
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct NodeFilter {
    filter_parts: Vec<(FilterVariant, bool)>,
    filter_origin: Option<String>,
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            filter_parts: Vec::new(),
            filter_origin: None,
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

        self.filter_parts.push((FilterVariant::IsLicensed, false));
        self.filter_parts
            .push((FilterVariant::IsUnmessagable, false));
        self.filter_parts.push((FilterVariant::HasTelemetry, false));
        self.filter_parts.push((FilterVariant::HasTracks, false));
        self.filter_parts.push((FilterVariant::HasPosition, false));
        self.filter_parts
            .push((FilterVariant::CompromisedPkey, false));
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
                    FilterVariant::CompromisedPkey => {
                        ui.selectable_label(*enabled, "Compromised PKey")
                    }
                    FilterVariant::Generic(generic) => {
                        ui.selectable_label(*enabled, format!("{}", generic))
                    }
                    FilterVariant::IsLicensed => ui.selectable_label(*enabled, "Licensed"),
                    FilterVariant::IsUnmessagable => ui.selectable_label(*enabled, "Unmessagable"),
                    FilterVariant::HasTelemetry => ui.selectable_label(*enabled, "Telemetry"),
                    FilterVariant::HasTracks => ui.selectable_label(*enabled, "Tracks"),
                    FilterVariant::HasPosition => ui.selectable_label(*enabled, "Position"),
                }
                .clicked()
                .then(|| {
                    *enabled = !*enabled;
                });
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
