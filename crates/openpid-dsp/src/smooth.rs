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

/// Local quadratic regression with tricube weights over a span of samples
/// (a "loess" of degree 2), the line smoothing of the Log viewer (D-011).
///
/// For each sample `i` the polynomial `y ≈ a + b·d + c·d²` is fitted by
/// weighted least squares to the `span` samples around it, `d` being the
/// distance from `i` in samples, with the weights `(1 − (|d|/h)³)³`, where
/// `h` is the largest `|d|` in the window (so its outermost samples weigh
/// nothing); the smoothed value is `a`, the fit at `i`. The window is centred
/// where it can be, `h = (span − 1)/2`; near the two ends of the series it is
/// shifted to stay inside the data, and the fit is still evaluated at `i`.
/// Samples that are not valid take no part in any fit.
///
/// The span is counted in samples. The reference tool was seen at 1 kHz
/// only, so whether its span is samples or milliseconds is not known.
#[derive(Debug, Clone, PartialEq)]
pub struct QuadraticLoess {
    span: usize,
    /// The weights of a centred window with every sample valid: the smoothed
    /// value is their dot product with the window, computed once.
    interior: Vec<f64>,
}

/// The tricube weight of a distance `u` scaled to −1 … 1.
fn tricube(u: f64) -> f64 {
    let a = 1.0 - u.abs().powi(3);
    if a <= 0.0 { 0.0 } else { a * a * a }
}

/// Weighted least squares of `y ≈ a + b·u + c·u²` over the points `(u, w, y)`;
/// returns `a`, the fit at `u = 0`. A window too thin for a parabola gets a
/// line, then a weighted mean; `None` when no point has any weight.
fn fit_at_zero(points: impl Iterator<Item = (f64, f64, f64)>) -> Option<f64> {
    let (mut s, mut t) = ([0.0f64; 5], [0.0f64; 3]);
    for (u, w, y) in points.filter(|p| p.1 > 0.0) {
        let mut p = w;
        for (k, sk) in s.iter_mut().enumerate() {
            *sk += p;
            if k < 3 {
                t[k] += p * y;
            }
            p *= u;
        }
    }
    let [s0, s1, s2, s3, s4] = s;
    let [t0, t1, t2] = t;
    // a Gram matrix: 0 <= det <= the product of its diagonal
    let det3 = s0 * (s2 * s4 - s3 * s3) - s1 * (s1 * s4 - s3 * s2) + s2 * (s1 * s3 - s2 * s2);
    let diag3 = s0 * s2 * s4;
    if diag3 > 0.0 && det3 > 1e-9 * diag3 {
        let num = t0 * (s2 * s4 - s3 * s3) - s1 * (t1 * s4 - s3 * t2) + s2 * (t1 * s3 - s2 * t2);
        return Some(num / det3);
    }
    let det2 = s0 * s2 - s1 * s1;
    if s0 * s2 > 0.0 && det2 > 1e-9 * s0 * s2 {
        return Some((t0 * s2 - s1 * t1) / det2);
    }
    (s0 > 0.0).then(|| t0 / s0)
}

impl QuadraticLoess {
    /// A smoother over `span` samples (made odd); a span below 3 smooths
    /// nothing, the "off" level.
    pub fn new(span: usize) -> Self {
        let span = if span < 3 { 1 } else { span | 1 };
        let h = (span - 1) / 2;
        let interior = (0..span)
            .map(|k| {
                let points = (0..span).map(|j| {
                    let u = (j as f64 - h as f64) / h.max(1) as f64;
                    (u, tricube(u), if j == k { 1.0 } else { 0.0 })
                });
                if span == 1 {
                    1.0
                } else {
                    fit_at_zero(points).unwrap_or(0.0)
                }
            })
            .collect();
        QuadraticLoess { span, interior }
    }

    /// The span in samples (1 for "off").
    pub fn span(&self) -> usize {
        self.span
    }

    /// Samples on each side of a centred window, `(span − 1)/2`.
    pub fn half(&self) -> usize {
        (self.span - 1) / 2
    }

    /// The weights of a centred window with every sample valid, from the
    /// farthest sample before `i` to the farthest after it.
    pub fn interior_kernel(&self) -> &[f64] {
        &self.interior
    }

    /// The window of sample `i` in a series of `n`: `lo..hi` and the largest
    /// distance from `i` in it.
    fn window(&self, n: usize, i: usize) -> (usize, usize, usize) {
        let lo = i
            .saturating_sub(self.half())
            .min(n.saturating_sub(self.span));
        let hi = (lo + self.span).min(n);
        (lo, hi, (i - lo).max(hi - 1 - i))
    }

