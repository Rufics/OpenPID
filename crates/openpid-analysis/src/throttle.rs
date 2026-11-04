//! Frequency × throttle amplitude heatmap: the noise signature of a channel
//! as a function of throttle position.
//!
//! Method (as in PIDtoolbox): the epoch is cut into short segments, each
//! segment gets a windowed single-sided amplitude spectrum and a mean
//! throttle; for every throttle bin `t ∈ 0..=100 %` the spectra of all
//! segments with `|throttle − t| ≤ half_window` are averaged; the grid is then
//! smoothed with a box filter.

use crate::prepare::prepare;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_core::{ChannelId, Unit};
use openpid_dsp::smooth::box_filter_2d;
use openpid_dsp::spectrum::amplitude_spectrum;
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThrottleHeatmapParams {
    pub channel: ChannelId,
    /// Segment length in seconds (PTB: 0.2 s).
    pub segment_s: f64,
    /// Throttle half-window in percent for averaging (PTB: 6).
    pub half_window: f64,
    pub window: Window,
    pub max_hz: f64,
    /// Box smoothing widths: along throttle (bins) and along frequency (as a
    /// fraction of the segment length in samples, PTB: 1/100).
    pub smooth_throttle: usize,
    pub smooth_freq_frac: f64,
}

impl Default for ThrottleHeatmapParams {
    fn default() -> Self {
        ThrottleHeatmapParams {
            channel: ChannelId::Gyro {
                axis: openpid_core::Axis::Roll,
                stage: openpid_core::GyroStage::Filtered,
            },
            segment_s: 0.2,
            half_window: 6.0,
            window: Window::Hann,
            max_hz: 1000.0,
            smooth_throttle: 8,
            smooth_freq_frac: 0.01,
        }
    }
}

impl std::hash::Hash for ThrottleHeatmapParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.channel.hash(state);
        self.segment_s.to_bits().hash(state);
        self.half_window.to_bits().hash(state);
        format!("{:?}", self.window).hash(state);
        self.max_hz.to_bits().hash(state);
        self.smooth_throttle.hash(state);
        self.smooth_freq_frac.to_bits().hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThrottleHeatmapOutput {
    pub channel: ChannelId,
    pub unit: String,
    pub fs: f64,
    pub nperseg: usize,
    pub segments: usize,
    /// x = throttle (%), y = frequency (Hz), z = mean amplitude.
    pub grid: Grid2D,
    /// Number of segments that contributed to each throttle bin.
    pub counts: Vec<usize>,
}

#[derive(Debug, Default, Clone, Copy)]
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
        if p.segment_s <= 0.0 || p.max_hz <= 0.0 {
            return Err(AnalysisError::InvalidParams(
                "segment_s and max_hz must be positive".into(),
            ));
        }
        let sig = prepare(ctx, &p.channel, None)?;
        let thr = prepare(ctx, &ChannelId::Throttle, Some(&Unit::Percent))?;
        let nperseg = openpid_dsp::next_smooth((p.segment_s * sig.fs).round().max(16.0) as usize);
        let x = sig.filled();
        let t = thr.filled();
        if x.len() < nperseg {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, segment needs {nperseg}",
                x.len()
            )));
        }
        let fft = Fft::new();
        let freqs = openpid_dsp::fft::rfft_freqs(nperseg, sig.fs);
        let nf = freqs.partition_point(|f| *f <= p.max_hz);
        // per-segment spectra and mean throttle
        let mut spectra: Vec<Vec<f64>> = Vec::new();
        let mut throttles: Vec<f64> = Vec::new();
        let mut start = 0;
        while start + nperseg <= x.len() {
            let valid = sig.data.valid[start..start + nperseg]
                .iter()
                .filter(|v| **v)
                .count();
            if valid * 10 >= nperseg * 9 {
                let mut a = amplitude_spectrum(&fft, &x[start..start + nperseg], p.window);
                a.truncate(nf);
                spectra.push(a);
                throttles.push(t[start..start + nperseg].iter().sum::<f64>() / nperseg as f64);
            }
            start += nperseg;
        }
        if spectra.is_empty() {
            return Err(AnalysisError::NotEnoughData(
                "no complete segment in the epoch".into(),
            ));
        }
        let nx = 101usize;
        let mut z = vec![0.0; nx * nf];
        let mut counts = vec![0usize; nx];
        for (ix, count) in counts.iter_mut().enumerate() {
            let centre = ix as f64;
            let mut acc = vec![0.0; nf];
            for (s, th) in spectra.iter().zip(&throttles) {
                if (th - centre).abs() <= p.half_window {
                    for (k, v) in s.iter().enumerate() {
                        acc[k] += v;
                    }
                    *count += 1;
                }
            }
            if *count > 0 {
                for (k, v) in acc.iter().enumerate() {
                    z[k * nx + ix] = v / *count as f64;
                }
            }
        }
        let wy = ((p.smooth_freq_frac * nperseg as f64).round() as usize).max(1);
        let z = box_filter_2d(&z, nx, nf, p.smooth_throttle.max(1), wy);
        let grid = Grid2D {
            x: (0..nx).map(|i| i as f64).collect(),
            y: freqs[..nf].to_vec(),
            z,
            x_label: "throttle (%)".into(),
            y_label: "frequency (Hz)".into(),
            z_label: format!("amplitude ({})", sig.unit),
        };
        Ok(ThrottleHeatmapOutput {
            channel: p.channel.clone(),
            unit: sig.unit.to_string(),
            fs: sig.fs,
            nperseg,
            segments: spectra.len(),
            grid,
            counts,
        })
    }
}
