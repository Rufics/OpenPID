//! Step response of one axis from setpoint → gyro.
//!
//! The epoch is cut into overlapping windows; each window yields a step
//! response; windows with too little stick input or a response that does
//! not settle are discarded; the rest are averaged and summarised into rise
//! time, overshoot and settling metrics. Two estimators do this
//! ([`StepMethod`]): the spectral division of [`crate::step_windows`], which
//! is the default and what the reference tool computes, and the Wiener
//! deconvolution with a cut-off this crate started with. The latency of the
//! axis is measured independently of either, from the setpoint and the gyro
//! themselves.

use crate::prepare::{Prepared, prepare};
use crate::step_windows::{self, Windows};
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Grid2D, Requirement};
use openpid_core::{Axis, ChannelId, GyroStage, Unit};
use openpid_dsp::deconv::{DeconvParams, impulse_response, step_from_impulse};
use openpid_dsp::smooth::moving_average;
use openpid_dsp::xcorr::lag_of_max;
use openpid_dsp::{Fft, Window};
use serde::{Deserialize, Serialize};

/// How the response of a window is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMethod {
    /// Windows of 2 s every 0.1 s; in each the gyro's transform divided by
    /// the setpoint's after a Hann window ([`crate::step_windows`]); a
    /// window counts when the stick was 20 deg/s from its centre on average
    /// and its response stays between 0.5 and 1.5 after 150 ms; the plain
    /// mean of those. What the reference tool computes
    /// (decision D-002, issue #172); on plants of known
    /// response it is the true response to a few thousandths.
    #[default]
    Division,
    /// Wiener deconvolution with a low-pass at `cutoff_hz`, windows chosen
    /// by their peak input and weighted by it. It rounds a sharp response
    /// (6 % of overshoot on a pure delay) and reads the peak of an
    /// underdamped one 0.04 too high; kept for comparison.
    Wiener,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Parameters of [`StepAnalysis`]. With [`StepMethod::Division`] only
/// `axis`, `smoothing` and the histogram fields are used: the windows and
/// their limits are those of [`crate::step_windows`].
#[serde(default)]
pub struct StepParams {
    /// Axis to analyse.
    pub axis: Axis,
    /// The estimator.
    pub method: StepMethod,
    /// Smoothing of the gyro before [`StepMethod::Division`]: 0 ("off") …
    /// 3 ("high"), see [`crate::step_windows::SMOOTHING`].
    pub smoothing: usize,
    /// Window length in seconds (2 s).
    pub window_s: f64,
    /// Number of sub-windows per window (hop = window / superpos; PID-Analyzer: 16).
    pub superpos: usize,
    /// Deconvolution cut-off (Hz).
    pub cutoff_hz: f64,
    /// Windows whose peak setpoint is below this (deg/s) are ignored. 80
    /// matches the window counts and curves of the reference tool on the
    /// committed fixtures (#33, `docs/DESIGN.md` §6.2); gentle stick
    /// movements give a deconvolution dominated by noise.
    pub min_input: f64,
    /// A window whose response settles (mean of the last quarter) outside
    /// this range is dropped as a failed deconvolution: a gyro that ends at
    /// half or one and a half times the setpoint is not a step response.
    pub settle_range: (f64, f64),
    /// Boundary between the "low" and "high" input groups (deg/s; PID-Analyzer: 500).
    pub high_input: f64,
    /// Length of the response in seconds (0.5).
    pub response_s: f64,
    /// Centred moving-average width in milliseconds applied to the mean (0 = none).
    pub smooth_ms: f64,
    /// Y-offset correction: normalise every window's response on its
    /// settled tail (mean of the last quarter) before averaging, so a gain
    /// mismatch between setpoint and gyro does not shift the curves off 1.
    pub y_offset: bool,
    /// Vertical range and bin count of the response histogram.
    pub hist_range: (f64, f64),
    /// Number of amplitude bins in the histogram.
    pub hist_bins: usize,
}

