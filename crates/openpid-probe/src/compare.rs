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
//! | spectrum | the solid curve (the filtered gyro), dB against frequency, with `OpenPID`'s spectrum of the same channel: the column below 100 Hz with its one-second segments, the full range with its quarter-second ones; largest and median difference over the common range |
//! | step response | the mean response against time with `OpenPID`'s, at the window length of the copy; the Peak plot with `OpenPID`'s Peak |
//! | throttle and time maps | every pixel of the map as a colour step between the two ends of its colour bar, with `OpenPID`'s map at the same throttle (or time) and frequency on the same scale; the share within one step, and the printed mean and peak |
//! | filter simulator | every curve given in the results file with the copy's nearest curve: magnitude, delay, phase or step response against frequency or time |
//! | log viewer | every line of the plot, matched by its colour to the channel of the decoded log it is nearest to: value against time |
//!
//! `OpenPID`'s results come from a results file (`--ours`: the
//! `ours.json` of the pack, or any file of the schema in
//! `docs/calibration/PROTOCOL.md`) and, for what the file does not hold,
//! are computed from the round's logs as `pack` computes them. A copy that
//! is not a copy of the round's plan, or whose setting has no counterpart
//! in `OpenPID`, is listed as not compared.

use crate::capture::Window;
use crate::catalogue::{self, Probe};
use crate::emf::{self, Axes, ColourBar, Curve, Metafile, Picture};
use crate::pack;
use openpid_analysis::{
    Analysis, AnalysisCtx, Grid2D, StftAnalysis, StftParams, ThrottleHeatmap, ThrottleHeatmapParams,
};
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
pub const TOLERANCES: [Tolerance; 10] = [
    Tolerance {
        check: "spectrum",
        limit: 0.05,
        meaning: "largest difference, dB",
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
        meaning: "share of the pixels within one colour step, at least",
    },
    Tolerance {
        check: "map numbers",
        limit: 0.01,
        meaning: "difference of the printed mean and peak, share of the printed value",
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

/// What came of one copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within the tolerance.
    Pass,
    /// Beyond it.
    Fail,
    /// Not held against anything (the reason is in the row).
    NotCompared,
    /// The copy could not be read.
    Error,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::NotCompared => "not compared",
            Verdict::Error => "ERROR",
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
    /// Whether no copy failed or could not be read.
    pub fn passed(&self) -> bool {
        !self
            .rows
            .iter()
            .any(|r| matches!(r.verdict, Verdict::Fail | Verdict::Error))
    }

    fn count(&self, verdict: Verdict) -> usize {
        self.rows.iter().filter(|r| r.verdict == verdict).count()
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
        let compared = self.count(Verdict::Pass) + self.count(Verdict::Fail);
        let _ = writeln!(
            s,
            "\n## Summary\n\n- {} copies in the folder: {compared} compared, {} PASS, {} FAIL; \
             {} not compared; {} could not be read.\n- Copies of the plan not in the folder: {}.\n- \
             Result: **{}**.",
            self.rows.len(),
            self.count(Verdict::Pass),
            self.count(Verdict::Fail),
            self.count(Verdict::NotCompared),
            self.count(Verdict::Error),
            if self.missing.is_empty() {
                "none".to_string()
            } else {
                format!("{} ({})", self.missing.len(), self.missing.join(", "))
            },
            if self.passed() { "PASS" } else { "FAIL" }
        );
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
    /// Amplitudes, row by row: `z[row · x.len() + column]`.
    pub z: Vec<f64>,
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

/// A map as compared: columns, rows and values (amplitudes).
#[derive(Debug, Clone, PartialEq)]
struct Grid {
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
}

impl Grid {
    fn of(g: &Grid2D) -> Grid {
        Grid {
            x: g.x.clone(),
            y: g.y.clone(),
            z: g.z.clone(),
        }
    }

    /// The cell at `(x, y)`, if the map reaches there.
    fn cell(&self, x: f64, y: f64) -> Option<usize> {
        let ix = nearest_index(&self.x, x)?;
        let iy = nearest_index(&self.y, y)?;
        let i = iy * self.x.len() + ix;
        (i < self.z.len()).then_some(i)
    }
}

/// The index of the value of `sorted` nearest to `v`, unless `v` lies
/// beyond the ends by more than half the distance to the neighbour.
fn nearest_index(sorted: &[f64], v: f64) -> Option<usize> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    let half = if n > 1 {
        0.5 * (sorted[n - 1] - sorted[0]).abs() / (n - 1) as f64
    } else {
        0.5
    };
    if v < sorted[0] - half || v > sorted[n - 1] + half {
        return None;
    }
    let i = sorted.partition_point(|s| *s < v);
    Some(match i {
        0 => 0,
        i if i >= n => n - 1,
        i if v - sorted[i - 1] < sorted[i] - v => i - 1,
        i => i,
    })
}

/// One channel of a decoded log: its key, times in seconds and values.
type Line = (String, Vec<f64>, Vec<f64>);

/// Where `OpenPID`'s results come from: a results file, and the logs for
/// what it does not hold.
struct Source {
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

    /// `OpenPID`'s spectrum of the filtered gyro: frequencies and levels.
    fn spectrum(
        &mut self,
        probe: &Probe,
        axis: Axis,
        segment_s: f64,
    ) -> Result<(Vec<f64>, Vec<f64>), String> {
        let given = self.probe_in(probe).and_then(|p| {
            let a = p.axes.iter().find(|a| a.axis == axis.name())?;
            let s = a
                .spectra
                .iter()
                .find(|s| s.channel == "gyro" && s.segment_s == segment_s)?;
            Some((s.freq.clone(), s.power_db.clone()))
        });
        if let Some(given) = given {
            return Ok(given);
        }
        let session = self.session(probe)?;
        let spectra = pack::spectra(session, axis, pack::epoch_of(&probe.flight), probe.id)
            .map_err(|e| e.to_string())?;
        spectra
            .into_iter()
            .find(|s| s.channel == "gyro" && s.segment_s == segment_s)
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
        let epoch = pack::epoch_of(&probe.flight);
        let step =
            pack::ours_step(session, axis, epoch, window_s, probe.id).map_err(|e| e.to_string())?;
        Ok((step.time_ms, step.mean, step.peak))
    }

    /// `OpenPID`'s map of a gyro channel.
    fn map(
        &mut self,
        probe: &Probe,
        axis: Axis,
        stage: GyroStage,
        window: Window,
    ) -> Result<Grid, String> {
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
        if let Some(m) = given {
            return Ok(Grid {
                x: m.x.clone(),
                y: m.y.clone(),
                z: m.z.clone(),
            });
        }
        let session = self.session(probe)?;
        let ctx = AnalysisCtx {
            session,
            epoch: Some(pack::epoch_of(&probe.flight)),
        };
        let id = ChannelId::Gyro { axis, stage };
        let grid = if window == Window::Time {
            let params = StftParams {
                channel: id,
                ..StftParams::default()
            };
            StftAnalysis.run(&ctx, &params).map(|o| Grid::of(&o.grid))
        } else {
            let params = ThrottleHeatmapParams {
                channel: id,
                ..ThrottleHeatmapParams::default()
            };
            ThrottleHeatmap
                .run(&ctx, &params)
                .map(|o| Grid::of(&o.grid))
        };
        grid.map_err(|e| format!("OpenPID's map: {e}"))
    }

    /// The channels of the decoded log that a plot of the main window may
    /// draw: those of its axis (and the debug value of that place), or for
    /// the lower plot the throttle and the motors.
    fn lines(&mut self, probe: &Probe, plot: &str) -> Result<Vec<Line>, String> {
        let session = self.session(probe)?;
        let times: Vec<f64> = (0..session.len())
            .map(|i| session.time_s(i).unwrap_or(f64::NAN))
            .collect();
        Ok(session
            .channels
            .iter()
            .filter(|c| drawn_in(&c.id, plot))
            .map(|c| {
                let (t, v): (Vec<f64>, Vec<f64>) = times
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| Some((*t, c.data.get(i)?)))
                    .unzip();
                (c.id.key(), t, v)
            })
            .collect())
    }
}

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
/// and whether it passed (`None`: not compared).
struct Outcome {
    against: String,
    numbers: String,
    pass: Option<bool>,
}

fn not_compared(reason: impl Into<String>) -> Outcome {
    Outcome {
        against: reason.into(),
        numbers: String::new(),
        pass: None,
    }
}

fn axis_of(word: &str) -> Option<Axis> {
    Axis::ALL.into_iter().find(|a| a.name() == word)
}

/// The tokens of a name between `-`.
fn tokens(rest: &str) -> Vec<&str> {
    rest.split('-').filter(|t| !t.is_empty()).collect()
}

/// The polylines of more than two points, those of one pen joined in the
/// order they were drawn, in data units.
fn groups(
    m: &Metafile,
    scales: &Axes,
    keep: impl Fn(&Curve) -> bool,
) -> Vec<(u32, Vec<(f64, f64)>)> {
    let mut out: Vec<(emf::Pen, Vec<(f64, f64)>)> = Vec::new();
    for c in m.curves.iter().filter(|c| c.points.len() > 2 && keep(c)) {
        let points = scales.curve(c);
        match out.iter_mut().find(|(pen, _)| *pen == c.pen) {
            Some((_, all)) => all.extend(points),
            None => out.push((c.pen, points)),
        }
    }
    out.into_iter().map(|(pen, p)| (pen.colour, p)).collect()
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

fn spectrum(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let words = tokens(copy.rest.strip_prefix("spectrum-").unwrap_or(&copy.rest));
    let (Some(column), Some(axis), Some(probe)) = (
        words.last(),
        words.len().checked_sub(2).and_then(|i| axis_of(words[i])),
        copy.probe,
    ) else {
        return Ok(not_compared(
            "the name does not say the log, the axis and the column",
        ));
    };
    let segment_s = if *column == "sub" { 1.0 } else { 0.25 };
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let points: Vec<(f64, f64)> = groups(m, &scales, |c| c.pen.solid())
        .into_iter()
        .flat_map(|(_, p)| p)
        .collect();
    let (freq, db) = src.spectrum(probe, axis, segment_s)?;
    let (max, median, n) =
        differences(&points, &freq, &db).ok_or("the copy and OpenPID share no frequencies")?;
    Ok(Outcome {
        against: format!(
            "OpenPID's spectrum of the filtered {} gyro, {segment_s} s segments",
            axis.name()
        ),
        numbers: format!(
            "largest {} dB, median {} dB over {n} points",
            number(max),
            number(median)
        ),
        pass: Some(max <= limit("spectrum")),
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

fn step_response(
    m: &Metafile,
    src: &mut Source,
    probe: &Probe,
    axis: Axis,
    window_s: f64,
) -> Result<Outcome, String> {
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let mut points: Vec<(f64, f64)> = groups(m, &scales, |_| true)
        .into_iter()
        .max_by_key(|(_, p)| p.len())
        .map(|(_, p)| p)
        .unwrap_or_default();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (time_ms, mean, _) = src.step(probe, axis, window_s)?;
    let (max, median, n) =
        differences(&points, &time_ms, &mean).ok_or("the copy and OpenPID share no times")?;
    Ok(Outcome {
        against: format!(
            "OpenPID's mean step response of {}, windows of {window_s} s",
            axis.name()
        ),
        numbers: format!(
            "largest {}, median {} over {n} points",
            number(max),
            number(median)
        ),
        pass: Some(max <= limit("step")),
    })
}

/// The Peak a Peak plot draws: the middle of its coloured bar, where its
/// two halves meet.
fn drawn_peak(m: &Metafile) -> Option<f64> {
    let ends: Vec<f64> = m
        .curves
        .iter()
        .filter(|c| c.pen.colour != 0 && c.points.len() == 2)
        .filter(|c| (c.points[0].0 - c.points[1].0).abs() < 1e-6)
        .flat_map(|c| [c.points[0].1, c.points[1].1])
        .collect();
    let shared = ends
        .iter()
        .find(|e| ends.iter().filter(|f| (*f - *e).abs() < 1e-6).count() > 1);
    shared.copied().or_else(|| {
        let low = ends.iter().copied().fold(f64::INFINITY, f64::min);
        let high = ends.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (low <= high).then_some(0.5 * (low + high))
    })
}

fn step_peak(
    m: &Metafile,
    src: &mut Source,
    probe: &Probe,
    axis: Axis,
    window_s: f64,
) -> Result<Outcome, String> {
    let (_, y) = Axes::upright(m).map_err(|e| e.to_string())?;
    let drawn = y.value(drawn_peak(m).ok_or("the plot has no bar")?);
    let (_, _, peak) = src.step(probe, axis, window_s)?;
    let d = (drawn - peak).abs();
    Ok(Outcome {
        against: format!("OpenPID's Peak of {}, windows of {window_s} s", axis.name()),
        numbers: format!(
            "copy {}, OpenPID {}, difference {}",
            number(drawn),
            number(peak),
            number(d)
        ),
        pass: Some(d <= limit("peak")),
    })
}

// -------------------------------------------------------------------- maps

/// The axis, channel and setting of a map copy, and whether `OpenPID` has a
/// counterpart for the setting: `throttle-<setting>-<axis>-<channel>` or
/// `time-<channel>-<setting>-<axis>`.
fn map_parts(rest: &str, window: Window) -> Option<(Axis, GyroStage, String)> {
    let (words, stage, axis) = if window == Window::Time {
        let words = tokens(rest.strip_prefix("time-")?);
        let (stage, axis) = (*words.first()?, *words.last()?);
        (
            words[1..words.len().saturating_sub(1).max(1)].to_vec(),
            stage,
            axis,
        )
    } else {
        let words = tokens(rest.strip_prefix("throttle-")?);
        let n = words.len();
        let (axis, stage) = (*words.get(n.checked_sub(2)?)?, *words.last()?);
        (words[..n - 2].to_vec(), stage, axis)
    };
    let stage = match stage {
        "prefilt" => GyroStage::Raw,
        "gyro" => GyroStage::Filtered,
        _ => return None,
    };
    Some((axis_of(axis)?, stage, words.join("-")))
}

/// Whether a map setting only changes the view: the decibels of the
/// throttle map, the view below 100 Hz, the ends of the colour bar.
fn view_only(setting: &str) -> bool {
    setting.is_empty()
        || ["psd", "sub100"].contains(&setting)
        || ["scale-", "zmin-", "zmax-"]
            .iter()
            .any(|p| setting.starts_with(p))
}

fn map(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let (Some((axis, stage, setting)), Some(probe)) =
        (map_parts(&copy.rest, copy.window), copy.probe)
    else {
        return Ok(not_compared(
            "the name does not say the log, the axis and the channel",
        ));
    };
    if !view_only(&setting) {
        return Ok(not_compared(format!(
            "the setting {setting} has no counterpart in OpenPID"
        )));
    }
    let grid = src.map(probe, axis, stage, copy.window)?;
    let sibling = (axis != Axis::Roll)
        .then(|| src.sibling(&copy.name, axis))
        .flatten();
    let held = hold_map(m, &grid, sibling.as_ref())?;
    let mut numbers = format!(
        "{:.2} % of {} pixels within one colour step",
        100.0 * held.share,
        held.pixels
    );
    let mut pass = held.share >= limit("map");
    for (name, printed, ours) in &held.printed {
        let d = (printed - ours).abs() / printed.abs().max(1e-12);
        let _ = write!(
            numbers,
            "; {name} copy {}, OpenPID {}",
            number(*printed),
            number(*ours)
        );
        pass &= d <= limit("map numbers");
    }
    Ok(Outcome {
        against: format!(
            "OpenPID's map of the {} {} gyro on the copy's colour scale ({} … {}{})",
            if stage == GyroStage::Raw {
                "unfiltered"
            } else {
                "filtered"
            },
            axis.name(),
            number(held.ends.0),
            number(held.ends.1),
            if held.decibels { " dB" } else { "" }
        ),
        numbers,
        pass: Some(pass),
    })
}

/// A map held against a copy.
struct HeldMap {
    share: f64,
    pixels: usize,
    ends: (f64, f64),
    decibels: bool,
    /// The numbers the copy prints and `OpenPID`'s over the same cells.
    printed: Vec<(&'static str, f64, f64)>,
}

/// The picture of the map (the largest with a palette) and of its colour
/// bar (the largest other one).
fn map_pictures(m: &Metafile) -> Option<(&Picture, Option<&Picture>)> {
    let area = |p: &&Picture| p.width * p.height;
    let map = m
        .pictures
        .iter()
        .filter(|p| !p.indices.is_empty())
        .max_by_key(area)?;
    let bar = m
        .pictures
        .iter()
        .filter(|p| !std::ptr::eq(*p, map))
        .max_by_key(area);
    Some((map, bar))
}

/// The cell of `grid` under pixel `(x, y)` of a map placed at `place`,
/// when the pixel lies inside the box and inside the cell: a pixel across
/// the edge of two cells may be a blend of the two in the copy, so only
/// the pixels whose edges lie in one cell are compared. Along an axis on
/// which the cells are narrower than a pixel (`fine`), every pixel is.
fn inner_cell(
    place: &Picture,
    scales: &Axes,
    grid: &Grid,
    (x, y): (usize, usize),
    fine: (bool, bool),
) -> Option<usize> {
    let f = scales.frame;
    let page = place.centre_of(x, y);
    if page.0 < f.left || page.0 > f.right || page.1 < f.top || page.1 > f.bottom {
        return None;
    }
    let at = |p: (f64, f64)| {
        let (dx, dy) = scales.data(p);
        grid.cell(dx, dy)
    };
    let here = at(page)?;
    let (w, h) = half_pixel(place);
    let across = [(w, 0.0), (-w, 0.0)].into_iter().filter(|_| !fine.0);
    let up = [(0.0, h), (0.0, -h)].into_iter().filter(|_| !fine.1);
    across
        .chain(up)
        .all(|(dx, dy)| at((page.0 + dx, page.1 + dy)) == Some(here))
        .then_some(here)
}

/// A little less than half a pixel of `place`, across and up, device units.
fn half_pixel(place: &Picture) -> (f64, f64) {
    (
        0.49 * place.size.0 / place.width.max(1) as f64,
        0.49 * place.size.1 / place.height.max(1) as f64,
    )
}

/// Whether the cells of `grid` are narrower than a pixel of `place`:
/// across and up.
fn finer_than_pixels(place: &Picture, scales: &Axes, grid: &Grid) -> (bool, bool) {
    let (w, h) = half_pixel(place);
    let spacing = |v: &[f64]| {
        v.len()
            .checked_sub(1)
            .filter(|n| *n > 0)
            .map_or(f64::INFINITY, |n| (v[n] - v[0]).abs() / n as f64)
    };
    let pixel_x = (scales.x.value(2.0 * w) - scales.x.value(0.0)).abs();
    let pixel_y = (scales.y.value(2.0 * h) - scales.y.value(0.0)).abs();
    (pixel_x > spacing(&grid.x), pixel_y > spacing(&grid.y))
}

/// The picture placed on the box of the plot where it fills it: a
/// picture is placed on whole device units, the box to a fraction of one,
/// and a map fills its box exactly.
fn on_frame(picture: &Picture, f: &emf::Frame) -> Picture {
    let mut place = Picture {
        at: picture.at,
        size: picture.size,
        width: picture.width,
        height: picture.height,
        palette: Vec::new(),
        indices: Vec::new(),
        colours: Vec::new(),
    };
    let fills = |at: f64, size: f64, from: f64, to: f64| {
        (at - from).abs() <= 1.0 && (at + size - to).abs() <= 1.0
    };
    if fills(picture.at.0, picture.size.0, f.left, f.right) {
        (place.at.0, place.size.0) = (f.left, f.right - f.left);
    }
    if fills(picture.at.1, picture.size.1, f.top, f.bottom) {
        (place.at.1, place.size.1) = (f.top, f.bottom - f.top);
    }
    place
}

/// The colour bar of a map copy, or, for a copy without one, that of the
/// copy `sibling` (the throttle window draws one bar, above the roll
/// plot, for the three maps).
fn bar_of(m: &Metafile, sibling: Option<&Metafile>) -> Result<ColourBar, String> {
    let own = map_pictures(m).and_then(|(_, bar)| bar);
    match (own, sibling) {
        (Some(bar), _) => {
            ColourBar::of(m, bar).ok_or_else(|| "the colour bar has no labels".into())
        }
        (None, Some(other)) => {
            bar_of(other, None).map_err(|e| format!("the roll plot's copy: {e}"))
        }
        (None, None) => {
            Err("the copy has no colour bar, and no copy of the roll plot has one".into())
        }
    }
}

fn hold_map(m: &Metafile, grid: &Grid, sibling: Option<&Metafile>) -> Result<HeldMap, String> {
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let (picture, _) = map_pictures(m).ok_or("the copy has no map")?;
    let bar = bar_of(m, sibling)?;
    let decibels = bar.low < 0.0;
    let steps: Vec<f64> = picture.palette.iter().map(|c| bar.step(*c)).collect();
    let place = on_frame(picture, &scales.frame);
    let fine = finer_than_pixels(&place, &scales, grid);
    let (mut within, mut pixels, mut sum, mut peak) = (0usize, 0usize, 0.0, f64::NEG_INFINITY);
    let cell = |s: f64| s.min(255.999).floor();
    for y in 0..picture.height {
        for x in 0..picture.width {
            let Some(i) = inner_cell(&place, &scales, grid, (x, y), fine) else {
                continue;
            };
            let value = if decibels {
                20.0 * grid.z[i].max(1e-12).log10()
            } else {
                grid.z[i]
            };
            let index = picture.indices[y * picture.width + x];
            let drawn = steps.get(usize::from(index)).copied().unwrap_or(0.0);
            if (cell(drawn) - cell(bar.step_of(value))).abs() <= 1.0 {
                within += 1;
            }
            pixels += 1;
            sum += value;
            peak = peak.max(value);
        }
    }
    if pixels == 0 {
        return Err("the copy and OpenPID's map share no cells".into());
    }
    let mut printed = Vec::new();
    for (name, ours) in [("mean", sum / pixels as f64), ("peak", peak)] {
        let given = m.texts.iter().find_map(|t| {
            t.text
                .trim()
                .strip_prefix(&format!("{name}="))?
                .trim()
                .parse()
                .ok()
        });
        if let Some(given) = given {
            printed.push((name, given, ours));
        }
    }
    Ok(HeldMap {
        share: within as f64 / pixels as f64,
        pixels,
        ends: (bar.low, bar.high),
        decibels,
        printed,
    })
}

// ---------------------------------------------------------------- filters

fn filters(copy: &Copy, m: &Metafile, src: &Source) -> Result<Outcome, String> {
    let plot = tokens(&copy.rest).last().copied().unwrap_or_default();
    let check = match plot {
        "magnitude" => "filter magnitude",
        "delay" => "filter delay",
        "phase" => "filter phase",
        "step" => "filter step",
        _ => return Ok(not_compared(format!("the {plot} plot is not compared"))),
    };
    let Some(given) = src.file.filters.iter().find(|f| f.copy == copy.name) else {
        return Ok(not_compared(
            "OpenPID's curves for this setting are not in the results file (--ours; \
             docs/calibration/PROTOCOL.md)",
        ));
    };
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let drawn = groups(m, &scales, |_| true);
    let mut worst = 0.0f64;
    let mut parts = Vec::new();
    for curve in &given.curves {
        let best = drawn
            .iter()
            .filter_map(|(colour, p)| Some((*colour, differences(p, &curve.x, &curve.y)?.0)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or_else(|| format!("no curve of the copy shares a range with {}", curve.label))?;
        worst = worst.max(best.1);
        parts.push(format!(
            "{} ↔ #{:06x}: {}",
            curve.label,
            best.0,
            number(best.1)
        ));
    }
    Ok(Outcome {
        against: format!("{} curves of the results file", given.curves.len()),
        numbers: format!("largest {}; {}", number(worst), parts.join("; ")),
        pass: Some(worst <= limit(check) && !given.curves.is_empty()),
    })
}

// ------------------------------------------------------------- log viewer

fn viewer(copy: &Copy, m: &Metafile, src: &mut Source) -> Result<Outcome, String> {
    let words = tokens(copy.rest.strip_prefix("log-").unwrap_or(&copy.rest));
    let (Some(plot), Some(probe)) = (words.last().copied(), copy.probe) else {
        return Ok(not_compared("the name does not say the log and the plot"));
    };
    let setting = words[..words.len() - 1].join("-");
    if setting.starts_with("smooth-") && setting != "smooth-off" {
        return Ok(not_compared(format!(
            "the setting {setting} has no counterpart in OpenPID"
        )));
    }
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let f = scales.frame;
    let (bottom, top) = (scales.y.value(f.bottom), scales.y.value(f.top));
    let tolerance = limit("line") * (top - bottom).abs();
    let candidates = src.lines(probe, plot)?;
    let fs = probe.flight.fs;
    let delta = 0.5 * scales.x.slope.abs() + 1.5 / fs;
    let clip = (bottom.min(top), bottom.max(top));
    let mut pass = true;
    let mut parts = Vec::new();
    for (colour, points) in groups(m, &scales, |_| true) {
        let best = candidates
            .iter()
            .filter_map(|(key, t, v)| Some((key, line_distance(&points, t, v, delta, clip)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((key, d)) = best {
            pass &= d <= tolerance;
            parts.push(format!("#{colour:06x} {key} {}", number(d)));
        } else {
            pass = false;
            parts.push(format!("#{colour:06x} no channel"));
        }
    }
    Ok(Outcome {
        against: format!("the channels of the decoded log for the {plot} plot"),
        numbers: format!(
            "largest distance per line (tolerance {}): {}",
            number(tolerance),
            parts.join("; ")
        ),
        pass: Some(pass && !parts.is_empty()),
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
            Window::Throttle | Window::Time => map(copy, &m, src),
            Window::Filters => filters(copy, &m, src),
            Window::LogViewer => viewer(copy, &m, src),
            Window::Other => Ok(not_compared("a copy of a window that is not compared")),
        });
    let (against, numbers, verdict) = match outcome {
        Ok(o) => {
            let verdict = match o.pass {
                Some(true) => Verdict::Pass,
                Some(false) => Verdict::Fail,
                None => Verdict::NotCompared,
            };
            (o.against, o.numbers, verdict)
        }
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
    let (file, source) = match ours {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let file: OursFile = serde_json::from_str(&text)
                .map_err(|e| format!("{} is not a results file: {e}", path.display()))?;
            (
                file,
                format!(
                    "{}, and computed from the logs of the round as `pack` computes them where \
                     it has none",
                    path.display()
                ),
            )
        }
        None => (
            OursFile::default(),
            "computed from the logs of the round as `pack` computes them".to_string(),
        ),
    };
    let mut src = Source {
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
