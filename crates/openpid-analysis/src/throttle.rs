//! Frequency × throttle map: the spectrum of a channel as a function of the
//! throttle position, computed as the reference tool computes its own
//! (decision D-005; `docs/DESIGN.md` §6.4 states the rule and what it was
//! held to).
//!
//! The epoch is cut into overlapping segments of 0.3 s. Every segment gets
//! the spectrum of its Hann-windowed samples, `|X| / L` with no compensation
//! for the window, and the mean of its throttle. Column `c` of 100 holds the
//! segments with `c − 1 < throttle ≤ c + 1` and is their plain mean; the
//! matrix is then smoothed with a Gaussian kernel whose size follows the
//! [`Smoothing`] level. A second matrix holds the same in decibels of a
//! power density. [`ThrottleHeatmapOutput::mean`] and
//! [`ThrottleHeatmapOutput::peak`] are the two numbers printed on a map.
//!
//! A cell is therefore **not** the amplitude of a tone: a tone of amplitude
//! `A` on a bin reads `A / 4` before the smoothing and about `0.035 · A`
//! after the default one.
//!
//! When the log carries motor rpm, the output also holds an [`RpmOverlay`]:
//! the median rpm of every motor per column (the same segments), so a
//! front-end can draw the motor harmonics `k · rpm / 60` over the map,
//! together with the static notches and the dynamic-notch band from the
//! settings.

use crate::prepare::Prepared;
use crate::prepare::prepare;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_core::{ChannelId, Unit};
use openpid_dsp::Fft;
use serde::{Deserialize, Serialize};

/// Length of a segment, seconds.
pub const SEGMENT_S: f64 = 0.3;
/// Distance between the starts of two segments on a stretch shorter than
/// [`HOP_MEDIUM_FROM_S`], seconds.
pub const HOP_SHORT_S: f64 = 0.06;
/// Distance between segments from [`HOP_MEDIUM_FROM_S`] to
/// [`HOP_LONG_FROM_S`], seconds.
pub const HOP_MEDIUM_S: f64 = 0.1;
/// Distance between segments on a stretch of [`HOP_LONG_FROM_S`] or longer,
/// seconds.
pub const HOP_LONG_S: f64 = 0.3;
/// Length of the stretch from which the segments are [`HOP_MEDIUM_S`] apart,
/// seconds. The reference tool was seen with 0.06 s at 10, 14.5 and 19 s and
/// with 0.1 s at 21.25 s and longer, so the threshold lies above 19 s and at
/// or below 21.25 s; 20 is the round number inside that bracket.
pub const HOP_MEDIUM_FROM_S: f64 = 20.0;
/// Length of the stretch from which the segments are [`HOP_LONG_S`] apart,
/// seconds. The reference tool was seen with 0.1 s up to 57.25 s and with
/// 0.3 s at 64 s and longer; 60 is the round number inside that bracket.
pub const HOP_LONG_FROM_S: f64 = 60.0;
/// Percent of a segment's samples that must be valid for it to be used.
pub const MIN_VALID_PERCENT: usize = 90;
/// Number of throttle columns; column `c` (1-based) is drawn from `c − 1` to
/// `c` percent.
pub const COLUMNS: usize = 100;
/// Standard deviation of the smoothing kernel, in cells along either axis.
pub const KERNEL_SIGMA: f64 = 4.0;
/// Value of the decibel matrix where there is nothing: a column without
/// segments, and everything outside the matrix for the smoothing.
pub const DB_FLOOR: f64 = -40.0;
/// Colour scale of the linear matrix, as the reference tool draws it.
pub const LINEAR_SCALE: (f64, f64) = (0.0, 0.5);
/// Colour scale of the decibel matrix, as the reference tool draws it.
pub const DB_SCALE: (f64, f64) = (DB_FLOOR, 10.0);
/// Smallest argument of the logarithm of a decibel (a bin that is exactly 0).
const LOG_FLOOR: f64 = 1e-300;

/// How strongly the matrix is smoothed: level `n` of 1 to 5 convolves it with
/// a Gaussian kernel of `5·n` taps along the throttle and `n` taps along the
/// frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Smoothing {
    /// Level 1: 5 × 1 taps.
    Low,
    /// Level 2: 10 × 2 taps.
    LowMed,
    /// Level 3: 15 × 3 taps (the default).
    #[default]
    Medium,
    /// Level 4: 20 × 4 taps.
    MedHigh,
    /// Level 5: 25 × 5 taps.
    High,
}

