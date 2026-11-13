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
use egui_plot::{HLine, Legend, Line, LineStyle, PlotPoints, Points};
use openpid_analysis::step_windows;
use openpid_analysis::{
    FitnessLevel, StepMethod, StepOutput, StepParams, StepPerformance, StepTargets, VerdictLevel,
    verdicts,
};
use openpid_core::Axis;
use openpid_core::settings::{PidGains, Settings};
use std::sync::Arc;

/// Width of the control panel (R4).
pub const PANEL_W: f32 = 236.0;
/// Width of the collapsed panel strip.
const COLLAPSED_W: f32 = 26.0;
/// Width of each of the Step tab's two narrow plots.
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

/// Which latency [`latency_values`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LatencyShown {
    /// Time for the mean response to reach 50 %.
    #[default]
    HalfRise,
    /// Cross-correlation lag of setpoint and gyro.
    Xcorr,
}

/// A number of a step response that a narrow plot beside the response
/// draws, one point per log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepNumber {
    /// The peak of the mean response ([`peak_values`]).
    Peak,
    /// The SR latency: the time to 50 % ([`LatencyShown::HalfRise`]).
    Latency,
    /// The delay from setpoint to gyro ([`LatencyShown::Xcorr`]).
    Delay,
    /// `StepPerformance::rise_ms`.
    RiseTime,
    /// `StepPerformance::bandwidth_hz`.
    Bandwidth,
    /// `StepPerformance::sensitivity_db`.
    SensitivityPeak,
    /// `StepPerformance::sensitivity_hz`.
    SensitivityFreq,
    /// `StepPerformance::phase_margin_deg`.
    PhaseMargin,
    /// `StepPerformance::crossover_hz`.
    Crossover,
}

/// What a horizontal line of a narrow plot stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Where the number is when the gyro follows the setpoint (grey).
    Reference,
    /// From here on the number asks for attention (amber).
    Caution,
    /// From here on it is too much (red).
    Limit,
}

/// A sensitivity peak from here on is drawn against the caution line, dB.
pub const SENSITIVITY_CAUTION_DB: f64 = 4.0;
/// A sensitivity peak from here on leaves too little stability margin, dB.
pub const SENSITIVITY_LIMIT_DB: f64 = 6.0;

impl StepNumber {
    /// Its name, as the hints of the list have it.
    pub fn name(self) -> &'static str {
        match self {
            StepNumber::Peak => tr("Peak"),
            StepNumber::Latency => tr("Latency"),
            StepNumber::Delay => tr("SP-gyro delay"),
            StepNumber::RiseTime => tr("Rise time"),
            StepNumber::Bandwidth => tr("Bandwidth"),
            StepNumber::SensitivityPeak => tr("Sensitivity peak"),
            StepNumber::SensitivityFreq => tr("Sensitivity freq"),
            StepNumber::PhaseMargin => tr("Phase margin"),
            StepNumber::Crossover => tr("Crossover freq"),
        }
    }

    /// The label of its plot's axis, with the unit (after the axis name).
    pub fn axis_label(self) -> &'static str {
        match self {
            StepNumber::Peak => tr("peak"),
            StepNumber::Latency => tr("latency (ms)"),
            StepNumber::Delay => tr("SP-gyro delay (ms)"),
            StepNumber::RiseTime => tr("rise time (ms)"),
            StepNumber::Bandwidth => tr("bandwidth (Hz)"),
            StepNumber::SensitivityPeak => tr("sensitivity (dB)"),
            StepNumber::SensitivityFreq => tr("sensitivity (Hz)"),
            StepNumber::PhaseMargin => tr("phase margin (°)"),
            StepNumber::Crossover => tr("crossover (Hz)"),
        }
    }

    /// What it is, in a sentence: the hint of the list.
    pub fn hint(self) -> &'static str {
        match self {
            StepNumber::Peak => {
                tr("the peak of the mean response; the bar is the spread of the windows' peaks")
            }
            StepNumber::Latency => tr(
                "the time the response takes to reach 50 % (SR latency); the bar is its spread over the windows",
            ),
            StepNumber::Delay => tr("the delay from setpoint to gyro, by cross-correlation"),
            StepNumber::RiseTime => tr("the time the mean response takes from 10 % to 90 % of 1"),
            StepNumber::Bandwidth => tr(
                "the frequency at which the closed loop falls to -3 dB: up to there the gyro follows the setpoint",
            ),
            StepNumber::SensitivityPeak => tr(
                "the most the loop amplifies a disturbance: below 4 dB is comfortable, 6 dB and more is too little stability margin",
            ),
            StepNumber::SensitivityFreq => {
                tr("the frequency at which the loop amplifies a disturbance the most")
            }
            StepNumber::PhaseMargin => tr(
                "how far the open loop's phase is from -180° at the crossover: the less, the nearer the loop is to oscillating",
            ),
            StepNumber::Crossover => tr("the frequency at which the open-loop gain is 0 dB"),
        }
    }

    /// The horizontal lines of its plot: the level and what it stands for.
    pub fn marks(self) -> &'static [(f64, Mark)] {
        match self {
            StepNumber::Peak => &[(1.0, Mark::Reference)],
            StepNumber::SensitivityPeak => &[
                (SENSITIVITY_CAUTION_DB, Mark::Caution),
                (SENSITIVITY_LIMIT_DB, Mark::Limit),
            ],
            _ => &[],
        }
    }

    /// Values its plot always has in view, so that the logs are read
    /// against the same frame and the lines of [`StepNumber::marks`] are
    /// inside it.
    pub fn in_view(self) -> &'static [f64] {
        match self {
            StepNumber::Peak => &[0.8, 1.5],
            StepNumber::SensitivityPeak => &[0.0, 7.0],
            StepNumber::PhaseMargin => &[0.0, 90.0],
            _ => &[0.0],
        }
    }

    /// The part of its plot's id that names it.
    fn key(self) -> &'static str {
        match self {
            StepNumber::Peak => "peak",
            StepNumber::Latency => "latency",
            StepNumber::Delay => "delay",
            StepNumber::RiseTime => "rise",
            StepNumber::Bandwidth => "bandwidth",
            StepNumber::SensitivityPeak => "sensitivity",
            StepNumber::SensitivityFreq => "sensitivity-freq",
            StepNumber::PhaseMargin => "margin",
            StepNumber::Crossover => "crossover",
        }
    }
}

