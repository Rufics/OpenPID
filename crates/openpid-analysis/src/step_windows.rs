//! The step response of an axis from windows of the flight, one spectral
//! division per window ([`crate::StepMethod::Division`]).
//!
//! This is the rule found in the reference tool by comparing what it draws
//! for synthetic logs of known content (decision D-002, issue #172), on
//! arrays of samples:
//!
//! 1. the filtered gyro is smoothed by a local regression over 1, 5, 11 or
//!    21 samples ([`SMOOTHING`]); the setpoint is not;
//! 2. the samples are cut into windows of two seconds, a fortieth of a
//!    window (0.05 s) apart ([`Windows`]);
//! 3. in every window the setpoint and the gyro go through a Hann window
//!    and are followed by 200 zeros; the gyro's transform is divided by the
//!    setpoint's with a small constant in the denominator; the result, back
//!    in time, is summed up, and its first half second is the window's
//!    response ([`response`]);
//! 4. a window counts when the stick was off its centre by 20 deg/s or more
//!    on average ([`stick_moved`]), and when its response stays between 0.5
//!    and 1.5 from 150 ms on ([`settled`]);
//! 5. the curve is the mean of the windows that count, and the latency is
//!    read from the windows ([`read`]).
//!
//! The peak is this program's own (decision D-003): the highest value of the
//! whole curve, with the spread of the windows' highest values as its bar.
//! The reference tool reads the peak in the first 150 ms of the curve and
//! the bar among the first 150 samples of each window, so a response that
//! tops later than that reads lower than it gets.
//!
//! On plants whose step response is known in closed form the curve is the
//! true response to a few thousandths.

use openpid_dsp::fft::{Complex, Fft};
use openpid_dsp::smooth::local_line;
use std::f64::consts::TAU;

/// The smoothing levels, lowest first, with the samples of the regression
/// window of each: the same number of samples at every log rate.
pub const SMOOTHING: [(&str, usize); 4] = [("off", 1), ("low", 5), ("medium", 11), ("high", 21)];

/// Length of a window, seconds.
pub const WINDOW_S: f64 = 2.0;
/// Distance between the starts of two windows, seconds: a fortieth of a
/// window, as version 0.94 of the reference tool has it (0.9 had a
/// twentieth, and half the windows).
pub const HOP_S: f64 = 0.05;
/// Length of the response that is kept, seconds.
pub const SHOWN_S: f64 = 0.5;
/// The mean distance of the stick from its centre a window needs, deg/s.
pub const STICK_MEAN: f64 = 20.0;
/// Where a response has to stay from [`settled_from`] on.
pub const SETTLED: (f64, f64) = (0.5, 1.5);
/// Zeros that follow a window before its transform: the same number at
/// every log rate.
const PAD: usize = 200;
/// The constant in the denominator of the division, for transforms divided
/// by their length.
const FLOOR: f64 = 1.0e-4;
/// From here on a response has to stay inside [`SETTLED`], seconds.
const SETTLED_FROM_S: f64 = 0.15;
/// The level at which the latency of a window is read.
const LATENCY_LEVEL: f64 = 0.5;
/// The rate the setpoint and the gyro are brought to before their delay is
/// read, Hz.
const DELAY_RATE: f64 = 8000.0;
/// The longest delay looked for, seconds.
const DELAY_MAX_S: f64 = 0.2;

/// Where the windows of a stretch of samples lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windows {
    /// The first sample of the first window.
    pub first: usize,
    /// Samples in a window.
    pub length: usize,
    /// Samples between the starts of two windows.
    pub hop: usize,
    /// Number of windows.
    pub count: usize,
}

impl Windows {
    /// The windows of the `samples` samples that start at sample `first`,
    /// at `fs`. As the reference tool counts them, a stretch of exactly one
    /// window holds none: the count is the whole number of hops in what is
    /// left after one window.
    pub fn over(first: usize, samples: usize, fs: f64) -> Windows {
        let length = ((WINDOW_S * fs).round() as usize).max(2);
        let hop = ((HOP_S * fs).round() as usize).max(1);
        Windows {
            first,
            length,
            hop,
            count: samples.saturating_sub(length) / hop,
        }
    }

