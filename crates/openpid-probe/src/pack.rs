//! The pack of a round: every probe as a log file, the manifest that says
//! what is true about each, and what `OpenPID` computes for the same files.
//! From round 2 on also what the model of the reference tool
//! ([`crate::model`]) expects the tool to show, written before the tool is
//! asked.
//!
//! The pack is a pure function of the round and of this crate: built twice,
//! it is the same bytes. That is what lets a reading taken months ago be
//! traced back to the exact log it was taken from.

use crate::capture;
use crate::catalogue::{self, Area, Probe};
use crate::model;
use crate::signal::{AxisSamples, Carrier, FlightSpec, Wave, band_bins};
use crate::step_model;
use crate::truth::ResponseTruth;
use openpid_analysis::{
    Analysis, AnalysisCtx, Epoch, PsdAnalysis, PsdParams, StepAnalysis, StepParams,
};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage, LogFormat, OpenOptions, PidTerm};
use openpid_dsp::Fft;
use openpid_fmt_blackbox::BlackboxFormat;
use openpid_fmt_blackbox::write::{Target, WriteError, WriteOptions, betaflight};
use serde::Serialize;
use std::f64::consts::TAU;
use std::path::Path;

/// Version of the layout of `manifest.json` and `ours.json`.
const FORMAT: u32 = 1;
/// A flight controller logs the time since power-up; the probes start half a
/// minute after it.
const TIME_OFFSET_US: i64 = 30_000_000;
/// The release the logs are written for: Betaflight 2025.12, the newest
/// the reference tool reads. Every log it has opened from this crate was
/// one of that release.
pub const TARGET: Target = Target::V2025_12;
/// The first round whose manifest says what is true about the plants in the
/// frequency domain. The manifests of the rounds before it stay as they
/// were when the rounds were run.
const TRUTH_FROM_ROUND: u32 = 7;
/// Seconds left out at each end of a log before `OpenPID`'s own analysis, as
/// the reference tool leaves them out (`docs/DESIGN.md` §6.7).
const TRIM_S: f64 = 1.0;
/// The last round whose `ours.json` holds the spectra alone. The packs of
/// rounds 1 to 16 keep their bytes; from round 17 on `ours.json` also
/// holds `OpenPID`'s step response of every probe of that window, which
/// `openpid-probe compare` holds the reference tool's copies against.
pub const SPECTRA_ONLY_UP_TO: u32 = 16;

/// Why a pack could not be built.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// The catalogue has no such round.
    #[error("there is no round {0}; the rounds are {rounds:?}", rounds = catalogue::ROUNDS)]
    UnknownRound(u32),
    /// A probe could not be written as a log.
    #[error("probe {probe}: {source}")]
    Write {
        /// Id of the probe.
        probe: &'static str,
        /// What the writer refused.
        source: WriteError,
    },
    /// A written log could not be read back or analysed.
    #[error("probe {probe}: {message}")]
    Read {
        /// Id of the probe.
        probe: &'static str,
        /// What went wrong.
        message: String,
    },
    /// A file of the pack could not be written to disk.
    #[error("cannot write {path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// The system's error.
        source: std::io::Error,
    },
}

/// A tone of a channel: what was asked for and what the log holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToneFact {
    /// Frequency, Hz.
    pub hz: f64,
    /// Peak amplitude asked for, deg/s.
    pub amplitude: f64,
    /// `20·log10(amplitude)`.
    pub amplitude_db: f64,
    /// Mean square while it sounds, `amplitude² / 2`, as `10·log10`.
    pub power_db: f64,
    /// Where it sounds, seconds from the start of the log.
    pub span_s: (f64, f64),
    /// Amplitude of that frequency in the samples of the written log (the
    /// whole deg/s a log keeps), over the span.
    pub logged_amplitude: f64,
}

/// A band of noise of a channel: what was asked for and what the log holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BandFact {
    /// Lower edge, Hz.
    pub from_hz: f64,
    /// Upper edge, Hz.
    pub to_hz: f64,
    /// RMS asked for, deg/s.
    pub rms: f64,
    /// Its density inside the band, (deg/s)²/Hz.
    pub density: f64,
    /// `10·log10(density)`.
    pub density_db: f64,
    /// RMS of the logged samples inside the band, from their spectrum.
    pub logged_rms: f64,
}

/// What is true about one gyro channel of one axis.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelTruth {
    /// The tones of the channel.
    pub tones: Vec<ToneFact>,
    /// The bands of noise of the channel.
    pub bands: Vec<BandFact>,
    /// RMS of the white noise asked for, deg/s (0 without noise).
    pub noise_rms: f64,
    /// Its two-sided-folded density over 0 … fs/2, (deg/s)²/Hz.
    pub noise_density: f64,
    /// `10·log10(noise_density)`; absent without noise.
    pub noise_density_db: Option<f64>,
    /// Mean of the logged samples, deg/s.
    pub logged_mean: f64,
    /// RMS of the logged samples around their mean, deg/s.
    pub logged_rms: f64,
    /// Smallest and largest logged sample, deg/s.
    pub logged_range: (f64, f64),
    /// Largest difference between a logged sample and the exact one, deg/s:
    /// the rounding of the log, at most 0.5.
    pub rounding: f64,
}

/// One axis of a probe in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AxisEntry {
    /// `roll`, `pitch` or `yaw`.
    pub axis: &'static str,
    /// What the axis settles.
    pub purpose: &'static str,
    /// The gyro before the filters.
    pub unfiltered: ChannelTruth,
    /// The gyro after the filters.
    pub filtered: ChannelTruth,
    /// What is true about the plant of the axis in the frequency domain:
    /// from round 7 on, for an axis that follows its stick.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseTruth>,
    /// What a step response is when one or another channel is taken for
    /// the input of the craft: for an axis whose RC command or feedforward
    /// is not the setpoint's (round 17 on).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<StickPaths>,
}

/// The step response of an axis as it comes out with each of three
/// channels taken for the input of the craft, in closed form from the
/// plant's response to a unit step `p(t)`. The flight's setpoint `s(t)` is
/// what the craft follows: its gyro is `p` applied to `s`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StickPaths {
    /// How each of the three follows from `p`.
    pub rules: [String; 3],
    /// The times of the curves, ms.
    pub time_ms: Vec<f64>,
    /// The logged setpoint taken for the input: `p(t)`.
    pub setpoint: Vec<f64>,
    /// The setpoint the RC command asks for through the craft's rates.
    pub rc_command: Vec<f64>,
    /// The setpoint with the feedforward added to it.
    pub setpoint_and_feedforward: Vec<f64>,
}

/// A channel beyond the gyro that carries something of its own, or that
/// follows from those that do (PID error, PID sum): what is true about it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelEntry {
    /// Its field in the log (`axisP[0]`, `motor[2]`, `pidError[0]` …);
    /// PID error and PID sum are no field, a tool computes them.
    pub field: String,
    /// Its entry in the list of channels of the reference tool's spectrum
    /// window.
    pub listed_as: &'static str,
    /// The unit of the truth.
    pub unit: &'static str,
    /// How many units of the log one unit of the truth is.
    pub logged_per_unit: f64,
    /// What one unit of the truth is in what the reference tool draws,
    /// where that is known: a motor reads the logged number over 20
    /// (round 15).
    pub reference_per_unit: f64,
    /// Its tones (the level of each: `power_db`), noise and logged range,
    /// in the unit of the truth.
    pub truth: ChannelTruth,
}

