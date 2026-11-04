//! Resampling of irregularly timed samples onto a uniform grid.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Uniform {
    /// Time of the first grid sample, microseconds.
    pub start_us: i64,
    pub dt_us: f64,
    pub values: Vec<f64>,
    /// `false` where the grid point falls inside a gap or next to an invalid sample.
    pub valid: Vec<bool>,
}

impl Uniform {
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn sample_rate_hz(&self) -> f64 {
        1.0e6 / self.dt_us
    }
}

/// Linear interpolation of `(time_us[i], x[i])` onto a grid with step `dt_us`
/// starting at `time_us[0]`. Grid points bracketed by two source samples that
/// are more than `max_gap_us` apart, or by an invalid source sample, are
/// marked invalid (their value is 0.0).
pub fn to_uniform(
    time_us: &[i64],
    x: &[f64],
    valid: Option<&[bool]>,
    dt_us: f64,
    max_gap_us: f64,
) -> Uniform {
    assert_eq!(time_us.len(), x.len());
    if time_us.is_empty() || dt_us <= 0.0 {
        return Uniform {
            start_us: 0,
            dt_us: dt_us.max(1.0),
            values: Vec::new(),
            valid: Vec::new(),
        };
    }
    let start = time_us[0];
    let span = (time_us[time_us.len() - 1] - start) as f64;
    let n = (span / dt_us).floor() as usize + 1;
    let mut values = Vec::with_capacity(n);
    let mut flags = Vec::with_capacity(n);
    let is_ok = |i: usize| valid.is_none_or(|v| v[i]);
    let mut j = 0usize; // index of the source sample at or before the grid point
    for k in 0..n {
        let t = start as f64 + k as f64 * dt_us;
        while j + 1 < time_us.len() && (time_us[j + 1] as f64) <= t {
            j += 1;
        }
        if (time_us[j] as f64 - t).abs() < 1e-9 || j + 1 >= time_us.len() {
            values.push(x[j]);
            flags.push(is_ok(j));
            continue;
        }
        let t0 = time_us[j] as f64;
        let t1 = time_us[j + 1] as f64;
        let gap = t1 - t0;
        if gap > max_gap_us || !is_ok(j) || !is_ok(j + 1) {
            values.push(0.0);
            flags.push(false);
        } else {
            let a = (t - t0) / gap;
            values.push(x[j] + a * (x[j + 1] - x[j]));
            flags.push(true);
        }
    }
    Uniform {
        start_us: start,
        dt_us,
        values,
        valid: flags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_and_flags_gaps() {
        let t = [0i64, 1000, 2000, 5000, 6000];
        let x = [0.0, 1.0, 2.0, 5.0, 6.0];
        let u = to_uniform(&t, &x, None, 1000.0, 1500.0);
        assert_eq!(u.values.len(), 7);
        assert_eq!(u.valid, vec![true, true, true, false, false, true, true]);
        assert_eq!(u.values[2], 2.0);
        assert_eq!(u.values[6], 6.0);
        let u = to_uniform(&t, &x, None, 500.0, 1500.0);
        assert!((u.values[1] - 0.5).abs() < 1e-12);
        assert!(!u.valid[7]);
        let v = [true, false, true, true, true];
        let u = to_uniform(&t, &x, Some(&v), 500.0, 1500.0);
        assert!(!u.valid[1] && !u.valid[2] && !u.valid[3] && u.valid[4]);
    }
}
