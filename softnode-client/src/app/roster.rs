use crate::app::{
    byte_node_id::ByteNodeId,
    data::{NodeInfo, TelemetryVariant},
    settings::Settings,
    telemetry::Telemetry,
};
use egui::{Frame, Vec2};
use meshtastic_connect::keyring::node_id::NodeId;
use std::collections::HashMap;

#[derive(serde::Deserialize, serde::Serialize)]
pub enum Panel {
    Journal,
    Telemetry(Telemetry),
    Settings(Settings),
    Rssi(NodeId, Telemetry),
    GatewayByRSSI(NodeId, Telemetry),
    GatewayByHops(NodeId, Telemetry),
    Map,
}

pub trait Plugin {
    fn panel_header_ui(self: &mut Self, ui: &mut egui::Ui) -> PanelCommand;
    fn panel_node_ui(self: &mut Self, ui: &mut egui::Ui, node_info: &NodeInfo) -> PanelCommand;
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct Roster {
    pub show: bool,
    pub telemetry_enabled_for: HashMap<TelemetryVariant, Vec<NodeId>>,
    pub filter: String,
    pub offset: Vec2,
    #[serde(skip)]
    pub roster_heights: HashMap<NodeId, f32>,
}

#[derive(Default)]
pub enum PanelCommand {
    #[default]
    Nothing,
    // HideRoster,
    NextPanel(Panel),
}

impl Roster {
    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        mut roster_plugins: Vec<Box<dyn Plugin + 'a>>,
        nodes: Vec<&NodeInfo>,
        hide_on_action: bool,
    ) -> Option<Panel> {
        ui.horizontal(|ui| {
            egui::TextEdit::singleline(&mut self.filter)
                .desired_width(f32::INFINITY)
                .hint_text("Search node by id or name")
                .show(ui);
            ui.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    self.show = false;
                    self.filter.clear();
                }
            })
        });

        for roster_plugin in roster_plugins.iter_mut() {
            roster_plugin.panel_header_ui(ui);
        }

        let mut filtered_nodes = if self.filter.is_empty() {
            nodes
        } else {
            let normalized_filter = self.filter.to_lowercase();
            let splitted_filter = normalized_filter.split_whitespace().collect::<Vec<&str>>();
            let part_node_ids = splitted_filter
                .iter()
                .map(|splitted| ByteNodeId::try_from(*splitted).ok())
                .filter(|v| v.is_some())
                .flatten()
                .collect::<Vec<_>>();

            nodes
                .iter()
                .filter(|node_info| {
                    let mut skip = true;
                    for part_node_id in &part_node_ids {
                        if *part_node_id == node_info.node_id {
                            skip = false;
                            break;
                        }
                    }
                    if skip {
                        for filter in &splitted_filter {
                            if node_info.node_id.to_string().contains(filter) {
                                skip = false;
                                break;
                            }
                        }
                    }
                    if let Some(extended_info) = node_info.extended_info_history.last() {
                        if skip {
                            for filter in &splitted_filter {
                                if extended_info.short_name.to_lowercase().contains(filter) {
                                    skip = false;
                                    break;
                                }
                            }
                        }
                        if skip {
                            for filter in &splitted_filter {
                                if extended_info.long_name.to_lowercase().contains(filter) {
                                    skip = false;
                                    break;
                                }
                            }
                        }
                    }
                    !skip
                })
                .map(|v| *v)
                .collect()
        };
        filtered_nodes.sort_by_key(|node_info| node_info.node_id);

        let scroll_area = egui::ScrollArea::vertical();
        let scroll_area = if self.filter.is_empty() {
            scroll_area.scroll_offset(self.offset)
        } else {
            scroll_area
        };

        let mut next_page = None;
        let mut y_offset = 0.0;
        let scroll_area_output = scroll_area.show_viewport(ui, |ui, viewport| {
            const DEFAULT_HEIGHT: f32 = 20.0;

            for (index, node_info) in filtered_nodes.iter().enumerate() {
                let probably_height = *self
                    .roster_heights
                    .get(&node_info.node_id)
                    .unwrap_or(&DEFAULT_HEIGHT);

                if y_offset + probably_height < viewport.top() {
                    y_offset += probably_height;
                    ui.add_space(probably_height);
                    continue;
                }

                if y_offset > viewport.bottom() {
                    ui.add_space((filtered_nodes.len() - index) as f32 * DEFAULT_HEIGHT);
                    continue;
                }

                let (panel_command, height) = self.node_ui(ui, node_info, &mut roster_plugins);
                match panel_command {
                    PanelCommand::Nothing => {
                        self.roster_heights
                            .entry(node_info.node_id)
                            .and_modify(|v| *v = height)
                            .or_insert(height);
                        y_offset += height;
                    }
                    PanelCommand::NextPanel(panel) => {
                        next_page = Some(panel);
                        if hide_on_action {
                            self.show = false;
                        }
                        ui.ctx().request_repaint();
                        break;
                    }
                }
            }
        });

        if self.filter.is_empty() {
            self.offset = scroll_area_output.state.offset;
        }
        next_page
    }

    fn node_ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        node_info: &NodeInfo,
        roster_plugins: &mut Vec<Box<dyn Plugin + 'a>>,
    ) -> (PanelCommand, f32) {
        let telemetry_variants = node_info
            .telemetry
            .iter()
            .map(|(k, v)| (k, v.len()))
            .filter(|(_, v)| *v > 1)
            .map(|(k, _)| k)
            .collect::<Vec<_>>();

        let show_node_info = |ui: &mut egui::Ui| -> PanelCommand {
            let mut panel_command = PanelCommand::Nothing;
            if let Some(extended) = node_info.extended_info_history.last() {
                let node_id_str = node_info.node_id.to_string();
                ui.horizontal(|ui| {
                    if node_id_str.ends_with(extended.short_name.as_str()) {
                        ui.heading(node_id_str);
                    } else {
                        ui.heading(format!("{} {}", node_id_str, extended.short_name));
                    }
                });
                if extended.long_name.len() > 0 {
                    ui.label(extended.long_name.clone());
                }
            } else {
                ui.heading(node_info.node_id.to_string());
            }
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if !node_info.packet_statistics.is_empty() {
                    if ui.button("RSSI").clicked() {
                        panel_command = PanelCommand::NextPanel(Panel::Rssi(
                            node_info.node_id,
                            Default::default(),
                        ));
                        return;
                    }
                }

                if !node_info.gateway_for.is_empty() {
                    ui.menu_button(format!("Gateway {}", node_info.gateway_for.len()), |ui| {
                        if ui.button("by RSSI").clicked() {
                            panel_command = PanelCommand::NextPanel(Panel::GatewayByRSSI(
                                node_info.node_id,
                                Default::default(),
                            ));
                            return;
                        }
                        if ui.button("by Hops").clicked() {
                            panel_command = PanelCommand::NextPanel(Panel::GatewayByHops(
                                node_info.node_id,
                                Default::default(),
                            ));
                            return;
                        }
                    });
                }
            });
            panel_command
        };
        let show_plugins = |ui: &mut egui::Ui| -> PanelCommand {
            for roster_plugin in roster_plugins.iter_mut() {
                let probably_panel_command = roster_plugin.panel_node_ui(ui, node_info);
                if !matches!(probably_panel_command, PanelCommand::Nothing) {
                    return probably_panel_command;
                }
                ui.add_space(5.0);
            }

            for telemetry_variant in telemetry_variants {
                let position = self
                    .telemetry_enabled_for
                    .get(telemetry_variant)
                    .map(|v| v.iter().position(|v| v == &node_info.node_id))
                    .unwrap_or(None);
                let mut enabled = position.is_some();

                ui.checkbox(&mut enabled, telemetry_variant.to_string());

                if enabled && position.is_none() {
                    self.telemetry_enabled_for
                        .entry(*telemetry_variant)
                        .or_insert(Default::default())
                        .push(node_info.node_id);
                } else if !enabled {
                    if let Some(position) = position {
                        self.telemetry_enabled_for
                            .entry(*telemetry_variant)
                            .and_modify(|v| {
                                v.swap_remove(position);
                            });
                    }
                }
            }
            PanelCommand::Nothing
        };

        let mut panel_command = PanelCommand::Nothing;
        let height = Frame::group(ui.style())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                panel_command = show_node_info(ui);
                if matches!(panel_command, PanelCommand::Nothing) {
                    panel_command = show_plugins(ui);
                }
            })
            .response
            .rect
            .height();
        (panel_command, height)
    }
}
