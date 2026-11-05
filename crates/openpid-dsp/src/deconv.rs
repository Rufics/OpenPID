//! Regularised (Wiener-style) deconvolution for system identification:
//! estimate the impulse/step response of the plant from an input signal
//! (setpoint) and the measured output (gyro).

use crate::fft::{Complex, Fft, rfft_freqs};
use crate::window::Window;
use crate::{DspError, next_smooth};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// Parameters of [`impulse_response`].
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

    /// Deterministic pseudo-random values in [-1, 1).
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

    /// `H = Y·conj(X) / (|X|² + λ)` is linear in the output for a fixed input
    /// and regularisation: `h(x, a·y₁ + b·y₂) = a·h(x, y₁) + b·h(x, y₂)`.
    #[test]
    fn linear_in_the_output_for_a_fixed_input() {
        let fft = Fft::new();
        let fs = 1000.0;
        let n = 2000;
        let x = noise(n, 1);
        let y1 = noise(n, 2);
        let y2 = noise(n, 3);
        let (a, b) = (2.5, -0.75);
        let mix: Vec<f64> = y1.iter().zip(&y2).map(|(p, q)| a * p + b * q).collect();
        for p in [
            DeconvParams::default(),
            DeconvParams {
                cutoff_hz: 400.0,
                window: Window::Rectangular,
                ..Default::default()
            },
        ] {
            let h1 = impulse_response(&fft, &x, &y1, fs, 100, &p).unwrap();
            let h2 = impulse_response(&fft, &x, &y2, fs, 100, &p).unwrap();
            let hm = impulse_response(&fft, &x, &mix, fs, 100, &p).unwrap();
            let scale = h1.iter().map(|v| v.abs()).fold(0.0, f64::max).max(1e-12);
            for k in 0..100 {
                let expected = a * h1[k] + b * h2[k];
                assert!(
                    (hm[k] - expected).abs() < 1e-9 * scale,
                    "tap {k}: {} vs {expected}",
                    hm[k]
                );
            }
        }
    }

    /// Slow exponential taps: a plant whose energy sits well below the
    /// deconvolution cutoff, so the regularisation does not bias it.
    fn slow_fir() -> Vec<f64> {
        (0..40).map(|k| 0.1 * 0.9f64.powi(k)).collect()
    }

    fn convolve(x: &[f64], taps: &[f64]) -> Vec<f64> {
        (0..x.len())
            .map(|i| {
                taps.iter()
                    .enumerate()
                    .filter(|(k, _)| i >= *k)
                    .map(|(k, t)| t * x[i - k])
                    .sum()
            })
            .collect()
    }

    /// A low-pass FIR plant driven by white noise is recovered tap by tap,
    /// and the step response is the running sum of the taps.
    #[test]
    fn recovers_a_fir_plant_and_its_step_is_the_cumulative_sum() {
        let fft = Fft::new();
        let fs = 1000.0;
        let taps = slow_fir();
        let x = noise(8000, 7);
        let y = convolve(&x, &taps);
        let p = DeconvParams {
            cutoff_hz: 350.0,
            reg_low: 1e-6,
            ..Default::default()
        };
        let h = impulse_response(&fft, &x, &y, fs, 60, &p).unwrap();
        // the regularisation blanks the band above the cutoff, which costs the
        // taps a little of their high-frequency content (≈ 15 % of tap 0 here);
        // inside the trusted band the estimate matches the plant's response
        let pad = 1024;
        let spectrum = |v: &[f64]| {
            let mut buf = vec![0.0; pad];
            buf[..v.len()].copy_from_slice(v);
            fft.rfft(&buf)
        };
        let (est, truth) = (spectrum(&h), spectrum(&taps));
        let trusted = ((0.75 * p.cutoff_hz) / (fs / pad as f64)) as usize;
        for k in 0..=trusted {
            assert!(
                (est[k] - truth[k]).norm() < 0.03 * truth[0].norm(),
                "bin {k}: {} vs {}",
                est[k],
                truth[k]
            );
        }
        for (k, t) in taps.iter().enumerate() {
            assert!((h[k] - t).abs() < 0.02, "tap {k}: {} vs {t}", h[k]);
        }
        assert!(
            h[40..].iter().all(|v| v.abs() < 0.01),
            "nothing after the last tap"
        );
        let step = step_from_impulse(&h);
        let mut acc = 0.0;
        for (k, v) in step.iter().enumerate() {
            acc += h[k];
            assert!((v - acc).abs() < 1e-12, "running sum at {k}");
        }
        let total: f64 = taps.iter().sum();
        assert!(
            (step[59] - total).abs() < 0.03,
            "settles at the DC gain {total}: {}",
            step[59]
        );
        assert_eq!(step_from_impulse(&[1.0, 2.0, 3.0]), vec![1.0, 3.0, 6.0]);
        assert!(step_from_impulse(&[]).is_empty());
    }

    /// The periodic Hann taper is symmetric about the record centre
    /// (`w[i] == w[n − i]`, `w[0] == 0`), and tapering or not gives the same
    /// DC gain for a plant inside the cutoff.
    #[test]
    fn hann_taper_is_symmetric_and_does_not_change_the_dc_gain() {
        for n in [16usize, 257, 1000] {
            let w = Window::Hann.values(n);
            assert_eq!(w[0], 0.0);
            for i in 1..n {
                assert!((w[i] - w[n - i]).abs() < 1e-12, "n {n}, i {i}");
            }
            assert!(w.iter().all(|v| (0.0..=1.0 + 1e-12).contains(v)));
        }
        let fft = Fft::new();
        let fs = 1000.0;
        let taps = slow_fir();
        let x = noise(8000, 11);
        let y = convolve(&x, &taps);
        let total: f64 = taps.iter().sum();
        let mut gains = Vec::new();
        for window in [Window::Hann, Window::Rectangular, Window::Hamming] {
            let p = DeconvParams {
                cutoff_hz: 350.0,
                reg_low: 1e-6,
                window,
                ..Default::default()
            };
            let h = impulse_response(&fft, &x, &y, fs, 60, &p).unwrap();
            let g = step_from_impulse(&h)[59];
            assert!((g - total).abs() < 0.03, "{window:?}: {g} vs {total}");
            gains.push(g);
        }
        assert!(
            (gains[0] - gains[1]).abs() < 0.02,
            "Hann vs rectangular: {gains:?}"
        );
    }
}
