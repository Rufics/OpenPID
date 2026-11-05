//! Filter primitives: PT1, cascaded PT1 (PT2/PT3), biquad low-pass and notch.

use openpid_dsp::Complex;
use std::f64::consts::PI;

/// A causal, time-invariant filter at a fixed sample rate.
pub trait Filter {
    /// Clear the state (output and history to zero).
    fn reset(&mut self);
    /// Feed one sample and get the filtered one.
    fn update(&mut self, x: f64) -> f64;
    /// Steady-state complex gain at `f_hz` (the transfer function evaluated
    /// on the unit circle at `z = e^{j 2π f / fs}`).
    fn response(&self, f_hz: f64) -> Complex<f64>;
    /// Sample rate in Hz.
    fn sample_rate(&self) -> f64;

    /// Magnitude of [`Filter::response`] in dB.
    fn gain_db(&self, f_hz: f64) -> f64 {
        20.0 * self.response(f_hz).norm().log10()
    }

    /// Phase of [`Filter::response`] in radians, in (−π, π].
    fn phase(&self, f_hz: f64) -> f64 {
        self.response(f_hz).arg()
    }
}

/// `z^{-1}` at `f_hz` for sample rate `fs`.
fn z_inv(f_hz: f64, fs: f64) -> Complex<f64> {
    Complex::from_polar(1.0, -2.0 * PI * f_hz / fs)
}

/// First-order low-pass (`PT1`): `y += k (x − y)` with `k = dt / (RC + dt)`,
/// `RC = 1 / (2π f_c)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pt1 {
    k: f64,
    y: f64,
    fs: f64,
}

impl Pt1 {
    /// Low-pass with cutoff `cutoff_hz` at sample rate `fs`.
    ///
    /// # Panics
    /// Panics unless `0 < cutoff_hz` and `0 < fs`.
    pub fn new(cutoff_hz: f64, fs: f64) -> Self {
        assert!(
            cutoff_hz > 0.0 && fs > 0.0,
            "cutoff and fs must be positive"
        );
        let rc = 1.0 / (2.0 * PI * cutoff_hz);
        let dt = 1.0 / fs;
        Pt1 {
            k: dt / (rc + dt),
            y: 0.0,
            fs,
        }
    }

    /// The per-sample gain `k`.
    pub fn gain(&self) -> f64 {
        self.k
    }
}

impl Filter for Pt1 {
    fn reset(&mut self) {
        self.y = 0.0;
    }

    fn update(&mut self, x: f64) -> f64 {
        self.y += self.k * (x - self.y);
        self.y
    }

    fn response(&self, f_hz: f64) -> Complex<f64> {
        // H(z) = k / (1 − (1 − k) z⁻¹)
        Complex::new(self.k, 0.0) / (Complex::new(1.0, 0.0) - z_inv(f_hz, self.fs) * (1.0 - self.k))
    }

    fn sample_rate(&self) -> f64 {
        self.fs
    }
}

/// Cutoff correction of an `order`-stage PT1 cascade: each stage is set to
/// `f_c × correction` so that the cascade is at −3 dB at `f_c`, i.e.
/// `1 / sqrt(2^(1/order) − 1)` (≈ 1.554 for PT2, ≈ 1.961 for PT3).
///
/// # Panics
/// Panics when `order` is zero.
pub fn pt_cutoff_correction(order: u32) -> f64 {
    assert!(order > 0, "order must be at least 1");
    1.0 / (2f64.powf(1.0 / f64::from(order)) - 1.0).sqrt()
}

/// `order` cascaded PT1 stages with the cutoff correction (`PT2`, `PT3`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ptn {
    stages: Vec<Pt1>,
}

impl Ptn {
    /// `order` cascaded stages with overall cutoff `cutoff_hz`.
    ///
    /// # Panics
    /// Panics unless `order >= 1`, `0 < cutoff_hz` and `0 < fs`.
    pub fn new(order: u32, cutoff_hz: f64, fs: f64) -> Self {
        let corrected = cutoff_hz * pt_cutoff_correction(order);
        Ptn {
            stages: (0..order).map(|_| Pt1::new(corrected, fs)).collect(),
        }
    }

    /// Number of stages.
    pub fn order(&self) -> usize {
        self.stages.len()
    }
}

