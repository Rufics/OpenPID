//! The step response the reference tool draws for an axis, as the first
//! round on that window found it (`docs/calibration/step.md`).
//!
//! Like [`crate::model`] it is a model of observed behaviour: what the tool
//! showed for probe logs of known content, read from plots it copies as
//! vector graphics. On the readings of `tests/calibration/step/` it is
//! exact to the precision of those copies. It has been fitted to one round
//! only; the next round's predictions are made with it, so that the round
//! confirms it or corrects it.
//!
//! The steps:
//! 1. the filtered gyro is smoothed by a local regression over 1, 5, 11 or
//!    21 samples ([`SMOOTHING`]); the setpoint is not;
//! 2. the log without its first and last second is cut into windows of two
//!    seconds, a tenth of a second apart ([`grid`]);
//! 3. in every window the setpoint and the gyro go through a Hann window
//!    and are followed by 200 zeros; the gyro's transform is divided by the
//!    setpoint's with a small constant in the denominator; the result, back
//!    in time, is summed up, and its first half second is the window's
//!    response ([`response`]);
//! 4. a window counts when the stick was off its centre by 20 deg/s or more
//!    on average, and when its response stays between 0.5 and 1.5 from
//!    150 ms on ([`counts`]);
//! 5. the curve is the mean of the windows that count; "Peak" and
//!    "SR Latency" are read from them ([`Step`]).

use crate::model;
use openpid_dsp::fft::{Complex, Fft};
use std::f64::consts::TAU;

/// The smoothing levels of the tool's step-response window, in the order
/// of its list, with the samples of the regression window of each. Found
/// at 2 kHz; whether the windows follow the log rate is not known.
pub const SMOOTHING: [(&str, usize); 4] = [("off", 1), ("low", 5), ("medium", 11), ("high", 21)];

/// Length of a window, seconds.
const WINDOW_S: f64 = 2.0;
/// Distance between the starts of two windows, seconds.
const HOP_S: f64 = 0.1;
/// Zeros that follow a window before its transform: the same number at 1,
/// 2 and 4 kHz.
const PAD: usize = 200;
/// Length of the response that is kept, seconds.
const SHOWN_S: f64 = 0.5;
/// The constant in the denominator of the division, for transforms
/// divided by their length.
const FLOOR: f64 = 1.0e-4;
/// The mean distance of the stick from its centre a window needs, deg/s.
const STICK_MEAN: f64 = 20.0;
/// From here on a response has to stay inside [`SETTLED`], seconds. Found
/// at 2 kHz, where it is sample 294 … 317; at the other rates no window
/// was near the limit, so it may be a number of samples instead.
const SETTLED_FROM_S: f64 = 0.15;
/// Where a response has to stay from [`SETTLED_FROM_S`] on.
const SETTLED: (f64, f64) = (0.5, 1.5);
/// The level at which the latency of a window is read.
const LATENCY_LEVEL: f64 = 0.5;
/// The samples of a window's response whose highest value goes into the
/// bar of the "Peak" plot: the same number at 1, 2 and 4 kHz.
const PEAK_SAMPLES: usize = 150;
/// The rate the setpoint and the gyro are brought to before their delay
/// is read, Hz.
const DELAY_RATE: f64 = 8000.0;
/// The longest delay looked for, seconds. Round 4 had none above 19 ms;
/// where the tool stops looking is not known.
const DELAY_MAX_S: f64 = 0.2;

/// Samples by which the first window starts after the first second for
/// most logs. For one log of round 4 (`t9-short-2k`, 12 s) it was three.
pub const LATE: usize = 2;

/// Where the windows of a log lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    /// The first sample of the first window.
    pub first: usize,
    /// Samples in a window.
    pub length: usize,
    /// Samples between the starts of two windows.
    pub hop: usize,
    /// Number of windows.
    pub count: usize,
}

impl Grid {
    /// The first sample of window `index`.
    pub fn start(&self, index: usize) -> usize {
        self.first + index * self.hop
    }
}

