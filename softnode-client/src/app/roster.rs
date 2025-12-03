use crate::app::{
    byte_node_id::ByteNodeId,
    data::{NodeInfo, NodeInfoExtended, TelemetryVariant},
    radio_telemetry::RadioTelemetry,
    settings::Settings,
    telemetry::Telemetry,
};
use egui::{Color32, Frame, Label, RichText, Stroke, Vec2};
use meshtastic_connect::keyring::node_id::NodeId;
use std::collections::HashMap;

#[derive(serde::Deserialize, serde::Serialize)]
pub enum Panel {
    Journal,
    Telemetry(Telemetry),
    Settings(Settings),
    Rssi(NodeId, RadioTelemetry),
    Hops(NodeId, RadioTelemetry),
    GatewayByRSSI(NodeId, RadioTelemetry),
    GatewayByHops(NodeId, RadioTelemetry),
    Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Selection {
    Primary = 10,
    Secondary = 20,
    None = 30,
}

pub trait Plugin {
    fn node_is_selected(&self, _node_info: &NodeInfo) -> Selection {
        Selection::None
    }

    fn node_is_dropped(&self, _node_info: &NodeInfo) -> bool {
        false
    }
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
        let overall_nodes = nodes.len();
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

        let normalized_filter = self.filter.to_lowercase();
        let splitted_filter = normalized_filter.split_whitespace().collect::<Vec<&str>>();
        let part_node_ids = splitted_filter
            .iter()
            .map(|splitted| ByteNodeId::try_from(*splitted).ok())
            .filter(|v| v.is_some())
            .flatten()
            .collect::<Vec<_>>();

        let mut is_dropped = |node_info: &NodeInfo| -> Option<Selection> {
            let mut selection = Selection::None;
            for roster_plugin in roster_plugins.iter_mut() {
                let nselection = roster_plugin.node_is_selected(node_info);
                if nselection != Selection::None {
                    selection = nselection;
                }
                if roster_plugin.node_is_dropped(node_info) {
                    return None;
                }
            }
            if normalized_filter.is_empty() {
                return Some(selection);
            }
            for part_node_id in &part_node_ids {
                if *part_node_id == node_info.node_id {
                    return Some(selection);
                }
            }
            for filter in &splitted_filter {
                if node_info.node_id.to_string().contains(filter) {
                    return Some(selection);
                }
            }
            if let Some(extended_info) = node_info.extended_info_history.last() {
                for filter in &splitted_filter {
                    if extended_info.short_name.to_lowercase().contains(filter) {
                        return Some(selection);
                    }
                }
                for filter in &splitted_filter {
                    if extended_info.long_name.to_lowercase().contains(filter) {
                        return Some(selection);
                    }
                }
            }
            return None;
        };

        let mut filtered_nodes: Vec<(&NodeInfo, Selection)> = nodes
            .iter()
            .map(|node_info| {
                if let Some(selection) = is_dropped(node_info) {
                    (Some(node_info), selection)
                } else {
                    (None, Selection::None)
                }
            })
            .filter(|(node_info_or_not, _)| node_info_or_not.is_some())
            .map(|(node_info, selection)| (*node_info.unwrap(), selection))
            .collect();
        filtered_nodes.sort_by_key(|(node_info, _)| node_info.node_id);
        filtered_nodes.sort_by_key(|(_, selection)| *selection);

        let scroll_area = egui::ScrollArea::vertical().auto_shrink(false);
        let scroll_area = if self.filter.is_empty() {
            scroll_area.scroll_offset(self.offset)
        } else {
            scroll_area
        };

