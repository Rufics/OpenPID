//! The step response of an axis from windows of the flight, one spectral
//! division per window ([`crate::StepMethod::Division`]).
//!
//! This is the rule of the web edition 1.0 of the reference tool (decision
//! D-015), found by comparing what it draws for synthetic logs of known
//! content; version 0.94 had the same rule without the padding to a power
//! of two and without the distance to the median (decision D-002, issue
//! #172). On arrays of samples:
//!
//! 1. the filtered gyro is smoothed by a local regression over 1, 5, 11 or
//!    21 samples ([`SMOOTHING`]); the setpoint is not;
//! 2. the samples are cut into windows of two seconds, a fortieth of a
//!    window (0.05 s) apart, from the first sample on ([`Windows`]);
//! 3. in every window the setpoint and the gyro go through a Hann window,
//!    are followed by 200 zeros and padded with more zeros to a power of
//!    two ([`padded_length`]); the gyro's transform is divided by the
//!    setpoint's with a small constant in the denominator; the result, back
//!    in time, is summed up, and its first half second is the window's
//!    response ([`response`]);
//! 4. a window counts when the stick was off its centre by 20 deg/s or more
//!    on average ([`stick_moved`]), when its response stays between 0.5
//!    and 1.5 from 150 ms on ([`settled`]), and when the mean of that part
//!    of its response lies within three scaled median absolute deviations
//!    of the median over the windows that passed so far ([`near_median`]);
//! 5. the curve is the mean of the windows that count, and the latency is
//!    read from the windows ([`read`]).
//!
//! The peak is this program's own (decision D-003): the highest value of the
//! whole curve, with the spread of the windows' highest values as its bar.
//! The reference tool reads the peak in the first 150 ms of the curve and
//! the bar over the first 150 ms of each window (version 0.94: its first
//! 150 samples), so a response that tops later than that reads lower than
//! it gets.
//!
//! On plants whose step response is known in closed form the curve is the
//! true response to a few thousandths.

use openpid_dsp::fft::{Complex, Fft};
use openpid_dsp::smooth::local_line;
use std::f64::consts::TAU;

/// The smoothing levels, lowest first, with the samples of the regression
/// window of each: the same number of samples at every log rate.
pub const SMOOTHING: [(&str, usize); 4] = [("off", 1), ("low", 5), ("medium", 11), ("high", 21)];

/// Windows per window length between the starts of two windows: a window
/// starts a fortieth of a window after the one before, as version 0.94 of
/// the reference tool has it at every window length (0.9 had a twentieth,
/// and half the windows).
pub const HOPS_PER_WINDOW: f64 = 40.0;
/// Length of the response that is kept, seconds.
pub const SHOWN_S: f64 = 0.5;
/// The mean distance of the stick from its centre a window needs, deg/s.
pub const STICK_MEAN: f64 = 20.0;
/// The width of the band around 1 a response has to stay in from
/// [`settled_from`] on: 0.5 … 1.5 ("SS width" in the reference tool).
pub const SETTLED_WIDTH: f64 = 1.0;
/// How many scaled median absolute deviations the mean of a settled
/// response may lie from the median of them ("Dist. to median").
pub const MEDIAN_DISTANCE: f64 = 3.0;
/// The median absolute deviation times this is the standard deviation of a
/// normal distribution.
const MAD_SCALE: f64 = 1.4826;
/// Zeros that follow a window before its transform: the same number at
/// every log rate.
const PAD: usize = 200;
/// The constant in the denominator of the division, for transforms divided
/// by the length of a window and its 200 zeros.
const FLOOR: f64 = 1.0e-4;
/// From here on a response has to stay inside the band of [`SETTLED_WIDTH`]
/// around 1, seconds.
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
    /// The windows of `window_s` seconds of the `samples` samples that
    /// start at sample `first`, at `fs`, a fortieth of a window apart: the
    /// first starts at the first sample, and every window that ends at the
    /// last sample or before it counts. (Version 0.94 of the reference tool
    /// had one window fewer: a stretch of exactly one window held none.)
    pub fn over(first: usize, samples: usize, fs: f64, window_s: f64) -> Windows {
        let length = ((window_s * fs).round() as usize).max(2);
        let hop = ((window_s / HOPS_PER_WINDOW * fs).round() as usize).max(1);
        let count = if samples < length {
            0
        } else {
            (samples - length) / hop + 1
        };
        Windows {
            first,
            length,
            hop,
            count,
        }
    }

    /// The first sample of window `index`.
    pub fn start(&self, index: usize) -> usize {
        self.first + index * self.hop
    }
}

