//! The Spectrum tab in the axis-row layout (docs/design/spectrum-tab, V1):
//! roll / pitch / yaw stacked, each with the full spectrum and the range
//! below 100 Hz side by side; per log the chosen quantity solid and, for the
//! gyro, the unfiltered gyro dotted. Every control is in the panel on the
//! right.

use crate::app::App;
use crate::axis_layout::{control_panel, row_height};
use crate::colormap::ACCENT;
use crate::i18n::tr;
use crate::views::{hz, nearest_bin, show_error, slot_color};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{
    GridInput, GridMark, HoverPosition, Line, LineStyle, Plot, PlotPoint, PlotPoints, Text, VLine,
};
use openpid_analysis::{PsdOutput, PsdParams};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage, PidTerm};
use openpid_dsp::Peak;
use std::sync::Arc;

/// Upper end of the low-frequency column.
const LOW_MAX_HZ: f64 = 100.0;
/// Shortest segment of the low-frequency column: 1 Hz bins below 100 Hz.
const LOW_SEGMENT_S: f64 = 1.0;
/// Lowest frequency on a logarithmic axis.
const LOG_MIN_HZ: f64 = 2.0;
/// Automatic peaks start here: below it is the flying itself, not noise.
pub const PEAK_MIN_HZ: f64 = 20.0;
/// Height of the frequency label under the last row.
const X_LABEL_H: f32 = 18.0;
/// Share of the width taken by the full-spectrum column.
const FULL_SHARE: f32 = 0.6;

/// Smoothing levels: the width (Hz) of the moving average over the power.
pub const SMOOTHING: [(&str, f64); 6] = [
    ("off", 0.0),
    ("low", 2.0),
    ("low-med", 4.0),
    ("medium", 8.0),
    ("med-high", 16.0),
    ("high", 32.0),
];

/// What the rows show, per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quantity {
    /// The filtered gyro, with the unfiltered gyro as its prefilt.
    #[default]
    Gyro,
    /// The D term.
    DTerm,
    /// The P term.
    PTerm,
    /// The PID sum.
    PidSum,
    /// Setpoint minus gyro.
    PidError,
    /// The setpoint.
    Setpoint,
}

impl Quantity {
    pub const ALL: [Quantity; 6] = [
        Quantity::Gyro,
        Quantity::DTerm,
        Quantity::PTerm,
        Quantity::PidSum,
        Quantity::PidError,
        Quantity::Setpoint,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Quantity::Gyro => tr("Gyro"),
            Quantity::DTerm => tr("D-term"),
            Quantity::PTerm => tr("P-term"),
            Quantity::PidSum => tr("PID sum"),
            Quantity::PidError => tr("PID error"),
            Quantity::Setpoint => tr("Setpoint"),
        }
    }

    /// The channel on `axis`.
    pub fn channel(self, axis: Axis) -> ChannelId {
        match self {
            Quantity::Gyro => ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            },
            Quantity::DTerm => ChannelId::PidTerm {
                axis,
                term: PidTerm::D,
            },
            Quantity::PTerm => ChannelId::PidTerm {
                axis,
                term: PidTerm::P,
            },
            Quantity::PidSum => ChannelId::PidTerm {
                axis,
                term: PidTerm::Sum,
            },
            Quantity::PidError => ChannelId::PidError { axis },
            Quantity::Setpoint => ChannelId::Setpoint { axis },
        }
    }

    /// Its unfiltered counterpart on `axis`, when there is one.
    pub fn prefilt(self, axis: Axis) -> Option<ChannelId> {
        match self {
            Quantity::Gyro => Some(ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            }),
            _ => None,
        }
    }

    /// The quantity a channel shows (projects store the channel).
    pub fn of(channel: &ChannelId) -> Quantity {
        match channel {
            ChannelId::PidTerm {
                term: PidTerm::D, ..
            } => Quantity::DTerm,
            ChannelId::PidTerm {
                term: PidTerm::P, ..
            } => Quantity::PTerm,
            ChannelId::PidTerm {
                term: PidTerm::Sum, ..
            } => Quantity::PidSum,
            ChannelId::PidError { .. } => Quantity::PidError,
            ChannelId::Setpoint { .. } => Quantity::Setpoint,
            _ => Quantity::Gyro,
        }
    }
}

