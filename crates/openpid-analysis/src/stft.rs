//! Frequency × time map: the spectrum of one channel as a function of the
//! time, computed as the web edition 1.0 of the reference tool computes its
//! own (decisions D-006 and D-015; `docs/DESIGN.md` §6.5 states the rule and
//! what it was seen on).
//!
//! The stretch is cut into segments of 0.3 s, `L` samples, a
//! [`TimeSmoothing`] step apart. A column is one segment: its samples times
//! a symmetric Hann window, zero-padded to the next power of two of `L`
//! (1 024 points at 2 kHz), an FFT, and per bin the decibels of
//! `2·|X|² / (L·fs)` (the bins of 0 Hz and of half the rate without the 2),
//! with no compensation for the window. The decibels of a column are then
//! averaged along the frequency over as many rows as the [`FreqSmoothing`]
//! level says, fewer at the two ends of the axis.
//!
//! A cell is therefore a power density in decibels, **not** the amplitude of
//! a tone: a tone of amplitude `A` on a bin reads `20·log10(A) − 14.26` dB
//! before the smoothing.
//!
//! The first columns: as in the reference tool, the first `F` columns of
//! time smoothing factor `F` hold the segment that *starts* at their time,
//! every later one the segment centred at its time.

use crate::prepare::prepare;
use openpid_core::ChannelId;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_dsp::Fft;
use serde::{Deserialize, Serialize};

/// Length of a segment, seconds.
pub const SEGMENT_S: f64 = 0.3;
/// Colour scale of the map in decibels, as the reference tool draws it.
pub const DB_SCALE: (f64, f64) = (-30.0, 10.0);
/// Lowest value of a cell, dB: a cell below it after the smoothing is held
/// there, as the web edition draws a channel that is all zero (seen only on
/// such channels; the cells of a flight lie far above it).
pub const DB_FLOOR: f64 = -120.0;
/// Smallest argument of the logarithm of a bin before the smoothing (a bin
/// that is exactly 0 reads −300 dB, not −∞).
const LOG_FLOOR: f64 = 1e-30;

/// A list of four levels, weakest first, with the names a project file and
/// the command line use, and the factor of each level.
macro_rules! levels {
    (
        $(#[$doc:meta])* $name:ident, default $default:ident {
            $low:literal = $f_low:literal,
            $medium:literal = $f_medium:literal,
            $high:literal = $f_high:literal,
            $max:literal = $f_max:literal $(,)?
        }
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name {
            #[doc = $low]
            Low,
            #[doc = $medium]
            Medium,
            #[doc = $high]
            High,
            #[doc = $max]
            Max,
        }

        impl Default for $name {
            fn default() -> Self {
                $name::$default
            }
        }

        impl $name {
            /// Every level, weakest first.
            pub const ALL: [$name; 4] = [$name::Low, $name::Medium, $name::High, $name::Max];

            /// The name of the level: `low`, `medium`, `high`, `max`.
            pub fn name(self) -> &'static str {
                match self {
                    $name::Low => "low",
                    $name::Medium => "medium",
                    $name::High => "high",
                    $name::Max => "max",
                }
            }

            /// A level by its name.
            pub fn parse(text: &str) -> Option<$name> {
                $name::ALL.into_iter().find(|level| level.name() == text)
            }

            /// The factor of the level in the reference tool's list.
            pub fn factor(self) -> usize {
                match self {
                    $name::Low => $f_low,
                    $name::Medium => $f_medium,
                    $name::High => $f_high,
                    $name::Max => $f_max,
                }
            }
        }
    };
}

levels! {
    /// How strongly a column is smoothed along the frequency ("Freq
    /// smoothing" of the reference tool): factor `F`, a moving average of the
    /// decibels over `2·floor(F/2) + 1` rows.
    FreqSmoothing, default Medium {
        "Factor 2: the mean of 3 rows." = 2,
        "Factor 5: the mean of 5 rows (the default)." = 5,
        "Factor 10: the mean of 11 rows." = 10,
        "Factor 20: the mean of 21 rows." = 20,
    }
}

levels! {
    /// How close the columns are along the time ("Time smoothing" of the
    /// reference tool): factor `F`, the segments `L/F` samples apart. Nothing
    /// is averaged along the time; a higher level only puts more, overlapping,
    /// segments on the axis.
    TimeSmoothing, default Max {
        "Factor 1: a segment apart (0.3 s); the segments do not overlap." = 1,
        "Factor 2: half a segment apart (0.15 s)." = 2,
        "Factor 5: a fifth of a segment apart (0.06 s)." = 5,
        "Factor 10: a tenth of a segment apart (0.03 s; the default)." = 10,
    }
}

