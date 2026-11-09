//! Frequency × throttle amplitude heatmap: the noise signature of a channel
//! as a function of throttle position.
//!
//! Method: the epoch is cut into short segments, each
//! segment gets a windowed single-sided amplitude spectrum and a mean
//! throttle; for every throttle bin `t ∈ 0..=100 %` the spectra of all
//! segments with `|throttle − t| ≤ half_window` are averaged; the grid is then
//! smoothed with a box filter.
//!
//! When the log carries motor rpm, the output also holds an [`RpmOverlay`]:
//! the median rpm of every motor per throttle bin (same segments, same
//! window), so a front-end can draw the motor harmonics `k · rpm / 60` over
//! the heatmap, together with the static notches and the dynamic-notch band
//! from the settings.

use crate::prepare::Prepared;
use crate::prepare::prepare;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_core::{ChannelId, Unit};
use openpid_dsp::smooth::box_filter_2d;
use openpid_dsp::spectrum::amplitude_spectrum;
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Parameters of [`ThrottleHeatmap`].
#[serde(default)]
pub struct ThrottleHeatmapParams {
    /// Channel to analyse.
    pub channel: ChannelId,
    /// Segment length in seconds (0.2 s).
    pub segment_s: f64,
    /// Throttle half-window in percent for averaging (6).
    pub half_window: f64,
    /// Window applied to every segment.
    pub window: Window,
    /// Upper frequency bound of the output (Hz).
    pub max_hz: f64,
    /// Box smoothing widths: along throttle (bins) and along frequency (as a
    /// fraction of the segment length in samples, 1/100).
    pub smooth_throttle: usize,
    /// Box smoothing width along frequency, as a fraction of the segment length.
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
/// Motor speed per throttle bin and the notch configuration, for drawing
/// over the heatmap.
pub struct RpmOverlay {
    /// Per motor (index order), the median rpm of the segments in every
    /// throttle bin of the grid; `None` where no segment fell in the bin.
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
    /// Frequency of harmonic `k` (1-based) of motor `motor` in throttle bin `bin`, Hz.
    pub fn harmonic_hz(&self, motor: usize, k: u32, bin: usize) -> Option<f64> {
        let rpm = (*self.motors.get(motor)?.get(bin)?)?;
        Some(rpm / 60.0 * f64::from(k))
    }
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
    /// Number of segments with a complete spectrum.
    pub segments: usize,
    /// x = throttle (%), y = frequency (Hz), z = mean amplitude.
    pub grid: Grid2D,
    /// Number of segments that contributed to each throttle bin.
    pub counts: Vec<usize>,
    /// Motor speeds per throttle bin, when the log has motor rpm.
    pub overlay: Option<RpmOverlay>,
}

#[derive(Debug, Default, Clone, Copy)]
/// Frequency × throttle amplitude heatmap of one channel.
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
        let motors: Vec<Prepared> = (0..16u8)
            .map_while(|idx| prepare(ctx, &ChannelId::MotorRpm { idx }, None).ok())
            .collect();
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
        // per segment, the mean rpm of every motor (only on valid samples)
        let mut rpms: Vec<Vec<Option<f64>>> = vec![Vec::new(); motors.len()];
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
                for (m, motor) in motors.iter().enumerate() {
                    let (sum, n) = (start..start + nperseg)
                        .filter(|&i| motor.data.valid[i])
                        .fold((0.0, 0usize), |(s, n), i| (s + motor.data.values[i], n + 1));
                    rpms[m].push((n * 2 >= nperseg).then(|| sum / n as f64));
                }
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
        let overlay = (!motors.is_empty()).then(|| {
            let chain = &ctx.session.settings.gyro_filters;
            RpmOverlay {
                motors: rpms
                    .iter()
                    .map(|per_segment| {
                        (0..nx)
                            .map(|ix| {
                                let centre = ix as f64;
                                let mut v: Vec<f64> = per_segment
                                    .iter()
                                    .zip(&throttles)
                                    .filter(|(_, th)| (*th - centre).abs() <= p.half_window)
                                    .filter_map(|(r, _)| *r)
                                    .collect();
                                if v.is_empty() {
                                    return None;
                                }
                                v.sort_by(f64::total_cmp);
                                Some(v[v.len() / 2])
                            })
                            .collect()
                    })
                    .collect(),
                harmonics: chain.rpm_notch.map_or(3, |r| r.harmonics.max(1)),
                rpm_min_hz: chain.rpm_notch.map(|r| r.min_hz),
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
            overlay,
        })
    }
}
