//! Lag of the cross-correlation peak between an input and an output signal.

/// Lag, in samples, at which the cross-correlation of `x` (input) and `y`
/// (output) peaks, searched over `0..=max_lag`; a positive value means `y`
/// lags `x`. Means are removed and every lag is normalised by its overlap, so
/// a pure delay of `d` samples gives `d` for any zero-mean input. The integer
/// peak is refined by parabolic interpolation of its two neighbours.
///
/// `None` when the signals are shorter than `max_lag + 2`, when `max_lag` is
/// zero, or when no lag correlates positively (nothing to measure).
pub fn lag_of_max(x: &[f64], y: &[f64], max_lag: usize) -> Option<f64> {
    let n = x.len().min(y.len());
    if max_lag == 0 || n < max_lag + 2 {
        return None;
    }
    let mx = x[..n].iter().sum::<f64>() / n as f64;
    let my = y[..n].iter().sum::<f64>() / n as f64;
    let r: Vec<f64> = (0..=max_lag)
        .map(|k| {
            let s: f64 = x[..n - k]
                .iter()
                .zip(&y[k..n])
                .map(|(a, b)| (a - mx) * (b - my))
                .sum();
            s / (n - k) as f64
        })
        .collect();
    let (k, &peak) = r.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    if peak <= 0.0 {
        return None;
    }
    let refined = if k > 0 && k < max_lag {
        let (a, b, c) = (r[k - 1], r[k], r[k + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > f64::EPSILON {
            let delta = 0.5 * (a - c) / denom;
            if delta.abs() <= 1.0 { delta } else { 0.0 }
        } else {
            0.0
        }
    } else {
        0.0
    };
    Some(k as f64 + refined)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random noise in [-1, 1).
    fn noise(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn pure_delay_and_first_order_plant() {
        let x = noise(4000, 7);
        let d = 9;
        let mut y = vec![0.0; x.len()];
        for i in d..x.len() {
            y[i] = 2.0 * x[i - d] + 5.0; // gain and offset do not matter
        }
        let lag = lag_of_max(&x, &y, 50).unwrap();
        assert!((lag - d as f64).abs() < 0.05, "lag {lag}");
        // a first-order lag after the delay keeps the peak at the delay
        let mut z = vec![0.0; x.len()];
        let a = 1.0 - (-1.0f64 / 15.0).exp();
        for i in 1..x.len() {
            let u = if i >= d { x[i - d] } else { 0.0 };
            z[i] = z[i - 1] + a * (u - z[i - 1]);
        }
        let lag = lag_of_max(&x, &z, 50).unwrap();
        assert!((lag - d as f64).abs() <= 1.0, "lag {lag}");
        // zero delay sits at the lower edge, no interpolation past it
        assert!(lag_of_max(&x, &x, 20).unwrap().abs() < 1e-12);
    }

    #[test]
    fn degenerate_inputs() {
        let x = noise(100, 1);
        assert_eq!(lag_of_max(&x, &x, 0), None, "no lags to search");
        assert_eq!(lag_of_max(&x[..5], &x[..5], 10), None, "too short");
        let flat = vec![3.0; 100];
        assert_eq!(lag_of_max(&x, &flat, 10), None, "constant output");
        let neg: Vec<f64> = x.iter().map(|v| -v).collect();
        assert_ne!(
            lag_of_max(&x, &neg, 10),
            Some(0.0),
            "a negative peak is never reported"
        );
        let ramp: Vec<f64> = (0..100).map(f64::from).collect();
        let down: Vec<f64> = ramp.iter().map(|v| -v).collect();
        assert_eq!(lag_of_max(&ramp, &down, 10), None, "no positive lag at all");
        // a peak at the edge is reported without interpolation
        let mut y = vec![0.0; 100];
        y[10..].copy_from_slice(&x[..90]);
        assert_eq!(lag_of_max(&x, &y, 10), Some(10.0));
    }
}
