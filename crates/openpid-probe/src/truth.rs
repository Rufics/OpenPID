//! What is true, in closed form, about how an axis follows its stick: the
//! numbers a tool may read off the frequency response of a step response,
//! computed from the plant itself.
//!
//! A plant is the transfer `T` from the rate asked for to the rate flown
//! ([`Plant::transfer`]), its delay and its gain included. Three curves are
//! read from it:
//! - `|T|`, how much of a stick movement of a frequency the craft flies:
//!   the bandwidth is where it has fallen to half the power it has at 0 Hz;
//! - `|1 − T|`, how much of it the craft does not fly (the sensitivity):
//!   its highest value and where it is;
//! - `T / (1 − T)`, the open loop that would close to `T`: where its gain
//!   crosses 1, and how far its phase is from −180° there (the phase
//!   margin).
//!
//! For a pure delay of `d` seconds the answers are exact: `|1 − T|` peaks
//! at 2 (6.02 dB) at `1 / (2·d)`, the open loop crosses 1 at `1 / (6·d)`
//! with a phase margin of 60°, and `|T|` never falls.

use crate::signal::Plant;
use openpid_dsp::fft::Complex;
use serde::Serialize;
use std::f64::consts::{FRAC_1_SQRT_2, TAU};

/// The highest frequency looked at, Hz: half of the lowest log rate of the
/// probes.
const TOP_HZ: f64 = 500.0;
/// Distance between two frequencies of the first pass, Hz.
const STEP_HZ: f64 = 0.01;

impl Plant {
    /// The transfer from the rate asked for to the rate flown at `hz`: the
    /// gain and the phase of the response to a stick moved as a sine.
    pub fn transfer(&self, hz: f64) -> Complex<f64> {
        let w = TAU * hz;
        let j = Complex::new(0.0, 1.0);
        let (shape, delay_ms) = match *self {
            Plant::None => return Complex::new(0.0, 0.0),
            Plant::Delay { delay_ms } => (Complex::new(1.0, 0.0), delay_ms),
            Plant::FirstOrder {
                tau_ms,
                delay_ms,
                gain,
            } => (gain / (1.0 + j * w * tau_ms / 1000.0), delay_ms),
            Plant::SecondOrder {
                natural_hz,
                damping,
                delay_ms,
                gain,
            } => {
                let w0 = TAU * natural_hz;
                let below = w0 * w0 - w * w + 2.0 * damping * w0 * w * j;
                (gain * w0 * w0 / below, delay_ms)
            }
        };
        shape * Complex::from_polar(1.0, -w * delay_ms / 1000.0)
    }
}

/// What is true about a plant in the frequency domain, up to 500 Hz. A
/// number is absent where the plant has none.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResponseTruth {
    /// `|T|` at 0 Hz.
    pub gain: f64,
    /// The lowest frequency at which `|T|` is half the power it is at
    /// 0 Hz (3.01 dB below it), Hz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bandwidth_hz: Option<f64>,
    /// The highest `|1 − T|`, as `20·log10`; absent when `|1 − T|` has no
    /// peak but only rises towards a limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitivity_peak_db: Option<f64>,
    /// The frequency of that peak, Hz: the lowest one, where several are
    /// equally high.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitivity_peak_hz: Option<f64>,
    /// The lowest frequency at which the gain of `T / (1 − T)` falls
    /// through 1, Hz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossover_hz: Option<f64>,
    /// The phase of `T / (1 − T)` at the crossover, above −180°, degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_margin_deg: Option<f64>,
}

/// The frequency between `low` and `high` at which `f` changes its sign,
/// by halving.
fn crossing(f: impl Fn(f64) -> f64, mut low: f64, mut high: f64) -> f64 {
    let rising = f(low) < 0.0;
    for _ in 0..60 {
        let middle = f64::midpoint(low, high);
        if (f(middle) < 0.0) == rising {
            low = middle;
        } else {
            high = middle;
        }
    }
    f64::midpoint(low, high)
}