/// One probe in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeEntry {
    /// Id of the probe.
    pub id: &'static str,
    /// Its log file.
    pub file: String,
    /// Size of the file, bytes.
    pub bytes: usize,
    /// FNV-1a 64 of the file, hexadecimal: tells one build of a log from
    /// another.
    pub fnv1a64: String,
    /// The area it is about.
    pub area: Area,
    /// What it is for.
    pub title: &'static str,
    /// Log rate, Hz.
    pub fs: f64,
    /// Length, seconds.
    pub duration_s: f64,
    /// Number of logged samples.
    pub samples: usize,
    /// The three axes.
    pub axes: Vec<AxisEntry>,
    /// The channels beyond the gyro that carry something of their own, and
    /// those that follow from them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelEntry>,
    /// The whole flight, as the generator takes it.
    pub flight: FlightSpec,
}

/// `manifest.json`: what is true about every log of the pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Manifest {
    /// Layout version.
    pub format: u32,
    /// The round.
    pub round: u32,
    /// The commit the pack was built from, as given by the caller.
    pub built_from: String,
    /// The probes.
    pub probes: Vec<ProbeEntry>,
    /// The filters the capture plan types into the filter simulator, which
    /// reads no log: exactly, so that `OpenPID`'s curves can be computed for
    /// them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<capture::FilterSet>,
}

/// One spectrum `OpenPID` computes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Spectrum {
    /// `gyro` or `gyro_unfiltered`; from round 17 on, for a probe of the
    /// channels of the spectrum window's list, also one of
    /// [`LISTED_CHANNELS`] and `motor1` … `motor4` (listed under roll).
    pub channel: &'static str,
    /// Segment length, seconds.
    pub segment_s: f64,
    /// Bin width, Hz.
    pub bin_hz: f64,
    /// Number of averaged segments.
    pub segments: usize,
    /// Frequency of every bin, Hz.
    pub freq: Vec<f64>,
    /// `10·log10` of the power density, (deg/s)²/Hz, before the smoothing
    /// the Spectrum tab draws with.
    pub power_db: Vec<f64>,
}

/// What `OpenPID` computes for one axis of a probe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OursAxis {
    /// `roll`, `pitch` or `yaw`.
    pub axis: &'static str,
    /// The spectra of the Spectrum tab: the full range and the range below
    /// 100 Hz, for both gyro channels.
    pub spectra: Vec<Spectrum>,
}

/// `OpenPID`'s step response of one axis.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OursStep {
    /// `roll`, `pitch` or `yaw`.
    pub axis: &'static str,
    /// Length of the window a response is computed from, seconds.
    pub window_s: f64,
    /// Time of every point of the curve, ms.
    pub time_ms: Vec<f64>,
    /// The mean response.
    pub mean: Vec<f64>,
    /// "Peak": the mean of the windows' peaks, as the Peak plot draws it.
    pub peak: f64,
}

/// What `OpenPID` computes for one probe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OursProbe {
    /// Id of the probe.
    pub id: &'static str,
    /// The part of the log analysed, seconds from its start.
    pub epoch_s: (f64, f64),
    /// The three axes.
    pub axes: Vec<OursAxis>,
    /// The step response of every axis, with the window the step response
    /// window comes up with: for a probe of that window, from the round
    /// after [`SPECTRA_ONLY_UP_TO`] on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub step: Vec<OursStep>,
}

/// `ours.json`: `OpenPID`'s results on the logs of the pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ours {
    /// Layout version.
    pub format: u32,
    /// The round.
    pub round: u32,
    /// The probes.
    pub probes: Vec<OursProbe>,
}

/// The level the model expects at one point of a curve.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedPoint {
    /// Frequency of the point, Hz.
    pub hz: f64,
    /// Level, dB.
    pub db: f64,
}

/// The expected curve of one axis, at the frequencies of [`PREDICTED_HZ`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedAxis {
    /// `roll`, `pitch` or `yaw`.
    pub axis: &'static str,
    /// The points.
    pub points: Vec<PredictedPoint>,
}

/// The expected curves at one smoothing level.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedLevel {
    /// Name of the level in the tool's list.
    pub smoothing: &'static str,
    /// Points of the regression window the model uses for it.
    pub window: usize,
    /// The three axes (the filtered gyro: the solid curve).
    pub axes: Vec<PredictedAxis>,
}

/// What the model expects for one probe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedProbe {
    /// Id of the probe.
    pub id: &'static str,
    /// First sample of the analysed span, counting from 0.
    pub first_sample: usize,
    /// Number of samples analysed.
    pub samples: usize,
    /// Distance between two points of a curve, Hz.
    pub bin_hz: f64,
    /// The five smoothing levels.
    pub levels: Vec<PredictedLevel>,
}

/// The value the model expects at one point of a step response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedStepPoint {
    /// Time of the point, ms.
    pub ms: f64,
    /// The response there.
    pub value: f64,
}

/// What the model expects of the step response of one axis at one
/// setting of the window: a smoothing level and a length of the window.
/// With no window that counts, only the counts are there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedStepLevel {
    /// Name of the level in the tool's list.
    pub smoothing: &'static str,
    /// Length of the window, seconds: two as the tool comes up.
    pub window_s: f64,
    /// Windows that count: the `n` of the tool's legend.
    pub windows: usize,
    /// "Peak": the highest value of the curve in its first 150 ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak: Option<f64>,
    /// The bar of "Peak".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_bar: Option<f64>,
    /// "SR Latency", ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    /// The bar of "SR Latency", ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_bar_ms: Option<f64>,
    /// The curve at the times of [`PREDICTED_MS`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub curve: Vec<PredictedStepPoint>,
}

/// What the model expects of the step response of one axis.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedStepAxis {
    /// `roll`, `pitch` or `yaw`.
    pub axis: &'static str,
    /// "SP-Gyro Delay", ms (the part of the model that missed eight plots
    /// of 47 in rounds 4 and 5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sp_gyro_ms: Option<f64>,
    /// As the window comes up (no smoothing, windows of two seconds), and
    /// at the other settings the round captures.
    pub levels: Vec<PredictedStepLevel>,
}

/// What the model expects of the step response window for one probe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PredictedStepProbe {
    /// Id of the probe.
    pub id: &'static str,
    /// First sample of the first window, counting from 0.
    pub first_sample: usize,
    /// Samples in a window and between the starts of two windows.
    pub window: (usize, usize),
    /// Windows the log holds, counted or not.
    pub windows_in_log: usize,
    /// The three axes.
    pub axes: Vec<PredictedStepAxis>,
}

