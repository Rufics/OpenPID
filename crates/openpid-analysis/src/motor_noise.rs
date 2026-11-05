//! Motor / prop noise: how loud every motor's harmonics are on the gyro.
//!
//! Each motor's speed (logged eRPM, or the estimate of [`crate::RpmEstimator`]
//! when the log has none) gives the frequency of its harmonics in every
//! spectrum frame; the gyro amplitude at those frequencies, averaged over
//! the frames, is the motor's noise signature. A motor that stands out
//! (bent prop, worn bearing) shows a higher amplitude than its siblings at
//! the same harmonic.

use crate::prepare::prepare;
use crate::rpm::{RpmEstimateParams, RpmEstimator};
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_core::{Axis, ChannelId, GyroStage};
use openpid_dsp::spectrum::{WelchParams, stft, welch};
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
/// Parameters of [`MotorNoiseAnalysis`].
pub struct MotorNoiseParams {
    /// Gyro channel to listen to (unfiltered where the log has it).
    pub channel: ChannelId,
    /// Spectrum segment length, seconds.
    pub segment_s: f64,
    /// Harmonics per motor.
    pub harmonics: u32,
    /// Half-width of the band read around a harmonic, in spectrum bins.
    pub bins: usize,
    /// Upper frequency of the reported spectrum, Hz (`None` = Nyquist).
    pub max_hz: Option<f64>,
}

impl Default for MotorNoiseParams {
    fn default() -> Self {
        MotorNoiseParams {
            channel: ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Raw,
            },
            segment_s: 0.25,
            harmonics: 3,
            bins: 1,
            max_hz: None,
        }
    }
}

impl std::hash::Hash for MotorNoiseParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.channel.hash(state);
        self.segment_s.to_bits().hash(state);
        self.harmonics.hash(state);
        self.bins.hash(state);
        self.max_hz.map(f64::to_bits).hash(state);
    }
}

/// One harmonic of one motor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HarmonicNoise {
    /// Harmonic number (1 = fundamental).
    pub k: u32,
    /// Mean frequency over the frames, Hz.
    pub mean_hz: f64,
    /// Mean gyro amplitude at the harmonic, in the channel's unit.
    pub amplitude: f64,
    /// `amplitude` over the mean of all motors at this harmonic (1 = average).
    pub ratio: f64,
}

/// One motor's signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotorNoise {
    /// Motor index.
    pub idx: u8,
    /// Mean speed over the frames, rpm.
    pub mean_rpm: f64,
    /// Harmonics 1..=`harmonics`.
    pub harmonics: Vec<HarmonicNoise>,
    /// Sum of the harmonic amplitudes.
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`MotorNoiseAnalysis`].
pub struct MotorNoiseOutput {
    /// Channel listened to.
    pub channel: ChannelId,
    /// Unit of the amplitudes.
    pub unit: String,
    /// Sample rate used.
    pub fs: f64,
    /// Where the motor speeds came from: `logged` or `estimated`.
    pub rpm_source: String,
    /// Per motor.
    pub motors: Vec<MotorNoise>,
    /// Index of the loudest motor (by `total`).
    pub loudest: Option<u8>,
    /// Frequency axis of the reference spectrum, Hz.
    pub freq: Vec<f64>,
    /// Welch PSD of the channel in dB, for the plot behind the markers.
    pub psd_db: Vec<f64>,
    /// Frames averaged.
    pub frames: usize,
}

#[derive(Debug, Default, Clone, Copy)]
/// Per-motor harmonic amplitudes on the gyro.
pub struct MotorNoiseAnalysis;

impl Analysis for MotorNoiseAnalysis {
    type Params = MotorNoiseParams;
    type Output = MotorNoiseOutput;