impl FreqSmoothing {
    /// Rows the mean of a smoothed row is taken over: `2·floor(F/2) + 1`,
    /// that is 3, 5, 11 and 21.
    pub fn rows(self) -> usize {
        2 * (self.factor() / 2) + 1
    }
}

impl TimeSmoothing {
    /// Distance between two segments of `nperseg` samples, in samples:
    /// `nperseg / F` rounded to a whole sample, at least 1.
    pub fn hop(self, nperseg: usize) -> usize {
        let hop = nperseg as f64 / self.factor() as f64;
        (hop.round() as usize).max(1)
    }
}

/// Parameters of [`StftAnalysis`]. A project saved before the map followed
/// the reference tool carries other fields here (segment length, hop,
/// window); they are ignored on loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StftParams {
    /// Channel to analyse.
    pub channel: ChannelId,
    /// Smoothing of every column along the frequency.
    pub smooth_freq: FreqSmoothing,
    /// Distance between the columns along the time.
    pub smooth_time: TimeSmoothing,
    /// Upper frequency bound of the output (Hz); `None`: half the sample
    /// rate, as far as the map goes. The rows above it are left out after
    /// the smoothing, so the rows kept do not depend on it.
    pub max_hz: Option<f64>,
}

impl Default for StftParams {
    fn default() -> Self {
        StftParams {
            channel: ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            },
            smooth_freq: FreqSmoothing::default(),
            smooth_time: TimeSmoothing::default(),
            max_hz: None,
        }
    }
}

impl std::hash::Hash for StftParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.channel.hash(state);
        self.smooth_freq.hash(state);
        self.smooth_time.hash(state);
        self.max_hz.map(f64::to_bits).hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`StftAnalysis`].
pub struct StftOutput {
    /// Analysed channel.
    pub channel: ChannelId,
    /// Unit symbol of the channel.
    pub unit: String,
    /// Sample rate used.
    pub fs: f64,
    /// Segment length in samples.
    pub nperseg: usize,
    /// Length of the transform: the segment zero-padded to the next power
    /// of two.
    pub nfft: usize,
    /// Distance between two columns, samples.
    pub hop: usize,
    /// Smoothing level along the frequency.
    pub smooth_freq: FreqSmoothing,
    /// Level of the distance between the columns.
    pub smooth_time: TimeSmoothing,
    /// x = time of each column (s from session start), y = frequency (Hz),
    /// z = `10·log10(2·|X|² / (L·fs))` smoothed along the frequency:
    /// decibels of the channel's unit squared per Hz.
    pub grid: Grid2D,
}

/// Length of a segment at the sample rate `fs`, in samples:
/// `round(0.3 · fs)`, at least 2.
pub fn segment_len(fs: f64) -> usize {
    ((SEGMENT_S * fs).round() as usize).max(2)
}

/// Length of the transform of a segment of `nperseg` samples: the next
/// power of two (1 024 for 600).
pub fn transform_len(nperseg: usize) -> usize {
    nperseg.next_power_of_two()
}

/// The number of columns of a stretch of `n` samples,
/// `floor((n − L) / hop) + 1`: the columns `k` whose time `k·hop` leaves a
/// whole segment before the end. `None` when not even one segment fits.
pub fn column_count(n: usize, nperseg: usize, hop: usize) -> Option<usize> {
    Some(n.checked_sub(nperseg)? / hop.max(1) + 1)
}

/// The first sample of the segment of column `k`, whose time is sample
/// `k·hop` of the stretch: the first `F` columns of `level` hold the segment
/// that starts there, the later ones the segment centred there
/// (`k·hop − L/2`, `L/2` rounded down).
pub fn segment_start(k: usize, nperseg: usize, level: TimeSmoothing) -> usize {
    let at = k * level.hop(nperseg);
    if k < level.factor() {
        at
    } else {
        at.saturating_sub(nperseg / 2)
    }
}

/// Symmetric Hann window of `n` samples: `0.5 − 0.5·cos(2π·i / (n − 1))`.
fn hann_symmetric(n: usize) -> Vec<f64> {
    let last = (n.max(2) - 1) as f64;
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / last).cos())
        .collect()
}

