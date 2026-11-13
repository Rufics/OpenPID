//! The steps the capture script walks through for a round: which windows of
//! the reference tool to open, what to click and which pictures to save.
//!
//! The script is `tools/capture/capture.ps1`; it is carried inside every
//! pack, so a pack is all the owner needs. Positions are pixels of the
//! round 0 screenshot of the main window
//! (`docs/calibration/REFERENCE-SCREENS.md`), or of the pictures the script
//! itself saves; the script scales them to the window as it is.
//!
//! The positions are those of version 0.94 of the tool, measured in
//! pictures of it; the plans of every round are made for that version, and
//! none of them fits version 0.9, which rounds 1 to 5 were run on.
//!
//! From round 2 on the curves are not read from pictures: every plot is
//! copied as a vector graphic through the small toolbar the tool has above
//! a plot, and the script saves the copy from the clipboard.

use crate::catalogue::{Area, Probe};
use serde::Serialize;

/// The capture script, as committed.
const SCRIPT: &str = include_str!("../../../tools/capture/capture.ps1");

/// Title of the tool's main window.
const LOG_VIEWER: &str = "Log Viewer";
/// Title of its spectrum window.
const SPECTRUM: &str = "Spectral Analyzer";
/// Title of its step response window.
const STEP: &str = "Step Response Tool";
/// Title of its window of frequency against throttle.
const THROTTLE: &str = "Frequency x Throttle";

/// The rows of plots of the step response window in a picture the script
/// saves: top and bottom of the axes of roll, pitch and yaw.
const STEP_ROWS: [(u32, u32); 3] = [(172, 753), (887, 1468), (1603, 2183)];
/// Its three columns: the name of the plot, left and right of the axes.
const STEP_COLUMNS: [(&str, u32, u32); 3] = [
    ("response", 189, 1700),
    ("peak", 2192, 2607),
    ("latency", 2759, 3174),
];
/// Controls of the step response window, in a picture the script saves.
mod step_at {
    /// The first log of the list; the next ones follow 37.9 pixels apart.
    pub const LOG: (u32, u32) = (3515, 177);
    pub const RUN: (u32, u32) = (3466, 821);
    pub const SMOOTHING_LIST: (u32, u32) = (3464, 1093);
    /// "off", "low", "medium", "high" of the open smoothing list.
    pub const SMOOTHING: [(&str, (u32, u32)); 4] = [
        ("off", (3464, 1136)),
        ("low", (3464, 1184)),
        ("medium", (3464, 1231)),
        ("high", (3464, 1278)),
    ];
    /// The list that chooses what the two right columns of plots show.
    pub const PAIR_LIST: (u32, u32) = (3640, 1093);
    /// The items of that list when it is open: the first name of a pair is
    /// drawn in the middle column of plots, the second in the right one.
    /// The first pair is what the window comes up with.
    pub const PAIRS: [(&str, (u32, u32)); 5] = [
        ("SR Peak | SR Latency", (3515, 1136)),
        ("SR Latency | SP-Gyro Delay", (3515, 1184)),
        ("Rise Time | Bandwidth", (3515, 1231)),
        ("Sensitivity Peak | Sensitivity Freq", (3515, 1278)),
        ("Phase Margin | Crossover Freq", (3515, 1326)),
    ];
    /// The list, below "Y max", of the length of the window a response is
    /// computed from.
    pub const WINDOW_LIST: (u32, u32) = (3553, 1563);
    /// The items of that list when it is open, "1sec deconv. window" to
    /// "5sec": the seconds and where the item is. Two seconds is what the
    /// window comes up with.
    pub const WINDOWS: [(u32, (u32, u32)); 5] = [
        (1, (3553, 1608)),
        (2, (3553, 1656)),
        (3, (3553, 1703)),
        (4, (3553, 1751)),
        (5, (3553, 1798)),
    ];
    /// The length the window comes up with, seconds.
    pub const WINDOW_DEFAULT_S: u32 = 2;
}

/// The lengths of the window, seconds, the first step-response log of a
/// round up to [`RULE_ROUNDS`] is captured with beyond the one the tool
/// comes up with: to learn how the length enters. The model predicted
/// nothing for them.
const WINDOW_LENGTHS_S: [u32; 2] = [1, 3];

/// The last of the rounds that settled the rule of the step response
/// itself. Up to it a round captures the delay between setpoint and gyro,
/// the response at every smoothing level for some logs and the window
/// lengths of its first log; the later rounds are about the numbers the
/// tool reads off the response, and capture the pairs of plots only.
///
/// Every window's response (the "Raw" plots) was captured up to round 6
/// and is captured no more: the rule does not need them, and a plot of
/// 600 curves takes longer to copy than the script waits.
pub const RULE_ROUNDS: u32 = 6;

/// A setting of the step response window: the smoothing level (its place
/// in the tool's list, 0 for "off") and the seconds of the window.
pub type Setting = (usize, u32);

/// The setting the window comes up with: no smoothing, two seconds.
pub const DEFAULT_SETTING: Setting = (0, step_at::WINDOW_DEFAULT_S);

/// The settings, beyond the one the window comes up with, at which every
/// plot of a probe is captured again: for one probe of round 7 windows of
/// one and of three seconds and the smoothing level "high", to see whether
/// the numbers read off the response follow them as the response does.
pub fn settings_captured(probe: &Probe) -> &'static [Setting] {
    if probe.id == "y4-damping-2k" {
        &[(0, 1), (0, 3), (3, step_at::WINDOW_DEFAULT_S)]
    } else {
        &[]
    }
}

/// The name of a setting in the names of its files: "window-1s",
/// "smoothing-high"; nothing for the setting the window comes up with.
fn setting_name((level, seconds): Setting) -> String {
    let mut name = String::new();
    if seconds != DEFAULT_SETTING.1 {
        name = format!("window-{seconds}s");
    }
    if level != DEFAULT_SETTING.0 {
        let (smoothing, _) = step_at::SMOOTHING[level % step_at::SMOOTHING.len()];
        name = joined(&name, &format!("smoothing-{smoothing}"));
    }
    name
}

/// Two parts of a file name with a hyphen between them, or the one that is
/// not empty.
fn joined(first: &str, second: &str) -> String {
    match (first.is_empty(), second.is_empty()) {
        (true, _) => second.to_string(),
        (_, true) => first.to_string(),
        _ => format!("{first}-{second}"),
    }
}

/// The pairs of [`step_at::PAIRS`] a round captures beyond the first two:
/// the place of the pair in that list, the name of the picture, and what
/// the plots of the middle and of the right column are called in the names
/// of their copies.
const PARAMETERS: [(usize, &str, [&str; 2]); 3] = [
    (2, "rise-bandwidth", ["risetime", "bandwidth"]),
    (3, "sensitivity", ["sensitivity-peak", "sensitivity-freq"]),
    (4, "phase-crossover", ["phase-margin", "crossover"]),
];

/// Controls of the main window, in its reference screenshot.
mod log_at {
    pub const OPEN_FILE: (u32, u32) = (3437, 227);
    pub const SPECTRUM: (u32, u32) = (3556, 871);
    pub const STEP: (u32, u32) = (3556, 962);
}

/// Controls of the spectrum window, in a picture the script saves.
mod spectrum_at {
    /// The first log of the list; the next ones follow 37.9 pixels apart.
    pub const LOG: (u32, u32) = (3515, 133);
    pub const RUN: (u32, u32) = (3468, 996);
    pub const SMOOTHING_LIST: (u32, u32) = (3640, 1316);
    /// Column of the items of the open smoothing list.
    pub const SMOOTHING_X: u32 = 3591;
    /// The "Y max" box.
    pub const Y_MAX: (u32, u32) = (3610, 1612);
}

/// Controls and plots of the window of frequency against throttle of
/// version 0.94, in a picture the script saves. Read off a picture of the
/// window half this size (the owner, 2026-11-13), so each is right to a
/// few pixels; the open lists are taken to look as those of the step
/// response window do.
mod throttle_at {
    /// "Freq x Throttle" of the spectrum window.
    pub const OPEN: (u32, u32) = (3555, 1078);
    pub const RUN: (u32, u32) = (3468, 179);
    /// The plots of the first two columns, which come up as "Gyro prefilt"
    /// and "Gyro": the name in the files, left and right of the axes.
    pub const COLUMNS: [(&str, u32, u32); 2] = [("prefilt", 193, 848), ("gyro", 1013, 1667)];
    /// Top and bottom of the axes of roll, pitch and yaw.
    pub const ROWS: [(u32, u32); 3] = [(318, 874), (994, 1553), (1672, 2229)];
    /// The list of logs above each of those columns.
    pub const LOG_LISTS: [(u32, u32); 2] = [(482, 109), (1319, 109)];
    /// The switch "<100Hz" beside each of those lists.
    pub const BELOW_100: [(u32, u32); 2] = [(690, 109), (1540, 109)];
    /// The "scale" box of each of those columns.
    pub const SCALES: [(u32, u32); 2] = [(113, 274), (943, 274)];
    pub const SMOOTHING_LIST: (u32, u32) = (3640, 508);
    /// The switches "2D" and "PSD".
    pub const TWO_D: (u32, u32) = (3447, 603);
    pub const PSD: (u32, u32) = (3599, 603);
    /// The first item of an open list is this far below the list, and the
    /// next ones follow 47.5 pixels apart.
    pub const ITEM_FIRST: u32 = 43;

    /// Item `row` (from 0) of the open list at `list`.
    pub fn item(list: (u32, u32), row: usize) -> (u32, u32) {
        (list.0, list.1 + ITEM_FIRST + (475 * row as u32) / 10)
    }
}

/// The smoothing levels of the window of frequency against throttle, in
/// the order of its list. "medium" is what the window comes up with.
const THROTTLE_SMOOTHING: [&str; 5] = ["low", "low-med", "medium", "med-high", "high"];
/// The place of "medium" in [`THROTTLE_SMOOTHING`].
const THROTTLE_MEDIUM: usize = 2;
/// The colour scale typed for one probe in place of the 0.5 the window
/// comes up with: wide enough for a tone of 10 deg/s.
const THROTTLE_SCALE: &str = "5";
/// The colour scale the window comes up with.
const THROTTLE_SCALE_DEFAULT: &str = "0.5";

