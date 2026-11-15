//! Frequency × time map: the spectrum of one channel as a function of the
//! time, computed as the reference tool computes its own (decision D-006;
//! `docs/DESIGN.md` §6.5 states the rule, the one exception and what the
//! rule was seen on).
//!
//! The epoch is cut into segments of 0.3 s whose centres are one
//! [`TimeSmoothing`] step apart. A column is one segment: its samples times
//! a symmetric Hann window, an FFT, and per bin the decibels of
//! `2·|X|² / (L·fs)` (the bin of 0 Hz without the 2), with no compensation
//! for the window. The decibels of a column are then smoothed along the
//! frequency by a line fitted to the nearest rows, as many as the
//! [`FreqSmoothing`] level says.
//!
//! A cell is therefore a power density in decibels, **not** the amplitude of
//! a tone: a tone of amplitude `A` on a bin reads `20·log10(A) − 14.26` dB
//! before the smoothing and `20·log10(A) − 17.71` dB after the default one.
//!
//! The exception: the reference tool draws a few of its first columns out of
//! place. Here every column sits at the middle of its segment.

use crate::prepare::prepare;
use openpid_core::ChannelId;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_dsp::Fft;
use serde::{Deserialize, Serialize};

/// Length of a segment, seconds.
pub const SEGMENT_S: f64 = 0.3;
/// Rows a smoothed row is fitted to at [`FreqSmoothing::Low`].
pub const SPAN_LOW: usize = 5;
/// Rows a smoothed row is fitted to at [`FreqSmoothing::Medium`].
pub const SPAN_MEDIUM: usize = 11;
/// Rows a smoothed row is fitted to at [`FreqSmoothing::High`].
pub const SPAN_HIGH: usize = 21;
/// Colour scale of the map in decibels, as the reference tool draws it.
pub const DB_SCALE: (f64, f64) = (-30.0, 10.0);
/// Smallest argument of the logarithm of a decibel (a bin that is exactly
/// 0 reads −300 dB, not −∞).
const LOG_FLOOR: f64 = 1e-30;

/// A list of four levels, weakest first, with the names a project file and
/// the command line use.
macro_rules! levels {
    ($(#[$doc:meta])* $name:ident { $off:literal, $low:literal, $medium:literal, $high:literal }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name {
            #[doc = $off]
            Off,
            #[doc = $low]
            #[default]
            Low,
            #[doc = $medium]
            Medium,
            #[doc = $high]
            High,
        }

        impl $name {
            /// Every level, weakest first.
            pub const ALL: [$name; 4] = [$name::Off, $name::Low, $name::Medium, $name::High];

            /// The name of the level: `off`, `low`, `medium`, `high`.
            pub fn name(self) -> &'static str {
                match self {
                    $name::Off => "off",
                    $name::Low => "low",
                    $name::Medium => "medium",
                    $name::High => "high",
                }
            }

            /// A level by its name; `med` is `medium`, as the reference tool
            /// writes it.
            pub fn parse(text: &str) -> Option<$name> {
                let text = if text == "med" { "medium" } else { text };
                $name::ALL.into_iter().find(|level| level.name() == text)
            }
        }
    };
}

levels! {
    /// How strongly a column is smoothed along the frequency ("smooth freq
    /// axis" of the reference tool): the number of rows the line of a
    /// smoothed row is fitted to.
    FreqSmoothing {
        "No smoothing: a row is its own bin.",
        "A line through 5 rows (the default).",
        "A line through 11 rows.",
        "A line through 21 rows."
    }
}

levels! {
    /// How close the columns are along the time ("smooth time axis" of the
    /// reference tool): the distance between the centres of two segments, as
    /// a part of the segment. Nothing is averaged along the time; a higher
    /// level only puts more, overlapping, segments on the axis.
    TimeSmoothing {
        "A segment apart (0.3 s): the segments do not overlap.",
        "Half a segment apart (0.15 s; the default).",
        "A fifth of a segment apart (0.06 s).",
        "A tenth of a segment apart (0.03 s)."
    }
}

