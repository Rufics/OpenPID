//! The axis-row layout of the analysis tabs (docs/design/axis-layout, R4):
//! roll / pitch / yaw stacked, every control in a collapsible panel on the
//! right. This module holds the shared pieces and the tabs moved to it.

use crate::app::App;
use crate::i18n::tr;
use crate::panel;
use crate::shortcuts::{self, Action};
use crate::views::{grid_heatmap, show_error, slot_color, verdict_color};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{HLine, Legend, Line, LineStyle, Plot, PlotPoints, Points};
use openpid_analysis::{FitnessLevel, StepParams, StepTargets, VerdictLevel, verdicts};
use openpid_core::Axis;
use openpid_core::settings::{PidGains, Settings};
use std::sync::Arc;

/// Width of the control panel (R4).
pub const PANEL_W: f32 = 236.0;
/// Width of the collapsed panel strip.
const COLLAPSED_W: f32 = 26.0;
/// Width of each of the Step tab's Peak and Latency plots.
const SIDE_PLOT_W: f32 = 120.0;

/// The tab's control panel on the right (R4): `body` draws the controls. Open
/// or collapsed follows `Prefs::panel_open`; the header button and Ctrl+.
/// toggle it.
pub fn control_panel(app: &mut App, ui: &mut Ui, id: &str, body: impl FnOnce(&mut App, &mut Ui)) {
    let ctx = ui.ctx().clone();
    if !app.prefs.panel_open {
        egui::Panel::right(format!("{id}-collapsed"))
            .resizable(false)
            .exact_size(COLLAPSED_W)
            .show(ui, |ui| {
                let r = ui.button("<").on_hover_text(shortcuts::hint(
                    &ctx,
                    tr("Show the control panel"),
                    Action::TogglePanel,
                ));
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        tr("Show the control panel"),
                    )
                });
                if r.clicked() {
                    app.prefs.panel_open = true;
                    app.save_prefs();
                }
            });
        return;
    }
    egui::Panel::right(id.to_string())
        .resizable(false)
        .exact_size(PANEL_W)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(tr("Control panel"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui.button(">").on_hover_text(shortcuts::hint(
                        &ctx,
                        tr("Hide the control panel"),
                        Action::TogglePanel,
                    ));
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Button,
                            true,
                            tr("Hide the control panel"),
                        )
                    });
                    if r.clicked() {
                        app.prefs.panel_open = false;
                        app.save_prefs();
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| body(app, ui));
        });
}

/// The axes the axis-row tabs show (docs/design/axis-focus): all three, or
/// the one chosen with "Show" in the panel or the keys R, P, Y.
pub fn shown_axes(app: &App) -> Vec<Axis> {
    app.tools
        .axis_focus
        .map_or_else(|| Axis::ALL.to_vec(), |a| vec![a])
}

/// Show one axis at full height (`Some`) or all three (`None`); the chosen
/// axis becomes the tabs' selected axis (export, project).
pub fn focus_axis(app: &mut App, axis: Option<Axis>) {
    app.tools.axis_focus = axis;
    if let Some(a) = axis {
        app.tools.axis = a;
        app.tools.step.axis = a;
    }
}

/// The "Show" row at the top of an axis-row tab's panel: all, roll, pitch, yaw.
pub fn axes_row(app: &mut App, ui: &mut Ui) {
    panel::section(ui, tr("Axes"));
    let current = app.tools.axis_focus;
    let mut chosen = current;
    let keys = shortcuts::keys_of(ui.ctx(), Action::FocusAxis(Axis::Roll));
    let hint = format!(
        "{} ({keys}, P, Y)",
        tr(
            "all three axes, or one at full height; the key of the shown axis brings all three back"
        )
    );
    // as a panel row, but each button keeps its own name for screen readers
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(w, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(egui::Label::new(tr("Show")).sense(egui::Sense::hover()))
                .on_hover_text(&hint);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // right to left: yaw first, "all" last
                let options = [
                    (Some(Axis::Yaw), "yaw"),
                    (Some(Axis::Pitch), "pitch"),
                    (Some(Axis::Roll), "roll"),
                    (None, tr("all")),
                ];
                for (value, name) in options {
                    if ui
                        .selectable_label(current == value, name)
                        .on_hover_text(&hint)
                        .clicked()
                    {
                        chosen = value;
                    }
                }
            });
        },
    );
    if chosen != current {
        focus_axis(app, chosen);
    }
}

