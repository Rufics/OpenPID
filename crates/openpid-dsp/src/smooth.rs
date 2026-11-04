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
}
