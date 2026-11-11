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
use crate::signal::{AxisSamples, FlightSpec, Wave, band_bins};
use openpid_analysis::{Analysis, AnalysisCtx, Epoch, PsdAnalysis, PsdParams};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage, LogFormat, OpenOptions};
use openpid_dsp::Fft;
use openpid_fmt_blackbox::BlackboxFormat;
use openpid_fmt_blackbox::write::{WriteError, WriteOptions, betaflight};
use serde::Serialize;
use std::f64::consts::TAU;
use std::path::Path;

/// Version of the layout of `manifest.json` and `ours.json`.
const FORMAT: u32 = 1;
/// A flight controller logs the time since power-up; the probes start half a
/// minute after it.
const TIME_OFFSET_US: i64 = 30_000_000;
/// Seconds left out at each end of a log before `OpenPID`'s own analysis, as
/// the reference tool leaves them out (`docs/DESIGN.md` §6.7).
const TRIM_S: f64 = 1.0;

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
}

/// One spectrum `OpenPID` computes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Spectrum {
    /// `gyro` or `gyro_unfiltered`.
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

/// What `OpenPID` computes for one probe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OursProbe {
    /// Id of the probe.
    pub id: &'static str,
    /// The part of the log analysed, seconds from its start.
    pub epoch_s: (f64, f64),
    /// The three axes.
    pub axes: Vec<OursAxis>,
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
    /// The probes.
    pub probes: Vec<PredictedProbe>,
}

/// The frequencies at which a prediction is written down, Hz (those below
/// half the log rate; the point nearest to each).
pub const PREDICTED_HZ: [f64; 18] = [
    0.0, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0,
    200.0, 400.0,
];

/// What the predictions of a round rest on beyond what is measured.
const ASSUMES: [&str; 3] = [
    "the smoothing spans are counted in points and are the same at every log rate and length",
    "the spans of low, low-med, med-high and high are 5, 35, 285 and 553 points (fitted to \
     pictures; medium, 121, is exact)",
    "the analysed span starts three samples after the first second and is one sample short of \
     the log without its first and last second, at every rate and length",
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
fn spectra(
    session: &FlightSession,
    axis: Axis,
    epoch: Epoch,
    probe: &'static str,
) -> Result<Vec<Spectrum>, PackError> {
    let ctx = AnalysisCtx {
        session,
        epoch: Some(epoch),
    };
    let mut out = Vec::new();
    for (stage, channel) in [
        (GyroStage::Filtered, "gyro"),
        (GyroStage::Raw, "gyro_unfiltered"),
    ] {
        // the two columns of the tab: the whole range, and below 100 Hz
        // with one-second segments
        for (segment_s, max_hz) in [(0.25, None), (1.0, Some(100.0))] {
            let params = PsdParams {
                channel: ChannelId::Gyro { axis, stage },
                segment_s,
                max_hz,
                ..PsdParams::default()
            };
            let psd = PsdAnalysis
                .run(&ctx, &params)
                .map_err(|e| PackError::Read {
                    probe,
                    message: format!("spectrum of {channel}: {e}"),
                })?;
            out.push(Spectrum {
                channel,
                segment_s,
                bin_hz: rounded(psd.fs / psd.nperseg as f64),
                segments: psd.segments,
                freq: psd.freq.iter().map(|f| rounded(*f)).collect(),
                power_db: psd.power_db.iter().map(|p| rounded(*p)).collect(),
            });
        }
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
    for (smoothing, window) in model::LEVELS {
        let mut axes = Vec::new();
        for (axis, values) in &channels {
            let curve = model::curve(values, fs, window).ok_or_else(too_short)?;
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

/// One probe as the pack holds it.
struct Built {
    /// Its entry in the manifest.
    entry: ProbeEntry,
    /// `OpenPID`'s results.
    ours: OursProbe,
    /// What the model of the reference tool expects.
    predicted: PredictedProbe,
    /// The log.
    bytes: Vec<u8>,
}

/// One probe: its log, its manifest entry, `OpenPID`'s results and the
/// model's predictions.
fn build_probe(probe: &Probe) -> Result<Built, PackError> {
    let id = probe.id;
    let flight = &probe.flight;
    let options = WriteOptions {
        time_offset_us: TIME_OFFSET_US,
        // the name a tool shows for the log, so a screenshot says which probe it is
        headers: vec![("Craft name".into(), id.into())],
        ..WriteOptions::default()
    };
    let bytes = betaflight(&flight.build(), &options)
        .map_err(|source| PackError::Write { probe: id, source })?;
    let read = |message: String| PackError::Read { probe: id, message };
    let mut log = BlackboxFormat::new()
        .open(&bytes, &OpenOptions::default())
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
    let epoch = Epoch {
        start_s: TRIM_S,
        end_s: flight.duration_s - TRIM_S,
    };
    let mut axes = Vec::new();
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
        });
        ours.push(OursAxis {
            axis: axis_name(axis),
            spectra: spectra(&session, axis, epoch, id)?,
        });
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
        flight: flight.clone(),
    };
    let ours = OursProbe {
        id,
        epoch_s: (epoch.start_s, epoch.end_s),
        axes: ours,
    };
    Ok(Built {
        entry,
        ours,
        predicted: predicted(&session, flight.fs, id)?,
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
    };
    let mut ours = Ours {
        format: FORMAT,
        round,
        probes: Vec::new(),
    };
    let mut logs = Vec::new();
    let mut expected = Vec::new();
    for probe in &probes {
        let built = build_probe(probe)?;
        logs.push((built.entry.file.clone(), built.bytes));
        manifest.probes.push(built.entry);
        ours.probes.push(built.ours);
        expected.push(built.predicted);
    }
    // round 1 was run before there was a model: its pack stays as it was
    let predictions = (round >= 2).then(|| Predictions {
        format: FORMAT,
        round,
        assumes: ASSUMES.to_vec(),
        probes: expected,
    });
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
        let count = |action: &str| {
            self.plan
                .steps
                .iter()
                .filter(|s| s.action == action)
                .count()
        };
        out.extend(capture::files(
            self.manifest.round,
            count("shot"),
            count("clip_save"),
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
}