    /// The fit at `i` from the valid samples of its window.
    fn fit(&self, y: &[f64], valid: &impl Fn(usize) -> bool, i: usize) -> f64 {
        let (lo, hi, reach) = self.window(y.len(), i);
        if reach == 0 {
            return y[i];
        }
        let points = (lo..hi).filter(|&j| valid(j)).map(|j| {
            let u = (j as f64 - i as f64) / reach as f64;
            (u, tricube(u), y[j])
        });
        fit_at_zero(points).unwrap_or(y[i])
    }

    /// The smoothed values of the samples `from..to` of `y` (clamped to the
    /// series), using the samples around that range as far as the windows
    /// reach. `valid(i)` says whether sample `i` is valid; an invalid sample
    /// keeps its value and stays invalid for the caller.
    pub fn smooth_range(
        &self,
        y: &[f64],
        valid: impl Fn(usize) -> bool,
        from: usize,
        to: usize,
    ) -> Vec<f64> {
        let n = y.len();
        let to = to.min(n);
        let from = from.min(to);
        if self.span == 1 {
            return y[from..to].to_vec();
        }
        let h = self.half();
        // invalid samples counted from `base`: a window with none of them
        // takes the precomputed weights
        let base = from.saturating_sub(h);
        let top = (to + h).min(n);
        let mut invalid = Vec::with_capacity(top - base + 1);
        invalid.push(0usize);
        for j in base..top {
            invalid.push(invalid[invalid.len() - 1] + usize::from(!valid(j)));
        }
        (from..to)
            .map(|i| {
                if !valid(i) {
                    y[i]
                } else if i >= h && i + h < n && invalid[i + h + 1 - base] == invalid[i - h - base]
                {
                    dot(&self.interior, &y[i - h..=i + h])
                } else {
                    self.fit(y, &valid, i)
                }
            })
            .collect()
    }

    /// The smoothed value of sample `i`, or `None` when it is not valid.
    pub fn at(&self, y: &[f64], valid: impl Fn(usize) -> bool, i: usize) -> Option<f64> {
        if i >= y.len() || !valid(i) {
            return None;
        }
        self.smooth_range(y, valid, i, i + 1).first().copied()
    }
}