impl Filter for Ptn {
    fn reset(&mut self) {
        self.stages.iter_mut().for_each(Pt1::reset);
    }

    fn update(&mut self, x: f64) -> f64 {
        self.stages.iter_mut().fold(x, |v, s| s.update(v))
    }

    fn response(&self, f_hz: f64) -> Complex<f64> {
        self.stages
            .iter()
            .fold(Complex::new(1.0, 0.0), |h, s| h * s.response(f_hz))
    }

    fn sample_rate(&self) -> f64 {
        self.stages[0].fs
    }
}

/// Betaflight's notch quality from centre and cutoff frequency:
/// `Q = f_center · f_cutoff / (f_center² − f_cutoff²)`.
///
/// # Panics
/// Panics unless `0 < cutoff_hz < center_hz`.
pub fn notch_q(center_hz: f64, cutoff_hz: f64) -> f64 {
    assert!(
        cutoff_hz > 0.0 && cutoff_hz < center_hz,
        "notch needs 0 < cutoff < center"
    );
    center_hz * cutoff_hz / (center_hz * center_hz - cutoff_hz * cutoff_hz)
}

/// Second-order IIR section (RBJ cookbook), direct form II transposed.
#[derive(Debug, Clone, PartialEq)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: f64,
    s2: f64,
    fs: f64,
}

impl Biquad {
    fn from_rbj(fs: f64, b: [f64; 3], a: [f64; 3]) -> Self {
        Biquad {
            b0: b[0] / a[0],
            b1: b[1] / a[0],
            b2: b[2] / a[0],
            a1: a[1] / a[0],
            a2: a[2] / a[0],
            s1: 0.0,
            s2: 0.0,
            fs,
        }
    }

    fn omega(f_hz: f64, fs: f64) -> (f64, f64) {
        let w = 2.0 * PI * f_hz / fs;
        (w.sin(), w.cos())
    }

    /// Butterworth low-pass (`Q = 1/√2`) with cutoff `cutoff_hz`.
    ///
    /// # Panics
    /// Panics unless `0 < cutoff_hz < fs / 2`.
    pub fn lowpass(cutoff_hz: f64, fs: f64) -> Self {
        Self::lowpass_q(cutoff_hz, std::f64::consts::FRAC_1_SQRT_2, fs)
    }

    /// Low-pass with an explicit quality factor.
    ///
    /// # Panics
    /// Panics unless `0 < cutoff_hz < fs / 2` and `q > 0`.
    pub fn lowpass_q(cutoff_hz: f64, q: f64, fs: f64) -> Self {
        assert!(
            cutoff_hz > 0.0 && cutoff_hz < fs / 2.0 && q > 0.0,
            "lowpass needs 0 < cutoff < fs/2 and q > 0"
        );
        let (sn, cs) = Self::omega(cutoff_hz, fs);
        let alpha = sn / (2.0 * q);
        Self::from_rbj(
            fs,
            [(1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0],
            [1.0 + alpha, -2.0 * cs, 1.0 - alpha],
        )
    }

    /// Notch at `center_hz` with the −3 dB edge at `cutoff_hz` (see [`notch_q`]).
    ///
    /// # Panics
    /// Panics unless `0 < cutoff_hz < center_hz < fs / 2`.
    pub fn notch(center_hz: f64, cutoff_hz: f64, fs: f64) -> Self {
        Self::notch_q(center_hz, notch_q(center_hz, cutoff_hz), fs)
    }

    /// Notch at `center_hz` with quality `q`.
    ///
    /// # Panics
    /// Panics unless `0 < center_hz < fs / 2` and `q > 0`.
    pub fn notch_q(center_hz: f64, q: f64, fs: f64) -> Self {
        assert!(
            center_hz > 0.0 && center_hz < fs / 2.0 && q > 0.0,
            "notch needs 0 < center < fs/2 and q > 0"
        );
        let (sn, cs) = Self::omega(center_hz, fs);
        let alpha = sn / (2.0 * q);
        Self::from_rbj(
            fs,
            [1.0, -2.0 * cs, 1.0],
            [1.0 + alpha, -2.0 * cs, 1.0 - alpha],
        )
    }

    /// Normalised coefficients `(b0, b1, b2, a1, a2)` with `a0 = 1`.
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64) {
        (self.b0, self.b1, self.b2, self.a1, self.a2)
    }
}

