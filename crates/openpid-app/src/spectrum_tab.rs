//! The Spectrum tab in the axis-row layout (docs/design/spectrum-tab, V1):
//! roll / pitch / yaw stacked, each with the full spectrum and the range
//! below 100 Hz side by side; per log the chosen quantity solid and, for the
//! gyro, the unfiltered gyro dotted. Every control is in the panel on the
//! right.

use crate::app::App;
use crate::axis_layout::{control_panel, row_height};
use crate::colormap::ACCENT;
use crate::i18n::tr;
use crate::panel;
use crate::views::{hz, nearest_bin, show_error, slot_color};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{
    GridInput, GridMark, HoverPosition, Line, LineStyle, Plot, PlotPoint, PlotPoints, Text, VLine,
};
use openpid_analysis::{CURVE_LEVELS, PsdMethod, PsdOutput, PsdParams};
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

/// Smoothing levels of the power density: the width (Hz) of the moving
/// average over the power. The curve has its own five levels
/// ([`CURVE_LEVELS`]), which are part of how it is computed.
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

/// What the rows are drawn with, read once per frame.
struct Setup {
    quantity: Quantity,
    display: SpectrumDisplay,
    db: bool,
    width_hz: f64,
    base: PsdParams,
}

/// The Spectrum tab.
pub fn spectrum(app: &mut App, ui: &mut Ui) {
    control_panel(app, ui, "spectrum-panel", controls);
    let full_w = ui.available_width();
    ui.separator();
    let display = app.tools.spectrum_display.clone();
    let setup = Setup {
        quantity: Quantity::of(&app.tools.spectrum_channel),
        // the curve comes smoothed; only the density is smoothed for display
        width_hz: match app.tools.psd.method {
            PsdMethod::Curve => 0.0,
            PsdMethod::Density => SMOOTHING[display.smoothing.min(SMOOTHING.len() - 1)].1,
        },
        display,
        db: app.tools.psd_db,
        base: app.tools.psd.clone(),
    };
    // the export and the project keep the channel of the selected axis
    app.tools.psd.channel = setup.quantity.channel(app.tools.axis);
    let rows: Vec<(Axis, AxisTraces)> = crate::axis_layout::shown_axes(app)
        .into_iter()
        .map(|axis| (axis, axis_traces(app, ui, &setup, axis)))
        .collect();
    let peaks = auto_peaks(app, &rows);
    let y = y_range(&rows, &setup.display, setup.db);
    legend(app, ui, setup.quantity, &setup.display, full_w);
    // the last row also carries the frequency label: every plot area the same height
    let n = rows.len().max(1);
    let h = row_height(ui, n, X_LABEL_H / n as f32);
    let markers = app.spectrum.markers.clone();
    let mut clicked_at: Option<f64> = None;
    for (i, (axis, t)) in rows.iter().enumerate() {
        let last = i == 2;
        let h = if last { h + X_LABEL_H } else { h };
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let wf = (full_w * FULL_SHARE - gap).max(200.0);
            let wl = (full_w - wf - gap).max(120.0);
            let full = column_plot(
                ui,
                ColumnPlot {
                    id: ("spec-full", *axis),
                    axis: *axis,
                    traces: &t.full,
                    peaks: &peaks,
                    markers: &markers,
                    log: setup.display.log_freq,
                    low: false,
                    y,
                    db: setup.db,
                    size: egui::vec2(wf, h),
                    last,
                },
            );
            app.probes
                .insert(format!("spectrum/{}", axis.name()), full.probe);
            let low = column_plot(
                ui,
                ColumnPlot {
                    id: ("spec-low", *axis),
                    axis: *axis,
                    traces: &t.low,
                    peaks: &[],
                    markers: &markers,
                    log: false,
                    low: true,
                    y,
                    db: setup.db,
                    size: egui::vec2(wl, h),
                    last,
                },
            );
            clicked_at = clicked_at.or(full.clicked).or(low.clicked);
        });
    }
    app.spectrum.peaks = peaks;
    if let Some(x) = clicked_at.filter(|x| *x >= 0.0) {
        let snapped = snap(&rows, x);
        app.spectrum.markers.push(snapped);
    }
}