        let mut next_page = None;
        let mut y_offset = 0.0;
        let scroll_area_output = scroll_area.show_viewport(ui, |ui, viewport| {
            const DEFAULT_HEIGHT: f32 = 20.0;

            y_offset += Frame::new()
                .show(ui, |ui| {
                    if overall_nodes != filtered_nodes.len() {
                        ui.label(format!(
                            "filtered nodes: {}/{}",
                            filtered_nodes.len(),
                            overall_nodes
                        ));
                    } else {
                        ui.label(format!("nodes: {}", filtered_nodes.len()));
                    }
                })
                .response
                .rect
                .height();

            for (index, (node_info, selection)) in filtered_nodes.iter().enumerate() {
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
                let (panel_command, height) =
                    self.node_ui(ui, node_info, &mut roster_plugins, *selection);
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
        selection: Selection,
    ) -> (PanelCommand, f32) {
        let telemetry_variants = node_info
            .telemetry
            .iter()
            .map(|(k, v)| (k, v.values.len()))
            .filter(|(_, v)| *v > 1)
            .map(|(k, _)| k)
            .collect::<Vec<_>>();

        let show_extended = |ui: &mut egui::Ui, extended: &NodeInfoExtended, is_via_mqtt: bool| {
            let node_id_str = node_info.node_id.to_string();
            ui.horizontal(|ui| {
                ui.label(extended.short_name.clone())
                    .on_hover_text("Node's short name");

                if extended.long_name.len() > 0 {
                    let long_name = RichText::new(extended.long_name.clone()).strong();
                    let label = Label::new(long_name).wrap_mode(egui::TextWrapMode::Wrap);
                    ui.add(label).on_hover_text("Node's long name");
                }
            });
            ui.horizontal(|ui| {
                if is_via_mqtt {
                    ui.label(RichText::new("").color(Color32::LIGHT_GRAY))
                        .on_hover_text("Some packets hearrd via MQTT");
                }
                if let Some(pkey) = extended.pkey {
                    let key_size = pkey.as_bytes().len() * 8;
                    ui.label(RichText::new("🔒").color(Color32::LIGHT_GREEN))
                        .on_hover_text(format!("{} bit key is announced", key_size));
                } else {
                    if !extended.is_licensed {
                        ui.label(RichText::new("🔓").color(Color32::LIGHT_RED))
                            .on_hover_text("No key is announced");
                    }
                }
                if extended.is_licensed {
                    ui.label(RichText::new("🖹").color(Color32::LIGHT_BLUE))
                        .on_hover_text("Node is licensed radio:\nmeaning that node can not\nuse crypto to send messages");
                }
                if let Some(is_unmessagable) = extended.is_unmessagable {
                    if is_unmessagable {
                        ui.label(RichText::new("🚫").color(Color32::LIGHT_RED))
                            .on_hover_text("Node is unmessagable (infrastructure)");
                    }
                }
                if node_id_str == extended.announced_node_id {
                    ui.label(RichText::new(node_id_str))
                        .on_hover_text(format!("Announced: {}", extended.announced_node_id));
                }
                else {
                    ui.label(RichText::new(node_id_str.clone()).color(Color32::LIGHT_RED))
                        .on_hover_text(format!("NodeID is {} but announced id is {}", node_id_str, extended.announced_node_id));
                }
            });
        };

        let show_node_info = |ui: &mut egui::Ui| -> PanelCommand {
            let mut panel_command = PanelCommand::Nothing;
            let via_mqtt = node_info
                .packet_statistics
                .iter()
                .any(|node_packet| node_packet.via_mqtt);
            ui.vertical(|ui| {
                if let Some(extended) = node_info.extended_info_history.last() {
                    show_extended(ui, extended, via_mqtt);
                } else {
                    if via_mqtt {
                        ui.label(RichText::new("").color(Color32::LIGHT_GRAY))
                            .on_hover_text("Some packets hearrd via MQTT");
                    }
                    ui.label(node_info.node_id.to_string())
                        .on_hover_text("No NodeInfo announced");
                }
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if !node_info.packet_statistics.is_empty() {
                    ui.menu_button(format!("Heard {}", node_info.gatewayed_by.len()), |ui| {
                        if ui.button("by RSSI").clicked() {
                            panel_command = PanelCommand::NextPanel(Panel::Rssi(
                                node_info.node_id,
                                Default::default(),
                            ));
                            return;
                        }
                        if ui.button("by Hops").clicked() {
                            panel_command = PanelCommand::NextPanel(Panel::Hops(
                                node_info.node_id,
                                Default::default(),
                            ));
                            return;
                        }
                    });
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
        let mut frame = Frame::group(ui.style());
        match selection {
            Selection::None => {}
            Selection::Primary => {
                frame = frame.stroke(Stroke::new(2.0, Color32::LIGHT_BLUE));
            }
            Selection::Secondary => {
                frame = frame.stroke(Stroke::new(0.5, Color32::LIGHT_BLUE));
            }
        }
        let height = frame
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
