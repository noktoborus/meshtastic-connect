use meshtastic_connect::keyring::Keyring;

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct Settings {
    pub keyring_edit: String,
    pub encoder_error: Option<String>,
}

const SPACE_SIZE: f32 = 3.0;

impl Settings {
    pub fn new(keyring: &Keyring) -> Self {
        Self {
            encoder_error: None,
            keyring_edit: serde_yaml_ng::to_string(keyring).unwrap(),
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, keyring: &mut Keyring) -> bool {
        let mut need_update = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Theme");
                egui::widgets::global_theme_preference_buttons(ui);

                let theme =
                    egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());

                ui.add_space(SPACE_SIZE);
                ui.heading("Keyring");

                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                    let mut layout_job = egui_extras::syntax_highlighting::highlight(
                        ui.ctx(),
                        ui.style(),
                        &theme,
                        buf.as_str(),
                        "yaml",
                    );
                    layout_job.wrap.max_width = wrap_width;
                    ui.fonts(|f| f.layout_job(layout_job))
                };

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.keyring_edit)
                            .font(egui::TextStyle::Monospace) // for cursor height
                            .code_editor()
                            .desired_rows(12)
                            .lock_focus(true)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layouter),
                    );
                });

                if let Some(error_text) = &self.encoder_error {
                    ui.label(error_text);
                }

                if ui.button("Save and reload").clicked() {
                    match serde_yaml_ng::from_str::<Keyring>(&self.keyring_edit) {
                        Ok(new_keyring) => {
                            *keyring = new_keyring;
                            self.encoder_error = None;
                            need_update = true;
                        }
                        Err(error) => {
                            log::error!("keyring parsing error: {error}");
                            self.encoder_error = Some(error.to_string());
                        }
                    }
                }

                ui.add_space(SPACE_SIZE);
            });
        });

        need_update
    }
}
