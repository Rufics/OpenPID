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
//!    starting two samples late ([`span`]);
//! 2. one transform of that span through a symmetric Hann window;
//! 3. `2·|X|² / (fs·N)` in decibels, the point at 0 Hz not doubled
//!    ([`raw_db`]);
//! 4. a local regression of the decibel values: at every point a line is
//!    fitted to the nearest points with tricube weights ([`smooth`]), over
//!    a window that grows with the smoothing level and, slowly, with the
//!    length of the curve ([`window`]).

use openpid_dsp::fft::Fft;
use std::ops::Range;

/// The smoothing levels of the tool's spectrum window, in the order of its
/// list. "medium" is the tool's default.
pub const LEVELS: [&str; 5] = openpid_analysis::CURVE_LEVELS;

/// Points of the regression window at smoothing level `level` (0 for "low"
/// … 4 for "high") for a curve of `points` points: the cube of the level's
/// number (1 … 5) times the decimal logarithm of the number of points, made
/// odd. For the 28 000 points of a 30 s log at 2 kHz: 5, 35, 121, 285, 555.
///
/// Round 2 found it on thirty curves: six logs from 20 000 to 112 000
/// samples at five levels, every one exact.
pub fn window(level: usize, points: usize) -> usize {
    openpid_analysis::curve_window(level, points)
}

/// Seconds the tool leaves out at each end of a log.
const TRIM_S: f64 = 1.0;
/// Samples by which the analysed span starts after the first second.
const LATE: usize = 2;

/// The samples of a log of `len` samples at `fs` that go into the spectrum,
/// or `None` when the log is too short to have any.
///
/// Two things about it are observed and not understood: for one log of
/// round 2 (12 s at 8 kHz) the span was one sample longer at its end, and
/// for one flight (`s9-bands-2k`, alone or in company) it starts one sample
/// later. Either moves a curve by about 0.01 dB at worst.
pub fn span(len: usize, fs: f64) -> Option<Range<usize>> {
    let trim = (TRIM_S * fs).round() as usize;
    let count = len.checked_sub(2 * trim + 1)?;
    let start = trim + LATE;
    (count > 1 && start + count <= len).then(|| start..start + count)
}

/// The transform of `x` as the tool scales it, decibels: one value per
/// point of the one-sided spectrum, `fs / x.len()` apart. This is the
/// level `OpenPID` itself computes for its curve (decision D-001).
pub fn raw_db(x: &[f64], fs: f64) -> Vec<f64> {
    openpid_analysis::curve_level_db(&Fft::new(), x, fs)
}

/// `values` smoothed by a local regression over `window` points
/// (`openpid_dsp::smooth::local_line`): at every point the value of the line
/// fitted to the `window` nearest points with tricube weights.
pub fn smooth(values: &[f64], window: usize) -> Vec<f64> {
    openpid_dsp::smooth::local_line(values, window)
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
/// from its first frame to its last) at smoothing level `level` (0 for
/// "low" … 4 for "high"), or `None` for a log too short to analyse.
pub fn curve(logged: &[f64], fs: f64, level: usize) -> Option<Curve> {
    curve_of(logged, fs, span(logged.len(), fs)?, level)
}

/// [`curve`] for a span given by hand, or `None` when the span is not
/// inside the log.
pub fn curve_of(logged: &[f64], fs: f64, span: Range<usize>, level: usize) -> Option<Curve> {
    let raw = raw_db(logged.get(span.clone())?, fs);
    if raw.is_empty() {
        return None;
    }
    Some(Curve {
        bin_hz: fs / span.len() as f64,
        db: smooth(&raw, window(level, raw.len())),
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    #[test]
    fn the_span_is_the_log_without_a_second_at_each_end_one_sample_short() {
        assert_eq!(span(60_000, 2000.0), Some(2002..58_001));
        assert_eq!(span(30_000, 1000.0), Some(1002..29_001));
        assert_eq!(span(24_000, 2000.0).map(|s| s.len()), Some(19_999));
        assert_eq!(span(4000, 2000.0), None);
        assert_eq!(span(0, 2000.0), None);
    }

    #[test]
    fn a_line_is_left_as_it_is_ends_included() {
        let line: Vec<f64> = (0..400).map(|i| 3.0 - 0.25 * f64::from(i)).collect();
        for window in [5, 35, 121, 285] {
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
        assert!((s[180] / peak - (1.0f64 - 0.125).powi(3)).abs() < 1.0e-12);
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

    #[test]
    fn the_window_is_the_level_cubed_times_the_logarithm_of_the_points() {
        // what round 2 measured: the points of each curve and its five windows
        let measured = [
            (10_000, [5, 33, 109, 257, 501]),
            (14_000, [5, 33, 111, 265, 519]),
            (28_000, [5, 35, 121, 285, 555]),
            (40_001, [5, 37, 125, 295, 575]),
            (45_000, [5, 37, 125, 297, 581]),
            (56_000, [5, 37, 129, 303, 593]),
        ];
        for (points, windows) in measured {
            for (level, want) in windows.into_iter().enumerate() {
                assert_eq!(
                    window(level, points),
                    want,
                    "{points} points, level {level}"
                );
            }
        }
        // a level beyond the list is the last one; no points is still a window
        assert_eq!(window(9, 28_000), 555);
        assert_eq!(window(2, 0), 1);
    }

    #[test]
    fn a_curve_is_made_of_the_span_the_transform_and_the_window() {
        let fs = 1000.0;
        let x: Vec<f64> = (0..6000)
            .map(|i| 5.0 * (TAU * 100.0 * f64::from(i) / fs).sin() + f64::from(i % 7))
            .collect();
        let c = curve(&x, fs, 2).unwrap_or_else(|| unreachable!());
        assert_eq!(c.span, 1002..5001);
        assert_eq!(c.db.len(), 2000);
        assert!((c.bin_hz - fs / 3999.0).abs() < 1.0e-12);
        let by_hand = smooth(&raw_db(&x[1002..5001], fs), window(2, 2000));
        assert_eq!(c.db, by_hand);
        assert_eq!(c.at(100.0), Some(c.db[400]));
        assert_eq!(c.at(-1.0), None);
        assert_eq!(c.at(501.0), None);
        // a span outside the log, or a log too short
        assert!(curve_of(&x, fs, 5000..7000, 2).is_none());
        assert!(curve(&x[..1500], fs, 2).is_none());
    }
}
