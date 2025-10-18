use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui::{Align2, Area, Button, Color32, Context, Id, Pos2, Rect, Vec2, response};
use meshtastic_connect::keyring::node_id::NodeId;
use walkers::{
    HttpTiles, Map, MapMemory,
    extras::{LabeledSymbol, LabeledSymbolStyle, Place, Symbol},
    lon_lat,
    sources::OpenStreetMap,
};

use crate::app::{
    Panel, PanelCommand, color_generator,
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
    fix_gnss: &'a mut FixGnssLibrary,
    color_generator: color_generator::ColorGenerator,
}

impl<'a> MapPointsPlugin<'a> {
    pub fn new(
        nodes: &'a HashMap<NodeId, NodeInfo>,
        memory: &'a mut Memory,
        fix_gnss: &'a mut FixGnssLibrary,
    ) -> Self {
        Self {
            nodes,
            memory,
            fix_gnss,
            color_generator: Default::default(),
        }
    }
}

fn fix_or_position(
    fix_gnss: &FixGnssLibrary,
    node_id: NodeId,
    positions: &Vec<Position>,
) -> Option<walkers::Position> {
    fix_gnss
        .get(&node_id)
        .map(|fix| lon_lat(fix.longitude, fix.latitude))
        .or_else(|| {
            positions
                .last()
                .map(|pos| lon_lat(pos.longitude, pos.latitude))
        })
}

fn get_telemetry_label(node_info: &NodeInfo) -> String {
    [
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
                        TelemetryVariant::Temperature => Some(format!("{:.2} °C", value.value)),
                        TelemetryVariant::Humidity => Some(format!("{:.2}%", value.value)),
                        TelemetryVariant::Lux => Some(format!("{:.2} lx", value.value)),
                        TelemetryVariant::BarometricPressure => {
                            Some(format!("{:.2} hPa", value.value))
                        }
                        TelemetryVariant::Radiation => Some(format!("{:.2} μSv/h", value.value)),
                        _ => None,
                    })
                    .flatten()
            })
            .flatten()
    })
    .filter(|v| v.is_some())
    .flatten()
    .fold(String::new(), |a, b| a + b.as_str() + "\n")
}

impl<'a> MapPointsPlugin<'a> {
    const SYMBOL_SIZE_SELECT_FACTOR: f32 = 1.8;

    fn draw_radiated_connections(
        self: &mut Box<Self>,
        ui: &mut egui::Ui,
        onscreen_position: Pos2,
        node_id: NodeId,
        projector: &walkers::Projector,
        current_datetime: DateTime<Utc>,
    ) {
        for (gateway_info, other_position) in self
            .nodes
            .values()
            .map(|node_info| {
                node_info
                    .gateway_for
                    .get(&node_id)
                    .map(|gateway_info| {
                        fix_or_position(&self.fix_gnss, node_info.node_id, &node_info.position)
                            .map(|position| (gateway_info.last(), position))
                    })
                    .flatten()
            })
            .filter(|v| v.is_some())
            .flatten()
        {
            draw_connection(
                ui,
                onscreen_position,
                projector.project(other_position).to_pos2(),
                current_datetime,
                gateway_info,
                self.color_generator.next_color(),
            );
        }
    }

    fn draw_received_connections(
        self: &mut Box<Self>,
        ui: &mut egui::Ui,
        gateway_onscreen_position: Pos2,
        gateway_node_info: &'a NodeInfo,
        projector: &walkers::Projector,
        current_datetime: DateTime<Utc>,
    ) -> Vec<NodeId> {
        let mut not_on_map_nodes = Vec::new();
        for (node_id, gateway_info) in gateway_node_info.gateway_for.iter() {
            let connection_color = self.color_generator.next_color();
            if let Some(other_position) = self
                .nodes
                .get(node_id)
                .map(|node_info| {
                    fix_or_position(&self.fix_gnss, node_info.node_id, &node_info.position)
                })
                .flatten()
            {
                draw_connection(
                    ui,
                    gateway_onscreen_position,
                    projector.project(other_position).to_pos2(),
                    current_datetime,
                    gateway_info.last(),
                    connection_color,
                );
            } else {
                not_on_map_nodes.push(*node_id);
            }
        }
        not_on_map_nodes
    }