/// The row of log `row` (from 0) of a list whose first row is at `first`:
/// 37.9 pixels a row.
fn log_row(first: u32, row: usize) -> u32 {
    first + (379 * row as u32) / 10
}

/// The smoothing levels of the spectrum window with the row of each in the
/// open list (pixels of a picture the script saves), in the order they are
/// captured: the default (medium) last, so the tool is left as it was found.
const SMOOTHING: [(&str, u32); 5] = [
    ("low", 1360),
    ("low-med", 1407),
    ("med-high", 1502),
    ("high", 1550),
    ("medium", 1455),
];

/// The plots of the spectrum window in a picture the script saves: left,
/// top, right, bottom of the axes of each row (roll, pitch, yaw), "Full
/// Spectrum" first, then "Sub 100Hz".
const PLOTS: [[(u32, u32, u32, u32); 2]; 3] = [
    [(227, 160, 1662, 753), (1814, 160, 3250, 753)],
    [(227, 875, 1662, 1468), (1814, 875, 3250, 1468)],
    [(227, 1590, 1662, 2183), (1814, 1590, 3250, 2183)],
];
/// The rows of [`PLOTS`].
const AXES: [&str; 3] = ["roll", "pitch", "yaw"];
/// The columns of [`PLOTS`], as the copies are named.
const COLUMNS: [&str; 2] = ["full", "sub"];
/// The upper limit of the plots from round 3 on, dB: a flight with stick
/// moves is above the tool's default of 20 dB at low frequencies, and a
/// vector copy holds only what is inside the plot.
pub const Y_MAX_DB: u32 = 70;

/// The "..." button of a plot, from the top right corner of its axes: this
/// far left of it and this far above. A click expands the plot's toolbar to
/// the left of the button, and it stays expanded; the same spot then shows
/// ">>", which collapses it.
const TOOLBAR_DOTS: (u32, u32) = (14, 15);
/// The export icon, the first of the expanded toolbar, likewise.
const EXPORT_ICON: (u32, u32) = (181, 15);
/// "Copy as vector graphic", the third item of the menu the export icon
/// opens under itself: this far below the top of the axes.
const COPY_ITEM: u32 = 90;
/// How long the pointer rests where a toolbar or a menu has to appear
/// before it clicks there, ms.
const MENU_MS: u32 = 500;
/// How long a first try at a copy is waited for, seconds.
const COPY_WAIT_S: u32 = 3;

/// The frame of a reference screenshot: where the inside of its window is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frame {
    /// First column of the inside of the window.
    pub left: u32,
    /// First row below the title bar.
    pub top: u32,
    /// Width of the inside.
    pub width: u32,
    /// Height of the inside.
    pub height: u32,
}

/// The reference screenshots the positions are measured in.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frames {
    /// The main window, round 0.
    pub log: Frame,
    /// A picture of the spectrum or of the step response window saved by
    /// the script: all of it is the inside of the window.
    pub shot: Frame,
}

/// One step of the script. Only the fields a step uses are written.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Step {
    /// What to do: `click`, `open_file`, `settle`, `shot`, `pause`,
    /// `windows`, `hover`, `clip_wait`, `clip_save`, `type`.
    pub action: &'static str,
    /// Part of the title of the window the step is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<&'static str>,
    /// What is clicked, for the log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The reference screenshot of `at`.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub frame: Option<&'static str>,
    /// Position in the reference screenshot, pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<(u32, u32)>,
    /// Remember what the plots look like before the click.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark: Option<bool>,
    /// Files for the "open" dialog, relative to the script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// Longest wait for the plots to change since the mark, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_s: Option<f64>,
    /// How long the plots must stay the same to count as drawn, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_s: Option<f64>,
    /// Longest wait for the window to come up, or for a copy to reach the
    /// clipboard, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_s: Option<u32>,
    /// `clip_wait`: how many of the next steps are passed over when
    /// something has been copied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<u32>,
    /// Name of the picture, without the extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Length of a pause, or of the rest after the pointer has arrived,
    /// milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u32>,
    /// The click copies something: the clipboard is watched from here on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<bool>,
    /// `false`: the window is not brought to the front first (an open menu
    /// would close).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front: Option<bool>,
    /// What is typed into a text box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The step belongs to the copy of a plot: it is passed over once the
    /// copies of a picture have been given up ([`Plan::give_up`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
}

/// `steps.json` of a pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    /// What the run does, for the reader of the file.
    pub about: String,
    /// The reference screenshots.
    pub refs: Frames,
    /// After a copy that failed, the other copies up to the next picture
    /// are passed over: where the plots of a window may not be copied at
    /// all, a run does not spend most of its time waiting for them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub give_up: bool,
    /// The steps in order.
    pub steps: Vec<Step>,
}

fn click(window: &'static str, frame: &'static str, name: &str, at: (u32, u32)) -> Step {
    Step {
        action: "click",
        window: Some(window),
        name: Some(name.to_string()),
        frame: Some(frame),
        at: Some(at),
        ..Step::default()
    }
}

fn pause(ms: u32) -> Step {
    Step {
        action: "pause",
        ms: Some(ms),
        ..Step::default()
    }
}

fn settle(window: &'static str, change_s: Option<f64>, quiet_s: f64) -> Step {
    Step {
        action: "settle",
        window: Some(window),
        change_s,
        quiet_s: Some(quiet_s),
        ..Step::default()
    }
}

fn shot(window: &'static str, file: String) -> Step {
    Step {
        action: "shot",
        window: Some(window),
        file: Some(file),
        ..Step::default()
    }
}

/// The pointer travels to `at` of a picture of `window` and rests there
/// for `ms`.
fn hover(window: &'static str, name: String, at: (u32, u32), ms: u32) -> Step {
    Step {
        action: "hover",
        window: Some(window),
        name: Some(name),
        frame: Some("shot"),
        at: Some(at),
        ms: Some(ms),
        front: Some(false),
        ..Step::default()
    }
}

/// Name of the vector copy of one plot: `axis` 0 … 2, `column` 0 (the whole
/// range) or 1 (below 100 Hz).
pub fn spectrum_copy(
    round: u32,
    probe: &str,
    smoothing: &str,
    axis: usize,
    column: usize,
) -> String {
    format!(
        "r{round}-{probe}-spectrum-{smoothing}-{}-{}",
        AXES[axis % 3],
        COLUMNS[column % 2]
    )
}

/// The plots copied as vector graphics at a smoothing level: none in round
/// 1 (it was read from pictures); from round 2 on the three plots below
/// 100 Hz, which carry every point of their curve, and at the tool's
/// default level also the three over the whole range.
fn copied(round: u32, smoothing: &str) -> Vec<(usize, usize)> {
    if round < 2 {
        return Vec::new();
    }
    let columns: &[usize] = if smoothing == "medium" { &[1, 0] } else { &[1] };
    columns
        .iter()
        .flat_map(|column| (0..3).map(move |axis| (axis, *column)))
        .collect()
}

/// The steps that copy one plot of the spectrum window.
fn copy_plot(steps: &mut Vec<Step>, axis: usize, column: usize, file: String) {
    let plot = format!("{} {}", AXES[axis % 3], COLUMNS[column % 2]);
    copy_axes(steps, SPECTRUM, &plot, PLOTS[axis % 3][column % 2], file);
}

/// The steps that copy one plot as a vector graphic and save the copy.
/// `axes` is left, top, right, bottom of the plot in a picture of `window`.
///
/// A plot has a "..." button above its top right corner ([`TOOLBAR_DOTS`]):
/// a click expands a toolbar, whose first icon opens a menu with the copy
/// as its third item. Nothing appears on hovering, but a toolbar and a
/// menu take a moment to appear, so the pointer rests on each spot before
/// it clicks there. Whether the toolbar is expanded when the script arrives
/// cannot be known, and the button collapses an expanded one, so the copy
/// is tried from both states:
///
/// 1. a click on the button, the export icon, the third item of its menu;
/// 2. `clip_wait`: when something has been copied, the second try is
///    passed over;
/// 3. the button once more and the same again: the first click collapsed
///    the toolbar, this one expands it;
/// 4. `clip_save`, and a click on the button, which collapses the toolbar.
///
/// When no copy has come by `clip_save`, the script goes through the four
/// steps before it again, slower. Every step of a copy is marked as one
/// (`copy`), for a plan that gives up the copies of a picture after one
/// that failed ([`Plan::give_up`]).
fn copy_axes(
    steps: &mut Vec<Step>,
    window: &'static str,
    plot: &str,
    axes: (u32, u32, u32, u32),
    file: String,
) {
    let first = steps.len();
    let (_, top, right, _) = axes;
    let dots = (right - TOOLBAR_DOTS.0, top - TOOLBAR_DOTS.1);
    let export = (right - EXPORT_ICON.0, top - EXPORT_ICON.1);
    let item = (export.0, top + COPY_ITEM);
    let toolbar = |name: String, front: Option<bool>| Step {
        front,
        ..click(window, "shot", &name, dots)
    };
    // the pointer rests on a spot, then clicks it
    let press = |steps: &mut Vec<Step>, name: String, at: (u32, u32), clip: Option<bool>| {
        steps.push(hover(window, name.clone(), at, MENU_MS));
        steps.push(Step {
            clip,
            front: Some(false),
            ..click(window, "shot", &name, at)
        });
    };
    let copy = |steps: &mut Vec<Step>| {
        press(steps, format!("{plot} export menu"), export, None);
        press(
            steps,
            format!("{plot} copy as vector graphic"),
            item,
            Some(true),
        );
    };
    steps.push(toolbar(format!("{plot} toolbar"), None));
    copy(steps);
    steps.push(Step {
        action: "clip_wait",
        wait_s: Some(COPY_WAIT_S),
        // the button once more and the four steps of a copy
        skip: Some(5),
        ..Step::default()
    });
    steps.push(toolbar(format!("{plot} toolbar"), Some(false)));
    copy(steps);
    steps.push(Step {
        action: "clip_save",
        file: Some(file),
        // a copy is on the clipboard within a second or two
        wait_s: Some(6),
        ..Step::default()
    });
    steps.push(toolbar(format!("{plot} toolbar (close)"), Some(false)));
    steps.push(pause(200));
    for step in &mut steps[first..] {
        step.copy = Some(true);
    }
}

