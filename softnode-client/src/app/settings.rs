use std::sync::LazyLock;

use meshtastic_connect::keyring::{Keyring, key::Key, node_id::NodeId};

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct Settings {
    pub keyring_edit: String,
    pub encoder_error: Option<String>,
}

const SPACE_SIZE: f32 = 13.0;

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
            #[cfg(target_arch = "wasm32")]
            ui.label("Настройки хранятся локально в хранилище браузера");
            #[cfg(target_os = "windows")]
            ui.label("Настройки хранятся локально в %APPDATA%\\Softnode");
            #[cfg(target_os = "linux")]
            ui.label("Настройки хранятся локально в $HOME/.local/share/Softnode");
            ui.label("При обновлении версии приложения, настройки могут быть сброшены на значения по умолчанию");
            ui.add_space(SPACE_SIZE);

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Theme");
                egui::widgets::global_theme_preference_buttons(ui);

                let theme =
                    egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());

                ui.add_space(SPACE_SIZE);
                ui.heading("Ключи");

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
                ui.collapsing("Информация о ключах", |ui| {
                    #[cfg(target_arch = "wasm32")]
                    ui.label("Ключи не передаются на сервер, а хранятся в браузере в локальном хранилище в открытом виде.");

                    ui.label("При применении ключей, все сообщения будут обновлены");

                    static KEYRING_YAML: LazyLock<String> = LazyLock::new(|| {
                        let mut example_keyring = Keyring::new();
                        example_keyring.add_channel("SecretChannel", Key::K256(Default::default())).unwrap();
                        example_keyring.add_channel("AES-128_Channel", Key::K128(Default::default())).unwrap();
                        example_keyring.add_peer(NodeId::from(0xb00bb00b), Default::default()).unwrap();
                        example_keyring.add_remote_peer(NodeId::from(0xdeadbeef), Default::default()).unwrap();
                        match serde_yaml_ng::to_string(&example_keyring) {
                            Ok(yaml) => yaml,
                            Err(err) => format!("serialize error: {err}"),
                        }
                    });

                    ui.label("Пример конфигурации:");
                    let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());
                    egui_extras::syntax_highlighting::code_view_ui(ui, &theme, KEYRING_YAML.as_str(), "yaml");
                });
                ui.add(
                    egui::TextEdit::multiline(&mut self.keyring_edit)
                        .font(egui::TextStyle::Monospace) // for cursor height
                        .code_editor()
                        .desired_rows(12)
                        .lock_focus(true)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );

                if let Some(error_text) = &self.encoder_error {
                    ui.label(error_text);
                }

                if ui.button("Применить ключи").clicked() {
                    match serde_yaml_ng::from_str::<Keyring>(&self.keyring_edit) {
                        Ok(new_keyring) => {
                            log::info!("New keyring: {:?}", new_keyring);
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
