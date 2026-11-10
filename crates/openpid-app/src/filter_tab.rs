//! Filter Sim in the panel layout (docs/design/filter-tab, S2): on the left
//! the filter chain, part by part, at a typical moment of the flight
//! (magnitude, delay and phase against frequency, as the reference tool
//! draws its filters); on the right the docked log's unfiltered gyro through
//! that chain against the filtered gyro it logged. Every control, the
//! what-if values of each filter included, is in the panel on the right.

use crate::app::{App, Loaded};
use crate::axis_layout::control_panel;
use crate::colormap::{ACCENT, MUTED};
use crate::i18n::tr;
use crate::panel;
use crate::prefs::PlotColours;
use crate::views::{decimate, raw_gyro_fix, show_error, slot_color};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Line, LineStyle, Plot, PlotPoints};
use openpid_analysis::{FilterOverrides, FilterSimOutput, PartResponse};
use openpid_core::settings::LowpassKind;
use openpid_core::{Axis, ChannelId, GyroStage};
use openpid_sim::{ChainModel, LowpassModel};
use std::sync::Arc;

/// Width of the y-axis labels, the same on the plots of one column.
const Y_AXIS_W: f32 = 52.0;

/// Low-pass topologies of the what-if editor.
const LOWPASS_KINDS: [(LowpassKind, &str); 4] = [
    (LowpassKind::Pt1, "PT1"),
    (LowpassKind::Biquad, "biquad"),
    (LowpassKind::Pt2, "PT2"),
    (LowpassKind::Pt3, "PT3"),
];

/// Colour of part `i` of the chain.
fn part_colour(i: usize) -> Color32 {
    let p: &[Color32] = match crate::colormap::scheme() {
        PlotColours::Explorer => &crate::colormap::EXPLORER_PALETTE,
        PlotColours::ColourBlindSafe => crate::colormap::slot_palette(),
    };
    // skip the palette's red, kept for the log's own lines
    p[(i + 1) % p.len()]
}

/// Colour of the whole chain: the text colour.
fn total_colour() -> Color32 {
    if crate::theme::is_dark() {
        Color32::from_gray(235)
    } else {
        Color32::from_gray(30)
    }
}

/// The tab.
pub fn filter_sim(app: &mut App, ui: &mut Ui) {
    let output = docked_output(app);
    let seed = match output.as_ref().map(|(_, o)| o.as_ref()) {
        Some(Ok(o)) => match o.as_ref() {
            Output::FilterSim(f) => Some(f.model.clone()),
            _ => None,
        },
        _ => None,
    };
    let fit_source = match output.as_ref().map(|(_, o)| o) {
        Some(Ok(o)) => Some(Arc::clone(o)),
        _ => None,
    };
    control_panel(app, ui, "filter-panel", |app, ui| {
        controls(app, ui, seed.as_ref(), fit_source.as_deref());
    });
    ui.separator();
    let Some((slot, result)) = output else {
        if let Some(slot) = app.dock_slot()
            && app.loaded(slot).is_some()
        {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    tr("no unfiltered gyro in this log"),
                    raw_gyro_fix(app, slot)
                ))
                .color(MUTED),
            );
        }
        return;
    };
    let o = match result {
        Ok(o) => o,
        Err(Some(e)) => {
            show_error(ui, slot, &e);
            return;
        }
        Err(None) => {
            ui.spinner();
            return;
        }
    };
    let Output::FilterSim(f) = o.as_ref() else {
        return;
    };
    let gap = 10.0;
    let left_w = ((ui.available_width() - gap) * 0.5).floor();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.vertical(|ui| {
            ui.set_width(left_w);
            chain_column(ui, f);
        });
        ui.vertical(|ui| log_column(app, ui, slot, f));
    });
}

/// The docked log's result: `Err(None)` while computing; `None` when the
/// log has no unfiltered gyro on the axis (no job).
type DockedResult = Option<(Slot, Result<Arc<Output>, Option<String>>)>;

