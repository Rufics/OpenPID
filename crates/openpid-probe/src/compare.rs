//! `openpid-probe compare`: the reference tool's copies of a round held
//! against `OpenPID`'s own results for the same logs and settings.
//!
//! The capture plan of the round names every copy and the window it is
//! made in ([`crate::capture::Plan::copies`]); the name says the log, the axis or
//! plot and the setting. Each copy in the folder is read
//! ([`crate::emf`]), its axes are calibrated from its ticks, and what it
//! draws is compared with `OpenPID`'s result:
//!
//! | Copy | Compared on |
//! |---|---|
//! | spectrum | the solid curve (the filtered gyro; the dotted one of the unfiltered gyro is drawn with the same pen, in dashes of its own), dB against frequency, with the spectrum the Spectrum tab draws by default for the same channel ([`pack::app_psd`]): the column below 100 Hz and the full range; the median difference over the common range, against the resolution of the copy |
//! | spectrum of an entry of the list of channels | every line of the copy with `OpenPID`'s spectrum of the channel the entry stands for (the gyro channels, the D, P and F terms, the setpoint, the PID error and sum, the debug value of the axis's place); the line and the channel that agree best. The "Motors" entry draws one plot of every motor: each line with the motor nearest it, in the reference tool's units (D-008) |
//! | step response | the mean response of the newest run of the copy (the window keeps the runs before it, one colour each) against time with `OpenPID`'s, at the window length of the copy; the Peak plot's newest bar with `OpenPID`'s Peak |
//! | throttle and time maps | the map placed cell by cell on the picture's pixels, each pixel in the middle of a cell as a colour step between the two ends of its colour bar (each cell's mean colour for a picture of 16 bits), with `OpenPID`'s map on the same scale; the share within one colour level of the copy, and the printed mean and peak against `OpenPID`'s |
//! | filter simulator | every curve of the manifest's set of filters, computed with `OpenPID`'s filter simulator, or given in the results file, with the copy's nearest curve: magnitude, delay, phase or step response against frequency or time; the delays the copy prints |
//! | log viewer | every line of the plot, matched by its colour to the channel of the decoded log it is nearest to (smoothed as the copy's setting smooths it): value against time |
//!
//! `OpenPID`'s results come from a results file (`--ours`: the
//! `ours.json` of the pack, or any file of the schema in
//! `docs/calibration/PROTOCOL.md`) and, for what the file does not hold,
//! are computed from the round's logs as `pack` computes them. A copy that
//! is not a copy of the round's plan, or whose setting has no counterpart
//! in `OpenPID`, is listed as not compared.
//!
//! Where the owner has decided that `OpenPID` differs from the reference
//! tool, a copy that agrees with `OpenPID`'s result once the decided
//! difference is taken out is reported as that decided difference, not as
//! a failure: a motor's percent (D-008), and the stretch of a log the
//! analyses take by default (D-012), for which `OpenPID`'s result is
//! computed again on the reference tool's stretch when a copy fails on
//! `OpenPID`'s own.

mod filters;
mod maps;

use crate::capture::Window;
use crate::catalogue::{self, Probe};
use crate::emf::{self, Axes, Curve, Metafile, Scale};
use crate::pack;
use crate::smoothing;
use openpid_analysis::{Analysis, AnalysisCtx, Epoch, StftAnalysis, StftParams};
use openpid_analysis::{ThrottleHeatmap, ThrottleHeatmapParams};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

/// One tolerance of the comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// The check it is for.
    pub check: &'static str,
    /// The limit.
    pub limit: f64,
    /// What the limit is a limit of.
    pub meaning: &'static str,
}

/// The tolerances of every check: the one table a round is passed against.
pub const TOLERANCES: [Tolerance; 11] = [
    Tolerance {
        check: "spectrum",
        limit: 1.0,
        meaning: "median difference, units of the copy's resolution: the decibels of one unit \
                  of its metafile up the plot",
    },
    Tolerance {
        check: "step",
        limit: 0.01,
        meaning: "largest difference of the response",
    },
    Tolerance {
        check: "peak",
        limit: 0.01,
        meaning: "difference of the Peak",
    },
    Tolerance {
        check: "map",
        limit: 0.99,
        meaning: "share of the pixels in the middle of a cell (of the cells, for a picture of \
                  16 bits) within one level of the copy's colours: one colour step, or the \
                  steps one level of a 5-bit channel spans",
    },
    Tolerance {
        check: "map numbers",
        limit: 1.0,
        meaning: "difference of the printed mean and peak, units of their last printed digit",
    },
    Tolerance {
        check: "filter magnitude",
        limit: 0.001,
        meaning: "largest difference, ratio",
    },
    Tolerance {
        check: "filter delay",
        limit: 0.001,
        meaning: "largest difference, ms",
    },
    Tolerance {
        check: "filter phase",
        limit: 0.1,
        meaning: "largest difference, degrees",
    },
    Tolerance {
        check: "filter step",
        limit: 0.001,
        meaning: "largest difference of the response",
    },
    Tolerance {
        check: "printed delay",
        limit: 0.5,
        meaning: "difference of a delay the filter simulator prints, units of its last digit",
    },
    Tolerance {
        check: "line",
        limit: 0.005,
        meaning: "largest difference, share of the span of the plot's axis",
    },
];

/// The limit of `check`.
fn limit(check: &str) -> f64 {
    TOLERANCES
        .iter()
        .find(|t| t.check == check)
        .map_or(0.0, |t| t.limit)
}

/// The decided differences a copy can be reported as: the entry of
/// `docs/calibration/DECISIONS.md` and what it means for a copy.
pub const DECIDED: [(&str, &str); 2] = [
    (
        "D-008",
        "a motor's percent: OpenPID's is the share of the range between the two numbers of \
         `motorOutput`, the reference tool draws the logged number over 20; the owner kept \
         OpenPID's (2026-11-16). The copy agrees with OpenPID's motor taken to the reference \
         tool's units",
    ),
    (
        "D-012",
        "the stretch of a log the analyses take by default: OpenPID's automatic stretch, from \
         one second after the start to one second before the end; the reference tool's from \
         sample fs + 2, fs·(T − 2) − 1 samples, a log of 10 s or less whole; the owner kept \
         OpenPID's as the default (2026-11-18). The copy differs from OpenPID's result on \
         OpenPID's stretch and agrees with OpenPID's result on the reference tool's",
    ),
];

/// What came of one copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within the tolerance.
    Pass,
    /// Beyond it.
    Fail,
    /// Within the tolerance once a difference the owner decided on is
    /// taken out: the entry of [`DECIDED`].
    Decided(&'static str),
    /// Not held against anything (the reason is in the row).
    NotCompared,
    /// The copy could not be read.
    Error,
}

impl Verdict {
    fn word(self) -> String {
        match self {
            Verdict::Pass => "PASS".into(),
            Verdict::Fail => "FAIL".into(),
            Verdict::Decided(id) => format!("decided difference ({id})"),
            Verdict::NotCompared => "not compared".into(),
            Verdict::Error => "ERROR".into(),
        }
    }
}