/// Spectrum tab display options (not analysis parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumDisplay {
    /// Index into [`SMOOTHING`]; medium by default.
    pub smoothing: usize,
    /// Draw the unfiltered counterpart dotted.
    pub prefilt: bool,
    /// Fixed lower / upper end of the power axis; `None` = automatic.
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    /// Logarithmic frequency axis on the full-spectrum column.
    pub log_freq: bool,
}

impl Default for SpectrumDisplay {
    fn default() -> Self {
        SpectrumDisplay {
            smoothing: 3,
            prefilt: true,
            y_min: None,
            y_max: None,
            log_freq: false,
        }
    }
}

/// A moving average over `width_hz` of the power (linear units), as the
/// "smoothing" of the panel; `width_hz` ≤ one bin returns the input.
pub fn smooth_power(freq: &[f64], power: &[f64], width_hz: f64) -> Vec<f64> {
    let df = match freq {
        [a, b, ..] => (b - a).abs(),
        _ => return power.to_vec(),
    };
    let half = ((width_hz / df) / 2.0).floor() as usize;
    if half == 0 {
        return power.to_vec();
    }
    (0..power.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half).min(power.len() - 1);
            power[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64
        })
        .collect()
}

/// One trace ready to draw.
struct Trace {
    slot: Slot,
    prefilt: bool,
    x: Vec<f64>,
    y: Vec<f64>,
    /// The analysis behind it (peaks, labels).
    out: Arc<Output>,
}

/// Both columns of one axis.
#[derive(Default)]
struct AxisTraces {
    full: Vec<Trace>,
    low: Vec<Trace>,
}

/// Grid marks 1, 2, 5 per decade for a log10 axis.
fn log_grid(input: GridInput) -> Vec<GridMark> {
    let (lo, hi) = input.bounds;
    let mut out = Vec::new();
    let mut decade = lo.floor() as i32;
    while f64::from(decade) <= hi {
        for (m, step) in [(1.0, 1.0), (2.0, 0.1), (5.0, 0.1)] {
            let v = f64::from(decade) + f64::log10(m);
            if v >= lo && v <= hi {
                out.push(GridMark {
                    value: v,
                    step_size: step,
                });
            }
        }
        decade += 1;
    }
    out
}

