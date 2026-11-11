//! The curve the reference tool draws as the spectrum of a channel, as the
//! calibration rounds found it (`docs/calibration/spectrum.md`).
//!
//! It is a model of observed behaviour: what the tool showed for probe logs
//! of known content, read from plots it copies as vector graphics. On the
//! points of `tests/calibration/spectrum/` it is exact to the precision of
//! those copies. A round's predictions are made with it before the owner
//! runs the round, so that the round confirms the model or corrects it.
//!
//! The steps:
//! 1. the log without its first and last second, one sample short and
//!    starting three samples late ([`span`]);
//! 2. one transform of that span through a symmetric Hann window;
//! 3. `2·|X|² / (fs·N)` in decibels, the point at 0 Hz not doubled
//!    ([`raw_db`]);
//! 4. a local regression of the decibel values: at every point a line is
//!    fitted to the nearest points with tricube weights ([`smooth`]).

use openpid_dsp::fft::Fft;
use std::f64::consts::TAU;
use std::ops::Range;

/// The smoothing levels of the tool's spectrum window with the number of
/// points of each regression window. "medium", the tool's default, is exact;
/// the other four were fitted to the pixels of screenshots and are the
/// hypothesis round 2 tests.
pub const LEVELS: [(&str, usize); 5] = [
    ("low", 5),
    ("low-med", 35),
    ("medium", 121),
    ("med-high", 285),
    ("high", 553),
];

/// Seconds the tool leaves out at each end of a log.
const TRIM_S: f64 = 1.0;
/// Samples by which the analysed span starts after the first second.
const LATE: usize = 3;

/// The samples of a log of `len` samples at `fs` that go into the spectrum,
/// or `None` when the log is too short to have any.
pub fn span(len: usize, fs: f64) -> Option<Range<usize>> {
    let trim = (TRIM_S * fs).round() as usize;
    let count = len.checked_sub(2 * trim + 1)?;
    let start = trim + LATE;
    (count > 1 && start + count <= len).then(|| start..start + count)
}

/// The transform of `x` as the tool scales it, decibels: one value per
/// point of the one-sided spectrum, `fs / x.len()` apart.
pub fn raw_db(x: &[f64], fs: f64) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return Vec::new();
    }
    let windowed: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, v)| v * (0.5 - 0.5 * (TAU * i as f64 / (n - 1) as f64).cos()))
        .collect();
    let scale = fs * n as f64;
    Fft::new()
        .rfft(&windowed)
        .iter()
        .enumerate()
        .map(|(k, bin)| {
            let sides = if k == 0 { 1.0 } else { 2.0 };
            // a point of no power at all stays a number
            10.0 * (sides * bin.norm_sqr() / scale).max(1.0e-300).log10()
        })
        .collect()
}

/// The tricube weight at a distance of `d` window half-widths.
fn tricube(d: f64) -> f64 {
    let d = d.abs().min(1.0);
    (1.0 - d * d * d).powi(3)
}

/// `values` smoothed by a local regression over `window` points: at every
/// point the value of the line fitted to the `window` nearest points, the
/// weights tricube and scaled so that the farthest of those points weighs
/// nothing. Away from the ends the window is centred and the fit is a
/// weighted mean; at an end the window stays inside the data and the line
/// carries the slope.
///
/// A `window` below three, or values shorter than the window, come back
/// unchanged.
pub fn smooth(values: &[f64], window: usize) -> Vec<f64> {
    let n = values.len();
    let window = window | 1;
    if window < 3 || n < window {
        return values.to_vec();
    }
    let half = window / 2;
    let kernel: Vec<f64> = (0..window)
        .map(|j| tricube((j as f64 - half as f64) / half as f64))
        .collect();
    let total: f64 = kernel.iter().sum();
    let mut out = vec![0.0; n];
    for (i, value) in out.iter_mut().enumerate().take(n - half).skip(half) {
        let slice = &values[i - half..=i + half];
        *value = slice.iter().zip(&kernel).map(|(v, w)| v * w).sum::<f64>() / total;
    }
    let mut end = |i: usize| {
        let first = i.saturating_sub(half).min(n - window);
        let reach = (i - first).max(first + window - 1 - i) as f64;
        // weighted least squares of v = a + b·(j − i): `a` is the value at i
        let (mut sw, mut sx, mut sxx, mut sv, mut sxv) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (j, v) in values.iter().enumerate().skip(first).take(window) {
            let x = j as f64 - i as f64;
            let w = tricube(x / reach);
            sw += w;
            sx += w * x;
            sxx += w * x * x;
            sv += w * v;
            sxv += w * x * v;
        }
        let det = sw * sxx - sx * sx;
        out[i] = if det.abs() > f64::EPSILON * sw * sxx {
            (sxx * sv - sx * sxv) / det
        } else {
            sv / sw
        };
    };
    for i in (0..half).chain(n - half..n) {
        end(i);
    }
    out
}

