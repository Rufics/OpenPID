//! Motor speed from the noise it leaves on the gyro, for logs without ESC
//! telemetry: the strongest spectral peaks of a sliding spectrum of the
//! unfiltered gyro are candidates for the motors' fundamental (or a low
//! harmonic of it); a dynamic programme picks the path through the frames
//! that keeps the speed continuous. With four motors near one speed the
//! estimate follows their mean; with the RPM filter engaged `gyro.filtered`
//! carries no peaks, so the analysis insists on `gyro.raw`.

use crate::prepare::prepare;
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_core::{Axis, ChannelId, GyroStage};
use openpid_dsp::spectrum::stft;
use openpid_dsp::{Fft, Window, find_peaks};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
/// Parameters of [`RpmEstimator`].
pub struct RpmEstimateParams {
    /// Channel to listen to (an unfiltered gyro axis).
    pub channel: ChannelId,
    /// Lowest plausible motor speed, rpm.
    pub rpm_min: f64,
    /// Highest plausible motor speed, rpm.
    pub rpm_max: f64,
    /// Spectrum segment length, seconds.
    pub segment_s: f64,
    /// Hop between segments as a fraction of the segment.
    pub hop: f64,
    /// Harmonics a peak may be (1 = the fundamental only).
    pub harmonics: u32,
    /// Penalty per relative speed jump between frames (higher = smoother).
    pub continuity: f64,
}

impl Default for RpmEstimateParams {
    fn default() -> Self {
        RpmEstimateParams {
            channel: ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Raw,
            },
            rpm_min: 1500.0,
            rpm_max: 35_000.0,
            segment_s: 0.25,
            hop: 0.25,
            harmonics: 3,
            continuity: 8.0,
        }
    }
}

impl std::hash::Hash for RpmEstimateParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.channel.hash(state);
        for v in [
            self.rpm_min,
            self.rpm_max,
            self.segment_s,
            self.hop,
            self.continuity,
        ] {
            v.to_bits().hash(state);
        }
        self.harmonics.hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`RpmEstimator`].
pub struct RpmEstimateOutput {
    /// Channel listened to.
    pub channel: ChannelId,
    /// Sample rate used.
    pub fs: f64,
    /// Centre time of every frame, seconds from the start of the epoch.
    pub time_s: Vec<f64>,
    /// Estimated motor speed per frame, rpm.
    pub rpm: Vec<f64>,
    /// How much the chosen peak stands above the band (0 = nothing found, 1 = a clean tone).
    pub confidence: Vec<f64>,
    /// Which harmonic of the estimate the chosen peak was (1 = fundamental).
    pub harmonic: Vec<u8>,
    /// Mean of the logged motor speeds at the frame times, when the log has them.
    pub logged_rpm: Option<Vec<f64>>,
    /// Median relative error against `logged_rpm`.
    pub median_error: Option<f64>,
    /// Frames analysed.
    pub frames: usize,
}

/// A candidate motor speed in one frame.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    rpm: f64,
    /// Peak amplitude over the band median, ≥ 1.
    strength: f64,
    harmonic: u8,
}

#[derive(Debug, Default, Clone, Copy)]
/// Motor speed from the gyro's noise peaks.
pub struct RpmEstimator;

impl Analysis for RpmEstimator {
    type Params = RpmEstimateParams;
    type Output = RpmEstimateOutput;