/// The Spectrum tab.
pub fn spectrum(app: &mut App, ui: &mut Ui) {
    control_panel(app, ui, "spectrum-panel", controls);
    let full_w = ui.available_width();
    ui.separator();
    let quantity = Quantity::of(&app.tools.spectrum_channel);
    let display = app.tools.spectrum_display.clone();
    let db = app.tools.psd_db;
    let width_hz = SMOOTHING[display.smoothing.min(SMOOTHING.len() - 1)].1;
    let base = app.tools.psd.clone();
    // the export and the project keep the channel of the selected axis
    app.tools.psd.channel = quantity.channel(app.tools.axis);
    let mut rows: Vec<(Axis, AxisTraces)> = Vec::new();
    for axis in Axis::ALL {
        let mut t = AxisTraces::default();
        for slot in app.visible_slots() {
            let Some(session) = app.loaded(slot).and_then(|l| l.current().cloned()) else {
                continue;
            };
            let mut channels = vec![(quantity.channel(axis), false)];
            if display.prefilt
                && let Some(p) = quantity.prefilt(axis)
            {
                channels.push((p, true));
            }
            for (channel, prefilt) in channels {
                if !has(&session, &channel) {
                    continue;
                }
                let full = PsdParams {
                    channel: channel.clone(),
                    ..base.clone()
                };
                let low = PsdParams {
                    channel,
                    segment_s: base.segment_s.max(LOW_SEGMENT_S),
                    max_hz: Some(LOW_MAX_HZ),
                    ..base.clone()
                };
                for (params, into) in [(full, &mut t.full), (low, &mut t.low)] {
                    match app.result(slot, Kind::Psd(params)) {
                        Some(Ok(o)) => {
                            if let Some(tr) = trace(slot, prefilt, o, db, width_hz) {
                                into.push(tr);
                            }
                        }
                        Some(Err(e)) => show_error(ui, slot, &e),
                        None => {}
                    }
                }
            }
        }
        rows.push((axis, t));
    }
    // automatic peaks of every solid full-range trace
    let peaks: Vec<(Slot, Axis, Vec<Peak>)> = rows
        .iter()
        .flat_map(|(axis, t)| {
            t.full.iter().filter(|tr| !tr.prefilt).filter_map(|tr| {
                let Output::Psd(p) = tr.out.as_ref() else {
                    return None;
                };
                let n = app.spectrum.auto_peaks;
                let mut pk: Vec<Peak> = p
                    .peaks(n + 8, app.spectrum.min_prominence_db)
                    .into_iter()
                    .filter(|k| k.x >= PEAK_MIN_HZ)
                    .collect();
                pk.truncate(n);
                Some((tr.slot, *axis, pk))
            })
        })
        .collect();
    let (y_lo, y_hi) = y_range(&rows, &display, db);
    legend(app, ui, quantity, &display, full_w);
    // the last row also carries the frequency label: every plot area the same height
    let h = row_height(ui, 3, X_LABEL_H / 3.0);
    let markers = app.spectrum.markers.clone();
    let mut clicked_at: Option<f64> = None;
    for (i, (axis, t)) in rows.iter().enumerate() {
        let last = i == 2;
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let wf = (full_w * FULL_SHARE - gap).max(200.0);
            let wl = (full_w - wf - gap).max(120.0);
            let full = column_plot(ColumnPlot {
                ui,
                id: ("spec-full", *axis),
                axis: *axis,
                traces: &t.full,
                peaks: &peaks,
                markers: &markers,
                log: display.log_freq,
                low: false,
                y: (y_lo, y_hi),
                db,
                size: egui::vec2(wf, if last { h + X_LABEL_H } else { h }),
                last,
            });
            if *axis == Axis::Roll {
                app.spectrum.plot_frame = Some(full.frame);
                app.spectrum.x_bounds = Some(full.x_bounds);
            }
            let low = column_plot(ColumnPlot {
                ui,
                id: ("spec-low", *axis),
                axis: *axis,
                traces: &t.low,
                peaks: &[],
                markers: &markers,
                log: false,
                low: true,
                y: (y_lo, y_hi),
                db,
                size: egui::vec2(wl, if last { h + X_LABEL_H } else { h }),
                last,
            });
            clicked_at = clicked_at.or(full.clicked).or(low.clicked);
        });
    }
    app.spectrum.peaks = peaks;
    if let Some(x) = clicked_at
        && x >= 0.0
    {
        // snap to the nearest bin of the first trace so the read-out shows a real value
        let snapped = rows
            .first()
            .and_then(|(_, t)| t.full.first())
            .and_then(|tr| match tr.out.as_ref() {
                Output::Psd(p) => nearest_bin(&p.freq, x).map(|i| p.freq[i]),
                _ => None,
            })
            .unwrap_or(x);
        app.spectrum.markers.push(snapped);
    }
}

fn has(session: &FlightSession, channel: &ChannelId) -> bool {
    session.channel(channel).is_some()
}

/// A drawable trace from a PSD: smoothed, in dB or linear.
fn trace(slot: Slot, prefilt: bool, out: Arc<Output>, db: bool, width_hz: f64) -> Option<Trace> {
    let Output::Psd(p) = out.as_ref() else {
        return None;
    };
    let (x, y) = points(p, db, width_hz);
    Some(Trace {
        slot,
        prefilt,
        x,
        y,
        out,
    })
}

fn points(p: &PsdOutput, db: bool, width_hz: f64) -> (Vec<f64>, Vec<f64>) {
    let power = smooth_power(&p.freq, &p.power, width_hz);
    let y = if db {
        power
            .iter()
            .map(|v| 10.0 * v.max(f64::MIN_POSITIVE).log10())
            .collect()
    } else {
        power
    };
    (p.freq.clone(), y)
}

/// The power range of every row and column: the panel's fixed ends, else
/// the drawn data above 2 Hz (the DC bins would set the top), padded and
/// rounded to 10 dB.
fn y_range(rows: &[(Axis, AxisTraces)], d: &SpectrumDisplay, db: bool) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, t) in rows {
        for tr in t.full.iter().chain(&t.low) {
            for (x, y) in tr.x.iter().zip(&tr.y) {
                if *x >= LOG_MIN_HZ && y.is_finite() {
                    lo = lo.min(*y);
                    hi = hi.max(*y);
                }
            }
        }
    }
    if !lo.is_finite() {
        (lo, hi) = if db { (-60.0, 20.0) } else { (0.0, 1.0) };
    } else if db {
        lo = ((lo - 3.0) / 10.0).floor() * 10.0;
        hi = ((hi + 3.0) / 10.0).ceil() * 10.0;
    } else {
        lo = 0.0;
        hi *= 1.05;
    }
    (
        d.y_min.unwrap_or(lo),
        d.y_max.unwrap_or(hi).max(d.y_min.unwrap_or(lo) + 1e-9),
    )
}