/// `predictions.json`: what the model of the reference tool expects it to
/// show for the logs of the pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Predictions {
    /// Layout version.
    pub format: u32,
    /// The round.
    pub round: u32,
    /// What the model assumes where the rounds so far leave a choice.
    pub assumes: Vec<&'static str>,
    /// The spectrum of every probe of a spectrum round.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<PredictedProbe>,
    /// The step response of every probe of a step-response round.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub step: Vec<PredictedStepProbe>,
}

/// The frequencies at which a prediction is written down, Hz (those below
/// half the log rate; the point nearest to each).
pub const PREDICTED_HZ: [f64; 18] = [
    0.0, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0,
    200.0, 400.0,
];

/// What the predictions of a round rest on beyond what is measured.
const ASSUMES: [&str; 3] = [
    "the regression window of smoothing level L (1 for low ... 5 for high) is L cubed times \
     log10 of the number of points of the curve, made odd (found in round 2 on six logs)",
    "the analysed span starts two samples after the first second and is one sample short of \
     the log without its first and last second (round 2; at 8 kHz it was one sample longer)",
    "a realistic flight is treated as the probes were: nothing but the trim, the transform \
     and the smoothing",
];

/// The times at which a predicted step response is written down, ms.
pub const PREDICTED_MS: [f64; 18] = [
    0.0, 5.0, 10.0, 12.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0, 150.0, 200.0, 250.0,
    300.0, 400.0, 500.0,
];

/// What the predictions of a step-response round rest on beyond what is
/// measured.
const STEP_ASSUMES: [&str; 3] = [
    "version 0.94 of the tool moves its windows by a fortieth of their length, 0.05 s for \
     the 2 s it comes up with, where version 0.9 moved them by 0.1 s (measured in round 6: \
     every plot of five logs, and windows of 1 s and of 3 s on one of them); the rest of the \
     rule is that of version 0.9, which held on every plot of round 6",
    "the first window starts two samples after the first second (on three logs, of 12 s, of \
     18 s and of 41 s, it was three; on another of 12 s two; the curve moves by less than \
     0.001)",
    "the delay between setpoint and gyro is read at 8 kHz from straight lines between the \
     samples, up to 200 ms (right on 39 of 47 plots of rounds 4 and 5, one step of 0.125 ms \
     off on seven, 10 ms off on a creeping plant at 1 kHz)",
];

/// A built pack.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    /// What is true about the logs.
    pub manifest: Manifest,
    /// What `OpenPID` computes for them.
    pub ours: Ours,
    /// What the model of the reference tool expects, from round 2 on.
    pub predictions: Option<Predictions>,
    /// File name and bytes of every log.
    pub logs: Vec<(String, Vec<u8>)>,
    /// What the capture script does with them in the reference tool.
    pub plan: capture::Plan,
}

/// FNV-1a, 64 bit.
fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Six decimals: far below anything a screen shows, and the same text on
/// every machine.
fn rounded(v: f64) -> f64 {
    (v * 1.0e6).round() / 1.0e6
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::Roll => "roll",
        Axis::Pitch => "pitch",
        Axis::Yaw => "yaw",
    }
}

/// Amplitude of the `hz` component of `x` from sample `from` on.
fn amplitude_at(x: &[f64], hz: f64, fs: f64, from: usize) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let (mut s, mut c) = (0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let phase = TAU * hz * (from + i) as f64 / fs;
        s += v * phase.sin();
        c += v * phase.cos();
    }
    2.0 * (s * s + c * c).sqrt() / x.len() as f64
}

/// What is true about a channel made of `waves`, and what `logged` holds.
fn channel_truth(
    flight: &FlightSpec,
    waves: &[Wave],
    exact: &[f64],
    logged: &[f64],
) -> ChannelTruth {
    let fs = flight.fs;
    let n = logged.len();
    let tones = waves
        .iter()
        .filter_map(|wave| match wave {
            Wave::Tone {
                hz,
                amplitude,
                from_s,
                to_s,
                ..
            } => {
                let from = from_s.unwrap_or(0.0).max(0.0);
                let to = to_s.unwrap_or(flight.duration_s).min(flight.duration_s);
                let first = ((from * fs).round() as usize).min(n);
                let last = ((to * fs).round() as usize).clamp(first, n);
                Some(ToneFact {
                    hz: *hz,
                    amplitude: *amplitude,
                    amplitude_db: rounded(20.0 * amplitude.log10()),
                    power_db: rounded(10.0 * (amplitude * amplitude / 2.0).log10()),
                    span_s: (from, to),
                    logged_amplitude: rounded(amplitude_at(&logged[first..last], *hz, fs, first)),
                })
            }
            _ => None,
        })
        .collect();
    let bands = band_facts(flight, waves, logged);
    let noise_power: f64 = waves
        .iter()
        .map(|wave| match wave {
            Wave::Noise { rms, .. } => rms * rms,
            _ => 0.0,
        })
        .sum();
    let noise_density = noise_power / (fs / 2.0);
    let mean = logged.iter().sum::<f64>() / n.max(1) as f64;
    let variance = logged.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n.max(1) as f64;
    let (low, high) = logged
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
    let rounding = exact
        .iter()
        .zip(logged)
        .fold(0.0f64, |m, (e, l)| m.max((e - l).abs()));
    ChannelTruth {
        tones,
        bands,
        noise_rms: rounded(noise_power.sqrt()),
        noise_density: rounded(noise_density),
        noise_density_db: (noise_power > 0.0).then(|| rounded(10.0 * noise_density.log10())),
        logged_mean: rounded(mean),
        logged_rms: rounded(variance.sqrt()),
        logged_range: (low, high),
        rounding: rounded(rounding),
    }
}

/// The bands of `waves`, with the power the logged samples have in each.
fn band_facts(flight: &FlightSpec, waves: &[Wave], logged: &[f64]) -> Vec<BandFact> {
    if !waves.iter().any(|w| matches!(w, Wave::Band { .. })) {
        return Vec::new();
    }
    let n = logged.len();
    let spectrum = Fft::new().rfft(logged);
    waves
        .iter()
        .filter_map(|wave| match wave {
            Wave::Band {
                from_hz,
                to_hz,
                rms,
                ..
            } => {
                let (first, end) = band_bins(n, flight.fs, *from_hz, *to_hz);
                let power: f64 = spectrum[first..end]
                    .iter()
                    .map(|c| 2.0 * c.norm_sqr() / (n as f64 * n as f64))
                    .sum();
                let density = rms * rms / (to_hz - from_hz);
                Some(BandFact {
                    from_hz: *from_hz,
                    to_hz: *to_hz,
                    rms: rounded(*rms),
                    density: rounded(density),
                    density_db: rounded(10.0 * density.log10()),
                    logged_rms: rounded(power.sqrt()),
                })
            }
            _ => None,
        })
        .collect()
}

/// The samples of a gyro channel of a decoded log.
fn logged(
    session: &FlightSession,
    axis: Axis,
    stage: GyroStage,
    probe: &'static str,
) -> Result<Vec<f64>, PackError> {
    let id = ChannelId::Gyro { axis, stage };
    session
        .channel(&id)
        .filter(|c| c.data.all_valid())
        .map(|c| c.data.values().to_vec())
        .ok_or_else(|| PackError::Read {
            probe,
            message: format!("the written log has no complete {} channel", id.key()),
        })
}