impl Smoothing {
    /// Every level, weakest first.
    pub const ALL: [Smoothing; 5] = [
        Smoothing::Low,
        Smoothing::LowMed,
        Smoothing::Medium,
        Smoothing::MedHigh,
        Smoothing::High,
    ];

    /// The level as a number, 1 to 5.
    pub fn level(self) -> usize {
        match self {
            Smoothing::Low => 1,
            Smoothing::LowMed => 2,
            Smoothing::Medium => 3,
            Smoothing::MedHigh => 4,
            Smoothing::High => 5,
        }
    }

    /// The level of a number 1 to 5.
    pub fn from_level(level: usize) -> Option<Smoothing> {
        Smoothing::ALL.get(level.checked_sub(1)?).copied()
    }

    /// The name of the level: `low`, `low-med`, `medium`, `med-high`, `high`.
    pub fn name(self) -> &'static str {
        match self {
            Smoothing::Low => "low",
            Smoothing::LowMed => "low-med",
            Smoothing::Medium => "medium",
            Smoothing::MedHigh => "med-high",
            Smoothing::High => "high",
        }
    }

    /// A level by its name or its number.
    pub fn parse(text: &str) -> Option<Smoothing> {
        Smoothing::ALL
            .into_iter()
            .find(|s| s.name() == text)
            .or_else(|| Smoothing::from_level(text.parse().ok()?))
    }

    /// Taps of the kernel along the throttle and along the frequency.
    pub fn kernel_size(self) -> (usize, usize) {
        (5 * self.level(), self.level())
    }
}

/// Parameters of [`ThrottleHeatmap`]. A project saved before the map followed
/// the reference tool carries other fields here (segment length, throttle
/// window, box smoothing, frequency limit); they are ignored on loading.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ThrottleHeatmapParams {
    /// Channel to analyse.
    pub channel: ChannelId,
    /// Smoothing level of the matrices.
    pub smoothing: Smoothing,
}

impl Default for ThrottleHeatmapParams {
    fn default() -> Self {
        ThrottleHeatmapParams {
            channel: ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            },
            smoothing: Smoothing::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Motor speed per throttle column and the notch configuration, for drawing
/// over the map.
pub struct RpmOverlay {
    /// Per motor (index order), the median over the segments of every
    /// throttle column of their mean rpm; `None` where no segment fell in
    /// the column.
    pub motors: Vec<Vec<Option<f64>>>,
    /// Harmonics the firmware's RPM filter removes (3 when it has none configured).
    pub harmonics: u32,
    /// Lowest frequency the RPM filter acts on, Hz.
    pub rpm_min_hz: Option<f64>,
    /// Static gyro notches as (centre, cutoff), Hz.
    pub notches: Vec<(f64, f64)>,
    /// Dynamic notch band (min, max), Hz.
    pub dyn_notch_hz: Option<(f64, f64)>,
}

impl RpmOverlay {
    /// Frequency of harmonic `k` (1-based) of motor `motor` in throttle
    /// column `bin` (0-based), Hz.
    pub fn harmonic_hz(&self, motor: usize, k: u32, bin: usize) -> Option<f64> {
        let rpm = (*self.motors.get(motor)?.get(bin)?)?;
        Some(rpm / 60.0 * f64::from(k))
    }
}

/// The matrices before the smoothing: per throttle column the mean of the
/// spectra of its segments. Row-major, `z[row · COLUMNS + column]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnMeans {
    /// Unit symbol of the channel.
    pub unit: String,
    /// Sample rate used, Hz.
    pub fs: f64,
    /// Segment length in samples.
    pub nperseg: usize,
    /// Distance between the starts of two segments, samples.
    pub hop: usize,
    /// Number of segment positions in the epoch (used or not).
    pub positions: usize,
    /// Number of segments used (enough valid samples).
    pub segments: usize,
    /// Number of frequency rows: bins `0 … nperseg / 2 − 1`.
    pub rows: usize,
    /// Number of segments in every column.
    pub counts: Vec<usize>,
    /// Mean of `|X| / L`; 0 in a column without segments.
    pub linear: Vec<f64>,
    /// Mean of `10·log10(2·|X|² / (L·fs))`; [`DB_FLOOR`] in a column without
    /// segments.
    pub db: Vec<f64>,
    /// Per motor and column, the median of the segments' mean rpm.
    pub motors: Vec<Vec<Option<f64>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`ThrottleHeatmap`].
pub struct ThrottleHeatmapOutput {
    /// Analysed channel.
    pub channel: ChannelId,
    /// Unit symbol of the channel.
    pub unit: String,
    /// Sample rate used.
    pub fs: f64,
    /// Segment length in samples.
    pub nperseg: usize,
    /// Distance between the starts of two segments, samples.
    pub hop: usize,
    /// Number of segments used.
    pub segments: usize,
    /// Smoothing level of both grids.
    pub smoothing: Smoothing,
    /// Mean of `grid` over every column and the rows from a tenth of the
    /// rows up (the lowest frequencies are left out).
    pub mean: f64,
    /// Maximum of `grid` over the same cells.
    pub peak: f64,
    /// Mean of `grid_db` over the same cells.
    pub mean_db: f64,
    /// Maximum of `grid_db` over the same cells.
    pub peak_db: f64,
    /// Number of segments in every throttle column.
    pub counts: Vec<usize>,
    /// x = throttle (%, the middle of each column), y = frequency (Hz),
    /// z = the smoothed mean of `|X| / L`.
    pub grid: Grid2D,
    /// The same in decibels: the smoothed mean of `10·log10(2·|X|² / (L·fs))`.
    pub grid_db: Grid2D,
    /// Motor speeds per throttle column, when the log has motor rpm.
    pub overlay: Option<RpmOverlay>,
}

impl ThrottleHeatmapOutput {
    /// The grid of a view: the decibel one or the linear one.
    pub fn view(&self, db: bool) -> &Grid2D {
        if db { &self.grid_db } else { &self.grid }
    }

