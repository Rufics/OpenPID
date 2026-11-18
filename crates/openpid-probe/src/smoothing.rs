//! The line smoothing of the reference tool's main window, as round 16
//! observed it (`docs/calibration/log-viewer.md`): each level is a local
//! quadratic regression with the tricube weights `(1 − (d/d_max)³)³` over a
//! span of 11 ("low"), 21 ("med"), 41 ("med-high") or 81 samples ("high"),
//! centred on the sample; round 17 saw the span counted in samples at 2 and
//! 4 kHz too.
//!
//! This is a model of the observed rule, written for `openpid-probe
//! compare`: `OpenPID`'s own line smoothing (decision D-011) is not on the
//! base of this crate, so the copies are held against the rule itself.
//! Near the ends of a series, where the reference tool's behaviour was not
//! asked, the window is shifted to stay inside the samples and the fit is
//! still read at the sample.

/// The levels of the list by the names the copies carry, with their spans
/// in samples.
pub const LEVELS: [(&str, usize); 4] = [("low", 11), ("med", 21), ("med-high", 41), ("high", 81)];

/// The span of a level of the list, by its name in the copies.
pub fn span_of(level: &str) -> Option<usize> {
    LEVELS
        .iter()
        .find(|(name, _)| *name == level)
        .map(|(_, s)| *s)
}

/// The tricube weight of a distance scaled to −1 … 1.
fn tricube(u: f64) -> f64 {
    let a = 1.0 - u.abs().powi(3);
    if a <= 0.0 { 0.0 } else { a * a * a }
}

/// The weighted least-squares parabola through `(u, w, y)`, read at
/// `u = 0`; `None` when the points do not hold one.
fn parabola_at_zero(points: impl Iterator<Item = (f64, f64, f64)>) -> Option<f64> {
    // the moments Σ w·u^k (k = 0 … 4) and Σ w·u^k·y (k = 0 … 2)
    let mut m = [0.0f64; 5];
    let mut r = [0.0f64; 3];
    for (u, w, y) in points {
        let mut p = w;
        for (k, mk) in m.iter_mut().enumerate() {
            *mk += p;
            if k < 3 {
                r[k] += p * y;
            }
            p *= u;
        }
    }
    let [s0, s1, s2, s3, s4] = m;
    let [t0, t1, t2] = r;
    let det = s0 * (s2 * s4 - s3 * s3) - s1 * (s1 * s4 - s3 * s2) + s2 * (s1 * s3 - s2 * s2);
    if det.abs() <= 1e-12 * (s0 * s2 * s4).abs().max(f64::MIN_POSITIVE) {
        return None;
    }
    let num = t0 * (s2 * s4 - s3 * s3) - s1 * (t1 * s4 - s3 * t2) + s2 * (t1 * s3 - s2 * t2);
    Some(num / det)
}

/// The samples `y` smoothed over `span` samples (made odd); a span below
/// three leaves them as they are.
pub fn smoothed(y: &[f64], span: usize) -> Vec<f64> {
    let n = y.len();
    let span = span | 1;
    if span < 3 || n < span {
        return y.to_vec();
    }
    let half = (span - 1) / 2;
    // the weights of a centred window, computed once
    let centred: Vec<f64> = (0..span)
        .map(|k| {
            let points = (0..span).map(|j| {
                let u = (j as f64 - half as f64) / half as f64;
                (u, tricube(u), if j == k { 1.0 } else { 0.0 })
            });
            parabola_at_zero(points).unwrap_or(0.0)
        })
        .collect();
    (0..n)
        .map(|i| {
            if i >= half && i + half < n {
                return centred
                    .iter()
                    .zip(&y[i - half..=i + half])
                    .map(|(k, v)| k * v)
                    .sum();
            }
            let lo = i.saturating_sub(half).min(n - span);
            let reach = (i - lo).max(lo + span - 1 - i) as f64;
            let points = (lo..lo + span).map(|j| {
                let u = (j as f64 - i as f64) / reach;
                (u, tricube(u), y[j])
            });
            parabola_at_zero(points).unwrap_or(y[i])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pulse_comes_back_as_the_kernel_observed() {
        // log-viewer.md: the one-sample pulse comes back at 0.317, 0.159,
        // 0.079 and 0.040 of its height
        for ((name, span), peak) in LEVELS.into_iter().zip([0.317, 0.159, 0.079, 0.040]) {
            let mut y = vec![0.0; 301];
            y[150] = 1.0;
            let s = smoothed(&y, span);
            assert!((s[150] - peak).abs() < 0.0006, "{name}: {}", s[150]);
            assert_eq!(span_of(name), Some(span));
            // side lobes below zero, nothing beyond the span
            assert!(s[150 + span / 2 - 1] < 0.0, "{name}");
            assert!(s[150 + span / 2 + 1].abs() < 1e-12, "{name}");
        }
        assert_eq!(span_of("off"), None);
    }

    #[test]
    fn a_parabola_is_kept_and_the_ends_are_fitted() {
        let y: Vec<f64> = (0..50)
            .map(|i| 3.0 + 0.5 * f64::from(i) - 0.02 * f64::from(i * i))
            .collect();
        let s = smoothed(&y, 21);
        for (a, b) in y.iter().zip(&s) {
            assert!((a - b).abs() < 1e-9, "{a} {b}");
        }
        assert_eq!(smoothed(&y[..5], 21), y[..5].to_vec());
        assert_eq!(smoothed(&y, 1), y);
    }
}