/// Height of each of `n` stacked plot rows in the remaining space, `extra`
/// pixels per row taken by labels above the plot.
pub fn row_height(ui: &Ui, n: usize, extra: f32) -> f32 {
    let n = n.max(1) as f32;
    let gap = ui.spacing().item_spacing.y;
    ((ui.available_height() - gap * (n - 1.0)) / n - extra).max(60.0)
}

// ------------------------------------------------------------------- step

/// What the Latency plot of the Step tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LatencyShown {
    /// Time for the mean response to reach 50 %.
    #[default]
    HalfRise,
    /// Cross-correlation lag of setpoint and gyro.
    Xcorr,
}

/// Step tab display options (not analysis parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct StepDisplay {
    /// Upper end of the response axis.
    pub y_max: f64,
    /// Draw the histogram of the individual responses.
    pub raw: bool,
    /// What the Latency plot shows.
    pub latency: LatencyShown,
    /// The verdict table under the plots is open.
    pub details: bool,
}

impl Default for StepDisplay {
    fn default() -> Self {
        StepDisplay {
            y_max: 1.5,
            raw: false,
            latency: LatencyShown::HalfRise,
            details: false,
        }
    }
}

/// Smoothing levels, as the width of the moving average (ms).
const SMOOTHING: [(&str, f64); 4] = [
    ("off", 0.0),
    ("low", 10.0),
    ("medium", 20.0),
    ("high", 40.0),
];

fn step_controls(app: &mut App, ui: &mut Ui) {
    axes_row(app, ui);
    panel::section(ui, tr("Display"));
    let smoothing: Vec<(f64, String)> = SMOOTHING
        .iter()
        .map(|(name, ms)| (*ms, tr(name).to_string()))
        .collect();
    panel::choice(
        ui,
        "step-smoothing",
        tr("Smoothing"),
        tr(
            "a moving average over the mean response: off, 10, 20 or 40 ms; more smoothing hides noise, and a little of the overshoot",
        ),
        &mut app.tools.step.smooth_ms,
        &smoothing,
    );
    panel::choice(
        ui,
        "step-latency",
        tr("Latency"),
        tr(
            "what the Latency plots show: the time the mean response takes to reach 50 % (SR latency), or the delay from setpoint to gyro by cross-correlation",
        ),
        &mut app.tools.step_display.latency,
        &[
            (LatencyShown::HalfRise, tr("SR latency (50 %)").to_string()),
            (LatencyShown::Xcorr, tr("SP-gyro delay (xcorr)").to_string()),
        ],
    );
    panel::drag(
        ui,
        tr("Y max"),
        tr("upper end of the response axis; 1 is the setpoint"),
        &mut app.tools.step_display.y_max,
        0.05,
        1.1..=5.0,
        "",
    );
    panel::check(
        ui,
        &mut app.tools.step.y_offset,
        tr("Y correction"),
        tr("normalise every window on its settled tail before averaging"),
    );
    panel::check(
        ui,
        &mut app.tools.step_display.raw,
        tr("Raw responses"),
        tr("the individual responses behind the mean, as a density"),
    );
    panel::section(ui, tr("Analysis"));
    panel::drag(
        ui,
        tr("Window"),
        tr(
            "length of each piece of the log the response is computed on; longer windows see slow settling, shorter ones give more windows",
        ),
        &mut app.tools.step.window_s,
        0.1,
        0.5..=10.0,
        " s",
    );
    panel::drag(
        ui,
        tr("Cut-off"),
        tr("low-pass on setpoint and gyro before the deconvolution: noise above it is ignored"),
        &mut app.tools.step.cutoff_hz,
        1.0,
        5.0..=200.0,
        " Hz",
    );
    panel::drag(
        ui,
        tr("Min input"),
        tr(
            "windows whose stick input stays below this rate are skipped: too small a step to judge",
        ),
        &mut app.tools.step.min_input,
        5.0,
        0.0..=1000.0,
        " °/s",
    );
    panel::drag(
        ui,
        tr("High input"),
        tr("windows above this rate form the \"high input\" group, drawn apart from the low one"),
        &mut app.tools.step.high_input,
        10.0,
        50.0..=2000.0,
        " °/s",
    );
}