/// One copy in the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The file, without `.emf`.
    pub copy: String,
    /// What kind of copy it is.
    pub kind: &'static str,
    /// What of `OpenPID`'s it is held against (or why it is not).
    pub against: String,
    /// The numbers.
    pub numbers: String,
    /// The verdict.
    pub verdict: Verdict,
}

/// The comparison of a round.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The round.
    pub round: u32,
    /// Where `OpenPID`'s results came from.
    pub source: String,
    /// Every copy in the folder.
    pub rows: Vec<Row>,
    /// The copies of the plan the folder does not have.
    pub missing: Vec<String>,
}

impl Report {
    /// Whether no copy failed or could not be read; a decided difference
    /// is not a failure.
    pub fn passed(&self) -> bool {
        !self
            .rows
            .iter()
            .any(|r| matches!(r.verdict, Verdict::Fail | Verdict::Error))
    }

    fn count(&self, verdict: impl Fn(Verdict) -> bool) -> usize {
        self.rows.iter().filter(|r| verdict(r.verdict)).count()
    }

    /// The report as Markdown.
    pub fn markdown(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# Round {}: the reference tool's copies against OpenPID\n",
            self.round
        );
        let _ = writeln!(s, "OpenPID's results: {}.\n", self.source);
        s.push_str("| Copy | Kind | Held against | Numbers | Result |\n|---|---|---|---|---|\n");
        for r in &self.rows {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} | {} |",
                r.copy,
                r.kind,
                cell(&r.against),
                cell(&r.numbers),
                r.verdict.word()
            );
        }
        let decided = self.count(|v| matches!(v, Verdict::Decided(_)));
        let pass = self.count(|v| v == Verdict::Pass);
        let fail = self.count(|v| v == Verdict::Fail);
        let _ = writeln!(
            s,
            "\n## Summary\n\n- {} copies in the folder: {} compared, {pass} PASS, {decided} \
             decided differences, {fail} FAIL; {} not compared; {} could not be read.\n- Copies \
             of the plan not in the folder: {}.\n- Result: **{}**.",
            self.rows.len(),
            pass + decided + fail,
            self.count(|v| v == Verdict::NotCompared),
            self.count(|v| v == Verdict::Error),
            if self.missing.is_empty() {
                "none".to_string()
            } else {
                format!("{} ({})", self.missing.len(), self.missing.join(", "))
            },
            if self.passed() { "PASS" } else { "FAIL" }
        );
        let named: Vec<&(&str, &str)> = DECIDED
            .iter()
            .filter(|(id, _)| self.rows.iter().any(|r| r.verdict == Verdict::Decided(id)))
            .collect();
        if !named.is_empty() {
            s.push_str("\n## Decided differences\n\n");
            for (id, what) in named {
                let n = self.count(|v| v == Verdict::Decided(id));
                let _ = writeln!(s, "- **{id}** ({n} copies): {what}.");
            }
        }
        s.push_str("\n## Tolerances\n\n| Check | Limit | Of |\n|---|---|---|\n");
        for t in TOLERANCES {
            let _ = writeln!(s, "| {} | {} | {} |", t.check, t.limit, t.meaning);
        }
        s
    }
}

/// Text for a cell of a Markdown table.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

// ---------------------------------------------------------------- results

/// A results file: the `ours.json` of a pack, or a file of the same
/// schema with the sections a pack does not carry (`maps`, `filters`).
/// Every section may be left out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OursFile {
    /// Per log: the spectra of every axis, the step response.
    pub probes: Vec<ProbeIn>,
    /// Maps of frequency against throttle or time.
    pub maps: Vec<MapIn>,
    /// The curves of the filter simulator's plots, by the name of the copy.
    pub filters: Vec<FilterIn>,
}

/// The results for one log.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProbeIn {
    /// Id of the log.
    pub id: String,
    /// Its axes.
    pub axes: Vec<AxisIn>,
    /// Its step responses.
    pub step: Vec<StepIn>,
}

/// The results for one axis of a log.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AxisIn {
    /// `roll`, `pitch` or `yaw`.
    pub axis: String,
    /// Its spectra.
    pub spectra: Vec<SpectrumIn>,
}

/// A spectrum.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SpectrumIn {
    /// `gyro` or `gyro_unfiltered`.
    pub channel: String,
    /// Segment length, seconds: 1 for the column below 100 Hz, 0.25 for
    /// the full range.
    pub segment_s: f64,
    /// Frequencies, Hz.
    pub freq: Vec<f64>,
    /// Levels, dB.
    pub power_db: Vec<f64>,
}

/// A step response.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StepIn {
    /// `roll`, `pitch` or `yaw`.
    pub axis: String,
    /// Length of the window, seconds.
    pub window_s: f64,
    /// Times, ms.
    pub time_ms: Vec<f64>,
    /// The mean response.
    pub mean: Vec<f64>,
    /// The Peak.
    pub peak: f64,
}

/// A map.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapIn {
    /// Id of the log.
    pub probe: String,
    /// `roll`, `pitch` or `yaw`.
    pub axis: String,
    /// `gyro` or `gyro_unfiltered`.
    pub channel: String,
    /// `throttle` or `time`.
    pub window: String,
    /// Column coordinates: throttle in percent or time in seconds.
    pub x: Vec<f64>,
    /// Row coordinates, Hz.
    pub y: Vec<f64>,
    /// Values, row by row: `z[row · x.len() + column]`.
    pub z: Vec<f64>,
    /// Whether `z` is in decibels; amplitudes otherwise, which a copy
    /// whose colour bar is in decibels is held against as `20·log10`.
    pub decibels: bool,
    /// The mean the map prints, when `OpenPID` prints one; computed from
    /// `z` by the reference tool's rule otherwise.
    pub mean: Option<f64>,
    /// The peak the map prints, likewise.
    pub peak: Option<f64>,
}

/// The curves of one plot of the filter simulator.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FilterIn {
    /// The name of the copy, without `.emf`.
    pub copy: String,
    /// The curves.
    pub curves: Vec<CurveIn>,
}

/// One curve: points in the units of the plot's axes.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CurveIn {
    /// What it is, for the report.
    pub label: String,
    /// Across, rising.
    pub x: Vec<f64>,
    /// Up.
    pub y: Vec<f64>,
}

/// One channel of a decoded log: its key, times in seconds and values.
type Line = (String, Vec<f64>, Vec<f64>);

/// Which stretch of a log `OpenPID`'s result is computed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stretch {
    /// `OpenPID`'s own, as the pack computes its results.
    Own,
    /// The reference tool's (decision D-012).
    Reference,
}

/// Where `OpenPID`'s results come from: a results file, and the logs for
/// what it does not hold.
struct Source {
    /// The round.
    round: u32,
    /// The folder of the copies.
    folder: std::path::PathBuf,
    file: OursFile,
    sessions: HashMap<&'static str, Result<FlightSession, String>>,
}

