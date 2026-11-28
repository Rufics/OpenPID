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
/// Title of its window of frequency against time.
const TIME: &str = "Frequency x Time";

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
    /// "Reset", beside "Run", which clears the plots of the earlier runs:
    /// measured in round 17's picture of the window (`r17-e3-step-2k-step`,
    /// 3686 × 2357 pixels; the button is pixels 3472 to 3633 across and
    /// 767 to 830 down there).
    pub const RESET: (u32, u32) = (3643, 821);
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

/// The lengths of the window, seconds, beyond the one the window comes up
/// with, at which the response and the Peak of a probe are captured once
/// more: for the probe of round 18 that asks windows of 4 and 5 s (R1).
pub fn windows_captured(probe: &Probe) -> &'static [u32] {
    if probe.id == "d3-win-2k" {
        &[4, 5]
    } else {
        &[]
    }
}

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
    /// The buttons under it that no round has pressed before round 12: the
    /// name on the button, the name in the files, the position.
    pub const OTHERS: [(&str, &str, (u32, u32)); 3] = [
        ("PID slider tool", "sliders", (3556, 1056)),
        ("Filter Sim", "filters", (3556, 1148)),
        ("Setup Info", "setup", (3556, 1240)),
    ];
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
    /// The "Y min" box beside it, measured for round 17 in the picture
    /// round 16 saved of the window (`r16-spectrum-run`: 3686 × 2357
    /// pixels, scaled to the frame `shot`; the box is pixels 3372 to 3452
    /// across and 1539 to 1594 down there).
    pub const Y_MIN: (u32, u32) = (3499, 1612);
    /// The middle of the first row ("Gyro") of the list of channels below
    /// the list of logs, measured in the same picture: the list is pixels
    /// 3310 to 3633 across and 493 to 916 down there, its rows 37 pixels
    /// high (38.1 here), the first two highlighted as the window comes up
    /// (494 to 567). The rows of the pictures of rounds 8 to 11 lie on the
    /// same pixels. Eleven rows and no scroll bar: [`super::SPECTRUM_CHANNELS`].
    pub const CHANNEL: (u32, u32) = (3515, 528);
    /// The distance between two rows of that list, tenths of a pixel.
    pub const CHANNEL_ROW_TENTHS: u32 = 381;
    /// The one large plot the "Motors" entry of that list draws in place
    /// of the six, left, top, right and bottom of its axes: measured in
    /// round 17's picture of the window with the entry picked
    /// (`r17-i1-chan-2k-spectrum-motors`, 3686 × 2357 pixels; the axes are
    /// pixels 184 to 3132 across and 141 to 2121 down there).
    pub const MOTORS_PLOT: (u32, u32, u32, u32) = (189, 145, 3212, 2184);

    /// The middle of row `row` (from 0) of the list of channels.
    pub fn channel(row: usize) -> (u32, u32) {
        (
            CHANNEL.0,
            CHANNEL.1 + (CHANNEL_ROW_TENTHS * row as u32 + 5) / 10,
        )
    }
}

/// The entries of the spectrum window's list of channels from the top, as
/// version 0.94 lists them (round 16's picture): the name in the files,
/// the entry, and whether the plots are copied after a pick, which they
/// are for every entry `OpenPID` has a spectrum of. "Test Signal", which
/// version 0.9 listed, is not in this version's list.
pub const SPECTRUM_CHANNELS: [(&str, &str, bool); 11] = [
    ("gyro", "Gyro", true),
    ("prefilt", "Gyro prefilt", true),
    ("dterm", "Dterm", true),
    ("dterm-prefilt", "Dterm prefilt", false),
    ("pterm", "Pterm", true),
    ("piderror", "PID error", true),
    ("setpoint", "Set point", true),
    ("fterm", "Fterm", true),
    ("pidsum", "PIDsum", true),
    ("motors", "Motors", true),
    ("debug", "debug", true),
];
/// The name in the files of the copy of the one plot the "Motors" entry
/// draws: the eight motors over the whole range.
pub const MOTORS_COPY: &str = "all";
/// The lower end of the spectrum plots round 17 types for the other
/// channels, dB, and the one the window comes up with: the floor of a
/// motor line, the logged number over 20, lies near -67 dB.
const Y_MIN_CHANNELS: &str = "-90";
const Y_MIN_DEFAULT: &str = "-50";

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
    /// The lists of logs above the two other columns, which come up as the
    /// two D-term channels. From round 10 on they are given the same log:
    /// until then they kept the first log of the list.
    pub const OTHER_LOG_LISTS: [(u32, u32); 2] = [(2156, 109), (2996, 109)];
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

/// Controls and plots of the window of frequency against time of version
/// 0.94, read off two half-size pictures of it (the owner, 2026-11-13
/// before a Run and 2026-11-14 after one), so each is right to a few
/// pixels. Round 10 is the first to click here.
mod time_at {
    /// "Freq x Time" of the spectrum window.
    pub const OPEN: (u32, u32) = (3555, 1160);
    pub const LOG_LIST: (u32, u32) = (3551, 168);
    pub const CHANNEL_LIST: (u32, u32) = (3551, 245);
    pub const RUN: (u32, u32) = (3463, 330);
    /// The two smoothing lists: along the frequency, along the time.
    pub const SMOOTH_LISTS: [(&str, (u32, u32)); 2] =
        [("smooth-freq", (3463, 478)), ("smooth-time", (3638, 478))];
    /// The switch "< 100Hz".
    pub const BELOW_100: (u32, u32) = (3408, 720);
    /// The box "Z min".
    pub const Z_MIN: (u32, u32) = (3500, 838);
    /// The box "Z max".
    pub const Z_MAX: (u32, u32) = (3611, 838);
    /// Left and right of the axes of the three plots.
    pub const SIDES: (u32, u32) = (304, 3118);
    /// Top and bottom of the axes of roll, pitch and yaw.
    pub const ROWS: [(u32, u32); 3] = [(147, 753), (910, 1516), (1673, 2279)];
}

/// The channels of the window of frequency against time that are captured:
/// the name in the files and the place in the window's list of channels,
/// taken to be in the order of the spectrum window's list. "Gyro prefilt"
/// is what the window comes up with.
const TIME_CHANNELS: [(&str, usize); 2] = [("prefilt", 1), ("gyro", 0)];
/// The lower end of the colour scale typed for one probe in place of the
/// -30 dB the window comes up with: low enough for the floor of a quiet
/// axis.
const TIME_Z_MIN: &str = "-70";
/// The lower end of the colour scale the window comes up with, dB.
const TIME_Z_MIN_DEFAULT: &str = "-30";
/// The upper end of the colour scale typed for one probe in place of the
/// 10 dB the window comes up with.
const TIME_Z_MAX: &str = "0";
/// The upper end of the colour scale the window comes up with, dB.
const TIME_Z_MAX_DEFAULT: &str = "10";
/// The first round that asks the window of frequency against time about
/// its two smoothing lists; the round before it took a first look.
pub const TIME_LEVELS: u32 = 11;
/// How long the script waits for the maps of that window to start changing
/// after a setting was changed, seconds. A setting that leaves the picture
/// as it was (the log the window came up with) costs this long.
const TIME_CHANGE_S: f64 = 25.0;
/// The place of "low" in either smoothing list of that window (off, low,
/// med, high): what both come up with.
const TIME_LOW: usize = 1;
/// The settings of the two smoothing lists captured for the first log
/// beyond what the window comes up with: the name in the files, the item
/// of the list for the frequency axis, the item of the list for the time
/// axis. One list at a time first, then both.
const TIME_SMOOTHING: [(&str, usize, usize); 8] = [
    ("freq-off", 0, TIME_LOW),
    ("freq-med", 2, TIME_LOW),
    ("freq-high", 3, TIME_LOW),
    ("time-off", TIME_LOW, 0),
    ("time-med", TIME_LOW, 2),
    ("time-high", TIME_LOW, 3),
    ("both-off", 0, 0),
    ("both-high", 3, 3),
];
/// The settings of the two lists captured for the flight of a round, to
/// hold what the first log taught against.
const TIME_SMOOTHING_FLIGHT: [(&str, usize, usize); 2] = [("both-med", 2, 2), ("both-high", 3, 3)];

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
    /// The inside of the small window of the slider tool, in its own
    /// pixels; only in the plans that click in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slider: Option<Frame>,
}

/// One step of the script. Only the fields a step uses are written.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Step {
    /// What to do: `click`, `open_file`, `settle`, `shot`, `pause`,
    /// `windows`, `hover`, `clip_wait`, `clip_save`, `type`, `key`,
    /// `tables`.
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
    /// The key of a `key` step: `esc` or `enter`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<&'static str>,
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
    named(format!("r{round}-{probe}-step"), [what, plot])
}

