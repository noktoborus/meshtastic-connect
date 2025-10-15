use chrono::NaiveDate;

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
        let mut last_date = NaiveDate::MIN;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::default().show(ui, |ui| {
                    egui::Grid::new("journal").show(ui, |ui| {
                        ui.heading("Время");
                        ui.heading("Шлюз");
                        ui.heading("От");
                        ui.heading("Кому");
                        ui.heading("Тип");
                        ui.end_row();

                        for entry in journal.iter().rev().take(self.show_limit) {
                            if last_date != entry.timestamp.date_naive() {
                                last_date = entry.timestamp.date_naive();
                                ui.heading(last_date.format("%Y-%m-%d").to_string());
                                ui.end_row();
                            }

                            ui.label(entry.timestamp.format("%H:%M:%S%.3f").to_string());

                            if let Some(gateway) = entry.gateway {
                                ui.label(gateway.to_string());
                            } else {
                                ui.label(entry.relay.to_string());
                            }
                            ui.label(entry.from.to_string());
                            ui.label(entry.to.as_str());
                            ui.label(entry.message_type.as_str());
                            ui.label(entry.message_hint.as_str());
                            ui.end_row();
                        }
                        if journal.len() > self.show_limit {
                            if ui.button("показывать больше").clicked() {
                                self.show_limit += SHOW_LIMIT_BASE;
                            }
                        }
                    });
                });
            });
    }
}