impl Filter for Biquad {
    fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    fn update(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }

    fn response(&self, f_hz: f64) -> Complex<f64> {
        let z1 = z_inv(f_hz, self.fs);
        let z2 = z1 * z1;
        let num = Complex::new(self.b0, 0.0) + z1 * self.b1 + z2 * self.b2;
        let den = Complex::new(1.0, 0.0) + z1 * self.a1 + z2 * self.a2;
        num / den
    }

    fn sample_rate(&self) -> f64 {
        self.fs
    }
}

/// One stage of a [`Chain`].
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// First-order low-pass.
    Pt1(Pt1),
    /// Cascaded PT1 low-pass (PT2/PT3).
    Ptn(Ptn),
    /// Biquad low-pass or notch.
    Biquad(Biquad),
}

impl Filter for Stage {
    fn reset(&mut self) {
        match self {
            Stage::Pt1(f) => f.reset(),
            Stage::Ptn(f) => f.reset(),
            Stage::Biquad(f) => f.reset(),
        }
    }

    fn update(&mut self, x: f64) -> f64 {
        match self {
            Stage::Pt1(f) => f.update(x),
            Stage::Ptn(f) => f.update(x),
            Stage::Biquad(f) => f.update(x),
        }
    }

    fn response(&self, f_hz: f64) -> Complex<f64> {
        match self {
            Stage::Pt1(f) => f.response(f_hz),
            Stage::Ptn(f) => f.response(f_hz),
            Stage::Biquad(f) => f.response(f_hz),
        }
    }

    fn sample_rate(&self) -> f64 {
        match self {
            Stage::Pt1(f) => f.sample_rate(),
            Stage::Ptn(f) => f.sample_rate(),
            Stage::Biquad(f) => f.sample_rate(),
        }
    }
}

/// Filters applied one after the other, as a firmware's gyro or D-term chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    stages: Vec<Stage>,
    fs: f64,
}

impl Chain {
    /// An empty (pass-through) chain at sample rate `fs`.
    pub fn new(fs: f64) -> Self {
        Chain {
            stages: Vec::new(),
            fs,
        }
    }

    /// Append a stage (its sample rate must match the chain's).
    ///
    /// # Panics
    /// Panics if the stage runs at another sample rate.
    pub fn push(&mut self, stage: Stage) -> &mut Self {
        assert!(
            (stage.sample_rate() - self.fs).abs() < 1e-9,
            "stage sample rate differs from the chain's"
        );
        self.stages.push(stage);
        self
    }

    /// The stages in order.
    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }

    /// Run a whole signal through a fresh copy of the chain.
    pub fn filter_signal(&self, x: &[f64]) -> Vec<f64> {
        let mut c = self.clone();
        c.reset();
        x.iter().map(|&v| c.update(v)).collect()
    }
}

impl Filter for Chain {
    fn reset(&mut self) {
        self.stages.iter_mut().for_each(Stage::reset);
    }

    fn update(&mut self, x: f64) -> f64 {
        self.stages.iter_mut().fold(x, |v, s| s.update(v))
    }

    fn response(&self, f_hz: f64) -> Complex<f64> {
        self.stages
            .iter()
            .fold(Complex::new(1.0, 0.0), |h, s| h * s.response(f_hz))
    }

