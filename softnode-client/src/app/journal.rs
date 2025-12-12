use egui::{Align2, Area, Frame, Label, RichText, ScrollArea, TextWrapMode};
use meshtastic_connect::keyring::node_id::NodeId;

use crate::app::{byte_node_id::ByteNodeId, data::NodeInfo, node_book::NodeBook, roster};

use super::data::JournalData;

const SHOW_LIMIT_BASE: usize = 150;

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct ScrollInfo {
    y_offset: f32,
    journal_length: usize,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct JournalPanel {
    show_limit: usize,
    #[serde(skip)]
    journal_rows_height: Vec<Option<f32>>,
    #[serde(skip)]
    scroll_info: Option<ScrollInfo>,
}

impl JournalPanel {
    pub fn new() -> Self {
        JournalPanel {
            show_limit: SHOW_LIMIT_BASE,
            journal_rows_height: Vec::new(),
            scroll_info: None,
        }
    }

    fn show_journal_entry(
        &self,
        ui: &mut egui::Ui,
        journal: &Vec<JournalData>,
        journal_index: usize,
    ) -> Result<f32, ()> {
        if journal_index >= journal.len() {
            return Err(());
        }
        let entry = &journal[journal_index];
        let previous_date = (journal_index > 0)
            .then(|| {
                journal
                    .get(journal_index - 1)
                    .map(|v| v.timestamp.date_naive())
            })
            .flatten()
            .unwrap_or_default();

        let header = |ui: &mut egui::Ui| {
            ui.vertical(|ui| {
                if journal_index == journal.len() - 1
                    || previous_date != entry.timestamp.date_naive()
                {
                    ui.heading(entry.timestamp.format("%Y-%m-%d").to_string());
                }
                ui.horizontal(|ui| {
                    let timestamp_text = entry.timestamp.format("%H:%M:%S");
                    ui.add(Label::new(timestamp_text.to_string()).wrap_mode(TextWrapMode::Extend));
                    ui.label(format!("0x{:02x}", entry.channel))
                        .on_hover_text("Channel's hash or number");

                    if entry.to != NodeId::broadcast() {
                        ui.label(entry.from.to_string())
                            .on_hover_text("Sender Node ID");
                        ui.label("➡");
                        ui.label(entry.to.to_string())
                            .on_hover_text("Recipient Node ID");
                    } else {
                        ui.label(entry.from.to_string())
                            .on_hover_text("Sender Node ID for broadcast message");
                    }

                    if let Some(gateway) = entry.gateway {
                        if gateway == entry.from {
                            ui.small(RichText::new(gateway.to_string()).strong())
                                .on_hover_text("Gateway is same as sender");
                        } else {
                            ui.small(gateway.to_string())
                                .on_hover_text("Gateway Node ID");
                        }
                    }

                    if entry.relay != ByteNodeId::zero() {
                        ui.small(entry.relay.to_string())
                            .on_hover_text("Relay Node ID");
                    }

                    if entry.via_mqtt {
                        ui.small("via MQTT")
                            .on_hover_text("The packet route passed through MQTT");
                    }

                    if (entry.hop_start >= entry.hop_limit
                        && entry.hop_start - entry.hop_limit == 0)
                        || entry.hop_limit == 7
                    {
                        ui.small(RichText::new("direct").strong())
                            .on_hover_text(format!(
                                "Direct connection to gateway (limit: {}, start: {})",
                                entry.hop_limit, entry.hop_start
                            ));
                    } else if entry.hop_start >= entry.hop_limit {
                        let away = entry.hop_start - entry.hop_limit;
                        ui.small(away.to_string()).on_hover_text(format!(
                            "{} hops away (limit: {}, start: {})",
                            away, entry.hop_limit, entry.hop_start
                        ));
                    } else {
                        ui.small(format!("{}/{}", entry.hop_limit, entry.hop_start))
                            .on_hover_text(format!(
                                "hop limit: {}, hop start: {}",
                                entry.hop_limit, entry.hop_start
                            ));
                    }
                });
            })
        };

        let body = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                if entry.is_encrypted {
                    ui.add_sized([10.0, 10.0], Label::new("🔒"))
                        .on_hover_text("PKI or symmetric encryption is used.");
                } else {
                    ui.add_sized([10.0, 10.0], Label::new(""));
                }
                if entry.is_pki {
                    ui.add_sized([10.0, 10.0], Label::new("👤"))
                        .on_hover_text("Message marked as PKI encrypted");
                } else {
                    ui.add_sized([10.0, 10.0], Label::new(""));
                }

                if entry.message_type != "TEXT_MESSAGE_APP" {
                    ui.label(entry.message_type.as_str());
                }

                let text = entry.message_hint.as_str();
                ui.add(Label::new(RichText::new(text).monospace()).wrap_mode(TextWrapMode::Wrap));
            })
        };
        Ok(Frame::default()
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    header(ui);
                    body(ui);
                })
            })
            .response
            .rect
            .height())
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, journal: &Vec<JournalData>) {
        let mut scroll_area = ScrollArea::both().auto_shrink([false; 2]);
        let default_message_height = 20.0;
        let journal_length = journal.len();
        if self.journal_rows_height.len() != journal_length {
            self.journal_rows_height.resize(journal_length, None);
        }
        if let Some(scroll_info) = self.scroll_info.take() {
            let y_offset = scroll_info.y_offset
                + self.journal_rows_height[scroll_info.journal_length..journal_length]
                    .iter()
                    .map(|h| h.unwrap_or(default_message_height))
                    .sum::<f32>();

            scroll_area = scroll_area.vertical_scroll_offset(y_offset);
        }

        let y_offset = scroll_area
            .show_viewport(ui, |ui, viewport| {
                let mut y_offset = 0.0;

                for journal_index in (0..journal_length).rev() {
                    if y_offset > viewport.bottom() {
                        ui.add_space(default_message_height * journal_index as f32);
                        break;
                    }

                    if let Some(height_entry) = self.journal_rows_height.get(journal_index).clone()
                    {
                        let hypothetical_height = height_entry.unwrap_or(default_message_height);

                        if y_offset + hypothetical_height > viewport.top() {
                            if let Ok(height) = self.show_journal_entry(ui, journal, journal_index)
                            {
                                if self.journal_rows_height[journal_index] != Some(height) {
                                    self.journal_rows_height[journal_index] = Some(height);
                                }
                                y_offset += height;
                            } else {
                                log::error!("Failed to show journal entry");
                                break;
                            }
                        } else {
                            y_offset += hypothetical_height;
                            ui.add_space(hypothetical_height);
                        }
                    } else {
                        log::error!("Journal row height not found: probably memory issue");
                        break;
                    }
                }
            })
            .state
            .offset
            .y;

        if y_offset != 0.0 {
            self.scroll_info = Some(ScrollInfo {
                y_offset,
                journal_length,
            });
            Area::new(ui.id())
                .anchor(Align2::RIGHT_TOP, [-15.0, 35.0])
                .show(ui.ctx(), |ui| {
                    if ui.button("⬆").clicked() {
                        self.scroll_info = Some(ScrollInfo {
                            y_offset: 0.0,
                            journal_length,
                        });
                    }
                });
        }
    }
}

pub struct JournalRosterPlugin<'a> {
    _journal: &'a mut JournalPanel,
}

impl<'a> JournalRosterPlugin<'a> {
    pub fn new(journal: &'a mut JournalPanel) -> Self {
        Self { _journal: journal }
    }
}

impl<'a> roster::Plugin for JournalRosterPlugin<'a> {
    fn panel_header_ui(
        self: &mut Self,
        _ui: &mut egui::Ui,
        _nodebook: &mut NodeBook,
    ) -> roster::PanelCommand {
        roster::PanelCommand::Nothing
    }

    fn panel_node_ui(
        self: &mut Self,
        _ui: &mut egui::Ui,
        _node_info: &NodeInfo,
        _nodebook: &mut NodeBook,
    ) -> roster::PanelCommand {
        roster::PanelCommand::Nothing
    }
}