    /// `(mean, peak)` of a view.
    pub fn mean_peak(&self, db: bool) -> (f64, f64) {
        if db {
            (self.mean_db, self.peak_db)
        } else {
            (self.mean, self.peak)
        }
    }
}

/// Distance between the starts of two segments for a stretch of
/// `duration_s` seconds, in seconds.
pub fn hop_seconds(duration_s: f64) -> f64 {
    if duration_s < HOP_MEDIUM_FROM_S {
        HOP_SHORT_S
    } else if duration_s < HOP_LONG_FROM_S {
        HOP_MEDIUM_S
    } else {
        HOP_LONG_S
    }
}

/// Whether a segment of mean throttle `throttle` belongs to column `column`
/// (1-based): `column − 1 < throttle ≤ column + 1`.
pub fn in_column(throttle: f64, column: usize) -> bool {
    let c = column as f64;
    c - 1.0 < throttle && throttle <= c + 1.0
}

/// Symmetric Hann window of `n` samples: `0.5 − 0.5·cos(2π·i / (n − 1))`.
fn hann_symmetric(n: usize) -> Vec<f64> {
    let last = (n.max(2) - 1) as f64;
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / last).cos())
        .collect()
}

/// The middle value of `v` (the upper one of an even number).
fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// The matrices of `channel` over the context's epoch before the smoothing.
pub fn column_means(
    ctx: &AnalysisCtx<'_>,
    channel: &ChannelId,
) -> Result<ColumnMeans, AnalysisError> {
    let sig = prepare(ctx, channel, None)?;
    let thr = prepare(ctx, &ChannelId::Throttle, Some(&Unit::Percent))?;
    let motors: Vec<Prepared> = (0..16u8)
        .map_while(|idx| prepare(ctx, &ChannelId::MotorRpm { idx }, None).ok())
        .collect();
    let fs = sig.fs;
    let nperseg = ((SEGMENT_S * fs).round() as usize).max(4);
    let rows = nperseg / 2;
    let x = sig.filled();
    let t = thr.filled();
    let n = x.len().min(t.len());
    if n < nperseg {
        return Err(AnalysisError::NotEnoughData(format!(
            "{n} samples, a segment needs {nperseg}"
        )));
    }
    let hop = ((hop_seconds(n as f64 / fs) * fs).round() as usize).max(1);
    // the last position that would still fit is not taken
    let positions = (n - nperseg) / hop;
    let window = hann_symmetric(nperseg);
    let fft = Fft::new();
    let len = nperseg as f64;
    let mut linear = vec![0.0; COLUMNS * rows];
    let mut db = vec![0.0; COLUMNS * rows];
    let mut counts = vec![0usize; COLUMNS];
    // per used segment: its mean throttle and the mean rpm of every motor
    let mut throttles: Vec<f64> = Vec::new();
    let mut rpms: Vec<Vec<Option<f64>>> = vec![Vec::new(); motors.len()];
    let mut buf = vec![0.0; nperseg];
    let mut amp = vec![0.0; rows];
    let mut dens = vec![0.0; rows];
    for k in 0..positions {
        let start = k * hop;
        let end = start + nperseg;
        let valid = sig.data.valid[start..end].iter().filter(|v| **v).count();
        if valid * 100 < nperseg * MIN_VALID_PERCENT {
            continue;
        }
        for (b, (v, w)) in buf.iter_mut().zip(x[start..end].iter().zip(&window)) {
            *b = v * w;
        }
        let spec = fft.rfft(&buf);
        for (b, c) in spec.iter().take(rows).enumerate() {
            let power = c.norm_sqr();
            amp[b] = power.sqrt() / len;
            dens[b] = 10.0 * (2.0 * power / (len * fs)).max(LOG_FLOOR).log10();
        }
        let throttle = t[start..end].iter().sum::<f64>() / len;
        // the two columns around the throttle
        let lowest = (throttle.floor() - 1.0).max(1.0) as usize;
        for column in lowest..=(lowest + 3).min(COLUMNS) {
            if !in_column(throttle, column) {
                continue;
            }
            let ix = column - 1;
            counts[ix] += 1;
            for b in 0..rows {
                linear[b * COLUMNS + ix] += amp[b];
                db[b * COLUMNS + ix] += dens[b];
            }
        }
        throttles.push(throttle);
        for (m, motor) in motors.iter().enumerate() {
            let (sum, count) = (start..end.min(motor.len()))
                .filter(|&i| motor.data.valid[i])
                .fold((0.0, 0usize), |(s, c), i| (s + motor.data.values[i], c + 1));
            rpms[m].push((count * 2 >= nperseg).then(|| sum / count as f64));
        }
    }
    if throttles.is_empty() {
        return Err(AnalysisError::NotEnoughData(
            "no complete segment in the epoch".into(),
        ));
    }
    for (ix, count) in counts.iter().enumerate() {
        for b in 0..rows {
            let at = b * COLUMNS + ix;
            if *count > 0 {
                linear[at] /= *count as f64;
                db[at] /= *count as f64;
            } else {
                db[at] = DB_FLOOR;
            }
        }
    }
    let motors = rpms
        .iter()
        .map(|per_segment| {
            (1..=COLUMNS)
                .map(|column| {
                    median(
                        per_segment
                            .iter()
                            .zip(&throttles)
                            .filter(|(_, th)| in_column(**th, column))
                            .filter_map(|(r, _)| *r)
                            .collect(),
                    )
                })
                .collect()
        })
        .collect();
    Ok(ColumnMeans {
        unit: sig.unit.to_string(),
        fs,
        nperseg,
        hop,
        positions,
        segments: throttles.len(),
        rows,
        counts,
        linear,
        db,
        motors,
    })
}

