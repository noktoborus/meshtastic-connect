pub mod byte_node_id;
pub mod data;
mod journal;
mod map;
mod node_filter;
mod radio_telemetry;
pub mod settings;
mod telemetry;
mod telemetry_formatter;
use std::{collections::HashMap, f32, ops::ControlFlow, sync::Arc};
pub mod color_generator;
pub mod fix_gnss;
mod node_dump;
pub mod radio_center;
mod roster;
mod time_format;

use chrono::{DateTime, Utc};
use data::{JournalData, NodeInfo, StoredMeshPacket};
use egui::RichText;
use egui::mutex::Mutex;
use fix_gnss::FixGnssLibrary;
use journal::JournalPanel;
use map::MapPanel;
use meshtastic_connect::keyring::{Keyring, node_id::NodeId};
use node_dump::NodeDump;
use settings::Settings;
use telemetry::Telemetry;

use crate::app::data::{DataVariant, PublicKey, TelemetryValue};
use crate::app::journal::JournalRosterPlugin;
use crate::app::map::{MapContext, MapRosterPlugin};
use crate::app::node_filter::NodeFilter;
use crate::app::radio_center::assume_position;
use crate::app::roster::{Panel, Roster};
use crate::app::telemetry_formatter::TelemetryFormatter;

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
    pub node_filter: NodeFilter,
    pub telemetry_formatter: TelemetryFormatter,
    pub active_panel: Panel,
    pub roster: Roster,
    pub journal: JournalPanel,
    pub map: MapPanel,
    pub node_dump: NodeDump,
    pub update_interval_secs: std::time::Duration,
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
            node_filter: NodeFilter::default(),
            telemetry_formatter: TelemetryFormatter::default(),
            active_panel: Panel::Journal,
            journal: JournalPanel::new(),
            roster: Default::default(),
            map: Default::default(),
            node_dump: NodeDump::new(),
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
            .add_channel(channel_name.into(), "AQ==".try_into().unwrap())
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
                    *state.lock() = DownloadState::Delay;
                    let state = state.clone();
                    let egui_ctx = egui_ctx.clone();
                    run_after(delay_if_no_data, move || {
                        *state.lock() = DownloadState::Idle;
                        egui_ctx.request_repaint();
                    });
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
                        *state.lock() = DownloadState::Idle;
                        egui_ctx.request_repaint();
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

fn is_node_info(stored_mesh_packet: &StoredMeshPacket) -> bool {
    if let Some(DataVariant::Decrypted(_, ref data)) = stored_mesh_packet.data {
        if data.portnum() == meshtastic_connect::meshtastic::PortNum::NodeinfoApp {
            return true;
        }
    }
    false
}

