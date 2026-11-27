//! The spectrum of one channel: Welch's power spectral density, or the
//! smoothed curve of the whole log that the reference tool's web edition 1.0
//! draws (what the Spectrum tab shows by default: decisions D-001 and D-015).

use crate::prepare::{Prepared, prepare};
use openpid_core::ChannelId;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_dsp::spectrum::{WelchParams, welch};
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Parameters of [`PsdAnalysis`].
#[serde(default)]
pub struct PsdParams {
    /// Channel to analyse.
    pub channel: ChannelId,
    /// What is computed: the smoothed curve of the whole log, or the
    /// averaged power density.
    pub method: PsdMethod,
    /// Smoothing level of [`PsdMethod::Curve`]: 0 ("low") … 3 ("max"),
    /// see [`CURVE_LEVELS`]. Not used by [`PsdMethod::Density`].
    pub smoothing: usize,
    /// Segment length in seconds (rounded to a 5-smooth sample count).
    pub segment_s: f64,
    /// Overlap as a fraction of the segment (0..1).
    pub overlap: f64,
    /// Window applied to every segment.
    pub window: Window,
    /// Upper frequency bound of the output (Hz); `None` keeps everything up to Nyquist.
    pub max_hz: Option<f64>,
}

/// How a spectrum is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsdMethod {
    /// One Hann-windowed transform of the whole epoch, zero-padded to a
    /// power of two, in decibels, smoothed along the frequency by a moving
    /// average: the curve the reference tool's web edition 1.0 draws
    /// (decisions D-001 and D-015). The height of a peak follows its width
    /// as well as its strength: noise reads 6.8 dB below its density and a
    /// steady tone almost vanishes. The Spectrum tab and the command line
    /// draw this by default.
    Curve,
    /// Welch's method: the mean power of overlapping segments with the
    /// window's loss put back, a power density in `unit²/Hz`. Noise reads
    /// its density and a steady tone stands out. The default of the type,
    /// and what the other analyses (advice, motor noise) are built on.
    #[default]
    Density,
}

/// The smoothing levels of [`PsdMethod::Curve`], lowest first; the default
/// is "medium".
pub const CURVE_LEVELS: [&str; 4] = ["low", "medium", "high", "max"];

/// Points of the moving average of each of [`CURVE_LEVELS`].
const CURVE_POINTS: [usize; 4] = [51, 151, 301, 601];

fn default_smoothing() -> usize {
    1
}

/// Points of the moving average of the curve at smoothing level `level`
/// (0 … 3; a higher level counts as the highest): 51, 151, 301 or 601,
/// whatever the spacing of the points. The points are `fs / nfft` apart,
/// so the same level is a wider band in hertz for a shorter log.
pub fn curve_window(level: usize) -> usize {
    CURVE_POINTS[level.min(CURVE_POINTS.len() - 1)]
}

/// Length of the transform of the curve for `n` samples: the next power of
/// two at or above `n`.
pub fn curve_transform_len(n: usize) -> usize {
    n.next_power_of_two()
}

/// The transform of the whole of `x` through a symmetric Hann window,
/// zero-padded to [`curve_transform_len`] points, as a level in decibels,
/// `10·log10(2·|X|² / (fs·n))` with `n` the number of samples (not the
/// padded length): one value per point of the one-sided spectrum,
/// `fs / nfft` apart. The window's loss is not put back, the points at 0 Hz
/// and at the Nyquist frequency are not doubled, and the mean of `x` is
/// left in. Empty for fewer than two samples.
pub fn curve_level_db(fft: &Fft, x: &[f64], fs: f64) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return Vec::new();
    }
    let nfft = curve_transform_len(n);
    let mut padded = vec![0.0; nfft];
    for (i, (slot, v)) in padded.iter_mut().zip(x).enumerate() {
        *slot = v * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos());
    }
    let scale = fs * n as f64;
    let nyquist = nfft / 2;
    fft.rfft(&padded)
        .iter()
        .enumerate()
        .map(|(k, bin)| {
            let sides = if k == 0 || k == nyquist { 1.0 } else { 2.0 };
            // a point of no power at all stays a number
            10.0 * (sides * bin.norm_sqr() / scale).max(1.0e-300).log10()
        })
        .collect()
}