/// `name` with those of `parts` that are not empty after it, a hyphen
/// before each.
fn named(mut name: String, parts: [&str; 2]) -> String {
    for part in parts {
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
    /// The log chosen in row `row` (from 0) of the window's list: a click
    /// selects it alone.
    fn choose(self, steps: &mut Vec<Step>, row: usize) {
        let y = log_row(step_at::LOG.1, row);
        steps.push(step_click(&format!("log {}", self.id), (step_at::LOG.0, y)));
        steps.push(pause(300));
    }

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

    step_open(steps, round);
    for (row, probe) in probes.iter().enumerate().filter(|(_, p)| is_step(p)) {
        let log = StepLog {
            round,
            id: probe.id,
        };
        log.choose(steps, row);
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

/// The step response window opened from the main window, and a picture of
/// it as it comes up.
fn step_open(steps: &mut Vec<Step>, round: u32) {
    steps.push(click(LOG_VIEWER, "log", "Step Resp Tool", log_at::STEP));
    steps.push(Step {
        wait_s: Some(900),
        ..settle(STEP, None, 2.0)
    });
    steps.push(shot(STEP, format!("r{round}-step-open")));
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
    named(format!("r{round}-{probe}-throttle"), [what, plot])
}

/// Whether the maps of `probe` are also captured at the four smoothing
/// levels the window does not come up with: the stairs, for the spread of
/// a column next to its neighbours, and the held throttle, for the spread
/// of one column alone.
pub fn throttle_smoothing_captured(probe: &Probe) -> bool {
    ["z1-stairs-2k", "z3-hold-2k", "a9-flight-2k"].contains(&probe.id)
}

/// Whether the maps of `probe` are also captured on a wider colour scale
/// and below 100 Hz: the held throttle, whose one spot is then inside the
/// scale, and drawn large.
pub fn throttle_views_captured(probe: &Probe) -> bool {
    ["z3-hold-2k", "a9-flight-2k"].contains(&probe.id)
}

/// From this round on the throttle window is asked one thing only, the
/// distance between segments in logs of other lengths: the maps as the
/// window comes up are enough for it. The log is chosen in all four
/// columns from here on.
pub const THROTTLE_LENGTHS_ONLY: u32 = 10;

/// The last round that captured the "2D" lines and typed a colour scale:
/// the lines turned out to be a spectrum of the whole log, not a cut
/// through the map, and the typed scale did not hold.
pub const THROTTLE_FIRST_LOOK: u32 = 8;

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
        self.run(steps, what);
        for column in 0..throttle_at::COLUMNS.len() {
            for axis in 0..throttle_at::ROWS.len() {
                self.copy(steps, what, column, axis);
            }
        }
    }

    /// A Run, waited for, and a picture of the window named after the
    /// setting `what`.
    fn run(self, steps: &mut Vec<Step>, what: &str) {
        steps.push(Step {
            mark: Some(true),
            ..throttle_click("Run", throttle_at::RUN)
        });
        steps.push(settle(THROTTLE, Some(60.0), 1.5));
        steps.push(shot(THROTTLE, throttle_file(self.round, self.id, what, "")));
    }

    /// A copy of the plot of `axis` (0 … 2) in `column` (0 for the
    /// unfiltered gyro, 1 for the filtered one), named after `what`.
    fn copy(self, steps: &mut Vec<Step>, what: &str, column: usize, axis: usize) {
        let (channel, left, right) = throttle_at::COLUMNS[column % 2];
        let (top, bottom) = throttle_at::ROWS[axis % 3];
        let name = format!("{} {channel}", AXES[axis % 3]);
        let plot = format!("{}-{channel}", AXES[axis % 3]);
        let file = throttle_file(self.round, self.id, what, &plot);
        copy_axes(steps, THROTTLE, &name, (left, top, right, bottom), file);
    }

    /// The log chosen in the lists above the columns: in the first two, and
    /// from [`THROTTLE_LENGTHS_ONLY`] on in all four. `row` is its place in
    /// those lists.
    fn choose(self, steps: &mut Vec<Step>, row: usize) {
        let lists = throttle_at::LOG_LISTS.into_iter();
        let others = throttle_at::OTHER_LOG_LISTS.into_iter();
        let all = self.round >= THROTTLE_LENGTHS_ONLY;
        let others = others.take(if all { 2 } else { 0 });
        for (k, list) in lists.chain(others).enumerate() {
            let name = format!("log {} in column {}", self.id, k + 1);
            throttle_pick(steps, list, &name, row);
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

    /// The maps on the colour scale [`THROTTLE_SCALE`] (up to
    /// [`THROTTLE_FIRST_LOOK`]), then below 100 Hz, and both as the window
    /// came up again.
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
        if self.round <= THROTTLE_FIRST_LOOK {
            scale(steps, THROTTLE_SCALE);
            self.capture(steps, "scale-5");
            scale(steps, THROTTLE_SCALE_DEFAULT);
        }
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

/// The window of frequency against throttle, opened from the spectrum
/// window, and a picture of it as it comes up.
fn throttle_open(steps: &mut Vec<Step>, round: u32) {
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
}

/// The steps of the window of frequency against throttle for the probes of
/// a round. The window is opened from the spectrum window, after a Run
/// there. For every log: the log chosen in the first two columns ("Gyro
/// prefilt" and "Gyro"), then a Run, a picture and copies of the six plots
/// as the window comes up, the same with "PSD" on and, up to
/// [`THROTTLE_FIRST_LOOK`], with "2D" on; for the logs of
/// [`throttle_smoothing_captured`] also at the other smoothing levels, and
/// for those of [`throttle_views_captured`] below 100 Hz (and, up to that
/// round, on a wider colour scale).
fn throttle_maps(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    throttle_open(steps, round);
    let is_throttle = |p: &Probe| p.area == Area::Throttle;
    for (row, probe) in probes.iter().enumerate().filter(|(_, p)| is_throttle(p)) {
        let log = ThrottleLog {
            round,
            id: probe.id,
        };
        log.choose(steps, row);
        log.capture(steps, "");
        if round >= THROTTLE_LENGTHS_ONLY {
            continue;
        }
        log.switched(steps, "PSD", throttle_at::PSD, "psd");
        if round <= THROTTLE_FIRST_LOOK {
            log.switched(steps, "2D", throttle_at::TWO_D, "2d");
        }
        if throttle_smoothing_captured(probe) {
            log.smoothing_levels(steps);
        }
        if throttle_views_captured(probe) {
            log.views(steps);
        }
    }
}

/// The spectrum window opened from the main window, and a Run in it: the
/// windows of frequency against throttle and against time are opened from
/// there.
fn open_spectrum(steps: &mut Vec<Step>, round: u32) {
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
}

/// Name of a picture or of a copied plot of the window of frequency against
/// time: `what` is the setting ("prefilt", "gyro", "prefilt-sub100" …) and
/// `plot` the axis ("" for a picture of the window).
pub fn time_file(round: u32, probe: &str, what: &str, plot: &str) -> String {
    named(format!("r{round}-{probe}-time"), [what, plot])
}

/// A click in the window of frequency against time, at a position of a
/// picture of it.
fn time_click(name: &str, at: (u32, u32)) -> Step {
    click(TIME, "shot", name, at)
}

/// An item of a list of that window: a click on the list, a moment for it
/// to open, a click on the item.
fn time_pick(steps: &mut Vec<Step>, list: (u32, u32), name: &str, row: usize) {
    steps.push(time_click(&format!("{name} (the list)"), list));
    steps.push(pause(600));
    steps.push(time_click(name, throttle_at::item(list, row)));
    steps.push(pause(300));
}

/// A Run of the window of frequency against time, waited for, a picture of
/// the window and copies of its three plots, all named after `what`.
fn time_capture(steps: &mut Vec<Step>, round: u32, probe: &str, what: &str) {
    steps.push(Step {
        mark: Some(true),
        ..time_click("Run", time_at::RUN)
    });
    time_copies(steps, round, probe, what, 60.0);
}

/// A change of the settings of that window and the maps after it. The
/// picture is marked before the first step of `change`, not at the Run: the
/// window redraws as soon as a list or a box is set, and a Run after that
/// changes nothing (round 11 waited a minute for it each time). Then a Run,
/// a wait of at most [`TIME_CHANGE_S`] seconds for the redrawing, a picture
/// and the copies.
fn time_changed(
    steps: &mut Vec<Step>,
    round: u32,
    probe: &str,
    what: &str,
    change: impl FnOnce(&mut Vec<Step>),
) {
    let from = steps.len();
    change(steps);
    if let Some(first) = steps.get_mut(from) {
        first.mark = Some(true);
    }
    steps.push(time_click("Run", time_at::RUN));
    time_copies(steps, round, probe, what, TIME_CHANGE_S);
}

/// The wait for the maps of that window to be redrawn (at most `change_s`
/// seconds for the redrawing to start), a picture of the window and copies
/// of its three plots, all named after `what`.
fn time_copies(steps: &mut Vec<Step>, round: u32, probe: &str, what: &str, change_s: f64) {
    steps.push(settle(TIME, Some(change_s), 1.5));
    steps.push(shot(TIME, time_file(round, probe, what, "")));
    let (left, right) = time_at::SIDES;
    for (axis, (top, bottom)) in time_at::ROWS.into_iter().enumerate() {
        let file = time_file(round, probe, what, AXES[axis]);
        copy_axes(steps, TIME, AXES[axis], (left, top, right, bottom), file);
    }
}

/// The window of frequency against time, opened from the spectrum window,
/// and a picture of it as it comes up.
fn time_open(steps: &mut Vec<Step>, round: u32) {
    steps.push(click(SPECTRUM, "shot", "Freq x Time", time_at::OPEN));
    steps.push(Step {
        wait_s: Some(120),
        ..settle(TIME, None, 1.5)
    });
    steps.push(shot(TIME, format!("r{round}-time-open")));
}

/// Both smoothing lists of that window set: item `freq` of the list for
/// the frequency axis and item `time` of the list for the time axis.
fn time_smoothing(steps: &mut Vec<Step>, freq: usize, time: usize) {
    const LEVELS: [&str; 4] = ["off", "low", "med", "high"];
    for ((list, at), item) in time_at::SMOOTH_LISTS.into_iter().zip([freq, time]) {
        time_pick(steps, at, &format!("{list} {}", LEVELS[item]), item);
    }
}

/// A channel of that window chosen: its name in the files and its place in
/// the window's list of channels.
fn time_channel(steps: &mut Vec<Step>, (name, item): (&str, usize)) {
    let name = format!("channel {name}");
    time_pick(steps, time_at::CHANNEL_LIST, &name, item);
}

/// The maps of the unfiltered gyro of the log in row `row` of that window's
/// list, as the window comes up otherwise: the log and the channel chosen,
/// a Run, a picture, copies of the three plots.
fn time_first(steps: &mut Vec<Step>, round: u32, id: &str, row: usize) {
    let prefilt = TIME_CHANNELS[0];
    time_changed(steps, round, id, prefilt.0, |steps| {
        time_pick(steps, time_at::LOG_LIST, &format!("log {id}"), row);
        time_channel(steps, prefilt);
    });
}

/// The steps of the window of frequency against time from round
/// [`TIME_LEVELS`] on. For every log the maps of the unfiltered gyro as the
/// window comes up: a Run, a picture, copies of the three plots. For the
/// first log also the maps at every setting of [`TIME_SMOOTHING`] and with
/// [`TIME_Z_MAX`] typed into "Z max"; for the last, the flight, also the
/// maps of the filtered gyro and at the settings of
/// [`TIME_SMOOTHING_FLIGHT`]. The lists and the box are put back each time.
fn time_levels(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    time_open(steps, round);
    let is_time = |p: &Probe| p.area == Area::Time;
    let first = probes.iter().position(is_time);
    let last = probes.iter().rposition(is_time);
    let (prefilt, prefilt_item) = TIME_CHANNELS[0];
    let channel = time_channel;
    let z_max = |steps: &mut Vec<Step>, text: &str| {
        steps.push(Step {
            action: "type",
            text: Some(text.to_string()),
            ..time_click("Z max", time_at::Z_MAX)
        });
        steps.push(pause(300));
    };
    for (row, probe) in probes.iter().enumerate().filter(|(_, p)| is_time(p)) {
        time_first(steps, round, probe.id, row);
        let settings: &[(&str, usize, usize)] = if first == Some(row) {
            &TIME_SMOOTHING
        } else if last == Some(row) {
            let gyro = TIME_CHANNELS[1];
            time_changed(steps, round, probe.id, gyro.0, |steps| channel(steps, gyro));
            channel(steps, (prefilt, prefilt_item));
            &TIME_SMOOTHING_FLIGHT
        } else {
            &[]
        };
        for (name, freq, time) in settings {
            let what = format!("{prefilt}-{name}");
            time_changed(steps, round, probe.id, &what, |steps| {
                time_smoothing(steps, *freq, *time);
            });
        }
        if first == Some(row) {
            let what = format!("{prefilt}-zmax-0");
            time_changed(steps, round, probe.id, &what, |steps| {
                time_smoothing(steps, TIME_LOW, TIME_LOW);
                z_max(steps, TIME_Z_MAX);
            });
            z_max(steps, TIME_Z_MAX_DEFAULT);
        } else if !settings.is_empty() {
            time_smoothing(steps, TIME_LOW, TIME_LOW);
        }
    }
}

/// The steps of the window of frequency against time for the probes of a
/// round: a first look. The window is opened from the spectrum window. For
/// every log the maps of the unfiltered and of the filtered gyro: a Run, a
/// picture, copies of the three plots. For the first log also a picture of
/// each smoothing list while it is open (closed again with Esc), the maps
/// below 100 Hz, and the maps with the colour scale reaching down to
/// [`TIME_Z_MIN`] dB.
fn time_maps(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    time_open(steps, round);
    let is_time = |p: &Probe| p.area == Area::Time;
    let first = probes.iter().position(is_time);
    for (row, probe) in probes.iter().enumerate().filter(|(_, p)| is_time(p)) {
        let name = format!("log {}", probe.id);
        time_pick(steps, time_at::LOG_LIST, &name, row);
        for (channel, item) in TIME_CHANNELS {
            let name = format!("channel {channel}");
            time_pick(steps, time_at::CHANNEL_LIST, &name, item);
            time_capture(steps, round, probe.id, channel);
        }
        // back to the channel the window came up with
        let (channel, item) = TIME_CHANNELS[0];
        time_pick(
            steps,
            time_at::CHANNEL_LIST,
            &format!("channel {channel}"),
            item,
        );
        if first != Some(row) {
            continue;
        }
        for (list, at) in time_at::SMOOTH_LISTS {
            steps.push(time_click(&format!("{list} (the list)"), at));
            steps.push(pause(800));
            steps.push(shot(TIME, time_file(round, probe.id, "list", list)));
            steps.push(Step {
                action: "key",
                key: Some("esc"),
                ..Step::default()
            });
            steps.push(pause(400));
        }
        steps.push(time_click("< 100Hz on", time_at::BELOW_100));
        steps.push(pause(300));
        time_capture(steps, round, probe.id, "prefilt-sub100");
        steps.push(time_click("< 100Hz off", time_at::BELOW_100));
        steps.push(pause(300));
        let z_min = |steps: &mut Vec<Step>, text: &str| {
            steps.push(Step {
                action: "type",
                text: Some(text.to_string()),
                ..time_click("Z min", time_at::Z_MIN)
            });
            steps.push(pause(300));
        };
        z_min(steps, TIME_Z_MIN);
        time_capture(steps, round, probe.id, "prefilt-zmin-70");
        z_min(steps, TIME_Z_MIN_DEFAULT);
    }
}

/// What the plan of a round of the map of frequency against throttle does,
/// for the reader of `steps.json`.
fn throttle_about(round: u32, probes: &[Probe]) -> String {
    let levels = probes
        .iter()
        .filter(|p| throttle_smoothing_captured(p))
        .count();
    let first_look = round <= THROTTLE_FIRST_LOOK;
    let lines = if first_look {
        " and as lines (\"2D\")"
    } else {
        ""
    };
    let logs = if levels == 1 {
        "one log".to_string()
    } else {
        format!("{levels} logs")
    };
    let views = if first_look {
        "on a wider colour scale and below 100 Hz"
    } else {
        "below 100 Hz"
    };
    format!(
        "Round {round}: every log through one open dialog, then the window of frequency \
         against throttle for each: the maps of the unfiltered and of the filtered gyro as \
         the window comes up and in decibels (\"PSD\"){lines}, for {logs} at the other \
         smoothing levels, and for one {views}; a picture of the window and a vector copy of \
         every plot each time."
    )
}

/// Name of the picture of `probe` on the spectrum at a smoothing level.
pub fn spectrum_shot(round: u32, probe: &str, smoothing: &str) -> String {
    format!("r{round}-{probe}-spectrum-{smoothing}")
}

/// The first round that clicks inside the windows round 12 looked at.
pub const WINDOWS_ASK: u32 = 13;
/// The first round that sets the lists of the filter simulator and of the
/// slider tool, which round 13 only photographed.
pub const WINDOWS_MORE: u32 = 14;
/// Part of the title of the slider tool's window.
const SLIDERS: &str = "PID slider tool";
/// Part of the title of the filter simulator's window.
const FILTERS: &str = "Filter Simulator";
/// Part of the title of the window that lists the setup.
const SETUP: &str = "Setup Info";

/// Controls of the slider tool, in the pixels of the inside of its window
/// (the frame `slider`), read from the picture of round 12.
mod sliders_at {
    /// Width and height of the inside of the window.
    pub const SIZE: (u32, u32) = (1152, 1025);
    pub const RESET: (u32, u32) = (143, 75);
    pub const FIRMWARE_LIST: (u32, u32) = (143, 213);
    /// The boxes of the start gains and what round 13 types into them.
    pub const START: [(&str, (u32, u32), &str); 3] = [
        ("P", (401, 127), "45"),
        ("I", (401, 177), "80"),
        ("D", (401, 229), "30"),
    ];
    /// The first item of the open list; the next ones follow 46 pixels
    /// apart: Betaflight, INAV, FETTEC, `KISS_ULTRA`, `QuickSilver`, Ardupilot.
    pub const FIRMWARE_ITEM: (u32, u32) = (143, 258);
    pub const FIRMWARE: [&str; 6] = [
        "betaflight",
        "inav",
        "fettec",
        "kiss-ultra",
        "quicksilver",
        "ardupilot",
    ];
    /// The arrows at the two ends of a slider.
    pub const DOWN_X: u32 = 355;
    pub const UP_X: u32 = 1083;
    /// The sliders from the top: the name in the files and the row.
    pub const ROWS: [(&str, u32); 5] = [
        ("pd", 385),
        ("pi", 487),
        ("roll-pitch", 589),
        ("yaw", 691),
        ("master", 793),
    ];
}

/// Controls of the filter simulator, in a picture of its window.
mod filters_at {
    /// Its lists: the name in the files and the place.
    pub const LISTS: [(&str, (u32, u32)); 10] = [
        ("looprate", (3466, 157)),
        ("motors", (3647, 157)),
        ("lowpass-1", (3466, 235)),
        ("lowpass-2", (3466, 311)),
        ("rpm-harmonics", (3466, 387)),
        ("notch-f", (3466, 542)),
        ("notch-q", (3647, 542)),
        ("signal", (3556, 851)),
        ("noise", (3556, 929)),
        ("show", (3556, 1005)),
    ];
    /// Its three switches.
    pub const SWITCHES: [(&str, (u32, u32)); 3] = [
        ("combine-lpf", (3450, 1326)),
        ("log-frequency", (3450, 1402)),
        ("magnitude-db", (3450, 1481)),
    ];
    /// A box round 13 types into.
    pub struct Typed {
        /// The name in the files.
        pub name: &'static str,
        /// The place of the box.
        pub at: (u32, u32),
        /// What is typed.
        pub text: &'static str,
        /// What the box held, typed back afterwards.
        pub back: &'static str,
        /// The column of plots it changes.
        pub column: usize,
    }
    const fn typed(
        name: &'static str,
        at: (u32, u32),
        text: &'static str,
        back: &'static str,
        column: usize,
    ) -> Typed {
        Typed {
            name,
            at,
            text,
            back,
            column,
        }
    }
    /// The boxes round 13 types into.
    pub const BOXES: [Typed; 4] = [
        typed("lowpass-1-100", (3622, 245), "100", "250", 0),
        typed("rpm-q-250", (3622, 476), "250", "500", 1),
        typed("notch-q-300", (3622, 630), "300", "500", 1),
        typed("notch-center-300", (3622, 708), "300", "500", 1),
    ];
    /// The boxes of the cut-off of low-pass 1 and 2, of the lowest RPM
    /// notch and its Q, and of the static notch's Q, centre and cut-off.
    pub const LOWPASS_1_HZ: (u32, u32) = (3622, 245);
    pub const LOWPASS_2_HZ: (u32, u32) = (3622, 321);
    pub const RPM_HZ: (u32, u32) = (3622, 397);
    pub const RPM_Q: (u32, u32) = (3622, 476);
    pub const NOTCH_Q: (u32, u32) = (3622, 630);
    pub const NOTCH_CENTER: (u32, u32) = (3622, 708);
    pub const NOTCH_CUTOFF: (u32, u32) = (3622, 785);
    /// The place of a list by its name in [`LISTS`].
    pub fn list(name: &str) -> (u32, u32) {
        let found = LISTS.iter().find(|(n, _)| *n == name);
        found.map_or(LISTS[0].1, |(_, at)| *at)
    }
    /// The two columns of plots: the name, left and right of the axes.
    pub const COLUMNS: [(&str, u32, u32); 2] = [("lowpass", 267, 1661), ("notch", 1891, 3288)];
    /// The four rows of plots: the name, top and bottom of the axes.
    pub const ROWS: [(&str, u32, u32); 4] = [
        ("magnitude", 218, 630),
        ("delay", 752, 1164),
        ("phase", 1287, 1699),
        ("step", 1821, 2233),
    ];
}

/// Controls of the window that lists the setup, in a picture of it.
mod setup_at {
    /// The switch "Show Differences Only".
    pub const DIFFERENCES: (u32, u32) = (160, 49);
    /// The list of the logs.
    pub const LOG_LIST: (u32, u32) = (1015, 43);
}

/// Controls and plots of the main window of version 0.94. Its check-list is
/// measured in the reference screenshot (the frame `log`), everything else
/// in a picture of the window as the script saves one (the frame `shot`,
/// which fits the main window as it fits the others).
mod viewer_at {
    /// Left and right of the axes of the four plots.
    pub const SIDES: (u32, u32) = (510, 3307);
    /// The plots from the top: the name in the files, top and bottom of the
    /// axes.
    pub const ROWS: [(&str, u32, u32); 4] = [
        ("roll", 133, 655),
        ("pitch", 668, 1190),
        ("yaw", 1203, 1723),
        ("throttle", 1833, 2173),
    ];
    /// The column of the boxes of the check-list, in the frame `log`.
    pub const BOX_X: u32 = 67;
    /// The lines of the check-list from the top: the name beside the box,
    /// the row of the box in the frame `log`, and whether the line is on
    /// as the window comes up.
    pub const LINES: [(&str, u32, bool); 20] = [
        ("Gyro(pf)", 384, false),
        ("Gyro", 445, true),
        ("P-term", 506, false),
        ("I-term", 566, false),
        ("D-term(pf)", 627, false),
        ("D-term", 688, false),
        ("F-term", 748, false),
        ("Set point", 809, true),
        ("PID sum", 870, false),
        ("PID error", 930, false),
        ("Motor 1/5", 991, true),
        ("Motor 2/6", 1052, true),
        ("Motor 3/7", 1112, true),
        ("Motor 4/8", 1173, true),
        ("RPS 1/5", 1234, false),
        ("RPS 2/6", 1294, false),
        ("RPS 3/7", 1355, false),
        ("RPS 4/8", 1416, false),
        ("Throttle", 1476, true),
        ("Debug", 1537, false),
    ];
    /// The box of the y scale, what round 15 types into it and what it
    /// holds as the window comes up.
    pub const Y_SCALE: (u32, u32) = (132, 241);
    pub const Y_SCALE_TYPED: &str = "1000";
    pub const Y_SCALE_DEFAULT: &str = "500";
    /// The list "line smoothing".
    pub const SMOOTHING_LIST: (u32, u32) = (3468, 628);
    /// The items of that list when it is open, read from the picture round
    /// 15 took of it: the name in the files and the place. "off" is what
    /// the window comes up with.
    pub const SMOOTHING: [(&str, (u32, u32)); 5] = [
        ("off", (3468, 672)),
        ("low", (3468, 721)),
        ("med", (3468, 769)),
        ("med-high", (3468, 817)),
        ("high", (3468, 864)),
    ];
    /// The list of the logs at the top of the panel, at about this place:
    /// the window has it when more than one log is open, and no round
    /// before round 16 has clicked it. Its items are taken to lie as those
    /// of the tool's other lists do ([`super::throttle_at::item`]).
    pub const FILE_LIST: (u32, u32) = (3559, 262);
    /// What round 16 types into the y scale, for a line that may be large.
    pub const Y_SCALE_WIDE: &str = "2000";
    /// Where round 15 clicks into the roll plot, seconds of the flight.
    pub const CURSOR_S: f64 = 6.0;
    /// The button ">".
    pub const NEXT: (u32, u32) = (3642, 1607);
    /// The buttons "Markup" and "Period".
    pub const MARKUP: (u32, u32) = (3650, 457);
    pub const PERIOD: (u32, u32) = (3468, 457);

    /// The middle of the roll plot at `seconds` of a flight of `duration_s`
    /// that fills the time axis from side to side.
    pub fn roll_at(seconds: f64, duration_s: f64) -> (u32, u32) {
        let (left, right) = SIDES;
        let (_, top, bottom) = ROWS[0];
        let along = f64::from(right - left) * seconds / duration_s;
        (left + along.round() as u32, u32::midpoint(top, bottom))
    }
}

/// A list held open for a picture and closed again with Esc.
fn list_shot(steps: &mut Vec<Step>, list: Step, file: String) {
    let window = list.window.unwrap_or(LOG_VIEWER);
    steps.push(list);
    steps.push(pause(800));
    steps.push(shot(window, file));
    steps.push(Step {
        action: "key",
        key: Some("esc"),
        ..Step::default()
    });
    steps.push(pause(400));
}

/// The slider tool asked: the window opened from the main window, a
/// picture as it comes up; the start gains typed in, a picture; the arrows
/// of the sliders pressed (one press first, to see the size of a step),
/// a picture after each group; its list held open; "Reset", a picture.
fn sliders_ask(steps: &mut Vec<Step>, round: u32) {
    let (button, _, at) = log_at::OTHERS[0];
    let file = |what: &str| format!("r{round}-sliders-{what}");
    let inside = |name: &str, at: (u32, u32)| click(SLIDERS, "slider", name, at);
    steps.push(click(LOG_VIEWER, "log", button, at));
    steps.push(pause(4000));
    steps.push(shot(SLIDERS, file("open")));
    for (name, at, text) in sliders_at::START {
        steps.push(Step {
            action: "type",
            text: Some(text.to_string()),
            ..inside(&format!("start {name}"), at)
        });
        steps.push(pause(300));
    }
    steps.push(shot(SLIDERS, file("start")));
    // the slider, the arrow and how often it is pressed
    let moves: [(usize, bool, u32); 6] = [
        (0, true, 1),
        (0, true, 4),
        (1, false, 3),
        (2, true, 2),
        (3, false, 2),
        (4, true, 5),
    ];
    for (k, (row, up, presses)) in moves.into_iter().enumerate() {
        let (name, y) = sliders_at::ROWS[row];
        let (arrow, x) = if up {
            ("up", sliders_at::UP_X)
        } else {
            ("down", sliders_at::DOWN_X)
        };
        for _ in 0..presses {
            steps.push(inside(&format!("{name} {arrow}"), (x, y)));
            steps.push(pause(300));
        }
        steps.push(shot(
            SLIDERS,
            file(&format!("{}-{name}-{arrow}-{presses}", k + 1)),
        ));
    }
    let list = inside("firmware (the list)", sliders_at::FIRMWARE_LIST);
    list_shot(steps, list, file("list-firmware"));
    steps.push(inside("Reset", sliders_at::RESET));
    steps.push(pause(600));
    steps.push(shot(SLIDERS, file("reset")));
}

/// Copies of the four plots of column `column` of the filter simulator.
fn filters_copies(steps: &mut Vec<Step>, round: u32, what: &str, column: usize) {
    let (name, left, right) = filters_at::COLUMNS[column];
    for (row, top, bottom) in filters_at::ROWS {
        let plot = format!("{name} {row}");
        let file = format!("r{round}-filters-{what}-{name}-{row}");
        copy_axes(steps, FILTERS, &plot, (left, top, right, bottom), file);
    }
}

/// The filter simulator opened from the main window, and a picture of it
/// as it comes up.
fn filters_open(steps: &mut Vec<Step>, round: u32) {
    let (button, _, at) = log_at::OTHERS[1];
    steps.push(click(LOG_VIEWER, "log", button, at));
    steps.push(Step {
        wait_s: Some(120),
        ..settle(FILTERS, None, 1.5)
    });
    steps.push(shot(FILTERS, format!("r{round}-filters-open")));
}

/// The types of a low-pass of the filter simulator beyond the one its lists
/// come up with: the place in either list ("off" is the first item, "pt1",
/// which they come up with, the second) and the name in the files.
const LOWPASS_TYPES: [(usize, &str); 4] = [
    (2, "pt2"),
    (3, "pt3"),
    (4, "biquad-butter"),
    (5, "biquad-bessel"),
];
/// The place of "pt1" in those lists.
const LOWPASS_PT1: usize = 1;

/// A low-pass of the filter simulator: its type as its list names it and
/// its cut-off.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LowpassSet {
    /// `pt1`, `pt2`, `pt3`, `biquad-butter` or `biquad-bessel`.
    pub kind: &'static str,
    /// Cut-off, Hz.
    pub hz: u32,
}

/// The RPM notches of the filter simulator.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RpmSet {
    /// Harmonics: notches at the frequency and at its multiples.
    pub harmonics: u32,
    /// The frequency of the first, Hz.
    pub hz: u32,
    /// Their Q (the box holds a hundred times it).
    pub q: f64,
}

/// The static notch of the filter simulator, set by its "Notch Q".
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NotchSet {
    /// Centre, Hz.
    pub hz: u32,
    /// Its Q (the box holds a hundred times it).
    pub q: f64,
}

/// A set of filters typed into the filter simulator, which reads no log:
/// what a manifest says of it, so that `OpenPID`'s curves can be computed
/// for exactly these filters.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FilterSet {
    /// The loop rate of its list, Hz: the rate the filters run at.
    pub loop_rate_hz: u32,
    /// Motors: the RPM notches count once a motor in the printed delays.
    pub motors: u32,
    /// Low-pass 1.
    pub lowpass_1: LowpassSet,
    /// Low-pass 2.
    pub lowpass_2: LowpassSet,
    /// The RPM notches, as the window comes up.
    pub rpm: RpmSet,
    /// The static notch.
    pub notch: NotchSet,
    /// How the reference tool computes the filters and its plots (rounds
    /// 13, 14 and 16).
    pub rules: [&'static str; 5],
}

/// The loop rates of the filter simulator's list, Hz, in its order.
const LOOP_RATES: [u32; 5] = [8000, 4000, 3200, 2000, 1000];
/// The types of a low-pass in the filter simulator's lists, in their order.
const LOWPASS_KINDS: [&str; 6] = ["off", "pt1", "pt2", "pt3", "biquad-butter", "biquad-bessel"];

/// The set of filters round 17 types into the filter simulator, made for
/// no question: low-pass 1 of a type and a cut-off no round set, low-pass
/// 2 of another type, a static notch at another centre and Q, at a loop
/// rate of 4 kHz; the four motors and the RPM notches as the window comes
/// up.
pub const VERIFY_FILTERS: FilterSet = FilterSet {
    loop_rate_hz: 4000,
    motors: 4,
    lowpass_1: LowpassSet {
        kind: "pt2",
        hz: 210,
    },
    lowpass_2: LowpassSet {
        kind: "biquad-butter",
        hz: 420,
    },
    rpm: RpmSet {
        harmonics: 3,
        hz: 100,
        q: 5.0,
    },
    notch: NotchSet { hz: 330, q: 2.5 },
    rules: [
        "pt1: y += k·(x − y) with k = dt / (1 / (2π·fc) + dt) and dt = 1 / loop rate; pt2 and \
         pt3: two and three of them in a row, each at fc / sqrt(2^(1/n) − 1)",
        "biquad-butter: the second-order low-pass b = [(1 − cos w) / 2, 1 − cos w, \
         (1 − cos w) / 2], a = [1 + α, −2·cos w, 1 − α], w = 2π·fc / loop rate, \
         α = sin w / (2·Q), Q = 1/√2",
        "a notch: b = [1, −2·cos w, 1], a = [1 + α, −2·cos w, 1 − α] with its own Q; the RPM \
         notches at the frequency and at its multiples up to the harmonics",
        "the plots of a filter H: |H(f)|; the phase delay −arg H(f) / (2π·f) in ms; arg H(f) in \
         degrees; the response to a unit step, sample by sample",
        "the printed delay of a filter is its phase delay at 1 Hz; a column prints the sum of \
         its filters, an RPM notch counted once a motor; the total is the sum of the columns",
    ],
};

/// The filters the plan of `round` types into the filter simulator, when
/// it types a set of its own.
pub fn filter_set(round: u32) -> Option<FilterSet> {
    (round == VERIFICATION).then_some(VERIFY_FILTERS)
}

/// The filter simulator asked: the window opened from the main window; a
/// picture and copies of its eight plots as it comes up; a picture of each
/// of its lists held open; each of its three switches set, a picture (and
/// copies of the low-pass column for the first), and put back; four values
/// typed in, each with a picture and copies of the column it changes, and
/// put back.
fn filters_ask(steps: &mut Vec<Step>, round: u32) {
    let file = |what: &str| format!("r{round}-filters-{what}");
    let inside = |name: &str, at: (u32, u32)| click(FILTERS, "shot", name, at);
    filters_open(steps, round);
    for column in 0..filters_at::COLUMNS.len() {
        filters_copies(steps, round, "open", column);
    }
    for (name, at) in filters_at::LISTS {
        let list = inside(&format!("{name} (the list)"), at);
        list_shot(steps, list, file(&format!("list-{name}")));
    }
    for (k, (name, at)) in filters_at::SWITCHES.into_iter().enumerate() {
        steps.push(Step {
            mark: Some(true),
            ..inside(&format!("{name} on"), at)
        });
        steps.push(settle(FILTERS, Some(TIME_CHANGE_S), 1.5));
        steps.push(shot(FILTERS, file(name)));
        if k == 0 {
            filters_copies(steps, round, name, 0);
        }
        steps.push(inside(&format!("{name} off"), at));
        steps.push(pause(1500));
    }
    for b in filters_at::BOXES {
        let typed = |text: &str| Step {
            action: "type",
            text: Some(text.to_string()),
            ..inside(b.name, b.at)
        };
        steps.push(Step {
            mark: Some(true),
            ..typed(b.text)
        });
        steps.push(settle(FILTERS, Some(TIME_CHANGE_S), 1.5));
        steps.push(shot(FILTERS, file(b.name)));
        filters_copies(steps, round, b.name, b.column);
        steps.push(typed(b.back));
        steps.push(pause(1500));
    }
}

/// The rest of the slider tool: the start gains of round 13 typed in, the
/// PD ratio moved to 1.25, and then every other entry of its list chosen
/// in turn, a picture after each; the list is put back on its first entry.
fn sliders_more(steps: &mut Vec<Step>, round: u32) {
    let (button, _, at) = log_at::OTHERS[0];
    let file = |what: &str| format!("r{round}-sliders-{what}");
    let inside = |name: &str, at: (u32, u32)| click(SLIDERS, "slider", name, at);
    steps.push(click(LOG_VIEWER, "log", button, at));
    steps.push(pause(4000));
    for (name, at, text) in sliders_at::START {
        steps.push(Step {
            action: "type",
            text: Some(text.to_string()),
            ..inside(&format!("start {name}"), at)
        });
        steps.push(pause(300));
    }
    let (name, y) = sliders_at::ROWS[0];
    for _ in 0..5 {
        steps.push(inside(&format!("{name} up"), (sliders_at::UP_X, y)));
        steps.push(pause(300));
    }
    steps.push(shot(SLIDERS, file("betaflight")));
    let (x, first) = sliders_at::FIRMWARE_ITEM;
    let pick = |steps: &mut Vec<Step>, row: usize| {
        let name = sliders_at::FIRMWARE[row];
        steps.push(inside(
            &format!("{name} (the list)"),
            sliders_at::FIRMWARE_LIST,
        ));
        steps.push(pause(600));
        steps.push(inside(name, (x, first + 46 * row as u32)));
        steps.push(pause(1000));
    };
    for row in 1..sliders_at::FIRMWARE.len() {
        pick(steps, row);
        steps.push(shot(SLIDERS, file(sliders_at::FIRMWARE[row])));
    }
    pick(steps, 0);
}

/// An item of a list of the filter simulator chosen; the picture is marked
/// before the list is opened when `mark` says so.
fn filters_pick(steps: &mut Vec<Step>, list: &str, row: usize, mark: bool) {
    let at = filters_at::list(list);
    steps.push(Step {
        mark: mark.then_some(true),
        ..click(FILTERS, "shot", &format!("{list} (the list)"), at)
    });
    steps.push(pause(600));
    let item = throttle_at::item(at, row);
    steps.push(click(FILTERS, "shot", &format!("{list} item {row}"), item));
    steps.push(pause(400));
}

/// A value typed into a box of the filter simulator.
fn filters_type(steps: &mut Vec<Step>, name: &str, at: (u32, u32), text: &str, mark: bool) {
    steps.push(Step {
        action: "type",
        text: Some(text.to_string()),
        mark: mark.then_some(true),
        ..click(FILTERS, "shot", name, at)
    });
    steps.push(pause(400));
}

/// The wait for the plots of the filter simulator after a change, and a
/// picture of the window named after `what`.
fn filters_shot(steps: &mut Vec<Step>, round: u32, what: &str) {
    steps.push(settle(FILTERS, Some(TIME_CHANGE_S), 1.5));
    steps.push(shot(FILTERS, format!("r{round}-filters-{what}")));
}

/// The rest of the filter simulator, by the lists round 13 photographed.
/// Low-pass 1 as each of its other four types, with the plots of its
/// column copied; the other four loop rates, the same; one motor, one
/// harmonic, the RPM filter off, the notch off, a picture each; the notch
/// set by its cut-off; the impulse response and the chirp generator with
/// noise and with the filtered signal only; and a set of filters made for
/// no question, with all eight plots copied. Every list and box is put
/// back before the next is touched.
fn filters_more(steps: &mut Vec<Step>, round: u32) {
    filters_open(steps, round);
    for (row, name) in LOWPASS_TYPES {
        let what = format!("lowpass-1-{name}");
        filters_pick(steps, "lowpass-1", row, true);
        filters_shot(steps, round, &what);
        filters_copies(steps, round, &what, 0);
    }
    filters_pick(steps, "lowpass-1", LOWPASS_PT1, false);
    for (row, name) in [(1, "4k"), (2, "3k2"), (3, "2k"), (4, "1k")] {
        let what = format!("looprate-{name}");
        filters_pick(steps, "looprate", row, true);
        filters_shot(steps, round, &what);
        filters_copies(steps, round, &what, 0);
    }
    filters_pick(steps, "looprate", 0, false);
    // a list, the item for the picture, the item it came up with
    for (list, row, back, what) in [
        ("motors", 0, 3, "motors-1"),
        ("rpm-harmonics", 1, 3, "harmonics-1"),
        ("rpm-harmonics", 0, 3, "rpm-off"),
        ("notch-f", 0, 1, "notch-off"),
    ] {
        filters_pick(steps, list, row, true);
        filters_shot(steps, round, what);
        filters_pick(steps, list, back, false);
    }
    filters_pick(steps, "notch-q", 1, true);
    filters_type(
        steps,
        "Notch Cutoff",
        filters_at::NOTCH_CUTOFF,
        "400",
        false,
    );
    filters_shot(steps, round, "notch-cutoff-400");
    filters_copies(steps, round, "notch-cutoff-400", 1);
    filters_type(
        steps,
        "Notch Cutoff",
        filters_at::NOTCH_CUTOFF,
        "452",
        false,
    );
    filters_pick(steps, "notch-q", 0, false);
    filters_pick(steps, "signal", 1, true);
    filters_shot(steps, round, "impulse");
    for column in 0..filters_at::COLUMNS.len() {
        let (name, left, right) = filters_at::COLUMNS[column];
        let (row, top, bottom) = filters_at::ROWS[3];
        let file = format!("r{round}-filters-impulse-{name}-{row}");
        let plot = format!("{name} {row}");
        copy_axes(steps, FILTERS, &plot, (left, top, right, bottom), file);
    }
    filters_pick(steps, "signal", 2, true);
    filters_shot(steps, round, "chirp");
    filters_pick(steps, "noise", 2, true);
    filters_shot(steps, round, "chirp-noise-med");
    filters_pick(steps, "show", 0, true);
    filters_shot(steps, round, "chirp-noise-med-filtered");
    filters_pick(steps, "show", 2, false);
    filters_pick(steps, "noise", 0, false);
    filters_pick(steps, "signal", 0, false);
    // the set made for no question
    filters_pick(steps, "looprate", 1, true);
    filters_pick(steps, "motors", 5, false);
    filters_pick(steps, "rpm-harmonics", 2, false);
    for (name, at, text) in [
        ("lowpass 1 Hz", filters_at::LOWPASS_1_HZ, "180"),
        ("lowpass 2 Hz", filters_at::LOWPASS_2_HZ, "320"),
        ("RPM Hz", filters_at::RPM_HZ, "130"),
        ("RPM Q", filters_at::RPM_Q, "300"),
        ("Notch Q", filters_at::NOTCH_Q, "350"),
        ("Notch Center", filters_at::NOTCH_CENTER, "240"),
    ] {
        filters_type(steps, name, at, text, false);
    }
    filters_shot(steps, round, "holdout");
    for column in 0..filters_at::COLUMNS.len() {
        filters_copies(steps, round, "holdout", column);
    }
}

/// The list of the setup asked: the window opened from the main window, a
/// picture as it comes up, a picture with "Show Differences Only" set (and
/// the switch put back), and a picture of its list of logs held open.
fn setup_ask(steps: &mut Vec<Step>, round: u32) {
    let (button, _, at) = log_at::OTHERS[2];
    let file = |what: &str| format!("r{round}-setup-{what}");
    let inside = |name: &str, at: (u32, u32)| click(SETUP, "shot", name, at);
    steps.push(click(LOG_VIEWER, "log", button, at));
    steps.push(pause(4000));
    steps.push(shot(SETUP, file("open")));
    steps.push(inside("differences on", setup_at::DIFFERENCES));
    steps.push(pause(2000));
    steps.push(shot(SETUP, file("differences")));
    steps.push(inside("differences off", setup_at::DIFFERENCES));
    steps.push(pause(1000));
    let list = inside("logs (the list)", setup_at::LOG_LIST);
    list_shot(steps, list, file("list-logs"));
}

/// Copies of the four plots of the main window, named after `what`.
fn viewer_copies(steps: &mut Vec<Step>, round: u32, what: &str) {
    let (left, right) = viewer_at::SIDES;
    for (name, top, bottom) in viewer_at::ROWS {
        let file = format!("r{round}-log-{what}-{name}");
        copy_axes(steps, LOG_VIEWER, name, (left, top, right, bottom), file);
    }
}

/// The two buttons of the main window whose effect is not known, last in
/// the plan for that reason. "Markup": a click, a picture, a second click.
/// "Period": a click, the titles of the tool's windows into the log, a
/// picture, Esc. Both pictures are of the screen where the main window is,
/// as it is: should a button bring up a window, the main window is not
/// brought back over it.
fn viewer_unknown(steps: &mut Vec<Step>, round: u32) {
    let inside = |name: &str, at: (u32, u32)| click(LOG_VIEWER, "shot", name, at);
    let as_it_is = |what: &str| Step {
        front: Some(false),
        ..shot(LOG_VIEWER, format!("r{round}-log-{what}"))
    };
    steps.push(inside("Markup", viewer_at::MARKUP));
    steps.push(pause(1000));
    steps.push(as_it_is("markup"));
    steps.push(inside("Markup (again)", viewer_at::MARKUP));
    steps.push(pause(1000));
    steps.push(inside("Period", viewer_at::PERIOD));
    steps.push(pause(2000));
    steps.push(Step {
        action: "windows",
        ..Step::default()
    });
    steps.push(as_it_is("period"));
    steps.push(Step {
        action: "key",
        key: Some("esc"),
        ..Step::default()
    });
    steps.push(pause(400));
}

/// The main window asked what its lines read, when the round has a probe
/// for it (the first such probe is the log it shows); says whether it has.
/// Copies of its four plots as it comes up; every line of its check-list
/// that is off switched on, a picture and the four copies again; the y
/// scale typed and put back; the list of the line smoothing held open; a
/// click into the roll plot and one on the button ">", a picture each; and
/// the two buttons of [`viewer_unknown`].
fn viewer_lines(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) -> bool {
    let Some(probe) = probes.iter().find(|p| p.area == Area::LogViewer) else {
        return false;
    };
    let file = |what: &str| format!("r{round}-log-{what}");
    let inside = |name: &str, at: (u32, u32)| click(LOG_VIEWER, "shot", name, at);
    viewer_copies(steps, round, "open");
    for (name, y, _) in viewer_at::LINES.into_iter().filter(|(_, _, on)| !on) {
        steps.push(click(LOG_VIEWER, "log", name, (viewer_at::BOX_X, y)));
        steps.push(pause(400));
    }
    steps.push(settle(LOG_VIEWER, None, 1.5));
    steps.push(shot(LOG_VIEWER, file("all")));
    viewer_copies(steps, round, "all");
    let typed = |text: &str| Step {
        action: "type",
        text: Some(text.to_string()),
        ..inside("y scale", viewer_at::Y_SCALE)
    };
    steps.push(Step {
        mark: Some(true),
        ..typed(viewer_at::Y_SCALE_TYPED)
    });
    steps.push(settle(LOG_VIEWER, Some(TIME_CHANGE_S), 1.5));
    let name = format!("yscale-{}", viewer_at::Y_SCALE_TYPED);
    steps.push(shot(LOG_VIEWER, file(&name)));
    steps.push(typed(viewer_at::Y_SCALE_DEFAULT));
    steps.push(pause(1500));
    let list = inside("line smoothing (the list)", viewer_at::SMOOTHING_LIST);
    list_shot(steps, list, file("list-smoothing"));
    let at = viewer_at::roll_at(viewer_at::CURSOR_S, probe.flight.duration_s);
    steps.push(inside("roll plot", at));
    steps.push(pause(600));
    let name = format!("cursor-{}s", viewer_at::CURSOR_S);
    steps.push(shot(LOG_VIEWER, file(&name)));
    steps.push(inside(">", viewer_at::NEXT));
    steps.push(pause(600));
    steps.push(shot(LOG_VIEWER, file("cursor-next")));
    viewer_unknown(steps, round);
    true
}

/// The round that asks every window what the rounds before it left open:
/// its plan is made for the logs of that round and for no others.
pub const OPEN_ENDS: u32 = 16;
/// The verification round: every window asked again on logs made for no
/// question, the other channels of the spectrum window, the stick's path
/// into the step response and two leftovers of round [`OPEN_ENDS`]. Its
/// plan is made for its own logs.
pub const VERIFICATION: u32 = 17;
/// How long the main window is given to draw before its picture is taken,
/// after a log or a smoothing level was chosen or a value was typed,
/// milliseconds. A fixed time: the script cannot wait for this window to
/// stop changing once something was typed into it, because the text cursor
/// left in the box goes on blinking (round 15 waited 205 s for it).
const VIEWER_DRAW_MS: u32 = 3000;
/// The same after lines were ticked: some lines appear a few seconds after
/// their tick (round 15: the debug lines).
const VIEWER_TICK_MS: u32 = 4000;

/// Name of a picture or of a copied plot of the main window with the log
/// `probe` drawn: `what` is the setting ("" as the log comes up, "open",
/// "smooth-low", "dterm" …) and `plot` the plot ("" for a picture of the
/// window).
pub fn viewer_file(round: u32, probe: &str, what: &str, plot: &str) -> String {
    named(format!("r{round}-{probe}-log"), [what, plot])
}

/// A click in the main window, at a position of a picture of it.
fn viewer_click(name: &str, at: (u32, u32)) -> Step {
    click(LOG_VIEWER, "shot", name, at)
}

/// Clicks on the boxes of `lines` of the check-list, one after the other.
/// A click turns a line over: with `back` unset the lines leave the state
/// the window comes up with, with `back` set they return to it.
fn viewer_ticks(steps: &mut Vec<Step>, lines: &[&str], back: bool) {
    let listed = |line: &&str| {
        viewer_at::LINES
            .into_iter()
            .find(|(name, _, _)| name == line)
    };
    for (name, y, default) in lines.iter().filter_map(listed) {
        let state = if default == back { "on" } else { "off" };
        let at = (viewer_at::BOX_X, y);
        steps.push(click(LOG_VIEWER, "log", &format!("{name} {state}"), at));
        steps.push(pause(400));
    }
}

/// A level of the line smoothing of the main window chosen in its list,
/// and the time the window is given to draw.
fn viewer_smoothing(steps: &mut Vec<Step>, (level, at): (&str, (u32, u32))) {
    let list = viewer_at::SMOOTHING_LIST;
    steps.push(viewer_click("line smoothing (the list)", list));
    steps.push(pause(600));
    steps.push(viewer_click(&format!("line smoothing {level}"), at));
    steps.push(pause(VIEWER_DRAW_MS));
}

/// A number typed into the y scale of the main window, and the time the
/// window is given to draw: never a wait for it to stop changing.
fn viewer_scale(steps: &mut Vec<Step>, text: &str) {
    steps.push(Step {
        action: "type",
        text: Some(text.to_string()),
        ..viewer_click("y scale", viewer_at::Y_SCALE)
    });
    steps.push(pause(VIEWER_DRAW_MS));
}

/// A log of a round in the main window: its id and its row in the list of
/// the logs.
#[derive(Clone, Copy)]
struct ViewerLog<'a> {
    round: u32,
    id: &'a str,
    row: usize,
}

impl<'a> ViewerLog<'a> {
    /// The first log of `probes` whose name starts with `prefix`. Its row
    /// in the tool's lists is taken to be its place among the logs, which
    /// are opened in the order of their names: an assumption, since no
    /// round has opened logs in another order, and the reason a picture
    /// follows every choice of a log (the list in it names the log drawn).
    fn of(round: u32, probes: &'a [Probe], prefix: &str) -> Option<ViewerLog<'a>> {
        let row = probes.iter().position(|p| p.id.starts_with(prefix))?;
        let id = probes[row].id;
        Some(ViewerLog { round, id, row })
    }

    /// A picture of the window named after the setting `what`.
    fn shot(self, steps: &mut Vec<Step>, what: &str) {
        let file = viewer_file(self.round, self.id, what, "");
        steps.push(shot(LOG_VIEWER, file));
    }

    /// The log chosen in the list of the logs, time to draw it, a picture.
    fn select(self, steps: &mut Vec<Step>) {
        let list = viewer_at::FILE_LIST;
        let name = format!("log {}", self.id);
        steps.push(viewer_click(&format!("{name} (the list)"), list));
        steps.push(pause(600));
        steps.push(viewer_click(&name, throttle_at::item(list, self.row)));
        steps.push(pause(VIEWER_DRAW_MS));
        self.shot(steps, "");
    }

    /// Copies of the plots `plots` (places in [`viewer_at::ROWS`]), named
    /// after the setting `what`. A rate plot is copied with four lines
    /// ticked at most, the lower plot with the five it comes up with at
    /// most: with every line ticked the tool copied nothing (round 15).
    fn copies(self, steps: &mut Vec<Step>, what: &str, plots: &[usize]) {
        let (left, right) = viewer_at::SIDES;
        for (name, top, bottom) in plots.iter().filter_map(|k| viewer_at::ROWS.get(*k)) {
            let file = viewer_file(self.round, self.id, what, name);
            copy_axes(steps, LOG_VIEWER, name, (left, *top, right, *bottom), file);
        }
    }

    /// The button ">" of the player pressed `times` times, a picture after
    /// each: the read-out under the panel names a time and what the gyro
    /// and the motors hold there.
    fn next(self, steps: &mut Vec<Step>, times: usize) {
        for k in 1..=times {
            steps.push(viewer_click(">", viewer_at::NEXT));
            steps.push(pause(600));
            self.shot(steps, &format!("next-{k}"));
        }
    }

    /// The log that counts its samples: the three rate plots as they come
    /// up; each level of the line smoothing in turn with a picture and the
    /// plots of the pulses (roll) and of the noise (yaw) copied, and "off"
    /// again; ">" three times.
    fn samples(self, steps: &mut Vec<Step>) {
        self.select(steps);
        self.copies(steps, "open", &[0, 1, 2]);
        for item in viewer_at::SMOOTHING {
            let what = format!("smooth-{}", item.0);
            viewer_smoothing(steps, item);
            self.shot(steps, &what);
            self.copies(steps, &what, &[0, 2]);
        }
        viewer_smoothing(steps, viewer_at::SMOOTHING[0]);
        self.next(steps, 3);
    }

    /// The log that counts its samples at another rate: the roll plot (the
    /// pulses) as it comes up, with the line smoothing off, and at "med",
    /// a picture of each; "off" again.
    fn smoothing_pair(self, steps: &mut Vec<Step>) {
        self.select(steps);
        self.copies(steps, "smooth-off", &[0]);
        let med = viewer_at::SMOOTHING[2];
        viewer_smoothing(steps, med);
        let what = format!("smooth-{}", med.0);
        self.shot(steps, &what);
        self.copies(steps, &what, &[0]);
        viewer_smoothing(steps, viewer_at::SMOOTHING[0]);
    }

    /// The log whose gyro rises and falls evenly: the lower plot as it
    /// comes up (which throttle it draws); the two gyro lines and the two
    /// D-term lines on the rate plots, at the y scale the window comes up
    /// with and at [`viewer_at::Y_SCALE_WIDE`]; the four motor speeds alone
    /// on the lower plot (whether its axis grows past 150). The check-list
    /// and the y scale are put back.
    fn derivative(self, steps: &mut Vec<Step>) {
        // four lines on a rate plot: the setpoint makes room for the rest
        const RATES: [&str; 4] = ["Gyro(pf)", "D-term(pf)", "D-term", "Set point"];
        const SPEEDS: [&str; 9] = [
            "Motor 1/5",
            "Motor 2/6",
            "Motor 3/7",
            "Motor 4/8",
            "Throttle",
            "RPS 1/5",
            "RPS 2/6",
            "RPS 3/7",
            "RPS 4/8",
        ];
        self.select(steps);
        self.copies(steps, "open", &[3]);
        viewer_ticks(steps, &RATES, false);
        steps.push(pause(VIEWER_TICK_MS));
        self.shot(steps, "dterm");
        self.copies(steps, "dterm", &[0, 1]);
        let wide = format!("dterm-yscale-{}", viewer_at::Y_SCALE_WIDE);
        viewer_scale(steps, viewer_at::Y_SCALE_WIDE);
        self.shot(steps, &wide);
        self.copies(steps, &wide, &[0, 1]);
        viewer_scale(steps, viewer_at::Y_SCALE_DEFAULT);
        viewer_ticks(steps, &SPEEDS, false);
        steps.push(pause(VIEWER_TICK_MS));
        self.shot(steps, "rps");
        self.copies(steps, "rps", &[3]);
        viewer_ticks(steps, &SPEEDS, true);
        viewer_ticks(steps, &RATES, true);
        steps.push(pause(VIEWER_TICK_MS));
    }

    /// The log whose motor outputs are not digital: the lower plot as it
    /// comes up, and ">" once for the read-out of the motors.
    fn outputs(self, steps: &mut Vec<Step>) {
        self.select(steps);
        self.copies(steps, "open", &[3]);
        self.next(steps, 1);
    }
}

/// The two buttons of the main window whose effect is not known, tried
/// with clicks into the roll plot (round 15 pressed each once and saw
/// nothing change): "Markup", a click at a third and one at two thirds of
/// the width of the plot, a picture, "Markup" again; "Period", the same two
/// clicks, a picture, the titles of the tool's windows into the log, Esc.
/// The pictures are of the screen where the main window is, as it is.
fn viewer_marks(steps: &mut Vec<Step>, round: u32) {
    let places = [viewer_at::roll_at(1.0, 3.0), viewer_at::roll_at(2.0, 3.0)];
    let buttons = [
        ("Markup", viewer_at::MARKUP, "markup"),
        ("Period", viewer_at::PERIOD, "period"),
    ];
    for (button, at, what) in buttons {
        steps.push(viewer_click(button, at));
        steps.push(pause(1000));
        for (k, place) in places.into_iter().enumerate() {
            let name = format!("roll plot at {} of 3 ({button})", k + 1);
            steps.push(viewer_click(&name, place));
            steps.push(pause(600));
        }
        steps.push(Step {
            front: Some(false),
            ..shot(LOG_VIEWER, format!("r{round}-log-{what}"))
        });
        if button == "Markup" {
            steps.push(viewer_click("Markup (again)", at));
            steps.push(pause(1000));
        }
    }
    steps.push(Step {
        action: "windows",
        ..Step::default()
    });
    steps.push(Step {
        action: "key",
        key: Some("esc"),
        ..Step::default()
    });
    steps.push(pause(400));
}

/// The stairs of round [`OPEN_ENDS`]: the logs of that round whose names
/// start with "w", each with its row in the tool's lists.
fn open_stairs(probes: &[Probe]) -> impl Iterator<Item = (usize, &Probe)> {
    let stairs = |(_, p): &(usize, &Probe)| p.id.starts_with('w');
    probes.iter().enumerate().filter(stairs)
}

/// The steps of round [`OPEN_ENDS`] after the logs are open, the most
/// valuable first.
///
/// 1. The main window: its list of logs held open; then the three logs
///    made for it, each chosen in that list with a picture after the
///    choice ([`ViewerLog::samples`], [`ViewerLog::derivative`],
///    [`ViewerLog::outputs`]); then the two buttons of [`viewer_marks`].
/// 2. The window of frequency against throttle, as round 10 ran it, for
///    the eight stairs: the log in all four columns, a Run, a picture, and
///    one map copied, the unfiltered gyro of roll (the distance between
///    the segments is read off one map; the printed numbers of the others
///    are in the picture). Then the first log of the main window there: a
///    picture and the map of the unfiltered gyro of yaw, whose tone lies
///    in the column of the throttle the window reads.
/// 3. The window of frequency against time for the two shortest stairs, as
///    round 11 ran it for a log: a picture and the three maps. For a log
///    too short the window draws no map; the first copy then fails and the
///    other two are passed over ([`Plan::give_up`]).
/// 4. The filter simulator: low-pass 2 as each of the types beyond "pt1",
///    a picture each (the printed delays), and "pt1" again.
/// 5. The summary of the tables the tool has decoded the logs into, last:
///    by then every log has been drawn.
fn open_ends(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    let files = viewer_click("logs (the list)", viewer_at::FILE_LIST);
    list_shot(steps, files, format!("r{round}-log-list-files"));
    let viewer = |prefix: &str| ViewerLog::of(round, probes, prefix);
    if let Some(log) = viewer("v3-") {
        log.samples(steps);
    }
    let derivative = viewer("v1-");
    if let Some(log) = derivative {
        log.derivative(steps);
    }
    if let Some(log) = viewer("v2-") {
        log.outputs(steps);
    }
    viewer_marks(steps, round);

    open_spectrum(steps, round);
    throttle_open(steps, round);
    let stairs = open_stairs(probes).map(|(row, p)| (row, p.id, 0));
    // roll for the stairs, yaw (2) for the tone at a held throttle
    let held = derivative.map(|log| (log.row, log.id, 2));
    for (row, id, axis) in stairs.chain(held) {
        let log = ThrottleLog { round, id };
        log.choose(steps, row);
        log.run(steps, "");
        log.copy(steps, "", 0, axis);
    }

    time_open(steps, round);
    let short = |(_, p): &(usize, &Probe)| p.flight.duration_s < 15.0;
    for (row, probe) in open_stairs(probes).filter(short) {
        time_first(steps, round, probe.id, row);
    }

    filters_open(steps, round);
    for (row, name) in LOWPASS_TYPES {
        filters_pick(steps, "lowpass-2", row, true);
        filters_shot(steps, round, &format!("lowpass-2-{name}"));
    }
    filters_pick(steps, "lowpass-2", LOWPASS_PT1, false);

    steps.push(Step {
        action: "tables",
        ..Step::default()
    });
}

/// The row of the log `id` among `probes`, which is its row in the tool's
/// lists (the logs are opened in the order of their names).
fn row_of(probes: &[Probe], id: &str) -> Option<usize> {
    probes.iter().position(|p| p.id == id)
}

/// A log of the spectrum window as the window comes up (round 17): the log
/// chosen in row `row` of its list, a Run, a picture and copies of the six
/// plots at "medium".
fn spectrum_as_it_comes(steps: &mut Vec<Step>, round: u32, id: &str, row: usize) {
    let at = |name: &str, position: (u32, u32)| click(SPECTRUM, "shot", name, position);
    let y = log_row(spectrum_at::LOG.1, row);
    steps.push(Step {
        mark: Some(true),
        ..at(&format!("log {id}"), (spectrum_at::LOG.0, y))
    });
    steps.push(pause(250));
    steps.push(at("Run", spectrum_at::RUN));
    steps.push(settle(SPECTRUM, Some(20.0), 1.2));
    let level = "medium";
    steps.push(shot(SPECTRUM, spectrum_shot(round, id, level)));
    for (axis, column) in copied(round, level) {
        copy_plot(
            steps,
            axis,
            column,
            spectrum_copy(round, id, level, axis, column),
        );
    }
}

/// Name of a picture or of a copied plot of the spectrum window with one
/// channel of its list chosen: `channel` is its name in the files, `plot`
/// the axis and the column ("" for a picture of the window).
pub fn channel_file(round: u32, probe: &str, channel: &str, plot: &str) -> String {
    named(format!("r{round}-{probe}-spectrum"), [channel, plot])
}

/// The other channels of the spectrum window's list (S8): the log chosen
/// in row `row`, the plots' lower end typed down to [`Y_MIN_CHANNELS`], a
/// Run and a picture of the window with its list as it comes up; then
/// every entry of [`SPECTRUM_CHANNELS`] picked in turn, a Run and a picture
/// after each pick, and the plots of roll copied, below 100 Hz and over
/// the whole range, for the entries `OpenPID` has a counterpart of; the
/// "Motors" entry draws one large plot of every motor in place of the six,
/// which is copied alone ([`MOTORS_COPY`]). "Gyro" is picked again at the
/// end, and the lower end typed back.
fn spectrum_channels(steps: &mut Vec<Step>, round: u32, id: &str, row: usize) {
    let at = |name: &str, position: (u32, u32)| click(SPECTRUM, "shot", name, position);
    let y = log_row(spectrum_at::LOG.1, row);
    steps.push(at(&format!("log {id}"), (spectrum_at::LOG.0, y)));
    steps.push(pause(250));
    spectrum_type(steps, "Y min", spectrum_at::Y_MIN, Y_MIN_CHANNELS);
    steps.push(Step {
        mark: Some(true),
        ..at("Run", spectrum_at::RUN)
    });
    steps.push(settle(SPECTRUM, Some(20.0), 1.2));
    steps.push(shot(SPECTRUM, channel_file(round, id, "list", "")));
    for (k, (name, entry, copied)) in SPECTRUM_CHANNELS.into_iter().enumerate() {
        steps.push(Step {
            mark: Some(true),
            ..at(&format!("channel {entry}"), spectrum_at::channel(k))
        });
        steps.push(pause(400));
        steps.push(at("Run", spectrum_at::RUN));
        steps.push(settle(SPECTRUM, Some(20.0), 1.2));
        steps.push(shot(SPECTRUM, channel_file(round, id, name, "")));
        if copied && name == "motors" {
            let file = channel_file(round, id, name, MOTORS_COPY);
            copy_axes(steps, SPECTRUM, "motors", spectrum_at::MOTORS_PLOT, file);
        } else if copied {
            for column in [1, 0] {
                let plot = format!("{}-{}", AXES[0], COLUMNS[column]);
                copy_plot(steps, 0, column, channel_file(round, id, name, &plot));
            }
        }
    }
    let (_, gyro, _) = SPECTRUM_CHANNELS[0];
    steps.push(at(&format!("channel {gyro}"), spectrum_at::channel(0)));
    steps.push(pause(400));
    spectrum_type(steps, "Y min", spectrum_at::Y_MIN, Y_MIN_DEFAULT);
}

/// The maps of a log in the two windows of maps as each comes up, both
/// opened from the spectrum window: the throttle window with the log in its
/// four columns, a Run, a picture and its six plots (the unfiltered and the
/// filtered gyro); the time window with the log, the unfiltered gyro and
/// then the filtered one, a picture and the three plots each, and the
/// unfiltered gyro chosen again.
fn maps_as_they_come(steps: &mut Vec<Step>, round: u32, id: &str, row: usize) {
    throttle_open(steps, round);
    let log = ThrottleLog { round, id };
    log.choose(steps, row);
    log.capture(steps, "");
    time_open(steps, round);
    time_first(steps, round, id, row);
    let gyro = TIME_CHANNELS[1];
    time_changed(steps, round, id, gyro.0, |steps| time_channel(steps, gyro));
    time_channel(steps, TIME_CHANNELS[0]);
}

/// A log of the step response window as the window comes up: the plots of
/// the earlier runs cleared ("Reset"; every Run adds a curve of its own to
/// them otherwise), the log chosen in row `row` of its list, a Run, a
/// picture, and copies of the response and the Peak of the three axes.
fn step_as_it_comes(steps: &mut Vec<Step>, round: u32, id: &str, row: usize) {
    let log = StepLog { round, id };
    steps.push(step_click("Reset", step_at::RESET));
    steps.push(pause(1500));
    log.choose(steps, row);
    log.run(steps, Some(""));
    log.copies(steps, "", &NINE[..2]);
}

/// The place of `item` in `list`, the first item when it is not there.
fn place<T: PartialEq>(list: &[T], item: &T) -> usize {
    list.iter().position(|x| x == item).unwrap_or(0)
}

/// The filter simulator with the set of filters `set` typed in: the window
/// opened, the loop rate and the types of the two low-passes picked from
/// their lists with a picture after each pick, the cut-offs of the
/// low-passes and the centre and Q of the static notch typed, a picture,
/// and copies of the eight plots, named after `what`.
fn filters_typed(steps: &mut Vec<Step>, round: u32, set: &FilterSet, what: &str) {
    filters_open(steps, round);
    let picks = [
        ("looprate", place(&LOOP_RATES, &set.loop_rate_hz)),
        ("lowpass-1", place(&LOWPASS_KINDS, &set.lowpass_1.kind)),
        ("lowpass-2", place(&LOWPASS_KINDS, &set.lowpass_2.kind)),
    ];
    for (list, row) in picks {
        filters_pick(steps, list, row, true);
        filters_shot(steps, round, &format!("{what}-{list}"));
    }
    let q = |q: f64| format!("{:.0}", 100.0 * q);
    let typed = [
        (
            "lowpass 1 Hz",
            filters_at::LOWPASS_1_HZ,
            set.lowpass_1.hz.to_string(),
        ),
        (
            "lowpass 2 Hz",
            filters_at::LOWPASS_2_HZ,
            set.lowpass_2.hz.to_string(),
        ),
        (
            "Notch Center",
            filters_at::NOTCH_CENTER,
            set.notch.hz.to_string(),
        ),
        ("Notch Q", filters_at::NOTCH_Q, q(set.notch.q)),
    ];
    for (k, (name, at, text)) in typed.into_iter().enumerate() {
        filters_type(steps, name, at, &text, k == 0);
    }
    filters_shot(steps, round, what);
    for column in 0..filters_at::COLUMNS.len() {
        filters_copies(steps, round, what, column);
    }
}

/// The steps of round [`VERIFICATION`] after the logs are open, the most
/// valuable first; a picture follows every choice of a log and every pick
/// in a list.
///
/// 1. The spectrum window: the two flights made for it as it comes up
///    ([`spectrum_as_it_comes`]); then the probe of the other channels,
///    every entry of the list of channels in turn ([`spectrum_channels`]).
/// 2. The maps of the flight made for them in both windows of maps
///    ([`maps_as_they_come`]).
/// 3. The step response window: the two flights made for it as it comes
///    up ([`step_as_it_comes`]).
/// 4. The main window: the flight made for it chosen, its four plots
///    copied as they come up (two lines on each plot of an axis, the four
///    motors and the throttle on the lower one, which round 16 copied).
/// 5. The filter simulator with [`VERIFY_FILTERS`] ([`filters_typed`]).
/// 6. The step response window again for the stick's path.
/// 7. The main window for the log that counts its samples at 2 and 4 kHz
///    ([`ViewerLog::smoothing_pair`]).
/// 8. The short stairs: the throttle window, a picture and the map of the
///    unfiltered gyro of roll; the time window, a picture and the three
///    maps.
fn verification(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    let row = |id: &str| row_of(probes, id).map(|row| (row, probes[row].id));
    spectrum_open(steps, round);
    for (row, id) in ["e1-spec-1k", "e2-spec-4k"].into_iter().filter_map(row) {
        spectrum_as_it_comes(steps, round, id, row);
    }
    if let Some((row, id)) = row("i1-chan-2k") {
        spectrum_channels(steps, round, id, row);
    }
    if let Some((row, id)) = row("e5-maps-2k") {
        maps_as_they_come(steps, round, id, row);
    }
    step_open(steps, round);
    for (row, id) in ["e3-step-2k", "e4-step-8k"].into_iter().filter_map(row) {
        step_as_it_comes(steps, round, id, row);
    }
    if let Some(log) = ViewerLog::of(round, probes, "e6-") {
        log.select(steps);
        log.copies(steps, "open", &[0, 1, 2, 3]);
    }
    filters_typed(steps, round, &VERIFY_FILTERS, "verify");
    if let Some((row, id)) = row("j1-path-2k") {
        step_as_it_comes(steps, round, id, row);
    }
    for prefix in ["x1-", "x2-"] {
        if let Some(log) = ViewerLog::of(round, probes, prefix) {
            log.smoothing_pair(steps);
        }
    }
    let short: Vec<(usize, &str)> = ["x3-len11-2k", "x4-len12-2k", "x5-len13-2k"]
        .into_iter()
        .filter_map(row)
        .collect();
    for (row, id) in &short {
        let log = ThrottleLog { round, id };
        log.choose(steps, *row);
        log.run(steps, "");
        log.copy(steps, "", 0, 0);
    }
    for (row, id) in &short {
        time_first(steps, round, id, *row);
    }
}

/// The round that confirms decision D-012 = A ("Without the first and last
/// second", #200) and the fixed `compare` on logs made for no question,
/// narrows the open ends of the stretch and asks what no round asked yet.
/// Its plan is made for its own logs.
pub const CONFIRMATION: u32 = 18;

/// The first round run by hand: from round 19 on (W3) the rounds are run
/// on the web edition 1.0 of the reference tool, which the owner drives
/// in the browser, and the pack holds no capture plan, script or note (the
/// script clicks where version 0.94 has its buttons and fits no other).
pub const BY_HAND_FROM: u32 = 19;

/// Whether the pack of `round` holds a capture plan: every round before
/// [`BY_HAND_FROM`].
pub fn planned(round: u32) -> bool {
    round < BY_HAND_FROM
}

/// The logs of round [`CONFIRMATION`] each part of its plan is for, by
/// name: the spectrum window as it comes up; the flight of both maps; the
/// step response window as it comes up; the main window; the short
/// stairs, whose maps are copied in both windows; the logs of earlier
/// rounds read from sample `fs + 3`, in the throttle window and in the
/// time window; the log that pauses, also in the time window; the second
/// log of the pair of the setup list.
const CONFIRM_SPECTRA: [&str; 4] = ["b1-spc-2k", "b3-spc-8k", "b2-clk-2k", "b4-gap-2k"];
const CONFIRM_MAPS: &str = "b1-spc-2k";
const CONFIRM_STEPS: [&str; 3] = ["d1-stp-1k", "d2-stp-4k", "d3-win-2k"];
const CONFIRM_VIEWER: &str = "d1-";
const SHORT_STAIRS: [&str; 6] = ["l1-2k", "l2-2k", "l3-2k", "l4-2k", "l5-2k", "l6-1k"];
const LATER_THROTTLE: [&str; 2] = ["z8-short-2k", "a9-flight-2k"];
const LATER_TIME: [&str; 1] = ["f4-short-2k"];
const PAUSED_LOG: &str = "b4-gap-2k";
const PAIR_SECOND: &str = "b2-clk-2k";

/// The channels of the time window's list beyond the gyro that round
/// [`CONFIRMATION`] draws a map of (F1): the name in the files and the
/// place in the list, taken to be that of the spectrum window's list
/// ([`SPECTRUM_CHANNELS`]; a picture of the open list is taken first).
const TIME_OTHER_CHANNELS: [(&str, usize); 2] = [("dterm", 2), ("piderror", 5)];

/// How long the pointer rests on a plot before its picture is taken,
/// milliseconds: long enough for a read-out that follows the pointer.
const POINTER_MS: u32 = 1500;

/// The rows of `ids` among `probes`, with the names, in the order of `ids`.
fn rows_of<'a>(probes: &'a [Probe], ids: &[&str]) -> Vec<(usize, &'a str)> {
    ids.iter()
        .filter_map(|id| row_of(probes, id).map(|row| (row, probes[row].id)))
        .collect()
}

/// The step response window for the logs of [`CONFIRM_STEPS`] as it comes
/// up, and the probe of the windows also from windows of the lengths of
/// [`windows_captured`]: the length picked, a Run, a picture, the response
/// and the Peak copied; the length the window came up with again.
fn confirm_steps(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    step_open(steps, round);
    for (row, id) in rows_of(probes, &CONFIRM_STEPS) {
        step_as_it_comes(steps, round, id, row);
        let seconds = probes.get(row).map_or(&[][..], windows_captured);
        let log = StepLog { round, id };
        let mut now = DEFAULT_SETTING;
        for s in seconds {
            let to = (0, *s);
            step_set(steps, now, to);
            now = to;
            let what = setting_name(now);
            log.run(steps, Some(&what));
            log.copies(steps, &what, &NINE[..2]);
        }
        step_set(steps, now, DEFAULT_SETTING);
    }
}

/// The maps of the D term and of the PID error in the time window (F1),
/// for the flight of the maps while the window shows it: the list of
/// channels held open for a picture, then each channel of
/// [`TIME_OTHER_CHANNELS`] picked in turn, a picture and the three maps
/// each, and the unfiltered gyro picked again. (The throttle window's
/// picture of that flight shows its two D-term columns, T1; their plots
/// are not copied: their places were never measured.)
fn time_other_channels(steps: &mut Vec<Step>, round: u32, id: &str) {
    let list = time_click("channel (the list)", time_at::CHANNEL_LIST);
    list_shot(steps, list, time_file(round, id, "list-channels", ""));
    for channel in TIME_OTHER_CHANNELS {
        time_changed(steps, round, id, channel.0, |steps| {
            time_channel(steps, channel);
        });
    }
    time_channel(steps, TIME_CHANNELS[0]);
}

/// The open ends of the stretch: the short stairs in the throttle window
/// (a picture and the map of the unfiltered gyro of roll) and in the time
/// window (a picture and the three maps); the logs read from `fs + 3`
/// before, each chosen alone in the window it was read in (the throttle
/// window: the maps of both gyro channels of roll; the time window: the
/// three maps); and the log that pauses, in the time window.
fn stretch_ends(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    for (row, id) in rows_of(probes, &SHORT_STAIRS) {
        let log = ThrottleLog { round, id };
        log.choose(steps, row);
        log.run(steps, "");
        log.copy(steps, "", 0, 0);
    }
    for (row, id) in rows_of(probes, &LATER_THROTTLE) {
        let log = ThrottleLog { round, id };
        log.choose(steps, row);
        log.run(steps, "");
        log.copy(steps, "", 0, 0);
        log.copy(steps, "", 1, 0);
    }
    let timed = [&SHORT_STAIRS[..], &LATER_TIME, &[PAUSED_LOG]].concat();
    for (row, id) in rows_of(probes, &timed) {
        time_first(steps, round, id, row);
    }
}

/// The setup list for the pair of logs that differ in one header (U13):
/// the window opened, a picture; its list of logs held open, a picture;
/// the second log of the pair chosen in that list, so that the window
/// shows the pair should its other column hold the log of the row before
/// (as it held the second log with two, round 13), a picture; "Show
/// Differences Only" on, a picture, and off again.
fn setup_pair(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    let Some((row, id)) = rows_of(probes, &[PAIR_SECOND]).pop() else {
        return;
    };
    let (button, _, at) = log_at::OTHERS[2];
    let file = |what: &str| format!("r{round}-setup-{what}");
    let inside = |name: &str, at: (u32, u32)| click(SETUP, "shot", name, at);
    steps.push(click(LOG_VIEWER, "log", button, at));
    steps.push(pause(4000));
    steps.push(shot(SETUP, file("open")));
    let list = inside("logs (the list)", setup_at::LOG_LIST);
    list_shot(steps, list, file("list-logs"));
    steps.push(inside("logs (the list)", setup_at::LOG_LIST));
    steps.push(pause(600));
    let item = throttle_at::item(setup_at::LOG_LIST, row);
    steps.push(inside(&format!("log {id}"), item));
    steps.push(pause(2000));
    steps.push(shot(SETUP, file("pair")));
    steps.push(inside("differences on", setup_at::DIFFERENCES));
    steps.push(pause(2000));
    steps.push(shot(SETUP, file("pair-differences")));
    steps.push(inside("differences off", setup_at::DIFFERENCES));
    steps.push(pause(1000));
}

/// The pointer on a plot of `window` (T4, F7, S6): a picture of the window
/// brought to the front, the pointer resting in the middle of the plot
/// `axes` and a picture as the window is, a click there and a picture
/// again. Last in the plan: whatever the click leaves on the plot stays
/// out of every copy.
fn pointer(steps: &mut Vec<Step>, window: &'static str, axes: (u32, u32, u32, u32), what: &str) {
    let (left, top, right, bottom) = axes;
    let middle = (u32::midpoint(left, right), u32::midpoint(top, bottom));
    let as_it_is = |file: String| Step {
        front: Some(false),
        ..shot(window, file)
    };
    steps.push(shot(window, format!("{what}-before")));
    steps.push(hover(window, format!("{what} plot"), middle, POINTER_MS));
    steps.push(as_it_is(format!("{what}-hover")));
    steps.push(Step {
        front: Some(false),
        ..click(window, "shot", &format!("{what} plot (click)"), middle)
    });
    steps.push(pause(POINTER_MS));
    steps.push(as_it_is(format!("{what}-click")));
    steps.push(Step {
        action: "key",
        key: Some("esc"),
        ..Step::default()
    });
    steps.push(pause(400));
}

/// The pointer on the map of the unfiltered gyro of roll in the throttle
/// window and in the time window, and on the plot of roll over the whole
/// range in the spectrum window.
fn pointers(steps: &mut Vec<Step>, round: u32) {
    let (_, left, right) = throttle_at::COLUMNS[0];
    let (top, bottom) = throttle_at::ROWS[0];
    let throttle = (left, top, right, bottom);
    pointer(
        steps,
        THROTTLE,
        throttle,
        &format!("r{round}-pointer-throttle"),
    );
    let (left, right) = time_at::SIDES;
    let (top, bottom) = time_at::ROWS[0];
    pointer(
        steps,
        TIME,
        (left, top, right, bottom),
        &format!("r{round}-pointer-time"),
    );
    let spectrum = PLOTS[0][0];
    pointer(
        steps,
        SPECTRUM,
        spectrum,
        &format!("r{round}-pointer-spectrum"),
    );
}

/// The steps of round [`CONFIRMATION`] after the logs are open, the most
/// valuable first; a picture follows every choice of a log.
///
/// 1. Confirm (A): the spectrum window for the logs of [`CONFIRM_SPECTRA`]
///    as it comes up ([`spectrum_as_it_comes`]): two flights made for no
///    question, the log whose clock runs off its rate (C1, with the tones
///    of the peak labels, S5) and the log that pauses (C2). The maps of the
///    first flight in both windows of maps ([`maps_as_they_come`]), and its
///    D term and PID error in the time window ([`time_other_channels`],
///    C). The step response window for the logs of [`CONFIRM_STEPS`]
///    ([`confirm_steps`]: "Reset" before each; R1, R3, R6 and R15 on the
///    last). The main window for the flight of the step response at 1 kHz:
///    a picture (the length and the rate it prints, C6) and the four plots
///    copied.
/// 2. The stretch (B): [`stretch_ends`].
/// 3. What no round asked (C): [`setup_pair`], and last [`pointers`].
fn confirmation(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    spectrum_open(steps, round);
    for (row, id) in rows_of(probes, &CONFIRM_SPECTRA) {
        spectrum_as_it_comes(steps, round, id, row);
    }
    if let Some((row, id)) = rows_of(probes, &[CONFIRM_MAPS]).pop() {
        maps_as_they_come(steps, round, id, row);
        time_other_channels(steps, round, id);
    }
    confirm_steps(steps, round, probes);
    if let Some(log) = ViewerLog::of(round, probes, CONFIRM_VIEWER) {
        log.select(steps);
        log.copies(steps, "open", &[0, 1, 2, 3]);
    }
    stretch_ends(steps, round, probes);
    setup_pair(steps, round, probes);
    pointers(steps, round);
}

/// How long a window that was never opened by the script is given to come
/// up before its picture is taken, milliseconds.
const LOOK_MS: u32 = 10_000;

/// A first look at the windows the main window opens and no round has
/// asked: for each a click on its button, a wait, the list of the tool's
/// windows into the log (their titles are not known yet) and a picture of
/// the place of the main window on the screen, with whatever lies there
/// (the main window is not brought to the front for it).
fn windows_look(steps: &mut Vec<Step>, round: u32) {
    for (name, file, at) in log_at::OTHERS {
        steps.push(click(LOG_VIEWER, "log", name, at));
        steps.push(pause(LOOK_MS));
        steps.push(Step {
            action: "windows",
            ..Step::default()
        });
        steps.push(Step {
            front: Some(false),
            ..shot(LOG_VIEWER, format!("r{round}-look-{file}"))
        });
    }
}

/// What the plan of a round does, for the reader of `steps.json`.
fn about(round: u32, probes: &[Probe]) -> String {
    if round == CONFIRMATION {
        return format!(
            "Round {round}: every log through one open dialog, then, the most valuable first: \
             the spectrum window for two flights made for no question, a log whose clock runs \
             2 % off its header's rate (with two tones on every axis for the peak labels) and \
             one that pauses for 5 % of its frames, six plots copied each; the maps of the \
             first flight in the window of frequency against throttle and in that against \
             time, there also of its D term and its PID error; the step response window for \
             two flights (\"Reset\" before each) and for a log of small and large moves, \
             also from windows of 4 and 5 s; the main window for the flight of 30.000 s at 1 \
             kHz, its four plots copied; the stretch: six short flights of stairs (10.1 to \
             10.9 s) in both windows of maps, the three logs of earlier rounds that were read \
             from sample fs + 3, and the log that pauses in the time window; the setup list \
             for two logs that differ in one header; last a picture of each window of maps \
             and of the spectrum window with the pointer resting on a plot and after a click \
             there."
        );
    }
    if round == VERIFICATION {
        return format!(
            "Round {round}: every log through one open dialog, then every window again on \
             logs made for no question, the most valuable first. The spectrum window: two \
             flights as it comes up, six plots copied each; then the log of the other \
             channels with every entry of the list of channels picked in turn, a picture \
             after each pick and the two plots of roll copied. The maps of one flight in the \
             window of frequency against throttle and in that against time. The step response \
             window: two flights, the response and the Peak copied. The main window: one \
             flight, its four plots copied. The filter simulator with a set of filters typed \
             in, its eight plots copied. The step response of a log whose RC command and \
             feedforward are not the setpoint's. The log of round 16 that counts its samples \
             at 2 and 4 kHz, its roll plot with the line smoothing off and at med. Three short \
             flights of stairs in both windows of maps."
        );
    }
    if round == OPEN_ENDS {
        return format!(
            "Round {round}: every log through one open dialog, then what the rounds before \
             left open in each window. The main window: its list of logs held open; three \
             logs chosen in it, a picture after each choice; for the first the levels of the \
             line smoothing and the button \">\"; for the second the D-term lines at two y \
             scales and the motor speeds; for the third the motor lines; then \"Markup\" and \
             \"Period\" with two clicks into the roll plot each. The window of frequency \
             against throttle for eight flights of other lengths and for one with the stick \
             and the mixer at two throttles: a picture and one copied map each. The window of \
             frequency against time for the two shortest flights. The filter simulator with \
             low-pass 2 as each of its other types. Last a summary of the tables the tool \
             has decoded the logs into (tables.txt)."
        );
    }
    let time = probes.iter().any(|p| p.area == Area::Time);
    if probes.iter().all(|p| p.area == Area::LogViewer) {
        return format!(
            "Round {round}: the log through the open dialog, then the main window itself, \
             which draws the channels of the log as lines. Its four plots copied as it comes \
             up; every line of its check-list switched on, a picture and the four plots \
             copied again; its y scale typed as 1000 and put back; its list of the line \
             smoothing held open; a click into the roll plot at 6 s and one on the button \
             \">\", a picture each; last the buttons \"Markup\" and \"Period\", whose effect \
             is not known, a picture each."
        );
    }
    if probes.iter().all(|p| p.area == Area::Windows) && round >= WINDOWS_MORE {
        return format!(
            "Round {round}: the log through the open dialog, then two windows of the main \
             window. The slider tool: start gains, one slider moved, and every entry of its \
             list in turn, a picture each. The filter simulator: low-pass 1 as each of its \
             types and at each loop rate with its plots copied, the motors, the harmonics \
             and the notch switched, the notch set by its cut-off, the impulse response and \
             the chirp, and a set of filters made for no question with all its plots copied."
        );
    }
    if probes.iter().all(|p| p.area == Area::Windows) && round >= WINDOWS_ASK {
        return format!(
            "Round {round}: the logs through the open dialog, then three windows of the main \
             window. The slider tool: start gains typed in, the sliders moved by their arrows, \
             a picture after each. The filter simulator: its plots copied as it comes up, a \
             picture of each of its lists held open, its three switches, and four values \
             typed in with the plots copied again. The setup list: as it comes up, with the \
             differences only, and its list of logs."
        );
    }
    if probes.iter().all(|p| p.area == Area::Windows) {
        return format!(
            "Round {round}: the log through the open dialog, then a first look at three windows \
             of the main window that were never opened by the script (the sliders, the \
             filters, the setup): a click on the button, ten seconds, a picture of the screen \
             where the main window is. Nothing is clicked inside those windows."
        );
    }
    if probes.iter().all(|p| p.area == Area::StepResponse) {
        step_about(round, probes)
    } else if time && round >= TIME_LEVELS {
        format!(
            "Round {round}: every log through one open dialog, then the window of frequency \
             against time: for every log the maps of the unfiltered gyro as the window \
             comes up; for the first also at eight other settings of its two smoothing \
             lists and with the colour scale ending at 0 dB; for the last, a flight, also \
             the maps of the filtered gyro and at two settings of the lists; a picture of \
             the window and a vector copy of every plot each time."
        )
    } else if time {
        format!(
            "Round {round}: every log through one open dialog; the window of frequency \
             against throttle for the logs of other lengths (the maps as it comes up); \
             then a first look at the window of frequency against time: for every log of \
             its own the maps of the unfiltered and of the filtered gyro, for the first \
             also its two smoothing lists, the view below 100 Hz and a colour scale down \
             to -70 dB; a picture of the window and a vector copy of every plot each time."
        )
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
    }
}

/// The spectrum window opened from the main window, a picture as it comes
/// up, and from round 3 on [`Y_MAX_DB`] typed into its "Y max".
fn spectrum_open(steps: &mut Vec<Step>, round: u32) {
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
        spectrum_type(steps, "Y max", spectrum_at::Y_MAX, &Y_MAX_DB.to_string());
    }
}