impl Source {
    /// The copy of the roll plot of the same name as `name`, an `axis`
    /// plot, if the folder has it and it can be read.
    fn sibling(&self, name: &str, axis: Axis) -> Option<Metafile> {
        let words: Vec<&str> = name
            .split('-')
            .map(|w| if w == axis.name() { "roll" } else { w })
            .collect();
        let path = self.folder.join(format!("{}.emf", words.join("-")));
        emf::read(&std::fs::read(path).ok()?).ok()
    }

    fn session(&mut self, probe: &Probe) -> Result<&FlightSession, String> {
        self.sessions
            .entry(probe.id)
            .or_insert_with(|| pack::session_of(probe).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn probe_in(&self, probe: &Probe) -> Option<&ProbeIn> {
        self.file.probes.iter().find(|p| p.id == probe.id)
    }

    /// The stretch of `probe` the results are computed on: the app's as the
    /// log comes up, or the reference tool's.
    fn epoch(&mut self, probe: &Probe, stretch: Stretch) -> Result<Epoch, String> {
        Ok(match stretch {
            Stretch::Own => pack::app_epoch(self.session(probe)?),
            Stretch::Reference => pack::reference_epoch(&probe.flight),
        })
    }

    /// `OpenPID`'s spectrum of the channel the results name `channel` on
    /// `axis` (`gyro`, `gyro_unfiltered`, one of
    /// [`pack::LISTED_CHANNELS`] or [`pack::MOTOR_CHANNELS`], which the
    /// results list under roll): frequencies and levels. On `OpenPID`'s own
    /// stretch the results file is asked first.
    fn spectrum(
        &mut self,
        probe: &Probe,
        (axis, channel): (Axis, &'static str),
        (segment_s, stretch): (f64, Stretch),
    ) -> Result<(Vec<f64>, Vec<f64>), String> {
        let listed_under = if pack::MOTOR_CHANNELS.contains(&channel) {
            Axis::Roll
        } else {
            axis
        };
        let given = self.probe_in(probe).and_then(|p| {
            let a = p.axes.iter().find(|a| a.axis == listed_under.name())?;
            let s = a
                .spectra
                .iter()
                .find(|s| s.channel == channel && s.segment_s == segment_s)?;
            Some((s.freq.clone(), s.power_db.clone()))
        });
        if let Some(given) = given.filter(|_| stretch == Stretch::Own) {
            return Ok(given);
        }
        let id = pack::listed_channel(channel, axis)
            .ok_or_else(|| format!("OpenPID has no channel {channel}"))?;
        let epoch = self.epoch(probe, stretch)?;
        let session = self.session(probe)?;
        if session.channel(&id).is_none() {
            return Err(format!("the log has no {} channel", id.key()));
        }
        let psd = pack::app_psd();
        pack::spectrum_of(session, (&id, channel), (epoch, &psd), probe.id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|s| s.segment_s == segment_s)
            .map(|s| (s.freq, s.power_db))
            .ok_or_else(|| "OpenPID has no such spectrum".to_string())
    }

    /// `OpenPID`'s step response: times, mean and Peak.
    fn step(
        &mut self,
        probe: &Probe,
        axis: Axis,
        window_s: f64,
    ) -> Result<(Vec<f64>, Vec<f64>, f64), String> {
        let given = self.probe_in(probe).and_then(|p| {
            p.step
                .iter()
                .find(|s| s.axis == axis.name() && s.window_s == window_s)
                .map(|s| (s.time_ms.clone(), s.mean.clone(), s.peak))
        });
        if let Some(given) = given {
            return Ok(given);
        }
        let session = self.session(probe)?;
        let epoch = pack::app_epoch(session);
        let step =
            pack::ours_step(session, axis, epoch, window_s, probe.id).map_err(|e| e.to_string())?;
        Ok((step.time_ms, step.mean, step.peak))
    }

    /// `OpenPID`'s map of a gyro channel; on `OpenPID`'s own stretch the
    /// results file is asked first.
    fn map(
        &mut self,
        probe: &Probe,
        (axis, stage, window): (Axis, GyroStage, Window),
        stretch: Stretch,
    ) -> Result<maps::Grid, String> {
        let channel = stage_name(stage);
        let window_name = if window == Window::Time {
            "time"
        } else {
            "throttle"
        };
        let given = self.file.maps.iter().find(|m| {
            m.probe == probe.id
                && m.axis == axis.name()
                && m.channel == channel
                && m.window == window_name
        });
        if let Some(m) = given.filter(|_| stretch == Stretch::Own) {
            return Ok(maps::Grid::given(m));
        }
        let epoch = self.epoch(probe, stretch)?;
        let session = self.session(probe)?;
        let ctx = AnalysisCtx {
            session,
            epoch: Some(epoch),
        };
        let id = ChannelId::Gyro { axis, stage };
        let grid = if window == Window::Time {
            let params = StftParams {
                channel: id,
                ..StftParams::default()
            };
            StftAnalysis
                .run(&ctx, &params)
                .map(|o| maps::Grid::of(&o.grid))
        } else {
            let params = ThrottleHeatmapParams {
                channel: id,
                ..ThrottleHeatmapParams::default()
            };
            ThrottleHeatmap
                .run(&ctx, &params)
                .map(|o| maps::Grid::of(&o.grid))
        };
        grid.map_err(|e| format!("OpenPID's map: {e}"))
    }

    /// The channels of the decoded log that a plot of the main window may
    /// draw: those of its axis (and the debug value of that place), or for
    /// the lower plot the throttle and the motors, each motor also in the
    /// reference tool's units (its key ends in [`IN_ITS_UNITS`]: D-008).
    /// A line is smoothed over `span` samples (1: as logged).
    fn lines(&mut self, probe: &Probe, plot: &str, span: usize) -> Result<Vec<Line>, String> {
        let session = self.session(probe)?;
        let times: Vec<f64> = (0..session.len())
            .map(|i| session.time_s(i).unwrap_or(f64::NAN))
            .collect();
        let motors = &session.settings.motor;
        let to_logged = match (motors.output_low, motors.output_high) {
            (Some(low), Some(high)) if high > low => Some((low, high)),
            _ => None,
        };
        let mut out = Vec::new();
        for c in session.channels.iter().filter(|c| drawn_in(&c.id, plot)) {
            let (t, v): (Vec<f64>, Vec<f64>) = times
                .iter()
                .enumerate()
                .filter_map(|(i, t)| Some((*t, c.data.get(i)?)))
                .unzip();
            let v = smoothing::smoothed(&v, span);
            if let (ChannelId::Motor { .. }, Some((low, high))) = (&c.id, to_logged) {
                let logged = v.iter().map(|p| (low + p * (high - low) / 100.0) / 20.0);
                out.push((
                    format!("{}{IN_ITS_UNITS}", c.id.key()),
                    t.clone(),
                    logged.collect(),
                ));
            }
            out.push((c.id.key(), t, v));
        }
        Ok(out)
    }
}

/// The end of the key of a motor line in the reference tool's units: the
/// logged number over 20 (D-008).
const IN_ITS_UNITS: &str = " as the logged number over 20";

/// Whether the plot `plot` of the main window may draw channel `id`.
fn drawn_in(id: &ChannelId, plot: &str) -> bool {
    let of_axis = |axis: &Axis| axis.name() == plot;
    match id {
        ChannelId::Gyro { axis, .. }
        | ChannelId::Setpoint { axis }
        | ChannelId::PidTerm { axis, .. }
        | ChannelId::PidError { axis } => of_axis(axis),
        ChannelId::Debug { idx } => Axis::ALL
            .get(usize::from(*idx))
            .is_some_and(|a| a.name() == plot),
        ChannelId::Throttle | ChannelId::Motor { .. } | ChannelId::MotorRpm { .. } => {
            plot == "throttle"
        }
        _ => false,
    }
}

fn stage_name(stage: GyroStage) -> &'static str {
    match stage {
        GyroStage::Filtered => "gyro",
        GyroStage::Raw => "gyro_unfiltered",
    }
}

// ------------------------------------------------------------------ copies

/// A copy of the plan.
#[derive(Debug, Clone)]
struct Copy<'p> {
    name: String,
    window: Window,
    /// The log, when the name or the round tells it.
    probe: Option<&'p Probe>,
    /// The name after the round and the log.
    rest: String,
}

/// The log and the rest of a copy's name `r<round>-<log>-<rest>`; a copy
/// of a round of one log may leave the log out.
fn identify<'p>(round: u32, name: &str, window: Window, probes: &'p [Probe]) -> Copy<'p> {
    let after = name
        .strip_prefix(&format!("r{round}-"))
        .unwrap_or(name)
        .to_string();
    let named = probes
        .iter()
        .filter(|p| after.starts_with(&format!("{}-", p.id)))
        .max_by_key(|p| p.id.len());
    let (probe, rest) = match named {
        Some(p) => (Some(p), after[p.id.len() + 1..].to_string()),
        None if probes.len() == 1 && window != Window::Filters => (probes.first(), after),
        None => (None, after),
    };
    Copy {
        name: name.to_string(),
        window,
        probe,
        rest,
    }
}