impl Default for StepParams {
    fn default() -> Self {
        StepParams {
            axis: Axis::Roll,
            method: StepMethod::default(),
            smoothing: 0,
            window_s: 2.0,
            superpos: 16,
            cutoff_hz: 25.0,
            min_input: 80.0,
            settle_range: (0.5, 1.5),
            high_input: 500.0,
            response_s: 0.5,
            smooth_ms: 0.0,
            y_offset: false,
            hist_range: (-0.5, 2.5),
            hist_bins: 150,
        }
    }
}

impl std::hash::Hash for StepParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.axis.hash(state);
        for v in [
            self.window_s,
            self.cutoff_hz,
            self.min_input,
            self.high_input,
            self.response_s,
            self.smooth_ms,
            self.hist_range.0,
            self.hist_range.1,
        ] {
            v.to_bits().hash(state);
        }
        self.superpos.hash(state);
        self.hist_bins.hash(state);
        self.y_offset.hash(state);
        self.method.hash(state);
        self.smoothing.hash(state);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// Scalar characteristics of a normalised step response.
pub struct StepMetrics {
    /// Time to go from 10 % to 90 % of the final value, ms.
    pub rise_time_ms: Option<f64>,
    /// Time at which the response first crosses 50 %, ms.
    pub delay_ms: Option<f64>,
    /// Lag of the setpoint ↔ gyro cross-correlation peak over the epoch, ms,
    /// measured on the signals themselves, independently of the
    /// deconvolution. For a white input it is the transport delay of the
    /// axis; with smooth stick input the peak also carries part of the
    /// plant's own lag, so read it next to `delay_ms`.
    pub latency_ms: Option<f64>,
    /// Peak above the final value, percent of the final value.
    pub overshoot_pct: Option<f64>,
    /// Time after which the response stays within ±5 % of the final value, ms.
    pub settling_ms: Option<f64>,
    /// Mean of the last quarter of the response (should be ≈ 1).
    pub final_value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`StepAnalysis`].
pub struct StepOutput {
    /// Analysed axis.
    pub axis: Axis,
    /// The estimator that gave it.
    #[serde(default)]
    pub method: StepMethod,
    /// Sample rate used.
    pub fs: f64,
    /// Response time axis in milliseconds.
    pub time_ms: Vec<f64>,
    /// Weighted mean response over all accepted windows.
    pub mean: Vec<f64>,
    /// Mean over windows with peak input below `high_input` (if any).
    pub low: Option<Vec<f64>>,
    /// Mean over windows with peak input at or above `high_input` (if any).
    pub high: Option<Vec<f64>>,
    /// Metrics of `mean`.
    pub metrics: StepMetrics,
    /// Windows examined.
    pub windows_total: usize,
    /// Windows with enough stick input that went into the mean.
    pub windows_used: usize,
    /// Windows with enough stick input dropped because their response did not
    /// settle within `settle_range`.
    #[serde(default)]
    pub windows_rejected: usize,
    /// 2-D histogram of the individual responses: x = time (ms), y = amplitude, z = weight.
    pub histogram: Grid2D,
    /// Spread of the peak and of the time to 50 % over the used windows.
    #[serde(default)]
    pub spread: StepSpread,
}

/// Mean and standard deviation over the used windows of each window's own
/// peak and time to 50 % (the error bars of the Peak / Latency plots,
/// docs/design/axis-layout).
///
/// With [`StepMethod::Division`] they are what the reference tool shows:
/// the peak is the highest value of the mean response in its first 150 ms,
/// its deviation that of the highest of each window's first 150 samples;
/// the time to 50 % of a window is that of its first sample above 0.5,
/// counted from one and less a millisecond; the deviations are sample
/// deviations (`n − 1`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StepSpread {
    /// Mean of the windows' peaks; with [`StepMethod::Division`] the peak
    /// of the mean response in its first 150 ms.
    pub peak_mean: f64,
    /// Standard deviation of the windows' peaks.
    pub peak_sd: f64,
    /// Mean time to 50 % of the windows that reach it, milliseconds.
    pub half_ms_mean: f64,
    /// Standard deviation of that time, milliseconds.
    pub half_ms_sd: f64,
}

