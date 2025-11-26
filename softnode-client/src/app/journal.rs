use chrono::NaiveDate;
use egui::{Label, RichText, ScrollArea, TextWrapMode};
use egui_extras::{Column, TableBuilder};
use meshtastic_connect::keyring::node_id::NodeId;

use crate::app::{byte_node_id::ByteNodeId, data::NodeInfo, roster};

use super::data::JournalData;

const SHOW_LIMIT_BASE: usize = 150;

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct JournalPanel {
    show_limit: usize,
    #[serde(skip)]
    journal_rows_height: Vec<Option<f32>>,
}

impl JournalPanel {
    pub fn new() -> Self {
        JournalPanel {
            show_limit: SHOW_LIMIT_BASE,
            journal_rows_height: Vec::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, journal: &Vec<JournalData>) {
        let text_style = egui::TextStyle::Body;
        // static row_height is incorrect:
        // text may use many lines, so we need to calculate it dynamically
        let default_row_height = ui.text_style_height(&text_style) * 2.0;

        if self.journal_rows_height.len() != journal.len() {
            self.journal_rows_height.resize(journal.len(), None);
        }

        ScrollArea::horizontal().show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Min))
                .resizable(false)
                .column(Column::remainder())
                .body(|body| {
                    let calculated_heights = &self.journal_rows_height;
                    // use to reverse table
                    let reverse_index = |i: usize| journal.len() - i - 1;
                    let row_height = |i: usize| {
                        calculated_heights
                            .get(reverse_index(i))
                            .and_then(|h| *h)
                            .unwrap_or(default_row_height)
                    };
                    let journal_length = journal.len();
                    let mut new_lengths = Vec::new();

                    body.heterogeneous_rows((0..journal_length).map(row_height), |mut row| {
                        // It is revered table
                        let index = reverse_index(row.index());
                        let entry = &journal[index];
                        let previous_date = (index > 0)
                            .then(|| journal.get(index - 1).map(|v| v.timestamp.date_naive()))
                            .flatten()
                            .unwrap_or_default();

                        row.col(|ui| {
                            let response = ui.vertical(|ui| {
                                ui.vertical(|ui| {
                                    if previous_date != entry.timestamp.date_naive() {
                                        ui.heading(entry.timestamp.format("%Y-%m-%d").to_string());
                                    }
                                    ui.horizontal(|ui| {
                                        let timestamp_text = entry.timestamp.format("%H:%M:%S");
                                        ui.add(
                                            Label::new(timestamp_text.to_string())
                                                .wrap_mode(TextWrapMode::Extend),
                                        );
                                        ui.label(format!("0x{:02x}", entry.channel))
                                            .on_hover_text("Channel's hash or number");


                                        if entry.to != NodeId::broadcast() {
                                            ui.label(entry.from.to_string()).on_hover_text("Sender Node ID");
                                            ui.label("➡");
                                            ui.label(entry.to.to_string())
                                                .on_hover_text("Recipient Node ID");
                                        } else {
                                            ui.label(entry.from.to_string()).on_hover_text("Sender Node ID for broadcast message");
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
                                            ui.small("via MQTT").on_hover_text(
                                                "The packet route passed through MQTT",
                                            );
                                        }

                                        if (entry.hop_start >= entry.hop_limit
                                            && entry.hop_start - entry.hop_limit == 0)
                                            || entry.hop_limit == 7
                                        {
                                            ui.small(RichText::new("direct").strong()).on_hover_text(format!(
                                                "Direct connection to gateway (limit: {}, start: {})",
                                                entry.hop_limit, entry.hop_start
                                            ));
                                        } else if entry.hop_start >= entry.hop_limit {
                                            let away = entry.hop_start - entry.hop_limit;
                                            ui.small(away.to_string()).on_hover_text(format!(
                                                "Away at {} hops (limit: {}, start: {})",
                                                away, entry.hop_limit, entry.hop_start
                                            ));
                                        } else {
                                            ui.small(format!(
                                                "{}/{}",
                                                entry.hop_limit, entry.hop_start
                                            ))
                                            .on_hover_text(format!(
                                                "hop limit: {}, hop start: {}",
                                                entry.hop_limit, entry.hop_start
                                            ));
                                        }
                                    });
                                });

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
                                    ui.add(
                                        Label::new(RichText::new(text).monospace())
                                            .wrap_mode(TextWrapMode::Wrap),
                                    );
                                });
                            });

                            let height = Some(response.response.rect.height());
                            if self.journal_rows_height[index] != height {
                                new_lengths.push((index, height));
                            }
                        });
                    });

                    if !new_lengths.is_empty() {
                        for (index, height) in new_lengths {
                            self.journal_rows_height[index] = height;
                        }
                    }
                });
        });
    }
}

pub struct JournalRosterPlugin<'a> {
    journal: &'a mut JournalPanel,
}

impl<'a> JournalRosterPlugin<'a> {
    pub fn new(journal: &'a mut JournalPanel) -> Self {
        Self { journal }
    }
}

impl<'a> roster::Plugin for JournalRosterPlugin<'a> {
    fn panel_header_ui(self: &mut Self, ui: &mut egui::Ui) -> roster::PanelCommand {
        roster::PanelCommand::Nothing
    }

    fn panel_node_ui(
        self: &mut Self,
        ui: &mut egui::Ui,
        node_info: &NodeInfo,
    ) -> roster::PanelCommand {
        roster::PanelCommand::Nothing
    }
}