/// The curve of [`PsdMethod::Curve`] for the samples `x`: their level
/// ([`curve_level_db`]) smoothed by a centred moving average of the
/// decibels over [`curve_window`] points, the mean over the points that
/// exist near the ends.
pub fn curve_db(fft: &Fft, x: &[f64], fs: f64, level: usize) -> Vec<f64> {
    let raw = curve_level_db(fft, x, fs);
    openpid_dsp::smooth::moving_average_cut(&raw, curve_window(level))
}

impl Default for PsdParams {
    fn default() -> Self {
        PsdParams {
            channel: ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            },
            method: PsdMethod::default(),
            smoothing: default_smoothing(),
            segment_s: 0.25,
            overlap: 0.5,
            window: Window::Hann,
            max_hz: None,
        }
    }
}

impl std::hash::Hash for PsdParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.channel.hash(state);
        self.method.hash(state);
        self.smoothing.hash(state);
        self.segment_s.to_bits().hash(state);
        self.overlap.to_bits().hash(state);
        format!("{:?}", self.window).hash(state);
        self.max_hz.map(f64::to_bits).hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`PsdAnalysis`].
pub struct PsdOutput {
    /// Analysed channel.
    pub channel: ChannelId,
    /// Unit symbol of the channel.
    pub unit: String,
    /// How it was estimated.
    #[serde(default)]
    pub method: PsdMethod,
    /// Sample rate used.
    pub fs: f64,
    /// Segment length in samples (for the curve, the samples of the whole
    /// epoch; its transform is zero-padded to [`curve_transform_len`]).
    pub nperseg: usize,
    /// Number of averaged segments (one for the curve).
    pub segments: usize,
    /// Frequency axis in Hz.
    pub freq: Vec<f64>,
    /// `unit² / Hz`. For the curve: `10^(power_db / 10)`, the smoothed
    /// level as a power, which is not a density.
    pub power: Vec<f64>,
    /// `10·log10(power)`; for the curve, the curve itself.
    pub power_db: Vec<f64>,
}

impl PsdOutput {
    /// The `max_peaks` most prominent spectral peaks (prominence measured in dB,
    /// at least `min_prominence_db`), highest prominence first.
    pub fn peaks(&self, max_peaks: usize, min_prominence_db: f64) -> Vec<openpid_dsp::Peak> {
        let df =
            self.freq.get(1).copied().unwrap_or(1.0) - self.freq.first().copied().unwrap_or(0.0);
        openpid_dsp::find_peaks(
            &self.freq,
            &self.power_db,
            max_peaks,
            min_prominence_db,
            2.0 * df,
        )
    }
}

/// The curve of the whole prepared epoch.
fn curve(p: &PsdParams, prep: &Prepared) -> PsdOutput {
    let x = prep.filled();
    let fft = Fft::new();
    let db = curve_db(&fft, &x, prep.fs, p.smoothing);
    let step = prep.fs / curve_transform_len(x.len()) as f64;
    let keep = p.max_hz.map_or(db.len(), |m| {
        ((m / step).floor() as usize + 1).min(db.len())
    });
    let power_db = db[..keep].to_vec();
    PsdOutput {
        channel: p.channel.clone(),
        unit: prep.unit.to_string(),
        method: PsdMethod::Curve,
        fs: prep.fs,
        nperseg: x.len(),
        segments: 1,
        freq: (0..keep).map(|k| k as f64 * step).collect(),
        power: power_db.iter().map(|d| 10f64.powf(d / 10.0)).collect(),
        power_db,
    }
}