/// The taps of the smoothing kernel along one axis: a Gaussian of
/// [`KERNEL_SIGMA`] centred on `(size − 1) / 2`, summing to 1. The kernel of
/// the matrix is the product of the taps of its two axes.
pub fn kernel_taps(size: usize) -> Vec<f64> {
    let centre = (size as f64 - 1.0) / 2.0;
    let raw: Vec<f64> = (0..size)
        .map(|i| (-(i as f64 - centre).powi(2) / (2.0 * KERNEL_SIGMA * KERNEL_SIGMA)).exp())
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.into_iter().map(|v| v / sum).collect()
}

/// `z` (row-major, `nx` columns × `ny` rows) convolved with `taps` along the
/// columns (`along_x`) or along the rows, zeros outside:
/// `out[p] = Σ taps[t] · z[p + o − t]`, where `o` is `(size − 1) / 2`, and
/// one more for an even number of taps.
fn convolve_axis(z: &[f64], nx: usize, ny: usize, taps: &[f64], along_x: bool) -> Vec<f64> {
    let size = taps.len();
    let offset = (size - 1) / 2 + usize::from(size.is_multiple_of(2));
    let (len, stride, lines, line_stride) = if along_x {
        (nx, 1, ny, nx)
    } else {
        (ny, nx, nx, 1)
    };
    let mut out = vec![0.0; z.len()];
    for line in 0..lines {
        let base = line * line_stride;
        for p in 0..len {
            // q = p + offset − t inside 0..len
            let first = (p + offset + 1).saturating_sub(len);
            let last = (p + offset).min(size - 1);
            let mut sum = 0.0;
            for (t, tap) in taps.iter().enumerate().take(last + 1).skip(first) {
                sum += tap * z[base + (p + offset - t) * stride];
            }
            out[base + p * stride] = sum;
        }
    }
    out
}