// Find compromised public keys if stored_mesh_packet contains NodeInfo
fn find_compromised_pkeys(node_id: NodeId, nodes: &mut HashMap<NodeId, NodeInfo>) {
    let mut compromised = false;
    if let Some(PublicKey::Key(pkey)) = nodes
        .get(&node_id)
        .map(|v| v.extended_info_history.last().map(|v| v.pkey.clone()))
        .flatten()
    {
        for other_node_info in nodes.values_mut() {
            if other_node_info.node_id == node_id {
                continue;
            }
            if let Some(other_extended) = other_node_info.extended_info_history.last_mut() {
                if other_extended.pkey == PublicKey::Key(pkey)
                    || other_extended.pkey == PublicKey::Compromised(pkey)
                {
                    other_extended.pkey = PublicKey::Compromised(pkey);
                    compromised = true;
                }
            };
        }
    }

    if compromised {
        nodes.entry(node_id).and_modify(|node_info| {
            if let Some(extended) = node_info.extended_info_history.last_mut() {
                if let PublicKey::Key(pkey) = extended.pkey {
                    extended.pkey = PublicKey::Compromised(pkey);
                }
            }
        });
    }
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
            let mut affected_nodes = Vec::new();
            let mut node_info_changed = Vec::new();

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

                entry.update(&stored_mesh_packet, &self.fix_gnss);
                self.journal.push(stored_mesh_packet.clone().into());
                if is_node_info(&stored_mesh_packet) {
                    node_info_changed.push(node_id);
                }
                affected_nodes.push(node_id);
            }

            for node_id in affected_nodes {
                let assumed_position = if let Some(node_info) = self.nodes.get(&node_id) {
                    if node_info.position.is_empty()
                        && (!node_info.gateway_for.is_empty() || !node_info.gatewayed_by.is_empty())
                    {
                        assume_position(node_info, &self.nodes, &self.fix_gnss)
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.nodes
                    .entry(node_id)
                    .and_modify(|v| v.assumed_position = assumed_position);
            }

            for node_id in node_info_changed {
                find_compromised_pkeys(node_id, &mut self.nodes);
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

impl SoftNodeApp {
    fn update_central_panel(&mut self, ctx: &egui::Context) {
        match &mut self.persistent.active_panel {
            Panel::Journal => {
                egui::CentralPanel::default()
                    .show(ctx, |ui| self.persistent.journal.ui(ui, &self.journal));
            }
            Panel::Telemetry(telemetry) => {
                let fill_color = ctx.style().visuals.extreme_bg_color;
                let frame = egui::Frame::default().fill(fill_color).inner_margin(0);
                egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
                    let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                    let mut telemetry_list = Vec::new();

                    for (telemetry_variant, enabled_for) in
                        self.persistent.roster.telemetry_enabled_for.iter()
                    {
                        for node_id in enabled_for {
                            if let Some(node_info) = self.nodes.get(node_id) {
                                if let Some(telemetry_store) =
                                    node_info.telemetry.get(telemetry_variant)
                                {
                                    if let Some(first) = telemetry_store.values.first() {
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
                                        telemetry_list.push((
                                            title,
                                            *telemetry_variant,
                                            telemetry_store,
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    if telemetry_list.len() != 0 {
                        telemetry.ui(
                            ui,
                            start_datetime,
                            telemetry_list,
                            &self.persistent.telemetry_formatter,
                        )
                    } else {
                        self.persistent.roster.show = true;
                        ui.label("Select a telemetry on left panel to display");
                    }
                });
            }
            Panel::Settings(settings) => {
                if settings.ui(
                    ctx,
                    &mut self.keyring,
                    &mut self.persistent.telemetry_formatter,
                ) {
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
                    self.persistent.active_panel = Panel::Journal;
                    ctx.request_repaint();
                }
            }
            Panel::Rssi(node_id, telemetry) => {
                let mut start_datetime = DateTime::<Utc>::MAX_UTC;

                if let Some(node_info) = &self.nodes.get(&node_id) {
                    let mut max_rssi = f32::MIN;
                    let mut rssi_per_gw: HashMap<Option<NodeId>, Vec<TelemetryValue>> =
                        Default::default();

                    for packet_info in &node_info.packet_statistics {
                        start_datetime = packet_info.timestamp.min(start_datetime);
                        if let Some(rx_info) = &packet_info.rx_info {
                            max_rssi = max_rssi.max(rx_info.rx_rssi as f32);
                            let rssi_telemetry = TelemetryValue {
                                timestamp: packet_info.timestamp,
                                value: rx_info.rx_rssi as f64,
                            };

                            rssi_per_gw
                                .entry(packet_info.gateway)
                                .or_insert(Default::default())
                                .push(rssi_telemetry);
                        }
                    }

                    let mut rssi_per_gw_sorted: Vec<_> = rssi_per_gw.iter().collect();
                    rssi_per_gw_sorted.sort_by_key(|(k, _)| **k);

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
                        if rssi_per_gw_sorted.len() != 0 {
                            telemetry.ui(
                                ui,
                                &self.nodes,
                                start_datetime,
                                rssi_per_gw_sorted,
                                None,
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
            Panel::Hops(node_id, telemetry) => {
                let mut start_datetime = DateTime::<Utc>::MAX_UTC;

                if let Some(node_info) = &self.nodes.get(&node_id) {
                    let mut hops_per_gw: HashMap<Option<NodeId>, Vec<TelemetryValue>> =
                        Default::default();
                    let mut dup_packets: HashMap<(Option<NodeId>, u32), Vec<TelemetryValue>> =
                        Default::default();

                    for packet_info in &node_info.packet_statistics {
                        start_datetime = packet_info.timestamp.min(start_datetime);
                        let distance = packet_info
                            .hop_distance
                            .map(|v| v as f64)
                            .unwrap_or(-(packet_info.hop_limit as f64));

                        let hop_telemetry = TelemetryValue {
                            timestamp: packet_info.timestamp,
                            value: distance,
                        };

                        dup_packets
                            .entry((packet_info.gateway, packet_info.packet_id))
                            .or_default()
                            .push(hop_telemetry.clone());

                        hops_per_gw
                            .entry(packet_info.gateway)
                            .or_default()
                            .push(hop_telemetry);
                    }

                    let mut hops_per_gw_sorted: Vec<_> = hops_per_gw.iter().collect();
                    hops_per_gw_sorted.sort_by_key(|(k, _)| **k);

                    egui::CentralPanel::default().show(ctx, |ui| {
                        let title =
                            if let Some(extended_info) = node_info.extended_info_history.last() {
                                format!(
                                    "{} {}\nHops away/hop limit per gateways",
                                    node_id, extended_info.short_name
                                )
                            } else {
                                format!("{}\nHops away/hop limit per gateways", node_id)
                            };
                        if hops_per_gw_sorted.len() != 0 {
                            telemetry.ui(
                                ui,
                                &self.nodes,
                                start_datetime,
                                hops_per_gw_sorted,
                                Some(dup_packets),
                                Some(title),
                                false,
                                None,
                            )
                        } else {
                            ui.label("No data");
                        }
                    });
                }
            }
            Panel::GatewayByRSSI(gateway_id, telemetry) => {
                if let Some(gateway_info) = self.nodes.get(gateway_id) {
                    let mut rssi_per_node: HashMap<Option<NodeId>, Vec<TelemetryValue>> =
                        Default::default();
                    let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                    let mut max_rssi = f32::MIN;

                    for (node_id, gateway_infos) in &gateway_info.gateway_for {
                        for gateway_info in gateway_infos {
                            start_datetime = gateway_info.timestamp.min(start_datetime);
                            let telemetry_value = TelemetryValue {
                                timestamp: gateway_info.timestamp,
                                value: gateway_info
                                    .rx_info
                                    .as_ref()
                                    .map(|rx_info| {
                                        max_rssi = max_rssi.max(rx_info.rx_rssi as f32);
                                        rx_info.rx_rssi as f64
                                    })
                                    .unwrap_or(0.0),
                            };
                            rssi_per_node
                                .entry(Some(*node_id))
                                .or_default()
                                .push(telemetry_value);
                        }
                    }

                    let mut sorted: Vec<_> = rssi_per_node.iter().collect();
                    sorted.sort_by_key(|(k, _)| *k);

                    egui::CentralPanel::default().show(ctx, |ui| {
                        if sorted.len() != 0 {
                            telemetry.ui(
                                ui,
                                &self.nodes,
                                start_datetime,
                                sorted,
                                None,
                                Some(format!("{} RSSI", gateway_id)),
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
                    self.persistent.map.ui(
                        ui,
                        &mut self.map_context,
                        &mut self.persistent.node_filter,
                        &self.nodes,
                        &mut self.fix_gnss,
                    )
                });
            }
            Panel::GatewayByHops(gateway_id, telemetry) => {
                if let Some(gateway_info) = self.nodes.get(gateway_id) {
                    let mut start_datetime = DateTime::<Utc>::MAX_UTC;
                    let mut hops_per_node: HashMap<Option<NodeId>, Vec<TelemetryValue>> =
                        Default::default();
                    let mut dup_packets: HashMap<(Option<NodeId>, u32), Vec<TelemetryValue>> =
                        Default::default();

                    for (node_id, gateway_infos) in &gateway_info.gateway_for {
                        for gateway_info in gateway_infos {
                            start_datetime = gateway_info.timestamp.min(start_datetime);
                            let telemetry_value = TelemetryValue {
                                timestamp: gateway_info.timestamp,
                                value: if let Some(hop_distance) = gateway_info.hop_distance {
                                    hop_distance as f64
                                } else {
                                    -(gateway_info.hop_limit as f64)
                                },
                            };

                            dup_packets
                                .entry((Some(*node_id), gateway_info.packet_id))
                                .or_default()
                                .push(telemetry_value.clone());

                            hops_per_node
                                .entry(Some(*node_id))
                                .or_default()
                                .push(telemetry_value);
                        }
                    }

                    let mut sorted: Vec<_> = hops_per_node.iter().collect();
                    sorted.sort_by_key(|(k, _)| *k);

                    egui::CentralPanel::default().show(ctx, |ui| {
                        if sorted.len() != 0 {
                            telemetry.ui(
                                ui,
                                &self.nodes,
                                start_datetime,
                                sorted,
                                Some(dup_packets),
                                Some(format!("{} hops", gateway_id)),
                                false,
                                None,
                            )
                        } else {
                            ui.label("No data");
                        }
                    });
                }
            }
            Panel::NodeDump => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.persistent.node_dump.ui(
                        ui,
                        self.persistent.node_filter.filter_for(&self.nodes),
                        &self.fix_gnss,
                    )
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
                                .selectable_label(self.persistent.roster.show, "🔍")
                                .clicked()
                            {
                                self.persistent.roster.show = !self.persistent.roster.show;
                            }

                            let menu_text = match self.persistent.active_panel {
                                Panel::Journal => "Journal".into(),
                                Panel::Telemetry(_) => "Telemetry".into(),
                                Panel::Settings(_) => "Settings".into(),
                                Panel::Rssi(node_id, _) => {
                                    format!("Heard by RSSI {}", node_id)
                                }
                                Panel::Hops(node_id, _) => {
                                    format!("Hops away {}", node_id)
                                }
                                Panel::GatewayByRSSI(node_id, _) => {
                                    format!("Income RSSI ({})", node_id)
                                }
                                Panel::Map => "Map".into(),
                                Panel::GatewayByHops(node_id, _) => {
                                    format!("Income hops ({})", node_id)
                                }
                                Panel::NodeDump => format!("Text"),
                            };

                            ui.menu_button(menu_text, |ui| {
                                if ui.button("Journal").clicked() {
                                    self.persistent.active_panel = Panel::Journal;
                                    self.persistent.roster.show = false;
                                }

                                if ui.button("Telemetry").clicked() {
                                    self.persistent.active_panel = Panel::Telemetry(Telemetry {});
                                    self.persistent.roster.show = false;
                                }

                                if ui.button("Map").clicked() {
                                    self.persistent.active_panel = Panel::Map;
                                    self.persistent.roster.show = false;
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
                                    self.persistent.roster.show = false;
                                }

                                let fps = (1.0 / ui.ctx().input(|i| i.stable_dt)).round();
                                ui.label(RichText::new(fps.to_string()).small());
                            });
                        })
                    });
            });
        });

        let roster = &mut self.persistent.roster;
        let hide_on_action = ctx.content_rect().width() < 400.0;

        // if ctx.content_rect().width() > 400.0 {

        if roster.show {
            let mut map_plugin = MapRosterPlugin::new(&mut self.persistent.map, &mut self.fix_gnss);
            let mut journal_plugin = JournalRosterPlugin::new(&mut self.persistent.journal);
            egui::SidePanel::left("Roster").show(ctx, |ui| {
                if let Some(next_panel) = roster.ui(
                    ui,
                    &self.persistent.telemetry_formatter,
                    vec![&mut map_plugin, &mut journal_plugin],
                    &mut self.persistent.node_filter,
                    &self.nodes,
                    hide_on_action,
                ) {
                    self.persistent.active_panel = next_panel;
                }
            });
        }
        self.update_central_panel(ctx);
        // } else {
        //     if list_panel.show {
        //         let nodes_list = self.nodes.iter().map(|(_, v)| v).collect();
        //         egui::CentralPanel::default().show(ctx, |ui| {
        //             if let Some(next_panel) = list_panel.ui(
        //                 ui,
        //                 nodes_list,
        //                 |ui, node_info| {
        //                     self.persistent.map.panel_node_ui(
        //                         ui,
        //                         node_info,
        //                         &mut self.map_context,
        //                         &mut self.fix_gnss,
        //                     )
        //                 },
        //                 None,
        //                 panel_filter,
        //             ) {
        //                 self.persistent.active_panel = next_panel;
        //             }
        //         });
        //     } else {
        //         self.update_central_panel(ctx);
        //     }
        // }
    }
}
