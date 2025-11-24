use crate::app::{
    ListPanelFilter,
    data::{NodeInfo, TelemetryVariant},
    journal::Journal,
    map::MapRosterPlugin,
    settings::Settings,
    telemetry::Telemetry,
};
use egui::{Color32, RichText};
use meshtastic_connect::keyring::node_id::NodeId;
use std::collections::HashMap;

#[derive(serde::Deserialize, serde::Serialize)]
pub enum Panel {
    Journal(Journal),
    Telemetry(Telemetry),
    Settings(Settings),
    Rssi(NodeId, Telemetry),
    Gateways(Option<NodeId>, Telemetry),
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
        roster_plugin: &mut MapRosterPlugin<'a>,
        mut nodes: Vec<&NodeInfo>,
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

        egui::ScrollArea::vertical().show(ui, |ui| {
            roster_plugin.panel_header_ui(ui);

            nodes.sort_by_key(|node_info| node_info.node_id);
            for node_info in nodes {
                if !self.filter.is_empty() {
                    let filter = self.filter.to_uppercase();
                    let mut skip = true;
                    if node_info
                        .node_id
                        .to_string()
                        .to_uppercase()
                        .contains(filter.as_str())
                    {
                        skip = false;
                    }
                    if let Some(extended_info) = node_info.extended_info_history.last() {
                        if extended_info
                            .short_name
                            .to_uppercase()
                            .contains(filter.as_str())
                        {
                            skip = false;
                        }
                        if extended_info
                            .long_name
                            .to_uppercase()
                            .contains(filter.as_str())
                        {
                            skip = false;
                        }
                    }
                    if skip {
                        continue;
                    }
                }

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
                            ui.heading(RichText::new("➧").color(Color32::from_rgb(0, 153, 255)));
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
                            next_page = Some(Panel::Rssi(node_info.node_id, Default::default()));
                        }
                    }

                    if !node_info.gateway_for.is_empty() {
                        if ui
                            .button(format!("Gateway {}", node_info.gateway_for.len()))
                            .clicked()
                        {
                            if hide_on_action {
                                self.show = false;
                            }
                            next_page =
                                Some(Panel::Gateways(Some(node_info.node_id), Default::default()))
                        }
                    }
                });
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

        next_page
    }
}