/// Mean and (population) standard deviation; `(0, 0)` for no values.
fn mean_sd(v: &[f64]) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let n = v.len() as f64;
    let m = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n;
    (m, var.sqrt())
}

/// [`StepSpread`] of the window responses sampled at `fs`.
fn spread(rows: &[(f64, Vec<f64>)], fs: f64) -> StepSpread {
    let peaks: Vec<f64> = rows
        .iter()
        .map(|(_, r)| r.iter().copied().fold(f64::MIN, f64::max))
        .collect();
    let halves: Vec<f64> = rows
        .iter()
        .filter_map(|(_, r)| r.iter().position(|v| *v >= 0.5))
        .map(|i| i as f64 * 1000.0 / fs)
        .collect();
    let (peak_mean, peak_sd) = mean_sd(&peaks);
    let (half_ms_mean, half_ms_sd) = mean_sd(&halves);
    StepSpread {
        peak_mean,
        peak_sd,
        half_ms_mean,
        half_ms_sd,
    }
}

/// Whether at least 95 % of the samples `at` of both channels are valid.
fn mostly_valid(sp: &Prepared, gy: &Prepared, at: std::ops::Range<usize>) -> bool {
    let valid = |p: &Prepared| p.data.valid[at.clone()].iter().filter(|v| **v).count();
    valid(sp) * 20 >= at.len() * 19 && valid(gy) * 20 >= at.len() * 19
}

/// How many step windows `axis` would contribute with `p`: `(used, total)`.
/// The same input selection as [`StepAnalysis::run`] (enough stick input,
/// at least 95 % valid samples) without the responses, so a fitness check
/// can judge the stick activity cheaply; the settle check of `run` can
/// still drop some of these.
pub fn count_windows(
    ctx: &AnalysisCtx<'_>,
    axis: Axis,
    p: &StepParams,
) -> Result<(usize, usize), AnalysisError> {
    let sp = prepare(
        ctx,
        &ChannelId::Setpoint { axis },
        Some(&Unit::DegreesPerSecond),
    )?;
    let gy = prepare(
        ctx,
        &ChannelId::Gyro {
            axis,
            stage: GyroStage::Filtered,
        },
        Some(&Unit::DegreesPerSecond),
    )?;
    let fs = sp.fs;
    let input = sp.filled();
    if p.method == StepMethod::Division {
        let windows = Windows::over(0, input.len(), fs);
        let used = (0..windows.count)
            .filter(|index| {
                let at = windows.start(*index)..windows.start(*index) + windows.length;
                mostly_valid(&sp, &gy, at.clone()) && step_windows::stick_moved(&input[at])
            })
            .count();
        return Ok((used, windows.count));
    }
    let win = ((p.window_s * fs).round() as usize).max(1);
    let hop = (win / p.superpos.max(1)).max(1);
    let (mut used, mut total) = (0, 0);
    let mut start = 0;
    while start + win <= input.len() {
        total += 1;
        let peak = input[start..start + win]
            .iter()
            .fold(0.0f64, |m, v| m.max(v.abs()));
        if peak >= p.min_input && mostly_valid(&sp, &gy, start..start + win) {
            used += 1;
        }
        start += hop;
    }
    Ok((used, total))
}

#[derive(Debug, Default, Clone, Copy)]
/// Step response by deconvolution of setpoint → gyro.
pub struct StepAnalysis;

fn weighted_mean(rows: &[(f64, Vec<f64>)], len: usize) -> Option<Vec<f64>> {
    let total: f64 = rows.iter().map(|(w, _)| w).sum();
    if rows.is_empty() || total <= 0.0 {
        return None;
    }
    let mut out = vec![0.0; len];
    for (w, r) in rows {
        for (o, v) in out.iter_mut().zip(r) {
            *o += w * v;
        }
    }
    for o in &mut out {
        *o /= total;
    }
    Some(out)
}