/// What a response plot draws: each log's mean, its PID label, and the
/// reason when no log has a response on the axis.
type Curves<'a> = (
    &'a [(Slot, Arc<Output>)],
    &'a [(Slot, Option<String>)],
    Option<&'a str>,
);

/// The result of one axis of one log.
type AxisResult = (Axis, Slot, Option<Result<Arc<Output>, String>>);

/// "45,80,30,40,120": P, I, D, D-max (or D-min), F of a log.
pub fn pid_text(settings: &Settings, g: &PidGains) -> String {
    let dm = g.d_max.or(g.d_min).unwrap_or(0.0);
    let f = g.f.unwrap_or(0.0);
    [g.p, g.i, g.d, dm, f]
        .map(|gain| settings.gain_text(gain))
        .join(",")
}

/// The Step tab (R4): the verdict table, then roll / pitch / yaw rows of the
/// response with its Peak and Latency plots.
pub fn step(app: &mut App, ui: &mut Ui) {
    // the panel first, so it takes the right edge of the tab at full height
    // before anything wider than the window can stretch the tab
    control_panel(app, ui, "step-panel", step_controls);
    let full_w = ui.available_width();
    ui.separator();
    let params = app.tools.step.clone();
    let targets = app.tools.recommend.targets;
    let display = app.tools.step_display.clone();
    // every axis of every visible slot: the table judges all three
    let mut rows: Vec<AxisResult> = Vec::new();
    for slot in app.visible_slots() {
        for axis in Axis::ALL {
            let p = StepParams {
                axis,
                ..params.clone()
            };
            rows.push((axis, slot, app.result(slot, Kind::Step(p))));
        }
    }
    details_panel(app, ui, &rows, &targets);
    let axes = shown_axes(app);
    let end_ms = params.response_s * 1000.0;
    let band = (
        targets.settling_ms_max.min(end_ms),
        1.0 - targets.final_tol,
        1.0 + targets.final_tol,
    );
    app.step_view.band = Some(band);
    let h = row_height(ui, axes.len(), 0.0);
    for axis in axes {
        let outputs: Vec<(Slot, Arc<Output>)> = rows
            .iter()
            .filter(|(a, _, _)| *a == axis)
            .filter_map(|(_, s, r)| match r {
                Some(Ok(o)) => Some((*s, o.clone())),
                _ => None,
            })
            .collect();
        let pids: Vec<(Slot, Option<String>)> = outputs
            .iter()
            .map(|(s, _)| {
                let pid = app.loaded(*s).and_then(|l| l.current()).and_then(|ses| {
                    ses.settings
                        .pid
                        .map(|p| pid_text(&ses.settings, p.get(axis)))
                });
                (*s, pid)
            })
            .collect();
        // an axis without a response says why inside its plot (e.g. an epoch
        // with no stick movement on it), instead of an empty frame
        let why: Vec<String> = rows
            .iter()
            .filter(|(a, _, _)| *a == axis)
            .filter_map(|(_, s, r)| match r {
                Some(Err(e)) => Some(format!("{}: {e}", s.name())),
                _ => None,
            })
            .collect();
        let note = (outputs.is_empty() && !why.is_empty()).then(|| why.join("\n"));
        ui.horizontal(|ui| {
            let w = (full_w - 2.0 * (SIDE_PLOT_W + ui.spacing().item_spacing.x)).max(200.0);
            response_plot(
                ui,
                axis,
                (&outputs, &pids, note.as_deref()),
                &display,
                &targets,
                band,
                end_ms,
                w,
                h,
            );
            peak_plot(ui, axis, &outputs, &display, w.min(SIDE_PLOT_W), h);
            latency_plot(ui, axis, &outputs, &display, h);
        });
    }
}