/// `text` typed into the box `name` of the spectrum window.
fn spectrum_type(steps: &mut Vec<Step>, name: &str, at: (u32, u32), text: &str) {
    steps.push(Step {
        action: "type",
        text: Some(text.to_string()),
        ..click(SPECTRUM, "shot", name, at)
    });
    steps.push(pause(300));
}

/// The spectrum window: opened, and for every spectrum probe of the round
/// the spectrum at each smoothing level, as a picture and, from round 2 on,
/// as vector copies of its plots.
fn spectrum_steps(steps: &mut Vec<Step>, round: u32, probes: &[Probe]) {
    spectrum_open(steps, round);
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
                copy_plot(steps, axis, column, file);
            }
        }
    }
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
    if [OPEN_ENDS, VERIFICATION, CONFIRMATION].contains(&round) {
        match round {
            OPEN_ENDS => open_ends(&mut steps, round, probes),
            VERIFICATION => verification(&mut steps, round, probes),
            _ => confirmation(&mut steps, round, probes),
        }
        steps.push(Step {
            action: "windows",
            ..Step::default()
        });
        return Plan {
            about: about(round, probes),
            give_up: true,
            refs: frames(false),
            steps,
        };
    }

    // the spectrum window, when the round has probes for it
    if probes.iter().any(|p| p.area == Area::Spectrum) {
        spectrum_steps(&mut steps, round, probes);
    }
    if probes.iter().any(|p| p.area == Area::StepResponse) {
        step_response(&mut steps, round, probes);
    }
    let throttle = probes.iter().any(|p| p.area == Area::Throttle);
    let time = probes.iter().any(|p| p.area == Area::Time);
    if throttle || time {
        open_spectrum(&mut steps, round);
    }
    if throttle {
        throttle_maps(&mut steps, round, probes);
    }
    let windows = probes.iter().any(|p| p.area == Area::Windows);
    let asked = windows && round >= WINDOWS_ASK;
    if windows && round >= WINDOWS_MORE {
        sliders_more(&mut steps, round);
        filters_more(&mut steps, round);
    } else if asked {
        sliders_ask(&mut steps, round);
        filters_ask(&mut steps, round);
        setup_ask(&mut steps, round);
    } else if windows {
        windows_look(&mut steps, round);
    }
    if time && round >= TIME_LEVELS {
        time_levels(&mut steps, round, probes);
    } else if time {
        time_maps(&mut steps, round, probes);
    }
    let viewer = viewer_lines(&mut steps, round, probes);
    steps.push(Step {
        action: "windows",
        ..Step::default()
    });
    Plan {
        about: about(round, probes),
        // the plots of the window of frequency against throttle had never
        // been copied when its first round was planned; nor had those of
        // the main window
        give_up: throttle || time || asked || viewer,
        refs: frames(asked),
        steps,
    }
}

