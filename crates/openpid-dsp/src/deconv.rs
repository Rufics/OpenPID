//! Regularised (Wiener-style) deconvolution for system identification:
//! estimate the impulse/step response of the plant from an input signal
//! (setpoint) and the measured output (gyro).

use crate::fft::{Complex, Fft, rfft_freqs};
use crate::window::Window;
use crate::{DspError, next_smooth};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DeconvParams {
    /// Frequencies above this are treated as noise and suppressed.
    pub cutoff_hz: f64,
    /// Regularisation below the cutoff (relative to the input power), small.
    pub reg_low: f64,
    /// Regularisation above the cutoff, effectively infinite.
    pub reg_high: f64,
    /// Width of the raised-cosine transition between the two regimes, as a
    /// fraction of `cutoff_hz` on each side.
    pub transition: f64,
    /// Taper applied to both signals before the transform. Without it the
    /// unobserved tail of the response past the record end biases the gain
    /// (about 10 % low for a 4 s record of a 20 ms plant); Hann removes it.
    pub window: Window,
}

impl Default for DeconvParams {
    fn default() -> Self {
        DeconvParams {
            cutoff_hz: 25.0,
            reg_low: 0.1,
            reg_high: 1.0e8,
            transition: 0.2,
            window: Window::Hann,
        }
    }
}

/// Regularisation weight `λ(f)`: `reg_low` well below the cutoff, `reg_high`
/// well above, raised-cosine in between.
pub fn regularisation(freq: f64, p: &DeconvParams) -> f64 {
    let lo = p.cutoff_hz * (1.0 - p.transition);
    let hi = p.cutoff_hz * (1.0 + p.transition);
    if freq <= lo {
        p.reg_low
    } else if freq >= hi {
        p.reg_high
    } else {
        let t = (freq - lo) / (hi - lo);
        let s = 0.5 - 0.5 * (std::f64::consts::PI * t).cos();
        // interpolate in log space so the ramp is smooth over 9 decades
        (p.reg_low.ln() * (1.0 - s) + p.reg_high.ln() * s).exp()
    }
}

/// Impulse response `h[0..resp_len]` such that `output ≈ input * h`.
///
/// `H = G·conj(X) / (|X|² + λ(f))`, computed on a 5-smooth FFT length of at
/// least `1.5 × len` so the circular convolution does not wrap into the
/// response window. Both inputs must have the same length.
pub fn impulse_response(
    fft: &Fft,
    input: &[f64],
    output: &[f64],
    fs: f64,
    resp_len: usize,
    p: &DeconvParams,
) -> Result<Vec<f64>, DspError> {
    if input.len() != output.len() {
        return Err(DspError::InvalidParam(
            "input and output lengths differ".into(),
        ));
    }
    if input.len() < 2 || resp_len == 0 {
        return Err(DspError::TooShort {
            need: 2,
            got: input.len(),
        });
    }
    if resp_len > input.len() {
        return Err(DspError::InvalidParam(
            "resp_len longer than the signals".into(),
        ));
    }
    let n = next_smooth(input.len() + input.len() / 2);
    let w = p.window.values(input.len());
    let mut xi = vec![0.0; n];
    let mut yo = vec![0.0; n];
    for i in 0..input.len() {
        xi[i] = input[i] * w[i];
        yo[i] = output[i] * w[i];
    }
    let xs = fft.rfft(&xi);
    let ys = fft.rfft(&yo);
    let freqs = rfft_freqs(n, fs);
    let hs: Vec<Complex<f64>> = xs
        .iter()
        .zip(&ys)
        .zip(&freqs)
        .map(|((x, y), f)| {
            let lambda = regularisation(*f, p);
            (y * x.conj()) / (x.norm_sqr() + lambda)
        })
        .collect();
    let mut h = fft.irfft(&hs, n);
    h.truncate(resp_len);
    Ok(h)
}

/// Running sum of an impulse response: the step response.
pub fn step_from_impulse(h: &[f64]) -> Vec<f64> {
    let mut acc = 0.0;
    h.iter()
        .map(|v| {
            acc += v;
            acc
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A first-order plant `y' = (x - y) / tau` driven by random steps must be
    /// recovered as the exponential step response `1 - exp(-t/tau)`.
    #[test]
    fn recovers_first_order_plant() {
        let fft = Fft::new();
        let fs = 1000.0;
        let n = 4000;
        let tau = 0.02;
        let mut seed = 0x1234_5678u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 2000) as f64 - 1000.0
        };
        let mut x = vec![0.0; n];
        let mut level = 0.0;
        for (i, v) in x.iter_mut().enumerate() {
            if i % 150 == 0 {
                level = rnd();
            }
            *v = level;
        }
        let mut y = vec![0.0; n];
        let alpha = (1.0 / fs) / tau;
        for i in 1..n {
            y[i] = y[i - 1] + alpha * (x[i] - y[i - 1]);
        }
        let p = DeconvParams {
            cutoff_hz: 100.0,
            ..Default::default()
        };
        let h = impulse_response(&fft, &x, &y, fs, 300, &p).unwrap();
        let s = step_from_impulse(&h);
        // discrete plant: s[i] = 1 - (1 - alpha)^(i + 1)
        for (i, v) in s.iter().enumerate().step_by(25) {
            let expected = 1.0 - (1.0 - alpha).powi(i as i32 + 1);
            assert!(
                (v - expected).abs() < 0.03,
                "t={i}ms got {v} want {expected}"
            );
        }
        assert_eq!(regularisation(0.0, &p), p.reg_low);
        assert_eq!(regularisation(1000.0, &p), p.reg_high);
        let mid = regularisation(p.cutoff_hz, &p);
        assert!(mid > p.reg_low && mid < p.reg_high);
    }
}
