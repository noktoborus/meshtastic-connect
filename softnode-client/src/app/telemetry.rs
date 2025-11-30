use chrono::{DateTime, NaiveTime, Utc};
use egui::{Align2, Color32, RichText, Style, TextStyle, epaint::Hsva};
use egui_plot::{HLine, Line, PlotItem, PlotUi, Points, Text};
use std::{sync::Arc, time::Duration};

use crate::app::data::{NodeTelemetry, TelemetryValue};

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

fn plot_value_is_printable(_plot_ui: &PlotUi<'_>) -> Option<TextStyle> {
    // let bounds = plot_ui.plot_bounds();
    // let visible_width = bounds.max()[0] - bounds.min()[0];
    // let visible_height = bounds.max()[1] - bounds.min()[1];

    // const ZOOM_THRESHOLD: f64 = 500.0;
    // const ZOOM_SMALL_THRESHOLD: f64 = 1200.0;

    // if visible_width < ZOOM_THRESHOLD && visible_height < ZOOM_THRESHOLD {
    Some(TextStyle::Body)
    // } else if visible_width < ZOOM_SMALL_THRESHOLD && visible_height < ZOOM_SMALL_THRESHOLD {
    //     Some(TextStyle::Small)
    // } else {
    //     None
    // }
}

fn plot_value(
    text_value_style: TextStyle,
    style: &Arc<Style>,
    name: &str,
    color: Color32,
    plot_ui: &mut PlotUi<'_>,
    value: &TelemetryValue,
    anchor: Align2,
    basetime: DateTime<Utc>,
) {
    let point = [
        ((value.timestamp.timestamp() - basetime.timestamp()) / 60) as f64,
        value.value,
    ];

    let title = format!("{:.2}", point[1]);

    let text_widget = RichText::new(title)
        .text_style(text_value_style)
        .background_color(style.visuals.extreme_bg_color.gamma_multiply(0.5));
    let text = Text::new(name, point.into(), text_widget)
        .color(color)
        .anchor(anchor);

    plot_ui.text(text);
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
        telemetry: Vec<(String, &NodeTelemetry)>,
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

        let legend = egui_plot::Legend::default()
            .position(egui_plot::Corner::LeftTop)
            .follow_insertion_order(true);

        let legend_plot = egui_plot::Plot::new("telemetry_plot")
            .legend(legend)
            .custom_x_axes(x_axes)
            .x_grid_spacer(Self::x_grid)
            .label_formatter(|a, b| lf.format(a, b));

        let style = ui.style().clone();
        legend_plot.show(ui, |plot_ui| {
            let text_value_style = plot_value_is_printable(plot_ui);
            for (title, node_telemetry) in telemetry.iter() {
                let mut min_value: Option<TelemetryValue> = None;
                let mut max_value: Option<TelemetryValue> = None;
                let points: Vec<[f64; 2]> = node_telemetry
                    .values
                    .iter()
                    .map(|v| {
                        if min_value.as_ref().map_or(true, |min| min.value > v.value) {
                            min_value = Some(v.clone());
                        }
                        if max_value.as_ref().map_or(true, |max| max.value < v.value) {
                            max_value = Some(v.clone());
                        }
                        [
                            ((v.timestamp.timestamp() - basetime.timestamp()) / 60) as f64,
                            v.value,
                        ]
                    })
                    .collect();
                let color = color_generator.next_color();
                let plot_points = Points::new(title, points.clone()).radius(4.0).color(color);
                if min_value != max_value {
                    if let Some(min_value) = &min_value {
                        plot_ui.hline(HLine::new(title, min_value.value).color(color).width(0.5));
                        plot_value(
                            TextStyle::Small,
                            &style,
                            title,
                            color,
                            plot_ui,
                            &min_value,
                            Align2::CENTER_TOP,
                            basetime,
                        );
                    }

                    if let Some(max_value) = &max_value {
                        plot_ui.hline(HLine::new(title, max_value.value).color(color).width(0.5));
                        plot_value(
                            TextStyle::Small,
                            &style,
                            title,
                            color,
                            plot_ui,
                            &max_value,
                            Align2::CENTER_BOTTOM,
                            basetime,
                        );
                    }
                }
                let id = PlotItem::id(&plot_points);
                let color = PlotItem::color(&plot_points);
                plot_ui.points(plot_points);
                plot_ui.line(Line::new(title, points).id(id).color(color).width(3.0));

                if let Some(text_value_style) = &text_value_style {
                    if let Some(first_value) = node_telemetry.values.first() {
                        if Some(first_value.value) != node_telemetry.values.last().map(|v| v.value)
                        {
                            plot_value(
                                text_value_style.clone(),
                                &style,
                                title,
                                color,
                                plot_ui,
                                first_value,
                                Align2::RIGHT_CENTER,
                                basetime,
                            );
                        }
                    }

                    if node_telemetry.values.len() > 1 {
                        for min_peaks_value in node_telemetry.min_peaks.iter() {
                            plot_value(
                                text_value_style.clone(),
                                &style,
                                title,
                                color,
                                plot_ui,
                                min_peaks_value,
                                Align2::CENTER_BOTTOM,
                                basetime,
                            );
                        }
                        for max_peaks_value in node_telemetry.max_peaks.iter() {
                            plot_value(
                                text_value_style.clone(),
                                &style,
                                title,
                                color,
                                plot_ui,
                                max_peaks_value,
                                Align2::CENTER_TOP,
                                basetime,
                            );
                        }
                    }
                }
                if let Some(last_value) = node_telemetry.values.last() {
                    plot_value(
                        TextStyle::Body,
                        &style,
                        title,
                        color,
                        plot_ui,
                        last_value,
                        Align2::LEFT_CENTER,
                        basetime,
                    );
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
            format!("{:.2}\n{}", point.y, str_datetime)
        } else {
            format!("{:.2}\n{}\n{}", point.y, s, str_datetime)
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