/// The windows of a log of `len` samples at `fs` whose first window starts
/// `late` samples after the first second, or `None` when the log holds no
/// window.
pub fn grid(len: usize, fs: f64, late: usize) -> Option<Grid> {
    let second = fs.round() as usize;
    let length = (WINDOW_S * fs).round() as usize;
    let hop = (HOP_S * fs).round() as usize;
    // the span of the spectrum: the log less two seconds and one sample
    let span = len.checked_sub(2 * second + 1)?;
    let count = span.checked_sub(length)? / hop.max(1);
    let grid = Grid {
        first: second + late,
        length,
        hop,
        count,
    };
    (count > 0 && grid.start(count - 1) + length <= len).then_some(grid)
}

/// What the response of every window of one length is computed with: the
/// plans of the transforms and the Hann window, made once.
struct Divider {
    fft: Fft,
    hann: Vec<f64>,
}

impl Divider {
    /// For windows of `length` samples.
    fn new(length: usize) -> Self {
        let last = length.saturating_sub(1).max(1) as f64;
        Divider {
            fft: Fft::new(),
            hann: (0..length)
                .map(|i| 0.5 - 0.5 * (TAU * i as f64 / last).cos())
                .collect(),
        }
    }

    /// The transform of a window through the Hann window, zeros after it.
    fn through(&self, x: &[f64]) -> Vec<Complex<f64>> {
        let mut buf = vec![Complex::new(0.0, 0.0); self.hann.len() + PAD];
        for ((slot, v), hann) in buf.iter_mut().zip(x).zip(&self.hann) {
            slot.re = v * hann;
        }
        self.fft.forward_inplace(&mut buf);
        buf
    }

    /// The first `points` samples of the response of a window.
    fn response(&self, setpoint: &[f64], gyro: &[f64], points: usize) -> Vec<f64> {
        let input = self.through(setpoint);
        let mut output = self.through(gyro);
        let padded = input.len();
        let floor = FLOOR * (padded * padded) as f64;
        for (y, x) in output.iter_mut().zip(&input) {
            *y = *y * x.conj() / (x.norm_sqr() + floor);
        }
        self.fft.inverse_inplace(&mut output);
        output
            .iter()
            .take(points)
            .scan(0.0, |sum, impulse| {
                *sum += impulse.re;
                Some(*sum)
            })
            .collect()
    }
}

/// The response of one window: `setpoint` and `gyro` are the samples of
/// the window, and the first `points` samples of the response come back.
pub fn response(setpoint: &[f64], gyro: &[f64], points: usize) -> Vec<f64> {
    let length = setpoint.len().min(gyro.len());
    if length < 2 {
        return Vec::new();
    }
    Divider::new(length).response(&setpoint[..length], &gyro[..length], points)
}

/// Whether the stick of a window, whose samples are `setpoint`, was far
/// enough from its centre on average for the window to count.
pub fn stick_moved(setpoint: &[f64]) -> bool {
    !setpoint.is_empty()
        && setpoint.iter().map(|v| v.abs()).sum::<f64>() >= STICK_MEAN * setpoint.len() as f64
}

/// Whether a `response` stays inside 0.5 … 1.5 from sample `from` on.
pub fn settled(response: &[f64], from: usize) -> bool {
    response.len() > from
        && response[from..]
            .iter()
            .all(|v| *v > SETTLED.0 && *v < SETTLED.1)
}

/// The first sample from which a response has to stay inside 0.5 … 1.5,
/// at `fs`: 150 ms.
pub fn settled_from(fs: f64) -> usize {
    (SETTLED_FROM_S * fs).round() as usize
}

/// Whether a window counts: `setpoint` are its samples, `response` its
/// response, which has to stay inside 0.5 … 1.5 from sample `from` on.
pub fn counts(setpoint: &[f64], response: &[f64], from: usize) -> bool {
    stick_moved(setpoint) && settled(response, from)
}

/// A window and its response.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Its place in the [`Grid`].
    pub index: usize,
    /// Its response, one value per sample from 0 to half a second.
    pub response: Vec<f64>,
}

