pub mod byte_node_id;
pub mod data;
pub mod settings;
mod telemetry;
use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Duration, Local, Utc};
use data::{NodeInfo, NodeTelemetry, StoredMeshPacket, TelemetryVariant};
use egui::mutex::Mutex;
use meshtastic_connect::keyring::{Keyring, node_id::NodeId};
use settings::Settings;
use telemetry::Telemetry;

#[derive(serde::Deserialize, serde::Serialize)]
enum Panel {
    Journal(Journal),
    Telemetry(Telemetry),
    Settings(Settings),
    Gateways(NodeId, Telemetry),
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
    keyring: Keyring,

    active_panel: Panel,
    list_panel: ListPanel,

    update_interval_secs: Duration,
}

#[derive(Default)]
pub struct SoftNodeApp {
    nodes: HashMap<NodeId, NodeInfo>,
    last_sync_point: Option<u64>,

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
            keyring,
            active_panel: Panel::Journal(Journal::default()),
            list_panel: Default::default(),
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

                    let entry = self.nodes.entry(node_id).or_insert_with(|| data::NodeInfo {
                        node_id,
                        ..Default::default()
                    });

                    entry.update(&stored_mesh_packet);
                    self.last_sync_point = Some(stored_mesh_packet.sequence_number);
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
                    let request = if let Some(sync_point) = self.last_sync_point {
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

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct ListPanel {
    telemetry_enabled_for: HashMap<TelemetryVariant, Vec<NodeId>>,
}

impl ListPanel {
    fn ui(
        &mut self,
        ctx: &egui::Context,
        nodes: Vec<&NodeInfo>,
        is_telemetry_page: bool,
    ) -> Option<Panel> {
        let mut next_page = None;
        egui::SidePanel::left("list_panel")
            .resizable(true)
            .default_width(300.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical(|ui| {
                        for node_info in nodes {
                            let telemetry_variants = is_telemetry_page.then(|| {
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
                                if node_id_str.ends_with(extended.short_name.as_str()) {
                                    ui.heading(node_id_str);
                                } else {
                                    ui.heading(format!("{} {}", node_id_str, extended.short_name));
                                }
                                if extended.long_name.len() > 0 {
                                    ui.label(extended.long_name.clone());
                                }
                            } else {
                                ui.heading(node_info.node_id.to_string());
                            }
                            ui.add_space(5.0);
                            if ui.button("RSSI").clicked() {
                                next_page =
                                    Some(Panel::Gateways(node_info.node_id, Default::default()));
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
                });
            });
        next_page
    }
}

impl SoftNodeApp {
    fn update_panel(&mut self, ctx: &egui::Context) -> Option<Panel> {
        let list_panel = &mut self.persistent.list_panel;
        let nodes_list = self.nodes.iter().map(|(_, v)| v).collect();

        match &mut self.persistent.active_panel {
            Panel::Journal(journal) => {
                if let Some(next_panel) = list_panel.ui(ctx, nodes_list, false) {
                    return Some(next_panel);
                }
                egui::CentralPanel::default().show(ctx, |ui| journal.ui(ui));
            }
            Panel::Telemetry(telemetry) => {
                if let Some(next_panel) = list_panel.ui(ctx, nodes_list, true) {
                    return Some(next_panel);
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                    let mut telemetry_list = Vec::new();

                    for (telemetry_variant, enabled_for) in
                        self.persistent.list_panel.telemetry_enabled_for.iter()
                    {
                        for node_id in enabled_for {
                            if let Some(node_info) = self.nodes.get(node_id) {
                                if let Some(list) = node_info.telemetry.get(telemetry_variant) {
                                    if list.len() <= 1 {
                                        continue;
                                    }
                                    if let Some(first) = list.first() {
                                        start_datetime = first.timestamp.min(start_datetime);
                                        let title = if let Some(extended) =
                                            node_info.extended_info_history.last()
                                        {
                                            format!(
                                                "{}: {} {}",
                                                telemetry_variant, node_id, extended.short_name
                                            )
                                        } else {
                                            format!("{}: {}", telemetry_variant, node_id)
                                        };
                                        telemetry_list.push((title, list));
                                    }
                                }
                            }
                        }
                    }

                    if telemetry_list.len() != 0 {
                        telemetry.ui(ui, start_datetime, telemetry_list, false, None)
                    } else {
                        ui.label("Select a telemetry on left panel to display");
                    }
                });
            }
            Panel::Settings(settings) => {
                if settings.ui(ctx, &mut self.persistent.keyring) {
                    self.last_sync_point = None;
                    self.nodes.clear();
                    self.persistent.active_panel = Panel::Journal(Journal {});
                    ctx.request_repaint();
                }
            }
            Panel::Gateways(node_id, telemetry) => {
                let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                let mut telemetry_list = Vec::new();

                if let Some(next_panel) = list_panel.ui(ctx, nodes_list, false) {
                    return Some(next_panel);
                }

                if let Some(node_info) = &self.nodes.get(&node_id) {
                    // let mut snr = Vec::new();
                    let mut rssi = Vec::new();
                    // let mut snr_per_gw: HashMap<NodeId, Vec<NodeTelemetry>> = Default::default();
                    let mut rssi_per_gw: HashMap<NodeId, Vec<NodeTelemetry>> = Default::default();

                    for packet_info in &node_info.packet_statistics {
                        start_datetime = packet_info.timestamp.min(start_datetime);
                        if let Some(rx_info) = &packet_info.rx_info {
                            // let snr_telemetry = NodeTelemetry {
                            //     timestamp: packet_info.timestamp,
                            //     value: rx_info.rx_snr as f64,
                            // };
                            let rssi_telemetry = NodeTelemetry {
                                timestamp: packet_info.timestamp,
                                value: rx_info.rx_snr as f64,
                            };

                            if let Some(gateway) = packet_info.gateway {
                                // snr_per_gw
                                //     .entry(gateway)
                                //     .or_insert(Default::default())
                                //     .push(snr_telemetry);

                                rssi_per_gw
                                    .entry(gateway)
                                    .or_insert(Default::default())
                                    .push(rssi_telemetry);
                            } else {
                                // snr.push(snr_telemetry);
                                rssi.push(rssi_telemetry)
                            }
                        }
                    }

                    // let mut snr_per_gw_sorted: Vec<_> = snr_per_gw.iter().collect();
                    // snr_per_gw_sorted.sort_by_key(|(k, _)| **k);

                    let mut rssi_per_gw_sorted: Vec<_> = rssi_per_gw.iter().collect();
                    rssi_per_gw_sorted.sort_by_key(|(k, _)| **k);

                    // for (gateway_id, list) in &snr_per_gw_sorted {
                    //     telemetry_list.push((format!("SNR {}", gateway_id), *list));
                    // }

                    for (gateway_id, list) in &rssi_per_gw_sorted {
                        let title = if let Some(gateway_extended_info) = self
                            .nodes
                            .get(gateway_id)
                            .map(|v| v.extended_info_history.last())
                            .flatten()
                        {
                            format!("RSSI {} {}", gateway_id, gateway_extended_info.short_name)
                        } else {
                            format!("RSSI {}", gateway_id)
                        };
                        telemetry_list.push((title, *list));
                    }

                    // telemetry_list.push((format!("SNR <unknown gateway>"), &snr));
                    telemetry_list.push((format!("RSSI <unknown gateway>"), &rssi));

                    egui::CentralPanel::default().show(ctx, |ui| {
                        let title =
                            if let Some(extended_info) = node_info.extended_info_history.last() {
                                format!(
                                    "{} {}\nRSSI per gateways",
                                    node_id, extended_info.short_name
                                )
                            } else {
                                format!("{}\nRSSI per gateways", node_id)
                            };
                        if telemetry_list.len() != 0 {
                            telemetry.ui(ui, start_datetime, telemetry_list, true, Some(title))
                        } else {
                            ui.label("No data");
                        }
                    });
                }
            }
        };
        None
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

        if let Some(next_panel) = self.update_panel(ctx) {
            self.persistent.active_panel = next_panel;
        }
    }
}

impl Journal {
    fn ui(&mut self, _ui: &mut egui::Ui) {
        // todo!()
    }
}