fn docked_output(app: &mut App) -> DockedResult {
    app.tools.filter_sim.axis = app.tools.axis;
    let slot = app.dock_slot()?;
    let axis = app.tools.axis;
    let has_raw = app.loaded(slot).and_then(Loaded::current).is_some_and(|s| {
        s.channel(&ChannelId::Gyro {
            axis,
            stage: GyroStage::Raw,
        })
        .is_some()
    });
    if !has_raw {
        return None;
    }
    let r = match app.result(slot, Kind::FilterSim(app.tools.filter_sim.clone())) {
        Some(Ok(o)) => Ok(o),
        Some(Err(e)) => Err(Some(e)),
        None => Err(None),
    };
    Some((slot, r))
}

/// Points of one response curve.
fn curve(freq: &[f64], ys: &[f64]) -> PlotPoints<'static> {
    PlotPoints::from(
        freq.iter()
            .zip(ys)
            .map(|(x, y)| [*x, *y])
            .collect::<Vec<[f64; 2]>>(),
    )
}

/// Which of a part's curves a plot shows.
#[derive(Clone, Copy)]
enum Curve {
    Magnitude,
    Delay,
    Phase,
}

impl Curve {
    fn of(self, p: &PartResponse) -> &[f64] {
        match self {
            Curve::Magnitude => &p.magnitude_db,
            Curve::Delay => &p.delay_ms,
            Curve::Phase => &p.phase_deg,
        }
    }
}

/// The chain, part by part: magnitude, delay and phase.
fn chain_column(ui: &mut Ui, f: &FilterSimOutput) {
    let r = &f.response;
    let mut at = Vec::new();
    if let Some(t) = r.throttle {
        at.push(format!("{} {:.0} %", tr("throttle"), t * 100.0));
    }
    if !r.motor_hz.is_empty() {
        let mean = r.motor_hz.iter().sum::<f64>() / r.motor_hz.len() as f64;
        at.push(format!("{} {mean:.0} Hz", tr("motors")));
    }
    ui.label(RichText::new(tr("THE FILTER CHAIN")).strong().size(12.0))
        .on_hover_text(tr(
            "the response of each filter at a typical moment of the flight: the mean throttle, the mean motor speeds, the median dynamic-notch centres",
        ));
    ui.label(
        RichText::new(format!(
            "{} {} · {} {:.2} ms",
            tr("at"),
            if at.is_empty() {
                tr("rest").to_string()
            } else {
                at.join(", ")
            },
            tr("delay at 100 Hz"),
            r.delay_100hz_ms
        ))
        .color(MUTED),
    );
    // the parts' names in their colours: the legend
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(RichText::new(tr("total")).color(total_colour()).strong());
        for (i, p) in r.parts.iter().enumerate() {
            ui.label(RichText::new(&p.label).color(part_colour(i)).size(12.0));
        }
        if f.response_as_flown.is_some() {
            ui.label(
                RichText::new(tr("as flown (dashed)"))
                    .color(MUTED)
                    .size(12.0),
            );
        }
    });
    let h = ((ui.available_height() - 2.0 * ui.spacing().item_spacing.y - 20.0) / 3.0).max(60.0);
    let link = egui::Id::unique("fsim-chain-x");
    let rows = [
        (
            Curve::Magnitude,
            "chain-mag",
            tr("magnitude (dB)"),
            Some((-40.0, 3.0)),
        ),
        (Curve::Delay, "chain-delay", tr("delay (ms)"), None),
        (Curve::Phase, "chain-phase", tr("phase (deg)"), None),
    ];
    let last = rows.len() - 1;
    for (i, (which, id, label, y)) in rows.into_iter().enumerate() {
        let mut plot = Plot::new(id)
            .height(h + if i == last { 20.0 } else { 0.0 })
            .y_axis_min_width(Y_AXIS_W)
            .show_axes([i == last, true])
            .link_axis(link, [true, false])
            .link_cursor(link, [true, false])
            .x_grid_spacer(crate::grid::spacer_x())
            .x_axis_formatter(crate::grid::label)
            .y_grid_spacer(crate::grid::spacer_y())
            .y_axis_formatter(crate::grid::label)
            .y_axis_label(label)
            .allow_scroll(false)
            .allow_drag([true, false])
            .allow_zoom([true, false]);
        if i == last {
            plot = plot.x_axis_label(tr("frequency (Hz)"));
        }
        if let Some((lo, hi)) = y {
            plot = plot.default_y_bounds(lo, hi);
        }
        plot.show(ui, |pu| {
            for (k, p) in r.parts.iter().enumerate() {
                pu.line(
                    Line::new(p.label.clone(), curve(&r.freq, which.of(p)))
                        .color(part_colour(k))
                        .width(1.2),
                );
            }
            if let Some(a) = &f.response_as_flown {
                pu.line(
                    Line::new(tr("total as flown"), curve(&r.freq, which.of(a)))
                        .color(MUTED)
                        .style(LineStyle::dashed_loose())
                        .width(1.4),
                );
            }
            pu.line(
                Line::new(tr("total"), curve(&r.freq, which.of(&r.total)))
                    .color(total_colour())
                    .width(2.0),
            );
        });
    }
}