/// What the tool shows for one axis of a log.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Where the windows of the log lie.
    pub grid: Grid,
    /// The windows that count, in the order of the log: the curves the
    /// tool draws with "Raw" on, and the `n` of its legend.
    pub windows: Vec<Window>,
    /// The curve: the mean of the windows, one value per sample.
    pub mean: Vec<f64>,
    /// "Peak": the highest value of the curve. On two of 26 plots of round
    /// 4, both of plants that never overshoot, the tool's value was the
    /// highest of the first 137 … 324 ms only; where it stops looking is
    /// not known, and this is the highest of the whole curve.
    pub peak: f64,
    /// The bar of "Peak": the spread (sample standard deviation) over the
    /// windows of the highest value among the first 150 samples of each.
    pub peak_spread: f64,
    /// "SR Latency", ms: the mean over the windows of the time of the
    /// first sample above 0.5, counted from one and less a millisecond.
    pub latency_ms: f64,
    /// The bar of "SR Latency": the spread of those times, ms.
    pub latency_spread_ms: f64,
}

/// Mean and sample standard deviation.
fn spread(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if values.len() < 2 {
        return (mean, 0.0);
    }
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, var.sqrt())
}

/// The step response of an axis: `setpoint` and `gyro` are the values a
/// log holds from its first frame to its last, `smoothing` the level (0
/// for "off" … 3 for "high"). `None` for a log too short for one window.
pub fn step(setpoint: &[f64], gyro: &[f64], fs: f64, smoothing: usize) -> Option<Step> {
    step_from(setpoint, gyro, fs, smoothing, LATE)
}

/// [`step`] for a log whose first window starts `late` samples after the
/// first second.
pub fn step_from(
    setpoint: &[f64],
    gyro: &[f64],
    fs: f64,
    smoothing: usize,
    late: usize,
) -> Option<Step> {
    step_with(setpoint, gyro, fs, smoothing, late, settled_from(fs))
}

/// The windows of a log whose stick moved enough, each with its response,
/// whether the response keeps its limits or not: `setpoint` and `gyro` as
/// for [`step`], the first window `late` samples after the first second.
pub fn candidates(
    setpoint: &[f64],
    gyro: &[f64],
    fs: f64,
    smoothing: usize,
    late: usize,
) -> Option<(Grid, Vec<Window>)> {
    let len = setpoint.len().min(gyro.len());
    let grid = grid(len, fs, late)?;
    let span = SMOOTHING[smoothing.min(SMOOTHING.len() - 1)].1;
    let gyro = model::smooth(&gyro[..len], span);
    let points = (SHOWN_S * fs).round() as usize + 1;
    let divider = Divider::new(grid.length);
    let windows = (0..grid.count)
        .filter_map(|index| {
            let at = grid.start(index)..grid.start(index) + grid.length;
            stick_moved(&setpoint[at.clone()]).then(|| Window {
                index,
                response: divider.response(&setpoint[at.clone()], &gyro[at], points),
            })
        })
        .collect();
    Some((grid, windows))
}

/// [`step_from`] with the responses held to 0.5 … 1.5 from sample
/// `from` on, for the reading of round 4 in which that limit is a number
/// of samples.
pub fn step_with(
    setpoint: &[f64],
    gyro: &[f64],
    fs: f64,
    smoothing: usize,
    late: usize,
    from: usize,
) -> Option<Step> {
    let (grid, windows) = candidates(setpoint, gyro, fs, smoothing, late)?;
    Some(read(grid, windows, fs, from))
}