    /// The first sample of window `index`.
    pub fn start(&self, index: usize) -> usize {
        self.first + index * self.hop
    }
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

/// The first sample from which a response has to stay inside 0.5 … 1.5,
/// at `fs`: 150 ms.
pub fn settled_from(fs: f64) -> usize {
    (SETTLED_FROM_S * fs).round() as usize
}

/// Whether a `response` stays inside 0.5 … 1.5 from sample `from` on.
pub fn settled(response: &[f64], from: usize) -> bool {
    response.len() > from
        && response[from..]
            .iter()
            .all(|v| *v > SETTLED.0 && *v < SETTLED.1)
}

/// Samples of the response that is kept, at `fs`: half a second and its
/// end.
pub fn shown(fs: f64) -> usize {
    (SHOWN_S * fs).round() as usize + 1
}

/// A window and its response.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowResponse {
    /// Its place among the [`Windows`].
    pub index: usize,
    /// Its response, one value per sample from 0 to half a second.
    pub response: Vec<f64>,
}

/// The windows whose stick moved enough, each with its response, whether
/// the response keeps its limits or not. `setpoint` and `gyro` are the
/// samples the `windows` index; the gyro is smoothed at level `smoothing`
/// (0 for "off" … 3 for "high") first. `usable` says whether a window (by
/// the range of its samples) may be taken at all.
pub fn candidates(
    setpoint: &[f64],
    gyro: &[f64],
    fs: f64,
    smoothing: usize,
    windows: Windows,
    usable: impl Fn(std::ops::Range<usize>) -> bool,
) -> Vec<WindowResponse> {
    let len = setpoint.len().min(gyro.len());
    let span = SMOOTHING[smoothing.min(SMOOTHING.len() - 1)].1;
    let gyro = local_line(&gyro[..len], span);
    let points = shown(fs);
    let divider = Divider::new(windows.length);
    (0..windows.count)
        .filter_map(|index| {
            let at = windows.start(index)..windows.start(index) + windows.length;
            if at.end > len || !usable(at.clone()) || !stick_moved(&setpoint[at.clone()]) {
                return None;
            }
            Some(WindowResponse {
                index,
                response: divider.response(&setpoint[at.clone()], &gyro[at], points),
            })
        })
        .collect()
}

/// What is read from the windows that count.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The windows that count, in the order of the flight.
    pub windows: Vec<WindowResponse>,
    /// The curve: their mean, one value per sample.
    pub mean: Vec<f64>,
    /// The highest value of the curve, over all of its half second.
    pub peak: f64,
    /// The spread (sample standard deviation) over the windows of the
    /// highest value of each, over the same half second.
    pub peak_spread: f64,
    /// The mean over the windows of the time of the first sample above 0.5,
    /// counted from one and less a millisecond, ms.
    pub latency_ms: f64,
    /// The spread of those times, ms.
    pub latency_spread_ms: f64,
}

/// Mean and sample standard deviation; `NaN` and zero for no values.
fn spread(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if values.len() < 2 {
        return (mean, 0.0);
    }
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, var.sqrt())
}

/// The [`Reading`] of the windows of [`candidates`]: those whose response
/// stays inside 0.5 … 1.5 from sample `from` on count.
pub fn read(mut windows: Vec<WindowResponse>, fs: f64, from: usize) -> Reading {
    windows.retain(|w| settled(&w.response, from));
    let points = shown(fs);
    let n = windows.len() as f64;
    let mean: Vec<f64> = (0..points)
        .map(|i| windows.iter().map(|w| w.response[i]).sum::<f64>() / n)
        .collect();
    let peak = mean.iter().copied().fold(f64::NAN, f64::max);
    let highest: Vec<f64> = windows
        .iter()
        .map(|w| w.response.iter().copied().fold(f64::MIN, f64::max))
        .collect();
    let times: Vec<f64> = windows
        .iter()
        .filter_map(|w| w.response.iter().position(|v| *v > LATENCY_LEVEL))
        .map(|i| (i + 1) as f64 * 1000.0 / fs - 1.0)
        .collect();
    let (_, peak_spread) = spread(&highest);
    let (latency_ms, latency_spread_ms) = spread(&times);
    Reading {
        windows,
        mean,
        peak,
        peak_spread,
        latency_ms,
        latency_spread_ms,
    }
}

