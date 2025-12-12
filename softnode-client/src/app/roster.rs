use crate::app::{
    data::{NodeInfo, NodeInfoExtended, PublicKey, TelemetryValue, TelemetryVariant},
    node_book::NodeBook,
    node_filter::NodeFilter,
    radio_telemetry::RadioTelemetry,
    settings::Settings,
    telemetry::Telemetry,
    telemetry_formatter::TelemetryFormatter,
    time_format::format_timediff,
};
use egui::{Align, Button, Color32, Frame, Layout, RichText, Stroke, Vec2};
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
    NodeDump,
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
    fn panel_header_ui(self: &mut Self, ui: &mut egui::Ui, nodebook: &mut NodeBook)
    -> PanelCommand;
    fn panel_node_ui(
        self: &mut Self,
        ui: &mut egui::Ui,
        node_info: &NodeInfo,
        nodebook: &mut NodeBook,
    ) -> PanelCommand;
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
        telemetry_formatter: &TelemetryFormatter,
        mut roster_plugins: Vec<&'a mut dyn Plugin>,
        node_filter: &mut NodeFilter,
        nodebook: &mut NodeBook,
        nodes: &HashMap<NodeId, NodeInfo>,
        hide_on_action: bool,
    ) -> Option<Panel> {
        ui.horizontal(|ui| {
            egui::TextEdit::singleline(&mut self.filter)
                .desired_width(f32::INFINITY)
                .hint_text("Search node by id or name")
                .show(ui);
        });

        for roster_plugin in roster_plugins.iter_mut() {
            roster_plugin.panel_header_ui(ui, nodebook);
        }

        node_filter.update_filter(self.filter.as_str());

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
                    node_filter.ui(ui);
                })
                .response
                .rect
                .height();

            let excess_nodebook_clone = nodebook.clone();
            let mut filtered_nodes: Vec<(&NodeInfo, Selection)> = node_filter
                .seeker_for(nodes, &excess_nodebook_clone)
                .map(|node_info| {
                    let mut selection = Selection::None;
                    for roster_plugin in roster_plugins.iter_mut() {
                        if roster_plugin.node_is_dropped(node_info) {
                            return (None, Selection::None);
                        }
                        let nselection = roster_plugin.node_is_selected(node_info);
                        if nselection != Selection::None {
                            selection = nselection;
                        }
                    }
                    (Some(node_info), selection)
                })
                .filter(|(node_info_or_not, _)| node_info_or_not.is_some())
                .map(|(node_info, selection)| (node_info.unwrap(), selection))
                .collect();

            y_offset += Frame::new()
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("nodes: {}", filtered_nodes.len()));
                        if ui.button("as text").clicked() {
                            next_page = Some(Panel::NodeDump);
                            if hide_on_action {
                                self.show = false;
                            }
                            ui.ctx().request_repaint();
                        };
                    });
                })
                .response
                .rect
                .height();

            filtered_nodes.sort_by_key(|(node_info, _)| node_info.node_id);
            filtered_nodes.sort_by_key(|(_, selection)| *selection);

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
                let (panel_command, height) = self.node_ui(
                    ui,
                    nodebook,
                    node_info,
                    &mut roster_plugins,
                    telemetry_formatter,
                    *selection,
                );
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
        nodebook: &mut NodeBook,
        node_info: &NodeInfo,
        roster_plugins: &mut Vec<&'a mut dyn Plugin>,
        telemetry_formatter: &TelemetryFormatter,
        selection: Selection,
    ) -> (PanelCommand, f32) {
        let current_datetime = chrono::Utc::now();
        let label_last_seen = |ui: &mut egui::Ui| {
            if let Some(label) = node_info
                .packet_statistics
                .last()
                .map(|v| format_timediff(v.timestamp, current_datetime))
                .flatten()
            {
                ui.label(label).on_hover_text("Last seen");
            }
        };

        let show_extended = |ui: &mut egui::Ui, extended: &NodeInfoExtended, is_via_mqtt: bool| {
            let node_id_str = node_info.node_id.to_string();
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(false, extended.short_name.clone())
                    .on_hover_text("Node's short name\nclick to copy")
                    .clicked()
                {
                    ui.ctx()
                        .copy_text(format!("{} {}", node_id_str, extended.short_name));
                }

                if extended.long_name.len() > 0 {
                    let long_name = RichText::new(extended.long_name.clone()).strong();
                    let label =
                        Button::selectable(false, long_name).wrap_mode(egui::TextWrapMode::Wrap);
                    if ui
                        .add(label)
                        .on_hover_text("Node's long name\nclick to copy")
                        .clicked()
                    {
                        ui.ctx()
                            .copy_text(format!("{} {}", node_id_str, extended.long_name));
                    };
                }
            });
            ui.horizontal(|ui| {
                if is_via_mqtt {
                    ui.label(RichText::new("").color(Color32::LIGHT_GRAY))
                        .on_hover_text("Some packets heard via MQTT");
                }
                if let PublicKey::Key(pkey) = extended.pkey {
                    let key_size = pkey.as_bytes().len() * 8;
                    let hover_text = format!("{} bit key: {}\nclick to copy key", key_size, pkey);
                    if ui.selectable_label(false, RichText::new("🔒").color(Color32::LIGHT_GREEN))
                        .on_hover_text(hover_text).clicked() {
                            ui.ctx().copy_text(pkey.to_string());
                        }
                } else if let PublicKey::Compromised(pkey) = extended.pkey {
                    let key_size = pkey.as_bytes().len() * 8;
                    let hover_text = format!("{} bit key: {}\nbut key used by another node\nclick to copy key", key_size, pkey);
                    if ui.selectable_label(false, RichText::new("🔒").color(Color32::YELLOW))
                        .on_hover_text(hover_text).clicked() {
                            ui.ctx().copy_text(pkey.to_string());
                        }
                } else {
                    if !extended.is_licensed {
                        ui.selectable_label(false, RichText::new("🔓").color(Color32::LIGHT_RED))
                            .on_hover_text("No key is announced");
                    }
                };

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
                    ui.selectable_label(false, RichText::new(node_id_str))
                        .on_hover_text(format!("Announced: {}", extended.announced_node_id))
                }
                else {
                    ui.selectable_label(false, RichText::new(node_id_str.clone()).color(Color32::LIGHT_RED))
                        .on_hover_text(format!("NodeID is {} but announced id is {}", node_id_str, extended.announced_node_id))
                }.clicked().then(|| ui.ctx().copy_text(node_info.node_id.into()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    label_last_seen(ui);
                });
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
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(false, node_info.node_id.to_string())
                            .on_hover_text("No NodeInfo announced")
                            .clicked()
                        {
                            ui.ctx().copy_text(node_info.node_id.into());
                        };
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            label_last_seen(ui);
                        });
                    });
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
                    let timediff = node_info
                        .gateway_for
                        .values()
                        .map(|v| v.iter().map(|v| v.timestamp).max())
                        .flatten()
                        .max()
                        .map(|timestamp| format_timediff(timestamp, current_datetime))
                        .flatten();
                    let label = if let Some(timediff) = timediff {
                        format!("Gateway {} ({} ago)", node_info.gateway_for.len(), timediff)
                    } else {
                        format!("Gateway {}", node_info.gateway_for.len())
                    };
                    ui.menu_button(label, |ui| {
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
        let mut show_telemetry_button =
            |ui: &mut egui::Ui,
             telemetry_variant: &TelemetryVariant,
             telemetry_value: &TelemetryValue,
             previous_value: Option<&TelemetryValue>| {
                let telemetry_enabled_index = self
                    .telemetry_enabled_for
                    .get(telemetry_variant)
                    .map(|v| v.iter().position(|v| v == &node_info.node_id))
                    .unwrap_or(None);

                let tooltip = telemetry_variant.to_string();
                let mut enabled = telemetry_enabled_index.is_some();
                let mut text = RichText::new(
                    telemetry_formatter.format(telemetry_value.value, *telemetry_variant),
                );
                if let Some(previous_value) = previous_value {
                    if previous_value.value > telemetry_value.value {
                        text = text.color(Color32::LIGHT_RED);
                    } else if previous_value.value < telemetry_value.value {
                        text = text.color(Color32::LIGHT_GREEN);
                    }
                }
                let label = ui
                    .selectable_label(enabled, text)
                    .on_hover_text(tooltip.as_str());
                if label.long_touched() {
                    label.show_tooltip_text(tooltip.as_str());
                };
                if label.clicked() {
                    enabled = !enabled;
                };

                if enabled && telemetry_enabled_index.is_none() {
                    self.telemetry_enabled_for
                        .entry(*telemetry_variant)
                        .or_insert(Default::default())
                        .push(node_info.node_id);
                } else if !enabled {
                    if let Some(position) = telemetry_enabled_index {
                        self.telemetry_enabled_for
                            .entry(*telemetry_variant)
                            .and_modify(|v| {
                                v.swap_remove(position);
                            });
                    }
                }
            };

        let mut show_plugins = |ui: &mut egui::Ui| -> PanelCommand {
            for roster_plugin in roster_plugins.iter_mut() {
                let probably_panel_command = roster_plugin.panel_node_ui(ui, node_info, nodebook);
                if !matches!(probably_panel_command, PanelCommand::Nothing) {
                    return probably_panel_command;
                }
                ui.add_space(5.0);
            }

            let device_telemetry = [
                TelemetryVariant::ChannelUtilization,
                TelemetryVariant::AirUtilTx,
                TelemetryVariant::Voltage,
                TelemetryVariant::BatteryLevel,
            ];

            let mut telemetry_variants = node_info
                .telemetry
                .iter()
                .map(|(k, _)| *k)
                .filter(|k| !device_telemetry.contains(k))
                .collect::<Vec<_>>();
            telemetry_variants.sort();

            ui.horizontal_wrapped(|ui| {
                for telemetry_variant in device_telemetry.iter() {
                    if let Some(telemetry_values) = node_info.telemetry.get(telemetry_variant) {
                        let mut iterator = telemetry_values.values.iter();
                        if let Some(telemetry_value) = iterator.next_back() {
                            let previous = iterator.next_back();
                            show_telemetry_button(ui, telemetry_variant, telemetry_value, previous);
                        }
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                for telemetry_variant in telemetry_variants.iter() {
                    if let Some(telemetry_values) = node_info.telemetry.get(telemetry_variant) {
                        let mut iterator = telemetry_values.values.iter();
                        if let Some(telemetry_value) = iterator.next_back() {
                            let previous = iterator.next_back();
                            show_telemetry_button(ui, telemetry_variant, telemetry_value, previous);
                        }
                    }
                }
            });

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