/// The spectra of the Spectrum tab for one axis.
pub(crate) fn spectra(
    session: &FlightSession,
    axis: Axis,
    epoch: Epoch,
    probe: &'static str,
) -> Result<Vec<Spectrum>, PackError> {
    let mut out = Vec::new();
    for (stage, channel) in [
        (GyroStage::Filtered, "gyro"),
        (GyroStage::Raw, "gyro_unfiltered"),
    ] {
        let id = ChannelId::Gyro { axis, stage };
        out.extend(spectrum_of(session, (&id, channel), epoch, probe)?);
    }
    Ok(out)
}

/// `OpenPID`'s two spectra of the channel `id`, named `name` in the
/// results: the two columns of the Spectrum tab, the whole range with
/// quarter-second segments and below 100 Hz with one-second segments.
pub(crate) fn spectrum_of(
    session: &FlightSession,
    (id, name): (&ChannelId, &'static str),
    epoch: Epoch,
    probe: &'static str,
) -> Result<Vec<Spectrum>, PackError> {
    let ctx = AnalysisCtx {
        session,
        epoch: Some(epoch),
    };
    let mut out = Vec::new();
    for (segment_s, max_hz) in [(0.25, None), (1.0, Some(100.0))] {
        let params = PsdParams {
            channel: id.clone(),
            segment_s,
            max_hz,
            ..PsdParams::default()
        };
        let psd = PsdAnalysis
            .run(&ctx, &params)
            .map_err(|e| PackError::Read {
                probe,
                message: format!("spectrum of {name}: {e}"),
            })?;
        out.push(Spectrum {
            channel: name,
            segment_s,
            bin_hz: rounded(psd.fs / psd.nperseg as f64),
            segments: psd.segments,
            freq: psd.freq.iter().map(|f| rounded(*f)).collect(),
            power_db: psd.power_db.iter().map(|p| rounded(*p)).collect(),
        });
    }
    Ok(out)
}

/// The channels of the reference tool's spectrum window beyond the gyro
/// that `OpenPID` has a spectrum of, as the results name them: the D, P
/// and F terms, the setpoint, the PID error, the PID sum and the debug
/// value of the axis's place.
pub const LISTED_CHANNELS: [&str; 7] = [
    "dterm", "pterm", "fterm", "setpoint", "piderror", "pidsum", "debug",
];
/// The names of the four motors in the results.
pub const MOTOR_CHANNELS: [&str; 4] = ["motor1", "motor2", "motor3", "motor4"];

/// The channel of the session a name of the results stands for on `axis`:
/// one of [`LISTED_CHANNELS`] or [`MOTOR_CHANNELS`], `gyro` or
/// `gyro_unfiltered`.
pub fn listed_channel(name: &str, axis: Axis) -> Option<ChannelId> {
    let term = |term: PidTerm| Some(ChannelId::PidTerm { axis, term });
    match name {
        "gyro" | "gyro_unfiltered" => Some(ChannelId::Gyro {
            axis,
            stage: if name == "gyro" {
                GyroStage::Filtered
            } else {
                GyroStage::Raw
            },
        }),
        "dterm" => term(PidTerm::D),
        "pterm" => term(PidTerm::P),
        "fterm" => term(PidTerm::F),
        "pidsum" => term(PidTerm::Sum),
        "setpoint" => Some(ChannelId::Setpoint { axis }),
        "piderror" => Some(ChannelId::PidError { axis }),
        "debug" => Some(ChannelId::Debug {
            idx: axis.index() as u8,
        }),
        _ => MOTOR_CHANNELS
            .iter()
            .position(|m| *m == name)
            .map(|idx| ChannelId::Motor { idx: idx as u8 }),
    }
}

/// `OpenPID`'s spectra of the channels of [`LISTED_CHANNELS`] the log has
/// on `axis`, and on roll those of the four motors.
fn listed_spectra(
    session: &FlightSession,
    axis: Axis,
    epoch: Epoch,
    probe: &'static str,
) -> Result<Vec<Spectrum>, PackError> {
    let motors: &[&'static str] = if axis == Axis::Roll {
        &MOTOR_CHANNELS
    } else {
        &[]
    };
    let mut out = Vec::new();
    for name in LISTED_CHANNELS.iter().chain(motors) {
        let Some(id) = listed_channel(name, axis).filter(|id| session.channel(id).is_some()) else {
            continue;
        };
        out.extend(spectrum_of(session, (&id, name), epoch, probe)?);
    }
    Ok(out)
}

/// The curves the model of the reference tool expects for a written log.
fn predicted(
    session: &FlightSession,
    fs: f64,
    id: &'static str,
) -> Result<PredictedProbe, PackError> {
    let too_short = || PackError::Read {
        probe: id,
        message: "the log is too short for the reference tool's spectrum".into(),
    };
    let mut channels = Vec::new();
    for axis in Axis::ALL {
        channels.push((axis, logged(session, axis, GyroStage::Filtered, id)?));
    }
    let mut levels = Vec::new();
    let mut span = None;
    for (level, smoothing) in model::LEVELS.into_iter().enumerate() {
        let mut axes = Vec::new();
        let mut window = 0;
        for (axis, values) in &channels {
            let curve = model::curve(values, fs, level).ok_or_else(too_short)?;
            window = model::window(level, curve.db.len());
            let points = PREDICTED_HZ
                .iter()
                .filter(|hz| **hz < fs / 2.0)
                .filter_map(|hz| {
                    let k = (hz / curve.bin_hz).round();
                    Some(PredictedPoint {
                        hz: rounded(k * curve.bin_hz),
                        db: rounded(curve.at(*hz)?),
                    })
                })
                .collect();
            axes.push(PredictedAxis {
                axis: axis_name(*axis),
                points,
            });
            span = Some((curve.span.start, curve.span.len(), curve.bin_hz));
        }
        levels.push(PredictedLevel {
            smoothing,
            window,
            axes,
        });
    }
    let (first_sample, samples, bin_hz) = span.ok_or_else(too_short)?;
    Ok(PredictedProbe {
        id,
        first_sample,
        samples,
        bin_hz: rounded(bin_hz),
        levels,
    })
}

/// The setpoint of an axis as a written log holds it.
fn logged_setpoint(
    session: &FlightSession,
    axis: Axis,
    probe: &'static str,
) -> Result<Vec<f64>, PackError> {
    let id = ChannelId::Setpoint { axis };
    session
        .channel(&id)
        .filter(|c| c.data.all_valid())
        .map(|c| c.data.values().to_vec())
        .ok_or_else(|| PackError::Read {
            probe,
            message: format!("the written log has no complete {} channel", id.key()),
        })
}

/// One smoothing level of one axis as the model expects it.
fn predicted_step_level(
    setpoint: &[f64],
    gyro: &[f64],
    fs: f64,
    (level, window_s): (usize, f64),
) -> Option<PredictedStepLevel> {
    let step = step_model::step_of(setpoint, gyro, fs, level, window_s)?;
    let some = !step.windows.is_empty();
    let at = |ms: f64| (ms * fs / 1000.0).round() as usize;
    Some(PredictedStepLevel {
        smoothing: step_model::SMOOTHING[level % step_model::SMOOTHING.len()].0,
        window_s,
        windows: step.windows.len(),
        peak: some.then(|| rounded(step.peak)),
        peak_bar: some.then(|| rounded(step.peak_spread)),
        latency_ms: some.then(|| rounded(step.latency_ms)),
        latency_bar_ms: some.then(|| rounded(step.latency_spread_ms)),
        curve: if some {
            PREDICTED_MS
                .into_iter()
                .map(|ms| PredictedStepPoint {
                    ms,
                    value: rounded(step.mean[at(ms)]),
                })
                .collect()
        } else {
            Vec::new()
        },
    })
}

/// The settings of the step response window the model's expectation is
/// written down for: as the window comes up, the three smoothing levels
/// when the round captures them for the probe, and the settings at which
/// the round captures every plot of the probe again. Each is a smoothing
/// level and the seconds of the window.
fn predicted_settings(probe: &Probe) -> Vec<(usize, f64)> {
    let smoothed = if capture::smoothing_captured(probe) {
        1..step_model::SMOOTHING.len()
    } else {
        0..0
    };
    let again = capture::settings_captured(probe);
    std::iter::once(capture::DEFAULT_SETTING)
        .chain(smoothed.map(|level| (level, capture::DEFAULT_SETTING.1)))
        .chain(again.iter().copied())
        .map(|(level, seconds)| (level, f64::from(seconds)))
        .collect()
}

/// The step response the model of the reference tool expects for a
/// written log, at the settings of [`predicted_settings`].
fn predicted_step(session: &FlightSession, probe: &Probe) -> Result<PredictedStepProbe, PackError> {
    let id = probe.id;
    let fs = probe.flight.fs;
    let too_short = || PackError::Read {
        probe: id,
        message: "the log is too short for the reference tool's step response".into(),
    };
    let settings = predicted_settings(probe);
    let mut axes = Vec::new();
    let mut grid = None;
    for axis in Axis::ALL {
        let setpoint = logged_setpoint(session, axis, id)?;
        let gyro = logged(session, axis, GyroStage::Filtered, id)?;
        grid = step_model::grid(
            setpoint.len().min(gyro.len()),
            fs,
            step_model::LATE,
            step_model::WINDOW_S,
        );
        let levels = settings
            .iter()
            .map(|setting| predicted_step_level(&setpoint, &gyro, fs, *setting))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(too_short)?;
        axes.push(PredictedStepAxis {
            axis: axis_name(axis),
            sp_gyro_ms: step_model::sp_gyro_delay_ms(&setpoint, &gyro, fs).map(rounded),
            levels,
        });
    }
    let grid = grid.ok_or_else(too_short)?;
    Ok(PredictedStepProbe {
        id,
        first_sample: grid.first,
        window: (grid.length, grid.hop),
        windows_in_log: grid.count,
        axes,
    })
}

/// One probe as the pack holds it.
struct Built {
    /// Its entry in the manifest.
    entry: ProbeEntry,
    /// `OpenPID`'s results.
    ours: OursProbe,
    /// What the model of the reference tool expects of its spectrum, for
    /// a probe of that window.
    predicted: Option<PredictedProbe>,
    /// ... and of its step response, for a probe of that one.
    predicted_step: Option<PredictedStepProbe>,
    /// The log.
    bytes: Vec<u8>,
}

/// The header lines of the craft the probes are flown by, `name:value`
/// each: the settings of a 2025.12 quad that the writer cannot know from a
/// synthetic flight (rates, gains, battery, the firmware's features). Every
/// log the reference tool has opened from this crate carried them, so a
/// probe log keeps them: whether the tool opens a log without them has not
/// been tried. One value differs from those logs: `features` has no GPS
/// bit, since a probe logs no GPS frames.
const CRAFT: &str = include_str!("craft.txt");

/// The header lines of the probes' craft (`craft.txt`) and the name of the
/// probe, as the writer takes them.
pub fn craft_headers(id: &str) -> Vec<(String, String)> {
    // the name a tool shows for the log, so a screenshot says which probe it is
    let name = ("Craft name".to_string(), id.to_string());
    let craft = CRAFT
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_string(), value.to_string()));
    std::iter::once(name).chain(craft).collect()
}

