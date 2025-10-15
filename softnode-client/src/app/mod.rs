pub mod byte_node_id;
pub mod data;
mod journal;
mod map;
pub mod settings;
mod telemetry;
use std::{collections::HashMap, f32, ops::ControlFlow, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use data::{JournalData, NodeInfo, NodeTelemetry, StoredMeshPacket, TelemetryVariant};
use egui::{Color32, RichText, mutex::Mutex};
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

pub enum DownloadState {
    Idle,
    WaitHeader,
    // Unknown full size
    Download,
    // Known full size
    DownloadWithSize(f32, usize),
}

impl std::fmt::Display for DownloadState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadState::Idle => write!(f, "Idle"),
            DownloadState::WaitHeader => write!(f, "Waiting"),
            DownloadState::Download => write!(f, "Downloading"),
            DownloadState::DownloadWithSize(done_percent, _) => {
                write!(f, "Downloading {:.2}%", done_percent)
            }
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
    update_interval_secs: Duration,
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
    // Persistent data
    persistent: PersistentData,
    bootstrap_done: bool,
    download_state: Arc<Mutex<DownloadState>>,
    download_promise: poll_promise::Promise<DownloadPromiseResult>,
}

impl Default for PersistentData {
    fn default() -> Self {
        Self {
            active_panel: Panel::Journal(Journal::new()),
            list_panel: Default::default(),
            map: Default::default(),
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

        let download_state: Arc<Mutex<DownloadState>> = Default::default();
        Self {
            journal: Default::default(),
            nodes: Default::default(),
            last_sync_point: Default::default(),
            map_context: MapContext::new(cc.egui_ctx.clone()),
            download_state: download_state.clone(),
            keyring,
            persistent: PersistentData::new(cc),
            bootstrap_done: false,
            download_promise: go_download_promise(
                std::time::Duration::default(),
                Default::default(),
                download_state,
                cc.egui_ctx.clone(),
            ),
        }
    }
}

type DownloadPromiseResult = Vec<StoredMeshPacket>;

fn go_download_promise(
    delay: std::time::Duration,
    last_sync_point: Option<u64>,
    state: Arc<Mutex<DownloadState>>,
    egui_ctx: egui::Context,
) -> poll_promise::Promise<DownloadPromiseResult> {
    poll_promise::Promise::spawn_thread("background http sync", move || {
        if !delay.is_zero() {
            log::info!("sync delay for {:?}", delay);
            std::thread::sleep(delay);
        }

        let api_url = format!("{}{}", env!("SOFTNODE_API_URL_BASE"), "/sync");
        let request = if let Some(sync_point) = last_sync_point {
            ehttp::Request::get(format!("{}?start={}", api_url, sync_point))
        } else {
            ehttp::Request::get(&api_url)
        };

        *state.lock() = DownloadState::WaitHeader;
        let inner_state = state.clone();
        let body = Arc::new(Mutex::new(Vec::new()));
        let inner_body = body.clone();
        log::info!("Fetching data: {} ...", api_url);
        ehttp::streaming::fetch_streaming_blocking(
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
                                    *inner_state.lock() =
                                        DownloadState::DownloadWithSize(0.0, length);
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
                            *inner_state.lock() = DownloadState::Idle;
                            ControlFlow::Break(())
                        }
                    },
                    ehttp::streaming::Part::Chunk(chunk) => {
                        let mut body = inner_body.lock();
                        let mut state = inner_state.lock();
                        body.extend_from_slice(&chunk);

                        let next_state = match *state {
                            DownloadState::Idle
                            | DownloadState::WaitHeader
                            | DownloadState::Download => DownloadState::Download,
                            DownloadState::DownloadWithSize(_, full_size) => {
                                DownloadState::DownloadWithSize(
                                    body.len() as f32 / full_size as f32 * 100.0,
                                    full_size,
                                )
                            }
                        };
                        *state = next_state;
                        ControlFlow::Continue(())
                    }
                }
            }),
        );

        *state.lock() = DownloadState::Idle;
        let body = body.lock();
        if body.len() != 0 {
            match serde_json::from_slice::<Vec<StoredMeshPacket>>(body.as_slice()) {
                Ok(body) => {
                    log::info!("Fetched {} packets", body.len());
                    egui_ctx.request_repaint();
                    body
                }
                Err(e) => {
                    log::error!("Fetching json error: {}", e);
                    egui_ctx.request_repaint();
                    Vec::new()
                }
            }
        } else {
            egui_ctx.request_repaint();
            Vec::new()
        }
    })
}

