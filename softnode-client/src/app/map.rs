use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui::{Align2, Area, Button, Color32, Context, Id, Pos2, Rect, Vec2};
use meshtastic_connect::keyring::node_id::NodeId;
use walkers::{
    HttpTiles, MapMemory,
    extras::{LabeledSymbol, LabeledSymbolStyle, Place, Symbol},
    lon_lat,
    sources::OpenStreetMap,
};

use crate::app::{
    Panel, PanelCommand,
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

impl<'a> walkers::Plugin for MapPointsPlugin<'a> {
    fn run(
        self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &walkers::Projector,
        _map_memory: &MapMemory,
    ) {
        let mut not_on_map_nodes = Vec::new();
        let current_datetime = chrono::Utc::now();
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
        let related_nodes = self
            .memory
            .selection
            .map(|selection| {
                if let MemorySelection::Node(selected_node_id) = selection {
                    self.nodes
                        .get(&selected_node_id)
                        .map(|node_info| Some(&node_info.gateway_for))
                        .flatten()
                } else {
                    None
                }
            })
            .flatten();
        for (node_id, node_info) in self.nodes {
            let selected = self
                .memory
                .selection
                .map(|selection| match selection {
                    MemorySelection::Node(selected_node_id) => selected_node_id == *node_id,
                    MemorySelection::Position(_) => false,
                })
                .unwrap_or(false);
            let mesh_position = fix_or_position(&self.fix_gnss, *node_id, &node_info.position);
            if let Some(position) = mesh_position.or_else(|| {
                selected.then_some(projector.unproject(response.rect.center().to_vec2()))
            }) {
                let onscreen_position = projector.project(position).to_pos2();
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
                .flatten();

                let gateway_text = if let Some(gateway_info) = related_nodes
                    .map(|nodes| {
                        nodes
                            .get(node_id)
                            .map(|gateway_info| gateway_info.last())
                            .flatten()
                    })
                    .flatten()
                {
                    let mut text = Vec::new();

                    if let Some(rx_info) = &gateway_info.rx_info {
                        text.push(format!(
                            "RSSI/SNR: {:.2} dB/{:.2} dB",
                            rx_info.rx_rssi, rx_info.rx_snr
                        ));
                    }
                    if let Some(hops_away) = gateway_info.hop_distance {
                        text.push(format!(
                            "Hops limit: {} (away: {})",
                            gateway_info.hop_limit, hops_away
                        ));
                    } else {
                        text.push(format!("Hops limit: {}", gateway_info.hop_limit));
                    }

                    text
                } else {
                    Vec::new()
                };
                let telemetry = gateway_text
                    .iter()
                    .fold(String::new(), |a, b| a + b.as_str() + "\n")
                    + telemetry
                        .fold(String::new(), |a, b| a + b.as_str() + "\n")
                        .as_str();

                let symbol = if node_info.gateway_for.is_empty() {
                    Some(Symbol::TwoCorners(String::from("👤")))
                } else {
                    Some(Symbol::Circle(String::from("👤")))
                };
                let radius = circle_radius(node_info.gateway_for.len());

                if let Some(clicked_pos) = clicked_pos {
                    if clicked_pos.distance(onscreen_position) < radius * 1.8 {
                        self.memory.selection = Some(MemorySelection::Node(*node_id));
                    }
                }

                let background = if selected {
                    Color32::RED.gamma_multiply(0.4)
                } else {
                    Color32::WHITE.gamma_multiply(0.4)
                };

                if selected {
                    if mesh_position.is_none() {
                        let buttons_position =
                            Pos2::new(onscreen_position.x, onscreen_position.y - 20.0);
                        if ui
                            .put(
                                Rect::from_center_size(buttons_position, Vec2::new(140., 20.)),
                                Button::new("Put here"),
                            )
                            .clicked()
                        {
                            self.fix_gnss
                                .entry(*node_id)
                                .and_modify(|v| {
                                    v.longitude = position.x();
                                    v.latitude = position.y();
                                })
                                .or_insert(FixGnss {
                                    node_id: *node_id,
                                    longitude: position.x(),
                                    latitude: position.y(),
                                });
                        };
                    }

                    for (node_id, gateway_info) in &node_info.gateway_for {
                        if let Some(other_position) = self
                            .nodes
                            .get(node_id)
                            .map(|node_info| {
                                fix_or_position(
                                    &self.fix_gnss,
                                    node_info.node_id,
                                    &node_info.position,
                                )
                            })
                            .flatten()
                        {
                            draw_connection(
                                ui,
                                onscreen_position,
                                projector.project(other_position).to_pos2(),
                                current_datetime,
                                gateway_info.last(),
                                Color32::RED,
                            );
                        } else {
                            if let Some(gateway_info) = gateway_info.last() {
                                not_on_map_nodes.push((node_id, gateway_info));
                            }
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
                                    fix_or_position(
                                        &self.fix_gnss,
                                        node_info.node_id,
                                        &node_info.position,
                                    )
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
                            Color32::GREEN,
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
                        label_corner_radius: 10.0,
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

        if !not_on_map_nodes.is_empty() {
            let rect = ui.ctx().viewport_rect();

            let nodes = |ui: &mut egui::Ui| {
                for (node_id, _gateway_info) in not_on_map_nodes {
                    let title = if let Some(extended) = self
                        .nodes
                        .get(node_id)
                        .map(|node_info| node_info.extended_info_history.last())
                        .flatten()
                    {
                        format!("{}\n{}", node_id, extended.short_name)
                    } else {
                        format!("{}\n", node_id)
                    };
                    if ui.add_sized((80.0, 32.0), Button::new(title)).clicked() {
                        self.memory.selection = Some(MemorySelection::Node(*node_id));
                    };
                }
            };

            if rect.width() > rect.height() {
                Area::new(Id::new("vertical_not_on_map_nodes"))
                    .anchor(Align2::RIGHT_CENTER, (-40.0, 0.0))
                    .show(ui.ctx(), |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([true, true])
                            .show(ui, |ui| {
                                ui.vertical(|ui| nodes(ui));
                            });
                    });
            } else {
                Area::new(Id::new("horizontal_not_on_map_nodes"))
                    .anchor(Align2::CENTER_BOTTOM, (0.0, -20.0))
                    .show(ui.ctx(), |ui| {
                        egui::ScrollArea::horizontal()
                            .auto_shrink([true, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| nodes(ui));
                                ui.add_space(8.0);
                            });
                    });
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
    const RSSI_RANGE: [i32; 2] = [-130, 10];
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