/// Name of a picture or of a copied plot of the step response window:
/// `what` is the setting ("", "raw", "spgyro", "smoothing-low" …) and
/// `plot` the plot ("" for a picture of the window).
pub fn step_file(round: u32, probe: &str, what: &str, plot: &str) -> String {
    let mut name = format!("r{round}-{probe}-step");
    for part in [what, plot] {
        if !part.is_empty() {
            name.push('-');
            name.push_str(part);
        }
    }
    name
}

/// Whether the response at the three smoothing levels is captured for a
/// probe: in round 4 for the plants at 2 kHz and for the probe with noise,
/// where smoothing has something to do, in round 5 for the probes that ask
/// what follows the log rate, and for one flight of the hold-out.
pub fn smoothing_captured(probe: &Probe) -> bool {
    [
        "t1-plants-2k",
        "t5-noise-2k",
        "u2-rates-1k",
        "u3-rates-4k",
        "w5-mixed-2k",
    ]
    .contains(&probe.id)
}

/// A click in the step response window, at a position of a picture of it.
fn step_click(name: &str, at: (u32, u32)) -> Step {
    click(STEP, "shot", name, at)
}

/// An item of a list of the step response window: a click on the list, a
/// moment for it to open, a click on the item.
fn step_pick(steps: &mut Vec<Step>, list: (u32, u32), name: &str, item: (u32, u32)) {
    steps.push(step_click(&format!("{name} (the list)"), list));
    steps.push(pause(600));
    steps.push(step_click(name, item));
    steps.push(pause(300));
}

/// A pair of [`step_at::PAIRS`] for the two right columns of plots.
fn step_pair(steps: &mut Vec<Step>, index: usize) {
    let (name, item) = step_at::PAIRS[index % step_at::PAIRS.len()];
    step_pick(steps, step_at::PAIR_LIST, name, item);
}

/// The window brought from the setting `from` to the setting `to`: the
/// length of the window first, then the smoothing level, each only when it
/// changes.
fn step_set(steps: &mut Vec<Step>, from: Setting, to: Setting) {
    if from.1 != to.1 {
        let item = step_at::WINDOWS.iter().find(|w| w.0 == to.1);
        let (seconds, item) = item.copied().unwrap_or(step_at::WINDOWS[1]);
        let name = format!("{seconds}sec deconv. window");
        step_pick(steps, step_at::WINDOW_LIST, &name, item);
    }
    if from.0 != to.0 {
        let (level, item) = step_at::SMOOTHING[to.0 % step_at::SMOOTHING.len()];
        let name = format!("smoothing {level}");
        step_pick(steps, step_at::SMOOTHING_LIST, &name, item);
    }
}

/// The log of a round whose plots are being captured.
#[derive(Clone, Copy)]
struct StepLog<'a> {
    round: u32,
    id: &'a str,
}

impl StepLog<'_> {
    /// A Run, waited for, and a picture of what it drew when `picture`
    /// names the setting the picture is of.
    fn run(self, steps: &mut Vec<Step>, picture: Option<&str>) {
        steps.push(Step {
            mark: Some(true),
            ..step_click("Run", step_at::RUN)
        });
        steps.push(settle(STEP, Some(20.0), 1.5));
        if let Some(what) = picture {
            steps.push(shot(STEP, step_file(self.round, self.id, what, "")));
        }
    }

    /// Copies of the plots of `columns` (the place of each in
    /// [`STEP_COLUMNS`] and its name in the files) for the three axes, at
    /// the setting `what`.
    fn copies(self, steps: &mut Vec<Step>, what: &str, columns: &[(usize, &str)]) {
        for (column, plot) in columns {
            let (_, left, right) = STEP_COLUMNS[*column % 3];
            for (axis, (top, bottom)) in STEP_ROWS.into_iter().enumerate() {
                let name = format!("{} {plot}", AXES[axis]);
                let plot = format!("{}-{plot}", AXES[axis]);
                let file = step_file(self.round, self.id, what, &plot);
                copy_axes(steps, STEP, &name, (left, top, right, bottom), file);
            }
        }
    }

    /// Every plot of the log at the setting the window is at, whose name
    /// in the files is `setting` (nothing as the window comes up): the
    /// nine plots, up to [`RULE_ROUNDS`] the delay between setpoint and
    /// gyro, and the three other pairs of plots ([`PARAMETERS`]). The
    /// first pair is chosen again at the end.
    fn plots(self, steps: &mut Vec<Step>, setting: &str) {
        self.run(steps, Some(setting));
        self.copies(steps, setting, &NINE);
        if self.round <= RULE_ROUNDS {
            // the right column of the second pair, where the first pair
            // has the latency
            let (latency, _, _) = STEP_COLUMNS[2];
            step_pair(steps, 1);
            self.run(steps, None);
            self.copies(steps, "spgyro", &[(2, latency)]);
        }
        for (index, picture, [middle, right]) in PARAMETERS {
            step_pair(steps, index);
            self.run(steps, Some(&joined(setting, picture)));
            let what = joined(setting, "params");
            self.copies(steps, &what, &[(1, middle), (2, right)]);
        }
        step_pair(steps, 0);
    }

    /// Every plot of the log again at each of `settings`
    /// ([`settings_captured`]), and the window as it came up at the end.
    /// The plots are drawn anew by the next Run; `run` asks for one here,
    /// for a log after which no other follows.
    fn settings(self, steps: &mut Vec<Step>, settings: &[Setting], run: bool) {
        let mut now = DEFAULT_SETTING;
        for setting in settings {
            step_set(steps, now, *setting);
            now = *setting;
            self.plots(steps, &setting_name(now));
        }
        step_set(steps, now, DEFAULT_SETTING);
        if run {
            self.run(steps, None);
        }
    }

    /// The nine plots from windows of other lengths
    /// ([`WINDOW_LENGTHS_S`]), and the length the window came up with
    /// again. The plots are drawn anew by the next Run; `run` asks for one
    /// here, for a log after which no other follows.
    fn window_lengths(self, steps: &mut Vec<Step>, run: bool) {
        let mut now = DEFAULT_SETTING;
        for seconds in WINDOW_LENGTHS_S {
            step_set(steps, now, (now.0, seconds));
            now.1 = seconds;
            let what = setting_name(now);
            self.run(steps, Some(&what));
            self.copies(steps, &what, &NINE);
        }
        step_set(steps, now, DEFAULT_SETTING);
        if run {
            self.run(steps, None);
        }
    }

    /// The response at the three smoothing levels, and no smoothing again.
    fn smoothing_levels(self, steps: &mut Vec<Step>) {
        let (response, _, _) = STEP_COLUMNS[0];
        for (level, item) in &step_at::SMOOTHING[1..] {
            let name = format!("smoothing {level}");
            step_pick(steps, step_at::SMOOTHING_LIST, &name, *item);
            self.run(steps, None);
            self.copies(steps, &format!("smoothing-{level}"), &[(0, response)]);
        }
        let (_, off) = step_at::SMOOTHING[0];
        step_pick(steps, step_at::SMOOTHING_LIST, "smoothing off", off);
    }
}

/// The three columns of plots as the window comes up, by their place in
/// [`STEP_COLUMNS`] and their names in the files.
const NINE: [(usize, &str); 3] = [
    (0, STEP_COLUMNS[0].0),
    (1, STEP_COLUMNS[1].0),
    (2, STEP_COLUMNS[2].0),
];

/// The steps of the step response window for the probes of a round: for
/// every log its nine plots as they come up and the three other pairs of
/// plots the window offers. Up to [`RULE_ROUNDS`] also the delay between
/// setpoint and gyro, for the first log the nine plots from windows of
/// other lengths, and for the logs of [`smoothing_captured`] the response
/// at the smoothing levels; after it, for the logs of
/// [`settings_captured`], all of it again at other settings.
fn step_response(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    let is_step = |p: &Probe| p.area == Area::StepResponse;
    let first = probes.iter().position(is_step);
    let last = probes.iter().rposition(is_step);

    steps.push(click(LOG_VIEWER, "log", "Step Resp Tool", log_at::STEP));
    steps.push(Step {
        wait_s: Some(900),
        ..settle(STEP, None, 2.0)
    });
    steps.push(shot(STEP, format!("r{round}-step-open")));
    for (row, probe) in probes.iter().enumerate().filter(|(_, p)| is_step(p)) {
        let log = StepLog {
            round,
            id: probe.id,
        };
        let y = log_row(step_at::LOG.1, row);
        steps.push(step_click(
            &format!("log {}", probe.id),
            (step_at::LOG.0, y),
        ));
        steps.push(pause(300));
        log.plots(steps, "");
        // where the window is set back to how it came up, the Run of what
        // follows draws the plots anew; where nothing follows, one is
        // asked for
        let is_last = last == Some(row);
        let smoothed = smoothing_captured(probe);
        if round <= RULE_ROUNDS && first == Some(row) {
            log.window_lengths(steps, is_last && !smoothed);
        }
        if smoothed {
            log.smoothing_levels(steps);
        }
        let again = settings_captured(probe);
        if !again.is_empty() {
            log.settings(steps, again, is_last);
        }
    }
}