/// Both columns of one axis for every visible log: the quantity and, when
/// shown and logged, its prefilt counterpart.
fn axis_traces(app: &mut App, ui: &mut Ui, s: &Setup, axis: Axis) -> AxisTraces {
    let mut t = AxisTraces::default();
    for slot in app.visible_slots() {
        let Some(session) = app.loaded(slot).and_then(|l| l.current().cloned()) else {
            continue;
        };
        let prefilt = s
            .quantity
            .prefilt(axis)
            .filter(|_| s.display.prefilt)
            .map(|p| (p, true));
        let channels = std::iter::once((s.quantity.channel(axis), false)).chain(prefilt);
        for (channel, prefilt) in channels.filter(|(c, _)| has(&session, c)) {
            let full = PsdParams {
                channel: channel.clone(),
                ..s.base.clone()
            };
            let low = PsdParams {
                channel,
                segment_s: s.base.segment_s.max(LOW_SEGMENT_S),
                max_hz: Some(LOW_MAX_HZ),
                ..s.base.clone()
            };
            for (params, into) in [(full, &mut t.full), (low, &mut t.low)] {
                match app.result(slot, Kind::Psd(params)) {
                    Some(Ok(o)) => into.extend(trace(slot, prefilt, o, s.db, s.width_hz)),
                    Some(Err(e)) => show_error(ui, slot, &e),
                    None => {}
                }
            }
        }
    }
    t
}

/// The automatic peaks of every solid full-range trace, from `PEAK_MIN_HZ` up.
fn auto_peaks(app: &App, rows: &[(Axis, AxisTraces)]) -> Vec<(Slot, Axis, Vec<Peak>)> {
    let n = app.spectrum.auto_peaks;
    let prominence = app.spectrum.min_prominence_db;
    let mut out = Vec::new();
    for (axis, t) in rows {
        for tr in t.full.iter().filter(|tr| !tr.prefilt) {
            if let Output::Psd(p) = tr.out.as_ref() {
                let mut pk: Vec<Peak> = p
                    .peaks(n + 8, prominence)
                    .into_iter()
                    .filter(|k| k.x >= PEAK_MIN_HZ)
                    .collect();
                pk.truncate(n);
                out.push((tr.slot, *axis, pk));
            }
        }
    }
    out
}

/// `x` snapped to the nearest bin of the first trace, so the read-out shows
/// a real value.
fn snap(rows: &[(Axis, AxisTraces)], x: f64) -> f64 {
    rows.first()
        .and_then(|(_, t)| t.full.first())
        .and_then(|tr| match tr.out.as_ref() {
            Output::Psd(p) => nearest_bin(&p.freq, x).map(|i| p.freq[i]),
            _ => None,
        })
        .unwrap_or(x)
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
    let y: Vec<f64> = if db {
        power
            .iter()
            .map(|v| 10.0 * v.max(f64::MIN_POSITIVE).log10())
            .collect()
    } else {
        power
    };
    thin(&p.freq, &y, MAX_DRAWN)
}

/// Most points a trace is drawn with. The curve of a long log at a high
/// rate has more than a million; a plot is a thousand pixels wide.
const MAX_DRAWN: usize = 8192;