    // Draw steps:
    // 1. Draw connection lines first
    // 2. Draw other nodes with RSSI/SNR/hops and without telemetry
    // 3. Draw selected nodes with gateway info and without telemetry
    fn draw_selected(
        mut self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &walkers::Projector,
        node_info: &'a NodeInfo,
        clicked_pos: Option<Pos2>,
    ) {
        let is_gateway = !node_info.gateway_for.is_empty();
        let current_datetime = chrono::Utc::now();
        let position = fix_or_position(&self.fix_gnss, node_info.node_id, &node_info.position)
            .unwrap_or(projector.unproject(response.rect.center().to_vec2()));
        let symbol_size = circle_radius(node_info.gateway_for.len());
        let onscreen_position = projector.project(position).to_pos2();
        if let Some(clicked_pos) = clicked_pos {
            if clicked_pos.distance(onscreen_position)
                < symbol_size * Self::SYMBOL_SIZE_SELECT_FACTOR
            {
                self.memory.selection = Some(MemorySelection::Node(node_info.node_id));
            }
        }

        let not_landed_nodes = if is_gateway {
            self.draw_received_connections(
                ui,
                onscreen_position,
                node_info,
                projector,
                current_datetime,
            )
        } else {
            self.draw_radiated_connections(
                ui,
                onscreen_position,
                node_info.node_id,
                projector,
                current_datetime,
            );
            Vec::new()
        };

        for (other_node_id, other_node_info) in self.nodes {
            if let Some(position) =
                fix_or_position(&self.fix_gnss, *other_node_id, &other_node_info.position)
            {
                let symbol_size = circle_radius(other_node_info.gateway_for.len());
                let onscreen_position = projector.project(position).to_pos2();
                if let Some(clicked_pos) = clicked_pos {
                    if clicked_pos.distance(onscreen_position)
                        < symbol_size * Self::SYMBOL_SIZE_SELECT_FACTOR
                    {
                        self.memory.selection = Some(MemorySelection::Node(*other_node_id));
                        ui.ctx().request_repaint();
                        return;
                    }
                }

                let (symbol_label, label) = if let Some(gateway_info) = node_info
                    .gateway_for
                    .get(other_node_id)
                    .map(|v| v.last())
                    .flatten()
                {
                    let symbol = if let Some(distance) = gateway_info.hop_distance {
                        distance.to_string()
                    } else {
                        "👤".to_string()
                    };

                    let label = gateway_info
                        .rx_info
                        .as_ref()
                        .map(|rx_info| {
                            format!("RSSI: {}\nSNR: {}", rx_info.rx_rssi, rx_info.rx_snr)
                        })
                        .unwrap_or(String::new());

                    let label = if gateway_info.timestamp < current_datetime {
                        let timediff = current_datetime - gateway_info.timestamp;
                        if timediff.num_hours() > 1 {
                            format!("{}\n{} hours ago", label, timediff.num_hours())
                        } else if timediff.num_minutes() > 1 {
                            format!("{}\n{} minutes ago", label, timediff.num_minutes())
                        } else {
                            format!("{}\n{} seconds ago", label, timediff.num_seconds())
                        }
                    } else {
                        label
                    };

                    let label = other_node_info
                        .extended_info_history
                        .last()
                        .map(|extended_info| {
                            format!(
                                "{}\n{}\n{}",
                                label, extended_info.short_name, node_info.node_id
                            )
                        })
                        .unwrap_or(label);

                    (symbol, label)
                } else {
                    ("👤".to_string(), "".to_string())
                };

                let symbol_background = Color32::WHITE.gamma_multiply(0.6);
                let symbol = if other_node_info.gateway_for.is_empty() {
                    Some(Symbol::TwoCorners(symbol_label))
                } else {
                    Some(Symbol::Circle(symbol_label))
                };

                LabeledSymbol {
                    position,
                    label,
                    symbol,
                    style: LabeledSymbolStyle {
                        label_corner_radius: 10.0,
                        symbol_size,
                        symbol_background,
                        ..Default::default()
                    },
                }
                .draw(ui, projector);
            }
        }

        let label = if let Some(extended_info) = node_info.extended_info_history.last() {
            format!("{}\n{}", extended_info.short_name, node_info.node_id)
        } else {
            node_info.node_id.to_string()
        };
        let label = if !not_landed_nodes.is_empty() {
            format!(
                "Received nodes: {}\nNowhere nodes: {}\n{}",
                node_info.gateway_for.len(),
                not_landed_nodes.len(),
                label
            )
        } else {
            label
        };
        let symbol_background = Color32::RED.gamma_multiply(0.6);
        let symbol = if is_gateway {
            Some(Symbol::Circle("👤".into()))
        } else {
            Some(Symbol::TwoCorners("👤".into()))
        };

        LabeledSymbol {
            position,
            label,
            symbol,
            style: LabeledSymbolStyle {
                label_corner_radius: 10.0,
                symbol_size,
                symbol_background,
                ..Default::default()
            },
        }
        .draw(ui, projector);
    }

