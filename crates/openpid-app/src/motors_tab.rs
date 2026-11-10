//! Motors in the axis-row layout (docs/design/motors-tab, M4): per axis the
//! unfiltered gyro's spectrum with the motor harmonics marked, and beside it
//! each motor's ×1, ×2, ×3 amplitude over the mean of the motors, so a
//! louder motor (bearing, bent prop, loose mount) stands out on every axis.
//! Every control is in the panel on the right.

use crate::app::{App, Loaded};
use crate::axis_layout::control_panel;
use crate::colormap::MUTED;
use crate::i18n::tr;
use crate::panel;
use crate::views::{motor_color, raw_gyro_fix, show_error};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Bar, BarChart, GridMark, HLine, Line, LineStyle, Plot, PlotPoints, VLine};
use openpid_analysis::MotorNoiseOutput;
use openpid_core::{Axis, ChannelId, GyroStage};
use std::sync::Arc;

/// Width of the bars beside each spectrum.
const BARS_W: f32 = 250.0;
/// Width of the y-axis labels, the same on every spectrum.
const Y_AXIS_W: f32 = 52.0;
/// A harmonic this much louder than the mean of the motors is marked.
const LOUD_RATIO: f64 = 1.3;

/// Display options of the tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MotorsDisplay {
    /// One marker per motor and harmonic instead of one per harmonic at the
    /// mean motor speed.
    pub per_motor: bool,
}

/// The unfiltered gyro on `axis`.
fn raw(axis: Axis) -> ChannelId {
    ChannelId::Gyro {
        axis,
        stage: GyroStage::Raw,
    }
}

/// The tab.
pub fn motors(app: &mut App, ui: &mut Ui) {
    let rows = results(app);
    control_panel(app, ui, "motors-panel", |app, ui| controls(app, ui, &rows));
    ui.separator();
    let Some(slot) = app.dock_slot() else { return };
    if rows.is_empty() {
        if app.loaded(slot).is_some() {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    tr("no unfiltered gyro in this log (the filtered one has the motor peaks removed);"),
                    raw_gyro_fix(app, slot)
                ))
                .color(MUTED),
            );
        }
        return;
    }
    ui.label(
        RichText::new(tr(
            "Lines: the motor harmonics, ×1 solid, ×2 and ×3 dashed. Bars: each motor's ×1, ×2, ×3 over the mean of the motors (1 = average); a louder motor stands out on every axis.",
        ))
        .color(MUTED),
    );
    let gap = ui.spacing().item_spacing.y;
    let n = rows.len().max(1) as f32;
    let h = ((ui.available_height() - gap * (n - 1.0) - 20.0) / n).max(60.0);
    let link = egui::Id::unique("motors-x");
    let last = rows.len() - 1;
    for (i, (axis, r)) in rows.iter().enumerate() {
        let height = h + if i == last { 20.0 } else { 0.0 };
        match r {
            Some(Ok(o)) => {
                let Output::MotorNoise(m) = o.as_ref() else {
                    continue;
                };
                ui.horizontal(|ui| {
                    // the plots' own margins take a few pixels more than their widths
                    let w =
                        ui.available_width() - BARS_W - 2.0 * ui.spacing().item_spacing.x - 16.0;
                    spectrum(
                        app,
                        ui,
                        slot,
                        *axis,
                        m,
                        egui::vec2(w, height),
                        i == last,
                        link,
                    );
                    bars(ui, *axis, m, egui::vec2(BARS_W, height));
                });
            }
            Some(Err(e)) => show_error(ui, slot, e),
            None => {
                ui.spinner();
            }
        }
    }
}

/// One row per axis with the unfiltered gyro: its result, `None` while computing.
type Rows = Vec<(Axis, Option<Result<Arc<Output>, String>>)>;

fn results(app: &mut App) -> Rows {
    let Some(slot) = app.dock_slot() else {
        return Vec::new();
    };
    let base = app.tools.motor_noise.clone();
    // the export and the project read the roll row's job
    app.tools.motor_noise.channel = raw(Axis::Roll);
    let mut rows = Vec::new();
    for axis in crate::axis_layout::shown_axes(app) {
        let has_raw = app
            .loaded(slot)
            .and_then(Loaded::current)
            .is_some_and(|s| s.channel(&raw(axis)).is_some());
        if !has_raw {
            continue;
        }
        let mut p = base.clone();
        p.channel = raw(axis);
        rows.push((axis, app.result(slot, Kind::MotorNoise(p))));
    }
    rows
}