/// At most about `max` points of a curve for drawing: the curve is cut into
/// equal runs and each run gives its lowest and its highest point, in the
/// order they come, so every peak and every dip stays on screen. A curve
/// that is short enough comes back as it is.
pub fn thin(x: &[f64], y: &[f64], max: usize) -> (Vec<f64>, Vec<f64>) {
    let n = x.len().min(y.len());
    if n <= max.max(4) {
        return (x[..n].to_vec(), y[..n].to_vec());
    }
    let run = n.div_ceil(max / 2);
    let mut out = (Vec::with_capacity(max + 2), Vec::with_capacity(max + 2));
    for start in (0..n).step_by(run) {
        let end = (start + run).min(n);
        let (mut lo, mut hi) = (start, start);
        for i in start..end {
            if y[i] < y[lo] {
                lo = i;
            }
            if y[i] > y[hi] {
                hi = i;
            }
        }
        let (first, second) = (lo.min(hi), lo.max(hi));
        out.0.push(x[first]);
        out.1.push(y[first]);
        if second != first {
            out.0.push(x[second]);
            out.1.push(y[second]);
        }
    }
    out
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
        // the density is of one-second segments there; the curve has none
        let low = match app.tools.psd.method {
            PsdMethod::Density => tr("below 100 Hz (1 s segments)"),
            PsdMethod::Curve => tr("below 100 Hz"),
        };
        ui.label(RichText::new(low).size(12.0).color(crate::colormap::MUTED));
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
struct ColumnPlot<'a> {
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
    /// Where the plot is and what it shows, in Hz.
    probe: crate::probe::PlotProbe,
    clicked: Option<f64>,
}

/// Frequency (Hz) to plot x and back: identity, or log10 on a log axis.
#[derive(Clone, Copy)]
struct FreqAxis {
    log: bool,
}

impl FreqAxis {
    fn to_x(self, f: f64) -> f64 {
        if self.log {
            f.max(LOG_MIN_HZ).log10()
        } else {
            f
        }
    }

    fn to_hz(self, x: f64) -> f64 {
        if self.log { 10f64.powf(x) } else { x }
    }
}

/// The plot of one cell with its labels, links and axis format.
fn cell_plot<'p>(c: &ColumnPlot<'_>, fa: FreqAxis) -> Plot<'p> {
    let unit = if c.db { "dB" } else { "power / Hz" };
    let mut plot = crate::grid::plot(c.id)
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
            Some(format!("{}\n{:.1} {unit}", hz(fa.to_hz(p.x)), p.y))
        });
    if !c.low {
        plot = plot.y_axis_label(format!("{} ({unit})", c.axis.name()));
    }
    if c.last {
        plot = plot.x_axis_label(if fa.log {
            tr("frequency (Hz, log)")
        } else {
            tr("frequency (Hz)")
        });
    }
    if fa.log {
        plot = plot
            .x_grid_spacer(log_grid)
            .x_axis_formatter(|m, _| format!("{:.0}", 10f64.powf(m.value)));
    }
    plot
}

/// The x range a cell resets to: 0–100 Hz, or 0 (2 Hz on a log axis) to Nyquist.
fn home_x(c: &ColumnPlot<'_>, fa: FreqAxis) -> (f64, f64) {
    let nyq = c
        .traces
        .iter()
        .filter_map(|t| t.x.last().copied())
        .fold(0.0_f64, f64::max)
        .max(10.0);
    if c.low {
        (0.0, LOW_MAX_HZ)
    } else if fa.log {
        (fa.to_x(LOG_MIN_HZ), fa.to_x(nyq))
    } else {
        (0.0, nyq)
    }
}

fn draw_traces(pu: &mut egui_plot::PlotUi<'_>, traces: &[Trace], fa: FreqAxis) {
    for t in traces {
        let pts: Vec<[f64; 2]> =
            t.x.iter()
                .zip(&t.y)
                .filter(|(f, _)| !fa.log || **f >= LOG_MIN_HZ)
                .map(|(f, y)| [fa.to_x(*f), *y])
                .collect();
        let line = Line::new("", PlotPoints::from(pts)).color(slot_color(t.slot));
        pu.line(if t.prefilt {
            line.style(LineStyle::dotted_dense()).width(1.2)
        } else {
            line.width(1.6)
        });
    }
}