/// The reference screenshots of a plan; with `slider` also the inside of
/// the slider tool's window.
fn frames(slider: bool) -> Frames {
    Frames {
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
        slider: slider.then_some(Frame {
            left: 0,
            top: 0,
            width: sliders_at::SIZE.0,
            height: sliders_at::SIZE.1,
        }),
    }
}

/// The window of the reference tool a plot was copied in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Window {
    /// The main window, which draws the channels of a log as lines.
    LogViewer,
    /// The spectrum window.
    Spectrum,
    /// The step response window.
    StepResponse,
    /// The map of frequency against throttle.
    Throttle,
    /// The map of frequency against time.
    Time,
    /// The filter simulator.
    Filters,
    /// Any other window.
    Other,
}

impl Window {
    /// The window whose title holds `title`.
    fn of(title: &str) -> Window {
        [
            (LOG_VIEWER, Window::LogViewer),
            (SPECTRUM, Window::Spectrum),
            (STEP, Window::StepResponse),
            (THROTTLE, Window::Throttle),
            (TIME, Window::Time),
            (FILTERS, Window::Filters),
        ]
        .into_iter()
        .find(|(t, _)| *t == title)
        .map_or(Window::Other, |(_, w)| w)
    }
}

impl Plan {
    /// Every plot the plan copies as a vector graphic: the name of the
    /// copy (without `.emf`) and the window it is copied in, which is the
    /// window of the step before the copy is saved.
    pub fn copies(&self) -> Vec<(String, Window)> {
        let mut window = Window::Other;
        let mut out = Vec::new();
        for step in &self.steps {
            if let Some(title) = step.window {
                window = Window::of(title);
            }
            if let (true, Some(file)) = (step.action == "clip_save", &step.file) {
                out.push((file.clone(), window));
            }
        }
        out
    }