/// The lowest frequency of the first pass after which `f` is negative
/// where it was not before, made exact by [`crossing`].
fn first_fall(f: impl Fn(f64) -> f64) -> Option<f64> {
    let points = (TOP_HZ / STEP_HZ).round() as usize;
    let at = |k: usize| k as f64 * STEP_HZ;
    // from the first frequency above 0 Hz: a plant of gain 1 has nothing to
    // divide by at 0 Hz
    (1..points)
        .find(|k| f(at(*k)) >= 0.0 && f(at(k + 1)) < 0.0)
        .map(|k| crossing(&f, at(k), at(k + 1)))
}

/// The highest value of `f` below [`TOP_HZ`] and its frequency (the lowest
/// of several that are equally high), or `None` when `f` is highest at the
/// end of the range.
fn peak(f: impl Fn(f64) -> f64) -> Option<(f64, f64)> {
    let points = (TOP_HZ / STEP_HZ).round() as usize;
    let at = |k: usize| k as f64 * STEP_HZ;
    let highest = (0..=points).map(|k| f(at(k))).fold(f64::MIN, f64::max);
    let k = (0..=points).find(|k| f(at(*k)) >= highest - 1.0e-9)?;
    if k == 0 || k == points {
        return None;
    }
    // the top lies where the slope changes its sign; a curve that only
    // flattens towards the end of the range has none
    let slope = |hz: f64| f(hz + 1.0e-4) - f(hz - 1.0e-4);
    if slope(at(k - 1)) <= 0.0 || slope(at(k + 1)) >= 0.0 {
        return None;
    }
    let hz = crossing(slope, at(k - 1), at(k + 1));
    Some((f(hz), hz))
}