/// Dashed lines and frequency labels on this axis's automatic peaks.
fn draw_peaks(pu: &mut egui_plot::PlotUi<'_>, c: &ColumnPlot<'_>, fa: FreqAxis) {
    for (slot, _, pk) in c.peaks.iter().filter(|p| p.1 == c.axis) {
        let colour = slot_color(*slot);
        let trace = c.traces.iter().find(|t| t.slot == *slot && !t.prefilt);
        for peak in pk {
            let y = trace
                .and_then(|t| t.y.get(peak.index).copied())
                .unwrap_or(peak.y)
                .clamp(c.y.0, c.y.1);
            pu.vline(
                VLine::new("", fa.to_x(peak.x))
                    .color(colour.gamma_multiply(0.5))
                    .style(LineStyle::dashed_loose())
                    .width(1.0),
            );
            pu.text(
                Text::new("", PlotPoint::new(fa.to_x(peak.x), y), hz(peak.x))
                    .color(colour)
                    .anchor(egui::Align2::LEFT_BOTTOM),
            );
        }
    }
}

fn column_plot(ui: &mut Ui, c: ColumnPlot<'_>) -> Drawn {
    let fa = FreqAxis {
        log: c.log && !c.low,
    };
    let (x0, x1) = home_x(&c, fa);
    let mut clicked = None;
    let mut probe = None;
    let plot = cell_plot(&c, fa);
    plot.show(ui, |pu| {
        pu.set_plot_bounds_y(c.y.0..=c.y.1);
        if pu.response().double_clicked() || !pu.plot_bounds().is_valid_x() {
            pu.set_plot_bounds_x(x0..=x1);
        }
        draw_traces(pu, c.traces, fa);
        draw_peaks(pu, &c, fa);
        for m in c.markers.iter().filter(|m| !c.low || **m <= LOW_MAX_HZ) {
            pu.vline(VLine::new("", fa.to_x(*m)).color(ACCENT).width(1.5));
        }
        if pu.response().clicked() {
            clicked = pu.pointer_coordinate().map(|p| fa.to_hz(p.x));
        }
        probe = Some(crate::probe::PlotProbe::of(pu.transform()));
    });
    let probe = probe.unwrap_or(crate::probe::PlotProbe::area(
        egui::Rect::NOTHING,
        (0.0, 1.0),
        (0.0, 1.0),
    ));
    Drawn {
        probe: if fa.log { probe.with_log_x() } else { probe },
        clicked,
    }
}