/// What the tool shows for the windows of [`candidates`]: those whose
/// response stays inside 0.5 … 1.5 from sample `from` on count.
pub fn read(grid: Grid, mut windows: Vec<Window>, fs: f64, from: usize) -> Step {
    windows.retain(|w| settled(&w.response, from));
    let points = (SHOWN_S * fs).round() as usize + 1;
    let n = windows.len() as f64;
    let mean: Vec<f64> = (0..points)
        .map(|i| windows.iter().map(|w| w.response[i]).sum::<f64>() / n)
        .collect();
    let peak = mean.iter().copied().fold(f64::NAN, f64::max);
    let highest: Vec<f64> = windows
        .iter()
        .map(|w| {
            let early = &w.response[..PEAK_SAMPLES.min(points)];
            early.iter().copied().fold(f64::MIN, f64::max)
        })
        .collect();
    let times: Vec<f64> = windows
        .iter()
        .filter_map(|w| w.response.iter().position(|v| *v > LATENCY_LEVEL))
        .map(|i| (i + 1) as f64 * 1000.0 / fs - 1.0)
        .collect();
    let (_, peak_spread) = spread(&highest);
    let (latency_ms, latency_spread_ms) = spread(&times);
    Step {
        grid,
        windows,
        mean,
        peak,
        peak_spread,
        latency_ms,
        latency_spread_ms,
    }
}

