pub mod byte_node_id;
pub mod data;
mod journal;
mod map;
pub mod settings;
mod telemetry;
use std::{collections::HashMap, f32, ops::ControlFlow, sync::Arc};
pub mod fix_gnss;

use chrono::{DateTime, Utc};
use data::{JournalData, NodeInfo, NodeTelemetry, StoredMeshPacket, TelemetryVariant};
use egui::{Color32, RichText, mutex::Mutex};
use fix_gnss::FixGnssLibrary;
use journal::Journal;
use map::MapPanel;
use meshtastic_connect::keyring::{Keyring, node_id::NodeId};
use settings::Settings;
use telemetry::Telemetry;

use crate::app::map::MapContext;

#[derive(serde::Deserialize, serde::Serialize)]
enum Panel {
    Journal(Journal),
    Telemetry(Telemetry),
    Settings(Settings),
    Rssi(NodeId, Telemetry),
    Gateways(Option<NodeId>, Telemetry),
    Map,
}

#[derive(Clone, Copy)]
pub enum DownloadState {
    Idle,
    WaitHeader,
    // Unknown full size
    Download,
    // Known full size
    DownloadWithSize(f32, usize),
    Parse,
    // Hold to next download action
    Delay,
}

impl std::fmt::Display for DownloadState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::WaitHeader => write!(f, "Waiting headers"),
            Self::Download => write!(f, "Downloading"),
            Self::DownloadWithSize(done_percent, _) => {
                write!(f, "Downloading {:.2}%", done_percent)
            }
            Self::Parse => write!(f, "Parsing"),
            Self::Delay => write!(f, "Resting"),
        }
    }
}