/// The verdicts of every visible log and axis, kept for the table, the
/// summary and the tests (`StepViewState::rows`).
fn judge(app: &mut App, rows: &[AxisResult], targets: &StepTargets) {
    app.step_view.rows.clear();
    for (axis, slot, result) in rows {
        if let Some(Ok(o)) = result
            && let Output::Step(st) = o.as_ref()
        {
            let v = verdicts(&st.metrics, st.windows_used, targets);
            app.step_view.rows.push((*slot, *axis, v));
        }
    }
}

/// The worst level of a log's verdicts as a log-check level, for its pill.
fn worst(levels: impl Iterator<Item = VerdictLevel>) -> FitnessLevel {
    levels.fold(FitnessLevel::Ok, |w, l| match (w, l) {
        (FitnessLevel::Fail, _) | (_, VerdictLevel::Fail) => FitnessLevel::Fail,
        (FitnessLevel::Warn, _) | (_, VerdictLevel::Warn) => FitnessLevel::Warn,
        _ => FitnessLevel::Ok,
    })
}

/// The details at the bottom of the Step tab (docs/design/axis-layout):
/// one line that is always shown (the "Details" toggle, a pill per log with
/// how many axes had enough data and its worst verdict, the Advice line)
/// and, when opened, the verdict table. Closed by default, so the table no
/// longer grows above the plots with every log.
fn details_panel(app: &mut App, ui: &mut Ui, rows: &[AxisResult], targets: &StepTargets) {
    judge(app, rows, targets);
    let max_table = ui.available_height() * 0.45;
    egui::Panel::bottom("step-details")
        .resizable(false)
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                details_toggle(app, ui);
                for slot in app.visible_slots() {
                    let mine: Vec<&AxisResult> = rows.iter().filter(|r| r.1 == slot).collect();
                    let judged: Vec<_> =
                        app.step_view.rows.iter().filter(|r| r.0 == slot).collect();
                    let level = if judged.is_empty() {
                        FitnessLevel::Fail
                    } else {
                        worst(judged.iter().flat_map(|r| r.2.iter().map(|v| v.level)))
                    };
                    let hover: Vec<String> = mine
                        .iter()
                        .map(|(axis, _, r)| {
                            let state = match r {
                                Some(Ok(_)) => tr("judged").to_string(),
                                Some(Err(e)) => e.clone(),
                                None => tr("running").to_string(),
                            };
                            format!("{} {}: {state}", slot.name(), axis.name())
                        })
                        .collect();
                    crate::views::pill(
                        ui,
                        &format!(
                            "{} {}/{} {}",
                            slot.name(),
                            judged.len(),
                            mine.len(),
                            tr("axes")
                        ),
                        level,
                    )
                    .on_hover_text(hover.join("\n"));
                }
                advice_line(app, ui);
            });
            if app.tools.step_display.details {
                // a bottom panel keeps last frame's height: ask for the
                // table's own height (header + one row per log and axis)
                let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                let want = ((rows.len() + 1) as f32 * row).min(max_table);
                egui::ScrollArea::vertical()
                    .id_salt("step-details-table")
                    .min_scrolled_height(want)
                    .max_height(max_table)
                    .show(ui, |ui| verdict_table(app, ui, rows));
            }
        });
}