    fn id(&self) -> &'static str {
        "motor-noise"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::Channel(ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Raw,
        })]
    }

    fn run(
        &self,
        ctx: &AnalysisCtx<'_>,
        p: &MotorNoiseParams,
    ) -> Result<MotorNoiseOutput, AnalysisError> {
        if p.segment_s <= 0.0 || p.harmonics == 0 {
            return Err(AnalysisError::InvalidParams(
                "segment_s > 0 and harmonics >= 1".into(),
            ));
        }
        let sig = prepare(ctx, &p.channel, None)?;
        let fs = sig.fs;
        let x = sig.filled();
        let nperseg = openpid_dsp::next_smooth((p.segment_s * fs).round().max(16.0) as usize);
        if x.len() < nperseg {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, segment needs {nperseg}",
                x.len()
            )));
        }
        let fft = Fft::new();
        let hop = nperseg / 2;
        let sg = stft(&fft, &x, fs, nperseg, hop, Window::Hann)
            .map_err(|e| AnalysisError::InvalidParams(e.to_string()))?;
        let nf = sg.freq.len();
        let df = sg.freq.get(1).copied().unwrap_or(1.0) - sg.freq[0];
        // motor speeds per frame: logged, else estimated for all motors alike
        let logged: Vec<Vec<f64>> = (0..16u8)
            .map_while(|idx| {
                prepare(ctx, &ChannelId::MotorRpm { idx }, None)
                    .ok()
                    .map(|m| m.filled())
            })
            .collect();
        let frame_rpm =
            |series: &[f64], t: f64| series[((t * fs).round() as usize).min(series.len() - 1)];
        let (per_motor, rpm_source): (Vec<Vec<f64>>, &str) = if logged.is_empty() {
            let est = RpmEstimator.run(
                ctx,
                &RpmEstimateParams {
                    channel: p.channel.clone(),
                    segment_s: p.segment_s,
                    ..Default::default()
                },
            )?;
            // the estimate's frames align with ours up to the hop; resample by time
            let rpm_at = |t: f64| {
                let i = est
                    .time_s
                    .partition_point(|x| *x < t)
                    .min(est.rpm.len() - 1);
                est.rpm[i]
            };
            (
                vec![sg.time.iter().map(|t| rpm_at(*t)).collect()],
                "estimated",
            )
        } else {
            (
                logged
                    .iter()
                    .map(|m| sg.time.iter().map(|t| frame_rpm(m, *t)).collect())
                    .collect(),
                "logged",
            )
        };
        let mut motors: Vec<MotorNoise> = Vec::new();
        for (m, speeds) in per_motor.iter().enumerate() {
            let mut harmonics = Vec::new();
            for k in 1..=p.harmonics {
                let (mut sum_amp, mut sum_hz, mut n) = (0.0, 0.0, 0usize);
                for (ti, rpm) in speeds.iter().enumerate() {
                    let f = rpm / 60.0 * f64::from(k);
                    let bin = (f / df).round() as usize;
                    if *rpm <= 0.0 || bin + p.bins >= nf || bin < p.bins {
                        continue;
                    }
                    let row = &sg.amp[ti * nf..(ti + 1) * nf];
                    let amp = row[bin - p.bins..=bin + p.bins]
                        .iter()
                        .copied()
                        .fold(0.0, f64::max);
                    sum_amp += amp;
                    sum_hz += f;
                    n += 1;
                }
                harmonics.push(HarmonicNoise {
                    k,
                    mean_hz: if n > 0 { sum_hz / n as f64 } else { 0.0 },
                    amplitude: if n > 0 { sum_amp / n as f64 } else { 0.0 },
                    ratio: 1.0,
                });
            }
            let mean_rpm = speeds.iter().sum::<f64>() / speeds.len().max(1) as f64;
            let total = harmonics.iter().map(|h| h.amplitude).sum();
            motors.push(MotorNoise {
                idx: m as u8,
                mean_rpm,
                harmonics,
                total,
            });
        }
        // ratios against the average motor at each harmonic
        for k in 0..p.harmonics as usize {
            let mean = motors.iter().map(|m| m.harmonics[k].amplitude).sum::<f64>()
                / motors.len().max(1) as f64;
            for m in &mut motors {
                m.harmonics[k].ratio = if mean > 0.0 {
                    m.harmonics[k].amplitude / mean
                } else {
                    1.0
                };
            }
        }
        let loudest = motors
            .iter()
            .max_by(|a, b| a.total.total_cmp(&b.total))
            .map(|m| m.idx);
        // the reference spectrum
        let psd = welch(
            &fft,
            &x,
            fs,
            &WelchParams {
                nperseg,
                noverlap: nperseg / 2,
                window: Window::Hann,
                detrend: true,
            },
        )
        .map_err(|e| AnalysisError::InvalidParams(e.to_string()))?;
        let keep = p
            .max_hz
            .map_or(psd.freq.len(), |m| psd.freq.partition_point(|f| *f <= m));
        Ok(MotorNoiseOutput {
            channel: p.channel.clone(),
            unit: sig.unit.to_string(),
            fs,
            rpm_source: rpm_source.into(),
            motors,
            loudest,
            freq: psd.freq[..keep].to_vec(),
            psd_db: psd.power[..keep]
                .iter()
                .map(|v| 10.0 * v.max(1e-30).log10())
                .collect(),
            frames: sg.time.len(),
        })
    }
}