/// What a comparison found: what it held the copy against, its numbers,
/// and its verdict.
struct Outcome {
    against: String,
    numbers: String,
    verdict: Verdict,
}

impl Outcome {
    /// A comparison that passed or failed.
    fn judged(against: String, numbers: String, pass: bool) -> Outcome {
        Outcome {
            against,
            numbers,
            verdict: if pass { Verdict::Pass } else { Verdict::Fail },
        }
    }
}

fn not_compared(reason: impl Into<String>) -> Outcome {
    Outcome {
        against: reason.into(),
        numbers: String::new(),
        verdict: Verdict::NotCompared,
    }
}

/// A comparison on one stretch: its numbers and whether they are within
/// the tolerance.
struct Held {
    numbers: String,
    pass: bool,
}

/// A spectrum held against a line of a copy, and the median difference
/// the nearest line is picked by.
struct HeldSpectrum {
    held: Held,
    median: f64,
}

/// The numbers and the verdict of a comparison whose result depends on the
/// stretch of the log: as held on `OpenPID`'s own stretch, and when that
/// fails, held again on the reference tool's (`reference`); a copy that
/// agrees there is the decided difference D-012.
fn on_stretches(own: Held, reference: impl FnOnce() -> Result<Held, String>) -> (String, Verdict) {
    if own.pass {
        return (own.numbers, Verdict::Pass);
    }
    match reference() {
        Ok(r) => (
            format!(
                "{}; on the reference tool's stretch: {}",
                own.numbers, r.numbers
            ),
            if r.pass {
                Verdict::Decided("D-012")
            } else {
                Verdict::Fail
            },
        ),
        Err(e) => (
            format!("{}; on the reference tool's stretch: {e}", own.numbers),
            Verdict::Fail,
        ),
    }
}

fn axis_of(word: &str) -> Option<Axis> {
    Axis::ALL.into_iter().find(|a| a.name() == word)
}

/// The tokens of a name between `-`.
fn tokens(rest: &str) -> Vec<&str> {
    rest.split('-').filter(|t| !t.is_empty()).collect()
}

/// The polylines of more than two points that `keep` keeps, grouped by
/// colour and by whether they are dashes ([`Curve::joined`]) in the order
/// each group is first drawn, in data units: those of one group joined in
/// the order they were drawn.
fn groups(
    m: &Metafile,
    scales: &Axes,
    keep: impl Fn(&Curve) -> bool,
) -> Vec<(u32, Vec<(f64, f64)>)> {
    // by colour and whether the curve is in dashes
    type Key = (u32, bool);
    let mut out: Vec<(Key, Vec<(f64, f64)>)> = Vec::new();
    for c in m.curves.iter().filter(|c| c.points.len() > 2 && keep(c)) {
        let key = (c.pen.colour, c.joined);
        let points = scales.curve(c);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, all)) => all.extend(points),
            None => out.push((key, points)),
        }
    }
    out.into_iter()
        .map(|((colour, _), p)| (colour, p))
        .collect()
}

/// The smallest distance between two of the positions `v` (device units)
/// that differ: the unit a metafile places its points on.
fn unit_of(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    v.windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > 1e-9)
        .min_by(f64::total_cmp)
}

/// The resolution of a copy along the axis `scale` (up: `up`), in its
/// units: what one unit of the metafile is worth there, from the points of
/// its polylines of more than two points.
fn resolution(m: &Metafile, scale: &Scale, up: bool) -> f64 {
    let positions: Vec<f64> = m
        .curves
        .iter()
        .filter(|c| c.points.len() > 2)
        .flat_map(|c| c.points.iter().map(|p| if up { p.1 } else { p.0 }))
        .collect();
    let mid = positions.iter().sum::<f64>() / positions.len().max(1) as f64;
    unit_of(positions).map_or(0.0, |u| (scale.value(mid + u) - scale.value(mid)).abs())
}

/// The value of the polyline `(xs, ys)` (rising `xs`) at `x`, inside it.
fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> Option<f64> {
    let n = xs.len().min(ys.len());
    if n == 0 || x < xs[0] || x > xs[n - 1] {
        return None;
    }
    let i = xs[..n].partition_point(|v| *v < x);
    if i == 0 {
        return Some(ys[0]);
    }
    let (x0, x1) = (xs[i - 1], xs[i.min(n - 1)]);
    let (y0, y1) = (ys[i - 1], ys[i.min(n - 1)]);
    Some(if x1 > x0 {
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    } else {
        y1
    })
}

/// The largest and the median difference between the points of a copy
/// and the curve `(xs, ys)` at their abscissae, over the range both
/// have, and the number of points; `None` when they share no range.
fn differences(points: &[(f64, f64)], xs: &[f64], ys: &[f64]) -> Option<(f64, f64, usize)> {
    let mut d: Vec<f64> = points
        .iter()
        .filter_map(|(x, y)| Some((interpolate(xs, ys, *x)? - y).abs()))
        .collect();
    if d.is_empty() {
        return None;
    }
    d.sort_by(f64::total_cmp);
    Some((d[d.len() - 1], d[d.len() / 2], d.len()))
}

