//! The spectrum of one channel: Welch's power spectral density, or the
//! smoothed curve of the whole log that the reference tool draws (what the
//! Spectrum tab shows by default: decision D-001, issue #171).

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
    /// Smoothing level of [`PsdMethod::Curve`]: 0 ("low") … 4 ("high"),
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
    /// One Hann-windowed transform of the whole epoch, in decibels, smoothed
    /// along the frequency by a local regression: the curve the reference
    /// tool draws (decision D-001, issue #171). The
    /// height of a peak follows its width as well as its strength: noise
    /// reads 6.8 dB below its density and a steady tone almost vanishes.
    /// The Spectrum tab and the command line draw this by default.
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
pub const CURVE_LEVELS: [&str; 5] = ["low", "low-med", "medium", "med-high", "high"];

fn default_smoothing() -> usize {
    2
}

/// Points of the regression window of the curve at smoothing level `level`
/// (0 … 4) for a curve of `points` points: the cube of the level's number
/// (1 … 5) times the decimal logarithm of the number of points, made odd.
/// For the 28 000 points of a 30 s log at 2 kHz: 5, 35, 121, 285, 555. The
/// same level is therefore a wider band in hertz for a shorter log.
pub fn curve_window(level: usize, points: usize) -> usize {
    let cube = (level.min(CURVE_LEVELS.len() - 1) + 1).pow(3) as f64;
    let exact = cube * (points.max(1) as f64).log10();
    // an even or fractional length goes to the odd number at or below it,
    // or just above an even one: 120.07 and 120 both give 121
    2 * (exact / 2.0 + 1.0e-9).floor() as usize + 1
}

/// The transform of the whole of `x` through a symmetric Hann window as a
/// level in decibels, `10·log10(2·|X|² / (fs·N))`: one value per point of
/// the one-sided spectrum, `fs / x.len()` apart. The window's loss is not
/// put back and the point at 0 Hz is not doubled; the mean of `x` is left
/// in. Empty for fewer than two samples.
pub fn curve_level_db(fft: &Fft, x: &[f64], fs: f64) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return Vec::new();
    }
    let windowed: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, v)| v * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos()))
        .collect();
    let scale = fs * n as f64;
    fft.rfft(&windowed)
        .iter()
        .enumerate()
        .map(|(k, bin)| {
            let sides = if k == 0 { 1.0 } else { 2.0 };
            // a point of no power at all stays a number
            10.0 * (sides * bin.norm_sqr() / scale).max(1.0e-300).log10()
        })
        .collect()
}

/// The curve of [`PsdMethod::Curve`] for the samples `x`: their level
/// ([`curve_level_db`]) smoothed by a local regression over
/// [`curve_window`] points, dB.
pub fn curve_db(fft: &Fft, x: &[f64], fs: f64, level: usize) -> Vec<f64> {
    let raw = curve_level_db(fft, x, fs);
    openpid_dsp::smooth::local_line(&raw, curve_window(level, raw.len()))
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
    /// Segment length in samples (the whole epoch for the curve).
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
    let step = prep.fs / x.len().max(1) as f64;
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