/// The log: spectra and time traces of the unfiltered, logged and simulated gyro.
fn log_column(app: &App, ui: &mut Ui, slot: Slot, f: &FilterSimOutput) {
    let colour = slot_color(slot);
    let whatif = f.whatif != f.model;
    ui.label(
        RichText::new(format!("{} · {}", tr("ON THE LOG"), axis_name(f.axis)))
            .strong()
            .size(12.0),
    )
    .on_hover_text(tr(
        "the log's unfiltered gyro through the modelled chain (sim) against the filtered gyro the flight controller logged",
    ));
    let fit = match &f.fit {
        Some(fit) => format!(
            "fit: residual {:.0} % of the filtered RMS, delay {:+.1} ms",
            fit.residual_rel * 100.0,
            fit.delay_ms
        ),
        None => tr("no gyro.filtered in the log to validate against").to_string(),
    };
    ui.label(RichText::new(fit).color(MUTED));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(RichText::new(tr("raw")).color(colour.gamma_multiply(0.45)));
        if f.logged.is_some() {
            ui.label(RichText::new(tr("logged filtered")).color(colour));
        }
        ui.label(RichText::new(tr("sim")).color(ACCENT));
        if whatif {
            ui.label(RichText::new(tr("sim as flown (dashed)")).color(colour));
        }
    });
    let h = ui.available_height();
    let spectrum_h = (h * 0.55).max(80.0);
    Plot::new("fsim-psd")
        .height(spectrum_h)
        .y_axis_min_width(Y_AXIS_W)
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .x_axis_label(tr("frequency (Hz)"))
        .y_axis_label("dB")
        .show(ui, |pu| {
            let line = |name: &str, ys: &[f64]| Line::new(name.to_string(), curve(&f.freq, ys));
            pu.line(
                line(tr("raw"), &f.psd_raw_db)
                    .color(colour.gamma_multiply(0.45))
                    .width(1.0),
            );
            if let Some(l) = &f.psd_logged_db {
                pu.line(line(tr("logged filtered"), l).color(colour).width(1.5));
            }
            if whatif {
                pu.line(
                    line(tr("sim as flown"), &f.psd_as_flown_db)
                        .color(colour)
                        .style(LineStyle::dashed_loose())
                        .width(1.0),
                );
            }
            pu.line(line(tr("sim"), &f.psd_sim_db).color(ACCENT).width(1.5));
        });
    let px = ui.available_width().max(100.0) as usize;
    let rate = crate::views::display_unit(&openpid_core::units::Unit::DegreesPerSecond, &app.prefs);
    Plot::new("fsim-time")
        .height((ui.available_height() - 4.0).max(60.0))
        .y_axis_min_width(Y_AXIS_W)
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .x_axis_label(tr("time (s)"))
        .y_axis_label(rate.1.clone())
        .allow_drag([true, false])
        .allow_zoom([true, false])
        .show(ui, |pu| {
            let series: Vec<(&str, &[f64], Color32, f32)> = [
                Some((
                    tr("raw"),
                    f.raw.as_slice(),
                    colour.gamma_multiply(0.45),
                    1.0,
                )),
                f.logged
                    .as_deref()
                    .map(|l| (tr("logged filtered"), l, colour, 1.2)),
                Some((tr("sim"), f.sim.as_slice(), ACCENT, 1.2)),
            ]
            .into_iter()
            .flatten()
            .collect();
            for (name, ys, c, w) in series {
                let mut pts = decimate(&f.time_s, ys, |_| true, px * 2);
                for p in &mut pts {
                    p[1] *= rate.0;
                }
                pu.line(
                    Line::new(name.to_string(), PlotPoints::from(pts))
                        .color(c)
                        .width(w),
                );
            }
        });
}