/// The largest distance of the points of a drawn line from a channel
/// `(t, v)`: a point counts as on the channel when its value lies between
/// the lowest and the highest value the channel takes within `delta` of
/// its time (the drawing places a point to a fraction of a device unit,
/// and a step is a vertical stroke). The channel is cut at the edges of
/// the plot, `clip`, as a line that leaves the plot is.
fn line_distance(
    points: &[(f64, f64)],
    t: &[f64],
    v: &[f64],
    delta: f64,
    clip: (f64, f64),
) -> Option<f64> {
    let n = t.len().min(v.len());
    if n == 0 {
        return None;
    }
    let cut = |x: f64| x.clamp(clip.0, clip.1);
    let mut worst: Option<f64> = None;
    for (ct, cv) in points {
        if *ct < t[0] - delta || *ct > t[n - 1] + delta {
            continue;
        }
        let lo = t[..n].partition_point(|s| *s < ct - delta);
        let hi = t[..n].partition_point(|s| *s <= ct + delta);
        let edges = [ct - delta, ct + delta]
            .into_iter()
            .filter_map(|e| interpolate(&t[..n], &v[..n], e));
        let (low, high) = v[lo..hi]
            .iter()
            .copied()
            .chain(edges)
            .map(cut)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                (a.min(x), b.max(x))
            });
        if low > high {
            continue;
        }
        let d = (low - cv).max(cv - high).max(0.0);
        worst = Some(worst.map_or(d, |w: f64| w.max(d)));
    }
    worst
}

fn number(v: f64) -> String {
    if v == 0.0 || (v.abs() >= 0.001 && v.abs() < 1.0e6) {
        format!("{v:.4}")
    } else {
        format!("{v:.3e}")
    }
}

// ----------------------------------------------------------------- spectra

/// The smoothing levels of the spectrum window as the names of copies hold
/// them: a copy named after one is of the window's channels as it comes
/// up, the filtered gyro drawn solid.
const SMOOTHING_LEVELS: [&str; 5] = ["low", "low-med", "medium", "med-high", "high"];
/// The level the spectrum window and `OpenPID`'s Spectrum tab come up with,
/// the one `OpenPID`'s results are at.
const DEFAULT_LEVEL: &str = "medium";

/// The results of `OpenPID` a copy of an entry of the spectrum window's list
/// of channels is held against, by the entry's name in the files
/// (`capture::SPECTRUM_CHANNELS`): the gyro channels, one of
/// [`pack::LISTED_CHANNELS`], or for "motors" each of the four motors.
fn channel_results(channel: &str) -> Option<Vec<&'static str>> {
    match channel {
        "gyro" => Some(vec!["gyro"]),
        "prefilt" => Some(vec!["gyro_unfiltered"]),
        "motors" => Some(pack::MOTOR_CHANNELS.to_vec()),
        other => pack::LISTED_CHANNELS
            .iter()
            .find(|c| **c == other)
            .map(|c| vec![*c]),
    }
}

/// What a name of the results is, for the report.
fn results_name(name: &str) -> String {
    match name {
        "gyro" => "the filtered gyro".into(),
        "gyro_unfiltered" => "the unfiltered gyro".into(),
        "dterm" => "the D term".into(),
        "pterm" => "the P term".into(),
        "fterm" => "the feedforward".into(),
        "setpoint" => "the setpoint".into(),
        "piderror" => "the PID error".into(),
        "pidsum" => "the PID sum".into(),
        "debug" => "the debug value of the axis's place".into(),
        motor => motor.replace("motor", "motor "),
    }
}

/// The points of a copy held against the spectrum `(freq, db)`, `unit` the
/// resolution of the copy in dB, `shift` added to the spectrum first.
fn hold_spectrum(
    points: &[(f64, f64)],
    (freq, db): &(Vec<f64>, Vec<f64>),
    unit: f64,
    shift: f64,
) -> Result<HeldSpectrum, String> {
    let shifted: Vec<f64> = db.iter().map(|d| d + shift).collect();
    let (max, median, n) =
        differences(points, freq, &shifted).ok_or("the copy and OpenPID share no frequencies")?;
    Ok(HeldSpectrum {
        held: Held {
            numbers: format!(
                "median {} dB ({} of the copy's unit), largest {} dB over {n} points",
                number(median),
                number(median / unit.max(1e-12)),
                number(max)
            ),
            pass: median <= limit("spectrum") * unit,
        },
        median,
    })
}

fn spectrum(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let words = tokens(copy.rest.strip_prefix("spectrum-").unwrap_or(&copy.rest));
    let Some(probe) = copy.probe else {
        return Ok(not_compared("the name does not say the log"));
    };
    if words.last() == Some(&crate::capture::MOTORS_COPY) {
        return motors_spectrum(m, src, probe);
    }
    let at = words.len().checked_sub(2);
    let (Some(column), Some(axis)) = (words.last(), at.and_then(|i| axis_of(words[i]))) else {
        return Ok(not_compared(
            "the name does not say the log, the axis and the column",
        ));
    };
    let segment_s = if *column == "sub" { 1.0 } else { 0.25 };
    let setting = words[..at.unwrap_or(0)].join("-");
    if !setting.is_empty() && !SMOOTHING_LEVELS.contains(&setting.as_str()) {
        return channel_spectrum(m, src, (probe, axis, segment_s), &setting);
    }
    if !setting.is_empty() && setting != DEFAULT_LEVEL {
        return Ok(not_compared(format!(
            "OpenPID's results are at the level the window comes up with, {DEFAULT_LEVEL}"
        )));
    }
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let unit = resolution(m, &scales.y, true);
    // the dotted line of the unfiltered gyro is drawn in dashes with the
    // pen of the solid one
    let points: Vec<(f64, f64)> = groups(m, &scales, |c| c.pen.solid() && !c.joined)
        .into_iter()
        .flat_map(|(_, p)| p)
        .collect();
    let channel = (axis, "gyro");
    let own = hold_spectrum(
        &points,
        &src.spectrum(probe, channel, (segment_s, Stretch::Own))?,
        unit,
        0.0,
    )?;
    let (numbers, verdict) = on_stretches(own.held, || {
        let theirs = src.spectrum(probe, channel, (segment_s, Stretch::Reference))?;
        hold_spectrum(&points, &theirs, unit, 0.0).map(|h| h.held)
    });
    Ok(Outcome {
        against: format!(
            "OpenPID's spectrum of the filtered {} gyro, {segment_s} s segments; the copy \
             reads to {} dB",
            axis.name(),
            number(unit)
        ),
        numbers,
        verdict,
    })
}

