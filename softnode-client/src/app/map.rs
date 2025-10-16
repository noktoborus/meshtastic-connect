use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui::{Color32, Context};
use meshtastic_connect::keyring::node_id::NodeId;
use walkers::{
    HttpTiles, MapMemory,
    extras::{LabeledSymbol, LabeledSymbolStyle, Place, Symbol},
    lon_lat,
    sources::OpenStreetMap,
};

use crate::app::{
    data::{GatewayInfo, NodeInfo, Position, TelemetryVariant},
    fix_gnss::{FixGnss, FixGnssLibrary},
};

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

const TRANSPARENCY_MAX: u8 = 255;
const TRANSPARENCY_MIN: u8 = 80;
const TRANSPARENCY_RANGE_HOURS: u8 = 24;

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone, Copy)]
enum MemorySelection {
    Node(NodeId),
    Position(walkers::Position),
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Memory {
    selection: Option<MemorySelection>,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct MapPanel {
    map_memory: MapMemory,
    memory: Memory,
}

pub struct MapPointsPlugin<'a> {
    nodes: &'a HashMap<NodeId, NodeInfo>,
    memory: &'a mut Memory,
    fix_gnss: &'a FixGnssLibrary,
}

impl<'a> MapPointsPlugin<'a> {
    pub fn new(
        nodes: &'a HashMap<NodeId, NodeInfo>,
        memory: &'a mut Memory,
        fix_gnss: &'a FixGnssLibrary,
    ) -> Self {
        Self {
            nodes,
            memory,
            fix_gnss,
        }
    }

    fn fix_or_position(
        &self,
        node_id: NodeId,
        positions: &Vec<Position>,
    ) -> Option<walkers::Position> {
        self.fix_gnss
            .get(&node_id)
            .map(|fix| lon_lat(fix.longitude, fix.latitude))
            .or_else(|| {
                positions
                    .last()
                    .map(|pos| lon_lat(pos.longitude, pos.latitude))
            })
    }
}

impl<'a> walkers::Plugin for MapPointsPlugin<'a> {
    fn run(
        self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &walkers::Projector,
        _map_memory: &MapMemory,
    ) {
        let current_datetime = chrono::Utc::now();
        let painter = ui.painter();
        let clicked_pos = response.clicked().then(|| response.hover_pos()).flatten();
        if clicked_pos.is_some() {
            println!("clicked: {:?}", clicked_pos);
            self.memory.selection = None;
        }
        for (node_id, node_info) in self.nodes {
            if let Some(position) = self.fix_or_position(*node_id, &node_info.position) {
                let on_screen_position = projector.project(position).to_pos2();
                let label = if let Some(extended_info) = node_info.extended_info_history.last() {
                    format!("{}\n{}", extended_info.short_name, node_info.node_id)
                } else {
                    node_info.node_id.to_string()
                };
                let telemetry = [
                    TelemetryVariant::Temperature,
                    TelemetryVariant::Humidity,
                    TelemetryVariant::Lux,
                    TelemetryVariant::BarometricPressure,
                    TelemetryVariant::Radiation,
                ]
                .iter()
                .map(|variant| {
                    node_info
                        .telemetry
                        .get(&variant)
                        .map(|list_or_none| {
                            list_or_none
                                .last()
                                .map(|value| match variant {
                                    TelemetryVariant::Temperature => {
                                        Some(format!("{:.2}°C", value.value))
                                    }
                                    TelemetryVariant::Humidity => {
                                        Some(format!("{:.2}%", value.value))
                                    }
                                    TelemetryVariant::Lux => Some(format!("{:.2}lx", value.value)),
                                    TelemetryVariant::BarometricPressure => {
                                        Some(format!("{:.2}hPa", value.value))
                                    }
                                    TelemetryVariant::Radiation => {
                                        Some(format!("{:.2}μSv/h", value.value))
                                    }
                                    _ => None,
                                })
                                .flatten()
                        })
                        .flatten()
                })
                .filter(|v| v.is_some())
                .flatten()
                .fold(String::new(), |a, b| a + b.as_str() + "\n");

                let symbol = if node_info.gateway_for.is_empty() {
                    Some(Symbol::TwoCorners(String::from("👤")))
                } else {
                    Some(Symbol::Circle(String::from("👤")))
                };
                let radius = circle_radius(node_info.gateway_for.len());

                if let Some(clicked_pos) = clicked_pos {
                    if clicked_pos.distance(on_screen_position) < radius * 1.8 {
                        self.memory.selection = Some(MemorySelection::Node(*node_id));
                    }
                }

                let selected = self
                    .memory
                    .selection
                    .map(|selection| match selection {
                        MemorySelection::Node(selected_node_id) => selected_node_id == *node_id,
                        MemorySelection::Position(point) => {
                            let point = projector.project(point).to_pos2();

                            painter.circle(
                                point,
                                3.0,
                                Color32::BLUE,
                                (2.0, Color32::BLUE.gamma_multiply(0.8)),
                            );
                            false
                        }
                    })
                    .unwrap_or(false);

                let background = if selected {
                    Color32::RED.gamma_multiply(0.4)
                } else {
                    Color32::WHITE.gamma_multiply(0.4)
                };

                if selected {
                    for (node_id, gateway_info) in &node_info.gateway_for {
                        if let Some(other_position) = self
                            .nodes
                            .get(node_id)
                            .map(|node_info| {
                                self.fix_or_position(node_info.node_id, &node_info.position)
                            })
                            .flatten()
                        {
                            let stroke =
                                opaque_width(current_datetime, gateway_info.last(), Color32::RED);

                            painter.line(
                                vec![
                                    on_screen_position,
                                    projector.project(other_position).to_pos2(),
                                ],
                                stroke,
                            );
                        }
                    }

                    for (gateway_info, other_position) in self
                        .nodes
                        .values()
                        .map(|node_info| {
                            node_info
                                .gateway_for
                                .get(node_id)
                                .map(|gateway_info| {
                                    self.fix_or_position(node_info.node_id, &node_info.position)
                                        .map(|position| (gateway_info.last(), position))
                                })
                                .flatten()
                        })
                        .filter(|v| v.is_some())
                        .flatten()
                    {
                        let stroke = opaque_width(current_datetime, gateway_info, Color32::GREEN);

                        painter.line(
                            vec![
                                on_screen_position,
                                projector.project(other_position).to_pos2(),
                            ],
                            stroke,
                        );
                    }
                }

                let label = if !telemetry.is_empty() {
                    format!("{}\n{}", telemetry, label)
                } else {
                    label
                };

                LabeledSymbol {
                    position,
                    label,
                    symbol,
                    style: LabeledSymbolStyle {
                        label_corner_radius: radius,
                        symbol_size: radius,
                        symbol_background: background,
                        ..Default::default()
                    },
                }
                .draw(ui, projector);
            }
            if self.memory.selection.is_none()
                && let Some(clicked_pos) = clicked_pos
            {
                self.memory.selection = Some(MemorySelection::Position(
                    projector.unproject(clicked_pos.to_vec2()),
                ));
            }
        }
    }
}

impl MapPanel {
    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        map_context: &mut MapContext,
        nodes: &HashMap<NodeId, NodeInfo>,
        fix_gnss: &FixGnssLibrary,
    ) {
        let map_nodes = MapPointsPlugin::new(nodes, &mut self.memory, fix_gnss);
        let map = walkers::Map::new(
            Some(&mut map_context.tiles),
            &mut self.map_memory,
            lon_lat(17.03664, 51.09916),
        )
        .with_plugin(map_nodes);

        ui.add(map);
    }