/// The pair of numbers the two narrow plots beside each response show: the
/// "Plots" list of the panel, the five pairs the reference tool offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidePlots {
    /// Peak and SR latency.
    #[default]
    PeakLatency,
    /// SR latency and the delay from setpoint to gyro.
    LatencyDelay,
    /// Rise time and bandwidth.
    RiseBandwidth,
    /// Sensitivity peak and its frequency.
    Sensitivity,
    /// Phase margin and crossover frequency.
    MarginCrossover,
}

impl SidePlots {
    /// Every pair, in the order of the list.
    pub const ALL: [SidePlots; 5] = [
        SidePlots::PeakLatency,
        SidePlots::LatencyDelay,
        SidePlots::RiseBandwidth,
        SidePlots::Sensitivity,
        SidePlots::MarginCrossover,
    ];

    /// The numbers of the left and of the right plot.
    pub fn numbers(self) -> [StepNumber; 2] {
        match self {
            SidePlots::PeakLatency => [StepNumber::Peak, StepNumber::Latency],
            SidePlots::LatencyDelay => [StepNumber::Latency, StepNumber::Delay],
            SidePlots::RiseBandwidth => [StepNumber::RiseTime, StepNumber::Bandwidth],
            SidePlots::Sensitivity => [StepNumber::SensitivityPeak, StepNumber::SensitivityFreq],
            SidePlots::MarginCrossover => [StepNumber::PhaseMargin, StepNumber::Crossover],
        }
    }

    /// Its entry in the list, short enough for the panel's value column.
    pub fn name(self) -> &'static str {
        match self {
            SidePlots::PeakLatency => tr("Peak | Latency"),
            SidePlots::LatencyDelay => tr("Latency | SP-gyro delay"),
            SidePlots::RiseBandwidth => tr("Rise time | Bandwidth"),
            SidePlots::Sensitivity => tr("Sensitivity peak | freq"),
            SidePlots::MarginCrossover => tr("Phase margin | Crossover"),
        }
    }

    /// What its two numbers are, a line each: the hint of its entry.
    pub fn hint(self) -> String {
        let [left, right] = self.numbers();
        format!(
            "{}: {}\n{}: {}",
            left.name(),
            left.hint(),
            right.name(),
            right.hint()
        )
    }
}

/// What a narrow plot drew: the axis, the number, and for every log its
/// value and, where it has one, the spread ([`side_values`]).
pub type SidePlot = (Axis, StepNumber, Vec<(Slot, f64, Option<f64>)>);

/// Step tab display options (not analysis parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct StepDisplay {
    /// Upper end of the response axis.
    pub y_max: f64,
    /// Draw the histogram of the individual responses.
    pub raw: bool,
    /// What the two narrow plots beside each response show.
    pub side: SidePlots,
    /// The verdict table under the plots is open.
    pub details: bool,
}

impl Default for StepDisplay {
    fn default() -> Self {
        StepDisplay {
            y_max: 1.5,
            raw: false,
            side: SidePlots::default(),
            details: false,
        }
    }
}

