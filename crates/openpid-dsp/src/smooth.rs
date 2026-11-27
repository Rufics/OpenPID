//! Smoothing helpers used for display-oriented post-processing.

/// Centred moving average of odd width `w` (like MATLAB `smooth(x, w)`):
/// near the edges the window shrinks symmetrically.
pub fn moving_average(x: &[f64], w: usize) -> Vec<f64> {
    let n = x.len();
    if n == 0 || w <= 1 {
        return x.to_vec();
    }
    let half = (w - 1) / 2;
    (0..n)
        .map(|i| {
            let r = half.min(i).min(n - 1 - i);
            let s: f64 = x[i - r..=i + r].iter().sum();
            s / (2 * r + 1) as f64
        })
        .collect()
}

/// A centred moving average cut at the ends of the series, the line
/// smoothing of the Log viewer (D-011, as the reference tool's web edition
/// 1.0 smooths its main window, D-015).
///
/// Sample `i` becomes the mean of the samples from `i − half` to
/// `i + half` that exist and are valid: near an end of the series the
/// window is cut there, lopsided and shorter (unlike [`moving_average`],
/// which shrinks it symmetrically). On a ramp `0, 1, 2, …` of `n` samples a
/// width of 21 gives 5 for the first sample (the mean of samples 0 … 10) and
/// `n − 6` for the last. A sample that is not valid (or not finite) takes
/// no part in any mean and keeps its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutMovingAverage {
    half: usize,
}

impl CutMovingAverage {
    /// A moving average over `width` samples: `half = width / 2` on each
    /// side, so an even width counts as the next odd one, and a width below
    /// two smooths nothing.
    pub const fn new(width: usize) -> Self {
        CutMovingAverage { half: width / 2 }
    }

    /// The width of a window away from the ends, `2·half + 1` samples (1: none).
    pub const fn width(self) -> usize {
        2 * self.half + 1
    }

    /// Samples on each side of a window away from the ends.
    pub const fn half(self) -> usize {
        self.half
    }

    /// The smoothed values of the samples `from..to` of `y` (clamped to the
    /// series), using the samples around that range as far as the windows
    /// reach. `valid(i)` says whether sample `i` is valid; an invalid sample
    /// keeps its value and stays invalid for the caller.
    pub fn smooth_range(
        self,
        y: &[f64],
        valid: impl Fn(usize) -> bool,
        from: usize,
        to: usize,
    ) -> Vec<f64> {
        let n = y.len();
        let to = to.min(n);
        let from = from.min(to);
        let h = self.half;
        if h == 0 {
            return y[from..to].to_vec();
        }
        let takes = |j: usize, v: f64| v.is_finite() && valid(j);
        // running sums of the samples that take part, and their count, from
        // `base`: the mean over `lo..hi` is a difference of two of each
        let base = from.saturating_sub(h);
        let top = (to + h).min(n);
        let mut sum = Vec::with_capacity(top - base + 1);
        let mut count = Vec::with_capacity(top - base + 1);
        let (mut s, mut c) = (0.0f64, 0usize);
        sum.push(s);
        count.push(c);
        for (j, &v) in y.iter().enumerate().take(top).skip(base) {
            if takes(j, v) {
                s += v;
                c += 1;
            }
            sum.push(s);
            count.push(c);
        }
        (from..to)
            .map(|i| {
                if !takes(i, y[i]) {
                    return y[i];
                }
                let lo = i.saturating_sub(h) - base;
                let hi = (i + h + 1).min(n) - base;
                (sum[hi] - sum[lo]) / (count[hi] - count[lo]) as f64
            })
            .collect()
    }

    /// The smoothed value of sample `i`, or `None` when it is not valid.
    pub fn at(self, y: &[f64], valid: impl Fn(usize) -> bool, i: usize) -> Option<f64> {
        if i >= y.len() || !valid(i) {
            return None;
        }
        self.smooth_range(y, valid, i, i + 1).first().copied()
    }
}