/// The legend over the rows: per log its solid line and, with prefilt, its
/// dotted one; the title of the low column over that column.
fn legend(app: &App, ui: &mut Ui, q: Quantity, d: &SpectrumDisplay, full_w: f32) {
    ui.horizontal(|ui| {
        let start = ui.cursor().left();
        for slot in app.visible_slots() {
            let c = slot_color(slot);
            swatch(ui, c, false);
            ui.label(RichText::new(format!("{} {}", slot.name(), q.name())).size(12.0));
            if d.prefilt && q.prefilt(Axis::Roll).is_some() {
                swatch(ui, c, true);
                ui.label(RichText::new(format!("{} {}", slot.name(), tr("prefilt"))).size(12.0));
            }
        }
        let low_x = start + full_w * FULL_SHARE;
        let used = ui.cursor().left();
        if low_x > used {
            ui.add_space(low_x - used);
        }
        ui.label(
            RichText::new(tr("below 100 Hz (1 s segments)"))
                .size(12.0)
                .color(crate::colormap::MUTED),
        );
    });
}

/// A short line sample: solid or dotted.
fn swatch(ui: &mut Ui, c: Color32, dotted: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 10.0), egui::Sense::hover());
    let y = rect.center().y;
    let stroke = egui::Stroke::new(1.8, c);
    if dotted {
        ui.painter().extend(egui::Shape::dotted_line(
            &[egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            c,
            4.0,
            1.0,
        ));
    } else {
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            stroke,
        );
    }
}