#[derive(Debug, Default, Clone, Copy)]
/// The spectrum of one channel: the reference tool's smoothed curve, or
/// Welch's power spectral density ([`PsdMethod`]).
pub struct PsdAnalysis;

impl Analysis for PsdAnalysis {
    type Params = PsdParams;
    type Output = PsdOutput;

    fn id(&self) -> &'static str {
        "psd"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::Channel(ChannelId::Time)]
    }

    fn run(&self, ctx: &AnalysisCtx<'_>, p: &PsdParams) -> Result<PsdOutput, AnalysisError> {
        if !(0.0..1.0).contains(&p.overlap) || p.segment_s <= 0.0 {
            return Err(AnalysisError::InvalidParams(
                "overlap must be in [0,1) and segment_s > 0".into(),
            ));
        }
        let prep = prepare(ctx, &p.channel, None)?;
        if p.method == PsdMethod::Curve {
            return Ok(curve(p, &prep));
        }
        let nperseg = openpid_dsp::next_smooth((p.segment_s * prep.fs).round().max(16.0) as usize);
        let x = prep.filled();
        if x.len() < nperseg {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, segment needs {nperseg}",
                x.len()
            )));
        }
        let fft = Fft::new();
        let wp = WelchParams {
            nperseg,
            noverlap: (p.overlap * nperseg as f64) as usize,
            window: p.window,
            detrend: true,
        };
        let psd = welch(&fft, &x, prep.fs, &wp)
            .map_err(|e| AnalysisError::InvalidParams(e.to_string()))?;
        let keep = p
            .max_hz
            .map_or(psd.freq.len(), |m| psd.freq.partition_point(|f| *f <= m));
        let freq = psd.freq[..keep].to_vec();
        let power = psd.power[..keep].to_vec();
        let power_db = power.iter().map(|v| 10.0 * v.max(1e-30).log10()).collect();
        Ok(PsdOutput {
            channel: p.channel.clone(),
            unit: prep.unit.to_string(),
            method: PsdMethod::Density,
            fs: prep.fs,
            nperseg,
            segments: psd.segments,
            freq,
            power,
            power_db,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uniform noise in −0.5 … 0.5 (variance 1/12), the same every run.
    fn noise(n: usize) -> Vec<f64> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect()
    }

    #[test]
    fn the_transform_is_padded_to_the_next_power_of_two() {
        let fft = Fft::new();
        for (n, nfft) in [
            (2, 2),
            (3000, 4096),
            (4096, 4096),
            (4097, 8192),
            (24_001, 32_768),
        ] {
            assert_eq!(curve_transform_len(n), nfft);
            let x = noise(n);
            assert_eq!(curve_level_db(&fft, &x, 2000.0).len(), nfft / 2 + 1, "{n}");
        }
        assert!(curve_level_db(&fft, &[1.0], 2000.0).is_empty());
    }

    #[test]
    fn a_tone_on_a_point_reads_its_power_through_the_window() {
        // n samples, not a power of two; the tone on point k of the padded
        // grid: |X| = A·Σw/2 with Σw = (n − 1)/2 for the symmetric window
        let (n, fs, amplitude, k) = (3000, 1000.0, 4.0, 1024);
        let nfft = curve_transform_len(n);
        let x: Vec<f64> = (0..n)
            .map(|i| amplitude * (std::f64::consts::TAU * (k * i) as f64 / nfft as f64).cos())
            .collect();
        let db = curve_level_db(&Fft::new(), &x, fs);
        let bin = amplitude * (n - 1) as f64 / 4.0;
        let want = 10.0 * (2.0 * bin * bin / (fs * n as f64)).log10();
        assert!((db[k] - want).abs() < 1.0e-6, "{} {want}", db[k]);
        // it is the top of the curve
        assert!(db.iter().all(|v| *v <= db[k] + 1.0e-9));
    }

    #[test]
    fn a_constant_is_undoubled_at_zero_and_an_alternation_at_nyquist() {
        // a constant c: X(0) = c·Σw; ±c alternating: X(Nyquist) = c·Σw too,
        // with Σw = (n − 1)/2; neither point is doubled
        let (n, fs, c) = (1000, 500.0, 3.0);
        let fft = Fft::new();
        let want = 10.0 * ((c * (n - 1) as f64 / 2.0).powi(2) / (fs * n as f64)).log10();
        let flat = curve_level_db(&fft, &vec![c; n], fs);
        assert!((flat[0] - want).abs() < 1.0e-9, "{} {want}", flat[0]);
        let alternating: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { c } else { -c }).collect();
        let db = curve_level_db(&fft, &alternating, fs);
        assert_eq!(db.len(), 513);
        assert!((db[512] - want).abs() < 1.0e-9, "{} {want}", db[512]);
    }

    #[test]
    fn white_noise_reads_6_8_db_below_its_density() {
        // the window's 3/8 that is not put back (−4.26 dB) and the mean of
        // the decibels of an exponential power (−2.51 dB)
        let (n, fs) = (60_000, 2000.0_f64);
        let x = noise(n);
        let density_db = 10.0 * ((1.0 / 12.0) / (fs / 2.0)).log10();
        let window = 10.0 * (3.0_f64 / 8.0).log10();
        let euler = -10.0 * std::f64::consts::LOG10_E * 0.577_215_664_901_532_9;
        let raw = curve_level_db(&Fft::new(), &x, fs);
        let mean = raw.iter().sum::<f64>() / raw.len() as f64;
        assert!(
            (mean - (density_db + window + euler)).abs() < 0.05,
            "{mean}"
        );
        // the smoothing keeps the level and narrows the spread
        let spread = |v: &[f64]| {
            let m = v.iter().sum::<f64>() / v.len() as f64;
            (v.iter().map(|d| (d - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
        };
        // the decibels of an exponential power spread by 5.6 dB
        assert!((spread(&raw) - 5.57).abs() < 0.1, "{}", spread(&raw));
        let curve = curve_db(&Fft::new(), &x, fs, 1);
        let inner = &curve[1000..curve.len() - 1000];
        let smoothed = inner.iter().sum::<f64>() / inner.len() as f64;
        assert!((smoothed - (density_db + window + euler)).abs() < 0.05);
        assert!(spread(inner) < 1.0, "{}", spread(inner));
    }

    #[test]
    fn the_curve_is_a_moving_average_of_points_cut_at_the_ends() {
        assert_eq!(CURVE_LEVELS, ["low", "medium", "high", "max"]);
        assert_eq!(
            (0..5).map(curve_window).collect::<Vec<_>>(),
            [51, 151, 301, 601, 601]
        );
        assert_eq!(CURVE_LEVELS[default_smoothing()], "medium");
        let fft = Fft::new();
        let (x, fs) = (noise(5000), 1000.0);
        let raw = curve_level_db(&fft, &x, fs);
        let mean = |s: &[f64]| s.iter().sum::<f64>() / s.len() as f64;
        for (level, half) in [(0, 25), (1, 75), (2, 150), (3, 300)] {
            let curve = curve_db(&fft, &x, fs, level);
            assert_eq!(curve.len(), raw.len());
            // the same number of points whatever the rate: inside, centred
            let i = 2000;
            assert!((curve[i] - mean(&raw[i - half..=i + half])).abs() < 1.0e-9);
            // at the ends, the mean over the points that exist
            assert!((curve[0] - mean(&raw[..=half])).abs() < 1.0e-9);
            let last = raw.len() - 1;
            assert!((curve[last] - mean(&raw[last - half..])).abs() < 1.0e-9);
            assert!((curve[10] - mean(&raw[..=10 + half])).abs() < 1.0e-9);
        }
    }
}