/// The "Details" toggle: a painted disclosure triangle and the word.
fn details_toggle(app: &mut App, ui: &mut Ui) {
    let open = app.tools.step_display.details;
    let text = tr("Details");
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(13.0),
        ui.visuals().strong_text_color(),
    );
    let size = egui::vec2(galley.size().x + 30.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, text));
    let v = ui.visuals();
    let fill = if resp.hovered() {
        v.widgets.hovered.weak_bg_fill
    } else {
        v.widgets.inactive.weak_bg_fill
    };
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    let c = egui::pos2(rect.left() + 12.0, rect.center().y);
    let tri = if open {
        vec![
            c + egui::vec2(-4.0, -2.0),
            c + egui::vec2(4.0, -2.0),
            c + egui::vec2(0.0, 3.0),
        ]
    } else {
        vec![
            c + egui::vec2(-2.0, -4.0),
            c + egui::vec2(3.0, 0.0),
            c + egui::vec2(-2.0, 4.0),
        ]
    };
    p.add(egui::Shape::convex_polygon(
        tri,
        v.strong_text_color(),
        egui::Stroke::NONE,
    ));
    let gy = rect.center().y - galley.size().y / 2.0;
    p.galley(
        egui::pos2(rect.left() + 22.0, gy),
        galley,
        v.strong_text_color(),
    );
    if resp
        .on_hover_text(tr(
            "the verdict table: every log and axis against the targets",
        ))
        .clicked()
    {
        app.tools.step_display.details = !open;
    }
}

fn verdict_table(app: &mut App, ui: &mut Ui, rows: &[AxisResult]) {
    let mut switch_to: Option<Axis> = None;
    let judged = app.step_view.rows.clone();
    egui::Grid::new("step-verdicts")
        .striped(true)
        .show(ui, |ui| {
            ui.label("");
            for h in crate::views::STEP_COLUMNS {
                ui.label(RichText::new(tr(h)).strong());
            }
            ui.end_row();
            for (axis, slot, result) in rows {
                let active = app.tools.axis_focus == Some(*axis);
                let name = format!("{} {}", slot.name(), axis.name());
                if ui
                    .selectable_label(
                        active,
                        RichText::new(name).color(slot_color(*slot)).strong(),
                    )
                    .on_hover_text(tr("click: this axis at full height"))
                    .clicked()
                {
                    switch_to = Some(*axis);
                }
                match result {
                    Some(Ok(_)) => {
                        let v = judged
                            .iter()
                            .find(|(s, a, _)| s == slot && a == axis)
                            .map(|r| r.2.as_slice())
                            .unwrap_or_default();
                        for vd in v {
                            ui.label(
                                RichText::new(format!("{} {}", vd.value, vd.level.marker()))
                                    .color(verdict_color(vd.level)),
                            )
                            .on_hover_text(&vd.note);
                        }
                    }
                    Some(Err(e)) => {
                        ui.label(
                            RichText::new(format!("{}: {e}", slot.name()))
                                .color(Color32::LIGHT_RED),
                        );
                    }
                    None => {
                        ui.spinner();
                    }
                }
                ui.end_row();
            }
        });
    if let Some(axis) = switch_to {
        focus_axis(app, Some(axis));
    }
}

/// The Advice line: a PID proposal on the first visible log, when the recommender has one.
fn advice_line(app: &mut App, ui: &mut Ui) {
    app.step_view.advice = None;
    let Some(slot) = app.visible_slots().first().copied() else {
        return;
    };
    let rp = app.tools.recommend.clone();
    let Some(Ok(o)) = app.result(slot, Kind::Recommend(rp)) else {
        return;
    };
    let Output::Recommend(r) = o.as_ref() else {
        return;
    };
    let lines: Vec<String> = r
        .proposals
        .iter()
        .filter(|p| p.path.starts_with("pid."))
        .map(|p| {
            let mut parts = p.path.split('.').skip(1);
            let axis = parts.next().unwrap_or("");
            let term = parts.next().unwrap_or("").to_uppercase();
            format!(
                "{} {axis} {term} {} -> {} ({})",
                slot.name(),
                p.from.as_deref().unwrap_or("?"),
                p.to,
                p.rationale
            )
        })
        .collect();
    let Some(first) = lines.first() else {
        return;
    };
    let line = format!("{}: {first}", tr("Advice"));
    app.step_view.advice = Some(line.clone());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(tr("Go to Advice")).clicked() {
            app.tab = crate::app::Tab::Advice;
        }
        if lines.len() > 1 {
            ui.label(format!("(+{})", lines.len() - 1));
        }
        ui.add(egui::Label::new(RichText::new(line).color(crate::colormap::ACCENT)).truncate())
            .on_hover_text(lines.join("\n"));
    });
}