impl FreqSmoothing {
    /// Rows the line of a smoothed row is fitted to; 1 for no smoothing.
    pub fn span(self) -> usize {
        match self {
            FreqSmoothing::Off => 1,
            FreqSmoothing::Low => SPAN_LOW,
            FreqSmoothing::Medium => SPAN_MEDIUM,
            FreqSmoothing::High => SPAN_HIGH,
        }
    }
}

impl TimeSmoothing {
    /// How many columns a segment is long.
    pub fn columns_per_segment(self) -> usize {
        match self {
            TimeSmoothing::Off => 1,
            TimeSmoothing::Low => 2,
            TimeSmoothing::Medium => 5,
            TimeSmoothing::High => 10,
        }
    }

    /// Distance between the centres of two segments of `nperseg` samples, in
    /// samples: the part of the segment rounded to a whole sample, at least 1.
    pub fn hop(self, nperseg: usize) -> usize {
        let hop = nperseg as f64 / self.columns_per_segment() as f64;
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
    /// Distance between the centres of two segments, samples.
    pub hop: usize,
    /// Smoothing level along the frequency.
    pub smooth_freq: FreqSmoothing,
    /// Level of the distance between the columns.
    pub smooth_time: TimeSmoothing,
    /// x = time of the middle of each segment (s from session start),
    /// y = frequency (Hz), z = `10·log10(2·|X|² / (L·fs))` smoothed along
    /// the frequency: decibels of the channel's unit squared per Hz.
    pub grid: Grid2D,
}

/// Length of a segment at the sample rate `fs`, in samples:
/// `round(0.3 · fs)`, at least 2.
pub fn segment_len(fs: f64) -> usize {
    ((SEGMENT_S * fs).round() as usize).max(2)
}

/// The columns of a stretch of `n` samples: the first and the last `m` for
/// which the segment of `nperseg` samples centred on sample `hop · m` is
/// taken, `[hop·m − nperseg/2, hop·m − nperseg/2 + nperseg)`. The first is
/// the lowest whose segment starts inside the stretch; the last is
/// `floor((n − 1 − nperseg) / hop)`, a little before the end of the stretch,
/// as the reference tool has it. `None` when no column fits.
pub fn columns(n: usize, nperseg: usize, hop: usize) -> Option<(usize, usize)> {
    let hop = hop.max(1);
    let first = (nperseg / 2).div_ceil(hop);
    let last = n.checked_sub(1 + nperseg)? / hop;
    (first <= last).then_some((first, last))
}

/// Symmetric Hann window of `n` samples: `0.5 − 0.5·cos(2π·i / (n − 1))`.
fn hann_symmetric(n: usize) -> Vec<f64> {
    let last = (n.max(2) - 1) as f64;
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / last).cos())
        .collect()
}

/// What the smoothed value of one row is made of: the first row it reads
/// and the weight of that row and of those after it.
type RowWeights = (usize, Vec<f64>);

/// The weights that make the smoothed value of row `i` of `rows` at `span`:
/// the first row read and one weight per row from there, summing to 1.
///
/// The rows read are the `span` nearest: `i − span/2 … i + span/2`, shifted
/// to stay inside the rows (all of them when there are fewer than `span`).
/// Row `j` weighs `(1 − (d / d_max)³)³`, with `d = |j − i|` and `d_max` the
/// largest `d` among the rows read; a straight line is fitted to the values
/// by weighted least squares and read at `i`. Away from the two ends the
/// rows lie evenly around `i` and that is their weighted mean; at an end
/// they lie to one side and the line is not level.
///
/// # Panics
/// Panics if `i` is not below `rows`.
pub fn line_weights(rows: usize, i: usize, span: usize) -> RowWeights {
    assert!(i < rows, "row {i} of {rows}");
    let count = span.clamp(1, rows);
    let first = i.saturating_sub(span / 2).min(rows - count);
    let last = first + count - 1;
    let d_max = (i - first).max(last - i);
    if d_max == 0 {
        return (first, vec![1.0]);
    }
    // every row read: its distance from row `i` (signed) and its weight
    let read: Vec<(f64, f64)> = (first..=last)
        .map(|j| {
            let d = j as f64 - i as f64;
            (d, (1.0 - (d.abs() / d_max as f64).powi(3)).powi(3))
        })
        .collect();
    let (mut s0, mut s1, mut s2) = (0.0, 0.0, 0.0);
    for (d, w) in &read {
        s0 += w;
        s1 += w * d;
        s2 += w * d * d;
    }
    let det = s0 * s2 - s1 * s1;
    // one row with a weight (two rows in all): no line, the row itself
    let level = det <= 1e-12 * s0 * s0;
    let weights = read
        .iter()
        .map(|(d, w)| {
            if level {
                w / s0
            } else {
                // the intercept of the fitted line, the distances being from `i`
                w * (s2 - s1 * d) / det
            }
        })
        .collect();
    (first, weights)
}

