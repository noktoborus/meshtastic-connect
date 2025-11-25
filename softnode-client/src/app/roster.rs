use crate::app::{
    ListPanelFilter,
    data::{NodeInfo, TelemetryVariant},
    settings::Settings,
    telemetry::Telemetry,
};
use egui::{Color32, RichText, Vec2};
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
        node_selected: Option<NodeId>,
        filter_by: ListPanelFilter,
        hide_on_action: bool,
    ) -> Option<Panel> {
        let mut next_page = None;

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

        let rows_size = 40.0;
        for roster_plugin in roster_plugins.iter_mut() {
            roster_plugin.panel_header_ui(ui);
        }

        let mut filtered_nodes = if self.filter.is_empty() {
            nodes
        } else {
            let normalized_filter = self.filter.to_lowercase();
            let splitted_filter = normalized_filter.split_whitespace().collect::<Vec<&str>>();
            nodes
                .iter()
                .filter(|node_info| {
                    let mut skip = true;
                    for filter in &splitted_filter {
                        if node_info.node_id.to_string().contains(filter) {
                            skip = false;
                            break;
                        }
                    }
                    if let Some(extended_info) = node_info.extended_info_history.last() {
                        for filter in &splitted_filter {
                            if extended_info.short_name.to_lowercase().contains(filter) {
                                skip = false;
                                break;
                            }
                        }
                        for filter in &splitted_filter {
                            if extended_info.long_name.to_lowercase().contains(filter) {
                                skip = false;
                                break;
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
        let scroll_area_output =
            scroll_area.show_rows(ui, rows_size, filtered_nodes.len(), |ui, row_range| {
                for node_info in &filtered_nodes[row_range.start..row_range.end] {
                    let mut show_and_filter_by_telemetry = false;
                    match filter_by {
                        ListPanelFilter::None => {}
                        ListPanelFilter::Telemetry => {
                            show_and_filter_by_telemetry = true;
                        }
                        ListPanelFilter::Gateway => {
                            if node_info.gateway_for.is_empty() {
                                continue;
                            }
                        }
                    }

                    let telemetry_variants = show_and_filter_by_telemetry.then(|| {
                        node_info
                            .telemetry
                            .iter()
                            .map(|(k, v)| (k, v.len()))
                            .filter(|(_, v)| *v > 1)
                            .map(|(k, _)| k)
                            .collect::<Vec<_>>()
                    });
                    if let Some(ref telemetry_variants) = telemetry_variants {
                        if telemetry_variants.is_empty() {
                            continue;
                        }
                    }
                    ui.separator();

                    if let Some(extended) = node_info.extended_info_history.last() {
                        let node_id_str = node_info.node_id.to_string();
                        ui.horizontal(|ui| {
                            if node_selected
                                .map(|v| v == node_info.node_id)
                                .unwrap_or(false)
                            {
                                ui.heading(
                                    RichText::new("➧").color(Color32::from_rgb(0, 153, 255)),
                                );
                            }
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
                                if hide_on_action {
                                    self.show = false;
                                }
                                next_page =
                                    Some(Panel::Rssi(node_info.node_id, Default::default()));
                            }
                        }

                        if !node_info.gateway_for.is_empty() {
                            ui.menu_button(
                                format!("Gateway {}", node_info.gateway_for.len()),
                                |ui| {
                                    if ui.button("by RSSI").clicked() {
                                        if hide_on_action {
                                            self.show = false;
                                        }
                                        next_page = Some(Panel::GatewayByRSSI(
                                            node_info.node_id,
                                            Default::default(),
                                        ))
                                    }
                                    if ui.button("by Hops").clicked() {
                                        if hide_on_action {
                                            self.show = false;
                                        }
                                        next_page = Some(Panel::GatewayByHops(
                                            node_info.node_id,
                                            Default::default(),
                                        ))
                                    }
                                },
                            );
                        }
                    });
                    for roster_plugin in roster_plugins.iter_mut() {
                        match roster_plugin.panel_node_ui(ui, node_info) {
                            PanelCommand::Nothing => {}
                            // PanelCommand::HideRoster => {
                            //     self.show = false;
                            //     ui.ctx().request_repaint();
                            //     return;
                            // }
                            PanelCommand::NextPanel(panel) => {
                                if hide_on_action {
                                    self.show = false;
                                }
                                next_page = Some(panel);
                                ui.ctx().request_repaint();
                                return;
                            }
                        }
                        ui.add_space(5.0);
                    }

                    if let Some(telemetry_variants) = telemetry_variants {
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
                    }
                    ui.add_space(10.0);
                }
            });

        if self.filter.is_empty() {
            self.offset = scroll_area_output.state.offset;
        }

        next_page
    }
}