/// The length of the transforms of a window of `length` samples: the
/// window and its 200 zeros, padded with more zeros to the next power of
/// two (the web edition of the reference tool; version 0.94 took the window
/// and its 200 zeros alone).
pub fn padded_length(length: usize) -> usize {
    (length + PAD).next_power_of_two()
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

    /// The transform of a window through the Hann window, zeros after it
    /// up to [`padded_length`].
    fn through(&self, x: &[f64]) -> Vec<Complex<f64>> {
        let mut buf = vec![Complex::new(0.0, 0.0); padded_length(self.hann.len())];
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
        // the transforms are divided by the window and its 200 zeros, not
        // by their padded length
        let divisor = (self.hann.len() + PAD) as f64;
        let floor = FLOOR * divisor * divisor;
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

/// Whether a `response` stays inside `1 ± width/2` (both ends included)
/// from sample `from` on.
pub fn settled(response: &[f64], from: usize, width: f64) -> bool {
    let band = 1.0 - width / 2.0..=1.0 + width / 2.0;
    response.len() > from && response[from..].iter().all(|v| band.contains(v))
}

/// The mean of a `response` from sample `from` on: where it settles.
fn settled_mean(response: &[f64], from: usize) -> f64 {
    let tail = &response[from.min(response.len())..];
    tail.iter().sum::<f64>() / tail.len() as f64
}

/// The median of `values`: the mean of the two middle ones for an even
/// number; `NaN` for none.
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    match n {
        0 => f64::NAN,
        _ if n % 2 == 1 => sorted[n / 2],
        _ => f64::midpoint(sorted[n / 2 - 1], sorted[n / 2]),
    }
}

/// Which of the `windows` lie near the others ("Dist. to median"): the
/// mean of each response from sample `from` on, `v`, within `distance`
/// times the scaled median absolute deviation, `1.4826·median|v − m|`, of
/// their median `m`.
pub fn near_median(windows: &[WindowResponse], from: usize, distance: f64) -> Vec<bool> {
    let means: Vec<f64> = windows
        .iter()
        .map(|w| settled_mean(&w.response, from))
        .collect();
    let middle = median(&means);
    let deviations: Vec<f64> = means.iter().map(|v| (v - middle).abs()).collect();
    let limit = distance * MAD_SCALE * median(&deviations);
    deviations.iter().map(|d| *d <= limit).collect()
}

/// Which windows count: the limits of a settled response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acceptance {
    /// The first sample from which a response has to stay settled
    /// ([`settled_from`]).
    pub from: usize,
    /// The width of the band around 1 it has to stay in ([`settled`]).
    pub width: f64,
    /// How far from the median of the others the mean of a settled
    /// response may lie, in scaled median absolute deviations
    /// ([`near_median`]); `None` for no such test.
    pub median_distance: Option<f64>,
}

impl Acceptance {
    /// The web edition's limits at `fs`: from 150 ms, 0.5 … 1.5, within
    /// three deviations of the median.
    pub fn at(fs: f64) -> Acceptance {
        Acceptance {
            from: settled_from(fs),
            width: SETTLED_WIDTH,
            median_distance: Some(MEDIAN_DISTANCE),
        }
    }

    /// The `windows` that keep these limits, in their order.
    pub fn keep(&self, mut windows: Vec<WindowResponse>) -> Vec<WindowResponse> {
        windows.retain(|w| settled(&w.response, self.from, self.width));
        if let Some(distance) = self.median_distance {
            let near = near_median(&windows, self.from, distance);
            let mut near = near.into_iter();
            windows.retain(|_| near.next().unwrap_or(false));
        }
        windows
    }
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

/// The [`Reading`] of the windows of [`candidates`]: those that keep the
/// limits of `acceptance` count.
pub fn read(windows: Vec<WindowResponse>, fs: f64, acceptance: &Acceptance) -> Reading {
    let windows = acceptance.keep(windows);
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
        // the web edition's stretch of a 30 s log at 2 kHz: samples 2 000 to
        // 58 000, both included
        let w = Windows::over(2000, 56_001, 2000.0, 2.0);
        assert_eq!((w.first, w.length, w.hop, w.count), (2000, 4000, 100, 521));
        // the last window ends at the stretch's last sample or before it,
        // the next would end after it
        assert_eq!(w.start(520) + w.length - 1, 57_999);
        assert!(w.start(521) + w.length - 1 > 58_000);
        // the same seconds at other rates give the same number of windows
        assert_eq!(Windows::over(1000, 28_001, 1000.0, 2.0).count, 521);
        assert_eq!(Windows::over(4000, 112_001, 4000.0, 2.0).count, 521);
        // exactly one window holds one, less holds none
        assert_eq!(Windows::over(0, 4000, 2000.0, 2.0).count, 1);
        assert_eq!(Windows::over(0, 3999, 2000.0, 2.0).count, 0);
        assert_eq!(Windows::over(0, 100, 2000.0, 2.0).count, 0);
        assert_eq!(Windows::over(0, 4100, 2000.0, 2.0).count, 2);
        // windows of 2, 4 and 5 s, a fortieth of their length apart, on the
        // 38 s stretch of a 40 s log: 721, 341 and 265, of which the web
        // edition accepted 721, 341 and 264 on roll (round W2)
        let w = Windows::over(2000, 76_001, 2000.0, 2.0);
        assert_eq!((w.length, w.hop, w.count), (4000, 100, 721));
        let w = Windows::over(2000, 76_001, 2000.0, 4.0);
        assert_eq!((w.length, w.hop, w.count), (8000, 200, 341));
        let w = Windows::over(2000, 76_001, 2000.0, 5.0);
        assert_eq!((w.length, w.hop, w.count), (10_000, 250, 265));
    }

