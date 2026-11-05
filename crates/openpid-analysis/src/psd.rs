//! Welch power spectral density of one channel.

use crate::prepare::prepare;
use openpid_core::ChannelId;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_dsp::spectrum::{WelchParams, welch};
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Parameters of [`PsdAnalysis`].
pub struct PsdParams {
    /// Channel to analyse.
    pub channel: ChannelId,
    /// Segment length in seconds (rounded to a 5-smooth sample count).
    pub segment_s: f64,
    /// Overlap as a fraction of the segment (0..1).
    pub overlap: f64,
    /// Window applied to every segment.
    pub window: Window,
    /// Upper frequency bound of the output (Hz); `None` keeps everything up to Nyquist.
    pub max_hz: Option<f64>,
}

impl Default for PsdParams {
    fn default() -> Self {
        PsdParams {
            channel: ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            },
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
    /// Sample rate used.
    pub fs: f64,
    /// Segment length in samples.
    pub nperseg: usize,
    /// Number of averaged segments.
    pub segments: usize,
    /// Frequency axis in Hz.
    pub freq: Vec<f64>,
    /// `unit² / Hz`
    pub power: Vec<f64>,
    /// `10·log10(power)`
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

#[derive(Debug, Default, Clone, Copy)]
/// Welch power spectral density of one channel.
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
            fs: prep.fs,
            nperseg,
            segments: psd.segments,
            freq,
            power,
            power_db,
        })
    }
}