/// The delay between a setpoint and a gyro, ms: both brought to 8 kHz
/// along straight lines between the samples, and the shift at which the
/// two agree best (the largest sum of products), up to 200 ms. `None` for
/// fewer than two samples.
///
/// It gave the reference tool's "SP-Gyro Delay" on 39 of 47 plots of
/// synthetic logs and was one step of 0.125 ms off on seven more; a part
/// of the rule is still missing.
pub fn delay_ms(setpoint: &[f64], gyro: &[f64], fs: f64) -> Option<f64> {
    let len = setpoint.len().min(gyro.len());
    if len < 2 {
        return None;
    }
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
    let setpoint = fine(&setpoint[..len]);
    let gyro = fine(&gyro[..len]);
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
    fn the_windows_of_a_stretch() {
        // 30 s at 2 kHz less its first and last second and one sample
        let w = Windows::over(2002, 55_999, 2000.0);
        assert_eq!((w.first, w.length, w.hop, w.count), (2002, 4000, 100, 519));
        assert_eq!(w.start(518), 53_802);
        // the same seconds at other rates give the same number of windows
        assert_eq!(Windows::over(1002, 27_999, 1000.0).count, 519);
        assert_eq!(Windows::over(4002, 111_999, 4000.0).count, 519);
        // exactly one window, or less, holds none
        assert_eq!(Windows::over(0, 4000, 2000.0).count, 0);
        assert_eq!(Windows::over(0, 100, 2000.0).count, 0);
        assert_eq!(Windows::over(0, 4100, 2000.0).count, 1);
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
        assert_eq!(shown(2000.0), 1001);
        assert!(stick_moved(&setpoint) && settled(&r, 300));
        // a stick near its centre does not count
        let quiet: Vec<f64> = setpoint.iter().map(|v| v / 10.0).collect();
        assert!(!stick_moved(&quiet) && !stick_moved(&[]));
        // ... nor does a response that leaves 0.5 … 1.5 after 150 ms
        let mut low = r.clone();
        low[299] = 0.4;
        assert!(settled(&low, 300));
        low[300] = 0.4;
        assert!(!settled(&low, 300) && settled(&low, 301));
        assert!(!settled(&low[..200], 300));
        assert!(response(&[1.0], &[1.0], 10).is_empty());
    }

    /// The gyro of a second-order plant (natural frequency `hz`, damping
    /// 0.4) behind `delay` samples, for a stick held between two samples;
    /// and the plant's response to a unit step, seconds after the delay.
    fn second_order(setpoint: &[f64], fs: f64, hz: f64, delay: usize) -> Vec<f64> {
        let (w, steps) = (TAU * hz, 64);
        let h = 1.0 / fs / f64::from(steps);
        let (mut y, mut v) = (0.0, 0.0);
        (0..setpoint.len())
            .map(|i| {
                let u = if i > delay {
                    setpoint[i - 1 - delay]
                } else {
                    0.0
                };
                for _ in 0..steps {
                    v += h * (w * w * (u - y) - 2.0 * 0.4 * w * v);
                    y += h * v;
                }
                y
            })
            .collect()
    }
    fn second_order_step(hz: f64, t: f64) -> f64 {
        let (w, z) = (TAU * hz, 0.4_f64);
        let wd = w * (1.0 - z * z).sqrt();
        1.0 - (-z * w * t).exp() * ((wd * t).cos() + z * w / wd * (wd * t).sin())
    }

    #[test]
    fn a_second_order_plant_gives_its_step_response() {
        let fs = 2000.0;
        let (setpoint, _) = delayed(40_000, 0);
        let windows = Windows::over(2, 39_998, fs);
        // 15 Hz behind 5 ms: the curve is the plant's, overshoot and all
        let gyro = second_order(&setpoint, fs, 15.0, 10);
        let all = candidates(&setpoint, &gyro, fs, 0, windows, |_| true);
        let r = read(all, fs, settled_from(fs));
        assert!(r.windows.len() > 170, "{}", r.windows.len());
        for i in (10..1000).step_by(10) {
            let truth = second_order_step(15.0, (i - 10) as f64 / fs);
            // the late part of the curve sinks by a hundredth: the Hann
            // window weighs the two signals by their place in the window
            assert!(
                (r.mean[i] - truth).abs() < 0.02,
                "{i}: {} {truth}",
                r.mean[i]
            );
        }
        // the true peak is 1 + exp(−π·ζ/√(1 − ζ²)) = 1.2538, the true
        // half-way time 18.11 ms
        assert!((r.peak - 1.2538).abs() < 0.005, "{}", r.peak);
        assert!((r.latency_ms - 18.11).abs() < 0.6, "{}", r.latency_ms);
        assert!(
            r.peak_spread > 0.0 && r.peak_spread < 0.1,
            "{}",
            r.peak_spread
        );
        assert!(r.latency_spread_ms < 1.0, "{}", r.latency_spread_ms);
        // 2.5 Hz: the plant tops at 223.7 ms, and the peak is read at the
        // curve's top, not in the first 150 ms, where it is still rising
        let gyro = second_order(&setpoint, fs, 2.5, 10);
        let all = candidates(&setpoint, &gyro, fs, 0, windows, |_| true);
        let r = read(all, fs, settled_from(fs));
        assert!(r.windows.len() > 150, "{}", r.windows.len());
        let top = r.mean.iter().copied().fold(f64::MIN, f64::max);
        // a response this slow is read low by the method: 1.19 where the
        // plant overshoots to 1.25
        assert!(top > 1.15 && top < 1.2538, "{top}");
        assert_eq!(r.peak, top);
        let at = r.mean.iter().position(|v| *v == top).unwrap_or(0);
        // the curve of a response this slow tops some 10 ms early
        assert!((427..=447).contains(&at), "{at}");
        assert!(r.peak > r.mean[299] + 0.1, "{} {}", r.peak, r.mean[299]);
        // the windows of a response this slow differ more than those of a
        // quick one: their highest values spread by 0.14
        assert!(
            r.peak_spread > 0.1 && r.peak_spread < 0.2,
            "{}",
            r.peak_spread
        );
    }

    #[test]
    fn the_reading_of_a_delayed_axis() {
        let (setpoint, gyro) = delayed(20_000, 20);
        let fs = 2000.0;
        let windows = Windows::over(2, 19_998, fs);
        assert_eq!(windows.count, 159);
        let all = candidates(&setpoint, &gyro, fs, 0, windows, |_| true);
        assert_eq!(all.len(), 159);
        let r = read(all.clone(), fs, settled_from(fs));
        assert_eq!((r.windows.len(), r.mean.len()), (159, 1001));
        assert!((r.peak - 1.0).abs() < 0.03, "{}", r.peak);
        // the first sample above 0.5 is the 20th: 21 samples of 0.5 ms
        // less a millisecond
        assert!((r.latency_ms - 9.5).abs() < 1.0e-9, "{}", r.latency_ms);
        assert_eq!(r.latency_spread_ms, 0.0);
        assert!(r.peak_spread > 0.0 && r.peak_spread < 0.05);
        assert_eq!(delay_ms(&setpoint, &gyro, fs), Some(10.0));
        assert_eq!(delay_ms(&[1.0], &[1.0], fs), None);
        // smoothing blurs the step and leaves its place
        let smoothed = candidates(&setpoint, &gyro, fs, 3, windows, |_| true);
        let high = read(smoothed, fs, settled_from(fs));
        assert!(high.mean[19] > r.mean[19] + 0.2 && high.mean[21] < r.mean[21] - 0.1);
        // a level beyond the list is the last of it
        let beyond = candidates(&setpoint, &gyro, fs, 9, windows, |_| true);
        assert_eq!(read(beyond, fs, settled_from(fs)), high);
        // windows that may not be taken are left out, and so are windows
        // that reach beyond the samples
        let half = candidates(&setpoint, &gyro, fs, 0, windows, |at| at.start < 8000);
        assert_eq!(half.len(), 80);
        let short = candidates(&setpoint[..10_000], &gyro, fs, 0, windows, |_| true);
        assert_eq!(short.len(), 60);
        // no window: no curve
        let none = read(Vec::new(), fs, 300);
        assert!(none.peak.is_nan() && none.latency_ms.is_nan());
        assert_eq!((none.peak_spread, none.latency_spread_ms), (0.0, 0.0));
        // one window has no spread
        let one = read(all[..1].to_vec(), fs, 300);
        assert_eq!((one.windows.len(), one.peak_spread), (1, 0.0));
    }
}