impl Default for DownloadState {
    fn default() -> Self {
        DownloadState::Idle
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct PersistentData {
    active_panel: Panel,

    list_panel: ListPanel,
    map: MapPanel,
    update_interval_secs: std::time::Duration,
}

pub struct SoftNodeApp {
    journal: Vec<JournalData>,
    nodes: HashMap<NodeId, NodeInfo>,
    last_sync_point: Option<u64>,

    map_context: MapContext,

    // Keyring data. Similar to persistent,
    // but saved separately, to avoid keyring drop
    // when persistent structure is updated
    keyring: Keyring,
    // GNSS fixes. Persistent as keyring data
    fix_gnss: FixGnssLibrary,
    // Persistent data
    persistent: PersistentData,
    bootstrap_done: bool,
    download_state: Arc<Mutex<DownloadState>>,
    download_data: Arc<Mutex<Vec<StoredMeshPacket>>>,
}

impl Default for PersistentData {
    fn default() -> Self {
        Self {
            active_panel: Panel::Journal(Journal::new()),
            list_panel: Default::default(),
            map: Default::default(),
            update_interval_secs: std::time::Duration::from_secs(5),
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
            match eframe::get_value(storage, eframe::APP_KEY) {
                Some(value) => value,
                None => Default::default(),
            }
        } else {
            Default::default()
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

fn default_keyring() -> Keyring {
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

    keyring
}

impl SoftNodeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let keyring = cc
            .storage
            .map(|storage| eframe::get_value(storage, PERSISTENT_KEYRING_KEY))
            .flatten()
            .unwrap_or_else(|| default_keyring());

        let fix_gnss = cc
            .storage
            .map(|storage| eframe::get_value(storage, PERSISTENT_FIX_GNSS_KEY))
            .flatten()
            .unwrap_or_else(|| Default::default());

        let persistent = PersistentData::new(cc);
        let download_state: Arc<Mutex<DownloadState>> = Default::default();
        let download_data: Arc<Mutex<Vec<StoredMeshPacket>>> = Default::default();
        go_download(
            persistent.update_interval_secs,
            Default::default(),
            download_state.clone(),
            download_data.clone(),
            cc.egui_ctx.clone(),
        );
        Self {
            journal: Default::default(),
            nodes: Default::default(),
            last_sync_point: Default::default(),
            map_context: MapContext::new(cc.egui_ctx.clone()),
            download_state,
            download_data,
            keyring,
            fix_gnss,
            persistent,
            bootstrap_done: false,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_after(delay: std::time::Duration, f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        f();
    });
}

#[cfg(target_arch = "wasm32")]
fn run_after(delay: std::time::Duration, f: impl FnOnce() + Send + 'static) {
    wasm_bindgen_futures::spawn_local(async move {
        let _ = wasmtimer::tokio::sleep(delay).await;
        f();
    });
}

fn go_download(
    delay_if_no_data: std::time::Duration,
    last_sync_point: Option<u64>,
    state: Arc<Mutex<DownloadState>>,
    data: Arc<Mutex<Vec<StoredMeshPacket>>>,
    egui_ctx: egui::Context,
) {
    *state.lock() = DownloadState::WaitHeader;
    let api_url = format!("{}{}", env!("SOFTNODE_API_URL_BASE"), "/sync");
    let request = if let Some(sync_point) = last_sync_point {
        ehttp::Request::get(format!("{}?start={}", api_url, sync_point))
    } else {
        ehttp::Request::get(&api_url)
    };

    let inner_state = state.clone();
    let body = Arc::new(Mutex::new(Vec::new()));
    let inner_body = body.clone();
    log::info!("Fetching data: {} ...", api_url);
    ehttp::streaming::fetch(
        request,
        Box::new(move |part| {
            let part = match part {
                Err(err) => {
                    log::error!("Fetching error: {}", err);
                    return ControlFlow::Break(());
                }
                Ok(part) => part,
            };

            match part {
                ehttp::streaming::Part::Response(response) => match response.status {
                    200 => {
                        match response
                            .headers
                            .get("Content-Length")
                            .ok_or_else(|| "No Content-Length".to_string())
                            .map(|v| {
                                v.parse::<usize>()
                                    .map_err(|e| format!("Content-Length parse problem: {e}"))
                            })
                            .flatten()
                        {
                            Ok(length) => {
                                *inner_state.lock() = DownloadState::DownloadWithSize(0.0, length);
                                log::info!("Fetching length: len={}", length);
                            }
                            Err(e) => {
                                *inner_state.lock() = DownloadState::Download;
                                log::error!(
                                    "Fetching length error: {}, continue download without length",
                                    e
                                )
                            }
                        }
                        ControlFlow::Continue(())
                    }
                    _ => {
                        log::error!(
                            "Fetching error: status code={}: {}",
                            response.status,
                            response.status_text
                        );
                        ControlFlow::Break(())
                    }
                },
                ehttp::streaming::Part::Chunk(chunk) => {
                    let mut body = inner_body.lock();
                    if !chunk.is_empty() {
                        body.extend_from_slice(&chunk);

                        let next_state = match *inner_state.lock() {
                            DownloadState::Idle
                            | DownloadState::WaitHeader
                            | DownloadState::Download => DownloadState::Download,
                            DownloadState::DownloadWithSize(_, full_size) => {
                                DownloadState::DownloadWithSize(
                                    body.len() as f32 / full_size as f32 * 100.0,
                                    full_size,
                                )
                            }
                            DownloadState::Delay | DownloadState::Parse => unreachable!(),
                        };
                        *inner_state.lock() = next_state;
                        ControlFlow::Continue(())
                    } else {
                        if body.len() != 0 {
                            *inner_state.lock() = DownloadState::Parse;
                            match serde_json::from_slice::<Vec<StoredMeshPacket>>(body.as_slice()) {
                                Ok(mut new_data) => {
                                    log::info!("Fetched {} packets", new_data.len());
                                    if new_data.is_empty() {
                                        *state.lock() = DownloadState::Delay;
                                        let state = state.clone();
                                        let egui_ctx = egui_ctx.clone();
                                        run_after(delay_if_no_data, move || {
                                            *state.lock() = DownloadState::Idle;
                                            egui_ctx.request_repaint();
                                        });
                                    } else {
                                        data.lock().append(&mut new_data);
                                        *state.lock() = DownloadState::Idle;
                                        egui_ctx.request_repaint();
                                    }
                                }
                                Err(e) => {
                                    log::error!("Fetching json error: {}", e);
                                    *inner_state.lock() = DownloadState::Delay;
                                    let state = state.clone();
                                    let egui_ctx = egui_ctx.clone();
                                    run_after(delay_if_no_data, move || {
                                        *state.lock() = DownloadState::Idle;
                                        egui_ctx.request_repaint();
                                    });
                                }
                            }
                        } else {
                            *inner_state.lock() = DownloadState::Delay;
                            let state = state.clone();
                            let egui_ctx = egui_ctx.clone();
                            run_after(delay_if_no_data, move || {
                                *state.lock() = DownloadState::Idle;
                                egui_ctx.request_repaint();
                            });
                        }
                        ControlFlow::Break(())
                    }
                }
            }
        }),
    );
}

impl SoftNodeApp {
    fn update_data(&mut self, ctx: &egui::Context) -> bool {
        let download_state = *self.download_state.lock();
        if matches!(download_state, DownloadState::Delay)
            || matches!(download_state, DownloadState::Idle)
        {
            let mut data: Vec<StoredMeshPacket> = self.download_data.lock().drain(..).collect();
            if let Some(last_record) = data.last() {
                self.last_sync_point = Some(last_record.sequence_number);
            }

            for stored_mesh_packet in data.drain(..) {
                let node_id = stored_mesh_packet.header.from;
                let stored_mesh_packet = stored_mesh_packet.decrypt(&self.keyring);

                if let Some(gateway_id) = stored_mesh_packet.gateway {
                    let gateway_entry =
                        self.nodes
                            .entry(gateway_id)
                            .or_insert_with(|| data::NodeInfo {
                                node_id: gateway_id,
                                ..Default::default()
                            });

                    gateway_entry.update_as_gateway(&stored_mesh_packet);
                }

                let entry = self.nodes.entry(node_id).or_insert_with(|| data::NodeInfo {
                    node_id,
                    ..Default::default()
                });

                entry.update(&stored_mesh_packet);
                self.journal.push(stored_mesh_packet.clone().into());
            }

            if matches!(download_state, DownloadState::Idle) {
                go_download(
                    self.persistent.update_interval_secs,
                    self.last_sync_point,
                    self.download_state.clone(),
                    self.download_data.clone(),
                    ctx.clone(),
                );
            }
        }

        false
    }
}

enum ListPanelFilter {
    None,
    Telemetry,
    Gateway,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct ListPanel {
    show: bool,
    telemetry_enabled_for: HashMap<TelemetryVariant, Vec<NodeId>>,
    filter: String,
}

impl ListPanel {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        mut nodes: Vec<&NodeInfo>,
        mut additional: impl FnMut(&mut egui::Ui, &NodeInfo),
        node_selected: Option<NodeId>,
        filter_by: ListPanelFilter,
    ) -> Option<Panel> {
        let mut next_page = None;

        ui.horizontal(|ui| {
            egui::TextEdit::singleline(&mut self.filter)
                .desired_width(f32::INFINITY)
                .hint_text("Search node by id or name")
                .show(ui)
                .response
                .request_focus();
            ui.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    self.show = false;
                    self.filter.clear();
                }
            })
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            nodes.sort_by_key(|node_info| node_info.node_id);
            for node_info in nodes {
                if !self.filter.is_empty() {
                    let filter = self.filter.to_uppercase();
                    let mut skip = true;
                    if node_info
                        .node_id
                        .to_string()
                        .to_uppercase()
                        .contains(filter.as_str())
                    {
                        skip = false;
                    }
                    if let Some(extended_info) = node_info.extended_info_history.last() {
                        if extended_info
                            .short_name
                            .to_uppercase()
                            .contains(filter.as_str())
                        {
                            skip = false;
                        }
                        if extended_info
                            .long_name
                            .to_uppercase()
                            .contains(filter.as_str())
                        {
                            skip = false;
                        }
                    }
                    if skip {
                        continue;
                    }
                }

                let mut show_and_filter_by_telemetry = false;
                match filter_by {
                    ListPanelFilter::None => {}
                    ListPanelFilter::Telemetry => {
                        show_and_filter_by_telemetry = true;
                    }
                    ListPanelFilter::Gateway => {
                        if node_info.gateway_for.is_empty() {
                            continue;
                        }
                    }
                }

                let telemetry_variants = show_and_filter_by_telemetry.then(|| {
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
                    ui.horizontal(|ui| {
                        if node_selected
                            .map(|v| v == node_info.node_id)
                            .unwrap_or(false)
                        {
                            ui.heading(RichText::new("➧").color(Color32::from_rgb(0, 153, 255)));
                        }
                        if node_id_str.ends_with(extended.short_name.as_str()) {
                            ui.heading(node_id_str);
                        } else {
                            ui.heading(format!("{} {}", node_id_str, extended.short_name));
                        }
                    });
                    if extended.long_name.len() > 0 {
                        ui.label(extended.long_name.clone());
                    }
                } else {
                    ui.heading(node_info.node_id.to_string());
                }
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    if !node_info.packet_statistics.is_empty() {
                        if ui.button("RSSI").clicked() {
                            self.show = false;
                            next_page = Some(Panel::Rssi(node_info.node_id, Default::default()));
                        }
                    }

                    if !node_info.gateway_for.is_empty() {
                        if ui
                            .button(format!("Gateway {}", node_info.gateway_for.len()))
                            .clicked()
                        {
                            self.show = false;
                            next_page =
                                Some(Panel::Gateways(Some(node_info.node_id), Default::default()))
                        }
                    }
                });
                additional(ui, node_info);
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

        next_page
    }
}

impl SoftNodeApp {
    fn update_central_panel(&mut self, ctx: &egui::Context) {
        match &mut self.persistent.active_panel {
            Panel::Journal(journal) => {
                egui::CentralPanel::default().show(ctx, |ui| journal.ui(ui, &self.journal));
            }
            Panel::Telemetry(telemetry) => {
                let frame = egui::Frame::default().inner_margin(0);
                egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
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
                        telemetry.ui(ui, start_datetime, telemetry_list, None, true, None)
                    } else {
                        self.persistent.list_panel.show = true;
                        ui.label("Select a telemetry on left panel to display");
                    }
                });
            }
            Panel::Settings(settings) => {
                if settings.ui(ctx, &mut self.keyring) {
                    self.last_sync_point = None;
                    self.download_state = Default::default();
                    self.download_data = Default::default();
                    go_download(
                        self.persistent.update_interval_secs,
                        self.last_sync_point,
                        self.download_state.clone(),
                        self.download_data.clone(),
                        ctx.clone(),
                    );
                    self.bootstrap_done = false;
                    self.nodes.clear();
                    self.journal.clear();
                    self.persistent.active_panel = Panel::Journal(Journal::new());
                    ctx.request_repaint();
                }
            }
            Panel::Rssi(node_id, telemetry) => {
                let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                let mut telemetry_list = Vec::new();

                if let Some(node_info) = &self.nodes.get(&node_id) {
                    // let mut snr = Vec::new();
                    let mut rssi = Vec::new();
                    let mut max_rssi = f32::MIN;
                    // let mut snr_per_gw: HashMap<NodeId, Vec<NodeTelemetry>> = Default::default();
                    let mut rssi_per_gw: HashMap<NodeId, Vec<NodeTelemetry>> = Default::default();

                    for packet_info in &node_info.packet_statistics {
                        start_datetime = packet_info.timestamp.min(start_datetime);
                        if let Some(rx_info) = &packet_info.rx_info {
                            // let snr_telemetry = NodeTelemetry {
                            //     timestamp: packet_info.timestamp,
                            //     value: rx_info.rx_snr as f64,
                            // };
                            max_rssi = max_rssi.max(rx_info.rx_rssi as f32);
                            let rssi_telemetry = NodeTelemetry {
                                timestamp: packet_info.timestamp,
                                value: rx_info.rx_rssi as f64,
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
                            telemetry.ui(
                                ui,
                                start_datetime,
                                telemetry_list,
                                Some(title),
                                false,
                                Some(max_rssi),
                            )
                        } else {
                            ui.label("No data");
                        }
                    });
                }
            }
            Panel::Gateways(gateway_id, telemetry) => {
                if let Some(gateway_info) = gateway_id.map(|v| self.nodes.get(&v)).flatten() {
                    let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                    let mut max_rssi = f32::MIN;
                    let rssi = gateway_info
                        .gateway_for
                        .iter()
                        .map(|(k, v)| {
                            (
                                k,
                                v.iter()
                                    .map(|v| {
                                        start_datetime = v.timestamp.min(start_datetime);
                                        NodeTelemetry {
                                            timestamp: v.timestamp,
                                            value: v
                                                .rx_info
                                                .as_ref()
                                                .map(|rx_info| {
                                                    max_rssi = max_rssi.max(rx_info.rx_rssi as f32);
                                                    rx_info.rx_rssi as f64
                                                })
                                                .unwrap_or(0.0),
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let rssi_with_refs: Vec<_> = rssi
                        .iter()
                        .map(|(node_id, v)| {
                            (
                                if let Some(extended_info) = self
                                    .nodes
                                    .get(node_id)
                                    .map(|node_info| node_info.extended_info_history.last())
                                    .flatten()
                                {
                                    format!("{} {}", node_id, extended_info.short_name)
                                } else {
                                    node_id.to_string()
                                },
                                v,
                            )
                        })
                        .collect();

                    egui::CentralPanel::default().show(ctx, |ui| {
                        if rssi.len() != 0 {
                            telemetry.ui(
                                ui,
                                start_datetime,
                                rssi_with_refs,
                                Some(format!("{} RSSI", gateway_id.unwrap())),
                                false,
                                Some(max_rssi),
                            )
                        } else {
                            ui.label("No data");
                        }
                    });
                }
            }
            Panel::Map => {
                let frame = egui::Frame::default().inner_margin(0);
                egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
                    self.persistent
                        .map
                        .ui(ui, &mut self.map_context, &self.nodes, &self.fix_gnss)
                });
            }
        };
    }
}

const PERSISTENT_KEYRING_KEY: &str = "keyring";
const PERSISTENT_FIX_GNSS_KEY: &str = "fix_gnss";

impl eframe::App for SoftNodeApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PERSISTENT_KEYRING_KEY, &self.keyring);
        eframe::set_value(storage, PERSISTENT_FIX_GNSS_KEY, &self.fix_gnss);
        self.persistent.save(storage);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.update_data(ctx) {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Updating...");
            });
            return;
        }
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::Frame::new().inner_margin(5.0).show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink(true)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(self.persistent.list_panel.show, "🔍")
                                .clicked()
                            {
                                self.persistent.list_panel.show = !self.persistent.list_panel.show;
                            }