/// "SP-Gyro Delay", ms: the setpoint and the gyro of the log without its
/// first and last second, brought to 8 kHz along straight lines between
/// the samples, and the shift at which the two agree best (the largest sum
/// of products). It gave the tool's value on 25 of the 27 plots of round
/// 4; on a noisy axis and on one of small stick moves the tool read 0.25
/// and 0.125 ms less, so a part of its rule is still missing.
pub fn sp_gyro_delay_ms(setpoint: &[f64], gyro: &[f64], fs: f64) -> Option<f64> {
    let span = model::span(setpoint.len().min(gyro.len()), fs)?;
    let factor = (DELAY_RATE / fs).round().max(1.0) as usize;
    let fine = |x: &[f64]| -> Vec<f64> {
        let mut out = Vec::with_capacity((x.len() - 1) * factor + 1);
        for pair in x.windows(2) {
            for step in 0..factor {
                let part = step as f64 / factor as f64;
                out.push(pair[0] + (pair[1] - pair[0]) * part);
            }
        }
        out.extend(x.last());
        out
    };
    let setpoint = fine(&setpoint[span.clone()]);
    let gyro = fine(&gyro[span]);
    let rate = fs * factor as f64;
    let n = setpoint.len();
    let lags = ((DELAY_MAX_S * rate).round() as usize).min(n);
    // the sums of products for every shift at once: the transform of one
    // times the conjugate of the other's, long enough for nothing to wrap
    let size = (n + lags).next_power_of_two();
    let fft = Fft::new();
    let through = |x: &[f64]| {
        let mut buf = vec![Complex::new(0.0, 0.0); size];
        for (slot, v) in buf.iter_mut().zip(x) {
            slot.re = *v;
        }
        fft.forward_inplace(&mut buf);
        buf
    };
    let input = through(&setpoint);
    let mut sums = through(&gyro);
    for (y, x) in sums.iter_mut().zip(&input) {
        *y *= x.conj();
    }
    fft.inverse_inplace(&mut sums);
    sums.iter()
        .take(lags)
        .enumerate()
        .max_by(|a, b| a.1.re.total_cmp(&b.1.re))
        .map(|(lag, _)| lag as f64 * 1000.0 / rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_of_a_log() {
        // 30 s at 2 kHz: 259 windows of 4000 samples, 200 apart, from 2002
        let g = grid(60_000, 2000.0, LATE).unwrap();
        assert_eq!((g.first, g.length, g.hop, g.count), (2002, 4000, 200, 259));
        assert_eq!(g.start(258), 53_602);
        // 12 s: 79
        assert_eq!(grid(24_000, 2000.0, 3).unwrap().count, 79);
        // the same seconds at other rates give the same number of windows
        assert_eq!(grid(30_000, 1000.0, LATE).unwrap().count, 259);
        assert_eq!(grid(120_000, 4000.0, LATE).unwrap().count, 259);
        // four seconds hold no window: two are cut off, and the two that
        // are left are one sample short
        assert_eq!(grid(8000, 2000.0, LATE), None);
        assert_eq!(grid(100, 2000.0, LATE), None);
        assert_eq!(grid(8201, 2000.0, LATE).unwrap().count, 1);
    }

    /// A stick that moves to a new level within 200 deg/s every 150 …
    /// 700 ms, and a gyro that follows it `delay` samples later.
    fn delayed(n: usize, delay: usize) -> (Vec<f64>, Vec<f64>) {
        let mut state = 12_345_u64;
        let mut next = |below: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % below
        };
        let mut setpoint = Vec::with_capacity(n);
        while setpoint.len() < n {
            let level = next(401) as f64 - 200.0;
            let hold = 300 + next(1100) as usize;
            setpoint.extend(std::iter::repeat_n(level, hold));
        }
        setpoint.truncate(n);
        let gyro = (0..n)
            .map(|i| if i < delay { 0.0 } else { setpoint[i - delay] })
            .collect();
        (setpoint, gyro)
    }

    #[test]
    fn a_pure_delay_is_a_step() {
        let (setpoint, gyro) = delayed(4000, 20);
        let r = response(&setpoint, &gyro, 1001);
        assert_eq!(r.len(), 1001);
        // the sample before the step already feels it
        assert!(r[..19].iter().all(|v| v.abs() < 0.02), "{:?}", &r[..20]);
        // one window alone wanders around the true level
        assert!(r[20..].iter().all(|v| (v - 1.0).abs() < 0.2));
        assert!(r[20] > 0.85 && r[22] > 0.95, "{:?}", &r[20..23]);
        assert_eq!(settled_from(2000.0), 300);
        assert!(counts(&setpoint, &r, 300));
        // a stick near its centre does not count, whatever the response
        let quiet: Vec<f64> = setpoint.iter().map(|v| v / 10.0).collect();
        assert!(!counts(&quiet, &r, 300));
        // ... nor does a response that leaves 0.5 … 1.5 after 150 ms
        let mut low = r.clone();
        low[299] = 0.4;
        assert!(counts(&setpoint, &low, 300));
        low[300] = 0.4;
        assert!(!counts(&setpoint, &low, 300));
        // ... unless the limit starts later
        assert!(counts(&setpoint, &low, 301));
        assert!(response(&[1.0], &[1.0], 10).is_empty());
        assert!(!counts(&[], &r, 300));
    }

    #[test]
    fn the_step_of_a_delayed_axis() {
        let (setpoint, gyro) = delayed(24_000, 20);
        let s = step(&setpoint, &gyro, 2000.0, 0).unwrap();
        assert_eq!(s.grid.count, 79);
        assert_eq!(s.windows.len(), 79);
        assert_eq!(s.mean.len(), 1001);
        assert!((s.peak - 1.0).abs() < 0.03, "{}", s.peak);
        // the first sample above 0.5 is the 20th: 21 samples of 0.5 ms
        // less a millisecond
        assert!((s.latency_ms - 9.5).abs() < 1.0e-9, "{}", s.latency_ms);
        assert_eq!(s.latency_spread_ms, 0.0);
        assert!(s.peak_spread > 0.0 && s.peak_spread < 0.05);
        assert_eq!(sp_gyro_delay_ms(&setpoint, &gyro, 2000.0), Some(10.0));
        // smoothing blurs the step and leaves its place
        let high = step(&setpoint, &gyro, 2000.0, 3).unwrap();
        assert!(high.mean[19] > s.mean[19] + 0.2 && high.mean[21] < s.mean[21] - 0.1);
        assert!(step(&setpoint[..8000], &gyro[..8000], 2000.0, 0).is_none());
        // a level beyond the list is the last of it
        assert_eq!(step(&setpoint, &gyro, 2000.0, 9).unwrap(), high);
        // one sample later, another set of windows
        let later = step_from(&setpoint, &gyro, 2000.0, 0, 3).unwrap();
        assert_eq!(later.grid.first, 2003);
        assert_ne!(later.mean, s.mean);
    }

    #[test]
    fn a_log_without_stick_moves_has_no_curve() {
        let s = step(&vec![0.0; 24_000], &vec![0.0; 24_000], 2000.0, 0).unwrap();
        assert!(s.windows.is_empty());
        assert!(s.peak.is_nan() && s.latency_ms.is_nan());
        assert_eq!(sp_gyro_delay_ms(&[0.0; 10], &[0.0; 10], 2000.0), None);
    }
}