fn axis_name(axis: Axis) -> &'static str {
    axis.name()
}

// ------------------------------------------------------------------- panel

/// The panel: axis, the what-if value of every filter, the fit, the analysis.
fn controls(app: &mut App, ui: &mut Ui, seed: Option<&ChainModel>, out: Option<&Output>) {
    panel::section(ui, tr("Data"));
    let axes: Vec<(Axis, String)> = Axis::ALL
        .iter()
        .map(|a| (*a, a.name().to_string()))
        .collect();
    panel::choice(
        ui,
        "fsim-axis",
        tr("Axis"),
        tr("the axis whose gyro runs through the chain"),
        &mut app.tools.axis,
        &axes,
    );
    app.tools.filter_sim.axis = app.tools.axis;
    if !app.tools.filter_sim.overrides.is_empty()
        && ui
            .button(tr("Reset to log"))
            .on_hover_text(tr("back to the filters the log flew with"))
            .clicked()
    {
        app.tools.filter_sim.overrides = FilterOverrides::default();
    }
    whatif_rows(ui, &mut app.tools.filter_sim.overrides, seed);
    if let Some(Output::FilterSim(f)) = out {
        fit_rows(ui, f);
    }
    panel::section(ui, tr("Analysis"));
    panel::drag(
        ui,
        tr("Segment"),
        tr(
            "length of each spectrum segment: longer gives finer frequency steps and fewer averages",
        ),
        &mut app.tools.filter_sim.segment_s,
        0.01,
        0.02..=2.0,
        "s",
    );
    panel::optional(
        ui,
        tr("Max frequency"),
        tr("the top of the frequency axes; auto: half the logging rate"),
        &mut app.tools.filter_sim.max_hz,
        500.0,
        "Hz",
    );
}

/// The model's fit to the logged filtered gyro, and the chain as described.
fn fit_rows(ui: &mut Ui, f: &FilterSimOutput) {
    panel::section(ui, tr("Model"));
    for line in &f.description {
        ui.label(RichText::new(line).color(MUTED).size(12.0));
    }
    let read = |ui: &mut Ui, label: &str, hint: &str, value: String| {
        panel::row(ui, label, hint, |ui| ui.label(value));
    };
    if let Some(fit) = &f.fit {
        read(
            ui,
            tr("Residual"),
            tr(
                "RMS of sim minus logged filtered gyro, relative to the logged RMS: how well the model matches the flight controller",
            ),
            format!("{:.0} %", fit.residual_rel * 100.0),
        );
        read(
            ui,
            tr("Unfiltered"),
            tr("the same for the unfiltered gyro: what the filters remove"),
            format!("{:.0} %", fit.unfiltered_rel * 100.0),
        );
        read(
            ui,
            tr("Delay"),
            tr("lag of the sim behind the logged gyro; negative: the sim leads"),
            format!("{:+.1} ms", fit.delay_ms),
        );
    }
    read(
        ui,
        tr("Throttle"),
        tr("whether the logged throttle moved the dynamic low-pass"),
        if f.throttle_driven {
            tr("drives LPF1").to_string()
        } else {
            tr("not logged").to_string()
        },
    );
    read(
        ui,
        tr("Motors"),
        tr("motors whose logged speed moved the RPM notches"),
        f.motors.to_string(),
    );
}

