use chrono::{DateTime, NaiveTime, Utc};
use egui::{Color32, epaint::Hsva};
use egui_plot::PlotItem;
use std::time::Duration;

use super::data::NodeTelemetry;

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct Telemetry {}

#[derive(Default)]
struct ColorGenerator {
    next_auto_color_idx: usize,
}

impl ColorGenerator {
    fn next_color(&mut self) -> Color32 {
        let i = self.next_auto_color_idx;
        self.next_auto_color_idx += 1;
        let golden_ratio = (5.0_f32.sqrt() - 1.0) / 2.0; // 0.61803398875
        let h = i as f32 * golden_ratio;
        Hsva::new(h, 0.85, 0.5, 1.0).into() // TODO(emilk): OkLab or some other perspective color space
    }
}

impl Telemetry {
    fn base_datetime(&self, start_time: DateTime<Utc>) -> DateTime<Utc> {
        let base_datetime = start_time
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        DateTime::<Utc>::from_naive_utc_and_offset(base_datetime, Utc)
    }

    const SECS_PER_MIN: f64 = 60.0;
    const MINS_PER_DAY: f64 = 24.0 * 60.0;
    const MINS_PER_H: f64 = 60.0;

    fn x_grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
        // Note: this always fills all possible marks. For optimization, `input.bounds`
        // could be used to decide when the low-interval grids (minutes) should be added.

        let mut marks = vec![];

        let (min, max) = input.bounds;
        let min = min.floor() as usize;
        let max = max.ceil() as usize;

        for i in min..=max {
            let step_size = if i % Self::MINS_PER_DAY as usize == 0 {
                // 1 day
                Self::MINS_PER_DAY
            } else if i % Self::MINS_PER_H as usize == 0 {
                // 1 hour
                Self::MINS_PER_H
            } else if i % 5 == 0 {
                // 5 minutes
                5.0
            } else {
                // skip grids below 5 minutes
                continue;
            };

            marks.push(egui_plot::GridMark {
                value: i as f64,
                step_size,
            });
        }

        marks
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        start_time: DateTime<Utc>,
        telemetry: Vec<(String, &Vec<NodeTelemetry>)>,
        title: Option<String>,
        draw_line: bool,
        stem_base: Option<f32>,
    ) {
        let mut color_generator: ColorGenerator = Default::default();
        let basetime = self.base_datetime(start_time);
        let tf = TimeFormatter::new(basetime);
        let lf = LabelFormatter::new(basetime);

        let x_axes = vec![
            egui_plot::AxisHints::new_x()
                .formatter(|a, b| tf.format(a, b))
                .placement(egui_plot::VPlacement::Top),
            egui_plot::AxisHints::new_x().formatter(|a, b| tf.format(a, b)),
        ];

        let legend = if let Some(title) = title {
            egui_plot::Legend::default().title(title.as_str())
        } else {
            egui_plot::Legend::default()
        }
        .follow_insertion_order(true);

        let legend_plot = egui_plot::Plot::new("telemetry_plot")
            .legend(legend)
            .custom_x_axes(x_axes)
            .x_grid_spacer(Self::x_grid)
            .label_formatter(|a, b| lf.format(a, b));

        legend_plot.show(ui, |plot_ui| {
            // if let Some((title, node_telemetry)) = telemetry.first() {
            for (title, node_telemetry) in telemetry.iter() {
                let points: Vec<[f64; 2]> = node_telemetry
                    .iter()
                    .map(|v| {
                        [
                            ((v.timestamp.timestamp() - basetime.timestamp()) / 60) as f64,
                            v.value,
                        ]
                    })
                    .collect();

                let color = color_generator.next_color();

                let mut plot_points = egui_plot::Points::new(title, points.clone())
                    .radius(4.0)
                    .color(color);
                if let Some(average) = stem_base {
                    plot_points = plot_points.stems(average);
                }
                if draw_line {
                    let id = PlotItem::id(&plot_points);
                    let color = PlotItem::color(&plot_points);
                    plot_ui.points(plot_points);
                    plot_ui.line(egui_plot::Line::new(title, points).id(id).color(color));
                } else {
                    plot_ui.points(plot_points);
                }
            }
        });
    }
}

struct LabelFormatter {
    start_time: DateTime<Utc>,
}

impl LabelFormatter {
    pub fn new(start_time: DateTime<Utc>) -> Self {
        Self { start_time }
    }

    fn format(&self, s: &str, point: &egui_plot::PlotPoint) -> String {
        let seconds = point.x * Telemetry::SECS_PER_MIN;
        let datetime = self.start_time + Duration::from_secs(seconds as u64);
        let str_datetime = datetime.format("%d/%m/%Y %H:%M");

        if s.is_empty() {
            format!("{}", str_datetime)
        } else {
            format!("{}\n{}", s, str_datetime)
        }
    }
}

struct TimeFormatter {
    start_time: DateTime<Utc>,
}

impl TimeFormatter {
    pub fn new(start_time: DateTime<Utc>) -> Self {
        Self { start_time }
    }

    pub fn format(
        &self,
        mark: egui_plot::GridMark,
        _range: &std::ops::RangeInclusive<f64>,
    ) -> String {
        fn hour(x: f64) -> f64 {
            (x.rem_euclid(Telemetry::MINS_PER_DAY) / Telemetry::MINS_PER_H).floor()
        }

        fn minute(x: f64) -> f64 {
            x.rem_euclid(Telemetry::MINS_PER_H).floor()
        }

        let minutes = mark.value;
        if is_approx_integer(minutes / Telemetry::MINS_PER_DAY) {
            let seconds = minutes * Telemetry::SECS_PER_MIN;
            let datetime = self.start_time + Duration::from_secs(seconds as u64);

            format!("{}", datetime.format("%d/%m/%Y"))
        } else {
            // Hours and minutes
            format!("{h}:{m:02}", h = hour(minutes), m = minute(minutes))
        }
    }
}

fn is_approx_integer(val: f64) -> bool {
    val.fract().abs() < 1e-6
}