/// The header lines the log of `probe` is written with: those of the
/// probes' craft, which has no debug mode set and digital ESCs, and for a
/// flight with debug values the debug mode it names, for one whose motor
/// outputs are not digital the protocol it names. The firmware logs its
/// debug values only while a mode is set, and the writer does as the
/// header says. (The range of the motor outputs, `motorOutput`, is no line
/// of the craft: the writer states the range the flight's session names.)
pub fn probe_headers(probe: &Probe) -> Vec<(String, String)> {
    let mut headers = craft_headers(probe.id);
    let mut set = |name: &str, to: String| {
        for (_, value) in headers.iter_mut().filter(|(key, _)| key == name) {
            value.clone_from(&to);
        }
    };
    if let Some(debug) = &probe.flight.debug {
        set("debug_mode", debug.mode.to_string());
    }
    if let Some(output) = &probe.flight.motor_output {
        set("motor_pwm_protocol", output.protocol.to_string());
    }
    headers
}

/// The log of `probe`, as a pack holds it: the same bytes in every round
/// that carries the probe.
///
/// # Errors
/// [`PackError::Write`] when the writer refuses the flight.
pub fn log_of(probe: &Probe) -> Result<Vec<u8>, PackError> {
    let options = WriteOptions {
        time_offset_us: TIME_OFFSET_US,
        headers: probe_headers(probe),
        target: TARGET,
        ..WriteOptions::default()
    };
    let probe_id = probe.id;
    betaflight(&probe.flight.build(), &options).map_err(|source| PackError::Write {
        probe: probe_id,
        source,
    })
}

/// `OpenPID`'s step response of one axis, with windows of `window_s`.
pub(crate) fn ours_step(
    session: &FlightSession,
    axis: Axis,
    epoch: Epoch,
    window_s: f64,
    probe: &'static str,
) -> Result<OursStep, PackError> {
    let ctx = AnalysisCtx {
        session,
        epoch: Some(epoch),
    };
    let params = StepParams {
        axis,
        window_s,
        ..StepParams::default()
    };
    let step = StepAnalysis
        .run(&ctx, &params)
        .map_err(|e| PackError::Read {
            probe,
            message: format!("step response of {}: {e}", axis_name(axis)),
        })?;
    Ok(OursStep {
        axis: axis_name(axis),
        window_s,
        time_ms: step.time_ms.iter().map(|t| rounded(*t)).collect(),
        mean: step.mean.iter().map(|v| rounded(*v)).collect(),
        peak: rounded(step.spread.peak_mean),
    })
}