    pub fn panel_ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        node_info: &NodeInfo,
        _map_context: &mut MapContext,
        fix_gnss: &mut FixGnssLibrary,
    ) {
        if fix_gnss.get(&node_info.node_id).is_some() {
            if ui.button("Unset FIX GNSS").clicked() {
                fix_gnss.remove(&node_info.node_id);
            }
        } else if let Some(selection) = self.memory.selection {
            match selection {
                MemorySelection::Node(_) => {}
                MemorySelection::Position(point) => {
                    if ui.button("Set fixed coordinates").clicked() {
                        fix_gnss.entry(node_info.node_id).or_insert(FixGnss {
                            node_id: node_info.node_id,
                            latitude: point.y(),
                            longitude: point.x(),
                        });
                    }
                }
            }
        }
    }
}

fn circle_radius(gateway_for: usize) -> f32 {
    const MIN: f32 = 15.0;
    const MAX: f32 = 26.0;
    const UPPER_NODES_LIMIT: usize = 100;

    if gateway_for >= UPPER_NODES_LIMIT {
        MAX
    } else {
        MIN + (gateway_for as f32 / UPPER_NODES_LIMIT as f32) * (MAX - MIN)
    }
}

fn opaque_width(
    current_datetime: DateTime<Utc>,
    gateway_info: Option<&GatewayInfo>,
    base_color: Color32,
) -> (f32, Color32) {
    if let Some(gateway_info) = gateway_info {
        let opaque = if current_datetime > gateway_info.timestamp {
            let diff = gateway_info.timestamp - current_datetime;
            let hours = diff.num_hours();

            if hours as u8 > TRANSPARENCY_RANGE_HOURS {
                TRANSPARENCY_MIN
            } else {
                TRANSPARENCY_MAX
                    - (hours as u8 * (TRANSPARENCY_MAX - TRANSPARENCY_MIN)
                        / TRANSPARENCY_RANGE_HOURS)
            }
        } else {
            255
        };
        (
            gateway_info.hop_limit as f32 + 1.0,
            Color32::from_rgba_unmultiplied(base_color.r(), base_color.g(), base_color.b(), opaque),
        )
    } else {
        (1.0, base_color)
    }
}
