//! Thin wrapper around `rustfft` with cached plans and real-input helpers.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use rustfft::FftPlanner;
pub use rustfft::num_complex::Complex;

/// FFT engine with plan cache. Not `Sync`; create one per thread.
pub struct Fft {
    planner: RefCell<FftPlanner<f64>>,
    forward: RefCell<HashMap<usize, Arc<dyn rustfft::Fft<f64>>>>,
    inverse: RefCell<HashMap<usize, Arc<dyn rustfft::Fft<f64>>>>,
}

impl std::fmt::Debug for Fft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fft")
            .field("cached_forward", &self.forward.borrow().len())
            .field("cached_inverse", &self.inverse.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Default for Fft {
    fn default() -> Self {
        Self::new()
    }
}

impl Fft {
    pub fn new() -> Self {
        Fft {
            planner: RefCell::new(FftPlanner::new()),
            forward: RefCell::default(),
            inverse: RefCell::default(),
        }
    }

    fn plan(&self, n: usize, inverse: bool) -> Arc<dyn rustfft::Fft<f64>> {
        let cache = if inverse {
            &self.inverse
        } else {
            &self.forward
        };
        if let Some(p) = cache.borrow().get(&n) {
            return Arc::clone(p);
        }
        let mut planner = self.planner.borrow_mut();
        let p = if inverse {
            planner.plan_fft_inverse(n)
        } else {
            planner.plan_fft_forward(n)
        };
        cache.borrow_mut().insert(n, Arc::clone(&p));
        p
    }

    /// Unnormalised forward DFT of a complex buffer, in place.
    pub fn forward_inplace(&self, buf: &mut [Complex<f64>]) {
        self.plan(buf.len(), false).process(buf);
    }

    /// Inverse DFT in place, normalised by `1/n` (so `inverse(forward(x)) == x`).
    pub fn inverse_inplace(&self, buf: &mut [Complex<f64>]) {
        let n = buf.len();
        self.plan(n, true).process(buf);
        let k = 1.0 / n as f64;
        for v in buf.iter_mut() {
            *v *= k;
        }
    }

    /// Forward DFT of a real signal, full spectrum (`n` bins).
    pub fn forward_real(&self, x: &[f64]) -> Vec<Complex<f64>> {
        let mut buf: Vec<Complex<f64>> = x.iter().map(|v| Complex::new(*v, 0.0)).collect();
        self.forward_inplace(&mut buf);
        buf
    }

    /// One-sided spectrum of a real signal: bins `0..=n/2` (like `numpy.fft.rfft`).
    pub fn rfft(&self, x: &[f64]) -> Vec<Complex<f64>> {
        let mut full = self.forward_real(x);
        full.truncate(x.len() / 2 + 1);
        full
    }

    /// Inverse of [`Fft::rfft`] for an output length `n` (like `numpy.fft.irfft`).
    pub fn irfft(&self, spec: &[Complex<f64>], n: usize) -> Vec<f64> {
        assert!(spec.len() > n / 2, "spectrum too short for n = {n}");
        let mut buf = vec![Complex::new(0.0, 0.0); n];
        for (k, v) in spec.iter().take(n / 2 + 1).enumerate() {
            buf[k] = *v;
            if k > 0 && k < n.div_ceil(2) {
                buf[n - k] = v.conj();
            }
        }
        self.inverse_inplace(&mut buf);
        buf.into_iter().map(|c| c.re).collect()
    }
}

/// Frequencies of the one-sided spectrum bins for sample rate `fs` (like `numpy.fft.rfftfreq`).
pub fn rfft_freqs(n: usize, fs: f64) -> Vec<f64> {
    (0..=n / 2).map(|k| k as f64 * fs / n as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_parseval() {
        let fft = Fft::new();
        for n in [1usize, 2, 7, 64, 100, 1000] {
            let x: Vec<f64> = (0..n).map(|i| ((i * 7919) % 13) as f64 - 6.0).collect();
            let spec = fft.forward_real(&x);
            let energy_t: f64 = x.iter().map(|v| v * v).sum();
            let energy_f: f64 = spec.iter().map(Complex::norm_sqr).sum::<f64>() / n as f64;
            assert!(
                (energy_t - energy_f).abs() <= 1e-9 * energy_t.max(1.0),
                "parseval n={n}"
            );
            let r = fft.rfft(&x);
            let back = fft.irfft(&r, n);
            for (a, b) in x.iter().zip(&back) {
                assert!((a - b).abs() < 1e-9, "roundtrip n={n}");
            }
        }
        assert_eq!(rfft_freqs(8, 1000.0), vec![0.0, 125.0, 250.0, 375.0, 500.0]);
    }
}