/// Mean of the settled tail (last quarter) of a step response.
/// `step` when it settles within `p.settle_range` (Y-offset applied when
/// asked), `None` for a failed deconvolution.
fn settled(mut step: Vec<f64>, p: &StepParams) -> Option<Vec<f64>> {
    let tail = tail_mean(&step);
    if !(p.settle_range.0..=p.settle_range.1).contains(&tail) {
        return None;
    }
    // a response that never settles (only with a wide settle_range) stays as it is
    if p.y_offset && tail.abs() > 0.2 {
        for v in &mut step {
            *v /= tail;
        }
    }
    Some(step)
}

fn tail_mean(step: &[f64]) -> f64 {
    let n = step.len();
    let tail = &step[n - n / 4..];
    if tail.is_empty() {
        return 0.0;
    }
    tail.iter().sum::<f64>() / tail.len() as f64
}

/// Longest lag searched for the latency, as a fraction of a second.
const LATENCY_MAX_LAG_S: f64 = 0.1;

/// Rise/delay/overshoot/settling of a normalised step response. `latency_ms`
/// is left `None`: it comes from the raw signals, not from the response.
pub fn metrics(step: &[f64], fs: f64) -> StepMetrics {
    let n = step.len();
    let ms = |i: usize| i as f64 * 1000.0 / fs;
    let final_value = tail_mean(step);
    if n < 4 || final_value.abs() < 1e-9 {
        return StepMetrics {
            rise_time_ms: None,
            delay_ms: None,
            latency_ms: None,
            overshoot_pct: None,
            settling_ms: None,
            final_value,
        };
    }
    let cross = |level: f64| step.iter().position(|v| v / final_value >= level);
    let (t10, t50, t90) = (cross(0.1), cross(0.5), cross(0.9));
    let rise_time_ms = match (t10, t90) {
        (Some(a), Some(b)) if b >= a => Some(ms(b - a)),
        _ => None,
    };
    let peak = step.iter().fold(f64::MIN, |m, v| m.max(*v));
    let overshoot_pct = if peak > final_value {
        Some((peak / final_value - 1.0) * 100.0)
    } else {
        Some(0.0)
    };
    let settling = (0..n)
        .rev()
        .find(|&i| (step[i] / final_value - 1.0).abs() > 0.05)
        .map_or(0, |i| i + 1);
    StepMetrics {
        rise_time_ms,
        delay_ms: t50.map(ms),
        latency_ms: None,
        overshoot_pct,
        settling_ms: if settling < n {
            Some(ms(settling))
        } else {
            None
        },
        final_value,
    }
}

/// The histogram of the responses `rows`, each with its weight: x = time,
/// y = amplitude, z = the share of the weight.
fn histogram(rows: &[(f64, &[f64])], time_ms: &[f64], p: &StepParams) -> Grid2D {
    let (ylo, yhi) = p.hist_range;
    let nb = p.hist_bins;
    let len = time_ms.len();
    let mut z = vec![0.0; nb * len];
    let wsum: f64 = rows.iter().map(|(w, _)| w).sum();
    for (w, r) in rows {
        for (ti, v) in r.iter().enumerate().take(len) {
            let f = (v - ylo) / (yhi - ylo);
            if (0.0..1.0).contains(&f) {
                let bi = (f * nb as f64) as usize;
                z[bi * len + ti] += w / wsum;
            }
        }
    }
    Grid2D {
        x: time_ms.to_vec(),
        y: (0..nb)
            .map(|i| ylo + (i as f64 + 0.5) * (yhi - ylo) / nb as f64)
            .collect(),
        z,
        x_label: "time (ms)".into(),
        y_label: "response".into(),
        z_label: "weight".into(),
    }
}