/// 2-D box filter of size `wy × wx` (odd sizes) with zero padding, row-major
/// `z[iy * nx + ix]`; equivalent to MATLAB `filter2(fspecial("average", [wy wx]), z)`.
///
/// # Panics
/// Panics if `z.len() != nx * ny`.
pub fn box_filter_2d(z: &[f64], nx: usize, ny: usize, wx: usize, wy: usize) -> Vec<f64> {
    assert_eq!(z.len(), nx * ny);
    let (wx, wy) = (wx.max(1) | 1, wy.max(1) | 1);
    let (hx, hy) = ((wx - 1) / 2, (wy - 1) / 2);
    let norm = 1.0 / (wx * wy) as f64;
    // separable: rows then columns, zeros outside
    let mut tmp = vec![0.0; nx * ny];
    for iy in 0..ny {
        for ix in 0..nx {
            let lo = ix.saturating_sub(hx);
            let hi = (ix + hx).min(nx - 1);
            tmp[iy * nx + ix] = z[iy * nx + lo..=iy * nx + hi].iter().sum();
        }
    }
    let mut out = vec![0.0; nx * ny];
    for ix in 0..nx {
        for iy in 0..ny {
            let lo = iy.saturating_sub(hy);
            let hi = (iy + hy).min(ny - 1);
            let s: f64 = (lo..=hi).map(|y| tmp[y * nx + ix]).sum();
            out[iy * nx + ix] = s * norm;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages() {
        assert_eq!(
            moving_average(&[1.0, 2.0, 3.0, 4.0, 5.0], 3),
            vec![1.0, 2.0, 3.0, 4.0, 5.0]
        );
        assert_eq!(moving_average(&[0.0, 3.0, 0.0], 3), vec![0.0, 1.0, 0.0]);
        let z = [0.0, 0.0, 0.0, 0.0, 9.0, 0.0, 0.0, 0.0, 0.0];
        let f = box_filter_2d(&z, 3, 3, 3, 3);
        assert!(f.iter().all(|v| (v - 1.0).abs() < 1e-12));
    }

    fn all(_: usize) -> bool {
        true
    }

    /// The four levels of the reference tool's web edition, 2, 5, 10 and 20,
    /// and their widths, `2·floor(S/2) + 1` samples.
    const LEVELS: [(usize, usize); 4] = [(2, 3), (5, 5), (10, 11), (20, 21)];

    #[test]
    fn a_one_sample_pulse_spreads_over_the_width() {
        // 500 deg/s for one sample, as the web edition draws it: 166.667,
        // 100, 45.455 and 23.810 over 3, 5, 11 and 21 samples
        let want = [166.667, 100.0, 45.455, 23.810];
        for ((level, width), height) in LEVELS.into_iter().zip(want) {
            let s = CutMovingAverage::new(level);
            assert_eq!(s.width(), width);
            let mut y = vec![0.0; 200];
            y[100] = 500.0;
            let out = s.smooth_range(&y, all, 0, y.len());
            for (i, v) in out.iter().enumerate() {
                let inside = i.abs_diff(100) <= s.half();
                let w = if inside { 500.0 / width as f64 } else { 0.0 };
                assert!((v - w).abs() < 1e-12, "{level}, {i}: {v}");
            }
            assert!((out[100] - height).abs() < 5e-4, "{level}: {}", out[100]);
            // the area is kept
            let area: f64 = out.iter().sum();
            assert!((area - 500.0).abs() < 1e-9);
        }
    }

    #[test]
    fn the_window_is_cut_at_the_ends() {
        // a ramp of 7000 samples: at level 20 the web edition draws 5 first
        // (the mean of samples 0 … 10) and 6994 last (6989 … 6999)
        let y: Vec<f64> = (0..7000).map(f64::from).collect();
        let s = CutMovingAverage::new(20);
        let out = s.smooth_range(&y, all, 0, y.len());
        assert_eq!((out[0], out[6999]), (5.0, 6994.0));
        // a ramp passes unchanged where the window is whole
        for (i, v) in out.iter().enumerate().skip(10).take(6980) {
            assert!((v - i as f64).abs() < 1e-9, "{i}: {v}");
        }
        // every level: the first samples average 0 … i + half
        for (level, width) in LEVELS {
            let s = CutMovingAverage::new(level);
            let h = (width - 1) / 2;
            let out = s.smooth_range(&y, all, 0, y.len());
            for i in 0..h {
                let first = (i + h) as f64 / 2.0;
                let last = 6999.0 - first;
                assert!((out[i] - first).abs() < 1e-9, "{level}, {i}: {}", out[i]);
                assert!((out[6999 - i] - last).abs() < 1e-9, "{level}, {i}");
            }
        }
        // the same at the ends of a short series
        let s = CutMovingAverage::new(5);
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        assert_eq!(
            s.smooth_range(&x, all, 0, 7),
            [2.0, 2.5, 3.0, 4.0, 5.0, 5.5, 6.0]
        );
        // a window wider than the series: every sample is the mean of all
        let wide = CutMovingAverage::new(101).smooth_range(&[0.0, 3.0, 9.0], all, 0, 3);
        assert!(wide.iter().all(|v| (v - 4.0).abs() < 1e-12), "{wide:?}");
    }

    #[test]
    fn a_part_of_the_series_gives_the_values_of_the_whole() {
        let y: Vec<f64> = (0..300)
            .map(|i| (f64::from(i) / 7.0).sin() * 40.0)
            .collect();
        for (level, _) in LEVELS {
            let s = CutMovingAverage::new(level);
            let whole = s.smooth_range(&y, all, 0, y.len());
            for (from, to) in [(0, 4), (3, 37), (150, 160), (290, 300), (295, 400)] {
                let part = s.smooth_range(&y, all, from, to);
                assert_eq!(part.len(), to.min(300) - from);
                for (k, v) in part.iter().enumerate() {
                    assert!((v - whole[from + k]).abs() < 1e-12);
                }
            }
            assert_eq!(s.at(&y, all, 0), Some(whole[0]));
            assert_eq!(s.at(&y, all, 299), Some(whole[299]));
            assert_eq!(s.at(&y, all, 300), None);
            // ranges beyond the series are clamped
            assert!(s.smooth_range(&y, all, 300, 310).is_empty());
            assert!(s.smooth_range(&y, all, 5, 3).is_empty());
        }
        assert_eq!(
            CutMovingAverage::new(10).smooth_range(&[5.0], all, 0, 1),
            [5.0]
        );
    }

    #[test]
    fn invalid_samples_take_no_part_and_stay() {
        let s = CutMovingAverage::new(10);
        // a line with a wild invalid sample and a NaN in the middle
        let mut y: Vec<f64> = (0..100).map(|i| 2.0 * f64::from(i)).collect();
        y[50] = 1.0e6;
        y[70] = f64::NAN;
        let valid = |i: usize| i != 50;
        let out = s.smooth_range(&y, valid, 0, y.len());
        assert_eq!(out[50], 1.0e6, "kept as it is");
        assert!(out[70].is_nan(), "kept as it is");
        // a window that holds one of them takes the mean of the others:
        // samples 40 … 49 for sample 45, 60 … 69 for sample 65
        assert!((out[45] - 89.0).abs() < 1e-9, "{}", out[45]);
        assert!((out[65] - 129.0).abs() < 1e-9, "{}", out[65]);
        // elsewhere a line passes unchanged
        for i in [5, 20, 30, 39, 61, 64, 76, 94] {
            assert!((out[i] - 2.0 * i as f64).abs() < 1e-9, "{i}: {}", out[i]);
        }
        assert_eq!(s.at(&y, valid, 50), None);
        // one valid sample in reach: its own value
        let y = [0.0, 4.0, 9.0, 6.0];
        let out = s.smooth_range(&y, |i| i == 2, 0, 4);
        assert_eq!(out, y);
        let out = s.smooth_range(&y, |i| i == 1 || i == 3, 0, 4);
        assert_eq!(out, [0.0, 5.0, 9.0, 5.0]);
    }

    #[test]
    fn off_returns_the_input() {
        let y = [1.0, -2.0, 7.5, 3.0];
        for width in [0, 1] {
            let s = CutMovingAverage::new(width);
            assert_eq!((s.width(), s.half()), (1, 0));
            assert_eq!(s.smooth_range(&y, all, 0, 4), y);
            assert_eq!(s.smooth_range(&y, all, 1, 3), [-2.0, 7.5]);
            assert_eq!(s.at(&y, all, 2), Some(7.5));
        }
        // an even width counts as the next odd one
        assert_eq!(CutMovingAverage::new(2).width(), 3);
        assert_eq!(CutMovingAverage::new(10), CutMovingAverage::new(11));
    }

    #[test]
    fn smoothing_takes_out_noise() {
        // white-ish noise around a slow tone: the residual shrinks with the width
        let mut seed = 1u64;
        let y: Vec<f64> = (0..4000)
            .map(|i| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let noise = (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
                (f64::from(i) / 500.0).sin() + noise
            })
            .collect();
        let mut last = f64::INFINITY;
        for (level, _) in LEVELS {
            let out = CutMovingAverage::new(level).smooth_range(&y, all, 0, y.len());
            let err: f64 = out
                .iter()
                .enumerate()
                .map(|(i, v)| (v - (i as f64 / 500.0).sin()).powi(2))
                .sum();
            assert!(err < last, "{level}: {err} vs {last}");
            last = err;
        }
    }
}