/// What the plan of a round of the step response does, for the reader of
/// `steps.json`.
fn step_about(round: u32, probes: &[Probe]) -> String {
    let logs = |count: usize| match count {
        1 => "one log".to_string(),
        2 => "two logs".to_string(),
        n => format!("{n} logs"),
    };
    let pairs = "the three other pairs of plots the window offers (rise time and bandwidth, \
                 sensitivity, phase margin and crossover)";
    let more = if round <= RULE_ROUNDS {
        let smoothed = probes.iter().filter(|p| smoothing_captured(p)).count();
        format!(
            "the delay between setpoint and gyro, {pairs}, for the first log its nine plots \
             from windows of one and of three seconds, and for {} the smoothing levels",
            logs(smoothed)
        )
    } else {
        let again = probes.iter().filter(|p| !settings_captured(p).is_empty());
        format!(
            "{pairs}, and for {} all of them again from windows of one and of three seconds \
             and at the smoothing level high",
            logs(again.count())
        )
    };
    format!(
        "Round {round}: every log through one open dialog, then the step response of each: \
         its nine plots, {more}, all copied as vector graphics."
    )
}

/// Name of a picture or of a copied plot of the window of frequency against
/// throttle: `what` is the setting ("" as the window comes up, "psd", "2d",
/// "smoothing-low" …) and `plot` the plot ("" for a picture of the window).
pub fn throttle_file(round: u32, probe: &str, what: &str, plot: &str) -> String {
    let mut name = format!("r{round}-{probe}-throttle");
    for part in [what, plot] {
        if !part.is_empty() {
            name.push('-');
            name.push_str(part);
        }
    }
    name
}

/// Whether the maps of `probe` are also captured at the four smoothing
/// levels the window does not come up with: the stairs, for the spread of
/// a column next to its neighbours, and the held throttle, for the spread
/// of one column alone.
pub fn throttle_smoothing_captured(probe: &Probe) -> bool {
    ["z1-stairs-2k", "z3-hold-2k"].contains(&probe.id)
}

/// Whether the maps of `probe` are also captured on a wider colour scale
/// and below 100 Hz: the held throttle, whose one spot is then inside the
/// scale, and drawn large.
pub fn throttle_views_captured(probe: &Probe) -> bool {
    probe.id == "z3-hold-2k"
}

/// A click in the window of frequency against throttle, at a position of a
/// picture of it.
fn throttle_click(name: &str, at: (u32, u32)) -> Step {
    click(THROTTLE, "shot", name, at)
}

/// An item of a list of that window: a click on the list, a moment for it
/// to open, a click on the item.
fn throttle_pick(steps: &mut Vec<Step>, list: (u32, u32), name: &str, row: usize) {
    steps.push(throttle_click(&format!("{name} (the list)"), list));
    steps.push(pause(600));
    steps.push(throttle_click(name, throttle_at::item(list, row)));
    steps.push(pause(300));
}

/// The log of a round whose maps are being captured.
#[derive(Clone, Copy)]
struct ThrottleLog<'a> {
    round: u32,
    id: &'a str,
}

impl ThrottleLog<'_> {
    /// A Run, waited for, a picture of the window and copies of the six
    /// plots of the first two columns, all named after the setting `what`.
    fn capture(self, steps: &mut Vec<Step>, what: &str) {
        steps.push(Step {
            mark: Some(true),
            ..throttle_click("Run", throttle_at::RUN)
        });
        steps.push(settle(THROTTLE, Some(60.0), 1.5));
        steps.push(shot(THROTTLE, throttle_file(self.round, self.id, what, "")));
        for (column, left, right) in throttle_at::COLUMNS {
            for (axis, (top, bottom)) in throttle_at::ROWS.into_iter().enumerate() {
                let name = format!("{} {column}", AXES[axis]);
                let plot = format!("{}-{column}", AXES[axis]);
                let file = throttle_file(self.round, self.id, what, &plot);
                copy_axes(steps, THROTTLE, &name, (left, top, right, bottom), file);
            }
        }
    }

    /// The maps with the switch at `at` on, and the switch off again.
    fn switched(self, steps: &mut Vec<Step>, name: &str, at: (u32, u32), what: &str) {
        steps.push(throttle_click(&format!("{name} on"), at));
        steps.push(pause(300));
        self.capture(steps, what);
        steps.push(throttle_click(&format!("{name} off"), at));
        steps.push(pause(300));
    }

    /// The maps at the four other smoothing levels, and "medium" again.
    fn smoothing_levels(self, steps: &mut Vec<Step>) {
        let pick = |steps: &mut Vec<Step>, row: usize| {
            let name = format!("smoothing {}", THROTTLE_SMOOTHING[row]);
            throttle_pick(steps, throttle_at::SMOOTHING_LIST, &name, row);
        };
        for (row, level) in THROTTLE_SMOOTHING.into_iter().enumerate() {
            if row != THROTTLE_MEDIUM {
                pick(steps, row);
                self.capture(steps, &format!("smoothing-{level}"));
            }
        }
        pick(steps, THROTTLE_MEDIUM);
    }

    /// The maps on the colour scale [`THROTTLE_SCALE`], then below 100 Hz,
    /// and both as the window came up again.
    fn views(self, steps: &mut Vec<Step>) {
        let scale = |steps: &mut Vec<Step>, text: &str| {
            for (k, at) in throttle_at::SCALES.into_iter().enumerate() {
                steps.push(Step {
                    action: "type",
                    text: Some(text.to_string()),
                    ..throttle_click(&format!("scale of column {}", k + 1), at)
                });
                steps.push(pause(300));
            }
        };
        scale(steps, THROTTLE_SCALE);
        self.capture(steps, "scale-5");
        scale(steps, THROTTLE_SCALE_DEFAULT);
        let below = |steps: &mut Vec<Step>, state: &str| {
            for (k, at) in throttle_at::BELOW_100.into_iter().enumerate() {
                let name = format!("<100Hz of column {} {state}", k + 1);
                steps.push(throttle_click(&name, at));
                steps.push(pause(300));
            }
        };
        below(steps, "on");
        self.capture(steps, "sub100");
        below(steps, "off");
    }
}

/// The steps of the window of frequency against throttle for the probes of
/// a round. The window is opened from the spectrum window, after a Run
/// there. For every log: the log chosen in the first two columns ("Gyro
/// prefilt" and "Gyro"), then a Run, a picture and copies of the six plots
/// as the window comes up, the same with "PSD" on and with "2D" on; for
/// the logs of [`throttle_smoothing_captured`] also at the other smoothing
/// levels, and for those of [`throttle_views_captured`] on a wider colour
/// scale and below 100 Hz.
fn throttle_maps(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    steps.push(click(
        LOG_VIEWER,
        "log",
        "Spectral Analyzer",
        log_at::SPECTRUM,
    ));
    steps.push(Step {
        wait_s: Some(900),
        ..settle(SPECTRUM, None, 1.5)
    });
    steps.push(Step {
        mark: Some(true),
        ..click(SPECTRUM, "shot", "Run", spectrum_at::RUN)
    });
    steps.push(settle(SPECTRUM, Some(60.0), 1.5));
    steps.push(shot(SPECTRUM, format!("r{round}-spectrum-run")));
    steps.push(click(
        SPECTRUM,
        "shot",
        "Freq x Throttle",
        throttle_at::OPEN,
    ));
    steps.push(Step {
        wait_s: Some(120),
        ..settle(THROTTLE, None, 1.5)
    });
    steps.push(shot(THROTTLE, format!("r{round}-throttle-open")));
    let is_throttle = |p: &Probe| p.area == Area::Throttle;
    for (row, probe) in probes.iter().enumerate().filter(|(_, p)| is_throttle(p)) {
        let log = ThrottleLog {
            round,
            id: probe.id,
        };
        for (k, list) in throttle_at::LOG_LISTS.into_iter().enumerate() {
            let name = format!("log {} in column {}", probe.id, k + 1);
            throttle_pick(steps, list, &name, row);
        }
        log.capture(steps, "");
        log.switched(steps, "PSD", throttle_at::PSD, "psd");
        log.switched(steps, "2D", throttle_at::TWO_D, "2d");
        if throttle_smoothing_captured(probe) {
            log.smoothing_levels(steps);
        }
        if throttle_views_captured(probe) {
            log.views(steps);
        }
    }
}

/// What the plan of a round of the map of frequency against throttle does,
/// for the reader of `steps.json`.
fn throttle_about(round: u32, probes: &[Probe]) -> String {
    let levels = probes
        .iter()
        .filter(|p| throttle_smoothing_captured(p))
        .count();
    format!(
        "Round {round}: every log through one open dialog, then the window of frequency \
         against throttle for each: the maps of the unfiltered and of the filtered gyro as \
         the window comes up, in decibels (\"PSD\") and as lines (\"2D\"), for {levels} logs \
         at the other smoothing levels, and for one on a wider colour scale and below 100 Hz; \
         a picture of the window and a vector copy of every plot each time."
    )
}

/// Name of the picture of `probe` on the spectrum at a smoothing level.
pub fn spectrum_shot(round: u32, probe: &str, smoothing: &str) -> String {
    format!("r{round}-{probe}-spectrum-{smoothing}")
}