#[allow(
    clippy::too_many_arguments,
    reason = "one plot of a row, every input is its own"
)]
fn response_plot(
    ui: &mut Ui,
    axis: Axis,
    (outputs, pids, note): Curves<'_>,
    display: &StepDisplay,
    targets: &StepTargets,
    band: (f64, f64, f64),
    end_ms: f64,
    width: f32,
    height: f32,
) {
    let warn_line = 1.0 + targets.overshoot_warn_pct / 100.0;
    let fail_line = 1.0 + targets.overshoot_fail_pct / 100.0;
    Plot::new(format!("step-{}", axis.name()))
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .legend(Legend::default())
        .x_axis_label(tr("time (ms)"))
        .y_axis_label(format!("{} {}", axis.name(), tr("response")))
        .width(width)
        .height(height)
        .include_x(0.0)
        .include_x(end_ms)
        .include_y(0.0)
        .include_y(display.y_max)
        .auto_bounds(egui::Vec2b::new(false, false))
        .default_y_bounds(0.0, display.y_max)
        .default_x_bounds(0.0, end_ms)
        .show(ui, |pu| {
            if let Some(note) = note {
                pu.text(
                    egui_plot::Text::new(
                        "",
                        egui_plot::PlotPoint::new(end_ms / 2.0, display.y_max / 2.0),
                        RichText::new(note).size(13.0),
                    )
                    .color(crate::colormap::MUTED),
                );
            }
            pu.polygon(
                egui_plot::Polygon::new(
                    // reference marks stay out of the legend (empty names)
                    "",
                    PlotPoints::from(vec![
                        [band.0, band.1],
                        [end_ms, band.1],
                        [end_ms, band.2],
                        [band.0, band.2],
                    ]),
                )
                .fill_color(Color32::from_rgba_unmultiplied(0, 158, 115, 30))
                .stroke(egui::Stroke::NONE),
            );
            pu.hline(
                HLine::new("", 1.0)
                    .color(crate::colormap::MUTED)
                    .style(LineStyle::dashed_dense()),
            );
            pu.hline(
                HLine::new("", warn_line)
                    .color(verdict_color(VerdictLevel::Warn))
                    .style(LineStyle::dashed_loose()),
            );
            pu.hline(
                HLine::new("", fail_line)
                    .color(verdict_color(VerdictLevel::Fail))
                    .style(LineStyle::dashed_loose()),
            );
            for (i, (slot, o)) in outputs.iter().enumerate() {
                let Output::Step(s) = o.as_ref() else {
                    continue;
                };
                if display.raw
                    && i == 0
                    && let Some(hm) = grid_heatmap(
                        &format!("{} responses", slot.name()),
                        &s.histogram,
                        0.0,
                        f64::MIN,
                    )
                {
                    pu.heatmap(hm);
                }
                let pts = |v: &[f64]| {
                    PlotPoints::from(
                        s.time_ms
                            .iter()
                            .zip(v)
                            .map(|(t, y)| [*t, *y])
                            .collect::<Vec<_>>(),
                    )
                };
                let pid = pids
                    .iter()
                    .find(|(p, _)| p == slot)
                    .and_then(|(_, t)| t.clone())
                    .map_or_else(String::new, |t| format!(" P,I,D,Dm,F {t}"));
                pu.line(
                    Line::new(
                        format!("{} mean{pid} (n={})", slot.name(), s.windows_used),
                        pts(&s.mean),
                    )
                    .color(slot_color(*slot))
                    .width(2.5),
                );
                // the input groups only when the windows fall into both
                let (low, high) = match (&s.low, &s.high) {
                    (Some(l), Some(h)) => (Some(l), Some(h)),
                    _ => (None, None),
                };
                if let Some(l) = low {
                    pu.line(
                        Line::new(format!("{} low input", slot.name()), pts(l))
                            .color(slot_color(*slot))
                            .width(1.0)
                            .style(LineStyle::dotted_dense()),
                    );
                }
                if let Some(hi) = high {
                    pu.line(
                        Line::new(format!("{} high input", slot.name()), pts(hi))
                            .color(slot_color(*slot))
                            .width(1.0)
                            .style(LineStyle::dashed_dense()),
                    );
                }
            }
        });
}