/// [`StepMethod::Division`] on the prepared setpoint and gyro of the epoch.
fn division(p: &StepParams, sp: &Prepared, gy: &Prepared) -> Result<StepOutput, AnalysisError> {
    let fs = sp.fs;
    let input = sp.filled();
    let output = gy.filled();
    let windows = Windows::over(0, input.len().min(output.len()), fs);
    if windows.count == 0 {
        return Err(AnalysisError::NotEnoughData(format!(
            "{} samples: a window needs {} and another {} after it",
            input.len(),
            windows.length,
            windows.hop
        )));
    }
    let candidates = step_windows::candidates(&input, &output, fs, p.smoothing, windows, |at| {
        mostly_valid(sp, gy, at)
    });
    let with_stick = candidates.len();
    let read = step_windows::read(candidates, fs, step_windows::settled_from(fs));
    let used = read.windows.len();
    if used == 0 {
        let (lo, hi) = step_windows::SETTLED;
        return Err(AnalysisError::NotEnoughData(if with_stick > 0 {
            format!(
                "{with_stick} of {} windows had the stick {} deg/s from its centre on average, \
                 but no response stayed between {lo} and {hi}",
                windows.count,
                step_windows::STICK_MEAN
            )
        } else {
            format!(
                "in none of {} windows was the stick {} deg/s from its centre on average",
                windows.count,
                step_windows::STICK_MEAN
            )
        }));
    }
    let time_ms: Vec<f64> = (0..read.mean.len())
        .map(|i| i as f64 * 1000.0 / fs)
        .collect();
    let rows: Vec<(f64, &[f64])> = read
        .windows
        .iter()
        .map(|w| (1.0, w.response.as_slice()))
        .collect();
    let mut metrics = metrics(&read.mean, fs);
    metrics.latency_ms = step_windows::delay_ms(&input, &output, fs);
    Ok(StepOutput {
        axis: p.axis,
        method: StepMethod::Division,
        fs,
        histogram: histogram(&rows, &time_ms, p),
        time_ms,
        low: None,
        high: None,
        metrics,
        windows_total: windows.count,
        windows_used: used,
        windows_rejected: with_stick - used,
        spread: StepSpread {
            peak_mean: read.peak,
            peak_sd: read.peak_spread,
            half_ms_mean: read.latency_ms,
            half_ms_sd: read.latency_spread_ms,
        },
        mean: read.mean,
    })
}

impl Analysis for StepAnalysis {
    type Params = StepParams;
    type Output = StepOutput;