/// The plan of a round: load every log at once, then for every spectrum
/// probe the spectrum at each smoothing level, as a picture and, from round
/// 2 on, as vector copies of its plots.
pub fn plan(round: u32, probes: &[Probe]) -> Plan {
    // the main window: all logs through one "open" dialog; decoding takes
    // several seconds a log
    let mut steps = vec![
        Step {
            mark: Some(true),
            ..click(LOG_VIEWER, "log", "Open File", log_at::OPEN_FILE)
        },
        Step {
            action: "open_file",
            paths: Some(probes.iter().map(|p| format!("{}.bbl", p.id)).collect()),
            ..Step::default()
        },
        settle(LOG_VIEWER, Some(900.0), 6.0),
        Step {
            action: "windows",
            ..Step::default()
        },
        shot(LOG_VIEWER, format!("r{round}-log-loaded")),
    ];

    // the spectrum window, when the round has probes for it
    if probes.iter().any(|p| p.area == Area::Spectrum) {
        steps.push(click(
            LOG_VIEWER,
            "log",
            "Spectral Analyzer",
            log_at::SPECTRUM,
        ));
        // the tool may still be decoding when the click lands: it then opens the
        // window once it is done, however long that takes
        steps.push(Step {
            wait_s: Some(900),
            ..settle(SPECTRUM, None, 1.5)
        });
        steps.push(shot(SPECTRUM, format!("r{round}-spectrum-open")));
        if round >= 3 {
            steps.push(Step {
                action: "type",
                text: Some(Y_MAX_DB.to_string()),
                ..click(SPECTRUM, "shot", "Y max", spectrum_at::Y_MAX)
            });
            steps.push(pause(300));
        }
        let spectrum: Vec<&Probe> = probes.iter().filter(|p| p.area == Area::Spectrum).collect();
        for probe in spectrum {
            // the logs are listed in the order of their names, which is the
            // order of the catalogue; the legend of every picture names its log
            let row = probes.iter().position(|p| p.id == probe.id).unwrap_or(0);
            let at = |name: &str, position: (u32, u32)| click(SPECTRUM, "shot", name, position);
            for (k, (level, item)) in SMOOTHING.into_iter().enumerate() {
                if k == 0 {
                    // a click selects this log alone
                    let y = log_row(spectrum_at::LOG.1, row);
                    steps.push(Step {
                        mark: Some(true),
                        ..at(&format!("log {}", probe.id), (spectrum_at::LOG.0, y))
                    });
                    steps.push(pause(250));
                    steps.push(at("smoothing list", spectrum_at::SMOOTHING_LIST));
                } else {
                    steps.push(Step {
                        mark: Some(true),
                        ..at("smoothing list", spectrum_at::SMOOTHING_LIST)
                    });
                }
                steps.push(pause(600));
                let name = format!("smoothing {level}");
                steps.push(at(&name, (spectrum_at::SMOOTHING_X, item)));
                steps.push(pause(200));
                steps.push(at("Run", spectrum_at::RUN));
                steps.push(settle(SPECTRUM, Some(20.0), 1.2));
                steps.push(shot(SPECTRUM, spectrum_shot(round, probe.id, level)));
                for (axis, column) in copied(round, level) {
                    let file = spectrum_copy(round, probe.id, level, axis, column);
                    copy_plot(&mut steps, axis, column, file);
                }
            }
        }
    }
    if probes.iter().any(|p| p.area == Area::StepResponse) {
        step_response(&mut steps, round, probes);
    }
    if probes.iter().any(|p| p.area == Area::Throttle) {
        throttle_maps(&mut steps, round, probes);
    }
    steps.push(Step {
        action: "windows",
        ..Step::default()
    });
    Plan {
        about: if probes.iter().all(|p| p.area == Area::StepResponse) {
            step_about(round, probes)
        } else if probes.iter().all(|p| p.area == Area::Throttle) {
            throttle_about(round, probes)
        } else if round < 2 {
            format!(
                "Round {round}: every log through one open dialog, then the spectrum of each \
                 at the five smoothing levels."
            )
        } else {
            format!(
                "Round {round}: every log through one open dialog, then the spectrum of each \
                 at the five smoothing levels, every plot below 100 Hz copied as a vector \
                 graphic."
            )
        },
        // the plots of the window of frequency against throttle had never
        // been copied when its first round was planned
        give_up: probes.iter().any(|p| p.area == Area::Throttle),
        refs: Frames {
            log: Frame {
                left: 1,
                top: 47,
                width: 3776,
                height: 2424,
            },
            shot: Frame {
                left: 0,
                top: 0,
                width: 3780,
                height: 2427,
            },
        },
        steps,
    }
}

/// Text with the line ends Windows tools expect.
fn crlf(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\n")
        .replace('\n', "\r\n")
        .into_bytes()
}