impl SoftNodeApp {
    fn update_data(&mut self, ctx: &egui::Context) -> bool {
        if let Some(promise_result) = self.download_promise.ready_mut() {
            let next_delay = if promise_result.len() == 0 {
                std::time::Duration::from_secs(5)
            } else {
                Default::default()
            };
            if let Some(last_record) = promise_result.last() {
                self.last_sync_point = Some(last_record.sequence_number);
            }

            for stored_mesh_packet in promise_result.drain(..) {
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

            self.download_promise = go_download_promise(
                next_delay,
                self.last_sync_point,
                self.download_state.clone(),
                ctx.clone(),
            );
            if !next_delay.is_zero() {
                self.bootstrap_done = true;
            }
        }

        !self.bootstrap_done
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
        node_selected: Option<NodeId>,
        filter_by: ListPanelFilter,
    ) -> Option<Panel> {
        let mut next_page = None;

        egui::Frame::new().inner_margin(3.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(f32::INFINITY)
                    .hint_text("Search note by id or name")
                    .show(ui)
                    .response
                    .request_focus();
                ui.input(|i| {
                    if i.key_pressed(egui::Key::Escape) {
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
                                ui.heading(
                                    RichText::new("➧").color(Color32::from_rgb(0, 153, 255)),
                                );
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
                                next_page =
                                    Some(Panel::Rssi(node_info.node_id, Default::default()));
                            }
                        }
                        if !node_info.gateway_for.is_empty() {
                            if ui
                                .button(format!("Gateway {}", node_info.gateway_for.len()))
                                .clicked()
                            {
                                next_page = Some(Panel::Gateways(
                                    Some(node_info.node_id),
                                    Default::default(),
                                ))
                            }
                        }
                    });
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
                        telemetry.ui(ui, start_datetime, telemetry_list, None, true, None)
                    } else {
                        ui.label("Select a telemetry on left panel to display");
                    }
                });
            }
            Panel::Settings(settings) => {
                if settings.ui(ctx, &mut self.keyring) {
                    self.last_sync_point = None;
                    std::mem::replace(
                        &mut self.download_promise,
                        go_download_promise(
                            Default::default(),
                            self.last_sync_point,
                            self.download_state.clone(),
                            ctx.clone(),
                        ),
                    )
                    .abort();
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
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.persistent
                        .map
                        .ui(ui, &mut self.map_context, &self.nodes)
                });
            }
        };
    }
}

const PERSISTENT_KEYRING_KEY: &str = "keyring";

impl eframe::App for SoftNodeApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PERSISTENT_KEYRING_KEY, &self.keyring);
        self.persistent.save(storage);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        {
            let state = self.download_state.lock();
            if !matches!(*state, DownloadState::Idle) {
                egui::Area::new("Download State".into())
                    .interactable(false)
                    .anchor(egui::Align2::LEFT_BOTTOM, (6.0, -6.0))
                    .show(ctx, |ui| {
                        ui.add(
                            egui::Label::new(format!("{}", *state))
                                .wrap_mode(egui::TextWrapMode::Extend),
                        )
                    });
            }
        }

        if self.update_data(ctx) {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Updating...");
            });
            return;
        }
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::ScrollArea::horizontal()
                .auto_shrink(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                matches!(self.persistent.active_panel, Panel::Settings(_)),
                                "⚙",
                            )
                            .clicked()
                        {
                            self.persistent.active_panel =
                                Panel::Settings(Settings::new(&self.keyring));
                        }
                        if ui
                            .selectable_label(
                                matches!(self.persistent.active_panel, Panel::Journal(_)),
                                "Journal",
                            )
                            .clicked()
                        {
                            self.persistent.active_panel = Panel::Journal(Journal::new());
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

                        if ui
                            .selectable_label(
                                matches!(self.persistent.active_panel, Panel::Gateways(_, _)),
                                "Gateways",
                            )
                            .clicked()
                        {
                            self.persistent.active_panel = Panel::Gateways(None, Telemetry {});
                        }

                        if ui
                            .selectable_label(
                                matches!(self.persistent.active_panel, Panel::Map),
                                "Map",
                            )
                            .clicked()
                        {
                            self.persistent.active_panel = Panel::Map;
                        }
                    })
                });
        });

        let list_panel = &mut self.persistent.list_panel;
        if ctx.content_rect().width() > 400.0 {
            if list_panel.show {
                let nodes_list = self.nodes.iter().map(|(_, v)| v).collect();
                egui::SidePanel::left("Roster").show(ctx, |ui| {
                    if let Some(next_panel) =
                        list_panel.ui(ui, nodes_list, None, ListPanelFilter::None)
                    {
                        self.persistent.active_panel = next_panel;
                    }
                });
            }
            self.update_central_panel(ctx);
        } else {
            if list_panel.show {
                let nodes_list = self.nodes.iter().map(|(_, v)| v).collect();
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(next_panel) =
                        list_panel.ui(ui, nodes_list, None, ListPanelFilter::None)
                    {
                        self.persistent.active_panel = next_panel;
                    }
                });
            } else {
                self.update_central_panel(ctx);
            }
        }
    }
}
