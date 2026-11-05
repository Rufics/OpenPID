//! Frequency × time amplitude heatmap (spectrogram) of one channel.

use crate::prepare::prepare;
use openpid_core::ChannelId;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_dsp::spectrum::stft;
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Parameters of [`StftAnalysis`].
#[serde(default)]
pub struct StftParams {
    /// Channel to analyse.
    pub channel: ChannelId,
    /// Segment length in seconds.
    pub segment_s: f64,
    /// Hop between segments as a fraction of the segment.
    pub hop: f64,
    /// Window applied to every segment.
    pub window: Window,
    /// Upper frequency bound of the output (Hz).
    pub max_hz: Option<f64>,
}

impl Default for StftParams {
    fn default() -> Self {
        StftParams {
            channel: ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            },
            segment_s: 0.2,
            hop: 0.25,
            window: Window::Hann,
            max_hz: Some(1000.0),
        }
    }
}

impl std::hash::Hash for StftParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.channel.hash(state);
        self.segment_s.to_bits().hash(state);
        self.hop.to_bits().hash(state);
        format!("{:?}", self.window).hash(state);
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
    /// x = time (s from session start), y = frequency (Hz), z = amplitude in channel units.
    pub grid: Grid2D,
}

#[derive(Debug, Default, Clone, Copy)]
/// Frequency × time amplitude heatmap of one channel.
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
        if !(0.0..=1.0).contains(&p.hop) || p.hop == 0.0 || p.segment_s <= 0.0 {
            return Err(AnalysisError::InvalidParams(
                "hop must be in (0,1] and segment_s > 0".into(),
            ));
        }
        let prep = prepare(ctx, &p.channel, None)?;
        let nperseg = openpid_dsp::next_smooth((p.segment_s * prep.fs).round().max(16.0) as usize);
        let hop = ((p.hop * nperseg as f64) as usize).max(1);
        let x = prep.filled();
        let fft = Fft::new();
        let s = stft(&fft, &x, prep.fs, nperseg, hop, p.window)
            .map_err(|e| AnalysisError::NotEnoughData(e.to_string()))?;
        let nf = p
            .max_hz
            .map_or(s.freq.len(), |m| s.freq.partition_point(|f| *f <= m));
        let mut z = Vec::with_capacity(nf * s.time.len());
        for fi in 0..nf {
            for ti in 0..s.time.len() {
                z.push(s.get(ti, fi));
            }
        }
        let grid = Grid2D {
            x: s.time.iter().map(|t| t + prep.t0_s).collect(),
            y: s.freq[..nf].to_vec(),
            z,
            x_label: "time (s)".into(),
            y_label: "frequency (Hz)".into(),
            z_label: format!("amplitude ({})", prep.unit),
        };
        Ok(StftOutput {
            channel: p.channel.clone(),
            unit: prep.unit.to_string(),
            fs: prep.fs,
            nperseg,
            grid,
        })
    }
}
