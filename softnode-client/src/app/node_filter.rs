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
            }
        }

        return false;
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct NodeFilter {
    filter_parts: Vec<FilterVariant>,
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            filter_parts: Vec::new(),
        }
    }
}

impl NodeFilter {
    pub fn new() -> Self {
        Self {
            filter_parts: Vec::new(),
        }
    }

    pub fn matches(&self, node: &NodeInfo) -> bool {
        for filter_part in &self.filter_parts {
            if !filter_part.matches(node) {
                return false;
            }
        }
        true
    }

    // Set new filter's string and parse to filter parts
    pub fn update_filter(&mut self, filter: &str) {
        self.filter_parts.clear();
        for unparsed_part in filter.split_whitespace() {
            if let Ok(pkey) = Key::try_from(unparsed_part) {
                self.filter_parts.push(FilterVariant::PublicPkey(pkey));
            } else if let Ok(node_id) = NodeId::try_from(unparsed_part) {
                self.filter_parts.push(FilterVariant::NodeId(node_id));
            } else if let Ok(byte_node_id) = ByteNodeId::try_from(unparsed_part) {
                self.filter_parts
                    .push(FilterVariant::ByteNodeId(byte_node_id));
            } else if unparsed_part.starts_with("pkey:compromised") {
                self.filter_parts.push(FilterVariant::CompromisedPkey);
            } else {
                self.filter_parts.push(FilterVariant::Generic(
                    unparsed_part.to_string().to_lowercase(),
                ));
            }
        }
    }

    // // Get parsed filter parts
    // pub fn filter_parts(&self) -> Vec<FilterVariant> {
    //     self.filter_parts.clone()
    // }

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