/// The part of a flight `OpenPID` analyses: all but its first and last
/// second, as the reference tool.
pub(crate) fn epoch_of(flight: &FlightSpec) -> Epoch {
    Epoch {
        start_s: TRIM_S,
        end_s: flight.duration_s - TRIM_S,
    }
}

/// The log of `probe` decoded as `OpenPID` decodes it.
///
/// # Errors
/// [`PackError`] when the log cannot be written, or does not read back
/// whole as one session.
pub fn session_of(probe: &Probe) -> Result<FlightSession, PackError> {
    read_back(probe, &log_of(probe)?)
}

/// The written log `bytes` of `probe`, read back; it must be one session
/// with every sample of the flight and no damage.
fn read_back(probe: &Probe, bytes: &[u8]) -> Result<FlightSession, PackError> {
    let id = probe.id;
    let flight = &probe.flight;
    let read = |message: String| PackError::Read { probe: id, message };
    let mut log = BlackboxFormat::new()
        .open(bytes, &OpenOptions::default())
        .map_err(|e| read(e.to_string()))?;
    if log.sessions.len() != 1 {
        return Err(read(format!(
            "{} logs in the written file",
            log.sessions.len()
        )));
    }
    let session = log.sessions.remove(0);
    let damage = &session.diagnostics;
    if session.len() != flight.len() || damage.corrupt_frames > 0 || damage.skipped_bytes > 0 {
        return Err(read(format!(
            "the written log reads back as {} of {} samples with {} corrupt frames",
            session.len(),
            flight.len(),
            damage.corrupt_frames
        )));
    }
    Ok(session)
}

/// The times of the curves of [`StickPaths`], ms: every millisecond up to
/// half a second.
const PATH_MS: usize = 500;

/// The waves of `flight`'s carried channel `carrier`.
fn carried_waves(flight: &FlightSpec, carrier: Carrier) -> impl Iterator<Item = &Wave> {
    flight
        .carried
        .iter()
        .filter(move |c| c.channel == carrier)
        .flat_map(|c| &c.waves)
}

/// The three answers of [`StickPaths`] for `axis` of `flight`, when its RC
/// command echoes the stick ([`Wave::Echo`]) or its feedforward kicks at
/// the moves ([`Wave::Kicks`]).
///
/// With the RC command `c·s(t − d)` taken for the input, the response is
/// `p(t + d) / c`. With the setpoint and a feedforward of `k` times every
/// move for `w` seconds, the input is `s(t) + k·(s(t) − s(t − w))`, and
/// the response `Σ_j r^j / (1 + k) · p(t − j·w)` with `r = k / (1 + k)`.
fn stick_paths(flight: &FlightSpec, axis: Axis) -> Option<StickPaths> {
    let echo = carried_waves(flight, Carrier::RcCommand { axis }).find_map(|w| match w {
        Wave::Echo { delay_ms, scale } => Some((*delay_ms, *scale)),
        _ => None,
    });
    let feedforward = Carrier::Term {
        axis,
        term: PidTerm::F,
    };
    let kick = carried_waves(flight, feedforward).find_map(|w| match w {
        Wave::Kicks { ms, gain } => Some((*ms, *gain)),
        _ => None,
    });
    if echo.is_none() && kick.is_none() {
        return None;
    }
    let plant = flight.axis(axis).plant;
    let p = |ms: f64| plant.step_response(ms / 1000.0);
    let time_ms: Vec<f64> = (0..=PATH_MS).map(|ms| ms as f64).collect();
    let (delay, scale) = echo.unwrap_or((0.0, 1.0));
    let (width, gain) = kick.unwrap_or((0.0, 0.0));
    let ratio = gain / (1.0 + gain);
    let kicked = |t: f64| {
        let mut sum = 0.0;
        let mut weight = 1.0 / (1.0 + gain);
        let mut at = t;
        while at >= 0.0 && weight > 1.0e-12 {
            sum += weight * p(at);
            weight *= ratio;
            if width <= 0.0 {
                break;
            }
            at -= width;
        }
        sum
    };
    let rules = [
        "the setpoint taken for the input: p(t), the plant's response to a unit step".to_string(),
        match echo {
            Some((d, c)) => format!(
                "the RC command taken for the input, through the craft's rates: it asks for \
                 {c} times the setpoint {d} ms later, so the response is p(t + {d} ms) / {c}"
            ),
            None => "the RC command is the setpoint's image: p(t)".to_string(),
        },
        match kick {
            Some((w, k)) => format!(
                "the setpoint with the feedforward added taken for the input: the feedforward is \
                 {k} times every move for {w} ms, so the response is the sum over j of \
                 r^j / (1 + {k}) · p(t − j·{w} ms) with r = {k} / (1 + {k})"
            ),
            None => "no feedforward: p(t)".to_string(),
        },
    ];
    Some(StickPaths {
        rules,
        setpoint: time_ms.iter().map(|t| rounded(p(*t))).collect(),
        rc_command: time_ms
            .iter()
            .map(|t| rounded(p(t + delay) / scale))
            .collect(),
        setpoint_and_feedforward: time_ms.iter().map(|t| rounded(kicked(*t))).collect(),
        time_ms,
    })
}

/// The entry of the reference tool's list of channels a carried channel
/// stands for, the unit of its truth and how many units of a log one unit
/// of it is (`motor_units`: those of one percent of a motor).
fn listed_as(carrier: Carrier, motor_units: f64) -> (&'static str, &'static str, f64) {
    let raw = "the firmware's units";
    match carrier {
        Carrier::Setpoint { .. } => ("Set point", "deg/s", 1.0),
        Carrier::RcCommand { .. } => (
            "(not listed)",
            "the firmware's units, ±500 at full stick",
            1.0,
        ),
        Carrier::Term { term, .. } => match term {
            PidTerm::P => ("Pterm", raw, 1.0),
            PidTerm::D => ("Dterm", raw, 1.0),
            PidTerm::F => ("Fterm", raw, 1.0),
            _ => ("(not listed)", raw, 1.0),
        },
        Carrier::Motor { .. } => ("Motors", "percent of the motor outputs' range", motor_units),
        Carrier::Debug { .. } => ("debug", raw, 1.0),
    }
}

/// The values of channel `id` in a session, all of them valid.
fn values_of(session: &FlightSession, id: &ChannelId) -> Option<Vec<f64>> {
    session
        .channel(id)
        .filter(|c| c.data.all_valid())
        .map(|c| c.data.values().to_vec())
}

/// The error for a channel the written log does not hold whole.
fn missing(field: &str) -> PackError {
    PackError::Read {
        probe: "carried channel",
        message: format!("the written log has no complete {field}"),
    }
}