    #[test]
    fn the_transforms_are_padded_to_a_power_of_two() {
        // a window, its 200 zeros, and zeros up to the next power of two
        assert_eq!(padded_length(4000), 8192);
        assert_eq!(padded_length(3896), 4096);
        assert_eq!(padded_length(3897), 8192);
        assert_eq!(padded_length(2000), 4096);
        assert_eq!(padded_length(8000), 16_384);
        assert_eq!(padded_length(10_000), 16_384);
        assert_eq!(padded_length(20_000), 32_768);
        let divider = Divider::new(4000);
        assert_eq!(divider.through(&vec![1.0; 4000]).len(), 8192);
    }

    #[test]
    fn the_padding_changes_the_response_and_not_its_level() {
        // the response of a window of 3 900 samples, whose 200 zeros make
        // 4 100, against the same window cut to 3 896, whose 200 zeros make
        // 4 096: both are transformed at a power of two (8 192 and 4 096)
        let (setpoint, gyro) = delayed(3900, 20);
        let long = response(&setpoint, &gyro, 1001);
        let short = response(&setpoint[..3896], &gyro[..3896], 1001);
        for r in [&long, &short] {
            assert!(r[..19].iter().all(|v| v.abs() < 0.03), "{:?}", &r[..20]);
            assert!(r[22..].iter().all(|v| (v - 1.0).abs() < 0.2));
        }
        // a window of 4 000 samples, transformed at 8 192 points, against
        // the transform over its 4 200 samples alone (version 0.94): the
        // same response to a few thousandths, not to the last digit
        let (setpoint, gyro) = delayed(4000, 20);
        let padded = response(&setpoint, &gyro, 1001);
        let unpadded = unpadded_response(&setpoint, &gyro, 1001);
        let largest = padded
            .iter()
            .zip(&unpadded)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(largest > 1.0e-6 && largest < 0.05, "{largest}");
    }

    /// The response of a window transformed over the window and its 200
    /// zeros alone, as version 0.94 of the reference tool had it.
    fn unpadded_response(setpoint: &[f64], gyro: &[f64], points: usize) -> Vec<f64> {
        let fft = Fft::new();
        let hann = Divider::new(setpoint.len()).hann;
        let through = |x: &[f64]| {
            let mut buf = vec![Complex::new(0.0, 0.0); x.len() + PAD];
            for ((slot, v), w) in buf.iter_mut().zip(x).zip(&hann) {
                slot.re = v * w;
            }
            fft.forward_inplace(&mut buf);
            buf
        };
        let input = through(setpoint);
        let mut output = through(gyro);
        let floor = FLOOR * (input.len() * input.len()) as f64;
        for (y, x) in output.iter_mut().zip(&input) {
            *y = *y * x.conj() / (x.norm_sqr() + floor);
        }
        fft.inverse_inplace(&mut output);
        output
            .iter()
            .take(points)
            .scan(0.0, |sum, h| {
                *sum += h.re;
                Some(*sum)
            })
            .collect()
    }

    /// A window whose response is `level` from the start.
    fn flat(index: usize, level: f64) -> WindowResponse {
        WindowResponse {
            index,
            response: vec![level; 1001],
        }
    }