/// `values` (one per row) smoothed with [`line_weights`] at `span`.
pub fn smooth_line(values: &[f64], span: usize) -> Vec<f64> {
    let kernel = Smoother::new(values.len(), span);
    let mut out = vec![0.0; values.len()];
    kernel.apply(values, &mut out);
    out
}

/// The weights of every row at one span, made once for all columns.
struct Smoother {
    /// `None` for no smoothing.
    rows: Option<Vec<RowWeights>>,
}

impl Smoother {
    fn new(rows: usize, span: usize) -> Smoother {
        Smoother {
            rows: (span > 1).then(|| (0..rows).map(|i| line_weights(rows, i, span)).collect()),
        }
    }

    /// `out` = `values` smoothed.
    fn apply(&self, values: &[f64], out: &mut [f64]) {
        let Some(rows) = &self.rows else {
            out.copy_from_slice(values);
            return;
        };
        for (o, (first, weights)) in out.iter_mut().zip(rows) {
            *o = weights
                .iter()
                .zip(&values[*first..])
                .map(|(w, v)| w * v)
                .sum();
        }
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
        let hop = p.smooth_time.hop(nperseg);
        let x = prep.filled();
        let Some((first, last)) = columns(x.len(), nperseg, hop) else {
            // the first column: its centre, half a segment and one sample
            // after its last one, as the rule of the last column has it
            let needed = hop * (nperseg / 2).div_ceil(hop) + nperseg + 1;
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, the first segment of {nperseg} needs {needed}",
                x.len()
            )));
        };
        let nt = last - first + 1;
        let bins = nperseg / 2 + 1;
        let freq: Vec<f64> = (0..bins).map(|k| k as f64 * fs / nperseg as f64).collect();
        let nf = p
            .max_hz
            .map_or(bins, |max| freq.partition_point(|f| *f <= max));
        let window = hann_symmetric(nperseg);
        let smoother = Smoother::new(bins, p.smooth_freq.span());
        let fft = Fft::new();
        let scale = nperseg as f64 * fs;
        let mut z = vec![0.0; nf * nt];
        let mut buf = vec![0.0; nperseg];
        let mut db = vec![0.0; bins];
        let mut column = vec![0.0; bins];
        for ti in 0..nt {
            let start = hop * (first + ti) - nperseg / 2;
            for (b, (v, w)) in buf
                .iter_mut()
                .zip(x[start..start + nperseg].iter().zip(&window))
            {
                *b = v * w;
            }
            let spec = fft.rfft(&buf);
            for (k, (d, c)) in db.iter_mut().zip(&spec).enumerate() {
                // one-sided: every bin but that of 0 Hz counts twice
                let sides = if k == 0 { 1.0 } else { 2.0 };
                *d = 10.0 * (sides * c.norm_sqr() / scale).max(LOG_FLOOR).log10();
            }
            smoother.apply(&db, &mut column);
            for (fi, v) in column.iter().take(nf).enumerate() {
                z[fi * nt + ti] = *v;
            }
        }
        let grid = Grid2D {
            x: (first..=last)
                .map(|m| (hop * m) as f64 / fs + prep.t0_s)
                .collect(),
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
            hop,
            smooth_freq: p.smooth_freq,
            smooth_time: p.smooth_time,
            grid,
        })
    }
}