/// What is true about the carried channel `carrier` of `flight`: `exact`
/// is the flight's session, `session` the written log read back.
fn carried_entry(
    flight: &FlightSpec,
    (exact, session): (&FlightSession, &FlightSession),
    carrier: Carrier,
) -> Result<ChannelEntry, PackError> {
    let motors = &session.settings.motor;
    let motor_units = match (motors.output_low, motors.output_high) {
        (Some(low), Some(high)) if high > low => (high - low) / 100.0,
        _ => 1.0,
    };
    let id = carrier.id();
    let field = carrier.field();
    let waves: Vec<Wave> = carried_waves(flight, carrier).cloned().collect();
    let truth = values_of(exact, &id).ok_or_else(|| missing(&field))?;
    let logged = values_of(session, &id).ok_or_else(|| missing(&field))?;
    let (listed, unit, per) = listed_as(carrier, motor_units);
    let reference = if matches!(carrier, Carrier::Motor { .. }) {
        per / 20.0
    } else {
        1.0
    };
    Ok(ChannelEntry {
        field,
        listed_as: listed,
        unit,
        logged_per_unit: rounded(per),
        reference_per_unit: rounded(reference),
        truth: channel_truth(flight, &waves, &truth, &logged),
    })
}

/// What is true about the PID error of `axis`, the setpoint less the
/// filtered gyro, whose tones are those of both.
fn pid_error_entry(
    flight: &FlightSpec,
    session: &FlightSession,
    axis: Axis,
) -> Result<ChannelEntry, PackError> {
    let samples = flight.samples(axis);
    let exact: Vec<f64> = samples
        .setpoint
        .iter()
        .zip(&samples.filtered)
        .map(|(s, g)| s - g)
        .collect();
    let mut waves: Vec<Wave> = carried_waves(flight, Carrier::Setpoint { axis })
        .cloned()
        .collect();
    if let crate::signal::Filtered::Separate { waves: more } = &flight.axis(axis).filtered {
        waves.extend(more.iter().cloned());
    }
    let field = format!("pidError[{}]", axis.index());
    let logged =
        values_of(session, &ChannelId::PidError { axis }).ok_or_else(|| missing(&field))?;
    Ok(ChannelEntry {
        field,
        listed_as: "PID error",
        unit: "deg/s",
        logged_per_unit: 1.0,
        reference_per_unit: 1.0,
        truth: channel_truth(flight, &waves, &exact, &logged),
    })
}

/// What is true about the PID sum of `axis`, the sum of its `terms`.
fn pid_sum_entry(
    flight: &FlightSpec,
    (exact, session): (&FlightSession, &FlightSession),
    axis: Axis,
    terms: &[Carrier],
) -> Result<ChannelEntry, PackError> {
    let mut sum = vec![0.0; flight.len()];
    let mut waves = Vec::new();
    for carrier in terms {
        let values = values_of(exact, &carrier.id()).unwrap_or_default();
        sum.iter_mut().zip(values).for_each(|(e, v)| *e += v);
        waves.extend(carried_waves(flight, *carrier).cloned());
    }
    let field = format!("pidSum[{}]", axis.index());
    let id = ChannelId::PidTerm {
        axis,
        term: PidTerm::Sum,
    };
    let logged = values_of(session, &id).ok_or_else(|| missing(&field))?;
    Ok(ChannelEntry {
        field,
        listed_as: "PIDsum",
        unit: "the firmware's units",
        logged_per_unit: 1.0,
        reference_per_unit: 1.0,
        truth: channel_truth(flight, &waves, &sum, &logged),
    })
}

/// What is true about the channels `flight` carries beyond the gyro, and
/// about the PID error and the PID sum of the axes they are on, against
/// what the written log `session` holds.
fn channel_entries(
    flight: &FlightSpec,
    session: &FlightSession,
) -> Result<Vec<ChannelEntry>, PackError> {
    if flight.carried.is_empty() {
        return Ok(Vec::new());
    }
    let exact = flight.build();
    let mut carriers: Vec<Carrier> = Vec::new();
    for c in &flight.carried {
        if !carriers.contains(&c.channel) {
            carriers.push(c.channel);
        }
    }
    let mut out = Vec::new();
    for carrier in &carriers {
        out.push(carried_entry(flight, (&exact, session), *carrier)?);
    }
    for axis in Axis::ALL {
        if carriers.contains(&Carrier::Setpoint { axis }) {
            out.push(pid_error_entry(flight, session, axis)?);
        }
        let terms: Vec<Carrier> = carriers
            .iter()
            .filter(|c| matches!(c, Carrier::Term { axis: a, .. } if *a == axis))
            .copied()
            .collect();
        if !terms.is_empty() {
            out.push(pid_sum_entry(flight, (&exact, session), axis, &terms)?);
        }
    }
    Ok(out)
}

/// One probe: its log, its manifest entry, `OpenPID`'s results and the
/// model's predictions.
fn build_probe(probe: &Probe, round: u32) -> Result<Built, PackError> {
    let id = probe.id;
    let flight = &probe.flight;
    let bytes = log_of(probe)?;
    let session = read_back(probe, &bytes)?;
    let epoch = epoch_of(flight);
    let mut axes = Vec::new();
    let mut step = Vec::new();
    let mut ours = Vec::new();
    for (axis, purpose) in Axis::ALL.into_iter().zip(probe.purpose) {
        let spec = flight.axis(axis);
        let AxisSamples {
            unfiltered,
            filtered,
            ..
        } = flight.samples(axis);
        let filtered_waves = match &spec.filtered {
            crate::signal::Filtered::Same => &spec.waves,
            crate::signal::Filtered::Separate { waves } => waves,
        };
        axes.push(AxisEntry {
            axis: axis_name(axis),
            purpose,
            unfiltered: channel_truth(
                flight,
                &spec.waves,
                &unfiltered,
                &logged(&session, axis, GyroStage::Raw, id)?,
            ),
            filtered: channel_truth(
                flight,
                filtered_waves,
                &filtered,
                &logged(&session, axis, GyroStage::Filtered, id)?,
            ),
            response: (round >= TRUTH_FROM_ROUND)
                .then(|| ResponseTruth::of(spec.plant))
                .flatten(),
            inputs: stick_paths(flight, axis),
        });
        let mut spectra = spectra(&session, axis, epoch, id)?;
        if round > SPECTRA_ONLY_UP_TO && probe.area == Area::Spectrum && !flight.carried.is_empty()
        {
            spectra.extend(listed_spectra(&session, axis, epoch, id)?);
        }
        ours.push(OursAxis {
            axis: axis_name(axis),
            spectra,
        });
        if round > SPECTRA_ONLY_UP_TO && probe.area == Area::StepResponse {
            let window_s = StepParams::default().window_s;
            step.push(ours_step(&session, axis, epoch, window_s, id)?);
        }
    }
    let entry = ProbeEntry {
        id,
        file: format!("{id}.bbl"),
        bytes: bytes.len(),
        fnv1a64: format!("{:016x}", fnv1a64(&bytes)),
        area: probe.area,
        title: probe.title,
        fs: flight.fs,
        duration_s: flight.duration_s,
        samples: flight.len(),
        axes,
        channels: channel_entries(flight, &session)?,
        flight: flight.clone(),
    };
    let ours = OursProbe {
        id,
        epoch_s: (epoch.start_s, epoch.end_s),
        axes: ours,
        step,
    };
    Ok(Built {
        entry,
        ours,
        predicted: (probe.area == Area::Spectrum)
            .then(|| predicted(&session, flight.fs, id))
            .transpose()?,
        predicted_step: (probe.area == Area::StepResponse)
            .then(|| predicted_step(&session, probe))
            .transpose()?,
        bytes,
    })
}