/// The tool's curve for one channel of a log.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// The samples of the log that were analysed.
    pub span: Range<usize>,
    /// Distance between two points, Hz.
    pub bin_hz: f64,
    /// The level at every point from 0 Hz to half the log rate, dB.
    pub db: Vec<f64>,
}

impl Curve {
    /// The level at the point nearest to `hz`, or `None` beyond the curve.
    pub fn at(&self, hz: f64) -> Option<f64> {
        let k = (hz / self.bin_hz).round();
        if k < 0.0 {
            return None;
        }
        self.db.get(k as usize).copied()
    }
}

/// The curve for the `logged` samples of a channel (the values a log holds,
/// from its first frame to its last) at a regression window of `window`
/// points, or `None` for a log too short to analyse.
pub fn curve(logged: &[f64], fs: f64, window: usize) -> Option<Curve> {
    let span = span(logged.len(), fs)?;
    let raw = raw_db(&logged[span.clone()], fs);
    Some(Curve {
        bin_hz: fs / span.len() as f64,
        db: smooth(&raw, window),
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_span_is_the_log_without_a_second_at_each_end_one_sample_short() {
        assert_eq!(span(60_000, 2000.0), Some(2003..58_002));
        assert_eq!(span(30_000, 1000.0), Some(1003..29_002));
        assert_eq!(span(24_000, 2000.0).map(|s| s.len()), Some(19_999));
        assert_eq!(span(4000, 2000.0), None);
        assert_eq!(span(0, 2000.0), None);
    }

    #[test]
    fn a_line_is_left_as_it_is_ends_included() {
        let line: Vec<f64> = (0..400).map(|i| 3.0 - 0.25 * f64::from(i)).collect();
        for (_, window) in LEVELS {
            if window > line.len() {
                continue;
            }
            for (got, want) in smooth(&line, window).iter().zip(&line) {
                assert!((got - want).abs() < 1.0e-9, "window {window}");
            }
        }
        // too short for the window, or no window: unchanged
        assert_eq!(smooth(&line[..100], 121), line[..100]);
        assert_eq!(smooth(&line, 1), line);
    }

    #[test]
    fn a_single_point_is_spread_as_the_tricube() {
        let mut x = vec![0.0; 301];
        x[150] = 1.0;
        let s = smooth(&x, 121);
        assert!((s.iter().sum::<f64>() - 1.0).abs() < 1.0e-12);
        // the weight is zero 60 points away and symmetric
        assert!(s[90].abs() < 1.0e-15 && s[210].abs() < 1.0e-15);
        assert!(s[91] > 0.0 && (s[91] - s[209]).abs() < 1.0e-15);
        let peak = s[150];
        assert!((s[180] / peak - tricube(0.5)).abs() < 1.0e-12);
        assert!((peak - 0.014_400).abs() < 1.0e-5, "{peak}");
    }

    #[test]
    fn a_tone_on_a_point_has_the_height_of_its_hann_line() {
        // 250 Hz in 1000 samples at 1 kHz would be on a point with a
        // periodic window; with the symmetric one it is all but there
        let fs = 1000.0;
        let n = 1000;
        let x: Vec<f64> = (0..n)
            .map(|i| 8.0 * (TAU * 250.0 * f64::from(i) / fs).sin())
            .collect();
        let db = raw_db(&x, fs);
        assert_eq!(db.len(), n as usize / 2 + 1);
        // amplitude² · N / (8 · fs)
        let want = 10.0 * (64.0 * f64::from(n) / (8.0 * fs)).log10();
        assert!((db[250] - want).abs() < 0.02, "{} against {want}", db[250]);
    }

    #[test]
    fn a_constant_shows_at_zero_hertz_undoubled() {
        let fs = 2000.0;
        let n = 4001;
        let x = vec![30.0; n];
        let db = raw_db(&x, fs);
        // the sum of a Hann window is (N − 1) / 2
        let sum = 30.0 * (n - 1) as f64 / 2.0;
        let want = 10.0 * (sum * sum / (fs * n as f64)).log10();
        assert!((db[0] - want).abs() < 1.0e-9, "{} against {want}", db[0]);
    }
}