    /// How many steps of the plan do `action`.
    pub fn count(&self, action: &str) -> usize {
        self.steps.iter().filter(|s| s.action == action).count()
    }

    /// About how long the script takes over the plan, seconds, when every
    /// copy comes at the first try (the second try is then passed over):
    /// the pauses and rests as written, half a second for a click, the
    /// quiet time and five seconds for a wait for the tool to redraw, five
    /// seconds a log for the tool to decode it, half a minute for the
    /// summary of the tables. With these a copy takes six seconds, as round
    /// 4 measured it. How long the tool computes is not known beforehand: a
    /// run takes longer when it is slow.
    pub fn seconds(&self) -> f64 {
        let mut total = 0.0;
        let mut skip = 0;
        for step in &self.steps {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let ms = f64::from(step.ms.unwrap_or(0)) / 1000.0;
            total += match step.action {
                "pause" => ms,
                "hover" => 0.4 + ms,
                "open_file" => 4.0 + 5.0 * step.paths.as_ref().map_or(1, Vec::len) as f64,
                "settle" => {
                    let change = if step.change_s.is_some() { 5.0 } else { 0.0 };
                    let window = if step.wait_s.is_some() { 3.0 } else { 0.0 };
                    0.5 + step.quiet_s.unwrap_or(1.2) + change + window
                }
                "clip_wait" => {
                    skip = step.skip.unwrap_or(0);
                    0.7
                }
                "clip_save" => 1.2,
                "type" => 1.6,
                "shot" => 0.7,
                "tables" => 30.0,
                _ => 0.5,
            };
        }
        total
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
    // a picture with its Run takes about five seconds, a copy six
    // (round 4, on version 0.9 of the tool: 20 pictures and 153 copies
    // in 17.5 minutes)
    let minutes = (shots * 5 + copies * 6).div_ceil(60) + 1;
    written(round, (shots, copies, minutes), predicted, false)
}

/// The files that make the pack of `plan` runnable: as [`files`], with the
/// pictures and the copies counted in the plan. From round [`OPEN_ENDS`] on
/// the note gives the time the steps themselves add up to
/// ([`Plan::seconds`]): a plan with long pauses and few copies takes longer
/// than its pictures and copies say.
pub fn files_of(round: u32, plan: &Plan, predicted: bool) -> Vec<(String, Vec<u8>)> {
    let (shots, copies) = (plan.count("shot"), plan.count("clip_save"));
    if round < OPEN_ENDS {
        return files(round, shots, copies, predicted);
    }
    let minutes = (plan.seconds() / 60.0).ceil() as usize;
    let tables = plan.count("tables") > 0;
    written(round, (shots, copies, minutes), predicted, tables)
}

/// The script, its launcher and the note for a run that saves `shots`
/// pictures and `copies` copied plots in about `minutes` minutes; with
/// `tables` the note says that the script also writes down a summary of the
/// tables the tool leaves behind.
fn written(
    round: u32,
    (shots, copies, minutes): (usize, usize, usize),
    predicted: bool,
    tables: bool,
) -> Vec<(String, Vec<u8>)> {
    let launcher = "@echo off\n\
                    cd /d \"%~dp0\"\n\
                    powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0capture.ps1\" \
                    -Steps \"steps.json\"\n\
                    echo.\n\
                    pause\n";
    let saves = if copies == 0 {
        format!("saves {shots} pictures of the\n   tool's windows")
    } else {
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
    let summary = if tables {
        "The script also writes down a summary of the tables (*.csv) the tool leaves next\n\
         to the logs: tables.txt, inside capture-results.zip. It copies, moves and\n\
         changes none of them.\n\n"
    } else {
        ""
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
         Leave both alone until it beeps twice. Now and then it stands still for up to\n   \
         half a minute: it is waiting for the tool, and a move of the mouse stops it.\n\
         4. Send back capture-results.zip, which appears next to the script.\n\
         \n\
         To stop it, move the mouse: it stops before its next move.\n\
         \n\
         {clipboard}\
         {summary}\
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
    fn round_9_copies_the_maps_without_the_lines() {
        let probes = catalogue::round(9).unwrap_or_default();
        let plan = plan(9, &probes);
        assert!(plan.give_up);
        assert!(
            plan.about
                .contains("for one log at the other smoothing levels")
        );
        assert!(plan.about.contains("for one below 100 Hz") && !plan.about.contains("2D"));
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // six plots at a setting: two settings a log, and for the flight
        // four smoothing levels and the view below 100 Hz
        let copies = names("clip_save");
        assert_eq!(copies.len(), 6 * (9 * 2 + 4 + 1));
        assert!(
            !copies
                .iter()
                .any(|c| c.contains("-2d-") || c.contains("scale"))
        );
        for (k, name) in [
            (0, "a1-len7-2k-throttle-roll-prefilt"),
            (6, "a1-len7-2k-throttle-psd-roll-prefilt"),
            (12, "a2-len16-2k-throttle-roll-prefilt"),
            (96, "a9-flight-2k-throttle-roll-prefilt"),
            (108, "a9-flight-2k-throttle-smoothing-low-roll-prefilt"),
            (132, "a9-flight-2k-throttle-sub100-roll-prefilt"),
            (137, "a9-flight-2k-throttle-sub100-yaw-gyro"),
        ] {
            assert_eq!(copies[k], format!("r9-{name}"));
        }
        let shots = names("shot");
        assert_eq!(shots.len(), 3 + copies.len() / 6);
        assert!(plan.steps.iter().all(|s| s.action != "type"));
        assert!(
            !plan
                .steps
                .iter()
                .any(|s| s.name.as_deref() == Some("2D on"))
        );
        // the nine logs of the list, in both columns
        let rows: Vec<u32> = plan
            .steps
            .iter()
            .filter(|s| {
                s.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("log a") && n.ends_with("in column 1"))
            })
            .filter_map(|s| s.at.map(|a| a.1))
            .collect();
        assert_eq!(rows, [152, 199, 247, 294, 342, 389, 437, 484, 532]);
        let note = files(9, shots.len(), copies.len(), false).remove(2).1;
        let note = String::from_utf8(note).unwrap_or_default();
        assert!(
            note.contains("138 plots") && note.contains("saves 26 pictures"),
            "{note}"
        );
    }

    /// The plan of round 15.
    fn round_15() -> Plan {
        plan(15, &catalogue::round(15).unwrap_or_default())
    }

    #[test]
    fn round_15_copies_the_plots_of_the_main_window_with_every_line_on() {
        let plan = round_15();
        assert!(plan.give_up && plan.refs.slider.is_none());
        assert!(plan.about.contains("the main window itself"));
        assert_eq!(
            names_of(&plan, "shot"),
            [
                "r15-log-loaded",
                "r15-log-all",
                "r15-log-yscale-1000",
                "r15-log-list-smoothing",
                "r15-log-cursor-6s",
                "r15-log-cursor-next",
                "r15-log-markup",
                "r15-log-period"
            ]
        );
        // the four plots as the window comes up, and with every line on
        assert_eq!(
            names_of(&plan, "clip_save"),
            [
                "r15-log-open-roll",
                "r15-log-open-pitch",
                "r15-log-open-yaw",
                "r15-log-open-throttle",
                "r15-log-all-roll",
                "r15-log-all-pitch",
                "r15-log-all-yaw",
                "r15-log-all-throttle"
            ]
        );
        let place = |file: &str| {
            let wanted = |s: &Step| s.file.as_deref() == Some(file);
            plan.steps.iter().position(wanted).unwrap_or(usize::MAX)
        };
        let (loaded, all) = (place("r15-log-loaded"), place("r15-log-all"));
        assert!(loaded < place("r15-log-open-roll") && place("r15-log-open-throttle") < all);
        assert!(all < place("r15-log-all-roll"));
        assert!(place("r15-log-all-throttle") < place("r15-log-yscale-1000"));
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        // each copy is of its own plot: the button of its toolbar is above
        // the top right corner of the axes, one plot below the other
        let buttons: Vec<(u32, u32)> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.ends_with(" toolbar")))
            .filter(|s| s.front.is_none())
            .filter_map(|s| s.at)
            .collect();
        let rows = [133 - 15, 668 - 15, 1203 - 15, 1833 - 15];
        let twice: Vec<(u32, u32)> = rows.iter().chain(&rows).map(|y| (3307 - 14, *y)).collect();
        assert_eq!(buttons, twice);
        // one log, and nothing but the main window
        let windows: Vec<&str> = plan.steps.iter().filter_map(|s| s.window).collect();
        assert!(windows.iter().all(|w| *w == LOG_VIEWER));
        assert_eq!(plan.steps[1].paths, Some(vec!["q1-values-2k.bbl".into()]));
        // rounds 12 to 14 keep their plans
        for (round, said) in [
            (12, "a first look at three windows"),
            (13, "The slider tool: start gains typed in"),
            (14, "a set of filters made for no question"),
        ] {
            let earlier = super::plan(round, &catalogue::round(round).unwrap_or_default());
            assert!(earlier.about.contains(said), "round {round}");
            assert!(!earlier.about.contains("the main window itself"));
            let files = names_of(&earlier, "shot");
            assert!(files.iter().all(|f| !f.contains("-log-all")), "{round}");
        }
    }

