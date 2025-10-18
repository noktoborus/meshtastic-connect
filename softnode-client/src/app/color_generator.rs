// stoled from https://github.com/emilk/egui_plot/blob/a5c5a623de5b30e6a84831f23e199bcd979e38f9/egui_plot/src/plot_ui.rs#L23

use egui::{Color32, epaint::Hsva};

#[derive(Default)]
pub struct ColorGenerator {
    next_auto_color_idx: usize,
}

impl ColorGenerator {
    pub fn next_color(&mut self) -> Color32 {
        let i = self.next_auto_color_idx;
        self.next_auto_color_idx += 1;
        let golden_ratio = (5.0_f32.sqrt() - 1.0) / 2.0; // 0.61803398875
        let h = i as f32 * golden_ratio;
        Hsva::new(h, 0.85, 0.5, 1.0).into() // TODO(emilk): OkLab or some other perspective color space
    }
}