/// The line of a copy that agrees best with one of `OpenPID`'s spectra:
/// the name of the spectrum, the colour of the line and the median
/// difference.
type Best = (&'static str, u32, f64);

/// A copy of the spectrum window with one entry of its list of channels
/// chosen, `channel` its name in the files: every line of the copy, of
/// whatever pen (a channel alone may not be drawn solid), held against
/// `OpenPID`'s spectrum of each channel the entry may stand for; the pair
/// that agrees best is reported.
fn channel_spectrum(
    m: &Metafile,
    src: &mut Source,
    (probe, axis, segment_s): (&Probe, Axis, f64),
    channel: &str,
) -> Result<Outcome, String> {
    let Some(names) = channel_results(channel) else {
        return Ok(not_compared(format!(
            "the channel {channel} has no counterpart in OpenPID's spectrum"
        )));
    };
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let unit = resolution(m, &scales.y, true);
    let drawn = groups(m, &scales, |_| true);
    let mut best: Option<Best> = None;
    for name in &names {
        let spectrum = src.spectrum(probe, (axis, name), (segment_s, Stretch::Own))?;
        for (colour, points) in &drawn {
            let Some((_, median, _)) = differences(points, &spectrum.0, &spectrum.1) else {
                continue;
            };
            if best.is_none_or(|(_, _, b)| median < b) {
                best = Some((name, *colour, median));
            }
        }
    }
    let (name, colour, _) = best.ok_or("the copy and OpenPID share no frequencies")?;
    let points: Vec<(f64, f64)> = drawn
        .iter()
        .filter(|(c, _)| *c == colour)
        .flat_map(|(_, p)| p.iter().copied())
        .collect();
    let channel = (axis, name);
    let own = hold_spectrum(
        &points,
        &src.spectrum(probe, channel, (segment_s, Stretch::Own))?,
        unit,
        0.0,
    )?;
    let (numbers, verdict) = on_stretches(own.held, || {
        let theirs = src.spectrum(probe, channel, (segment_s, Stretch::Reference))?;
        hold_spectrum(&points, &theirs, unit, 0.0).map(|h| h.held)
    });
    Ok(Outcome {
        against: format!(
            "OpenPID's spectrum of {} on {}, {segment_s} s segments, against the copy's line \
             #{colour:06x}; the copy reads to {} dB",
            results_name(name),
            axis.name(),
            number(unit)
        ),
        numbers,
        verdict,
    })
}

/// How many decibels a motor's spectrum in the reference tool's units (the
/// logged number over 20) lies above `OpenPID`'s (a percent of the range of
/// the outputs): `20·log10((high − low) / 2000)`, 0 when the log states no
/// range (D-008).
fn motor_shift_db(session: &FlightSession) -> f64 {
    let motors = &session.settings.motor;
    match (motors.output_low, motors.output_high) {
        (Some(low), Some(high)) if high > low => 20.0 * ((high - low) / 2000.0).log10(),
        _ => 0.0,
    }
}

/// The one plot of every motor the "Motors" entry of the spectrum window's
/// list draws over the whole range: each line held against the motor of
/// `OpenPID`'s that agrees best, in the reference tool's units. A copy all
/// of whose lines agree so is the decided difference D-008.
fn motors_spectrum(m: &Metafile, src: &mut Source, probe: &Probe) -> Result<Outcome, String> {
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let unit = resolution(m, &scales.y, true);
    let shift = motor_shift_db(src.session(probe)?);
    let mut motors = Vec::new();
    for name in pack::MOTOR_CHANNELS {
        motors.push((
            name,
            src.spectrum(probe, (Axis::Roll, name), (0.25, Stretch::Own))?,
        ));
    }
    let mut parts = Vec::new();
    let mut pass = true;
    for (colour, points) in groups(m, &scales, |c| c.pen.colour != 0) {
        let held = motors
            .iter()
            .filter_map(|(name, s)| Some((name, hold_spectrum(&points, s, unit, shift).ok()?)))
            .min_by(|a, b| a.1.median.total_cmp(&b.1.median));
        if let Some((name, h)) = held {
            pass &= h.held.pass;
            parts.push(format!(
                "#{colour:06x} {}: {}",
                results_name(name),
                h.held.numbers
            ));
        } else {
            pass = false;
            parts.push(format!("#{colour:06x}: no motor shares its frequencies"));
        }
    }
    Ok(Outcome {
        against: format!(
            "OpenPID's spectra of the motors, in the reference tool's units ({} dB: the logged \
             number over 20, D-008); the copy reads to {} dB",
            number(shift),
            number(unit)
        ),
        numbers: parts.join("; "),
        verdict: match (pass && !parts.is_empty(), shift == 0.0) {
            (false, _) => Verdict::Fail,
            (true, true) => Verdict::Pass,
            (true, false) => Verdict::Decided("D-008"),
        },
    })
}

// ----------------------------------------------------------- step response

/// The setting, axis and plot of a step response copy:
/// `step-<setting>-<axis>-<plot>`.
fn step_parts(rest: &str) -> Option<(String, Axis, String)> {
    let words = tokens(rest.strip_prefix("step-")?);
    let i = words.iter().position(|w| axis_of(w).is_some())?;
    Some((
        words[..i].join("-"),
        axis_of(words[i])?,
        words[i + 1..].join("-"),
    ))
}

/// The window length of a step response setting `OpenPID` has a
/// counterpart for: as the window comes up, or with another length.
fn window_of(setting: &str) -> Option<f64> {
    if setting.is_empty() {
        return Some(2.0);
    }
    setting
        .strip_prefix("window-")?
        .strip_suffix('s')?
        .parse()
        .ok()
}

fn step(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let (Some((setting, axis, plot)), Some(probe)) = (step_parts(&copy.rest), copy.probe) else {
        return Ok(not_compared(
            "the name does not say the log, the axis and the plot",
        ));
    };
    let Some(window_s) = window_of(&setting) else {
        return Ok(not_compared(format!(
            "the setting {setting} has no counterpart in OpenPID"
        )));
    };
    match plot.as_str() {
        "response" => step_response(m, src, probe, axis, window_s),
        "peak" => step_peak(m, src, probe, axis, window_s),
        other => Ok(not_compared(format!(
            "the {other} plot has no counterpart in OpenPID's results"
        ))),
    }
}

/// The colours of the runs a plot of the step response window holds, in
/// the order they were drawn: every Run adds its curve and its bar in the
/// next colour (red, orange, yellow …) until "Reset"; the newest is the
/// last.
fn runs(m: &Metafile, keep: impl Fn(&Curve) -> bool) -> Vec<u32> {
    let mut colours: Vec<u32> = Vec::new();
    for c in m.curves.iter().filter(|c| c.pen.colour != 0 && keep(c)) {
        if !colours.contains(&c.pen.colour) {
            colours.push(c.pen.colour);
        }
    }
    colours
}

fn step_response(
    m: &Metafile,
    src: &mut Source,
    probe: &Probe,
    axis: Axis,
    window_s: f64,
) -> Result<Outcome, String> {
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let colours = runs(m, |c| c.points.len() > 2);
    let newest = colours.last().copied();
    let mut points: Vec<(f64, f64)> = groups(m, &scales, |c| Some(c.pen.colour) == newest)
        .into_iter()
        .flat_map(|(_, p)| p)
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (time_ms, mean, _) = src.step(probe, axis, window_s)?;
    let (max, median, n) =
        differences(&points, &time_ms, &mean).ok_or("the copy and OpenPID share no times")?;
    Ok(Outcome::judged(
        format!(
            "OpenPID's mean step response of {}, windows of {window_s} s, against the newest of \
             the copy's {} runs (#{:06x})",
            axis.name(),
            colours.len(),
            newest.unwrap_or(0)
        ),
        format!(
            "largest {}, median {} over {n} points",
            number(max),
            number(median)
        ),
        max <= limit("step"),
    ))
}

/// The Peak a Peak plot draws for the newest of its runs: the middle of
/// that run's bar, where its two halves meet; and the number of runs.
fn drawn_peak(m: &Metafile) -> Option<(f64, usize)> {
    let upright = |c: &Curve| c.points.len() == 2 && (c.points[0].0 - c.points[1].0).abs() < 1e-6;
    let colours = runs(m, upright);
    let newest = *colours.last()?;
    let ends: Vec<f64> = m
        .curves
        .iter()
        .filter(|c| c.pen.colour == newest && upright(c))
        .flat_map(|c| [c.points[0].1, c.points[1].1])
        .collect();
    let shared = ends
        .iter()
        .find(|e| ends.iter().filter(|f| (*f - *e).abs() < 1e-6).count() > 1);
    let middle = shared.copied().or_else(|| {
        let low = ends.iter().copied().fold(f64::INFINITY, f64::min);
        let high = ends.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (low <= high).then_some(0.5 * (low + high))
    })?;
    Some((middle, colours.len()))
}

fn step_peak(
    m: &Metafile,
    src: &mut Source,
    probe: &Probe,
    axis: Axis,
    window_s: f64,
) -> Result<Outcome, String> {
    let (_, y) = Axes::upright(m).map_err(|e| e.to_string())?;
    let (at, runs) = drawn_peak(m).ok_or("the plot has no bar")?;
    let drawn = y.value(at);
    let (_, _, peak) = src.step(probe, axis, window_s)?;
    let d = (drawn - peak).abs();
    Ok(Outcome::judged(
        format!(
            "OpenPID's Peak of {}, windows of {window_s} s, against the newest of the copy's \
             {runs} bars",
            axis.name()
        ),
        format!(
            "copy {}, OpenPID {}, difference {}",
            number(drawn),
            number(peak),
            number(d)
        ),
        d <= limit("peak"),
    ))
}

// ------------------------------------------------------------- log viewer

/// The span in samples of a line smoothing setting of the main window
/// (`smooth-<level>`): 1 for none or "off", `None` for a level the model
/// does not know.
fn viewer_span(setting: &str) -> Option<usize> {
    match setting.strip_prefix("smooth-") {
        None | Some("off") => Some(1),
        Some(level) => smoothing::span_of(level),
    }
}

/// Whether a line of a plot of the main window agrees with a channel: the
/// channel's key and the distance, or why none agrees.
fn line_verdict(
    candidates: &[Line],
    points: &[(f64, f64)],
    (delta, clip, tolerance): (f64, (f64, f64), f64),
) -> (String, Verdict) {
    let best = |motor_units: bool| {
        candidates
            .iter()
            .filter(|(key, _, _)| key.ends_with(IN_ITS_UNITS) == motor_units)
            .filter_map(|(key, t, v)| Some((key, line_distance(points, t, v, delta, clip)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    };
    match (best(false), best(true)) {
        (Some((key, d)), _) if d <= tolerance => (format!("{key} {}", number(d)), Verdict::Pass),
        (own, Some((key, d))) if d <= tolerance => {
            let ours = own.map(|(k, o)| format!(" ({k} {})", number(o)));
            (
                format!("{key} {}{}", number(d), ours.unwrap_or_default()),
                Verdict::Decided("D-008"),
            )
        }
        (Some((key, d)), _) => (format!("{key} {}", number(d)), Verdict::Fail),
        (None, _) => ("no channel".into(), Verdict::Fail),
    }
}

fn viewer(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let words = tokens(copy.rest.strip_prefix("log-").unwrap_or(&copy.rest));
    let (Some(plot), Some(probe)) = (words.last().copied(), copy.probe) else {
        return Ok(not_compared("the name does not say the log and the plot"));
    };
    let setting = words[..words.len() - 1].join("-");
    let Some(span) = viewer_span(&setting) else {
        return Ok(not_compared(format!(
            "the setting {setting} has no counterpart in OpenPID"
        )));
    };
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let f = scales.frame;
    let (bottom, top) = (scales.y.value(f.bottom), scales.y.value(f.top));
    let tolerance = limit("line") * (top - bottom).abs();
    let candidates = src.lines(probe, plot, span)?;
    let fs = probe.flight.fs;
    let delta = 0.5 * scales.x.slope.abs() + 1.5 / fs;
    let clip = (bottom.min(top), bottom.max(top));
    let mut verdict = Verdict::Pass;
    let mut parts = Vec::new();
    for (colour, points) in groups(m, &scales, |_| true) {
        let (part, v) = line_verdict(&candidates, &points, (delta, clip, tolerance));
        verdict = match (verdict, v) {
            (Verdict::Fail, _) | (_, Verdict::Fail) => Verdict::Fail,
            (Verdict::Decided(id), _) | (_, Verdict::Decided(id)) => Verdict::Decided(id),
            _ => Verdict::Pass,
        };
        parts.push(format!("#{colour:06x} {part}"));
    }
    if parts.is_empty() {
        verdict = Verdict::Fail;
    }
    let smoothed = if span > 1 {
        format!(
            ", smoothed over {span} samples as the reference tool's line smoothing was observed \
             to (a model of the rule: OpenPID's own smoothing is not on this base)"
        )
    } else {
        String::new()
    };
    Ok(Outcome {
        against: format!("the channels of the decoded log for the {plot} plot{smoothed}"),
        numbers: format!(
            "largest distance per line (tolerance {}): {}",
            number(tolerance),
            parts.join("; ")
        ),
        verdict,
    })
}

// ------------------------------------------------------------------- round

fn kind(window: Window) -> &'static str {
    match window {
        Window::LogViewer => "log viewer lines",
        Window::Spectrum => "spectrum curve",
        Window::StepResponse => "step response",
        Window::Throttle => "throttle map",
        Window::Time => "time map",
        Window::Filters => "filter simulator",
        Window::Other => "other",
    }
}

fn compare_copy(copy: &Copy, bytes: &[u8], src: &mut Source) -> Row {
    let outcome = emf::read(bytes)
        .map_err(|e| e.to_string())
        .and_then(|m| match copy.window {
            Window::Spectrum => spectrum(copy, &m, src),
            Window::StepResponse => step(copy, &m, src),
            Window::Throttle | Window::Time => maps::map(copy, &m, src),
            Window::Filters => filters::filters(copy, &m, src),
            Window::LogViewer => viewer(copy, &m, src),
            Window::Other => Ok(not_compared("a copy of a window that is not compared")),
        });
    let (against, numbers, verdict) = match outcome {
        Ok(o) => (o.against, o.numbers, o.verdict),
        Err(e) => (String::new(), e, Verdict::Error),
    };
    Row {
        copy: copy.name.clone(),
        kind: kind(copy.window),
        against,
        numbers,
        verdict,
    }
}

/// The results file at `ours`, and where the results come from, for the
/// report.
fn results_of(ours: Option<&Path>) -> Result<(OursFile, String), String> {
    let Some(path) = ours else {
        return Ok((
            OursFile::default(),
            "computed from the logs of the round as `pack` computes them".to_string(),
        ));
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let file: OursFile = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not a results file: {e}", path.display()))?;
    Ok((
        file,
        format!(
            "{}, and computed from the logs of the round as `pack` computes them where it has \
             none (and on the reference tool's stretch, D-012)",
            path.display()
        ),
    ))
}

/// Holds every copy in `dir` against `OpenPID`'s results for round
/// `round`: those of `ours` (a results file) where it has them, the rest
/// computed from the round's logs.
///
/// # Errors
/// The message for the user when the round does not exist, or the folder
/// or the results file cannot be read.
pub fn compare(round: u32, dir: &Path, ours: Option<&Path>) -> Result<Report, String> {
    let probes = catalogue::round(round).ok_or_else(|| {
        format!(
            "there is no round {round}; the rounds are {:?}",
            catalogue::ROUNDS
        )
    })?;
    let plan = crate::capture::plan(round, &probes);
    let copies = plan.copies();
    let (file, source) = results_of(ours)?;
    let mut src = Source {
        round,
        folder: dir.to_path_buf(),
        file,
        sessions: HashMap::new(),
    };
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.to_ascii_lowercase().ends_with(".emf"))
        .collect();
    names.sort();
    let mut rows = Vec::new();
    for file_name in &names {
        let stem = &file_name[..file_name.len() - 4];
        let Some((_, window)) = copies.iter().find(|(c, _)| c == stem) else {
            rows.push(Row {
                copy: stem.to_string(),
                kind: "unknown",
                against: format!("not a copy of the plan of round {round}"),
                numbers: String::new(),
                verdict: Verdict::NotCompared,
            });
            continue;
        };
        let copy = identify(round, stem, *window, &probes);
        let row = match std::fs::read(dir.join(file_name)) {
            Ok(bytes) => compare_copy(&copy, &bytes, &mut src),
            Err(e) => Row {
                copy: copy.name.clone(),
                kind: kind(copy.window),
                against: String::new(),
                numbers: format!("cannot read the file: {e}"),
                verdict: Verdict::Error,
            },
        };
        rows.push(row);
    }
    let missing = copies
        .iter()
        .filter(|(c, _)| !names.iter().any(|n| n[..n.len() - 4] == *c))
        .map(|(c, _)| c.clone())
        .collect();
    Ok(Report {
        round,
        source,
        rows,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `compare` reads off the name of a copy, or why it would not
    /// compare it: `None` when the name says nothing it can use.
    fn parsed(copy: &Copy) -> Option<String> {
        let rest = &copy.rest;
        match copy.window {
            Window::Spectrum => {
                let words = tokens(rest.strip_prefix("spectrum-")?);
                if words.last() == Some(&crate::capture::MOTORS_COPY) {
                    return channel_results(&words[..words.len() - 1].join("-"))
                        .map(|m| format!("{} motors", m.len()));
                }
                let at = words.len().checked_sub(2)?;
                let axis = axis_of(words[at])?;
                let column = words.last()?;
                let setting = words[..at].join("-");
                let known = setting == DEFAULT_LEVEL || channel_results(&setting).is_some();
                known.then(|| format!("{setting} {} {column}", axis.name()))
            }
            Window::StepResponse => {
                let (setting, axis, plot) = step_parts(rest)?;
                window_of(&setting).map(|w| format!("{w} {} {plot}", axis.name()))
            }
            Window::Throttle | Window::Time => maps::parsed(rest, copy.window),
            Window::LogViewer => {
                let words = tokens(rest.strip_prefix("log-")?);
                let setting = words[..words.len().checked_sub(1)?].join("-");
                viewer_span(&setting).map(|s| format!("{} {s}", words.last().unwrap_or(&"")))
            }
            Window::Filters => filters::parsed(rest),
            Window::Other => None,
        }
    }

    #[test]
    fn every_copy_of_round_17_is_one_compare_knows() {
        let probes = catalogue::round(17).unwrap_or_default();
        let plan = crate::capture::plan(17, &probes);
        let copies = plan.copies();
        assert_eq!(copies.len(), 89);
        for (name, window) in &copies {
            let copy = identify(17, name, *window, &probes);
            assert_eq!(copy.probe.is_none(), *window == Window::Filters, "{name}");
            assert!(parsed(&copy).is_some(), "{name}: {window:?}");
        }
        // the line smoothing at "med" is held against the observed rule
        assert_eq!(viewer_span("smooth-med"), Some(21));
        assert_eq!(viewer_span("smooth-off"), Some(1));
        assert_eq!(viewer_span("smooth-odd"), None);
        // an entry of the list of channels stands for OpenPID's channels
        assert_eq!(channel_results("prefilt"), Some(vec!["gyro_unfiltered"]));
        assert_eq!(channel_results("motors").map(|m| m.len()), Some(4));
        assert_eq!(channel_results("pidsum"), Some(vec!["pidsum"]));
        assert_eq!(channel_results("dterm-prefilt"), None);
        for (name, said) in [
            ("gyro", "the filtered gyro"),
            ("gyro_unfiltered", "the unfiltered gyro"),
            ("dterm", "the D term"),
            ("pterm", "the P term"),
            ("fterm", "the feedforward"),
            ("setpoint", "the setpoint"),
            ("piderror", "the PID error"),
            ("pidsum", "the PID sum"),
            ("debug", "the debug value of the axis's place"),
            ("motor3", "motor 3"),
        ] {
            assert_eq!(results_name(name), said);
            assert!(pack::listed_channel(name, Axis::Pitch).is_some(), "{name}");
        }
        assert_eq!(pack::listed_channel("rpm", Axis::Roll), None);
    }

    #[test]
    fn a_comparison_that_fails_on_its_own_stretch_may_be_the_decided_one() {
        let held = |pass: bool| Held {
            numbers: format!("held {pass}"),
            pass,
        };
        let (numbers, verdict) = on_stretches(held(true), || unreachable!("not asked"));
        assert_eq!((numbers.as_str(), verdict), ("held true", Verdict::Pass));
        let (numbers, verdict) = on_stretches(held(false), || Ok(held(true)));
        assert_eq!(verdict, Verdict::Decided("D-012"));
        assert!(numbers.contains("on the reference tool's stretch: held true"));
        let (_, verdict) = on_stretches(held(false), || Ok(held(false)));
        assert_eq!(verdict, Verdict::Fail);
        let (numbers, verdict) = on_stretches(held(false), || Err("no log".into()));
        assert_eq!(verdict, Verdict::Fail);
        assert!(numbers.ends_with("no log"));
        assert_eq!(
            Verdict::Decided("D-008").word(),
            "decided difference (D-008)"
        );
        assert_eq!(unit_of(vec![3.0, 1.0, 1.0, 1.25, 7.0]), Some(0.25));
        assert_eq!(unit_of(vec![2.0, 2.0]), None);
    }
}