impl ResponseTruth {
    /// What is true about `plant`, or `None` for an axis that follows no
    /// stick.
    pub fn of(plant: Plant) -> Option<ResponseTruth> {
        if plant == Plant::None {
            return None;
        }
        let gain = plant.gain();
        let open = |hz: f64| {
            let t = plant.transfer(hz);
            t / (1.0 - t)
        };
        let bandwidth_hz = first_fall(|hz| plant.transfer(hz).norm() - gain * FRAC_1_SQRT_2);
        let sensitivity = peak(|hz| (1.0 - plant.transfer(hz)).norm());
        let crossover_hz = first_fall(|hz| open(hz).norm() - 1.0);
        let phase_margin_deg = crossover_hz.map(|hz| {
            let margin = 180.0 + open(hz).arg().to_degrees();
            // a phase beyond −180° is a negative margin
            if margin > 180.0 {
                margin - 360.0
            } else {
                margin
            }
        });
        let rounded = |v: f64| (v * 1.0e6).round() / 1.0e6;
        Some(ResponseTruth {
            gain,
            bandwidth_hz: bandwidth_hz.map(rounded),
            sensitivity_peak_db: sensitivity.map(|(top, _)| rounded(20.0 * top.log10())),
            sensitivity_peak_hz: sensitivity.map(|(_, hz)| rounded(hz)),
            crossover_hz: crossover_hz.map(rounded),
            phase_margin_deg: phase_margin_deg.map(rounded),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn close(got: Option<f64>, want: f64) -> bool {
        got.is_some_and(|v| (v - want).abs() < 2.0e-5 * want.abs().max(1.0))
    }

    #[test]
    fn a_pure_delay_has_its_closed_forms() {
        for delay_ms in [5.0, 10.0, 20.0] {
            let d = delay_ms / 1000.0;
            let truth = ResponseTruth::of(Plant::Delay { delay_ms }).unwrap();
            assert_eq!(truth.gain, 1.0);
            // |T| is 1 at every frequency
            assert_eq!(truth.bandwidth_hz, None);
            // |1 − T| = 2·|sin(π·f·d)|: 2 at 1 / (2·d), and again at three
            // and five times that; the lowest is taken
            assert!(close(truth.sensitivity_peak_db, 20.0 * 2.0f64.log10()));
            assert!(
                close(truth.sensitivity_peak_hz, 1.0 / (2.0 * d)),
                "{truth:?}"
            );
            // the open loop has gain 1 where the real part of T is one half
            assert!(close(truth.crossover_hz, 1.0 / (6.0 * d)), "{truth:?}");
            assert!(close(truth.phase_margin_deg, 60.0), "{truth:?}");
        }
        assert_eq!(ResponseTruth::of(Plant::None), None);
        assert_eq!(Plant::None.transfer(10.0).norm(), 0.0);
        // a quarter of a turn late at a quarter of the frequency of the delay
        let t = Plant::Delay { delay_ms: 10.0 }.transfer(25.0);
        assert!((t - Complex::new(0.0, -1.0)).norm() < 1.0e-12);
    }

    #[test]
    fn a_first_order_plant_has_its_closed_forms() {
        let plant = |tau_ms: f64, gain: f64| Plant::FirstOrder {
            tau_ms,
            delay_ms: 0.0,
            gain,
        };
        for tau_ms in [10.0, 30.0, 80.0] {
            let corner = 1000.0 / (TAU * tau_ms);
            let truth = ResponseTruth::of(plant(tau_ms, 1.0)).unwrap();
            assert!(close(truth.bandwidth_hz, corner), "{truth:?}");
            // the open loop is 1 / (j·w·tau): gain 1 at the same frequency,
            // a quarter of a turn behind
            assert!(close(truth.crossover_hz, corner), "{truth:?}");
            assert!(close(truth.phase_margin_deg, 90.0), "{truth:?}");
            // |1 − T| rises towards 1 and never turns
            assert_eq!(truth.sensitivity_peak_db, None);
            assert_eq!(truth.sensitivity_peak_hz, None);
        }
        // the bandwidth is counted from the gain at 0 Hz
        let low = ResponseTruth::of(plant(30.0, 0.8)).unwrap();
        assert_eq!(low.gain, 0.8);
        assert!(close(low.bandwidth_hz, 1000.0 / (TAU * 30.0)), "{low:?}");
        // ... the crossover is not: 0.8 / (1 + j·x) has a real part of one
        // half at x² = 0.6
        let x = 0.6f64.sqrt();
        assert!(
            close(low.crossover_hz, x * 1000.0 / (TAU * 30.0)),
            "{low:?}"
        );
        assert!(close(
            low.phase_margin_deg,
            180.0 - 2.0 * x.atan().to_degrees()
        ));
    }

    #[test]
    fn a_second_order_plant_has_its_closed_forms() {
        let plant = |damping: f64, delay_ms: f64| Plant::SecondOrder {
            natural_hz: 20.0,
            damping,
            delay_ms,
            gain: 1.0,
        };
        // without a delay: the bandwidth of the textbook,
        // f0·sqrt(1 − 2z² + sqrt(4z⁴ − 4z² + 2)); the open loop is
        // w0² / (s·(s + 2z·w0)), whose crossover and margin are closed too
        for z in [0.3f64, 0.5, 0.9] {
            let truth = ResponseTruth::of(plant(z, 0.0)).unwrap();
            let z2 = z * z;
            let bandwidth =
                20.0 * (1.0 - 2.0 * z2 + (4.0 * z2 * z2 - 4.0 * z2 + 2.0).sqrt()).sqrt();
            assert!(close(truth.bandwidth_hz, bandwidth), "{z}: {truth:?}");
            let ratio = ((4.0 * z2 * z2 + 1.0).sqrt() - 2.0 * z2).sqrt();
            assert!(close(truth.crossover_hz, 20.0 * ratio), "{z}: {truth:?}");
            let margin = (2.0 * z / ratio).atan().to_degrees();
            assert!(close(truth.phase_margin_deg, margin), "{z}: {truth:?}");
            // 1 − T = s·(s + 2z·w0) / (s² + 2z·w0·s + w0²) overshoots 1
            assert!(truth.sensitivity_peak_db.is_some_and(|db| db > 0.0));
        }
        // a delay takes its phase from the margin and leaves |T| alone
        let (clean, late) = (plant(0.5, 0.0), plant(0.5, 4.0));
        let (clean, late) = (
            ResponseTruth::of(clean).unwrap(),
            ResponseTruth::of(late).unwrap(),
        );
        assert_eq!(clean.bandwidth_hz, late.bandwidth_hz);
        assert!(late.phase_margin_deg < clean.phase_margin_deg);
        assert!(late.sensitivity_peak_db > clean.sensitivity_peak_db);
        // at its natural frequency the plant is a quarter of a turn late
        // and 1 / (2z) high
        let t = plant(0.5, 0.0).transfer(20.0);
        assert!((t - Complex::new(0.0, -1.0)).norm() < 1.0e-12, "{t}");
        assert!((PI / 2.0 + t.arg()).abs() < 1.0e-12);
    }
}