/// The mean frequency of harmonic `k` (1-based) over the motors.
fn mean_hz(m: &MotorNoiseOutput, k: u32) -> Option<f64> {
    let hz: Vec<f64> = m
        .motors
        .iter()
        .filter_map(|motor| motor.harmonics.iter().find(|h| h.k == k))
        .map(|h| h.mean_hz)
        .filter(|f| *f > 0.0)
        .collect();
    (!hz.is_empty()).then(|| hz.iter().sum::<f64>() / hz.len() as f64)
}

/// A row's spectrum with the harmonics marked.
#[allow(clippy::too_many_arguments, reason = "one plot of a row")]
fn spectrum(
    app: &App,
    ui: &mut Ui,
    slot: Slot,
    axis: Axis,
    m: &MotorNoiseOutput,
    size: egui::Vec2,
    last: bool,
    link: egui::Id,
) {
    let text = ui.visuals().text_color();
    let per_motor = app.tools.motors_display.per_motor;
    let harmonics = app.tools.motor_noise.harmonics;
    let mut plot = Plot::new(("motors-psd", slot.name(), axis.name()))
        .width(size.x)
        .height(size.y)
        .y_axis_min_width(Y_AXIS_W)
        .show_axes([last, true])
        .link_axis(link, [true, false])
        .link_cursor(link, [true, false])
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .y_axis_label(format!("{} {}", axis.name(), tr("prefilt (dB)")))
        .allow_scroll(false)
        .allow_drag([true, false])
        .allow_zoom([true, false]);
    if last {
        plot = plot.x_axis_label(tr("frequency (Hz)"));
    }
    plot.show(ui, |pu| {
        let pts: Vec<[f64; 2]> = m
            .freq
            .iter()
            .zip(&m.psd_db)
            .map(|(f, y)| [*f, *y])
            .collect();
        pu.line(
            Line::new(m.channel.key(), PlotPoints::from(pts))
                .color(Color32::from_gray(200))
                .width(1.2),
        );
        let style = |k: u32| {
            if k == 1 {
                LineStyle::Solid
            } else {
                LineStyle::dashed_dense()
            }
        };
        if per_motor {
            for motor in &m.motors {
                for h in motor.harmonics.iter().filter(|h| h.mean_hz > 0.0) {
                    pu.vline(
                        VLine::new(format!("M{} ×{}", motor.idx + 1, h.k), h.mean_hz)
                            .color(motor_color(usize::from(motor.idx)))
                            .style(style(h.k))
                            .width(1.0),
                    );
                }
            }
        } else {
            for k in 1..=harmonics {
                if let Some(f) = mean_hz(m, k) {
                    pu.vline(
                        VLine::new(format!("×{k}"), f)
                            .color(text.gamma_multiply(0.7))
                            .style(style(k))
                            .width(1.0),
                    );
                }
            }
        }
    });
}

/// Each motor's harmonics over the mean of the motors (1 = average).
fn bars(ui: &mut Ui, axis: Axis, m: &MotorNoiseOutput, size: egui::Vec2) {
    let n = m.motors.len();
    let harmonics = m
        .motors
        .iter()
        .map(|x| x.harmonics.len())
        .max()
        .unwrap_or(1);
    let width = 0.8 / harmonics as f64;
    let mut chart = Vec::new();
    for (i, motor) in m.motors.iter().enumerate() {
        let colour = motor_color(usize::from(motor.idx));
        for (k, h) in motor.harmonics.iter().enumerate() {
            let x = i as f64 + (k as f64 - (harmonics as f64 - 1.0) / 2.0) * width;
            let alpha = [1.0, 0.65, 0.4].get(k).copied().unwrap_or(0.3);
            chart.push(
                Bar::new(x, h.ratio)
                    .width(width * 0.9)
                    .fill(colour.gamma_multiply(alpha))
                    .stroke(egui::Stroke::NONE)
                    .name(format!(
                        "M{} ×{}: {:.2} ({} {:.2})",
                        motor.idx + 1,
                        h.k,
                        h.ratio,
                        tr("amplitude"),
                        h.amplitude
                    )),
            );
        }
    }
    let names: Vec<String> = m
        .motors
        .iter()
        .map(|motor| {
            let loud = m.loudest == Some(motor.idx) && n > 1;
            format!("M{}{}", motor.idx + 1, if loud { " !" } else { "" })
        })
        .collect();
    let plot = Plot::new(("motors-bars", axis.name()))
        .width(size.x)
        .height(size.y)
        .y_axis_min_width(28.0)
        .include_y(0.0)
        .include_y(1.5)
        .include_x(-0.6)
        .include_x(n as f64 - 0.4)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_grid([false, true])
        // a mark per motor: the motor names are the labels
        .x_grid_spacer(move |_| {
            (0..n)
                .map(|i| GridMark {
                    value: i as f64,
                    // a large step keeps the labels: egui_plot fades marks
                    // whose step is narrow on screen
                    step_size: 10.0,
                })
                .collect()
        })
        .x_axis_formatter(move |mark, _| {
            let i = mark.value.round();
            if (mark.value - i).abs() < 1e-6 && i >= 0.0 {
                names.get(i as usize).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        })
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label);
    plot.show(ui, |pu| {
        pu.hline(
            HLine::new(tr("mean of the motors"), 1.0)
                .color(MUTED)
                .style(LineStyle::dashed_loose()),
        );
        pu.bar_chart(BarChart::new(axis.name(), chart));
    });
}