    #[test]
    fn round_15_clicks_the_thirteen_lines_that_are_off() {
        let plan = round_15();
        let boxes: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.action == "click" && s.frame == Some("log"))
            .skip(1)
            .collect();
        let clicked: Vec<(&str, u32)> = boxes
            .iter()
            .map(|s| (s.name.as_deref().unwrap_or(""), s.at.unwrap_or_default().1))
            .collect();
        assert_eq!(
            clicked,
            [
                ("Gyro(pf)", 384),
                ("P-term", 506),
                ("I-term", 566),
                ("D-term(pf)", 627),
                ("D-term", 688),
                ("F-term", 748),
                ("PID sum", 870),
                ("PID error", 930),
                ("RPS 1/5", 1234),
                ("RPS 2/6", 1294),
                ("RPS 3/7", 1355),
                ("RPS 4/8", 1416),
                ("Debug", 1537)
            ]
        );
        assert!(boxes.iter().all(|s| s.at.map(|a| a.0) == Some(67)));
        // none of the seven that are on as the window comes up
        for on in ["Gyro", "Set point", "Throttle"] {
            assert!(!clicked.iter().any(|(name, _)| *name == on), "{on}");
        }
        assert!(!clicked.iter().any(|(name, _)| name.starts_with("Motor")));
        assert_eq!(viewer_at::LINES.iter().filter(|l| l.2).count(), 7);
        // the boxes of the list are about 61 pixels apart, top to bottom
        let rows: Vec<u32> = viewer_at::LINES.iter().map(|l| l.1).collect();
        assert!(rows.windows(2).all(|w| (60..=61).contains(&(w[1] - w[0]))));
        // a pause after each, and the picture only once the plots are quiet
        let first = plan.steps.iter().position(|s| s == boxes[0]).unwrap_or(0);
        let after = &plan.steps[first..first + 28];
        for pair in after[..26].chunks(2) {
            assert_eq!((pair[0].action, pair[1].ms), ("click", Some(400)));
        }
        assert_eq!(
            (after[26].action, after[26].change_s, after[26].quiet_s),
            ("settle", None, Some(1.5))
        );
        assert_eq!(after[27].file.as_deref(), Some("r15-log-all"));
    }

    #[test]
    fn round_15_puts_back_what_it_sets_in_the_main_window() {
        let plan = round_15();
        // the y scale is typed and put back, with a wait for the plots
        let typed: Vec<&Step> = plan.steps.iter().filter(|s| s.action == "type").collect();
        let texts: Vec<&str> = typed.iter().filter_map(|s| s.text.as_deref()).collect();
        assert_eq!(texts, ["1000", "500"]);
        assert!(typed.iter().all(|s| s.at == Some((132, 241))));
        assert!(typed.iter().all(|s| s.frame == Some("shot")));
        assert_eq!((typed[0].mark, typed[1].mark), (Some(true), None));
        let mut marked = false;
        for step in &plan.steps {
            marked |= step.mark == Some(true);
            if step.action == "settle" && step.change_s.is_some() {
                assert!(marked, "a wait for a change without a mark");
                marked = false;
            }
        }
        // the list that is opened is closed with Esc, after its picture
        let at = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted).unwrap_or(usize::MAX - 8)
        };
        let list = at("line smoothing (the list)");
        assert_eq!(plan.steps[list].at, Some((3468, 628)));
        assert_eq!(plan.steps[list + 1].ms, Some(800));
        let picture = plan.steps[list + 2].file.as_deref();
        assert_eq!(picture, Some("r15-log-list-smoothing"));
        assert_eq!(plan.steps[list + 3].key, Some("esc"));
        // the click into the roll plot is at 6 of its 20 seconds, half way up
        let cursor = at("roll plot");
        assert_eq!(plan.steps[cursor].at, Some((1349, 394)));
        assert_eq!(plan.steps[cursor].at, Some((510 + 2797 * 6 / 20, 394)));
        assert_eq!(plan.steps[at(">")].at, Some((3642, 1607)));
        // the buttons whose effect is not known come last: "Markup" twice,
        // then "Period" with the titles of the windows and Esc
        let (markup, again, period) = (at("Markup"), at("Markup (again)"), at("Period"));
        assert!(at(">") < markup && markup < again && again < period);
        assert_eq!(plan.steps[markup].at, Some((3650, 457)));
        assert_eq!(plan.steps[again].at, plan.steps[markup].at);
        assert_eq!(plan.steps[period].at, Some((3468, 457)));
        assert_eq!(plan.steps[markup + 1].ms, Some(1000));
        assert_eq!(plan.steps[period + 1].ms, Some(2000));
        let rest: Vec<&str> = plan.steps[period + 2..].iter().map(|s| s.action).collect();
        assert_eq!(rest, ["windows", "shot", "key", "pause", "windows"]);
        assert_eq!(plan.steps[period + 4].key, Some("esc"));
        let closed = plan.steps.iter().filter(|s| s.key == Some("esc")).count();
        assert_eq!(closed, 2);
        // their pictures are of the screen as it is
        for file in ["r15-log-markup", "r15-log-period"] {
            let wanted = |s: &&Step| s.file.as_deref() == Some(file);
            let picture = plan.steps.iter().find(wanted);
            assert_eq!(picture.and_then(|s| s.front), Some(false), "{file}");
        }
    }

    /// The plan of round 16.
    fn round_16() -> Plan {
        plan(16, &catalogue::round(16).unwrap_or_default())
    }

    /// The first step of `plan` called `name`.
    fn step_named<'a>(plan: &'a Plan, name: &str) -> &'a Step {
        let wanted = |s: &&Step| s.name.as_deref() == Some(name);
        let found = plan.steps.iter().find(wanted);
        found.unwrap_or_else(|| panic!("no step {name}"))
    }

    #[test]
    fn round_16_takes_its_pictures_and_copies_in_the_order_of_their_worth() {
        let plan = round_16();
        assert!(plan.give_up && plan.refs.slider.is_none());
        assert!(plan.about.contains("what the rounds before left open"));
        let shots = names_of(&plan, "shot");
        let log = |id: &str, what: &[&str]| -> Vec<String> {
            what.iter()
                .map(|w| viewer_file(16, id, w, ""))
                .collect::<Vec<_>>()
        };
        let stairs = [
            "w1-len21-2k",
            "w2-len22-2k",
            "w3-len22h-2k",
            "w4-len61-2k",
            "w5-len62-2k",
            "w6-len63-2k",
            "w7-len10-2k",
            "w8-len14-2k",
        ];
        let mut want: Vec<String> = vec!["r16-log-loaded".into(), "r16-log-list-files".into()];
        want.extend(log(
            "v3-samp-1k",
            &[
                "",
                "smooth-off",
                "smooth-low",
                "smooth-med",
                "smooth-med-high",
                "smooth-high",
                "next-1",
                "next-2",
                "next-3",
            ],
        ));
        want.extend(log(
            "v1-deriv-2k",
            &["", "dterm", "dterm-yscale-2000", "rps"],
        ));
        want.extend(log("v2-out-2k", &["", "next-1"]));
        want.extend(["r16-log-markup", "r16-log-period", "r16-spectrum-run"].map(String::from));
        want.push("r16-throttle-open".into());
        want.extend(stairs.map(|id| throttle_file(16, id, "", "")));
        want.push("r16-v1-deriv-2k-throttle".into());
        want.push("r16-time-open".into());
        want.extend(
            stairs[6..]
                .iter()
                .map(|id| time_file(16, id, "prefilt", "")),
        );
        want.push("r16-filters-open".into());
        want.extend(
            ["pt2", "pt3", "biquad-butter", "biquad-bessel"]
                .map(|kind| format!("r16-filters-lowpass-2-{kind}")),
        );
        assert_eq!(shots, want);
        assert_eq!(shots.len(), 38);
        assert_eq!(shots[2], "r16-v3-samp-1k-log");

        let copies = names_of(&plan, "clip_save");
        let mut want: Vec<String> = ["roll", "pitch", "yaw"]
            .map(|plot| viewer_file(16, "v3-samp-1k", "open", plot))
            .to_vec();
        for level in ["off", "low", "med", "med-high", "high"] {
            let what = format!("smooth-{level}");
            want.extend(["roll", "yaw"].map(|plot| viewer_file(16, "v3-samp-1k", &what, plot)));
        }
        let derivative = |what: &str, plot: &str| viewer_file(16, "v1-deriv-2k", what, plot);
        want.extend([
            derivative("open", "throttle"),
            derivative("dterm", "roll"),
            derivative("dterm", "pitch"),
            derivative("dterm-yscale-2000", "roll"),
            derivative("dterm-yscale-2000", "pitch"),
            derivative("rps", "throttle"),
            viewer_file(16, "v2-out-2k", "open", "throttle"),
        ]);
        want.extend(stairs.map(|id| throttle_file(16, id, "", "roll-prefilt")));
        want.push("r16-v1-deriv-2k-throttle-yaw-prefilt".into());
        for id in &stairs[6..] {
            want.extend(AXES.map(|axis| time_file(16, id, "prefilt", axis)));
        }
        assert_eq!(copies, want);
        assert_eq!(copies.len(), 35);
        assert_eq!(copies[13], "r16-v1-deriv-2k-log-open-throttle");
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        // every copy follows a picture of its window, so that a copy that
        // fails gives up only the copies up to the next picture
        let mut pictured = None;
        for step in &plan.steps {
            match step.action {
                "shot" => pictured = step.window,
                "click" if step.copy == Some(true) => assert_eq!(step.window, pictured),
                _ => {}
            }
        }
        // the summary of the tables once, after the last picture, and only
        // the list of the windows after it
        let rest: Vec<&str> = plan.steps.iter().rev().take(2).map(|s| s.action).collect();
        assert_eq!(rest, ["windows", "tables"]);
        assert_eq!(plan.count("tables"), 1);
        assert_eq!(plan.steps.len(), 874);
        // every action of the plan is one the script knows
        let json = serde_json::to_value(&plan).unwrap_or_default();
        for step in json["steps"].as_array().map_or(&[][..], Vec::as_slice) {
            let action = step["action"].as_str().unwrap_or("");
            assert!(
                SCRIPT.contains(&format!("$action -eq \"{action}\"")),
                "{action}"
            );
        }
        // the round before keeps its plan
        let earlier = round_15();
        assert_eq!((earlier.steps.len(), earlier.count("tables")), (173, 0));
    }

    #[test]
    fn round_16_takes_a_log_by_its_row_and_looks_after_every_choice() {
        let plan = round_16();
        let probes = catalogue::round(16).unwrap_or_default();
        // the fourteen logs through one dialog, in the order of their names
        let paths = plan.steps[1].paths.clone().unwrap_or_default();
        assert_eq!(paths.len(), 14);
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
        // the list of the logs is held open once, with a picture, and closed
        let list = plan
            .steps
            .iter()
            .position(|s| s.name.as_deref() == Some("logs (the list)"));
        let list = list.unwrap_or(usize::MAX - 8);
        assert_eq!(plan.steps[list].at, Some((3559, 262)));
        assert_eq!(plan.steps[list].frame, Some("shot"));
        let picture = plan.steps[list + 2].file.as_deref();
        assert_eq!(picture, Some("r16-log-list-files"));
        assert_eq!(plan.steps[list + 3].key, Some("esc"));
        // a log of the main window: the list, its row (the sixth, the
        // fourth, the fifth log), three seconds, a picture named after it
        for (id, row, y) in [
            ("v3-samp-1k", 5, 542),
            ("v1-deriv-2k", 3, 447),
            ("v2-out-2k", 4, 495),
        ] {
            assert_eq!(probes[row].id, id);
            let name = format!("log {id}");
            let wanted = |s: &Step| s.name.as_deref() == Some(name.as_str());
            let at = plan.steps.iter().position(wanted).unwrap_or(usize::MAX - 8);
            let open = &plan.steps[at - 2];
            assert_eq!(open.name, Some(format!("{name} (the list)")));
            assert_eq!(
                (open.at, open.window),
                (Some((3559, 262)), Some(LOG_VIEWER))
            );
            assert_eq!(plan.steps[at].at, Some((3559, y)));
            assert_eq!(y, 262 + 43 + 475 * row as u32 / 10);
            assert_eq!(plan.steps[at + 1].ms, Some(3000));
            assert_eq!(plan.steps[at + 2].file, Some(format!("r16-{id}-log")));
            assert_eq!(plan.steps[at + 2].action, "shot");
        }
        let chosen = |id: &str| {
            let name = format!("log {id}");
            plan.steps
                .iter()
                .position(|s| s.name.as_deref() == Some(name.as_str()))
        };
        assert!(chosen("v3-samp-1k") < chosen("v1-deriv-2k"));
        assert!(chosen("v1-deriv-2k") < chosen("v2-out-2k"));
        // the levels of the line smoothing where round 15 saw them, each
        // with its picture, and "off" at the end
        let picked: Vec<(&str, (u32, u32))> = plan
            .steps
            .iter()
            .filter(|s| {
                s.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("line smoothing "))
            })
            .filter(|s| s.name.as_deref() != Some("line smoothing (the list)"))
            .map(|s| (s.name.as_deref().unwrap_or(""), s.at.unwrap_or_default()))
            .collect();
        assert_eq!(
            picked,
            [
                ("line smoothing off", (3468, 672)),
                ("line smoothing low", (3468, 721)),
                ("line smoothing med", (3468, 769)),
                ("line smoothing med-high", (3468, 817)),
                ("line smoothing high", (3468, 864)),
                ("line smoothing off", (3468, 672)),
            ]
        );
        let lists = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref() == Some("line smoothing (the list)"));
        assert!(lists.clone().count() == 6 && lists.clone().all(|s| s.at == Some((3468, 628))));
        // ">" three times for the first log and once for the third
        let next: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref() == Some(">"))
            .collect();
        assert!(next.len() == 4 && next.iter().all(|s| s.at == Some((3642, 1607))));
        // nothing waits for the main window to stop changing once the logs
        // are open: a typed number leaves its text cursor blinking
        let waits = plan
            .steps
            .iter()
            .skip(3)
            .filter(|s| s.action == "settle" && s.window == Some(LOG_VIEWER));
        assert_eq!(waits.count(), 0);
        let typed: Vec<usize> = (0..plan.steps.len())
            .filter(|i| plan.steps[*i].action == "type")
            .collect();
        let texts: Vec<&str> = typed
            .iter()
            .filter_map(|i| plan.steps[*i].text.as_deref())
            .collect();
        assert_eq!(texts, ["2000", "500"]);
        for i in typed {
            let step = &plan.steps[i];
            assert_eq!((step.at, step.mark), (Some((132, 241)), None));
            assert_eq!(
                (plan.steps[i + 1].action, plan.steps[i + 1].ms),
                ("pause", Some(3000))
            );
        }
        // the two buttons whose effect is not known: two clicks into the
        // roll plot after each, at a third and at two thirds of its width
        let (markup, period) = (step_named(&plan, "Markup"), step_named(&plan, "Period"));
        assert_eq!(
            (markup.at, period.at),
            (Some((3650, 457)), Some((3468, 457)))
        );
        for button in ["Markup", "Period"] {
            let first = step_named(&plan, &format!("roll plot at 1 of 3 ({button})"));
            let second = step_named(&plan, &format!("roll plot at 2 of 3 ({button})"));
            assert_eq!(
                (first.at, second.at),
                (Some((1442, 394)), Some((2375, 394)))
            );
            assert_eq!(
                first.at,
                Some((510 + (3307 - 510) / 3, u32::midpoint(133, 655)))
            );
        }
        assert_eq!(step_named(&plan, "Markup (again)").at, markup.at);
        for file in ["r16-log-markup", "r16-log-period"] {
            let wanted = |s: &&Step| s.file.as_deref() == Some(file);
            let picture = plan.steps.iter().find(wanted);
            assert_eq!(picture.and_then(|s| s.front), Some(false), "{file}");
        }
    }

    #[test]
    fn round_16_ticks_few_lines_for_a_copy_and_puts_the_check_list_back() {
        let plan = round_16();
        let rates = &viewer_at::LINES[..10];
        let lower = &viewer_at::LINES[10..19];
        let mut ticked: Vec<bool> = viewer_at::LINES.iter().map(|l| l.2).collect();
        let on = |ticked: &[bool], lines: &[(&str, u32, bool)]| {
            let listed = |name: &str| viewer_at::LINES.iter().position(|l| l.0 == name);
            let is_on = |l: &&(&str, u32, bool)| listed(l.0).is_some_and(|k| ticked[k]);
            lines.iter().filter(is_on).count()
        };
        let mut clicks = Vec::new();
        let mut most = (0, 0);
        for step in plan.steps.iter().filter(|s| s.window == Some(LOG_VIEWER)) {
            let name = step.name.as_deref().unwrap_or("");
            if step.frame == Some("log") && step.at.map(|a| a.0) == Some(67) {
                let (line, state) = name.rsplit_once(' ').unwrap_or((name, ""));
                let k = viewer_at::LINES.iter().position(|l| l.0 == line);
                let k = k.unwrap_or_else(|| panic!("no line {line}"));
                assert_eq!(step.at, Some((67, viewer_at::LINES[k].1)));
                ticked[k] = !ticked[k];
                // the name of the click says what the click leaves
                assert_eq!(state, if ticked[k] { "on" } else { "off" }, "{name}");
                clicks.push(name);
            }
            // the first click of a copy: on the button of the plot's toolbar
            if step.front.is_none() && name.ends_with(" toolbar") {
                if name == "throttle toolbar" {
                    most.1 = on(&ticked, lower).max(most.1);
                } else {
                    most.0 = on(&ticked, rates).max(most.0);
                    assert!(on(&ticked, rates) <= 4, "{name}");
                }
                assert!(on(&ticked, lower) <= 5, "{name}");
                assert!(!ticked[19], "the debug line is never ticked");
            }
        }
        // four lines on a rate plot at most, the five of the lower plot
        assert_eq!(most, (4, 5));
        assert_eq!(clicks.len(), 2 * (4 + 9));
        assert_eq!(
            clicks[..4],
            ["Gyro(pf) on", "D-term(pf) on", "D-term on", "Set point off"]
        );
        assert_eq!(clicks[4], "Motor 1/5 off");
        assert_eq!(clicks[12], "RPS 4/8 on");
        // the check-list ends as the window came up with it
        let defaults: Vec<bool> = viewer_at::LINES.iter().map(|l| l.2).collect();
        assert_eq!(ticked, defaults);
        // lines are given four seconds before their picture
        let last_tick = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted).unwrap_or(usize::MAX - 8)
        };
        for (name, file) in [
            ("Set point off", "r16-v1-deriv-2k-log-dterm"),
            ("RPS 4/8 on", "r16-v1-deriv-2k-log-rps"),
        ] {
            let at = last_tick(name);
            assert_eq!(plan.steps[at + 2].ms, Some(4000));
            assert_eq!(plan.steps[at + 3].file.as_deref(), Some(file));
        }
    }

    #[test]
    fn round_16_asks_the_other_windows_after_the_main_window() {
        let plan = round_16();
        let position = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted).unwrap_or(usize::MAX)
        };
        let order = [
            position("Period"),
            position("Spectral Analyzer"),
            position("Freq x Throttle"),
            position("Freq x Time"),
            position("Filter Sim"),
        ];
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        let last_main = plan
            .steps
            .iter()
            .rposition(|s| s.file.as_deref() == Some("r16-log-period"));
        assert!(last_main.unwrap_or(usize::MAX) < order[1]);
        // the throttle window as round 10 ran it: the log in all four
        // columns by its row, one Run, a picture, one copy
        let columns = |log: &str| -> Vec<(u32, u32)> {
            let chosen = |s: &&Step| {
                let name = s.name.as_deref().unwrap_or("");
                name.starts_with(log) && name.contains("in column") && !name.ends_with("(the list)")
            };
            plan.steps
                .iter()
                .filter(chosen)
                .filter_map(|s| s.at)
                .collect()
        };
        // the seventh log of the list and the fourteenth
        let row = |k: u32| 109 + 43 + 475 * k / 10;
        assert_eq!(
            columns("log w1-len21-2k"),
            [
                (482, row(6)),
                (1319, row(6)),
                (2156, row(6)),
                (2996, row(6))
            ]
        );
        assert_eq!(columns("log w8-len14-2k")[0], (482, row(13)));
        assert_eq!(columns("log v1-deriv-2k in")[3], (2996, row(3)));
        let runs = |window: &str| {
            let run = |s: &&Step| s.name.as_deref() == Some("Run") && s.window == Some(window);
            plan.steps.iter().filter(run).count()
        };
        assert_eq!((runs(THROTTLE), runs(TIME), runs(SPECTRUM)), (9, 2, 1));
        // the copied map is the first of the first column, and for the log
        // with a held throttle the third
        let toolbar = |window: &str, name: &str| -> Vec<(u32, u32)> {
            let first = |s: &&Step| {
                s.window == Some(window) && s.front.is_none() && s.name.as_deref() == Some(name)
            };
            plan.steps
                .iter()
                .filter(first)
                .filter_map(|s| s.at)
                .collect()
        };
        assert_eq!(
            toolbar(THROTTLE, "roll prefilt toolbar"),
            [(848 - 14, 318 - 15); 8]
        );
        assert_eq!(
            toolbar(THROTTLE, "yaw prefilt toolbar"),
            [(848 - 14, 1672 - 15)]
        );
        // the time window for the two shortest flights, by their rows, with
        // the channel it comes up with; the picture is marked before the
        // choice and waited for no longer than 25 s
        for (id, k) in [("w7-len10-2k", 12), ("w8-len14-2k", 13)] {
            let at = position(&format!("log {id}"));
            assert_eq!(plan.steps[at].window, Some(TIME));
            assert_eq!(plan.steps[at].at, Some((3551, 168 + 43 + 475 * k / 10)));
            assert_eq!(plan.steps[at - 2].mark, Some(true));
            let copies: Vec<&str> = names_of(&plan, "clip_save")
                .into_iter()
                .filter(|f| f.starts_with(&format!("r16-{id}-time-")))
                .collect();
            assert_eq!(copies.len(), 3, "{id}");
        }
        let waits: Vec<Option<f64>> = plan
            .steps
            .iter()
            .filter(|s| s.action == "settle" && s.window == Some(TIME))
            .map(|s| s.change_s)
            .collect();
        assert_eq!(waits, [None, Some(25.0), Some(25.0)]);
        assert_eq!(toolbar(TIME, "roll toolbar").len(), 2);
        // the filter simulator: low-pass 2 as its four other types, a
        // picture each, no copy, and "pt1" again
        let picks: Vec<(u32, u32)> = plan
            .steps
            .iter()
            .filter(|s| {
                s.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("lowpass-2 item"))
            })
            .filter_map(|s| s.at)
            .collect();
        let item = |k: u32| (3466, 311 + 43 + 475 * k / 10);
        assert_eq!(picks, [item(2), item(3), item(4), item(5), item(1)]);
        assert_eq!(
            step_named(&plan, "lowpass-2 (the list)").at,
            Some((3466, 311))
        );
        let filters = plan.steps.iter().filter(|s| s.window == Some(FILTERS));
        assert!(filters.clone().all(|s| s.copy.is_none()));
        assert_eq!(filters.filter(|s| s.action == "shot").count(), 5);
        // a wait for a change always has its mark
        let mut marked = false;
        for step in &plan.steps {
            marked |= step.mark == Some(true);
            if step.action == "settle" && step.change_s.is_some() {
                assert!(marked, "a wait for a change without a mark");
                marked = false;
            }
        }
    }

    #[test]
    fn the_names_of_every_round_fit_the_open_dialog() {
        // the script refuses names that make more than 250 characters: the
        // name field of the dialog holds about 259
        for round in catalogue::ROUNDS.iter().filter(|r| planned(**r)) {
            let plan = plan(*round, &catalogue::round(*round).unwrap_or_default());
            let paths = plan.steps[1].paths.clone().unwrap_or_default();
            let typed: Vec<String> = paths.iter().map(|p| format!("\"{p}\"")).collect();
            let length = typed.join(" ").len();
            assert!(length <= 250, "round {round}: {length} characters");
        }
        assert!(SCRIPT.contains("if ($path.Length -gt 250) {"));
    }

    #[test]
    fn the_note_of_round_16_says_how_long_and_names_the_tables() {
        let plan = round_16();
        // under 25 minutes by the steps themselves, and more than the
        // pictures and the copies alone would say
        let seconds = plan.seconds();
        assert!(seconds > 600.0 && seconds < 25.0 * 60.0, "{seconds}");
        let files = files_of(16, &plan, false);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["capture.ps1", "run.cmd", "README.txt"]);
        let note = String::from_utf8(files[2].1.clone()).unwrap_or_default();
        assert!(note.is_ascii() && note.contains("Calibration round 16"));
        assert!(note.contains("saves 38 pictures"), "{note}");
        assert!(
            note.contains("35 plots copied as vector graphics"),
            "{note}"
        );
        let minutes = (seconds / 60.0).ceil();
        assert!(note.contains(&format!("{minutes} minutes")), "{note}");
        assert!(note.contains("summary of the tables"), "{note}");
        assert!(note.contains("tables.txt") && note.contains("changes none of them"));
        // the notes of the rounds before are what they were
        let earlier = round_15();
        assert_eq!(files_of(15, &earlier, false), super::files(15, 8, 8, false));
        let note = String::from_utf8(files_of(15, &earlier, false)[2].1.clone());
        assert!(!note.unwrap_or_default().contains("tables"));
        // the time of a plan: a copy that comes at the first try passes
        // over its second try, and a pause takes what it says
        let one = |step: Step| Plan {
            steps: vec![step],
            ..earlier.clone()
        };
        assert_eq!(one(pause(2500)).seconds(), 2.5);
        let copy: Vec<Step> = earlier
            .steps
            .iter()
            .filter(|s| s.copy == Some(true))
            .take(14)
            .cloned()
            .collect();
        let copy = Plan {
            steps: copy,
            ..earlier.clone()
        };
        assert!((copy.seconds() - 5.9).abs() < 0.05, "{}", copy.seconds());
    }

    #[test]
    fn the_script_sums_up_the_tables_without_touching_them() {
        let from = SCRIPT.find("function Format-Plain").unwrap_or(0);
        let to = SCRIPT.find("function Invoke-Step").unwrap_or(0);
        assert!(from > 0 && to > from);
        let tables = &SCRIPT[from..to];
        assert!(tables.contains("function Add-TableSummary($text, $file)"));
        assert!(tables.contains("function Write-Tables"));
        // tables written since the script started, in the folder of the
        // logs with its subfolders and in the folder of the tool's program
        assert!(SCRIPT.contains("$script:started = Get-Date"));
        assert!(tables.contains("$file.LastWriteTime -lt $script:started"));
        assert!(tables.contains("-Filter \"*.csv\" -File -Recurse"));
        assert!(tables.contains("(Get-Process -Id ([int]$script:toolPid)).Path"));
        assert!(tables.contains("[System.IO.File]::ReadLines($file.FullName)"));
        assert!(tables.contains("(Join-Path $outDir \"tables.txt\")"));
        assert!(tables.contains("nothing found"));
        // whatever goes wrong is written down, and the run goes on
        assert!(tables.matches("} catch {").count() >= 5);
        assert!(!tables.contains("throw"));
        // nothing is copied, moved, changed or deleted
        for verb in [
            "Copy-Item",
            "Move-Item",
            "Remove-Item",
            "Rename-Item",
            "Set-Content",
            "Add-Content",
            "Out-File",
            "Delete",
            "WriteAllText",
        ] {
            assert!(!tables.contains(verb), "{verb}");
        }
        assert_eq!(tables.matches("WriteAllLines").count(), 1);
        // a checkout on Windows may give the script CRLF line endings
        let lf = SCRIPT.replace("\r\n", "\n");
        assert!(lf.contains("if ($action -eq \"tables\") {\n        Write-Tables"));
        assert!(SCRIPT.is_ascii());
    }

    #[test]
    fn every_position_of_every_round_lies_inside_its_frame() {
        for round in catalogue::ROUNDS.iter().filter(|r| planned(**r)) {
            let plan = plan(*round, &catalogue::round(*round).unwrap_or_default());
            let mut positions = 0;
            for step in &plan.steps {
                let Some((x, y)) = step.at else { continue };
                let frame = match step.frame {
                    Some("log") => Some(&plan.refs.log),
                    Some("shot") => Some(&plan.refs.shot),
                    Some("slider") => plan.refs.slider.as_ref(),
                    _ => None,
                };
                let Some(f) = frame else {
                    panic!("round {round}: no frame for {step:?}");
                };
                let inside = (f.left..f.left + f.width).contains(&x)
                    && (f.top..f.top + f.height).contains(&y);
                assert!(inside, "round {round}: {step:?} is outside {f:?}");
                positions += 1;
            }
            assert!(positions > 0, "round {round}");
        }
    }

    #[test]
    fn round_14_sets_the_lists_of_the_filters_and_of_the_sliders() {
        let probes = catalogue::round(14).unwrap_or_default();
        let plan = plan(14, &probes);
        assert!(plan.give_up && plan.refs.slider.is_some());
        assert!(plan.about.contains("a set of filters made for no question"));
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // four types and four loop rates with the low-pass column, the
        // notch by its cut-off, two impulse responses, the eight plots of
        // the set made for no question
        let copies = names("clip_save");
        assert_eq!(copies.len(), 4 * 4 + 4 * 4 + 4 + 2 + 8);
        assert_eq!(copies[0], "r14-filters-lowpass-1-pt2-lowpass-magnitude");
        assert_eq!(copies[16], "r14-filters-looprate-4k-lowpass-magnitude");
        assert_eq!(copies[36], "r14-filters-impulse-lowpass-step");
        assert_eq!(copies[45], "r14-filters-holdout-notch-step");
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        let shots = names("shot");
        assert_eq!(shots.len(), 1 + 6 + 1 + 4 + 4 + 4 + 1 + 4 + 1);
        assert_eq!(shots[1], "r14-sliders-betaflight");
        assert_eq!(shots[6], "r14-sliders-ardupilot");
        assert_eq!(shots[shots.len() - 1], "r14-filters-holdout");
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        // an item of a list lies under its list, and every wait for a
        // change has a mark before it
        let mut marked = false;
        for pair in plan.steps.windows(3) {
            let (list, item) = (&pair[0], &pair[2]);
            let name = list.name.as_deref().unwrap_or("");
            if list.window == Some(FILTERS) && name.ends_with("(the list)") {
                let (at, under) = (list.at.unwrap_or_default(), item.at.unwrap_or_default());
                assert_eq!(under.0, at.0, "{name}");
                assert!(under.1 > at.1 + 40 && under.1 < at.1 + 300, "{name}");
            }
        }
        for step in plan.steps.iter().filter(|s| s.window == Some(FILTERS)) {
            marked |= step.mark == Some(true);
            if step.action == "settle" && step.change_s.is_some() {
                assert!(marked, "a wait for a change without a mark");
                marked = false;
            }
        }
        // the entries of the slider tool's list, one below the other
        let entries: Vec<u32> = sliders_at::FIRMWARE
            .iter()
            .filter_map(|name| {
                let wanted = |s: &&Step| s.name.as_deref() == Some(*name);
                plan.steps
                    .iter()
                    .find(wanted)
                    .and_then(|s| s.at)
                    .map(|a| a.1)
            })
            .collect();
        assert_eq!(entries.len(), 6);
        assert!(entries.windows(2).all(|w| w[1] == w[0] + 46));
        assert!(entries[0] > sliders_at::FIRMWARE_LIST.1 && entries[5] < sliders_at::SIZE.1);
        // round 13 keeps its plan
        let asked = super::plan(13, &catalogue::round(13).unwrap_or_default());
        assert!(
            asked
                .about
                .contains("The slider tool: start gains typed in")
        );
    }

    #[test]
    fn round_13_asks_the_sliders_the_filters_and_the_setup() {
        let probes = catalogue::round(13).unwrap_or_default();
        let plan = plan(13, &probes);
        assert!(plan.give_up);
        assert!(plan.about.contains("The slider tool: start gains typed in"));
        assert_eq!(
            plan.refs.slider.map(|f| (f.width, f.height)),
            Some((1152, 1025))
        );
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // the filter simulator: eight plots as it comes up, four with the
        // low-passes combined and four for each value typed in
        let copies = names("clip_save");
        assert_eq!(copies.len(), 8 + 4 + 4 * 4);
        assert_eq!(copies[0], "r13-filters-open-lowpass-magnitude");
        assert_eq!(copies[7], "r13-filters-open-notch-step");
        assert_eq!(copies[8], "r13-filters-combine-lpf-lowpass-magnitude");
        assert_eq!(copies[12], "r13-filters-lowpass-1-100-lowpass-magnitude");
        assert_eq!(copies[27], "r13-filters-notch-center-300-notch-step");
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        let shots = names("shot");
        assert_eq!(shots.len(), 1 + (2 + 6 + 2) + (1 + 10 + 3 + 4) + 3);
        assert_eq!(shots[1], "r13-sliders-open");
        assert_eq!(shots[3], "r13-sliders-1-pd-up-1");
        assert_eq!(shots[11], "r13-filters-open");
        assert_eq!(shots[shots.len() - 1], "r13-setup-list-logs");
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        // a click in the slider tool is inside its small window; one in the
        // other two is inside a picture of a window
        for step in &plan.steps {
            let Some((x, y)) = step.at else { continue };
            match step.window {
                Some(SLIDERS) => {
                    assert_eq!(step.frame, Some("slider"));
                    assert!(x < 1152 && y < 1025, "{:?}", step.name);
                }
                Some(FILTERS | SETUP) => {
                    assert_eq!(step.frame, Some("shot"));
                    assert!(x < 3780 && y < 2427, "{:?}", step.name);
                }
                _ => {}
            }
        }
        // what is typed into the filter simulator is taken back, and every
        // list that is opened is closed again
        let typed = |name: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == "type" && s.name.as_deref() == Some(name))
                .filter_map(|s| s.text.as_deref())
                .collect()
        };
        assert_eq!(typed("lowpass-1-100"), ["100", "250"]);
        assert_eq!(typed("notch-center-300"), ["300", "500"]);
        let lists = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.ends_with("(the list)")))
            .count();
        let closed = plan.steps.iter().filter(|s| s.key == Some("esc")).count();
        assert_eq!((lists, closed), (12, 12));
        // the switches go on and off again
        for (name, _) in filters_at::SWITCHES {
            let presses = |state: &str| {
                let wanted = format!("{name} {state}");
                plan.steps
                    .iter()
                    .filter(|s| s.name.as_deref() == Some(wanted.as_str()))
                    .count()
            };
            assert_eq!((presses("on"), presses("off")), (1, 1));
        }
        // round 12 keeps its plan: no frame for the slider tool there
        let look = super::plan(12, &catalogue::round(12).unwrap_or_default());
        assert!(look.refs.slider.is_none() && !look.give_up);
    }

    #[test]
    fn round_12_looks_at_three_windows_without_clicking_inside_them() {
        let probes = catalogue::round(12).unwrap_or_default();
        let plan = plan(12, &probes);
        assert!(!plan.give_up);
        assert!(plan.about.contains("a first look at three windows"));
        let shots: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == "shot")
            .filter_map(|s| s.file.as_deref())
            .collect();
        assert_eq!(
            shots,
            [
                "r12-log-loaded",
                "r12-look-sliders",
                "r12-look-filters",
                "r12-look-setup"
            ]
        );
        assert!(!plan.steps.iter().any(|s| s.action == "clip_save"));
        // every click after the log is loaded is on a button of the main
        // window, one below the other, under the two the rounds have used
        let clicks: Vec<&Step> = plan.steps.iter().filter(|s| s.action == "click").collect();
        assert_eq!(clicks.len(), 4);
        assert!(clicks.iter().all(|s| s.window == Some(LOG_VIEWER)));
        let ys: Vec<u32> = clicks[1..]
            .iter()
            .filter_map(|s| s.at)
            .map(|a| a.1)
            .collect();
        assert!(ys[0] > log_at::STEP.1 + 60);
        assert!(ys.windows(2).all(|w| w[1] > w[0] + 60 && w[1] < w[0] + 120));
        // the pictures of the look are of the screen as it is: the main
        // window is not brought back over the window that came up
        for step in plan.steps.iter().filter(|s| s.action == "shot").skip(1) {
            assert_eq!(step.front, Some(false));
        }
        // the titles of the windows go into the log after every click
        let listed = plan.steps.iter().filter(|s| s.action == "windows").count();
        assert_eq!(listed, 1 + 3 + 1);
    }

    #[test]
    fn round_11_asks_the_time_window_about_its_smoothing_lists() {
        let probes = catalogue::round(11).unwrap_or_default();
        let plan = plan(11, &probes);
        assert!(plan.give_up);
        assert!(plan.about.contains("its two smoothing lists"));
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // three plots at a setting: one a log, for the first log eight
        // settings of the lists and one of the colour scale more, for the
        // flight the other channel and two settings more
        let copies = names("clip_save");
        assert_eq!(copies.len(), 3 * (7 + 8 + 1 + 1 + 2));
        for (k, name) in [
            (0, "g1-levels-2k-time-prefilt-roll"),
            (3, "g1-levels-2k-time-prefilt-freq-off-roll"),
            (12, "g1-levels-2k-time-prefilt-time-off-roll"),
            (21, "g1-levels-2k-time-prefilt-both-off-roll"),
            (24, "g1-levels-2k-time-prefilt-both-high-roll"),
            (27, "g1-levels-2k-time-prefilt-zmax-0-roll"),
            (30, "g2-len8-2k-time-prefilt-roll"),
            (45, "g7-flight-2k-time-prefilt-roll"),
            (48, "g7-flight-2k-time-gyro-roll"),
            (51, "g7-flight-2k-time-prefilt-both-med-roll"),
            (56, "g7-flight-2k-time-prefilt-both-high-yaw"),
        ] {
            assert_eq!(copies[k], format!("r11-{name}"));
        }
        let shots = names("shot");
        assert_eq!(shots.len(), 2 + 1 + copies.len() / 3);
        assert_eq!(shots[2], "r11-time-open");
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        // the throttle window is not opened; the time window once
        let opened = |name: &str| {
            let wanted = |s: &&Step| s.name.as_deref() == Some(name);
            plan.steps.iter().filter(wanted).count()
        };
        assert_eq!(opened("Spectral Analyzer"), 1);
        assert_eq!((opened("Freq x Throttle"), opened("Freq x Time")), (0, 1));
        // an item of a smoothing list lies under its list, a row further
        // down for every level, and above the switch below the lists
        let at = |name: &str| {
            let wanted = |s: &&Step| s.name.as_deref() == Some(name);
            let step = plan.steps.iter().find(wanted);
            step.and_then(|s| s.at).unwrap_or_default()
        };
        for list in ["smooth-freq", "smooth-time"] {
            let ys: Vec<u32> = ["off", "low", "med", "high"]
                .iter()
                .map(|level| at(&format!("{list} {level}")).1)
                .collect();
            assert!(ys.windows(2).all(|w| w[1] > w[0] + 40), "{list}: {ys:?}");
            let head = at(&format!("{list} low (the list)"));
            assert!(ys[0] > head.1 && ys[3] < time_at::BELOW_100.1, "{list}");
            assert_eq!(at(&format!("{list} off")).0, head.0);
        }
        // the picture is marked before the first step of a change of the
        // settings, never at the Run, and waited for no longer than 25 s
        let mut marked = 0;
        for step in &plan.steps {
            let name = step.name.as_deref().unwrap_or("");
            if step.window != Some(TIME) {
                continue;
            }
            if step.mark == Some(true) {
                assert_ne!(name, "Run");
                marked += 1;
            }
            if step.action == "settle" && step.change_s.is_some() {
                assert_eq!(step.change_s, Some(25.0));
                assert_eq!(marked, 1, "one mark for every wait");
                marked = 0;
            }
        }
        // every setting of the lists is left as the window came up with it:
        // the last pick of either list is "low", and "Z max" ends at 10
        let last = |prefix: &str| {
            let wanted = |s: &&Step| {
                let name = s.name.as_deref().unwrap_or("");
                name.starts_with(prefix) && !name.ends_with("(the list)")
            };
            plan.steps
                .iter()
                .rev()
                .find(wanted)
                .and_then(|s| s.name.clone())
        };
        assert_eq!(last("smooth-freq").as_deref(), Some("smooth-freq low"));
        assert_eq!(last("smooth-time").as_deref(), Some("smooth-time low"));
        let typed: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref() == Some("Z max"))
            .filter_map(|s| s.text.as_deref())
            .collect();
        assert_eq!(typed, ["0", "10"]);
    }

    #[test]
    fn round_10_asks_the_throttle_window_once_and_looks_at_the_time_window() {
        let probes = catalogue::round(10).unwrap_or_default();
        let plan = plan(10, &probes);
        assert!(plan.give_up);
        assert!(
            plan.about
                .contains("a first look at the window of frequency against time")
        );
        let names = |action: &str| -> Vec<&str> {
            plan.steps
                .iter()
                .filter(|s| s.action == action)
                .filter_map(|s| s.file.as_deref())
                .collect()
        };
        // the throttle window: six plots a log, as it comes up; the time
        // window: three plots at a setting, two channels a log and two
        // views more for the first
        let copies = names("clip_save");
        assert_eq!(copies.len(), 6 * 6 + 3 * (4 * 2 + 2));
        assert!(
            !copies
                .iter()
                .any(|c| c.contains("-psd-") || c.contains("-2d-"))
        );
        for (k, name) in [
            (0, "e1-len23-2k-throttle-roll-prefilt"),
            (35, "e6-len75-2k-throttle-yaw-gyro"),
            (36, "f1-bursts-2k-time-prefilt-roll"),
            (39, "f1-bursts-2k-time-gyro-roll"),
            (42, "f1-bursts-2k-time-prefilt-sub100-roll"),
            (45, "f1-bursts-2k-time-prefilt-zmin-70-roll"),
            (48, "f2-steps-2k-time-prefilt-roll"),
            (65, "f4-short-2k-time-gyro-yaw"),
        ] {
            assert_eq!(copies[k], format!("r10-{name}"));
        }
        let shots = names("shot");
        assert_eq!(
            shots[9..14],
            [
                "r10-time-open",
                "r10-f1-bursts-2k-time-prefilt",
                "r10-f1-bursts-2k-time-gyro",
                "r10-f1-bursts-2k-time-list-smooth-freq",
                "r10-f1-bursts-2k-time-list-smooth-time",
            ]
        );
        assert_eq!(shots.len(), 3 + 6 + 1 + 4 * 2 + 4);
        let mut unique: Vec<&str> = shots.iter().chain(&copies).copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len() + copies.len());
        for (i, step) in plan.steps.iter().enumerate() {
            if step.action == "clip_save" {
                assert_copy(&plan.steps, i);
            }
        }
        // the spectrum window is opened once, and both windows from it
        let position = |name: &str| {
            let wanted = |s: &Step| s.name.as_deref() == Some(name);
            plan.steps.iter().position(wanted)
        };
        let opened = |name: &str| {
            let wanted = |s: &&Step| s.name.as_deref() == Some(name);
            plan.steps.iter().filter(wanted).count()
        };
        assert_eq!(opened("Spectral Analyzer"), 1);
        assert!(position("Freq x Throttle") < position("Freq x Time"));
        let open = position("Freq x Time").unwrap_or(0);
        assert_eq!(plan.steps[open].window, Some(SPECTRUM));
        assert_eq!(plan.steps[open + 1].window, Some(TIME));
        // the throttle window: the log in all four columns, one Run a log
        let columns = |log: &str| -> Vec<(u32, u32)> {
            plan.steps
                .iter()
                .filter(|s| {
                    s.name
                        .as_deref()
                        .is_some_and(|n| n.starts_with(log) && n.contains("in column"))
                        && !s.name.as_deref().is_some_and(|n| n.ends_with("(the list)"))
                })
                .filter_map(|s| s.at)
                .collect()
        };
        assert_eq!(
            columns("log e2-len25-2k"),
            [(482, 199), (1319, 199), (2156, 199), (2996, 199)]
        );
        let runs = |window: &str| {
            plan.steps
                .iter()
                .filter(|s| s.name.as_deref() == Some("Run") && s.window == Some(window))
                .count()
        };
        assert_eq!((runs(THROTTLE), runs(TIME)), (6, 10));
        // the time window: the seventh log of the list, then the channels
        let log = position("log f1-bursts-2k").unwrap_or(0);
        assert_eq!(plan.steps[log].at, Some((3551, 496)));
        assert_eq!(plan.steps[log].window, Some(TIME));
        let channel = |name: &str| position(name).and_then(|i| plan.steps[i].at);
        assert_eq!(channel("channel prefilt"), Some((3551, 335)));
        assert_eq!(channel("channel gyro"), Some((3551, 288)));
        // an open list is photographed and closed with Esc
        let keys: Vec<&Step> = plan.steps.iter().filter(|s| s.action == "key").collect();
        assert_eq!(keys.len(), 2);
        assert!(keys.iter().all(|s| s.key == Some("esc")));
        let typed: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == "type")
            .filter_map(|s| s.text.as_deref())
            .collect();
        assert_eq!(typed, ["-70", "-30"]);
        // the toolbar button of the first plot of the time window
        let toolbar = plan
            .steps
            .iter()
            .position(|s| s.window == Some(TIME) && s.name.as_deref() == Some("roll toolbar"));
        assert_eq!(plan.steps[toolbar.unwrap_or(0)].at, Some((3104, 132)));
        // every action of the plan is one the script knows, the key too
        let json = serde_json::to_value(&plan).unwrap_or_default();
        for step in json["steps"].as_array().map_or(&[][..], Vec::as_slice) {
            let action = step["action"].as_str().unwrap_or("");
            assert!(
                SCRIPT.contains(&format!("$action -eq \"{action}\"")),
                "{action}"
            );
        }
        assert!(SCRIPT.contains("\"esc\" = 0x1B"));
        assert_eq!(time_file(10, "f", "", ""), "r10-f-time");
        // a round of the throttle window alone still has its decibels
        let earlier = super::plan(9, &catalogue::round(9).unwrap_or_default());
        assert!(
            names_of(&earlier, "clip_save")
                .iter()
                .any(|c| c.contains("-psd-"))
        );
    }

    /// The files of the steps of `plan` with `action`.
    fn names_of<'a>(plan: &'a Plan, action: &str) -> Vec<&'a str> {
        plan.steps
            .iter()
            .filter(|s| s.action == action)
            .filter_map(|s| s.file.as_deref())
            .collect()
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
        // nothing is clicked while the tool shows its progress bar: a click
        // that lands meanwhile cuts its computation short (rounds 8 and 9
        // left such bars on the owner's screen)
        assert!(script.contains("function Wait-Idle"));
        assert!(script.contains("if ($step.front -ne $false) { Wait-Idle 180 }"));
        assert!(script.contains("public static IntPtr[] Progress(uint pid)"));
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

    fn round_17() -> Plan {
        plan(17, &catalogue::round(17).unwrap_or_default())
    }

    #[test]
    fn round_17_takes_its_pictures_and_copies_in_the_order_of_their_worth() {
        let plan = round_17();
        assert!(plan.give_up && plan.refs.slider.is_none());
        assert!(plan.about.contains("logs made for no question"));
        let shots = names_of(&plan, "shot");
        let mut want: Vec<String> = ["r17-log-loaded", "r17-spectrum-open"]
            .map(String::from)
            .to_vec();
        want.extend(["e1-spec-1k", "e2-spec-4k"].map(|id| spectrum_shot(17, id, "medium")));
        want.push(channel_file(17, "i1-chan-2k", "list", ""));
        want.extend(
            SPECTRUM_CHANNELS
                .iter()
                .map(|(name, _, _)| channel_file(17, "i1-chan-2k", name, "")),
        );
        want.extend(["r17-throttle-open", "r17-e5-maps-2k-throttle"].map(String::from));
        want.push("r17-time-open".into());
        want.extend(["prefilt", "gyro"].map(|c| time_file(17, "e5-maps-2k", c, "")));
        want.push("r17-step-open".into());
        want.extend(["e3-step-2k", "e4-step-8k"].map(|id| step_file(17, id, "", "")));
        want.push(viewer_file(17, "e6-view-2k", "", ""));
        want.push("r17-filters-open".into());
        want.extend(
            [
                "verify-looprate",
                "verify-lowpass-1",
                "verify-lowpass-2",
                "verify",
            ]
            .map(|w| format!("r17-filters-{w}")),
        );
        want.push(step_file(17, "j1-path-2k", "", ""));
        for id in ["x1-samp-2k", "x2-samp-4k"] {
            want.extend(["", "smooth-med"].map(|w| viewer_file(17, id, w, "")));
        }
        let short = ["x3-len11-2k", "x4-len12-2k", "x5-len13-2k"];
        want.extend(short.map(|id| throttle_file(17, id, "", "")));
        want.extend(short.map(|id| time_file(17, id, "prefilt", "")));
        assert_eq!(shots, want);
        assert_eq!(shots.len(), 41);

        let copies = names_of(&plan, "clip_save");
        assert_eq!(copies.len(), 89);
        // the most valuable first: the flights made for no question and the
        // channels, then the stick's path, then the leftovers of round 16
        let first = |part: &str| copies.iter().position(|c| c.contains(part)).unwrap();
        let order = [
            "e1-spec-1k",
            "e2-spec-4k",
            "i1-chan-2k",
            "e5-maps-2k-throttle",
            "e5-maps-2k-time",
            "e3-step-2k",
            "e4-step-8k",
            "e6-view-2k",
            "filters-verify",
            "j1-path-2k",
            "x1-samp-2k",
            "x2-samp-4k",
            "x3-len11-2k-throttle",
            "x3-len11-2k-time",
        ];
        assert!(
            order.windows(2).all(|w| first(w[0]) < first(w[1])),
            "{copies:?}"
        );
        let count = |part: &str| copies.iter().filter(|c| c.contains(part)).count();
        assert_eq!(count("e1-spec-1k"), 6);
        // nine entries with two plots each, and the one plot of the motors
        assert_eq!(count("i1-chan-2k"), 19);
        assert!(copies.contains(&"r17-i1-chan-2k-spectrum-motors-all"));
        // the step response window cleared before each of its logs
        let resets: Vec<usize> = plan
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name.as_deref() == Some("Reset"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(resets.len(), 3);
        for (i, id) in resets
            .iter()
            .zip(["e3-step-2k", "e4-step-8k", "j1-path-2k"])
        {
            let chosen = plan.steps[*i..]
                .iter()
                .find(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log ")));
            assert_eq!(
                chosen.and_then(|s| s.name.as_deref()),
                Some(format!("log {id}").as_str())
            );
            assert_eq!(plan.steps[*i].at, Some(step_at::RESET));
            assert_eq!(plan.steps[*i].window, Some(STEP));
        }
        assert_eq!(count("e5-maps-2k"), 12);
        assert_eq!(count("e3-step-2k"), 6);
        assert_eq!(count("e6-view-2k"), 4);
        assert_eq!(count("filters-verify"), 8);
        assert_eq!(count("j1-path-2k"), 6);
        assert_eq!(count("x1-samp-2k"), 2);
        assert_eq!(count("x3-len11-2k"), 4);
        assert!(copies.contains(&"r17-j1-path-2k-step-roll-peak"));
        assert!(copies.contains(&"r17-x2-samp-4k-log-smooth-med-roll"));
        // within half an hour, and the note says so
        assert!(plan.seconds() < 30.0 * 60.0, "{} s", plan.seconds());
        let files = files_of(17, &plan, true);
        let note = String::from_utf8_lossy(&files[2].1).into_owned();
        let minutes = (plan.seconds() / 60.0).ceil() as usize;
        assert!(note.contains(&format!("{minutes} minutes")), "{note}");
        assert!(note.contains("41 pictures") && note.contains("89 plots"));
    }

    #[test]
    fn round_17_picks_every_channel_of_the_spectrum_window_with_a_picture_after_each() {
        let plan = round_17();
        let steps = &plan.steps;
        let picks: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, s)| s.window == Some(SPECTRUM))
            .filter(|(_, s)| s.name.as_deref().is_some_and(|n| n.starts_with("channel ")))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(picks.len(), SPECTRUM_CHANNELS.len() + 1);
        for (k, i) in picks.iter().enumerate() {
            let (_, entry, _) = SPECTRUM_CHANNELS[k % SPECTRUM_CHANNELS.len()];
            assert_eq!(
                steps[*i].name.as_deref(),
                Some(format!("channel {entry}").as_str())
            );
            assert_eq!(steps[*i].at, Some(spectrum_at::channel(k % 11)));
            assert_eq!(steps[*i].window, Some(SPECTRUM));
            if k < SPECTRUM_CHANNELS.len() {
                // a Run and a picture before the next pick
                let next = picks.get(k + 1).copied().unwrap_or(steps.len());
                let between = &steps[*i..next];
                assert!(between.iter().any(|s| s.name.as_deref() == Some("Run")));
                assert_eq!(between.iter().filter(|s| s.action == "shot").count(), 1);
                let copied = between.iter().filter(|s| s.action == "clip_save").count();
                let (name, _, plots) = SPECTRUM_CHANNELS[k];
                let want = match (plots, name) {
                    (false, _) => 0,
                    // the one large plot of the motors
                    (true, "motors") => 1,
                    (true, _) => 2,
                };
                assert_eq!(copied, want, "{k}");
            }
        }
        // the rows: the first two at the highlighted rows of round 16's
        // picture (494 to 567 of 2357 pixels), the last inside the list
        let to_shot = |pixel: f64| pixel * 2427.0 / 2357.0;
        assert!((f64::from(spectrum_at::channel(0).1) - to_shot(512.5)).abs() < 1.0);
        assert!((f64::from(spectrum_at::channel(1).1) - to_shot(549.5)).abs() < 1.0);
        assert!(f64::from(spectrum_at::channel(10).1) < to_shot(916.0) - 15.0);
        // the lower end of the plots is typed down before and back after
        let typed: Vec<(&str, &str)> = steps
            .iter()
            .filter(|s| s.action == "type" && s.window == Some(SPECTRUM))
            .map(|s| {
                (
                    s.name.as_deref().unwrap_or(""),
                    s.text.as_deref().unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(typed, [("Y max", "70"), ("Y min", "-90"), ("Y min", "-50")]);
        let y_min = steps
            .iter()
            .position(|s| s.name.as_deref() == Some("Y min"));
        assert!(y_min.is_some_and(|i| i < picks[0]));
        assert_eq!(steps[y_min.unwrap_or(0)].at, Some(spectrum_at::Y_MIN));
    }

    #[test]
    fn round_17_types_the_filters_its_manifest_states() {
        let plan = round_17();
        assert_eq!(filter_set(17), Some(VERIFY_FILTERS));
        assert_eq!(filter_set(16), None);
        let in_filters: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.window == Some(FILTERS))
            .collect();
        let typed: Vec<(&str, &str)> = in_filters
            .iter()
            .filter(|s| s.action == "type")
            .map(|s| {
                (
                    s.name.as_deref().unwrap_or(""),
                    s.text.as_deref().unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            typed,
            [
                ("lowpass 1 Hz", "210"),
                ("lowpass 2 Hz", "420"),
                ("Notch Center", "330"),
                ("Notch Q", "250"),
            ]
        );
        let picked: Vec<&str> = in_filters
            .iter()
            .filter_map(|s| s.name.as_deref())
            .filter(|n| n.contains(" item "))
            .collect();
        assert_eq!(
            picked,
            ["looprate item 1", "lowpass-1 item 2", "lowpass-2 item 4"]
        );
        assert_eq!(LOOP_RATES[1], 4000);
        assert_eq!(
            (LOWPASS_KINDS[2], LOWPASS_KINDS[4]),
            ("pt2", "biquad-butter")
        );
        assert_eq!(place(&LOOP_RATES, &12_345), 0);
        let text = serde_json::to_string(&VERIFY_FILTERS).unwrap_or_default();
        assert!(text.contains("\"lowpass_1\":{\"kind\":\"pt2\",\"hz\":210}"));
        assert!(text.contains("\"notch\":{\"hz\":330,\"q\":2.5}"));
    }

    fn round_18() -> Plan {
        plan(18, &catalogue::round(18).unwrap_or_default())
    }

    #[test]
    fn round_18_takes_its_pictures_and_copies_in_the_order_of_their_worth() {
        let plan = round_18();
        assert!(plan.give_up && plan.refs.slider.is_none());
        assert!(plan.about.contains("fs + 3") && plan.about.is_ascii());
        let shots = names_of(&plan, "shot");
        let copies = names_of(&plan, "clip_save");
        let count = |part: &str| copies.iter().filter(|c| c.contains(part)).count();
        for (id, n) in [
            // six spectrum plots, six throttle maps, three time maps of
            // each gyro channel, of the D term and of the PID error
            ("b1-spc-2k", 24),
            ("b2-clk-2k", 6),
            ("b3-spc-8k", 6),
            // six spectrum plots and the three maps of the time window
            ("b4-gap-2k", 9),
            // the response and the Peak, and the main window's four plots
            ("d1-stp-1k", 10),
            ("d2-stp-4k", 6),
            // the response and the Peak from windows of 2, 4 and 5 s
            ("d3-win-2k", 18),
            ("l1-2k", 4),
            ("l6-1k", 4),
            ("z8-short-2k", 2),
            ("a9-flight-2k", 2),
            ("f4-short-2k", 3),
        ] {
            assert_eq!(count(id), n, "{id}");
        }
        assert_eq!(copies.len(), 110);
        assert_eq!(shots.len(), 50);
        // A first, then B
        let first = |part: &str| copies.iter().position(|c| c.contains(part)).unwrap();
        let order = [
            "b1-spc-2k-spectrum",
            "b3-spc-8k",
            "b2-clk-2k",
            "b4-gap-2k-spectrum",
            "b1-spc-2k-throttle",
            "b1-spc-2k-time-prefilt",
            "b1-spc-2k-time-gyro",
            "b1-spc-2k-time-dterm",
            "b1-spc-2k-time-piderror",
            "d1-stp-1k-step",
            "d2-stp-4k",
            "d3-win-2k-step-roll",
            "d3-win-2k-step-window-4s",
            "d3-win-2k-step-window-5s",
            "d1-stp-1k-log",
            "l1-2k-throttle",
            "l6-1k-throttle",
            "z8-short-2k",
            "a9-flight-2k",
            "l1-2k-time",
            "f4-short-2k",
            "b4-gap-2k-time",
        ];
        assert!(
            order.windows(2).all(|w| first(w[0]) < first(w[1])),
            "{copies:?}"
        );
        // the pointer pictures come last, after every copy
        let last_copy = plan.steps.iter().rposition(|s| s.action == "clip_save");
        let first_hover = plan
            .steps
            .iter()
            .position(|s| s.action == "hover" && !s.copy.unwrap_or(false));
        assert!(last_copy < first_hover);
        let pointers: Vec<String> = ["throttle", "time", "spectrum"]
            .iter()
            .flat_map(|w| ["before", "hover", "click"].map(|k| format!("r18-pointer-{w}-{k}")))
            .collect();
        assert_eq!(shots[shots.len() - 9..], pointers[..]);
        for want in ["r18-d1-stp-1k-log", "r18-setup-pair-differences"] {
            assert!(shots.contains(&want), "{want}");
        }
        // within half an hour, and the note says so
        assert!(plan.seconds() < 30.0 * 60.0, "{} s", plan.seconds());
        let files = files_of(18, &plan, true);
        let note = String::from_utf8_lossy(&files[2].1).into_owned();
        assert!(note.contains("50 pictures") && note.contains("110 plots"));
    }

    #[test]
    fn round_18_resets_the_step_window_and_asks_windows_of_four_and_five_seconds() {
        let plan = round_18();
        let steps = &plan.steps;
        let resets: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name.as_deref() == Some("Reset"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(resets.len(), 3);
        for (i, id) in resets.iter().zip(CONFIRM_STEPS) {
            let chosen = steps[*i..]
                .iter()
                .find(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log ")));
            assert_eq!(
                chosen.and_then(|s| s.name.as_deref()),
                Some(format!("log {id}").as_str())
            );
        }
        let lengths: Vec<&str> = steps
            .iter()
            .filter_map(|s| s.name.as_deref())
            .filter(|n| n.ends_with("deconv. window"))
            .collect();
        assert_eq!(
            lengths,
            [
                "4sec deconv. window",
                "5sec deconv. window",
                "2sec deconv. window"
            ]
        );
        // the lists of the spectrum and the step windows: nine rows at most
        let chosen = |s: &&Step| s.name.as_deref().is_some_and(|n| n.starts_with("log "));
        for (window, first) in [(SPECTRUM, spectrum_at::LOG), (STEP, step_at::LOG)] {
            let rows: Vec<(u32, u32)> = steps
                .iter()
                .filter(|s| s.window == Some(window))
                .filter(chosen)
                .filter_map(|s| s.at)
                .collect();
            assert!(!rows.is_empty());
            assert!(
                rows.iter()
                    .all(|at| at.0 == first.0 && at.1 <= log_row(first.1, 8))
            );
        }
        let probes = catalogue::round(18).unwrap_or_default();
        let d3 = probes.iter().find(|p| p.id == "d3-win-2k").unwrap();
        assert_eq!(windows_captured(d3), &[4, 5]);
        let without = probes.iter().filter(|p| windows_captured(p).is_empty());
        assert_eq!(without.count(), 15);
        // the open dialog takes their names
        let paths = plan.steps[1].paths.clone().unwrap_or_default();
        assert_eq!(paths.len(), 16);
    }

    #[test]
    fn round_18_picks_the_other_channels_of_the_time_window_after_a_picture_of_its_list() {
        let plan = round_18();
        let steps = &plan.steps;
        let at = |name: &str| steps.iter().position(|s| s.name.as_deref() == Some(name));
        let list = at("channel (the list)").unwrap();
        let dterm = at("channel dterm").unwrap();
        let error = at("channel piderror").unwrap();
        assert!(list < dterm && dterm < error);
        let listed = steps[list..].iter().find(|s| s.action == "shot");
        assert_eq!(
            listed.and_then(|s| s.file.as_deref()),
            Some("r18-b1-spc-2k-time-list-channels")
        );
        let item = |row| Some(throttle_at::item(time_at::CHANNEL_LIST, row));
        assert_eq!(steps[dterm].at, item(2));
        assert_eq!(steps[error].at, item(5));
        // and the unfiltered gyro again after them
        let back = steps[error..]
            .iter()
            .position(|s| s.name.as_deref() == Some("channel prefilt"));
        assert!(back.is_some());
        // the setup list: the second log of the pair in its list
        let in_setup: Vec<&Step> = steps
            .iter()
            .filter(|s| s.window == Some(SETUP))
            .filter(|s| s.name.as_deref() == Some("log b2-clk-2k"))
            .collect();
        assert_eq!(in_setup.len(), 1);
        assert_eq!(
            in_setup[0].at,
            Some(throttle_at::item(setup_at::LOG_LIST, 2))
        );
    }
}
