//! Welch power spectral density, STFT and single-sided amplitude spectra.

use crate::DspError;
use crate::fft::{Fft, rfft_freqs};
use crate::window::Window;
use serde::{Deserialize, Serialize};

/// Power spectral density, one-sided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Psd {
    pub freq: Vec<f64>,
    /// Power density in `unit² / Hz`.
    pub power: Vec<f64>,
    /// Number of segments averaged.
    pub segments: usize,
}

/// Parameters of [`welch`]; defaults mirror `scipy.signal.welch`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WelchParams {
    pub nperseg: usize,
    pub noverlap: usize,
    pub window: Window,
    /// Remove the mean of every segment before windowing (`detrend="constant"`).
    pub detrend: bool,
}

impl WelchParams {
    pub fn new(nperseg: usize) -> Self {
        WelchParams {
            nperseg,
            noverlap: nperseg / 2,
            window: Window::Hann,
            detrend: true,
        }
    }
}

/// Welch PSD estimate. Matches `scipy.signal.welch(x, fs, "hann", nperseg, noverlap,
/// detrend="constant", scaling="density", average="mean")`.
pub fn welch(fft: &Fft, x: &[f64], fs: f64, p: &WelchParams) -> Result<Psd, DspError> {
    if p.nperseg < 2 {
        return Err(DspError::InvalidParam("nperseg must be >= 2".into()));
    }
    if p.noverlap >= p.nperseg {
        return Err(DspError::InvalidParam("noverlap must be < nperseg".into()));
    }
    if x.len() < p.nperseg {
        return Err(DspError::TooShort {
            need: p.nperseg,
            got: x.len(),
        });
    }
    let n = p.nperseg;
    let step = n - p.noverlap;
    let w = p.window.values(n);
    let scale = 1.0 / (fs * w.iter().map(|v| v * v).sum::<f64>());
    let nbins = n / 2 + 1;
    let mut acc = vec![0.0f64; nbins];
    let mut segments = 0usize;
    let mut buf = vec![0.0f64; n];
    let mut start = 0;
    while start + n <= x.len() {
        let seg = &x[start..start + n];
        let mean = if p.detrend {
            seg.iter().sum::<f64>() / n as f64
        } else {
            0.0
        };
        for i in 0..n {
            buf[i] = (seg[i] - mean) * w[i];
        }
        let spec = fft.rfft(&buf);
        for (k, c) in spec.iter().enumerate() {
            acc[k] += c.norm_sqr();
        }
        segments += 1;
        start += step;
    }
    let mut power: Vec<f64> = acc.iter().map(|v| v * scale / segments as f64).collect();
    // one-sided: double everything except DC and (for even n) Nyquist
    let last = if n.is_multiple_of(2) {
        nbins - 1
    } else {
        nbins
    };
    for v in power.iter_mut().take(last).skip(1) {
        *v *= 2.0;
    }
    Ok(Psd {
        freq: rfft_freqs(n, fs),
        power,
        segments,
    })
}

/// Single-sided amplitude spectrum of one windowed segment: `|X_k| / sum(w)`,
/// doubled for `0 < k < n/2` so that a sine of amplitude `A` reads `A` at its bin.
pub fn amplitude_spectrum(fft: &Fft, x: &[f64], window: Window) -> Vec<f64> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    let w = window.values(n);
    let wsum: f64 = w.iter().sum();
    let buf: Vec<f64> = x.iter().zip(&w).map(|(a, b)| a * b).collect();
    let spec = fft.rfft(&buf);
    let nbins = n / 2 + 1;
    let last = if n.is_multiple_of(2) {
        nbins - 1
    } else {
        nbins
    };
    spec.iter()
        .enumerate()
        .map(|(k, c)| {
            let a = c.norm() / wsum;
            if k > 0 && k < last { 2.0 * a } else { a }
        })
        .collect()
}

/// Short-time amplitude spectrogram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spectrogram {
    /// Centre time of every segment, seconds from the first sample.
    pub time: Vec<f64>,
    pub freq: Vec<f64>,
    /// Row-major `[time][freq]` amplitudes.
    pub amp: Vec<f64>,
}

impl Spectrogram {
    pub fn get(&self, ti: usize, fi: usize) -> f64 {
        self.amp[ti * self.freq.len() + fi]
    }
}

/// STFT with amplitude normalisation of [`amplitude_spectrum`].
pub fn stft(
    fft: &Fft,
    x: &[f64],
    fs: f64,
    nperseg: usize,
    hop: usize,
    window: Window,
) -> Result<Spectrogram, DspError> {
    if nperseg < 2 || hop == 0 {
        return Err(DspError::InvalidParam(
            "nperseg >= 2 and hop >= 1 required".into(),
        ));
    }
    if x.len() < nperseg {
        return Err(DspError::TooShort {
            need: nperseg,
            got: x.len(),
        });
    }
    let freq = rfft_freqs(nperseg, fs);
    let mut time = Vec::new();
    let mut amp = Vec::new();
    let mut start = 0;
    while start + nperseg <= x.len() {
        time.push((start as f64 + nperseg as f64 / 2.0) / fs);
        amp.extend(amplitude_spectrum(fft, &x[start..start + nperseg], window));
        start += hop;
    }
    Ok(Spectrogram { time, freq, amp })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_amplitude_and_psd_power() {
        let fft = Fft::new();
        let fs = 1000.0;
        let n = 1000;
        let a = 3.0;
        let x: Vec<f64> = (0..n)
            .map(|i| a * (std::f64::consts::TAU * 50.0 * f64::from(i) / fs).sin())
            .collect();
        let amp = amplitude_spectrum(&fft, &x, Window::Hann);
        assert!(
            (amp[50] - a).abs() < 1e-9,
            "amplitude at 50 Hz = {}",
            amp[50]
        );
        // PSD integrates to the signal variance (a²/2) for a pure tone
        let psd = welch(&fft, &x, fs, &WelchParams::new(500)).unwrap();
        let df = psd.freq[1] - psd.freq[0];
        let total: f64 = psd.power.iter().sum::<f64>() * df;
        assert!(
            (total - a * a / 2.0).abs() < 0.02 * a * a,
            "variance {total}"
        );
        assert_eq!(psd.segments, 3);
        assert!(matches!(
            welch(&fft, &x[..100], fs, &WelchParams::new(500)),
            Err(DspError::TooShort { .. })
        ));
    }
}
