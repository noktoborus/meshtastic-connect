pub mod byte_node_id;
pub mod data;
mod telemetry;
use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Duration, Local, Utc};
use data::{NodeInfo, StoredMeshPacket};
use egui::mutex::Mutex;
use meshtastic_connect::keyring::{Keyring, node_id::NodeId};
use telemetry::Telemetry;

#[derive(serde::Deserialize, serde::Serialize)]
enum Panel {
    Journal(Journal),
    Telemetry(Telemetry),
    Settings(Settings),
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct Journal {}

pub enum UpdateState {
    IdleSince(DateTime<Local>),
    InProgress,
    Downloaded(Vec<StoredMeshPacket>),
}

impl Default for UpdateState {
    fn default() -> Self {
        UpdateState::IdleSince(DateTime::<Local>::default())
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct PersistentData {
    nodes: HashMap<NodeId, NodeInfo>,
    keyring: Keyring,

    active_panel: Panel,
    list_panel: ListPanel,

    last_sync_point: Option<u64>,
    update_interval_secs: Duration,
}

#[derive(Default)]
pub struct SoftNodeApp {
    persistent: PersistentData,
    downloads: Arc<Mutex<UpdateState>>,
}

impl Default for PersistentData {
    fn default() -> Self {
        let mut keyring = Keyring::default();

        for channel_name in [
            "ShortTurbo",
            "ShortFast",
            "ShortSlow",
            "MediumFast",
            "MediumSlow",
            "LongFast",
            "LongModerate",
            "LongSlow",
        ] {
            keyring
                .add_channel(
                    channel_name.into(),
                    "1PG7OiApB1nwvP+rz05pAQ==".try_into().unwrap(),
                )
                .unwrap();
        }

        Self {
            nodes: HashMap::new(),
            keyring,
            active_panel: Panel::Journal(Journal::default()),
            list_panel: ListPanel {},
            last_sync_point: None,
            update_interval_secs: Duration::seconds(5),
        }
    }
}

impl PersistentData {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

impl SoftNodeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            persistent: PersistentData::new(cc),
            ..Default::default()
        }
    }
}

impl SoftNodeApp {
    fn update_data(&mut self, ctx: &egui::Context) {
        let downloads: &mut UpdateState = &mut self.downloads.lock();
        match downloads {
            UpdateState::InProgress => {}
            UpdateState::Downloaded(stored_mesh_packets) => {
                let mesh_packets_count = stored_mesh_packets.len();

                log::info!("Fetched {} packets", mesh_packets_count);
                for stored_mesh_packet in stored_mesh_packets.drain(..) {
                    let node_id = stored_mesh_packet.header.from;

                    let stored_mesh_packet = stored_mesh_packet.decrypt(&self.persistent.keyring);

                    let entry =
                        self.persistent
                            .nodes
                            .entry(node_id)
                            .or_insert_with(|| data::NodeInfo {
                                node_id,
                                ..Default::default()
                            });

                    entry.update(&stored_mesh_packet);
                    self.persistent.last_sync_point = Some(stored_mesh_packet.sequence_number);
                }
                if mesh_packets_count != 0 {
                    *downloads = UpdateState::IdleSince(Default::default());
                } else {
                    *downloads = UpdateState::IdleSince(Local::now());
                }
                ctx.request_repaint_after(std::time::Duration::from_secs_f32(
                    self.persistent.update_interval_secs.as_seconds_f32() * 1.5,
                ));
            }
            UpdateState::IdleSince(start_time) => {
                // TODO: use settings' option for standalone and relative for web
                const API_ADDRESS: &str = "http://a.styxheim.ru:4881/api/softnode/sync";
                let elapsed = Local::now().signed_duration_since(start_time);

                if elapsed >= self.persistent.update_interval_secs {
                    let ctx = ctx.clone();
                    let request = if let Some(sync_point) = self.persistent.last_sync_point {
                        ehttp::Request::get(format!("{}?start={}", API_ADDRESS, sync_point))
                    } else {
                        ehttp::Request::get(API_ADDRESS)
                    };

                    *downloads = UpdateState::InProgress;
                    let downloads = self.downloads.clone();
                    ehttp::fetch(request, move |result| {
                        log::info!("Fetching data...");
                        match result {
                            Ok(data) => {
                                let mesh_packets = data.json::<Vec<StoredMeshPacket>>().unwrap();
                                *downloads.lock() = UpdateState::Downloaded(mesh_packets);
                            }
                            Err(err) => {
                                log::error!("Error fetching data: {:?}", err);
                                *downloads.lock() = UpdateState::IdleSince(Local::now());
                                // TODO: Handle the error
                            }
                        }
                        ctx.request_repaint();
                    });
                }
            }
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ListPanel {}

impl ListPanel {
    fn ui(&mut self, ctx: &egui::Context, nodes: Vec<&NodeInfo>, filter_to_telemetry: bool) {
        egui::SidePanel::left("list_panel")
            .resizable(false)
            .default_width(200.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for node_info in nodes {
                        if filter_to_telemetry {
                            if let Some(list) = node_info
                                .telemetry
                                .get(&data::TelemetryVariant::Temperature)
                            {
                                if list.len() <= 1 {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }

                        ui.add_sized(
                            [200.0, 20.0],
                            egui::Label::new(format!(
                                "!{:08x} ({})",
                                node_info.node_id,
                                node_info.short_name.clone().unwrap_or("".to_string())
                            )),
                        );
                    }
                });
            });
    }
}

impl eframe::App for SoftNodeApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.persistent.save(storage);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_data(ctx);
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        matches!(self.persistent.active_panel, Panel::Settings(_)),
                        "⚙",
                    )
                    .clicked()
                {
                    self.persistent.active_panel =
                        Panel::Settings(Settings::new(&self.persistent.keyring));
                }
                if ui
                    .selectable_label(
                        matches!(self.persistent.active_panel, Panel::Journal(_)),
                        "Journal",
                    )
                    .clicked()
                {
                    self.persistent.active_panel = Panel::Journal(Journal {});
                }
                if ui
                    .selectable_label(
                        matches!(self.persistent.active_panel, Panel::Telemetry(_)),
                        "Telemetry",
                    )
                    .clicked()
                {
                    self.persistent.active_panel = Panel::Telemetry(Telemetry {});
                }
            })
        });

        let list_panel = &mut self.persistent.list_panel;
        let nodes_list = self.persistent.nodes.iter().map(|(_, v)| v).collect();
        match self.persistent.active_panel {
            Panel::Journal(ref mut journal) => {
                list_panel.ui(ctx, nodes_list, false);
                egui::CentralPanel::default().show(ctx, |ui| journal.ui(ui));
            }
            Panel::Telemetry(ref mut telemetry) => {
                list_panel.ui(ctx, nodes_list, true);
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                    let mut telemetry_list = Vec::new();
                    for (node_id, node_info) in &self.persistent.nodes {
                        if let Some(list) = node_info
                            .telemetry
                            .get(&data::TelemetryVariant::Temperature)
                        {
                            if list.len() <= 1 {
                                continue;
                            }
                            if let Some(first) = list.first() {
                                start_datetime = first.timestamp.min(start_datetime);
                                let title = node_info
                                    .name
                                    .clone()
                                    .map(|v| format!("{} {}", node_id, v))
                                    .unwrap_or(node_id.to_string());
                                telemetry_list.push((title, list));
                            }
                        }
                    }

                    if telemetry_list.len() != 0 {
                        telemetry.ui(ui, "Temperature", start_datetime, telemetry_list)
                    } else {
                        ui.label("No telemetry data available");
                    }
                });
            }
            Panel::Settings(ref mut settings) => {
                if settings.ui(ctx, &mut self.persistent.keyring) {
                    self.persistent.last_sync_point = None;
                    self.persistent.nodes.clear();
                    self.persistent.active_panel = Panel::Journal(Journal {});
                    ctx.request_repaint();
                }
            }
        }
    }
}

impl Journal {
    fn ui(&mut self, _ui: &mut egui::Ui) {
        // todo!()
    }
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct Settings {
    keyring_edit: String,
    encoder_error: Option<String>,
}

impl Settings {
    fn new(keyring: &Keyring) -> Self {
        Self {
            encoder_error: None,
            keyring_edit: serde_yaml_ng::to_string(keyring).unwrap(),
        }
    }

    fn ui(&mut self, ctx: &egui::Context, keyring: &mut Keyring) -> bool {
        let mut need_update = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Theme");
                egui::widgets::global_theme_preference_buttons(ui);

                let theme =
                    egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());

                ui.add_space(1.0);
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

                ui.add_space(1.0);
            });
        });

        need_update
    }
}
