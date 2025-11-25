use chrono::NaiveDate;
use egui::{Label, ScrollArea, TextWrapMode};
use egui_extras::{Column, TableBuilder, TableRow};

use super::data::JournalData;

const SHOW_LIMIT_BASE: usize = 150;

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct Journal {
    show_limit: usize,
}

impl Journal {
    pub fn new() -> Self {
        Journal {
            show_limit: SHOW_LIMIT_BASE,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, journal: &Vec<JournalData>) {
        let text_style = egui::TextStyle::Body;
        let row_height = ui.text_style_height(&text_style);
        let heading_style = egui::TextStyle::Heading;
        let heading_height = ui.text_style_height(&heading_style);
        let mut date: Option<NaiveDate> = None;

        ScrollArea::horizontal().show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Min))
                .resizable(false)
                .columns(Column::auto(), 8)
                .column(Column::remainder().at_least(200.0))
                .header(heading_height, |mut header| {
                    header.col(|ui| {
                        ui.heading("👤")
                            .on_hover_text("Message marked as PKI encrypted");
                    });
                    header.col(|ui| {
                        ui.heading("🔒")
                            .on_hover_text("PKI or symmetric encryption is used.");
                    });
                    header.col(|ui| {
                        if let Some(date) = date {
                            ui.heading(date.format("%Y-%m-%d").to_string());
                        } else {
                            ui.heading("Time");
                        }
                    });
                    header.col(|ui| {
                        ui.heading("Gateway");
                    });
                    header.col(|ui| {
                        ui.heading("Ch.");
                    });
                    header.col(|ui| {
                        ui.heading("From");
                    });
                    header.col(|ui| {
                        ui.heading("To");
                    });
                    header.col(|ui| {
                        ui.heading("Type");
                    });
                    header.col(|ui| {
                        ui.heading("Hint");
                    });
                })
                .body(|body| {
                    let add_row_content = |mut row: TableRow<'_, '_>| {
                        let entry = &journal[journal.len() - row.index() - 1];
                        date = Some(entry.timestamp.date_naive());

                        row.col(|ui| {
                            if entry.is_pki {
                                ui.label("👤").on_hover_text("pki_encrypted flag is set");
                            } else {
                                ui.spacing();
                            }
                        });
                        row.col(|ui| {
                            if entry.is_encrypted {
                                ui.label("🔒").on_hover_text("Encrypted");
                            } else {
                                ui.spacing();
                            }
                        });

                        row.col(|ui| {
                            let text = entry.timestamp.format("%H:%M:%S%.3f");
                            ui.add(Label::new(text.to_string()).wrap_mode(TextWrapMode::Extend));
                        });

                        let gateway_str = entry
                            .gateway
                            .map(|g| g.to_string())
                            .unwrap_or_else(|| entry.relay.to_string());
                        row.col(|ui| {
                            ui.label(gateway_str);
                        });

                        row.col(|ui| {
                            ui.label(format!("0x{:02x}", entry.channel));
                        });
                        row.col(|ui| {
                            ui.label(entry.from.to_string());
                        });
                        row.col(|ui| {
                            ui.label(entry.to.to_string());
                        });
                        row.col(|ui| {
                            ui.label(entry.message_type.as_str());
                        });
                        row.col(|ui| {
                            let text = entry.message_hint.as_str();
                            ui.add(Label::new(text.to_string()).wrap_mode(TextWrapMode::Wrap));
                        });
                    };

                    body.rows(row_height, journal.len(), add_row_content);
                });
        });
    }
}