/// A what-if number: the log's value until edited, then the override (the
/// label turns accent-coloured).
#[allow(clippy::too_many_arguments, reason = "one row, eight knobs")]
fn whatif_value(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut Option<f64>,
    log_value: f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &str,
) {
    let text = RichText::new(label).color(if value.is_some() {
        ACCENT
    } else {
        ui.visuals().text_color()
    });
    let mut v = value.unwrap_or(log_value);
    let hint = format!("{hint} ({} {log_value})", tr("log:"));
    if panel::drag_value(
        ui,
        text,
        &hint,
        egui::DragValue::new(&mut v).speed(speed).range(range),
        unit,
    )
    .changed()
    {
        *value = Some(v);
    }
}

/// A what-if count (harmonics, notches).
fn whatif_count(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut Option<u32>,
    log_value: u32,
    max: u32,
) {
    let mut v = value.map(f64::from);
    whatif_value(
        ui,
        label,
        hint,
        &mut v,
        f64::from(log_value),
        1.0,
        0.0..=f64::from(max),
        "",
    );
    *value = v.map(|v| v.round() as u32);
}

/// A what-if low-pass topology.
fn whatif_kind(
    ui: &mut Ui,
    id: &str,
    value: &mut Option<LowpassKind>,
    log_kind: Option<&LowpassKind>,
) {
    let current = value.clone().or_else(|| log_kind.cloned());
    let mut index = current
        .as_ref()
        .and_then(|k| LOWPASS_KINDS.iter().position(|(kind, _)| kind == k))
        .unwrap_or(LOWPASS_KINDS.len());
    let mut options: Vec<(usize, String)> = LOWPASS_KINDS
        .iter()
        .enumerate()
        .map(|(i, (_, n))| (i, (*n).to_string()))
        .collect();
    if index == LOWPASS_KINDS.len() {
        options.push((index, "—".to_string()));
    }
    let before = index;
    panel::choice(
        ui,
        id,
        tr("Type"),
        tr(
            "the low-pass topology: PT1 is gentle and quick; PT2, PT3 and biquad cut harder and delay more",
        ),
        &mut index,
        &options,
    );
    if index != before
        && let Some((kind, _)) = LOWPASS_KINDS.get(index)
    {
        *value = Some(kind.clone());
    }
}

/// Topology, static cutoff and dynamic range of a low-pass as flown (zeros when absent).
fn lowpass_seed(l: Option<&LowpassModel>) -> (Option<LowpassKind>, f64, f64, f64) {
    match l {
        Some(l) => (
            Some(l.kind.clone()),
            l.cutoff_hz,
            l.dynamic.as_ref().map_or(0.0, |d| d.min_hz),
            l.dynamic.as_ref().map_or(0.0, |d| d.max_hz),
        ),
        None => (None, 0.0, 0.0, 0.0),
    }
}