/// The panel: data, markers, the motors.
fn controls(app: &mut App, ui: &mut Ui, rows: &Rows) {
    crate::axis_layout::axes_row(app, ui);
    panel::section(ui, tr("Data"));
    let mut harmonics = app.tools.motor_noise.harmonics as usize;
    if panel::drag_usize(
        ui,
        tr("Harmonics"),
        tr("multiples of each motor's frequency to read: 1 is the motor itself, 2 and 3 its overtones"),
        &mut harmonics,
        1..=8,
    )
    .changed()
    {
        app.tools.motor_noise.harmonics = harmonics as u32;
    }
    panel::drag(
        ui,
        tr("Segment"),
        tr(
            "length of each spectrum segment: longer gives finer frequency steps and fewer averages",
        ),
        &mut app.tools.motor_noise.segment_s,
        0.01,
        0.05..=2.0,
        "s",
    );
    panel::section(ui, tr("Markers"));
    panel::check(
        ui,
        &mut app.tools.motors_display.per_motor,
        tr("Per motor"),
        tr(
            "a marker per motor and harmonic in the motor's colour; off: one per harmonic at the mean motor speed",
        ),
    );
    let outputs: Vec<(Axis, &MotorNoiseOutput)> = rows
        .iter()
        .filter_map(|(axis, r)| match r {
            Some(Ok(o)) => match o.as_ref() {
                Output::MotorNoise(m) => Some((*axis, m)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let Some((_, first)) = outputs.first() else {
        return;
    };
    panel::section(ui, tr("Motors"));
    ui.label(
        RichText::new(format!(
            "{} {}, {} {}",
            tr("rpm"),
            first.rpm_source,
            first.frames,
            tr("frames")
        ))
        .color(MUTED),
    );
    if first.motors.len() > 1 {
        let per_axis: Vec<String> = outputs
            .iter()
            .filter_map(|(axis, m)| m.loudest.map(|i| format!("{} M{}", axis.name(), i + 1)))
            .collect();
        ui.label(format!("{}: {}", tr("loudest"), per_axis.join(", ")));
    }
    // ×1 over the mean, per axis: the reading of the bars as numbers
    ui.label(RichText::new(tr("×1 over the mean of the motors")).color(MUTED));
    // narrow enough for the panel's width
    egui::Grid::new("motors-table")
        .striped(true)
        .min_col_width(0.0)
        .spacing([10.0, 4.0])
        .num_columns(2 + outputs.len())
        .show(ui, |ui| {
            ui.label(RichText::new(tr("motor")).strong());
            ui.label(RichText::new(tr("rpm")).strong());
            for (axis, _) in &outputs {
                ui.label(RichText::new(axis.name()).strong());
            }
            ui.end_row();
            for (i, motor) in first.motors.iter().enumerate() {
                ui.label(
                    RichText::new(format!("M{}", motor.idx + 1))
                        .color(motor_color(usize::from(motor.idx))),
                );
                ui.label(format!("{:.0}", motor.mean_rpm));
                for (_, m) in &outputs {
                    let ratio = m
                        .motors
                        .get(i)
                        .and_then(|x| x.harmonics.first())
                        .map_or(f64::NAN, |h| h.ratio);
                    let text = RichText::new(format!("{ratio:.2}"));
                    ui.label(if ratio > LOUD_RATIO {
                        text.color(crate::colormap::ACCENT).strong()
                    } else {
                        text
                    });
                }
                ui.end_row();
            }
        });
}