    // Simple draw if no node selected
    fn draw_regular(
        self: &mut Box<Self>,
        ui: &mut egui::Ui,
        projector: &walkers::Projector,
        clicked_pos: Option<Pos2>,
    ) {
        for (node_id, node_info) in self.nodes {
            let mesh_position = fix_or_position(&self.fix_gnss, *node_id, &node_info.position);
            if let Some(position) = mesh_position {
                let symbol_size = circle_radius(node_info.gateway_for.len());
                let onscreen_position = projector.project(position).to_pos2();
                if let Some(clicked_pos) = clicked_pos {
                    if clicked_pos.distance(onscreen_position)
                        < symbol_size * Self::SYMBOL_SIZE_SELECT_FACTOR
                    {
                        self.memory.selection = Some(MemorySelection::Node(*node_id));
                        ui.ctx().request_repaint();
                        return;
                    }
                }

                let label = if let Some(extended_info) = node_info.extended_info_history.last() {
                    format!("{}\n{}", extended_info.short_name, node_info.node_id)
                } else {
                    node_info.node_id.to_string()
                };
                let symbol_background = Color32::WHITE.gamma_multiply(0.6);
                let symbol = if node_info.gateway_for.is_empty() {
                    Some(Symbol::TwoCorners("👤".into()))
                } else {
                    Some(Symbol::Circle("👤".into()))
                };
                let telemetry_label = get_telemetry_label(&node_info);
                let label = if telemetry_label.is_empty() {
                    label
                } else {
                    format!("{}\n{}", telemetry_label, label)
                };

                LabeledSymbol {
                    position,
                    label,
                    symbol,
                    style: LabeledSymbolStyle {
                        label_corner_radius: 10.0,
                        symbol_size,
                        symbol_background,
                        ..Default::default()
                    },
                }
                .draw(ui, projector);
            }
        }
    }
}

impl<'a> walkers::Plugin for MapPointsPlugin<'a> {
    fn run(
        mut self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &walkers::Projector,
        _map_memory: &MapMemory,
    ) {
        let clicked_pos = response.clicked().then(|| response.hover_pos()).flatten();
        if clicked_pos.is_some() {
            self.memory.selection = None;
        } else {
            ui.input(|reader| {
                if reader.key_pressed(egui::Key::Escape) {
                    self.memory.selection = None;
                }
            });
        }

        let selection = self
            .memory
            .selection
            .map(|selection| {
                if let MemorySelection::Node(selected_node_id) = selection {
                    self.nodes
                        .get(&selected_node_id)
                        .map(|selected_node_info| selected_node_info)
                } else {
                    None
                }
            })
            .flatten();

        if let Some(selection) = selection {
            self.draw_selected(ui, response, projector, selection, clicked_pos);
        } else {
            self.draw_regular(ui, projector, clicked_pos);
        }
    }
}