/// `z` (row-major, `nx` throttle columns × `ny` frequency rows) smoothed at
/// `level`, with `outside` as the value beyond the edges of the matrix.
///
/// # Panics
/// Panics if `z.len() != nx * ny`.
pub fn smooth(z: &[f64], nx: usize, ny: usize, level: Smoothing, outside: f64) -> Vec<f64> {
    assert_eq!(z.len(), nx * ny, "matrix size");
    let (along_throttle, along_frequency) = level.kernel_size();
    let shifted: Vec<f64> = z.iter().map(|v| v - outside).collect();
    let once = convolve_axis(&shifted, nx, ny, &kernel_taps(along_throttle), true);
    convolve_axis(&once, nx, ny, &kernel_taps(along_frequency), false)
        .into_iter()
        .map(|v| v + outside)
        .collect()
}

/// The first frequency row that counts for the printed mean and peak:
/// `rows / 10 − 1`.
pub fn first_counted_row(rows: usize) -> usize {
    (rows / 10).saturating_sub(1)
}

/// `(mean, peak)` of a matrix (row-major, `nx` columns) over every column
/// and the rows from [`first_counted_row`] up.
pub fn mean_peak(z: &[f64], nx: usize) -> (f64, f64) {
    let rows = z.len().checked_div(nx).unwrap_or(0);
    let cells = &z[(first_counted_row(rows) * nx).min(z.len())..];
    if cells.is_empty() {
        return (0.0, 0.0);
    }
    (
        cells.iter().sum::<f64>() / cells.len() as f64,
        cells.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

#[derive(Debug, Default, Clone, Copy)]
/// Frequency × throttle map of one channel.
pub struct ThrottleHeatmap;

impl Analysis for ThrottleHeatmap {
    type Params = ThrottleHeatmapParams;
    type Output = ThrottleHeatmapOutput;

    fn id(&self) -> &'static str {
        "throttle-heatmap"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::Channel(ChannelId::Throttle)]
    }

    fn run(
        &self,
        ctx: &AnalysisCtx<'_>,
        p: &ThrottleHeatmapParams,
    ) -> Result<ThrottleHeatmapOutput, AnalysisError> {
        let columns = column_means(ctx, &p.channel)?;
        let rows = columns.rows;
        let linear = smooth(&columns.linear, COLUMNS, rows, p.smoothing, 0.0);
        let db = smooth(&columns.db, COLUMNS, rows, p.smoothing, DB_FLOOR);
        let (mean, peak) = mean_peak(&linear, COLUMNS);
        let (mean_db, peak_db) = mean_peak(&db, COLUMNS);
        let x: Vec<f64> = (0..COLUMNS).map(|c| c as f64 + 0.5).collect();
        let y: Vec<f64> = (0..rows)
            .map(|b| b as f64 * columns.fs / columns.nperseg as f64)
            .collect();
        let grid = |z: Vec<f64>, z_label: String| Grid2D {
            x: x.clone(),
            y: y.clone(),
            z,
            x_label: "throttle (%)".into(),
            y_label: "frequency (Hz)".into(),
            z_label,
        };
        let overlay = (!columns.motors.is_empty()).then(|| {
            let chain = &ctx.session.settings.gyro_filters;
            RpmOverlay {
                motors: columns.motors.clone(),
                harmonics: chain.rpm_notch.as_ref().map_or(3, |r| r.harmonics.max(1)),
                rpm_min_hz: chain.rpm_notch.as_ref().map(|r| r.min_hz),
                notches: chain
                    .notches
                    .iter()
                    .filter(|n| n.center_hz > 0.0)
                    .map(|n| (n.center_hz, n.cutoff_hz))
                    .collect(),
                dyn_notch_hz: chain
                    .dyn_notch
                    .filter(|d| d.max_hz > d.min_hz)
                    .map(|d| (d.min_hz, d.max_hz)),
            }
        });
        Ok(ThrottleHeatmapOutput {
            channel: p.channel.clone(),
            fs: columns.fs,
            nperseg: columns.nperseg,
            hop: columns.hop,
            segments: columns.segments,
            smoothing: p.smoothing,
            mean,
            peak,
            mean_db,
            peak_db,
            counts: columns.counts,
            grid: grid(linear, format!("|X| / L ({})", columns.unit)),
            grid_db: grid(db, "10·log10(2·|X|² / (L·fs)) (dB)".into()),
            unit: columns.unit,
            overlay,
        })
    }
}
