//! Window functions with their normalisation constants.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Window {
    Rectangular,
    /// Periodic Hann (`scipy.signal.get_window("hann", n)`), the default everywhere.
    #[default]
    Hann,
    Hamming,
    BlackmanHarris,
    /// Tukey with taper fraction `alpha` in `0..=1`.
    Tukey(f64),
}

impl Window {
    /// Window coefficients, periodic ("DFT-even") variants as used for spectral analysis.
    pub fn values(self, n: usize) -> Vec<f64> {
        if n == 0 {
            return Vec::new();
        }
        let nf = n as f64;
        let tau = std::f64::consts::TAU;
        (0..n)
            .map(|i| {
                let x = i as f64 / nf; // periodic: divide by n, not n-1
                match self {
                    Window::Rectangular => 1.0,
                    Window::Hann => 0.5 - 0.5 * (tau * x).cos(),
                    Window::Hamming => 0.54 - 0.46 * (tau * x).cos(),
                    Window::BlackmanHarris => {
                        0.35875 - 0.48829 * (tau * x).cos() + 0.14128 * (2.0 * tau * x).cos()
                            - 0.01168 * (3.0 * tau * x).cos()
                    }
                    Window::Tukey(alpha) => {
                        let alpha = alpha.clamp(0.0, 1.0);
                        if alpha == 0.0 {
                            return 1.0;
                        }
                        let edge = alpha / 2.0;
                        if x < edge {
                            0.5 * (1.0 + (tau * (x / alpha - 0.5)).cos())
                        } else if x < 1.0 - edge {
                            1.0
                        } else {
                            0.5 * (1.0 + (tau * ((x - 1.0) / alpha + 0.5)).cos())
                        }
                    }
                }
            })
            .collect()
    }

    /// Sum of coefficients divided by `n` (amplitude correction factor).
    pub fn coherent_gain(self, n: usize) -> f64 {
        let w = self.values(n);
        w.iter().sum::<f64>() / n as f64
    }

    /// Sum of squared coefficients divided by `n` (power correction factor).
    pub fn power_gain(self, n: usize) -> f64 {
        let w = self.values(n);
        w.iter().map(|v| v * v).sum::<f64>() / n as f64
    }

    /// Equivalent noise bandwidth in bins.
    pub fn enbw_bins(self, n: usize) -> f64 {
        let w = self.values(n);
        let s1: f64 = w.iter().sum();
        let s2: f64 = w.iter().map(|v| v * v).sum();
        n as f64 * s2 / (s1 * s1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_properties() {
        let w = Window::Hann.values(8);
        assert!((w[0]).abs() < 1e-15);
        assert!((w[4] - 1.0).abs() < 1e-15);
        assert!((Window::Hann.coherent_gain(1024) - 0.5).abs() < 1e-12);
        assert!((Window::Hann.power_gain(1024) - 0.375).abs() < 1e-12);
        assert!((Window::Hann.enbw_bins(1024) - 1.5).abs() < 1e-12);
        assert_eq!(Window::Rectangular.values(3), vec![1.0; 3]);
        assert_eq!(Window::Tukey(0.0).values(4), vec![1.0; 4]);
        let t = Window::Tukey(1.0).values(16);
        let h = Window::Hann.values(16);
        for (a, b) in t.iter().zip(&h) {
            assert!((a - b).abs() < 1e-12, "tukey(1) == hann");
        }
    }
}