    fn id(&self) -> &'static str {
        "step-response"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![
            Requirement::Channel(ChannelId::Setpoint { axis: Axis::Roll }),
            Requirement::Channel(ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Filtered,
            }),
        ]
    }

    fn run(&self, ctx: &AnalysisCtx<'_>, p: &StepParams) -> Result<StepOutput, AnalysisError> {
        if p.window_s <= 0.0
            || p.superpos == 0
            || p.response_s <= 0.0
            || p.response_s >= p.window_s
            || p.hist_bins < 2
            || p.settle_range.0.is_nan()
            || p.settle_range.0 >= p.settle_range.1
        {
            return Err(AnalysisError::InvalidParams(
                "window_s > response_s > 0, superpos >= 1, hist_bins >= 2, settle_range lo < hi"
                    .into(),
            ));
        }
        let sp = prepare(
            ctx,
            &ChannelId::Setpoint { axis: p.axis },
            Some(&Unit::DegreesPerSecond),
        )?;
        let gy = prepare(
            ctx,
            &ChannelId::Gyro {
                axis: p.axis,
                stage: GyroStage::Filtered,
            },
            Some(&Unit::DegreesPerSecond),
        )?;
        let fs = sp.fs;
        if p.method == StepMethod::Division {
            return division(p, &sp, &gy);
        }
        let win = (p.window_s * fs).round() as usize;
        let hop = (win / p.superpos).max(1);
        let resp_len = (p.response_s * fs).round() as usize;
        let input = sp.filled();
        let output = gy.filled();
        if input.len() < win {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, window needs {win}",
                input.len()
            )));
        }
        let fft = Fft::new();
        let dp = DeconvParams {
            cutoff_hz: p.cutoff_hz,
            window: Window::Hann,
            ..Default::default()
        };
        let mut rows: Vec<(f64, Vec<f64>)> = Vec::new(); // (peak input, response)
        let mut total = 0usize;
        let mut rejected = 0usize;
        let mut start = 0;
        while start + win <= input.len() {
            total += 1;
            let seg_in = &input[start..start + win];
            let seg_out = &output[start..start + win];
            let peak = seg_in.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            if peak >= p.min_input && mostly_valid(&sp, &gy, start..start + win) {
                let h = impulse_response(&fft, seg_in, seg_out, fs, resp_len, &dp)
                    .map_err(|e| AnalysisError::InvalidParams(e.to_string()))?;
                match settled(step_from_impulse(&h), p) {
                    Some(step) => rows.push((peak, step)),
                    None => rejected += 1,
                }
            }
            start += hop;
        }
        if rows.is_empty() {
            return Err(AnalysisError::NotEnoughData(if rejected > 0 {
                format!(
                    "{rejected} of {total} windows had stick input above {} deg/s, but none \
                     settled between {} and {}",
                    p.min_input, p.settle_range.0, p.settle_range.1
                )
            } else {
                format!(
                    "none of {total} windows had stick input above {} deg/s",
                    p.min_input
                )
            }));
        }
        let smooth = |v: Vec<f64>| {
            if p.smooth_ms > 0.0 {
                moving_average(&v, ((p.smooth_ms / 1000.0 * fs) as usize) | 1)
            } else {
                v
            }
        };
        let low: Vec<(f64, Vec<f64>)> = rows
            .iter()
            .filter(|(w, _)| *w < p.high_input)
            .cloned()
            .collect();
        let high: Vec<(f64, Vec<f64>)> = rows
            .iter()
            .filter(|(w, _)| *w >= p.high_input)
            .cloned()
            .collect();
        let mean = smooth(weighted_mean(&rows, resp_len).expect("rows non-empty"));
        let mut metrics = metrics(&mean, fs);
        let max_lag = ((LATENCY_MAX_LAG_S * fs).round() as usize).max(1);
        metrics.latency_ms = lag_of_max(&input, &output, max_lag).map(|lag| lag * 1000.0 / fs);
        let time_ms: Vec<f64> = (0..resp_len).map(|i| i as f64 * 1000.0 / fs).collect();
        // histogram of all responses, weighted by input amplitude
        let weighted: Vec<(f64, &[f64])> = rows.iter().map(|(w, r)| (*w, r.as_slice())).collect();
        let histogram = histogram(&weighted, &time_ms, p);
        Ok(StepOutput {
            axis: p.axis,
            method: StepMethod::Wiener,
            fs,
            time_ms,
            mean,
            low: weighted_mean(&low, resp_len).map(smooth),
            high: weighted_mean(&high, resp_len).map(smooth),
            metrics,
            windows_total: total,
            windows_used: rows.len(),
            windows_rejected: rejected,
            spread: spread(&rows, fs),
            histogram,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_of_ideal_responses() {
        let fs = 1000.0;
        // first-order 1 - exp(-t/tau), tau = 20 ms
        let s: Vec<f64> = (0..500)
            .map(|i| 1.0 - (-f64::from(i) / 20.0).exp())
            .collect();
        let m = metrics(&s, fs);
        assert!((m.final_value - 1.0).abs() < 0.01);
        let rise = m.rise_time_ms.unwrap();
        assert!(
            (rise - 20.0 * (0.9f64.ln() - 0.1f64.ln()).abs()).abs() < 2.0,
            "rise {rise}"
        );
        assert!((m.delay_ms.unwrap() - 20.0 * 2f64.ln()).abs() < 2.0);
        assert!(m.overshoot_pct.unwrap().abs() < 1e-3);
        assert!((m.settling_ms.unwrap() - 20.0 * 20f64.ln()).abs() < 2.0);
        // with a 25 % overshoot bump around 60 ms
        let s2: Vec<f64> = s
            .iter()
            .enumerate()
            .map(|(i, v)| v + 0.25 * (-((i as f64 - 60.0) / 15.0).powi(2) / 2.0).exp())
            .collect();
        let o = metrics(&s2, fs).overshoot_pct.unwrap();
        assert!((o - 25.0).abs() < 5.0, "overshoot {o}");
        assert_eq!(metrics(&s2, fs).latency_ms, None);
        assert_eq!(metrics(&[0.0; 8], fs).rise_time_ms, None);
        assert_eq!(tail_mean(&[]), 0.0);
    }
}
