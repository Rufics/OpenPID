//! Peak picking on spectra: local maxima ranked by topographic prominence.
//!
//! Prominence of a local maximum is its height above the higher of the two
//! deepest valleys between it and the nearest higher samples on each side
//! (the convention of `scipy.signal.peak_prominences`). Working on a dB
//! spectrum makes the threshold a ratio, which is what a tuning eye uses.

/// One spectral peak.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// Index into the input arrays.
    pub index: usize,
    /// X coordinate (frequency) of the peak.
    pub x: f64,
    /// Y value at the peak.
    pub y: f64,
    /// Topographic prominence in the units of `y`.
    pub prominence: f64,
}

/// Prominence of the local maximum at `i`.
fn prominence(y: &[f64], i: usize) -> f64 {
    let peak = y[i];
    let mut left_min = peak;
    let mut j = i;
    while j > 0 {
        j -= 1;
        if y[j] > peak {
            break;
        }
        left_min = left_min.min(y[j]);
    }
    let mut right_min = peak;
    let mut j = i;
    while j + 1 < y.len() {
        j += 1;
        if y[j] > peak {
            break;
        }
        right_min = right_min.min(y[j]);
    }
    peak - left_min.max(right_min)
}

/// Up to `max_peaks` local maxima of `y` with prominence ≥ `min_prominence`,
/// at least `min_distance` apart in `x`, highest prominence first.
///
/// Non-finite samples never form a peak.
///
/// # Panics
/// If `x` and `y` differ in length.
pub fn find_peaks(
    x: &[f64],
    y: &[f64],
    max_peaks: usize,
    min_prominence: f64,
    min_distance: f64,
) -> Vec<Peak> {
    assert_eq!(x.len(), y.len(), "x and y must have the same length");
    let n = y.len();
    if n < 3 || max_peaks == 0 {
        return Vec::new();
    }
    let mut candidates: Vec<Peak> = (1..n - 1)
        .filter(|&i| y[i].is_finite() && y[i] > y[i - 1] && y[i] >= y[i + 1])
        .map(|i| Peak {
            index: i,
            x: x[i],
            y: y[i],
            prominence: prominence(y, i),
        })
        .filter(|p| p.prominence >= min_prominence)
        .collect();
    candidates.sort_by(|a, b| b.prominence.total_cmp(&a.prominence));
    let mut picked: Vec<Peak> = Vec::new();
    for p in candidates {
        if picked.len() >= max_peaks {
            break;
        }
        if picked.iter().all(|q| (q.x - p.x).abs() >= min_distance) {
            picked.push(p);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tones_over_a_noise_floor() {
        // a dB spectrum: flat floor with two peaks of different prominence
        let x: Vec<f64> = (0..200).map(|i| f64::from(i) * 5.0).collect();
        let mut y = vec![-60.0; 200];
        for (i, v) in y.iter_mut().enumerate() {
            let d1 = (f64::from(i as u32) - 30.0).abs();
            let d2 = (f64::from(i as u32) - 120.0).abs();
            *v += 30.0 * (-d1 * d1 / 4.0).exp() + 12.0 * (-d2 * d2 / 2.0).exp();
        }
        let peaks = find_peaks(&x, &y, 5, 6.0, 0.0);
        assert_eq!(peaks.len(), 2, "{peaks:?}");
        assert_eq!(peaks[0].x, 150.0);
        assert!((peaks[0].prominence - 30.0).abs() < 1e-9);
        assert_eq!(peaks[1].x, 600.0);
        assert!((peaks[1].prominence - 12.0).abs() < 1e-9);
        assert_eq!(find_peaks(&x, &y, 1, 6.0, 0.0).len(), 1);
        assert!(
            find_peaks(&x, &y, 5, 20.0, 0.0)
                .iter()
                .all(|p| p.x == 150.0)
        );
        assert!(find_peaks(&x, &y, 0, 0.0, 0.0).is_empty());
    }

    #[test]
    fn shoulder_peaks_and_distance() {
        // a tall peak with a small bump on its shoulder: the bump's prominence is only its own height
        let x: Vec<f64> = (0..50).map(f64::from).collect();
        let mut y: Vec<f64> = x.iter().map(|v| -(v - 20.0).abs()).collect();
        // bump on the shoulder: 3 above the slope, so 2 above the valley on its uphill side
        y[30] += 3.0;
        let peaks = find_peaks(&x, &y, 5, 0.5, 0.0);
        assert_eq!(peaks[0].x, 20.0);
        let bump = peaks.iter().find(|p| p.x == 30.0).unwrap();
        assert!(bump.prominence > 1.9 && bump.prominence < 2.1, "{bump:?}");
        // a minimum distance drops the bump
        let far = find_peaks(&x, &y, 5, 0.5, 15.0);
        assert_eq!(far.len(), 1);
        // NaN samples are skipped, not promoted
        y[10] = f64::NAN;
        assert!(
            find_peaks(&x, &y, 5, 0.5, 0.0)
                .iter()
                .all(|p| p.y.is_finite())
        );
        assert!(find_peaks(&x[..2], &y[..2], 5, 0.0, 0.0).is_empty());
    }
}