/// One point per log at x = its position, with an error bar.
fn per_log_points(pu: &mut egui_plot::PlotUi<'_>, values: &[(Slot, f64, Option<f64>)]) {
    for (i, (slot, y, sd)) in values.iter().enumerate() {
        let x = i as f64 + 1.0;
        pu.points(
            Points::new(format!("{} {y:.2}", slot.name()), vec![[x, *y]])
                .radius(4.0)
                .color(slot_color(*slot)),
        );
        // the spread: a vertical bar with short caps
        if let Some(sd) = sd.filter(|sd| *sd > 0.0) {
            let (lo, hi) = (y - sd, y + sd);
            let cap = 0.12;
            for (name, pts) in [
                ("spread", vec![[x, lo], [x, hi]]),
                ("spread low", vec![[x - cap, lo], [x + cap, lo]]),
                ("spread high", vec![[x - cap, hi], [x + cap, hi]]),
            ] {
                pu.line(
                    Line::new(format!("{} {name}", slot.name()), PlotPoints::from(pts))
                        .color(slot_color(*slot))
                        .width(1.5),
                );
            }
        }
    }
}

fn peak_plot(
    ui: &mut Ui,
    axis: Axis,
    outputs: &[(Slot, Arc<Output>)],
    _display: &StepDisplay,
    _w: f32,
    height: f32,
) {
    let values: Vec<(Slot, f64, Option<f64>)> = outputs
        .iter()
        .filter_map(|(slot, o)| match o.as_ref() {
            Output::Step(s) => Some((
                *slot,
                s.mean.iter().copied().fold(f64::MIN, f64::max),
                Some(s.spread.peak_sd),
            )),
            _ => None,
        })
        .collect();
    Plot::new(format!("step-peak-{}", axis.name()))
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .width(SIDE_PLOT_W)
        .height(height)
        .y_axis_label(format!("{} {}", axis.name(), tr("peak")))
        .include_y(0.8)
        .include_y(1.5)
        .include_x(0.5)
        .include_x(values.len().max(1) as f64 + 0.5)
        .show_x(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show(ui, |pu| {
            pu.hline(
                HLine::new("1.0 ", 1.0)
                    .color(crate::colormap::MUTED)
                    .style(LineStyle::dashed_dense()),
            );
            per_log_points(pu, &values);
        });
}

fn latency_plot(
    ui: &mut Ui,
    axis: Axis,
    outputs: &[(Slot, Arc<Output>)],
    display: &StepDisplay,
    height: f32,
) {
    let values: Vec<(Slot, f64, Option<f64>)> = outputs
        .iter()
        .filter_map(|(slot, o)| match o.as_ref() {
            Output::Step(s) => match display.latency {
                LatencyShown::HalfRise => s
                    .metrics
                    .delay_ms
                    .map(|d| (*slot, d, Some(s.spread.half_ms_sd))),
                LatencyShown::Xcorr => s.metrics.latency_ms.map(|d| (*slot, d, None)),
            },
            _ => None,
        })
        .collect();
    Plot::new(format!("step-latency-{}", axis.name()))
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .width(SIDE_PLOT_W)
        .height(height)
        .y_axis_label(format!("{} {}", axis.name(), tr("latency (ms)")))
        .include_y(0.0)
        .include_x(0.5)
        .include_x(values.len().max(1) as f64 + 0.5)
        .show_x(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show(ui, |pu| per_log_points(pu, &values));
}

/// Errors of the Step tab's slots, shown once above the rows.
#[allow(
    dead_code,
    reason = "kept for tabs that report errors outside the table"
)]
fn show_errors(ui: &mut Ui, errors: &[(Slot, String)]) {
    for (slot, e) in errors {
        show_error(ui, *slot, e);
    }
}
