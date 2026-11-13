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

/// 2-D box filter of size `wy × wx` (odd sizes) with zero padding, row-major
/// `z[iy * nx + ix]`; equivalent to MATLAB `filter2(fspecial("average", [wy wx]), z)`.
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

/// The tricube weight at a distance of `d` window half-widths.
fn tricube(d: f64) -> f64 {
    let d = d.abs().min(1.0);
    (1.0 - d * d * d).powi(3)
}

/// `values` smoothed by a local regression over `window` points (LOWESS of
/// degree one on an even grid): at every point the value of the line fitted
/// to the `window` nearest points, the weights tricube and scaled so that
/// the farthest of those points weighs nothing. Away from the ends the
/// window is centred and the fit is a weighted mean; at an end the window
/// stays inside the data and the line carries the slope into the end.
///
/// An even `window` counts as the next odd one. A `window` below three, or
/// values shorter than the window, come back unchanged.
pub fn local_line(values: &[f64], window: usize) -> Vec<f64> {
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

    #[test]
    fn local_line_leaves_a_line_as_it_is_ends_included() {
        let line: Vec<f64> = (0..400).map(|i| 3.0 - 0.25 * f64::from(i)).collect();
        for window in [5, 35, 121, 285] {
            for (got, want) in local_line(&line, window).iter().zip(&line) {
                assert!((got - want).abs() < 1.0e-9, "window {window}");
            }
        }
        // too short for the window, or no window: unchanged
        assert_eq!(local_line(&line[..100], 121), line[..100]);
        assert_eq!(local_line(&line, 1), line);
        // an even window is the next odd one
        assert_eq!(local_line(&line, 34), local_line(&line, 35));
    }

    #[test]
    fn local_line_spreads_a_point_as_the_tricube() {
        let mut x = vec![0.0; 301];
        x[150] = 1.0;
        let s = local_line(&x, 121);
        assert!((s.iter().sum::<f64>() - 1.0).abs() < 1.0e-12);
        // the weight is zero 60 points away and symmetric
        assert!(s[90].abs() < 1.0e-15 && s[210].abs() < 1.0e-15);
        assert!(s[91] > 0.0 && (s[91] - s[209]).abs() < 1.0e-15);
        let peak = s[150];
        assert!((s[180] / peak - tricube(0.5)).abs() < 1.0e-12);
        assert!((peak - 0.014_400).abs() < 1.0e-5, "{peak}");
    }

    #[test]
    fn local_line_follows_a_slope_into_the_end() {
        // a step far from the start: the first points are a line through
        // level values, so they stay level; a mean cut at the end would too,
        // but a ramp at the very start is kept where a cut mean would bend it
        let ramp: Vec<f64> = (0..200)
            .map(|i| if i < 60 { f64::from(i) } else { 60.0 })
            .collect();
        let s = local_line(&ramp, 41);
        assert!(
            s[0].abs() < 1.0e-9 && (s[10] - 10.0).abs() < 1.0e-9,
            "{} {}",
            s[0],
            s[10]
        );
    }
}