/// The what-if editor, seeded with the chain as flown.
fn whatif_rows(ui: &mut Ui, o: &mut FilterOverrides, seed: Option<&ChainModel>) {
    let (k1, hz1, dmin, dmax) = lowpass_seed(seed.and_then(|m| m.lowpass1.as_ref()));
    let (k2, hz2, _, _) = lowpass_seed(seed.and_then(|m| m.lowpass2.as_ref()));
    let rpm = seed.and_then(|m| m.rpm_notch.as_ref());
    let dn = seed.and_then(|m| m.dyn_notch.as_ref());
    let notch = seed.and_then(|m| m.notches.first());
    let cutoff_hint = tr("the cut-off frequency; 0 switches the filter off");
    panel::section(ui, tr("Low-pass 1"));
    whatif_kind(ui, "fsim-lpf1-kind", &mut o.lpf1_type, k1.as_ref());
    whatif_value(
        ui,
        tr("Cut-off"),
        cutoff_hint,
        &mut o.lpf1_hz,
        hz1,
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    let mut dyn_min = o.lpf1_dyn_hz.map(|d| d.0);
    let mut dyn_max = o.lpf1_dyn_hz.map(|d| d.1);
    whatif_value(
        ui,
        tr("Dyn min"),
        tr("the cut-off at zero throttle when the throttle moves it"),
        &mut dyn_min,
        dmin,
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    whatif_value(
        ui,
        tr("Dyn max"),
        tr("the cut-off at full throttle"),
        &mut dyn_max,
        dmax,
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    if dyn_min.is_some() || dyn_max.is_some() {
        o.lpf1_dyn_hz = Some((dyn_min.unwrap_or(dmin), dyn_max.unwrap_or(dmax)));
    }
    panel::section(ui, tr("Low-pass 2"));
    whatif_kind(ui, "fsim-lpf2-kind", &mut o.lpf2_type, k2.as_ref());
    whatif_value(
        ui,
        tr("Cut-off"),
        cutoff_hint,
        &mut o.lpf2_hz,
        hz2,
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    panel::section(ui, tr("RPM notch"));
    whatif_count(
        ui,
        tr("Harmonics"),
        tr("notches per motor, at the motor frequency and its multiples; 0 switches them off"),
        &mut o.rpm_harmonics,
        rpm.map_or(0, |r| r.harmonics),
        10,
    );
    let q_hint = tr("the notch quality: higher is narrower, with less delay and less removed");
    whatif_value(
        ui,
        "Q",
        q_hint,
        &mut o.rpm_q,
        rpm.map_or(5.0, |r| r.q),
        0.1,
        0.1..=20.0,
        "",
    );
    whatif_value(
        ui,
        tr("Min"),
        tr("the lowest notch centre"),
        &mut o.rpm_min_hz,
        rpm.map_or(100.0, |r| r.min_hz),
        5.0,
        0.0..=500.0,
        "Hz",
    );
    panel::section(ui, tr("Dynamic notch"));
    whatif_count(
        ui,
        tr("Count"),
        tr("notches that follow the strongest peaks of the gyro; 0 switches them off"),
        &mut o.dyn_count,
        dn.map_or(0, |d| d.count),
        8,
    );
    whatif_value(
        ui,
        "Q",
        q_hint,
        &mut o.dyn_q,
        dn.map_or(3.0, |d| d.q),
        0.1,
        0.1..=20.0,
        "",
    );
    whatif_value(
        ui,
        tr("Min"),
        tr("the lowest frequency the notches follow"),
        &mut o.dyn_min_hz,
        dn.map_or(150.0, |d| d.min_hz),
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    whatif_value(
        ui,
        tr("Max"),
        tr("the highest frequency the notches follow"),
        &mut o.dyn_max_hz,
        dn.map_or(600.0, |d| d.max_hz),
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    panel::section(ui, tr("Static notch"));
    whatif_value(
        ui,
        tr("Centre"),
        tr("the frequency the notch removes; 0 switches it off"),
        &mut o.notch_hz,
        notch.map_or(0.0, |n| n.center_hz),
        5.0,
        0.0..=1000.0,
        "Hz",
    );
    whatif_value(
        ui,
        tr("Cut-off"),
        tr("the lower edge of the notch: closer to the centre is narrower"),
        &mut o.notch_cutoff_hz,
        notch.map_or(0.0, |n| n.cutoff_hz),
        5.0,
        0.0..=1000.0,
        "Hz",
    );
}
