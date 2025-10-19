use crate::app::{PanelCommand, data::NodeInfo};

pub trait RosterPlugin {
    fn panel_header_ui(self: &mut Self, ui: &mut egui::Ui) -> PanelCommand;
    fn panel_node_ui(self: &mut Self, ui: &mut egui::Ui, node_info: &NodeInfo) -> PanelCommand;
}