/// The files that make a pack runnable on the owner's machine: the script,
/// its launcher and a note on how to run it.
pub fn files(round: u32, shots: usize, copies: usize, predicted: bool) -> Vec<(String, Vec<u8>)> {
    let launcher = "@echo off\n\
                    cd /d \"%~dp0\"\n\
                    powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0capture.ps1\" \
                    -Steps \"steps.json\"\n\
                    echo.\n\
                    pause\n";
    let saves = if copies == 0 {
        format!("saves {shots} pictures of the\n   tool's windows")
    } else {
        // a picture with its Run takes about five seconds, a copy six
        // (round 4, on version 0.9 of the tool: 20 pictures and 153 copies
        // in 17.5 minutes)
        let minutes = (shots * 5 + copies * 6).div_ceil(60) + 1;
        format!(
            "saves {shots} pictures of the\n   \
             tool's windows and {copies} plots copied as vector graphics, in about\n   \
             {minutes} minutes"
        )
    };
    let clipboard = if copies == 0 {
        ""
    } else {
        "The copied plots replace what was on the clipboard.\n\n"
    };
    let predictions = if predicted {
        " predictions.json is what the\nmodel of the reference tool expects it to show;"
    } else {
        ""
    };
    let note = format!(
        "Calibration round {round}, for version 0.94 of the reference tool\n\
         \n\
         1. Unpack everything into one folder.\n\
         2. Start version 0.94 of the reference tool anew and wait for its main window\n   \
         (Log Viewer). Open nothing in it and leave the window as it comes up. The\n   \
         script clicks where that version has its buttons; it fits no other.\n\
         3. Double-click run.cmd. After five seconds the script takes the mouse and the\n   \
         keyboard: it loads the logs of this folder and {saves}.\n   \
         Leave both alone until it beeps twice.\n\
         4. Send back capture-results.zip, which appears next to the script.\n\
         \n\
         To stop it, move the mouse: it stops before its next move.\n\
         \n\
         {clipboard}\
         What the files are: *.bbl are the probe logs; manifest.json says what is true\n\
         about each; ours.json is what OpenPID computes for them;{predictions} steps.json\n\
         is what the script does, step by step; capture.ps1 is the script.\n"
    );
    vec![
        ("capture.ps1".to_string(), crlf(SCRIPT)),
        ("run.cmd".to_string(), crlf(launcher)),
        ("README.txt".to_string(), crlf(&note)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue;

    #[test]
    fn round_1_takes_the_spectrum_of_every_log_at_every_level() {
        let probes = catalogue::round(1).unwrap_or_default();
        let plan = plan(1, &probes);
        let shots: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == "shot")
            .filter_map(|s| s.file.as_deref())
            .collect();
        assert_eq!(shots.len(), 2 + probes.len() * 5);
        assert_eq!(shots[0], "r1-log-loaded");
        assert_eq!(shots[1], "r1-spectrum-open");
        assert_eq!(shots[2], "r1-s1-levels-2k-spectrum-low");
        assert_eq!(shots[6], "r1-s1-levels-2k-spectrum-medium");
        assert_eq!(
            *shots.last().unwrap_or(&""),
            "r1-s9-bands-2k-spectrum-medium"
        );
        let mut unique = shots.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len(), "a picture is saved twice");

        // one dialog for all logs, in the order of the catalogue
        let open: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.action == "open_file")
            .collect();
        assert_eq!(open.len(), 1);
        let paths = open[0].paths.clone().unwrap_or_default();
        assert_eq!(paths.len(), probes.len());
        assert_eq!(paths[0], "s1-levels-2k.bbl");
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(sorted, paths, "the list of the tool is sorted by name");

        // every Run is waited for against a mark taken before the change
        let mut marked = false;
        for step in &plan.steps {
            marked |= step.mark == Some(true);
            if step.action == "settle" && step.change_s.is_some() {
                assert!(marked, "a wait for a change without a mark");
                marked = false;
            }
        }
        // every click is on the control panel at the right of its window:
        // the main window in its reference screenshot, the spectrum window
        // in a picture the script saves
        for step in plan.steps.iter().filter(|s| s.action == "click") {
            let (x, y) = step.at.unwrap_or((0, 0));
            assert!(x > 3300 && x < 3777, "{step:?}");
            assert!(y > 47 && y < 2427, "{step:?}");
            let frame = if step.window == Some(LOG_VIEWER) {
                "log"
            } else {
                "shot"
            };
            assert_eq!(step.frame, Some(frame), "{step:?}");
        }
        let at = |name: &str| {
            let step = plan.steps.iter().find(|s| s.name.as_deref() == Some(name));
            step.and_then(|s| s.at)
        };
        assert_eq!(at("Open File"), Some((3437, 227)));
        assert_eq!(at("Spectral Analyzer"), Some((3556, 871)));
        assert_eq!(at("smoothing list"), Some((3640, 1316)));
        assert_eq!(at("Run"), Some((3468, 996)));
        // the rows of the log list: 37.9 pixels apart
        let rows: Vec<u32> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log ")))
            .filter_map(|s| s.at.map(|a| a.1))
            .collect();
        assert_eq!(rows, [133, 170, 208, 246, 284, 322, 360, 398, 436]);
        assert_eq!(at("log s1-levels-2k"), Some((3515, 133)));
        // the five rows of the smoothing list
        let levels: Vec<(String, u32)> = plan
            .steps
            .iter()
            .filter(|s| {
                s.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("smoothing "))
            })
            .filter(|s| s.name.as_deref() != Some("smoothing list"))
            .take(5)
            .filter_map(|s| Some((s.name.clone()?, s.at.filter(|a| a.0 == 3591)?.1)))
            .collect();
        let expected = [
            ("smoothing low", 1360),
            ("smoothing low-med", 1407),
            ("smoothing med-high", 1502),
            ("smoothing high", 1550),
            ("smoothing medium", 1455),
        ];
        for (got, want) in levels.iter().zip(expected) {
            assert_eq!((got.0.as_str(), got.1), want);
        }
        assert_eq!(levels.len(), 5);
    }

    #[test]
    fn round_2_copies_the_plots_as_vector_graphics() {
        let probes = catalogue::round(2).unwrap_or_default();
        let plan = plan(2, &probes);
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        let shots = names("shot");
        assert_eq!(shots.len(), 2 + probes.len() * 5);
        let copies = names("clip_save");
        // below 100 Hz at every level, the whole range at the default one
        assert_eq!(copies.len(), probes.len() * (5 * 3 + 3));
        assert_eq!(copies[0], "r2-c1-mixed-2k-spectrum-low-roll-sub");
        assert_eq!(copies[2], "r2-c1-mixed-2k-spectrum-low-yaw-sub");
        assert_eq!(
            *copies.last().unwrap_or(&""),
            "r2-c6-short-8k-spectrum-medium-yaw-full"
        );
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());

        // a copy is tried from both states of the plot's toolbar
        let steps = &plan.steps;
        for (i, step) in steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(steps, i);
            }
        }
        // the pitch plot of the whole range: the button above the right
        // corner of its axes, the export icon left of it, the third item
        // of the menu under the icon
        let pitch = steps
            .iter()
            .position(|s| s.file.as_deref() == Some("r2-c1-mixed-2k-spectrum-medium-pitch-full"))
            .unwrap_or(11);
        assert_eq!(steps[pitch - 11].at, Some((1648, 860)));
        assert_eq!(steps[pitch - 3].at, Some((1481, 860)));
        assert_eq!(steps[pitch - 1].at, Some((1481, 965)));
        // every action is one the script knows
        for step in steps {
            assert!(
                SCRIPT.contains(&format!("$action -eq \"{}\"", step.action)),
                "{}",
                step.action
            );
        }
        // round 1 copies nothing
        let first = super::plan(1, &catalogue::round(1).unwrap_or_default());
        let copying = ["hover", "clip_wait", "clip_save"];
        assert!(first.steps.iter().all(|s| !copying.contains(&s.action)));
    }

    /// What the steps around the `clip_save` at `i` have to be: the button
    /// of the plot's toolbar, a try at the copy, the wait that passes over
    /// the second try, the button and the try again, the save, the button.
    fn assert_copy(steps: &[Step], i: usize) {
        let around: Vec<&str> = steps[i - 11..=i + 2].iter().map(|s| s.action).collect();
        assert_eq!(
            around,
            [
                "click",
                "hover",
                "click",
                "hover",
                "click",
                "clip_wait",
                "click",
                "hover",
                "click",
                "hover",
                "click",
                "clip_save",
                "click",
                "pause"
            ]
        );
        // both tries are the same four steps, and the wait passes over the
        // button and the second of them, up to the save
        let (first, second) = (&steps[i - 10..i - 6], &steps[i - 4..i]);
        assert_eq!(first, second);
        let wait = &steps[i - 6];
        assert_eq!((wait.wait_s, wait.skip), (Some(3), Some(5)));
        assert_eq!(steps[i].wait_s, Some(6));
        // the pointer rests on the icon and on the item before it clicks
        let [over_icon, icon, over_item, item] = second else {
            panic!("{second:?}");
        };
        assert_eq!((over_icon.at, over_icon.ms), (icon.at, Some(500)));
        assert_eq!((over_item.at, over_item.ms), (item.at, Some(500)));
        assert_eq!((icon.clip, item.clip), (None, Some(true)));
        // the button opens the toolbar, opens it again when the first click
        // closed it, and closes it at the end
        let dots = [&steps[i - 11], &steps[i - 5], &steps[i + 1]];
        assert!(dots.iter().all(|s| s.at == dots[0].at));
        let fronts = dots.map(|s| s.front);
        assert_eq!(fronts, [None, Some(false), Some(false)]);
        let close = dots[2].name.as_deref().unwrap_or("");
        assert!(close.ends_with("toolbar (close)"), "{close}");
        // the icon is left of the button on its row, the item under the icon
        let (dx, dy) = dots[0].at.unwrap_or((0, 0));
        let (ix, iy) = icon.at.unwrap_or((0, 0));
        assert_eq!((ix + 167, iy), (dx, dy));
        assert_eq!(item.at, Some((ix, iy + 105)));
        for s in steps[i - 11..i].iter().filter(|s| s.action != "clip_wait") {
            assert_eq!(s.frame, Some("shot"), "{s:?}");
            let (x, y) = s.at.unwrap_or((0, 0));
            assert!(x < 3780 && y < 2427, "{s:?}");
        }
        assert!(second.iter().all(|s| s.front == Some(false)));
    }

    #[test]
    fn round_3_raises_the_top_of_the_plots_first() {
        let probes = catalogue::round(3).unwrap_or_default();
        let plan = plan(3, &probes);
        let typed: Vec<&Step> = plan.steps.iter().filter(|s| s.action == "type").collect();
        assert_eq!(typed.len(), 1);
        assert_eq!(typed[0].text.as_deref(), Some("70"));
        assert_eq!(
            (typed[0].frame, typed[0].at),
            (Some("shot"), Some((3610, 1612)))
        );
        let at = |action: &str| plan.steps.iter().position(|s| s.action == action);
        // after the window is up, before the first Run
        assert!(at("type") > at("shot") && at("type") < at("clip_save"));
        let copies = plan
            .steps
            .iter()
            .filter(|s| s.action == "clip_save")
            .count();
        assert_eq!(copies, probes.len() * 18);
        assert!(SCRIPT.contains("$action -eq \"type\""));
        // rounds 1 and 2 stay as they were run
        for round in [1, 2] {
            let earlier = super::plan(round, &catalogue::round(round).unwrap_or_default());
            assert!(earlier.steps.iter().all(|s| s.action != "type"));
        }
    }

    #[test]
    fn round_4_copies_the_plots_of_the_step_response_window() {
        let probes = catalogue::round(4).unwrap_or_default();
        let plan = plan(4, &probes);
        // nothing of the spectrum window
        assert!(plan.steps.iter().all(|s| s.window != Some(SPECTRUM)));
        assert!(plan.about.contains("step response"));
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        let copies = names("clip_save");
        // a log: nine plots, three delays and three pairs of six plots;
        // nine plots twice more for the first log; and nine smoothed
        // responses for two logs
        assert_eq!(copies.len(), 9 * 30 + 2 * 9 + 2 * 9);
        let of_the_first_log = [
            (0, "roll-response"),
            (3, "roll-peak"),
            (8, "yaw-latency"),
            (9, "spgyro-roll-latency"),
            (12, "params-roll-risetime"),
            (15, "params-roll-bandwidth"),
            (18, "params-roll-sensitivity-peak"),
            (21, "params-roll-sensitivity-freq"),
            (24, "params-roll-phase-margin"),
            (29, "params-yaw-crossover"),
            (30, "window-1s-roll-response"),
            (38, "window-1s-yaw-latency"),
            (39, "window-3s-roll-response"),
            (48, "smoothing-low-roll-response"),
        ];
        for (k, name) in of_the_first_log {
            assert_eq!(copies[k], format!("r4-t1-plants-2k-step-{name}"));
        }
        assert_eq!(copies[57], "r4-t2-plants-1k-step-roll-response");
        assert!(!copies.iter().any(|c| c.contains("-raw-")));
        let windows = |c: &&&str| c.contains("-window-");
        assert!(copies.iter().filter(windows).all(|c| c.contains("t1")));
        assert!(copies.contains(&"r4-t5-noise-2k-step-smoothing-high-yaw-response"));
        assert!(
            !copies
                .iter()
                .any(|c| c.starts_with("r4-t2") && c.contains("smoothing"))
        );
        // a picture of the window as it opens, four a log, and two more
        // of the first log
        let shots = names("shot");
        assert_eq!(shots.len(), 2 + 9 * 4 + 2);
        assert_eq!(
            shots[2..9],
            [
                "r4-t1-plants-2k-step",
                "r4-t1-plants-2k-step-rise-bandwidth",
                "r4-t1-plants-2k-step-sensitivity",
                "r4-t1-plants-2k-step-phase-crossover",
                "r4-t1-plants-2k-step-window-1s",
                "r4-t1-plants-2k-step-window-3s",
                "r4-t2-plants-1k-step",
            ]
        );
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());

        // where a copy clicks: the button of the roll response plot, its
        // export icon and the item of its menu
        let find = |file: &str| {
            plan.steps
                .iter()
                .position(|s| s.file.as_deref() == Some(file))
                .unwrap_or(11)
        };
        let roll = find("r4-t1-plants-2k-step-roll-response");
        assert_eq!(plan.steps[roll - 11].at, Some((1686, 157)));
        assert_eq!(plan.steps[roll - 3].at, Some((1519, 157)));
        assert_eq!(plan.steps[roll - 1].at, Some((1519, 262)));
        let latency = find("r4-t1-plants-2k-step-yaw-latency");
        assert_eq!(plan.steps[latency - 3].at, Some((2993, 1588)));
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        assert_controls_of_the_step_window(&plan);
        for step in &plan.steps {
            // every step of that window is given in the script's own pictures
            if step.window == Some(STEP) && step.at.is_some() {
                assert_eq!(step.frame, Some("shot"), "{step:?}");
                let (x, y) = step.at.unwrap_or((0, 0));
                assert!(x < 3780 && y < 2427, "{step:?}");
            }
            assert!(
                SCRIPT.contains(&format!("$action -eq \"{}\"", step.action)),
                "{}",
                step.action
            );
        }
        // the logs of the list, 37.9 pixels apart
        let rows: Vec<u32> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log t")))
            .filter_map(|s| s.at.map(|a| a.1))
            .collect();
        assert_eq!(rows, [177, 214, 252, 290, 328, 366, 404, 442, 480]);
        // every Run is waited for against a mark
        let runs = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref() == Some("Run"))
            .count();
        assert_eq!(runs, 9 * 5 + 2 + 2 * 3);
        assert!(
            plan.steps
                .iter()
                .filter(|s| s.name.as_deref() == Some("Run"))
                .all(|s| s.mark == Some(true))
        );
    }

    /// Where the controls of the step response window are clicked, and the
    /// pairs of plots a log goes through.
    fn assert_controls_of_the_step_window(plan: &Plan) {
        let at = |name: &str| {
            let step = plan.steps.iter().find(|s| s.name.as_deref() == Some(name));
            step.and_then(|s| s.at)
        };
        assert_eq!(at("Step Resp Tool"), Some((3556, 962)));
        assert_eq!(at("Run"), Some((3466, 821)));
        // the plots of every window ("Raw") are captured no more
        assert_eq!(at("Raw"), None);
        assert_eq!(at("smoothing low (the list)"), Some((3464, 1093)));
        assert_eq!(at("smoothing low"), Some((3464, 1184)));
        assert_eq!(at("smoothing high"), Some((3464, 1278)));
        assert_eq!(at("smoothing off"), Some((3464, 1136)));
        // the list of pairs: for every log the delay, the three other
        // pairs, and the first pair again, which the window came up with
        let pairs: Vec<(&str, (u32, u32))> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.contains(" | ")))
            .filter_map(|s| Some((s.name.as_deref()?, s.at?)))
            .collect();
        let list = (3640, 1093);
        assert_eq!(
            pairs[..10],
            [
                ("SR Latency | SP-Gyro Delay (the list)", list),
                ("SR Latency | SP-Gyro Delay", (3515, 1184)),
                ("Rise Time | Bandwidth (the list)", list),
                ("Rise Time | Bandwidth", (3515, 1231)),
                ("Sensitivity Peak | Sensitivity Freq (the list)", list),
                ("Sensitivity Peak | Sensitivity Freq", (3515, 1278)),
                ("Phase Margin | Crossover Freq (the list)", list),
                ("Phase Margin | Crossover Freq", (3515, 1326)),
                ("SR Peak | SR Latency (the list)", list),
                ("SR Peak | SR Latency", (3515, 1136)),
            ]
        );
        assert_eq!(pairs.len() % 10, 0);
        // the length of the window: one second, three, and two again, for
        // the first log only
        let lengths: Vec<(&str, (u32, u32))> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.contains("deconv.")))
            .filter_map(|s| Some((s.name.as_deref()?, s.at?)))
            .collect();
        let list = (3553, 1563);
        assert_eq!(
            lengths,
            [
                ("1sec deconv. window (the list)", list),
                ("1sec deconv. window", (3553, 1608)),
                ("3sec deconv. window (the list)", list),
                ("3sec deconv. window", (3553, 1703)),
                ("2sec deconv. window (the list)", list),
                ("2sec deconv. window", (3553, 1656)),
            ]
        );
        // an open list is given 600 ms before its item is clicked
        for (i, step) in plan.steps.iter().enumerate() {
            if step
                .name
                .as_deref()
                .is_some_and(|n| n.ends_with("(the list)"))
            {
                assert_eq!(plan.steps[i + 1].ms, Some(600), "{step:?}");
            }
        }
    }

    #[test]
    fn round_6_copies_the_smoothing_levels_of_one_log() {
        let probes = catalogue::round(6).unwrap_or_default();
        let plan = plan(6, &probes);
        assert!(plan.about.contains("for one log the smoothing levels"));
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // 30 plots a log (nine as the window comes up, three delays,
        // three pairs of six); the nine plots of the first log from
        // windows of one and of three seconds; nine smoothed responses for
        // the last log only
        let copies = names("clip_save");
        assert_eq!(copies.len(), 5 * 30 + 2 * 9 + 9);
        for (k, name) in [
            (0, "w1-flight-2k-step-roll-response"),
            (11, "w1-flight-2k-step-spgyro-yaw-latency"),
            (29, "w1-flight-2k-step-params-yaw-crossover"),
            (30, "w1-flight-2k-step-window-1s-roll-response"),
            (47, "w1-flight-2k-step-window-3s-yaw-latency"),
            (48, "w2-flight-1k-step-roll-response"),
            (168, "w5-mixed-2k-step-smoothing-low-roll-response"),
            (176, "w5-mixed-2k-step-smoothing-high-yaw-response"),
        ] {
            assert_eq!(copies[k], format!("r6-{name}"));
        }
        let smoothed = copies.iter().filter(|c| c.contains("smoothing"));
        assert!(smoothed.clone().all(|c| c.contains("w5")));
        assert_eq!(smoothed.count(), 9);
        // the window as it opens, four pictures a log, two more of the
        // first log
        let shots = names("shot");
        assert_eq!(shots.len(), 2 + 5 * 4 + 2);
        assert!(shots.contains(&"r6-w1-flight-2k-step-window-3s"));
        assert_eq!(plan.steps.len(), 2707);
        // the window is two seconds again before the second log is run
        let position = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted)
        };
        assert!(position("2sec deconv. window") < position("log w2-flight-1k"));
        assert_controls_of_the_step_window(&plan);
        // the five logs of the list, in the order of their names
        let rows: Vec<u32> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log w")))
            .filter_map(|s| s.at.map(|a| a.1))
            .collect();
        assert_eq!(rows, [177, 214, 252, 290, 328]);
        // the script's own estimate of the time
        let note = files(6, shots.len(), copies.len(), true).remove(2).1;
        let note = String::from_utf8(note).unwrap_or_default();
        assert!(note.contains("saves 24 pictures"), "{note}");
        assert!(
            note.contains("177 plots") && note.contains("21 minutes"),
            "{note}"
        );
        // a round of one log, without smoothing levels: nothing runs after
        // the window is two seconds again, so the plan runs it
        let alone = super::plan(6, &probes[..1]);
        let runs = |plan: &Plan| {
            let run = |s: &&Step| s.name.as_deref() == Some("Run");
            plan.steps.iter().filter(run).count()
        };
        assert_eq!(runs(&alone), 5 + 2 + 1);
        assert_eq!(runs(&plan), 5 * 5 + 2 + 3);
        // the earlier step-response rounds capture the levels of two logs
        for round in [4, 5] {
            let earlier = super::plan(round, &catalogue::round(round).unwrap_or_default());
            assert!(earlier.about.contains("for two logs the smoothing levels"));
        }
    }

    #[test]
    fn round_7_copies_the_pairs_of_plots_and_one_log_at_other_settings() {
        let probes = catalogue::round(7).unwrap_or_default();
        let plan = plan(7, &probes);
        assert!(plan.about.contains("for one log all of them again"));
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // 27 plots a log (nine as the window comes up, three pairs of
        // six), and as many again at each of three settings for one log
        let copies = names("clip_save");
        assert_eq!(copies.len(), 8 * 27 + 3 * 27);
        for (k, name) in [
            (0, "y1-delays-2k-step-roll-response"),
            (9, "y1-delays-2k-step-params-roll-risetime"),
            (26, "y1-delays-2k-step-params-yaw-crossover"),
            (27, "y2-first-2k-step-roll-response"),
            (81, "y4-damping-2k-step-roll-response"),
            (108, "y4-damping-2k-step-window-1s-roll-response"),
            (117, "y4-damping-2k-step-window-1s-params-roll-risetime"),
            (135, "y4-damping-2k-step-window-3s-roll-response"),
            (162, "y4-damping-2k-step-smoothing-high-roll-response"),
            (
                188,
                "y4-damping-2k-step-smoothing-high-params-yaw-crossover",
            ),
            (189, "y5-rate-1k-step-roll-response"),
            (296, "y8-noise-2k-step-params-yaw-crossover"),
        ] {
            assert_eq!(copies[k], format!("r7-{name}"));
        }
        // neither every window's response nor the delay between setpoint
        // and gyro, and no smoothing level but the one of that log
        let no = ["-raw-", "-spgyro-", "smoothing-low", "smoothing-medium"];
        assert!(!copies.iter().any(|c| no.iter().any(|n| c.contains(n))));
        let settings = |c: &&&str| c.contains("-window-") || c.contains("-smoothing-");
        let again: Vec<&&str> = copies.iter().filter(settings).collect();
        assert_eq!(again.len(), 3 * 27);
        assert!(again.iter().all(|c| c.contains("y4-damping-2k")));
        // four pictures a log and at each of the settings
        let shots = names("shot");
        assert_eq!(shots.len(), 2 + 8 * 4 + 3 * 4);
        assert_eq!(
            shots[18..22],
            [
                "r7-y4-damping-2k-step-window-1s",
                "r7-y4-damping-2k-step-window-1s-rise-bandwidth",
                "r7-y4-damping-2k-step-window-1s-sensitivity",
                "r7-y4-damping-2k-step-window-1s-phase-crossover",
            ]
        );
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        assert_eq!(plan.steps.len(), 4511);
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }

        // the lists: three pairs a log and the first one again; and for the
        // one log the window of one second, of three, of two again, then
        // the smoothing level and no smoothing again
        let picked = |part: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter_map(|s| s.name.as_deref())
                .filter(|n| n.contains(part) && !n.ends_with("(the list)"))
                .collect()
        };
        let pairs = picked(" | ");
        assert_eq!(pairs.len(), (8 + 3) * 4);
        assert_eq!(
            pairs[..4],
            [
                "Rise Time | Bandwidth",
                "Sensitivity Peak | Sensitivity Freq",
                "Phase Margin | Crossover Freq",
                "SR Peak | SR Latency",
            ]
        );
        let lengths = picked("deconv.");
        assert_eq!(
            lengths,
            [
                "1sec deconv. window",
                "3sec deconv. window",
                "2sec deconv. window"
            ]
        );
        assert_eq!(picked("smoothing "), ["smoothing high", "smoothing off"]);
        let position = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted)
        };
        assert!(position("2sec deconv. window") < position("smoothing high"));
        assert!(position("smoothing off") < position("log y5-rate-1k"));
        assert_eq!(
            plan.steps[position("smoothing high").unwrap_or(0)].at,
            Some((3464, 1278))
        );
        // a Run for every picture but the first two, each against a mark
        let runs: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref() == Some("Run"))
            .collect();
        assert_eq!(runs.len(), shots.len() - 2);
        assert!(runs.iter().all(|s| s.mark == Some(true)));
        // the eight logs of the list
        let rows: Vec<u32> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log y")))
            .filter_map(|s| s.at.map(|a| a.1))
            .collect();
        assert_eq!(rows, [177, 214, 252, 290, 328, 366, 404, 442]);
        // the script's own estimate of the time
        let note = files(7, shots.len(), copies.len(), true).remove(2).1;
        let note = String::from_utf8(note).unwrap_or_default();
        assert!(note.contains("saves 46 pictures"), "{note}");
        assert!(
            note.contains("297 plots") && note.contains("35 minutes"),
            "{note}"
        );
        // the settings of a probe in the names of its files
        assert_eq!(setting_name(DEFAULT_SETTING), "");
        assert_eq!(setting_name((2, 5)), "window-5s-smoothing-medium");
        assert_eq!(joined("", ""), "");
        assert!(settings_captured(&probes[0]).is_empty());
        // one log alone at other settings: a Run after the window is as it
        // came up again, since nothing follows that would draw the plots
        let alone = super::plan(7, &probes[3..4]);
        let last_run = alone
            .steps
            .iter()
            .rposition(|s| s.name.as_deref() == Some("Run"));
        let back = alone
            .steps
            .iter()
            .position(|s| s.name.as_deref() == Some("smoothing off"));
        assert!(back < last_run);
    }

    #[test]
    fn round_8_copies_the_maps_of_the_throttle_window() {
        let probes = catalogue::round(8).unwrap_or_default();
        let plan = plan(8, &probes);
        assert!(plan.about.contains("frequency against throttle"));
        assert!(
            plan.about
                .contains("for 2 logs at the other smoothing levels")
        );
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // six plots at a setting: three settings a log, four smoothing
        // levels more for two logs, two views more for one
        let copies = names("clip_save");
        assert_eq!(copies.len(), 6 * (8 * 3 + 2 * 4 + 2));
        for (k, name) in [
            (0, "z1-stairs-2k-throttle-roll-prefilt"),
            (2, "z1-stairs-2k-throttle-yaw-prefilt"),
            (3, "z1-stairs-2k-throttle-roll-gyro"),
            (6, "z1-stairs-2k-throttle-psd-roll-prefilt"),
            (12, "z1-stairs-2k-throttle-2d-roll-prefilt"),
            (18, "z1-stairs-2k-throttle-smoothing-low-roll-prefilt"),
            (41, "z1-stairs-2k-throttle-smoothing-high-yaw-gyro"),
            (42, "z2-ramp-2k-throttle-roll-prefilt"),
            (60, "z3-hold-2k-throttle-roll-prefilt"),
            (102, "z3-hold-2k-throttle-scale-5-roll-prefilt"),
            (108, "z3-hold-2k-throttle-sub100-roll-prefilt"),
            (114, "z4-fine-2k-throttle-roll-prefilt"),
            (203, "z8-short-2k-throttle-2d-yaw-gyro"),
        ] {
            assert_eq!(copies[k], format!("r8-{name}"));
        }
        // a picture at every setting, after the three of the way there
        let shots = names("shot");
        assert_eq!(shots.len(), 3 + copies.len() / 6);
        assert_eq!(
            shots[..7],
            [
                "r8-log-loaded",
                "r8-spectrum-run",
                "r8-throttle-open",
                "r8-z1-stairs-2k-throttle",
                "r8-z1-stairs-2k-throttle-psd",
                "r8-z1-stairs-2k-throttle-2d",
                "r8-z1-stairs-2k-throttle-smoothing-low",
            ]
        );
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        // the window is opened from the spectrum window, after a Run there
        let position = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted)
        };
        let open = position("Freq x Throttle").unwrap_or(0);
        assert_eq!(plan.steps[open].window, Some(SPECTRUM));
        assert_eq!(plan.steps[open].at, Some((3555, 1078)));
        assert_eq!(plan.steps[open - 3].name.as_deref(), Some("Run"));
        assert_eq!(plan.steps[open - 3].window, Some(SPECTRUM));
        assert_eq!(plan.steps[open + 1].window, Some(THROTTLE));
        // a Run of the window for every picture of it, each against a mark
        let runs: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref() == Some("Run") && s.window == Some(THROTTLE))
            .collect();
        assert_eq!(runs.len(), shots.len() - 3);
        assert!(
            runs.iter()
                .all(|s| s.mark == Some(true) && s.at == Some((3468, 179)))
        );
        // every log is chosen in both columns, a row of the open list each
        let rows = |column: &str| -> Vec<(u32, u32)> {
            plan.steps
                .iter()
                .filter(|s| {
                    s.name
                        .as_deref()
                        .is_some_and(|n| n.starts_with("log z") && n.ends_with(column))
                })
                .filter_map(|s| s.at)
                .collect()
        };
        let first = rows("in column 1");
        assert_eq!(
            first,
            [152, 199, 247, 294, 342, 389, 437, 484].map(|y| (482, y))
        );
        let second: Vec<(u32, u32)> = first.iter().map(|(_, y)| (1319, *y)).collect();
        assert_eq!(rows("in column 2"), second);
        // the switches go on and off again; the smoothing ends at medium
        let picked = |part: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter_map(|s| s.name.as_deref())
                .filter(|n| n.contains(part) && !n.ends_with("(the list)"))
                .collect()
        };
        assert_eq!(picked("PSD o"), ["PSD on", "PSD off"].repeat(8));
        assert_eq!(picked("2D o"), ["2D on", "2D off"].repeat(8));
        assert_eq!(
            picked("smoothing "),
            [
                "smoothing low",
                "smoothing low-med",
                "smoothing med-high",
                "smoothing high",
                "smoothing medium",
            ]
            .repeat(2)
        );
        assert_eq!(
            plan.steps[position("smoothing high").unwrap_or(0)].at,
            Some((3640, 741))
        );
        let typed: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == "type")
            .filter_map(|s| s.text.as_deref())
            .collect();
        assert_eq!(typed, ["5", "5", "0.5", "0.5"]);
        assert_eq!(picked("<100Hz").len(), 4);
        // the toolbar button of the first plot
        let toolbar = position("roll prefilt toolbar").unwrap_or(0);
        assert_eq!(plan.steps[toolbar].at, Some((834, 303)));
        // the script's own estimate of the time
        let note = files(8, shots.len(), copies.len(), false).remove(2).1;
        let note = String::from_utf8(note).unwrap_or_default();
        assert!(note.contains("saves 37 pictures"), "{note}");
        assert!(
            note.contains("204 plots") && note.contains("25 minutes"),
            "{note}"
        );
        assert_eq!(throttle_file(8, "z", "", ""), "r8-z-throttle");
        // a copy that fails gives up the other copies of its picture: all
        // the steps of a copy are marked, and nothing else is
        assert!(plan.give_up);
        let marked = plan.steps.iter().filter(|s| s.copy == Some(true)).count();
        assert_eq!(marked, 14 * copies.len());
        let actions = ["shot", "settle", "type", "open_file", "windows"];
        assert!(
            plan.steps
                .iter()
                .filter(|s| actions.contains(&s.action) || s.name.as_deref() == Some("Run"))
                .all(|s| s.copy.is_none())
        );
        let json = serde_json::to_value(&plan).unwrap_or_default();
        assert_eq!(json["give_up"], true);
        assert!(SCRIPT.contains("$plan.give_up") && SCRIPT.contains("$step.copy"));
        // the rounds that were run before go on after a failed copy
        let earlier = super::plan(7, &catalogue::round(7).unwrap_or_default());
        assert!(!earlier.give_up);
        let json = serde_json::to_value(&earlier).unwrap_or_default();
        assert!(json.get("give_up").is_none());
    }

    #[test]
    fn the_plan_is_what_the_script_reads() {
        let probes = catalogue::round(1).unwrap_or_default();
        let json = serde_json::to_value(plan(1, &probes)).unwrap_or_default();
        assert_eq!(json["refs"]["log"]["width"], 3776);
        assert_eq!(json["refs"]["shot"]["width"], 3780);
        let first = &json["steps"][0];
        assert_eq!(first["action"], "click");
        assert_eq!(first["window"], "Log Viewer");
        assert_eq!(first["ref"], "log");
        assert_eq!(first["at"][0], 3437);
        assert_eq!(first["mark"], true);
        // a step carries only what it uses
        assert!(first.get("file").is_none() && first.get("ms").is_none());
        assert_eq!(json["steps"][1]["paths"][8], "s9-bands-2k.bbl");
        // every action is one the script knows
        let script = SCRIPT;
        for step in json["steps"].as_array().map_or(&[][..], Vec::as_slice) {
            let action = step["action"].as_str().unwrap_or("");
            assert!(
                script.contains(&format!("$action -eq \"{action}\"")),
                "{action}"
            );
        }
    }

    #[test]
    fn the_script_travels_with_the_pack() {
        let files = files(1, 47, 0, false);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["capture.ps1", "run.cmd", "README.txt"]);
        for (name, bytes) in &files {
            let text = String::from_utf8(bytes.clone()).unwrap_or_default();
            // Windows PowerShell reads a script without a byte-order mark in
            // the system code page: the files stay ASCII
            assert!(text.is_ascii(), "{name} is not ASCII");
            assert!(
                text.contains("\r\n") && !text.replace("\r\n", "").contains('\n'),
                "{name}"
            );
        }
        let script = String::from_utf8(files[0].1.clone()).unwrap_or_default();
        assert!(script.contains("function Invoke-Step"));
        assert!(script.contains("$script:skip--"));
        assert!(String::from_utf8_lossy(&files[1].1).contains("-Steps \"steps.json\""));
        assert!(String::from_utf8_lossy(&files[2].1).contains("saves 47 pictures"));
        let note =
            String::from_utf8(super::files(2, 32, 108, true)[2].1.clone()).unwrap_or_default();
        assert!(
            note.contains("108 plots copied as vector graphics"),
            "{note}"
        );
        assert!(note.contains("15 minutes") && note.contains("predictions.json"));
        assert!(note.contains("Calibration round 2, for version 0.94 of the reference tool"));
        assert!(note.is_ascii());
        // a round without a model says nothing of predictions
        let step =
            String::from_utf8(super::files(4, 20, 153, false)[2].1.clone()).unwrap_or_default();
        assert!(step.contains("153 plots") && !step.contains("predictions.json"));
    }
}
