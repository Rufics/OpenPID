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
//! Since decision D-002 the computation itself is `OpenPID`'s own
//! (`openpid_analysis::step_windows`); what is left here is what belongs
//! to the tool and not to the method: where its windows start in a log,
//! and which stretch its delay is read from. The tests of this crate thus
//! hold `OpenPID`'s code to what the tool drew.
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
use openpid_analysis::step_windows::{self, Reading, Windows};

pub use openpid_analysis::step_windows::{
    SMOOTHING, WindowResponse as Window, response, settled, settled_from, stick_moved,
};

/// Samples by which the first window starts after the first second for
/// most logs. For one log of round 4 (`t9-short-2k`, 12 s) it was three.
pub const LATE: usize = 2;

/// Where the windows of a log lie.
pub type Grid = Windows;

/// The windows of a log of `len` samples at `fs` whose first window starts
/// `late` samples after the first second, or `None` when the log holds no
/// window: the span of the spectrum (the log less two seconds and one
/// sample), cut as `OpenPID` cuts an epoch.
pub fn grid(len: usize, fs: f64, late: usize) -> Option<Grid> {
    let second = fs.round() as usize;
    let span = len.checked_sub(2 * second + 1)?;
    let grid = Windows::over(second + late, span, fs);
    (grid.count > 0 && grid.start(grid.count - 1) + grid.length <= len).then_some(grid)
}

/// Whether a window counts: `setpoint` are its samples, `response` its
/// response, which has to stay inside 0.5 … 1.5 from sample `from` on.
pub fn counts(setpoint: &[f64], response: &[f64], from: usize) -> bool {
    stick_moved(setpoint) && settled(response, from)
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
    let grid = grid(setpoint.len().min(gyro.len()), fs, late)?;
    let windows = step_windows::candidates(setpoint, gyro, fs, smoothing, grid, |_| true);
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
pub fn read(grid: Grid, windows: Vec<Window>, fs: f64, from: usize) -> Step {
    let Reading {
        windows,
        mean,
        peak,
        peak_spread,
        latency_ms,
        latency_spread_ms,
    } = step_windows::read(windows, fs, from);
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
    step_windows::delay_ms(&setpoint[span.clone()], &gyro[span], fs)
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

    #[test]
    fn a_log_without_stick_moves_has_no_curve() {
        let still = vec![0.0; 24_000];
        let s = step(&still, &still, 2000.0, 0).unwrap();
        assert!(s.windows.is_empty());
        assert!(s.peak.is_nan() && s.latency_ms.is_nan());
        assert!(!counts(&still[..4000], &[1.0; 1001], 300));
        assert_eq!(sp_gyro_delay_ms(&[0.0; 10], &[0.0; 10], 2000.0), None);
        assert!(step(&still[..8000], &still[..8000], 2000.0, 0).is_none());
        // one sample later, the same windows of a still log
        let later = step_from(&still, &still, 2000.0, 0, 3).unwrap();
        assert_eq!((later.grid.first, later.grid.count), (2003, 79));
    }
}