/// Builds the pack of `round`. `built_from` names the commit, for the
/// manifest.
///
/// # Errors
/// [`PackError`] when the round does not exist or a probe cannot be written
/// and read back whole.
pub fn build(round: u32, built_from: &str) -> Result<Pack, PackError> {
    let probes = catalogue::round(round).ok_or(PackError::UnknownRound(round))?;
    let mut manifest = Manifest {
        format: FORMAT,
        round,
        built_from: built_from.to_string(),
        probes: Vec::new(),
        filters: capture::filter_set(round),
    };
    let mut ours = Ours {
        format: FORMAT,
        round,
        probes: Vec::new(),
    };
    let mut logs = Vec::new();
    let mut expected = Vec::new();
    let mut expected_step = Vec::new();
    for probe in &probes {
        let built = build_probe(probe, round)?;
        logs.push((built.entry.file.clone(), built.bytes));
        manifest.probes.push(built.entry);
        ours.probes.push(built.ours);
        expected.extend(built.predicted);
        expected_step.extend(built.predicted_step);
    }
    // rounds 1 and 4 were run before there was a model of their window:
    // their packs stay as they were; from round 17 on a pack predicts both
    // windows
    let both = round > SPECTRA_ONLY_UP_TO && !(expected.is_empty() && expected_step.is_empty());
    let predictions = if both {
        Some(Predictions {
            format: FORMAT,
            round,
            assumes: ASSUMES.iter().chain(&STEP_ASSUMES).copied().collect(),
            probes: expected,
            step: expected_step,
        })
    } else if round >= 2 && !expected.is_empty() {
        Some(Predictions {
            format: FORMAT,
            round,
            assumes: ASSUMES.to_vec(),
            probes: expected,
            step: Vec::new(),
        })
    } else if round >= 5 && !expected_step.is_empty() {
        Some(Predictions {
            format: FORMAT,
            round,
            assumes: STEP_ASSUMES.to_vec(),
            probes: Vec::new(),
            step: expected_step,
        })
    } else {
        None
    };
    Ok(Pack {
        plan: capture::plan(round, &probes),
        manifest,
        ours,
        predictions,
        logs,
    })
}

/// A value as indented JSON with a final newline.
fn json(value: &impl Serialize) -> Vec<u8> {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text.into_bytes()
}

impl Pack {
    /// Every file of the pack: name and content, the logs first, then what
    /// is true and what `OpenPID` computes, then the capture script with its
    /// steps.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = self.logs.clone();
        out.push(("manifest.json".into(), json(&self.manifest)));
        out.push(("ours.json".into(), json(&self.ours)));
        if let Some(predictions) = &self.predictions {
            out.push(("predictions.json".into(), json(predictions)));
        }
        out.push(("steps.json".into(), json(&self.plan)));
        out.extend(capture::files_of(
            self.manifest.round,
            &self.plan,
            self.predictions.is_some(),
        ));
        out
    }

    /// Writes the pack into `dir`, which is created when it does not exist.
    ///
    /// # Errors
    /// [`PackError::Io`] with the file that could not be written.
    pub fn write_to(&self, dir: &Path) -> Result<(), PackError> {
        let io = |path: &Path, source| PackError::Io {
            path: path.display().to_string(),
            source,
        };
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        for (name, bytes) in self.files() {
            let path = dir.join(name);
            std::fs::write(&path, bytes).map_err(|e| io(&path, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_its_published_test_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn amplitude_of_a_component() {
        let fs = 1000.0;
        let x: Vec<f64> = (0..2000)
            .map(|i| 3.0 * (TAU * 50.0 * f64::from(i) / fs + 0.4).sin())
            .collect();
        assert!((amplitude_at(&x, 50.0, fs, 0) - 3.0).abs() < 1e-9);
        // a part of the signal, with the phase of its first sample
        assert!((amplitude_at(&x[500..1500], 50.0, fs, 500) - 3.0).abs() < 1e-9);
        assert!(amplitude_at(&x, 70.0, fs, 0) < 1e-9);
        assert_eq!(amplitude_at(&[], 50.0, fs, 0), 0.0);
        assert_eq!(rounded(1.234_567_89), 1.234_568);
    }

    #[test]
    fn ours_holds_the_step_response_from_the_round_after_sixteen_only() {
        // the shortest log of the step response window
        let probe = catalogue::ROUNDS
            .iter()
            .flat_map(|r| catalogue::round(*r).unwrap_or_default())
            .filter(|p| p.area == Area::StepResponse)
            .min_by_key(|p| p.flight.len())
            .unwrap_or_else(|| unreachable!());
        let later = build_probe(&probe, SPECTRA_ONLY_UP_TO + 1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(later.ours.step.len(), 3);
        for step in &later.ours.step {
            assert_eq!(step.window_s, 2.0);
            assert_eq!(step.time_ms.len(), step.mean.len());
            assert!(step.peak.is_finite() && step.peak > 0.5);
        }
        let text = String::from_utf8(json(&later.ours)).unwrap_or_default();
        assert!(text.contains("\"step\""));
        // the packs of rounds 1 to 16 keep their bytes: no such section
        let then = build_probe(&probe, 4).unwrap_or_else(|e| panic!("{e}"));
        assert!(then.ours.step.is_empty());
        let text = String::from_utf8(json(&then.ours)).unwrap_or_default();
        assert!(!text.contains("\"step\""));
    }

    #[test]
    fn a_carried_channel_is_listed_as_the_reference_tool_lists_it() {
        let roll = Axis::Roll;
        let term = |term: PidTerm| listed_as(Carrier::Term { axis: roll, term }, 18.0);
        assert_eq!(term(PidTerm::P).0, "Pterm");
        assert_eq!(term(PidTerm::D).0, "Dterm");
        assert_eq!(term(PidTerm::F).0, "Fterm");
        assert_eq!(term(PidTerm::I).0, "(not listed)");
        assert_eq!(
            listed_as(Carrier::RcCommand { axis: roll }, 18.0).0,
            "(not listed)"
        );
        assert_eq!(
            listed_as(Carrier::Setpoint { axis: roll }, 18.0),
            ("Set point", "deg/s", 1.0)
        );
        assert_eq!(listed_as(Carrier::Motor { idx: 1 }, 18.0).2, 18.0);
        assert_eq!(listed_as(Carrier::Debug { idx: 1 }, 18.0).0, "debug");
        // a flight that carries nothing has no channels and no inputs
        let quiet = FlightSpec::quiet(1000.0, 1.0);
        assert!(stick_paths(&quiet, roll).is_none());
        let session = quiet.build();
        assert!(
            channel_entries(&quiet, &session)
                .unwrap_or_default()
                .is_empty()
        );
        assert_eq!(
            missing("x").to_string(),
            "probe carried channel: the written log has no complete x"
        );
    }
}