impl MapPanel {
    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        map_context: &mut MapContext,
        nodes: &HashMap<NodeId, NodeInfo>,
        fix_gnss: &mut FixGnssLibrary,
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

    pub fn panel_node_ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        node_info: &NodeInfo,
        _map_context: &mut MapContext,
        fix_gnss: &mut FixGnssLibrary,
    ) -> PanelCommand {
        if fix_gnss.get(&node_info.node_id).is_some() {
            if ui.button("Unfix GNSS").clicked() {
                fix_gnss.remove(&node_info.node_id);
            }
        } else if let Some(position) =
            fix_or_position(fix_gnss, node_info.node_id, &node_info.position)
        {
            if ui.button("Show map").clicked() {
                self.memory.selection = Some(MemorySelection::Node(node_info.node_id));
                self.map_memory.center_at(position);
                return PanelCommand::NextPanel(Panel::Map);
            }
        } else {
            if ui.button("Fix GNSS").clicked() {
                self.memory.selection = Some(MemorySelection::Node(node_info.node_id));
                return PanelCommand::NextPanel(Panel::Map);
            }
        }

        PanelCommand::Nothing
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

fn width_by_rssi(rssi: i32) -> f32 {
    const RSSI_RANGE: [i32; 2] = [-120, 10];
    const WIDTH_RANGE: [f32; 2] = [2.0, 12.0];

    if rssi <= RSSI_RANGE[0] {
        WIDTH_RANGE[0]
    } else if rssi >= RSSI_RANGE[1] {
        WIDTH_RANGE[1]
    } else {
        WIDTH_RANGE[0]
            + ((rssi - RSSI_RANGE[0]) as f32 / (RSSI_RANGE[1] - RSSI_RANGE[0]) as f32)
                * (WIDTH_RANGE[1] - WIDTH_RANGE[0])
    }
}

fn opaque_by_timedelta(current_datetime: DateTime<Utc>, remote_datetime: DateTime<Utc>) -> f32 {
    const RANGE: [f32; 2] = [0.2, 1.0];
    const TIME_HOURS_LIMIT: i64 = 24;

    if current_datetime > remote_datetime {
        let diff = remote_datetime - current_datetime;
        let hours_diff = diff.num_hours();

        if hours_diff == 0 {
            RANGE[1]
        } else if hours_diff > TIME_HOURS_LIMIT {
            RANGE[0]
        } else {
            (hours_diff as f32 / TIME_HOURS_LIMIT as f32).max(RANGE[0])
        }
    } else {
        RANGE[1]
    }
}

fn opaque_width(
    current_datetime: DateTime<Utc>,
    gateway_info: Option<&GatewayInfo>,
    base_color: Color32,
) -> (f32, Color32) {
    if let Some(gateway_info) = gateway_info {
        let opaque = opaque_by_timedelta(current_datetime, gateway_info.timestamp);
        let color = base_color.gamma_multiply(opaque);
        let width = if let Some(rx_info) = &gateway_info.rx_info {
            width_by_rssi(rx_info.rx_rssi)
        } else {
            1.0
        };
        (width, color)
    } else {
        (1.0, base_color)
    }
}

fn draw_connection(
    ui: &mut egui::Ui,
    onscreen_position: Pos2,
    other_onscreen_position: Pos2,
    current_datetime: DateTime<Utc>,
    gateway_info: Option<&GatewayInfo>,
    color: Color32,
) {
    let stroke = opaque_width(current_datetime, gateway_info, color);
    let distance = onscreen_position.distance(other_onscreen_position);
    let dash_count = gateway_info
        .map(|gateway_info| {
            gateway_info
                .hop_distance
                .map(|hop_distance| hop_distance + 1)
        })
        .flatten()
        .unwrap_or(15);
    let gap_length = 15.0;
    let dash_length = (distance / dash_count as f32) - 15.0;

    let shape = egui::Shape::dashed_line(
        &vec![onscreen_position, other_onscreen_position],
        stroke,
        dash_length,
        gap_length,
    );

    ui.painter().add(shape);
}