/// Everything one plot of the grid needs.
struct ColumnPlot<'a, 'u> {
    ui: &'u mut Ui,
    id: (&'static str, Axis),
    axis: Axis,
    traces: &'a [Trace],
    peaks: &'a [(Slot, Axis, Vec<Peak>)],
    markers: &'a [f64],
    log: bool,
    low: bool,
    y: (f64, f64),
    db: bool,
    size: egui::Vec2,
    last: bool,
}

/// What a plot of the grid reports back.
struct Drawn {
    frame: egui::Rect,
    x_bounds: (f64, f64),
    clicked: Option<f64>,
}

fn column_plot(c: ColumnPlot<'_, '_>) -> Drawn {
    let tx = |f: f64| if c.log { f.max(LOG_MIN_HZ).log10() } else { f };
    let fx = move |x: f64| if c.log { 10f64.powf(x) } else { x };
    let nyq = c
        .traces
        .iter()
        .filter_map(|t| t.x.last().copied())
        .fold(0.0_f64, f64::max);
    let (x0, x1) = if c.low {
        (0.0, LOW_MAX_HZ)
    } else if c.log {
        (tx(LOG_MIN_HZ), tx(nyq.max(10.0)))
    } else {
        (0.0, nyq.max(10.0))
    };
    let unit = if c.db { "dB" } else { "power / Hz" };
    let mut plot = Plot::new(c.id)
        .width(c.size.x)
        .height(c.size.y)
        .show_x(true)
        .allow_scroll(false)
        .link_cursor(egui::Id::unique(c.id.0), [true, false])
        .link_axis(egui::Id::unique(c.id.0), [true, false])
        .label_formatter(move |pos| {
            let p = match pos {
                HoverPosition::NearDataPoint { position, .. }
                | HoverPosition::Elsewhere { position } => position,
            };
            Some(format!("{}\n{:.1} {unit}", hz(fx(p.x)), p.y))
        });
    if !c.low {
        plot = plot.y_axis_label(format!("{} ({unit})", c.axis.name()));
    }
    if c.last {
        plot = plot.x_axis_label(if c.log {
            tr("frequency (Hz, log)")
        } else {
            tr("frequency (Hz)")
        });
    }
    if c.log {
        plot = plot
            .x_grid_spacer(log_grid)
            .x_axis_formatter(|m, _| format!("{:.0}", 10f64.powf(m.value)));
    }
    let mut clicked = None;
    let mut frame = egui::Rect::NOTHING;
    let mut bounds = (x0, x1);
    plot.show(c.ui, |pu| {
        pu.set_plot_bounds_y(c.y.0..=c.y.1);
        if pu.response().double_clicked() || !pu.plot_bounds().is_valid_x() {
            pu.set_plot_bounds_x(x0..=x1);
        }
        for t in c.traces {
            let pts: Vec<[f64; 2]> =
                t.x.iter()
                    .zip(&t.y)
                    .filter(|(f, _)| !c.log || **f >= LOG_MIN_HZ)
                    .map(|(f, y)| [tx(*f), *y])
                    .collect();
            let colour = slot_color(t.slot);
            let mut line = Line::new("", PlotPoints::from(pts)).color(colour);
            line = if t.prefilt {
                line.style(LineStyle::dotted_dense()).width(1.2)
            } else {
                line.width(1.6)
            };
            pu.line(line);
        }
        for (slot, axis, pk) in c.peaks {
            if *axis != c.axis {
                continue;
            }
            let colour = slot_color(*slot);
            let trace = c.traces.iter().find(|t| t.slot == *slot && !t.prefilt);
            for peak in pk {
                let y = trace
                    .and_then(|t| t.y.get(peak.index).copied())
                    .unwrap_or(peak.y)
                    .clamp(c.y.0, c.y.1);
                pu.vline(
                    VLine::new("", tx(peak.x))
                        .color(colour.gamma_multiply(0.5))
                        .style(LineStyle::dashed_loose())
                        .width(1.0),
                );
                pu.text(
                    Text::new("", PlotPoint::new(tx(peak.x), y), hz(peak.x))
                        .color(colour)
                        .anchor(egui::Align2::LEFT_BOTTOM),
                );
            }
        }
        for m in c.markers {
            if c.low && *m > LOW_MAX_HZ {
                continue;
            }
            pu.vline(VLine::new("", tx(*m)).color(ACCENT).width(1.5));
        }
        if pu.response().clicked()
            && let Some(p) = pu.pointer_coordinate()
        {
            clicked = Some(fx(p.x));
        }
        let b = pu.plot_bounds();
        bounds = (fx(b.min()[0]), fx(b.max()[0]));
        frame = *pu.transform().frame();
    });
    Drawn {
        frame,
        x_bounds: bounds,
        clicked,
    }
}

/// The panel of the Spectrum tab.
fn controls(app: &mut App, ui: &mut Ui) {
    ui.label(RichText::new(tr("Data")).strong());
    let l = ui.label(tr("channel"));
    let mut q = Quantity::of(&app.tools.spectrum_channel);
    egui::ComboBox::from_id_salt("spectrum-quantity")
        .selected_text(q.name())
        .show_ui(ui, |ui| {
            for v in Quantity::ALL {
                ui.selectable_value(&mut q, v, v.name());
            }
        })
        .response
        .labelled_by(l.id);
    app.tools.spectrum_channel = q.channel(Axis::Roll);
    let d = &mut app.tools.spectrum_display;
    ui.add_enabled(
        q.prefilt(Axis::Roll).is_some(),
        egui::Checkbox::new(&mut d.prefilt, tr("prefilt (dotted)")),
    )
    .on_hover_text(tr("the unfiltered gyro beside the filtered one"));
    let l = ui.label(tr("smoothing"));
    egui::ComboBox::from_id_salt("spectrum-smoothing")
        .selected_text(tr(SMOOTHING[d.smoothing.min(SMOOTHING.len() - 1)].0))
        .show_ui(ui, |ui| {
            for (i, (name, w)) in SMOOTHING.iter().enumerate() {
                ui.selectable_value(&mut d.smoothing, i, format!("{} ({w:.0} Hz)", tr(name)));
            }
        })
        .response
        .labelled_by(l.id);
    ui.separator();
    ui.label(RichText::new(tr("Scale")).strong());
    ui.checkbox(&mut app.tools.psd_db, "dB");
    ui.checkbox(&mut d.log_freq, tr("log frequency"))
        .on_hover_text(tr("a logarithmic frequency axis on the full spectrum"));
    optional_bound(ui, "Y min", &mut d.y_min);
    optional_bound(ui, "Y max", &mut d.y_max);
    ui.separator();
    ui.label(RichText::new(tr("Peaks")).strong());
    ui.horizontal(|ui| {
        let l = ui.label(tr("peaks"));
        ui.add(egui::DragValue::new(&mut app.spectrum.auto_peaks).range(0..=10))
            .labelled_by(l.id);
    });
    ui.horizontal(|ui| {
        let l = ui.label(tr("prominence"));
        ui.add(
            egui::DragValue::new(&mut app.spectrum.min_prominence_db)
                .speed(0.5)
                .range(0.0..=60.0)
                .suffix(" dB"),
        )
        .labelled_by(l.id);
    });
    for line in readout(&app.spectrum.markers, &app.spectrum.peaks) {
        ui.add(egui::Label::new(RichText::new(line).small()).wrap());
    }
    if !app.spectrum.markers.is_empty() && ui.button(tr("Clear markers")).clicked() {
        app.spectrum.markers.clear();
    }
    ui.label(
        RichText::new(tr("click a plot to place a marker"))
            .small()
            .color(crate::colormap::MUTED),
    );
    ui.separator();
    ui.label(RichText::new(tr("Analysis")).strong());
    ui.horizontal(|ui| {
        let l = ui.label(tr("segment"));
        ui.add(
            egui::DragValue::new(&mut app.tools.psd.segment_s)
                .speed(0.01)
                .range(0.02..=2.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
    });
}

/// A power-axis end: "auto" until a value is set; the button clears it.
fn optional_bound(ui: &mut Ui, label: &'static str, value: &mut Option<f64>) {
    ui.horizontal(|ui| {
        let l = ui.label(tr(label));
        match value {
            Some(v) => {
                ui.add(egui::DragValue::new(v).speed(1.0)).labelled_by(l.id);
                if ui
                    .small_button(tr("auto"))
                    .on_hover_text(tr("back to the automatic range"))
                    .clicked()
                {
                    *value = None;
                }
            }
            None => {
                if ui
                    .button(tr("auto"))
                    .on_hover_text(tr("click to fix this end"))
                    .labelled_by(l.id)
                    .clicked()
                {
                    *value = Some(if label == "Y min" { -50.0 } else { 20.0 });
                }
            }
        }
    });
}

/// The marker list (with the delta of the last two) and each log's top peak
/// per axis.
fn readout(markers: &[f64], peaks: &[(Slot, Axis, Vec<Peak>)]) -> Vec<String> {
    let mut out = Vec::new();
    if !markers.is_empty() {
        let list: Vec<String> = markers.iter().map(|m| hz(*m)).collect();
        let delta = if let [.., a, b] = markers {
            format!("  Δ {}", hz((b - a).abs()))
        } else {
            String::new()
        };
        out.push(format!("{}: {}{delta}", tr("markers"), list.join(", ")));
    }
    let mut slots: Vec<Slot> = Vec::new();
    for (slot, _, _) in peaks {
        if !slots.contains(slot) {
            slots.push(*slot);
        }
    }
    for slot in slots {
        let tops: Vec<String> = peaks
            .iter()
            .filter(|p| p.0 == slot)
            .filter_map(|(_, axis, pk)| {
                pk.first()
                    .map(|t| format!("{} {}", axis.name()[..1].to_uppercase(), hz(t.x)))
            })
            .collect();
        if !tops.is_empty() {
            out.push(format!(
                "{} {}: {}",
                slot.name(),
                tr("top peak"),
                tops.join(" · ")
            ));
        }
    }
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn smoothing_averages_over_the_width() {
        let f: Vec<f64> = (0..10).map(f64::from).collect();
        let p = vec![0.0, 0.0, 0.0, 0.0, 9.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(smooth_power(&f, &p, 0.5), p, "below one bin: unchanged");
        let s = smooth_power(&f, &p, 2.0);
        assert_eq!(s[3], 3.0);
        assert_eq!(s[4], 3.0);
        assert_eq!(s[5], 3.0);
        assert_eq!(s[2], 0.0);
        assert!((s.iter().sum::<f64>() - 9.0).abs() < 1e-9, "power is kept");
    }

    #[test]
    fn quantities_map_to_channels_and_back() {
        for q in Quantity::ALL {
            assert_eq!(Quantity::of(&q.channel(Axis::Pitch)), q);
        }
        assert!(Quantity::Gyro.prefilt(Axis::Yaw).is_some());
        assert!(Quantity::DTerm.prefilt(Axis::Yaw).is_none());
        assert_eq!(Quantity::of(&ChannelId::Throttle), Quantity::Gyro);
    }

    #[test]
    fn log_grid_marks_one_two_five() {
        let marks = log_grid(GridInput {
            bounds: (f64::log10(2.0), f64::log10(500.0)),
            base_step_size: 0.1,
        });
        let hzs: Vec<f64> = marks
            .iter()
            .map(|m| (10f64.powf(m.value)).round())
            .collect();
        assert_eq!(hzs, vec![2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0]);
    }
}