    fn sample_rate(&self) -> f64 {
        self.fs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 8000.0;

    /// Steady-state amplitude and phase of a sine at `f` after running it
    /// through `filter`, measured by projection on sin/cos over whole periods.
    fn measured_response(filter: &mut dyn Filter, f: f64) -> Complex<f64> {
        filter.reset();
        let fs = filter.sample_rate();
        let periods = 200.0;
        let n = (periods * fs / f).round() as usize;
        let settle = n; // one more pass to reach steady state
        let (mut re, mut im) = (0.0, 0.0);
        for i in 0..settle + n {
            let t = i as f64 / fs;
            let y = filter.update((2.0 * PI * f * t).sin());
            if i >= settle {
                re += y * (2.0 * PI * f * t).sin();
                im += y * (2.0 * PI * f * t).cos();
            }
        }
        Complex::new(re, im) * (2.0 / n as f64)
    }

    #[test]
    fn pt1_matches_the_analytic_first_order_response() {
        let mut f = Pt1::new(100.0, FS);
        // −3 dB at the cutoff (the discrete gain is slightly below the continuous one)
        let g = f.gain_db(100.0);
        assert!((g + 3.0).abs() < 0.3, "{g} dB at cutoff");
        assert!(f.gain_db(1.0).abs() < 0.01);
        assert!((f.gain_db(1000.0) + 20.0).abs() < 1.0, "−20 dB/decade");
        // phase lags and the group delay at low frequency is ≈ RC
        assert!(f.phase(50.0) < 0.0);
        let rc = 1.0 / (2.0 * PI * 100.0);
        let delay = -(f.phase(5.0) - f.phase(4.0)) / (2.0 * PI * 1.0);
        assert!((delay - rc).abs() < 0.1 * rc, "delay {delay} vs RC {rc}");
        // the time-domain simulation agrees with the analytic response
        for freq in [20.0, 100.0, 400.0] {
            let m = measured_response(&mut f, freq);
            let a = f.response(freq);
            assert!((m - a).norm() < 2e-3, "{freq} Hz: {m} vs {a}");
        }
        // step response 1 − (1 − k)^n
        f.reset();
        let k = f.gain();
        for n in 1..=50 {
            let y = f.update(1.0);
            assert!((y - (1.0 - (1.0 - k).powi(n))).abs() < 1e-12);
        }
    }

    #[test]
    fn pt2_and_pt3_are_at_minus_3_db_at_the_cutoff() {
        assert!((pt_cutoff_correction(1) - 1.0).abs() < 1e-12);
        assert!((pt_cutoff_correction(2) - 1.553_773_974).abs() < 1e-8);
        assert!((pt_cutoff_correction(3) - 1.961_459_177).abs() < 1e-8);
        for order in [2u32, 3] {
            let mut f = Ptn::new(order, 100.0, FS);
            assert_eq!(f.order(), order as usize);
            // the discrete PT1 gain error compounds per stage at 8 kHz …
            let g = f.gain_db(100.0);
            assert!((g + 3.0).abs() < 0.6, "PT{order}: {g} dB at cutoff");
            // … and vanishes as the sample rate grows
            let fine = Ptn::new(order, 100.0, 64_000.0).gain_db(100.0);
            assert!((fine + 3.0).abs() < 0.1, "PT{order}: {fine} dB at 64 kHz");
            // each extra stage buys more attenuation a decade above the cutoff
            let pt1 = Pt1::new(100.0, FS).gain_db(1000.0);
            assert!(
                f.gain_db(1000.0) < pt1 - 8.0 * f64::from(order - 1),
                "PT{order}: {} dB vs PT1 {pt1} dB at 1 kHz",
                f.gain_db(1000.0)
            );
            let m = measured_response(&mut f, 100.0);
            let a = f.response(100.0);
            assert!((m - a).norm() < 2e-3, "PT{order}: {m} vs {a}");
        }
    }

    #[test]
    fn biquad_lowpass_is_butterworth() {
        let mut f = Biquad::lowpass(200.0, FS);
        assert!(
            (f.gain_db(200.0) + 3.01).abs() < 0.05,
            "{}",
            f.gain_db(200.0)
        );
        assert!(f.gain_db(10.0).abs() < 1e-3);
        // no peaking anywhere below the cutoff
        for i in 1..200 {
            assert!(f.gain_db(f64::from(i)) <= 1e-6);
        }
        // −40 dB/decade asymptote
        let slope = f.gain_db(2000.0) - f.gain_db(200.0);
        assert!(slope < -35.0, "{slope}");
        let m = measured_response(&mut f, 150.0);
        let a = f.response(150.0);
        assert!((m - a).norm() < 2e-3, "{m} vs {a}");
        let (b0, b1, b2, _, _) = f.coefficients();
        assert!(
            (b0 + b1 + b2 - (1.0 + f.a1 + f.a2)).abs() < 1e-12,
            "DC gain 1"
        );
    }

    #[test]
    fn notch_is_deep_at_the_centre_and_minus_3_db_at_the_cutoff() {
        let center = 250.0;
        let cutoff = 200.0;
        let q = notch_q(center, cutoff);
        assert!((q - 250.0 * 200.0 / (250.0f64.powi(2) - 200.0f64.powi(2))).abs() < 1e-12);
        let mut f = Biquad::notch(center, cutoff, FS);
        assert!(f.gain_db(center) < -80.0, "{}", f.gain_db(center));
        assert!(
            (f.gain_db(cutoff) + 3.0).abs() < 0.2,
            "{}",
            f.gain_db(cutoff)
        );
        // symmetric upper edge at center² / cutoff
        let upper = center * center / cutoff;
        assert!((f.gain_db(upper) + 3.0).abs() < 0.2, "{}", f.gain_db(upper));
        // a wide notch (Q ≈ 2.2) still nibbles a few hundredths of a dB far away
        assert!(f.gain_db(10.0).abs() < 0.05, "{}", f.gain_db(10.0));
        assert!(f.gain_db(3000.0).abs() < 0.05, "{}", f.gain_db(3000.0));
        let m = measured_response(&mut f, 150.0);
        let a = f.response(150.0);
        assert!((m - a).norm() < 2e-3, "{m} vs {a}");
        // the explicit-Q constructor gives the same filter
        assert_eq!(
            Biquad::notch_q(center, q, FS).coefficients(),
            f.coefficients()
        );
    }

    #[test]
    fn chain_multiplies_responses_and_runs_stages_in_order() {
        let mut chain = Chain::new(FS);
        chain
            .push(Stage::Ptn(Ptn::new(2, 150.0, FS)))
            .push(Stage::Biquad(Biquad::notch(300.0, 250.0, FS)))
            .push(Stage::Pt1(Pt1::new(500.0, FS)));
        assert_eq!(chain.stages().len(), 3);
        let expected = Ptn::new(2, 150.0, FS).response(120.0)
            * Biquad::notch(300.0, 250.0, FS).response(120.0)
            * Pt1::new(500.0, FS).response(120.0);
        assert!((chain.response(120.0) - expected).norm() < 1e-12);
        let m = measured_response(&mut chain, 120.0);
        assert!((m - expected).norm() < 2e-3, "{m} vs {expected}");
        // filter_signal starts from a clean state and equals stepping by hand
        let x: Vec<f64> = (0..100).map(|i| f64::from(i % 7)).collect();
        let y = chain.filter_signal(&x);
        chain.reset();
        for (i, v) in x.iter().enumerate() {
            assert!((chain.update(*v) - y[i]).abs() < 1e-12);
        }
        // an empty chain passes through
        assert!((Chain::new(FS).response(123.0) - Complex::new(1.0, 0.0)).norm() < 1e-15);
        assert_eq!(Chain::new(FS).filter_signal(&x), x);
    }

    #[test]
    fn every_filter_is_stable_across_the_parameter_range() {
        // impulse responses decay to nothing for cutoffs anywhere below Nyquist
        let mut seed = 12345u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..200 {
            let fs = 1000.0 + next() * 7000.0;
            let cutoff = 1.0 + next() * (fs / 2.0 - 2.0);
            let center = 2.0 + next() * (fs / 2.0 - 3.0);
            let notch_cut = center * (0.2 + next() * 0.7);
            let mut filters: Vec<Box<dyn Filter>> = vec![
                Box::new(Pt1::new(cutoff, fs)),
                Box::new(Ptn::new(3, cutoff, fs)),
                Box::new(Biquad::lowpass(cutoff, fs)),
                Box::new(Biquad::notch(center, notch_cut, fs)),
            ];
            for f in &mut filters {
                let mut peak = 0.0f64;
                let mut last = 0.0;
                for i in 0..20_000 {
                    last = f.update(if i == 0 { 1.0 } else { 0.0 });
                    peak = peak.max(last.abs());
                }
                assert!(peak.is_finite() && peak <= 1.0 + 1e-9, "peak {peak}");
                assert!(last.abs() < 1e-3, "tail {last} at fs {fs} cutoff {cutoff}");
                // |H| never exceeds 1 for these low-pass/notch sections
                for k in 1..50 {
                    let g = f.response(f64::from(k) * fs / 100.0 / 2.0).norm();
                    assert!(g <= 1.0 + 1e-9, "gain {g}");
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "notch needs")]
    fn notch_rejects_a_cutoff_above_the_centre() {
        let _ = notch_q(100.0, 150.0);
    }

    #[test]
    #[should_panic(expected = "cutoff and fs must be positive")]
    fn pt1_rejects_a_zero_cutoff() {
        let _ = Pt1::new(0.0, FS);
    }
}
