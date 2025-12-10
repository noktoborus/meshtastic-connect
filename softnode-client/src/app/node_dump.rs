use walkers::lon_lat;

use crate::app::{fix_gnss::FixGnssLibrary, node_filter::NodeFilterIterator};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct NodeDump {
    show_position: bool,
    show_pkey: bool,
}

impl NodeDump {
    pub fn new() -> Self {
        Self {
            show_position: true,
            show_pkey: false,
        }
    }

    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        node_iterator: NodeFilterIterator<'a>,
        fix_gnss: &FixGnssLibrary,
    ) {
        let mut text = String::new();
        let mut counter = 0;

        for node_info in node_iterator {
            counter += 1;
            let position = if self.show_position {
                let (position, position_marker) = if let Some(fix_position) = fix_gnss
                    .node_get(&node_info.node_id)
                    .map(|v| lon_lat(v.longitude, v.latitude))
                {
                    (fix_position, "!")
                } else if let Some(assumed) = node_info.assumed_position {
                    (assumed, "?")
                } else if let Some(received) = node_info.position.last() {
                    (lon_lat(received.longitude, received.latitude), " ")
                } else {
                    (lon_lat(0.0, 0.0), " ")
                };
                format!(
                    " {}[{:.5}, {:.5}]",
                    position_marker,
                    position.x(),
                    position.y()
                )
            } else {
                format!("")
            };

            let public_key = if self.show_pkey {
                if let Some(pkey) = node_info.extended_info_history.last().map(|v| &v.pkey) {
                    match pkey {
                        crate::app::data::PublicKey::None => "".to_string(),
                        crate::app::data::PublicKey::Key(key) => format!(" {}", key),
                        crate::app::data::PublicKey::Compromised(key) => {
                            format!("!{}", key)
                        }
                    }
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            };

            text.push_str(
                format!(
                    "{}:{} {} {}\n",
                    node_info.node_id,
                    position,
                    node_info
                        .extended_info_history
                        .last()
                        .map(|v| format!("{} ({})", v.short_name.clone(), v.long_name.clone()))
                        .unwrap_or_default(),
                    public_key,
                )
                .as_str(),
            );
        }

        text.push_str(format!("Total nodes: {}", counter).as_str());

        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📋 Copy").clicked() {
                    ui.ctx().copy_text(text.clone());
                }
                ui.checkbox(&mut self.show_pkey, "Show Public Key");
                ui.checkbox(&mut self.show_position, "Show Position");
            });
            ui.label(egui::RichText::new(&text).monospace());
        });
    }
}