/// `values` (one per row) averaged over `rows` rows centred on each: row
/// `i` is the mean of the rows `i − rows/2 … i + rows/2` that exist, so
/// fewer at the two ends. One row (or none) leaves the values as they are.
pub fn moving_average(values: &[f64], rows: usize) -> Vec<f64> {
    let half = rows / 2;
    (0..values.len())
        .map(|i| {
            let read = &values[i.saturating_sub(half)..(i + half + 1).min(values.len())];
            read.iter().sum::<f64>() / read.len() as f64
        })
        .collect()
}

/// What every column of one map is computed with.
struct Columns<'a> {
    x: &'a [f64],
    nperseg: usize,
    window: Vec<f64>,
    fft: Fft,
    /// `L · fs`.
    scale: f64,
    rows: usize,
    buf: Vec<f64>,
}

impl Columns<'_> {
    /// The decibels of the segment that starts at sample `start`, before the
    /// smoothing: bins `0 … nfft/2`.
    fn decibels(&mut self, start: usize) -> Vec<f64> {
        let samples = &self.x[start..start + self.nperseg];
        for (b, (v, w)) in self.buf.iter_mut().zip(samples.iter().zip(&self.window)) {
            *b = v * w;
        }
        let spec = self.fft.rfft(&self.buf);
        let last = spec.len() - 1;
        spec.iter()
            .enumerate()
            .map(|(k, c)| {
                // one-sided: every bin but those of 0 Hz and half the rate
                // counts twice
                let sides = if k == 0 || k == last { 1.0 } else { 2.0 };
                10.0 * (sides * c.norm_sqr() / self.scale).max(LOG_FLOOR).log10()
            })
            .collect()
    }

    /// The column of the segment that starts at `start`: its decibels
    /// averaged along the frequency, none below [`DB_FLOOR`].
    fn column(&mut self, start: usize) -> Vec<f64> {
        let mut column = moving_average(&self.decibels(start), self.rows);
        for v in &mut column {
            *v = v.max(DB_FLOOR);
        }
        column
    }
}

#[derive(Debug, Default, Clone, Copy)]
/// Frequency × time map of one channel, in decibels of a power density.
pub struct StftAnalysis;

impl Analysis for StftAnalysis {
    type Params = StftParams;
    type Output = StftOutput;

    fn id(&self) -> &'static str {
        "stft"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::Channel(ChannelId::Time)]
    }

    fn run(&self, ctx: &AnalysisCtx<'_>, p: &StftParams) -> Result<StftOutput, AnalysisError> {
        let prep = prepare(ctx, &p.channel, None)?;
        let fs = prep.fs;
        let nperseg = segment_len(fs);
        let nfft = transform_len(nperseg);
        let hop = p.smooth_time.hop(nperseg);
        let x = prep.filled();
        let Some(nt) = column_count(x.len(), nperseg, hop) else {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, a segment needs {nperseg}",
                x.len()
            )));
        };
        let bins = nfft / 2 + 1;
        let freq: Vec<f64> = (0..bins).map(|k| k as f64 * fs / nfft as f64).collect();
        let nf = p
            .max_hz
            .map_or(bins, |max| freq.partition_point(|f| *f <= max));
        let mut columns = Columns {
            x: &x,
            nperseg,
            window: hann_symmetric(nperseg),
            fft: Fft::new(),
            scale: nperseg as f64 * fs,
            rows: p.smooth_freq.rows(),
            buf: vec![0.0; nfft],
        };
        let mut z = vec![0.0; nf * nt];
        for k in 0..nt {
            let column = columns.column(segment_start(k, nperseg, p.smooth_time));
            for (fi, v) in column.iter().take(nf).enumerate() {
                z[fi * nt + k] = *v;
            }
        }
        let grid = Grid2D {
            x: (0..nt).map(|k| (hop * k) as f64 / fs + prep.t0_s).collect(),
            y: freq[..nf].to_vec(),
            z,
            x_label: "time (s)".into(),
            y_label: "frequency (Hz)".into(),
            z_label: format!("power density (dB, ({})²/Hz)", prep.unit),
        };
        Ok(StftOutput {
            channel: p.channel.clone(),
            unit: prep.unit.to_string(),
            fs,
            nperseg,
            nfft,
            hop,
            smooth_freq: p.smooth_freq,
            smooth_time: p.smooth_time,
            grid,
        })
    }
}