/// `Σ a·b`, in four running sums so the loop is not one long dependency chain.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut acc = [0.0f64; 4];
    let (ca, cb) = (a.chunks_exact(4), b.chunks_exact(4));
    let tail: f64 = ca
        .remainder()
        .iter()
        .zip(cb.remainder())
        .map(|(x, y)| x * y)
        .sum();
    for (x, y) in ca.zip(cb) {
        for ((s, x), y) in acc.iter_mut().zip(x).zip(y) {
            *s += x * y;
        }
    }
    acc[0] + acc[1] + acc[2] + acc[3] + tail
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

    /// The spans of the four levels and the centre weight the reference
    /// tool's kernel was measured with (to ±0.0002, at 1 kHz).
    const LEVELS: [(usize, f64); 4] = [(11, 0.3172), (21, 0.1588), (41, 0.0793), (81, 0.0395)];

    fn all(_: usize) -> bool {
        true
    }

    #[test]
    fn interior_kernels_match_the_reference_tool() {
        for (span, centre) in LEVELS {
            let s = QuadraticLoess::new(span);
            let k = s.interior_kernel();
            assert_eq!((s.span(), k.len()), (span, span));
            let h = s.half();
            assert!((k[h] - centre).abs() < 1e-3, "{span}: {}", k[h]);
            let sum: f64 = k.iter().sum();
            assert!((sum - 1.0).abs() < 1e-12, "{span}: {sum}");
            // symmetric, the outermost samples weigh nothing, side lobes below zero
            for d in 0..h {
                assert!((k[d] - k[span - 1 - d]).abs() < 1e-12);
            }
            assert!(k[0].abs() < 1e-15 && k[span - 1].abs() < 1e-15);
            let min = k.iter().copied().fold(f64::INFINITY, f64::min);
            assert!(min < 0.0, "{span}: {min}");
            // a one-sample pulse comes back as the kernel
            let mut y = vec![0.0; 3 * span];
            y[span + h] = 1.0;
            let out = s.smooth_range(&y, all, 0, y.len());
            for d in 0..span {
                assert!((out[span + d] - k[span - 1 - d]).abs() < 1e-12);
            }
        }
        let low = QuadraticLoess::new(11);
        let min = low.interior_kernel().iter().copied().fold(0.0, f64::min);
        assert!((min + 0.0375).abs() < 5e-4, "{min}");
    }

    #[test]
    fn a_quadratic_passes_unchanged_also_at_the_ends() {
        let y: Vec<f64> = (0..300)
            .map(|i| {
                let x = f64::from(i);
                3.0 - 0.7 * x + 0.02 * x * x
            })
            .collect();
        for (span, _) in LEVELS {
            let out = QuadraticLoess::new(span).smooth_range(&y, all, 0, y.len());
            for (a, b) in out.iter().zip(&y) {
                assert!(
                    (a - b).abs() < 1e-8 * b.abs().max(1.0),
                    "{span}: {a} vs {b}"
                );
            }
        }
    }

    #[test]
    fn the_ends_shift_the_window_inside_the_series() {
        // a cubic is not reproduced: the end values differ from those of a
        // window shrunk symmetrically, and follow a fit of the first `span`
        let y: Vec<f64> = (0..40).map(|i| (f64::from(i) / 7.0).powi(3)).collect();
        let s = QuadraticLoess::new(11);
        let out = s.smooth_range(&y, all, 0, y.len());
        // sample 0: the window is 0..11, the farthest distance 10
        let points = (0..11).map(|j| {
            let u = j as f64 / 10.0;
            (u, tricube(u), y[j])
        });
        let first = fit_at_zero(points).unwrap();
        assert!((out[0] - first).abs() < 1e-12, "{} vs {first}", out[0]);
        // the last sample mirrors that at the other end
        let points = (29..40).map(|j| {
            let u = (j as f64 - 39.0) / 10.0;
            (u, tricube(u), y[j])
        });
        let last = fit_at_zero(points).unwrap();
        assert!((out[39] - last).abs() < 1e-12);
        // a part of the series gives the same values as the whole
        let part = s.smooth_range(&y, all, 3, 37);
        assert_eq!(part.len(), 34);
        for (k, v) in part.iter().enumerate() {
            assert!((v - out[k + 3]).abs() < 1e-12);
        }
        assert_eq!(s.at(&y, all, 0), Some(out[0]));
        assert_eq!(s.at(&y, all, 40), None);
        // a series shorter than the span is one window
        let short = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
        let out = s.smooth_range(&short, all, 0, 6);
        assert_eq!(out.len(), 6);
        assert!(out.iter().zip(&short).any(|(a, b)| (a - b).abs() > 1e-6));
        // ranges beyond the series are clamped
        assert!(s.smooth_range(&short, all, 6, 9).is_empty());
        assert_eq!(s.smooth_range(&[5.0], all, 0, 1), vec![5.0]);
    }

    #[test]
    fn invalid_samples_take_no_part_and_stay() {
        let s = QuadraticLoess::new(21);
        // a line with a wild invalid sample in the middle
        let mut y: Vec<f64> = (0..100).map(|i| 2.0 * f64::from(i)).collect();
        y[50] = 1.0e6;
        let valid = |i: usize| i != 50;
        let out = s.smooth_range(&y, valid, 0, y.len());
        assert_eq!(out[50], 1.0e6, "kept as it is");
        for (i, v) in out.iter().enumerate().filter(|(i, _)| *i != 50) {
            assert!((v - 2.0 * i as f64).abs() < 1e-8, "{i}: {v}");
        }
        assert_eq!(s.at(&y, valid, 50), None);
        // two valid samples: the line through them; one: its value
        let y = [0.0, 4.0, 9.0, 6.0];
        let out = s.smooth_range(&y, |i| i == 1 || i == 3, 0, 4);
        assert!((out[1] - 4.0).abs() < 1e-12 && (out[3] - 6.0).abs() < 1e-12);
        let out = s.smooth_range(&y, |i| i == 2, 0, 4);
        assert_eq!(out[2], 9.0);
        // nothing with weight around a sample: its own value
        assert_eq!(fit_at_zero(std::iter::empty()), None);
        assert_eq!(fit_at_zero([(1.0, 0.0, 3.0)].into_iter()), None);
    }

    #[test]
    fn off_returns_the_input() {
        let y = [1.0, -2.0, 7.5, 3.0];
        for span in [0, 1, 2] {
            let s = QuadraticLoess::new(span);
            assert_eq!(s.span(), 1);
            assert_eq!(s.interior_kernel(), [1.0]);
            assert_eq!(s.smooth_range(&y, all, 0, 4), y);
            assert_eq!(s.smooth_range(&y, all, 1, 3), [-2.0, 7.5]);
            assert_eq!(s.at(&y, all, 2), Some(7.5));
        }
        // an even span is made odd
        assert_eq!(QuadraticLoess::new(10).span(), 11);
    }

    #[test]
    fn smoothing_takes_out_noise() {
        // white-ish noise around a slow tone: the residual shrinks with the span
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
        for (span, _) in LEVELS {
            let out = QuadraticLoess::new(span).smooth_range(&y, all, 0, y.len());
            let err: f64 = out
                .iter()
                .enumerate()
                .map(|(i, v)| (v - (i as f64 / 500.0).sin()).powi(2))
                .sum();
            assert!(err < last, "{span}: {err} vs {last}");
            last = err;
        }
    }
}
