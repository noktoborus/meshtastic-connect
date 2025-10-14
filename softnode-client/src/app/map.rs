use std::collections::HashMap;

use egui::Context;
use meshtastic_connect::keyring::node_id::NodeId;
use walkers::{HttpTiles, MapMemory, lon_lat, sources::OpenStreetMap};

use crate::app::data::NodeInfo;

pub struct MapContext {
    tiles: HttpTiles,
}

impl MapContext {
    pub fn new(egui_ctx: Context) -> Self {
        Self {
            tiles: HttpTiles::new(OpenStreetMap, egui_ctx),
        }
    }
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct Map {
    map_memory: MapMemory,
}

pub struct MapPointsPlugin<'a> {
    nodes: &'a HashMap<NodeId, NodeInfo>,
}

impl<'a> MapPointsPlugin<'a> {
    pub fn new(nodes: &'a HashMap<NodeId, NodeInfo>) -> Self {
        Self { nodes }
    }
}

impl<'a> walkers::Plugin for MapPointsPlugin<'a> {
    fn run(
        self: Box<Self>,
        ui: &mut egui::Ui,
        _response: &egui::Response,
        projector: &walkers::Projector,
        _map_memory: &MapMemory,
    ) {
        let painter = ui.painter();
        for (_, node_info) in self.nodes {
            if let Some(last_position) = node_info.position.last() {
                let position = lon_lat(last_position.longitude, last_position.latitude);
                let radius = 200.0 * projector.scale_pixel_per_meter(position);
                let center = projector.project(position).to_pos2();

                painter.circle_filled(center, radius, egui::Color32::BLACK);
            }
        }
    }
}

impl Map {
    pub fn new() -> Self {
        let mut map_memory = MapMemory::default();
        let _ = map_memory.set_zoom(4.0);
        Self { map_memory }
    }

    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        map_context: &mut MapContext,
        nodes: &HashMap<NodeId, NodeInfo>,
    ) {
        let map_nodes = MapPointsPlugin::new(nodes);
        let map = walkers::Map::new(
            Some(&mut map_context.tiles),
            &mut self.map_memory,
            lon_lat(17.03664, 51.09916),
        )
        .with_plugin(map_nodes);

        ui.add(map);
    }
}