/// Smoothing levels of [`StepMethod::Wiener`], as the width of the moving
/// average over the mean (ms).
const SMOOTHING: [(&str, f64); 4] = [
    ("off", 0.0),
    ("low", 10.0),
    ("medium", 20.0),
    ("high", 40.0),
];

fn step_controls(app: &mut App, ui: &mut Ui) {
    axes_row(app, ui);
    panel::section(ui, tr("Display"));
    let division = app.tools.step.method == StepMethod::Division;
    if division {
        // the levels of the method itself: the gyro is smoothed first
        let levels: Vec<(usize, String)> = step_windows::SMOOTHING
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (i, tr(name).to_string()))
            .collect();
        panel::choice(
            ui,
            "step-smoothing",
            tr("Smoothing"),
            tr(
                "a local regression of the gyro before the response is computed: off, 5, 11 or 21 samples; it brings back windows that noise had dropped, and blurs a sharp step",
            ),
            &mut app.tools.step.smoothing,
            &levels,
        );
    } else {
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
    }
    let pairs: Vec<(SidePlots, String, String)> = SidePlots::ALL
        .iter()
        .map(|pair| (*pair, pair.name().to_string(), pair.hint()))
        .collect();
    let side = app.tools.step_display.side;
    panel::choice_explained(
        ui,
        "step-side",
        tr("Plots"),
        &format!(
            "{}\n\n{}",
            tr("what the two narrow plots beside each response show, one point per log"),
            side.hint()
        ),
        &mut app.tools.step_display.side,
        &pairs,
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
    if !division {
        panel::check(
            ui,
            &mut app.tools.step.y_offset,
            tr("Y correction"),
            tr("normalise every window on its settled tail before averaging"),
        );
    }
    panel::check(
        ui,
        &mut app.tools.step_display.raw,
        tr("Raw responses"),
        tr("the individual responses behind the mean, as a density"),
    );
    panel::section(ui, tr("Analysis"));
    panel::choice(
        ui,
        "step-method",
        tr("Method"),
        tr(
            "windowed division: windows of 2 s every 0.05 s, the gyro's spectrum divided by the setpoint's; a window counts when the stick was 20 °/s from its centre on average and its response stays between 0.5 and 1.5 after 150 ms; the plain mean of those, as the reference tool computes it. Wiener with cut-off: OpenPID's former estimate, which rounds a sharp response and reads a peak a little high",
        ),
        &mut app.tools.step.method,
        &[
            (StepMethod::Division, tr("windowed division").to_string()),
            (StepMethod::Wiener, tr("Wiener with cut-off").to_string()),
        ],
    );
    if division {
        return;
    }
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

/// The "SR latency" of a result: with the windowed division the mean over
/// the windows of each window's time to 0.5, as the reference tool shows
/// it; with the Wiener estimate the time the mean response takes to reach
/// half of where it settles.
pub fn sr_latency_ms(s: &StepOutput) -> Option<f64> {
    match s.method {
        StepMethod::Division => Some(s.spread.half_ms_mean).filter(|v| v.is_finite()),
        StepMethod::Wiener => s.metrics.delay_ms,
    }
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
/// response with its two narrow plots (Peak and Latency, or another pair of
/// [`SidePlots`]).
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
    app.step_view.side.clear();
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
            let scale = ResponseScale {
                display: &display,
                targets: &targets,
                band,
                end_ms,
            };
            response_plot(
                ui,
                axis,
                (&outputs, &pids, note.as_deref()),
                &scale,
                egui::vec2(w, h),
            );
            for number in display.side.numbers() {
                let values = side_values(&outputs, number);
                side_plot(ui, axis, number, &values, h);
                app.step_view.side.push((axis, number, values));
            }
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

/// One plot in a tab's grid of axis rows: whose it is, which axis it shows,
/// how large it is and where it sits. The tabs pass it instead of a slot,
/// an axis, a size and two flags each.
#[derive(Debug, Clone, Copy)]
pub struct PlotCell {
    pub slot: Slot,
    pub axis: Axis,
    pub size: egui::Vec2,
    /// The first column of its row: it carries the y axis label.
    pub first: bool,
    /// The last row: it carries the x axis and its label.
    pub last: bool,
}

/// What the response plot is drawn against: the display options, the
/// targets with their band, and where the time axis ends.
struct ResponseScale<'a> {
    display: &'a StepDisplay,
    targets: &'a StepTargets,
    band: (f64, f64, f64),
    end_ms: f64,
}

fn response_plot(
    ui: &mut Ui,
    axis: Axis,
    (outputs, pids, note): Curves<'_>,
    scale: &ResponseScale<'_>,
    size: egui::Vec2,
) {
    let ResponseScale {
        display,
        targets,
        band,
        end_ms,
    } = *scale;
    let (width, height) = (size.x, size.y);
    let warn_line = 1.0 + targets.overshoot_warn_pct / 100.0;
    let fail_line = 1.0 + targets.overshoot_fail_pct / 100.0;
    crate::grid::plot(format!("step-{}", axis.name()))
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

/// What the Peak plot draws for every log: the highest point of its mean
/// response (with the windowed division, in its first 150 ms, as the
/// reference tool reads it), and the spread of the windows' peaks as the bar.
pub fn peak_values(outputs: &[(Slot, Arc<Output>)]) -> Vec<(Slot, f64, Option<f64>)> {
    outputs
        .iter()
        .filter_map(|(slot, o)| match o.as_ref() {
            Output::Step(s) => Some((
                *slot,
                match s.method {
                    StepMethod::Division => s.spread.peak_mean,
                    StepMethod::Wiener => s.mean.iter().copied().fold(f64::MIN, f64::max),
                },
                Some(s.spread.peak_sd),
            )),
            _ => None,
        })
        .collect()
}

/// What the Latency plot draws for every log: the SR latency with the
/// spread over the windows as the bar, or the delay from setpoint to gyro.
pub fn latency_values(
    outputs: &[(Slot, Arc<Output>)],
    shown: LatencyShown,
) -> Vec<(Slot, f64, Option<f64>)> {
    outputs
        .iter()
        .filter_map(|(slot, o)| match o.as_ref() {
            Output::Step(s) => match shown {
                LatencyShown::HalfRise => {
                    sr_latency_ms(s).map(|d| (*slot, d, Some(s.spread.half_ms_sd)))
                }
                LatencyShown::Xcorr => s.metrics.latency_ms.map(|d| (*slot, d, None)),
            },
            _ => None,
        })
        .collect()
}

/// What the narrow plot of `number` draws for every log: its value and,
/// for the peak and the SR latency, the spread over the windows as the bar
/// ([`peak_values`], [`latency_values`]); the performance numbers have no
/// bar. A log without the number has no point.
pub fn side_values(
    outputs: &[(Slot, Arc<Output>)],
    number: StepNumber,
) -> Vec<(Slot, f64, Option<f64>)> {
    let read = |of: fn(&StepPerformance) -> Option<f64>| {
        outputs
            .iter()
            .filter_map(|(slot, o)| match o.as_ref() {
                Output::Step(s) => of(&s.performance).map(|v| (*slot, v, None)),
                _ => None,
            })
            .collect()
    };
    match number {
        StepNumber::Peak => peak_values(outputs),
        StepNumber::Latency => latency_values(outputs, LatencyShown::HalfRise),
        StepNumber::Delay => latency_values(outputs, LatencyShown::Xcorr),
        StepNumber::RiseTime => read(|p| p.rise_ms),
        StepNumber::Bandwidth => read(|p| p.bandwidth_hz),
        StepNumber::SensitivityPeak => read(|p| p.sensitivity_db),
        StepNumber::SensitivityFreq => read(|p| p.sensitivity_hz),
        StepNumber::PhaseMargin => read(|p| p.phase_margin_deg),
        StepNumber::Crossover => read(|p| p.crossover_hz),
    }
}

/// A narrow plot beside the response of `axis`: `number` of every log
/// (`values`, from [`side_values`]) against the lines of its marks.
fn side_plot(
    ui: &mut Ui,
    axis: Axis,
    number: StepNumber,
    values: &[(Slot, f64, Option<f64>)],
    height: f32,
) {
    let mut plot = crate::grid::plot(format!("step-{}-{}", number.key(), axis.name()))
        .width(SIDE_PLOT_W)
        .height(height)
        .y_axis_label(format!("{} {}", axis.name(), number.axis_label()))
        .include_x(0.5)
        .include_x(values.len().max(1) as f64 + 0.5)
        .show_x(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false);
    for y in number.in_view() {
        plot = plot.include_y(*y);
    }
    plot.show(ui, |pu| {
        for (level, mark) in number.marks() {
            let (name, color, style) = match mark {
                Mark::Reference => (
                    "1.0 ".to_string(),
                    crate::colormap::MUTED,
                    LineStyle::dashed_dense(),
                ),
                Mark::Caution => (
                    format!("{level} dB"),
                    verdict_color(VerdictLevel::Warn),
                    LineStyle::dashed_loose(),
                ),
                Mark::Limit => (
                    format!("{level} dB"),
                    verdict_color(VerdictLevel::Fail),
                    LineStyle::dashed_loose(),
                ),
            };
            pu.hline(HLine::new(name, *level).color(color).style(style));
        }
        per_log_points(pu, values);
    });
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
