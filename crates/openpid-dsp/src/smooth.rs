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

/// Centred moving average over `w` points whose window is cut at the ends:
/// at every point the mean of the values from `w/2` before it to `w/2`
/// after it, over the points that exist, so near an end the window is
/// lopsided and holds fewer points (unlike [`moving_average`], which shrinks
/// it symmetrically). An even `w` counts as the next odd one; a `w` below
/// two leaves the values as they are.
pub fn moving_average_cut(x: &[f64], w: usize) -> Vec<f64> {
    let n = x.len();
    let half = w / 2;
    if n == 0 || half == 0 {
        return x.to_vec();
    }
    // running sums: the mean of x[lo..hi] is (sum[hi] − sum[lo]) / (hi − lo)
    let mut sum = Vec::with_capacity(n + 1);
    sum.push(0.0);
    let mut total = 0.0;
    for v in x {
        total += v;
        sum.push(total);
    }
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(n);
            (sum[hi] - sum[lo]) / (hi - lo) as f64
        })
        .collect()
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
    fn moving_average_cut_is_the_mean_of_the_points_that_exist() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        // inside: the centred mean of five points; at the ends fewer of them
        let s = moving_average_cut(&x, 5);
        let want = [2.0, 2.5, 3.0, 4.0, 5.0, 5.5, 6.0];
        for (got, want) in s.iter().zip(want) {
            assert!((got - want).abs() < 1.0e-12, "{s:?}");
        }
        // an even width is the next odd one; one or none leaves the values
        assert_eq!(moving_average_cut(&x, 4), s);
        assert_eq!(moving_average_cut(&x, 1), x);
        assert_eq!(moving_average_cut(&x, 0), x);
        assert!(moving_average_cut(&[], 5).is_empty());
        // a window wider than the values: every point is a mean of what exists
        let wide = moving_average_cut(&[0.0, 3.0, 9.0], 101);
        assert!(wide.iter().all(|v| (v - 4.0).abs() < 1.0e-12), "{wide:?}");
    }

    #[test]
    fn moving_average_cut_spreads_a_point_evenly_and_keeps_a_level() {
        let mut x = vec![0.0; 1001];
        x[500] = 151.0;
        let s = moving_average_cut(&x, 151);
        for (i, v) in s.iter().enumerate() {
            let want = if (425..=575).contains(&i) { 1.0 } else { 0.0 };
            assert!((v - want).abs() < 1.0e-12, "{i}: {v}");
        }
        // a level stays the level up to the very ends
        let level = vec![-37.5; 2000];
        assert!(
            moving_average_cut(&level, 601)
                .iter()
                .all(|v| (v + 37.5).abs() < 1.0e-9)
        );
        // the first point is the mean of itself and the half window after it
        let ramp: Vec<f64> = (0..400).map(f64::from).collect();
        let r = moving_average_cut(&ramp, 51);
        assert!((r[0] - 12.5).abs() < 1.0e-9 && (r[399] - 386.5).abs() < 1.0e-9);
        assert!((r[200] - 200.0).abs() < 1.0e-9);
    }
}