    #[test]
    fn a_window_far_from_the_median_is_left_out() {
        // nine windows settle near 1, one at 1.3: within 0.5 … 1.5, but
        // far from the others
        let mut windows: Vec<WindowResponse> = (0..9)
            .map(|i| flat(i, 1.0 + 0.01 * (i as f64 - 4.0)))
            .collect();
        windows.push(flat(9, 1.3));
        // the median is 1.005, the deviations 0.005 … 0.045 and 0.295,
        // their median 0.025: the limit is 3 · 1.4826 · 0.025 = 0.111
        let near = near_median(&windows, 300, 3.0);
        assert_eq!(near.iter().filter(|n| **n).count(), 9);
        assert!(!near[9]);
        // the median of an even number of values is the mean of the middle
        // two
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), 2.5);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert!(median(&[]).is_nan());
        let fs = 2000.0;
        let on = read(windows.clone(), fs, &Acceptance::at(fs));
        assert_eq!(on.windows.len(), 9);
        assert!(on.windows.iter().all(|w| w.index < 9));
        assert!((on.mean[500] - 1.0).abs() < 1.0e-12, "{}", on.mean[500]);
        // without the test the window counts, and pulls the mean up
        let off = Acceptance {
            median_distance: None,
            ..Acceptance::at(fs)
        };
        let all = read(windows.clone(), fs, &off);
        assert_eq!(all.windows.len(), 10);
        assert!((all.mean[500] - 1.03).abs() < 1.0e-12, "{}", all.mean[500]);
        // the test is over the windows that settled: a window that leaves
        // the band does not move the median
        windows.push(flat(10, 1.6));
        windows.push(flat(11, 1.7));
        assert_eq!(
            read(windows.clone(), fs, &Acceptance::at(fs)).windows.len(),
            9
        );
        // a wider band takes them in, and then the median test leaves out
        // all three
        let wide = Acceptance {
            width: 2.0,
            ..Acceptance::at(fs)
        };
        assert_eq!(read(windows.clone(), fs, &wide).windows.len(), 9);
        let wide = Acceptance {
            median_distance: None,
            ..wide
        };
        assert_eq!(read(windows, fs, &wide).windows.len(), 12);
        // one window is its own median
        let one = near_median(&[flat(0, 1.2)], 300, 3.0);
        assert_eq!(one, vec![true]);
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
        assert!(stick_moved(&setpoint) && settled(&r, 300, 1.0));
        // a stick near its centre does not count
        let quiet: Vec<f64> = setpoint.iter().map(|v| v / 10.0).collect();
        assert!(!stick_moved(&quiet) && !stick_moved(&[]));
        // ... nor does a response that leaves 0.5 … 1.5 after 150 ms
        let mut low = r.clone();
        low[299] = 0.4;
        assert!(settled(&low, 300, 1.0));
        low[300] = 0.4;
        assert!(!settled(&low, 300, 1.0) && settled(&low, 301, 1.0));
        assert!(!settled(&low[..200], 300, 1.0));
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
        let windows = Windows::over(2, 39_998, fs, 2.0);
        // 15 Hz behind 5 ms: the curve is the plant's, overshoot and all
        let gyro = second_order(&setpoint, fs, 15.0, 10);
        let all = candidates(&setpoint, &gyro, fs, 0, windows, |_| true);
        let r = read(all, fs, &Acceptance::at(fs));
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
        let r = read(all, fs, &Acceptance::at(fs));
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
        let windows = Windows::over(2, 19_998, fs, 2.0);
        assert_eq!(windows.count, 160);
        let all = candidates(&setpoint, &gyro, fs, 0, windows, |_| true);
        assert_eq!(all.len(), 160);
        let r = read(all.clone(), fs, &Acceptance::at(fs));
        // every response settles, and those of eleven windows settle a
        // little farther from the others than three deviations
        assert_eq!((r.windows.len(), r.mean.len()), (149, 1001));
        let off = Acceptance {
            median_distance: None,
            ..Acceptance::at(fs)
        };
        assert_eq!(read(all.clone(), fs, &off).windows.len(), 160);
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
        let high = read(smoothed, fs, &Acceptance::at(fs));
        assert!(high.mean[19] > r.mean[19] + 0.2 && high.mean[21] < r.mean[21] - 0.1);
        // a level beyond the list is the last of it
        let beyond = candidates(&setpoint, &gyro, fs, 9, windows, |_| true);
        assert_eq!(read(beyond, fs, &Acceptance::at(fs)), high);
        // windows that may not be taken are left out, and so are windows
        // that reach beyond the samples
        let half = candidates(&setpoint, &gyro, fs, 0, windows, |at| at.start < 8000);
        assert_eq!(half.len(), 80);
        let short = candidates(&setpoint[..10_000], &gyro, fs, 0, windows, |_| true);
        assert_eq!(short.len(), 60);
        // no window: no curve
        let none = read(Vec::new(), fs, &Acceptance::at(fs));
        assert!(none.peak.is_nan() && none.latency_ms.is_nan());
        assert_eq!((none.peak_spread, none.latency_spread_ms), (0.0, 0.0));
        // one window has no spread
        let one = read(all[..1].to_vec(), fs, &Acceptance::at(fs));
        assert_eq!((one.windows.len(), one.peak_spread), (1, 0.0));
    }
}