                            let menu_text = match self.persistent.active_panel {
                                Panel::Journal(_) => "Journal".into(),
                                Panel::Telemetry(_) => "Telemetry".into(),
                                Panel::Settings(_) => "Settings".into(),
                                Panel::Rssi(node_id, _) => {
                                    format!("Outcome radio {}", node_id)
                                }
                                Panel::Gateways(node_id, _) => {
                                    if node_id.is_some() {
                                        format!("Income radio ({})", node_id.unwrap())
                                    } else {
                                        "Income radio".into()
                                    }
                                }
                                Panel::Map => "Map".into(),
                            };

                            ui.menu_button(menu_text, |ui| {
                                if ui.button("Journal").clicked() {
                                    self.persistent.active_panel = Panel::Journal(Journal::new());
                                    self.persistent.list_panel.show = false;
                                }

                                if ui.button("Telemetry").clicked() {
                                    self.persistent.active_panel = Panel::Telemetry(Telemetry {});
                                    self.persistent.list_panel.show = false;
                                }

                                if ui.button("Map").clicked() {
                                    self.persistent.active_panel = Panel::Map;
                                    self.persistent.list_panel.show = false;
                                }
                            });

                            let state = *self.download_state.lock();
                            if !matches!(state, DownloadState::Delay) {
                                ui.add(
                                    egui::Label::new(format!("{}", state))
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                if ui
                                    .selectable_label(
                                        matches!(self.persistent.active_panel, Panel::Settings(_)),
                                        "⚙",
                                    )
                                    .clicked()
                                {
                                    self.persistent.active_panel =
                                        Panel::Settings(Settings::new(&self.keyring));
                                    self.persistent.list_panel.show = false;
                                }
                            });
                        })
                    });
            });
        });

        let panel_filter = match self.persistent.active_panel {
            Panel::Journal(_) | Panel::Settings(_) | Panel::Rssi(_, _) => ListPanelFilter::None,
            Panel::Telemetry(_) => ListPanelFilter::Telemetry,
            Panel::Gateways(_, _) => ListPanelFilter::Gateway,
            Panel::Map => ListPanelFilter::None,
        };

        let list_panel = &mut self.persistent.list_panel;
        if ctx.content_rect().width() > 400.0 {
            if list_panel.show {
                let nodes_list = self.nodes.iter().map(|(_, v)| v).collect();
                egui::SidePanel::left("Roster").show(ctx, |ui| {
                    if let Some(next_panel) = list_panel.ui(
                        ui,
                        nodes_list,
                        |ui, node_info| {
                            self.persistent.map.panel_ui(
                                ui,
                                node_info,
                                &mut self.map_context,
                                &mut self.fix_gnss,
                            );
                        },
                        None,
                        panel_filter,
                    ) {
                        self.persistent.active_panel = next_panel;
                    }
                });
            }
            self.update_central_panel(ctx);
        } else {
            if list_panel.show {
                let nodes_list = self.nodes.iter().map(|(_, v)| v).collect();
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(next_panel) = list_panel.ui(
                        ui,
                        nodes_list,
                        |ui, node_info| {
                            self.persistent.map.panel_ui(
                                ui,
                                node_info,
                                &mut self.map_context,
                                &mut self.fix_gnss,
                            );
                        },
                        None,
                        panel_filter,
                    ) {
                        self.persistent.active_panel = next_panel;
                    }
                });
            } else {
                self.update_central_panel(ctx);
            }
        }
    }
}