/// The panel of the Spectrum tab.
fn controls(app: &mut App, ui: &mut Ui) {
    crate::axis_layout::axes_row(app, ui);
    panel::section(ui, tr("Data"));
    let mut q = Quantity::of(&app.tools.spectrum_channel);
    let quantities: Vec<(Quantity, String)> = Quantity::ALL
        .iter()
        .map(|v| (*v, v.name().to_string()))
        .collect();
    panel::choice(
        ui,
        "spectrum-quantity",
        tr("Channel"),
        tr("what the rows show, on roll, pitch and yaw"),
        &mut q,
        &quantities,
    );
    app.tools.spectrum_channel = q.channel(Axis::Roll);
    let d = &mut app.tools.spectrum_display;
    panel::check_enabled(
        ui,
        q.prefilt(Axis::Roll).is_some(),
        &mut d.prefilt,
        tr("Prefilt (dotted)"),
        tr("the unfiltered gyro beside the filtered one; the gyro only"),
    );
    let methods = [
        (PsdMethod::Curve, tr("smoothed curve").to_string()),
        (PsdMethod::Density, tr("power density").to_string()),
    ];
    panel::choice(
        ui,
        "spectrum-method",
        tr("Spectrum"),
        tr(
            "smoothed curve: one transform of the whole log with its decibels smoothed, as the reference tool draws it; broad noise shows well, a steady tone almost vanishes. power density: averaged segments (Welch); noise reads its true density and a steady tone stands out",
        ),
        &mut app.tools.psd.method,
        &methods,
    );
    match app.tools.psd.method {
        PsdMethod::Curve => {
            let levels: Vec<(usize, String)> = CURVE_LEVELS
                .iter()
                .enumerate()
                .map(|(i, name)| (i, tr(name).to_string()))
                .collect();
            app.tools.psd.smoothing = app.tools.psd.smoothing.min(CURVE_LEVELS.len() - 1);
            panel::choice(
                ui,
                "spectrum-smoothing",
                tr("Smoothing"),
                tr(
                    "how wide the smoothing of the curve is, as in the reference tool; medium by default. The same level is a wider band for a shorter log",
                ),
                &mut app.tools.psd.smoothing,
                &levels,
            );
        }
        PsdMethod::Density => {
            let smoothing: Vec<(usize, String)> = SMOOTHING
                .iter()
                .enumerate()
                .map(|(i, (name, _))| (i, tr(name).to_string()))
                .collect();
            panel::choice(
                ui,
                "spectrum-smoothing",
                tr("Smoothing"),
                tr(
                    "a moving average of the power over 0, 2, 4, 8, 16 or 32 Hz; medium (8 Hz) by default",
                ),
                &mut d.smoothing,
                &smoothing,
            );
        }
    }
    panel::section(ui, tr("Scale"));
    panel::check(
        ui,
        &mut app.tools.psd_db,
        "dB",
        tr("power in decibels (10·log10) instead of unit²/Hz"),
    );
    panel::check(
        ui,
        &mut d.log_freq,
        tr("Log frequency"),
        tr("a logarithmic frequency axis on the full spectrum"),
    );
    panel::optional(
        ui,
        tr("Y min"),
        tr("lower end of the power axis; auto follows the drawn data"),
        &mut d.y_min,
        -50.0,
        if app.tools.psd_db { "dB" } else { "" },
    );
    panel::optional(
        ui,
        tr("Y max"),
        tr("upper end of the power axis; auto follows the drawn data"),
        &mut d.y_max,
        20.0,
        if app.tools.psd_db { "dB" } else { "" },
    );
    panel::section(ui, tr("Peaks"));
    panel::drag_usize(
        ui,
        tr("Count"),
        tr("how many automatic peaks to mark on each solid line, from 20 Hz up; 0 = none"),
        &mut app.spectrum.auto_peaks,
        0..=10,
    );
    panel::drag(
        ui,
        tr("Prominence"),
        tr("how far a peak must stand above its surroundings to be marked"),
        &mut app.spectrum.min_prominence_db,
        0.5,
        0.0..=60.0,
        " dB",
    );
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
    // the curve is one piece, the whole log: it has no segment to set
    if app.tools.psd.method == PsdMethod::Density {
        panel::section(ui, tr("Analysis"));
        panel::drag(
            ui,
            tr("Segment"),
            tr(
                "length of each averaged piece of the full spectrum: longer gives finer frequency steps and less averaging",
            ),
            &mut app.tools.psd.segment_s,
            0.01,
            0.02..=2.0,
            " s",
        );
    }
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
    fn a_long_curve_is_thinned_to_its_lows_and_highs() {
        let x: Vec<f64> = (0..100_000).map(f64::from).collect();
        let mut y: Vec<f64> = x.iter().map(|v| (v / 500.0).sin()).collect();
        y[40_123] = 9.0;
        y[77_777] = -9.0;
        let (tx, ty) = thin(&x, &y, 8192);
        assert!(tx.len() <= 8192 + 2 && tx.len() > 4000, "{}", tx.len());
        // in order, and the single spike and the single dip are kept
        assert!(tx.windows(2).all(|w| w[0] < w[1]));
        let at = |v: f64| tx.iter().position(|t| *t == v).map(|i| ty[i]);
        assert_eq!(at(40_123.0), Some(9.0));
        assert_eq!(at(77_777.0), Some(-9.0));
        // a short curve is not touched
        let (sx, sy) = thin(&x[..5000], &y[..5000], 8192);
        assert_eq!((sx.len(), sy.len()), (5000, 5000));
        assert_eq!(thin(&[], &[], 8192), (vec![], vec![]));
    }

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