    fn id(&self) -> &'static str {
        "rpm-estimate"
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
        p: &RpmEstimateParams,
    ) -> Result<RpmEstimateOutput, AnalysisError> {
        if p.rpm_min <= 0.0
            || p.rpm_max <= p.rpm_min
            || p.segment_s <= 0.0
            || !(0.0 < p.hop && p.hop <= 1.0)
            || p.harmonics == 0
        {
            return Err(AnalysisError::InvalidParams(
                "0 < rpm_min < rpm_max, segment_s > 0, 0 < hop <= 1, harmonics >= 1".into(),
            ));
        }
        if matches!(
            p.channel,
            ChannelId::Gyro {
                stage: GyroStage::Filtered,
                ..
            }
        ) {
            return Err(AnalysisError::InvalidParams(
                "the filtered gyro has its motor peaks removed; use gyro.raw.<axis>".into(),
            ));
        }
        let sig = prepare(ctx, &p.channel, None)?;
        let fs = sig.fs;
        let x = sig.filled();
        let nperseg = openpid_dsp::next_smooth((p.segment_s * fs).round().max(16.0) as usize);
        let hop = ((p.hop * nperseg as f64).round() as usize).clamp(1, nperseg);
        if x.len() < nperseg {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, segment needs {nperseg}",
                x.len()
            )));
        }
        let sg = stft(&Fft::new(), &x, fs, nperseg, hop, Window::Hann)
            .map_err(|e| AnalysisError::InvalidParams(e.to_string()))?;
        let df = sg.freq.get(1).copied().unwrap_or(1.0) - sg.freq[0];
        let (f_min, f_max) = (p.rpm_min / 60.0, p.rpm_max / 60.0);
        // search the fundamental's band and up to the highest harmonic of it
        let lo = ((f_min / df).floor() as usize).max(1);
        let hi = ((f_max * f64::from(p.harmonics) / df).ceil() as usize).min(sg.freq.len() - 1);
        if hi <= lo + 2 {
            return Err(AnalysisError::InvalidParams(
                "the rpm band is empty at this sample rate".into(),
            ));
        }
        let nf = sg.freq.len();
        let frames = sg.time.len();
        let mut candidates: Vec<Vec<Candidate>> = Vec::with_capacity(frames);
        for ti in 0..frames {
            let row = &sg.amp[ti * nf..(ti + 1) * nf];
            let band = &row[lo..=hi];
            let mut sorted: Vec<f64> = band.to_vec();
            sorted.sort_by(f64::total_cmp);
            let median = sorted[sorted.len() / 2].max(1e-12);
            let peaks = find_peaks(&sg.freq[lo..=hi], band, 6, 0.0, 2.0 * df);
            let mut c = Vec::new();
            for pk in peaks {
                for h in 1..=p.harmonics {
                    let f = pk.x / f64::from(h);
                    if f >= f_min && f <= f_max {
                        c.push(Candidate {
                            rpm: f * 60.0,
                            strength: pk.y / median,
                            harmonic: h as u8,
                        });
                    }
                }
            }
            if c.is_empty() {
                // nothing stands out: carry the band centre with no confidence
                c.push(Candidate {
                    rpm: f64::midpoint(p.rpm_min, p.rpm_max),
                    strength: 1.0,
                    harmonic: 1,
                });
            }
            candidates.push(c);
        }
        // dynamic programme: reward strength, penalise speed jumps and harmonics
        let score = |c: &Candidate| c.strength.ln() - 0.5 * f64::from(c.harmonic - 1);
        let mut best: Vec<Vec<(f64, usize)>> = Vec::with_capacity(frames);
        for (ti, cs) in candidates.iter().enumerate() {
            let mut row = Vec::with_capacity(cs.len());
            for c in cs {
                if ti == 0 {
                    row.push((score(c), 0));
                    continue;
                }
                let (prev_score, prev_idx) = candidates[ti - 1]
                    .iter()
                    .enumerate()
                    .map(|(j, pc)| {
                        let jump = (c.rpm / pc.rpm).ln().abs();
                        (best[ti - 1][j].0 - p.continuity * jump, j)
                    })
                    .fold((f64::NEG_INFINITY, 0), |m, v| if v.0 > m.0 { v } else { m });
                row.push((prev_score + score(c), prev_idx));
            }
            best.push(row);
        }
        let mut path = vec![0usize; frames];
        if let Some(last) = best.last() {
            path[frames - 1] = last
                .iter()
                .enumerate()
                .fold(
                    (0, f64::NEG_INFINITY),
                    |m, (j, v)| if v.0 > m.1 { (j, v.0) } else { m },
                )
                .0;
        }
        for ti in (1..frames).rev() {
            path[ti - 1] = best[ti][path[ti]].1;
        }
        let rpm: Vec<f64> = path
            .iter()
            .enumerate()
            .map(|(ti, &j)| candidates[ti][j].rpm)
            .collect();
        let confidence: Vec<f64> = path
            .iter()
            .enumerate()
            .map(|(ti, &j)| {
                let s = candidates[ti][j].strength;
                ((s - 1.0) / (s + 1.0)).clamp(0.0, 1.0)
            })
            .collect();
        let harmonic: Vec<u8> = path
            .iter()
            .enumerate()
            .map(|(ti, &j)| candidates[ti][j].harmonic)
            .collect();
        // validation against logged motor speeds, when the log has them
        let motors: Vec<Vec<f64>> = (0..16u8)
            .map_while(|idx| {
                prepare(ctx, &ChannelId::MotorRpm { idx }, None)
                    .ok()
                    .map(|m| m.filled())
            })
            .collect();
        let logged_rpm = (!motors.is_empty()).then(|| {
            sg.time
                .iter()
                .map(|t| {
                    let i = ((t * fs).round() as usize).min(x.len() - 1);
                    motors.iter().map(|m| m[i]).sum::<f64>() / motors.len() as f64
                })
                .collect::<Vec<f64>>()
        });
        let median_error = logged_rpm.as_ref().map(|l| {
            let mut e: Vec<f64> = rpm
                .iter()
                .zip(l)
                .filter(|(_, t)| **t > 0.0)
                .map(|(r, t)| (r - t).abs() / t)
                .collect();
            e.sort_by(f64::total_cmp);
            e.get(e.len() / 2).copied().unwrap_or(f64::NAN)
        });
        Ok(RpmEstimateOutput {
            channel: p.channel.clone(),
            fs,
            time_s: sg.time,
            rpm,
            confidence,
            harmonic,
            logged_rpm,
            median_error,
            frames,
        })
    }
}
