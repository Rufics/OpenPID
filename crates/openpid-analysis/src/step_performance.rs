//! Performance numbers of a step response: how fast the loop is and how
//! much stability margin it has, read from the mean response alone.
//!
//! Version 0.94 of the reference tool shows these six numbers beside its
//! step response. They are computed here the plain, documented way
//! (decision D-004, option A; `docs/DESIGN.md` §6.2), from the curve `s`
//! sampled at `fs`:
//!
//! 1. the rise time is the time the curve takes from 0.1 to 0.9 **of 1**,
//!    each level read on the straight line between the two samples around
//!    its first crossing;
//! 2. the impulse response is `h[k] = (s[k+1] − s[k]) / s[n−1]`: the
//!    differences of the curve, brought to a gain of 1 at 0 Hz as far as
//!    the curve has settled at its end;
//! 3. `H` is the transform of `h` followed by zeros up to `N`, the smallest
//!    power of two that is at least `fs` (and at least the length of `h`):
//!    the closed loop on the grid `f[k] = k·fs/N`, about 1 Hz apart;
//! 4. the bandwidth is where `20·log10|H|` first comes down through
//!    −3.0103 dB, on the straight line between the two grid points;
//! 5. the sensitivity peak is the highest `20·log10|1 − H|` at the grid
//!    points below 150 Hz ([`SENSITIVITY_BELOW_HZ`]), and the sensitivity
//!    frequency is that grid point;
//! 6. the open loop is `L = H / (1 − H)`; the crossover frequency is where
//!    `20·log10|L|` first comes down through 0 dB, on the straight line
//!    between the two grid points, and the phase margin is 180° plus the
//!    unwrapped phase of `L` read at the same place on the same line,
//!    brought into −180° … 180° by whole turns.
//!
//! The differences of step 2 stand at the sample they start from, half a
//! sample before the middle of the stretch they cover, so `H` leads the
//! plant by `180°·f/fs` (1.4° at 16 Hz and 2 kHz) and a step first seen at
//! sample `i` is read as a delay of `(i − 1)/fs`. A curve of half a second
//! tells nothing slower than about 2 Hz.

use openpid_dsp::{Complex, Fft};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// 1 as a complex number: what the closed loop is taken from.
const ONE: Complex<f64> = Complex::new(1.0, 0.0);

/// The level the closed loop has to fall to for the bandwidth, dB.
pub const BANDWIDTH_DB: f64 = -3.0103;
/// The sensitivity peak is looked for at the grid points below this, Hz.
pub const SENSITIVITY_BELOW_HZ: f64 = 150.0;
/// The levels of 1 between which the rise time is read.
pub const RISE_LEVELS: (f64, f64) = (0.1, 0.9);
/// A curve that ends closer to zero than this has no gain to bring to 1.
const TINY: f64 = 1.0e-9;
/// The fewest samples of a curve the frequency numbers are read from.
const MIN_SAMPLES: usize = 4;
/// No transform is taken for a rate above this, Hz: no log is that fast,
/// and the grid would be as long as the rate.
const MAX_FS: f64 = 1.0e6;

/// The performance numbers of a mean step response ([`performance`]); a
/// number the curve does not give is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StepPerformance {
    /// Time from 0.1 to 0.9 **of 1**, ms, each crossing read between the
    /// samples. [`crate::StepMetrics::rise_time_ms`] is another reading of
    /// the same curve: from 10 % to 90 % of where the curve settles, in
    /// whole samples; the two agree when the curve settles at 1.
    pub rise_ms: Option<f64>,
    /// Where the closed loop falls to −3 dB, Hz.
    pub bandwidth_hz: Option<f64>,
    /// The most the loop amplifies a disturbance: the highest `|1 − H|`
    /// below 150 Hz, dB.
    pub sensitivity_db: Option<f64>,
    /// The frequency of that peak, Hz.
    pub sensitivity_hz: Option<f64>,
    /// How far the phase of the open loop is from −180° where its gain is
    /// 0 dB, degrees.
    pub phase_margin_deg: Option<f64>,
    /// Where the gain of the open loop comes down through 0 dB, Hz.
    pub crossover_hz: Option<f64>,
}

impl StepPerformance {
    /// The same numbers without those that are not finite (a closed loop of
    /// exactly 1 has no open loop), and without half a pair: a peak without
    /// its frequency, a margin without its crossover.
    fn finite(mut self) -> Self {
        for number in [
            &mut self.rise_ms,
            &mut self.bandwidth_hz,
            &mut self.sensitivity_db,
            &mut self.sensitivity_hz,
            &mut self.phase_margin_deg,
            &mut self.crossover_hz,
        ] {
            *number = number.filter(|v| v.is_finite());
        }
        if self.sensitivity_db.is_none() {
            self.sensitivity_hz = None;
        }
        if self.phase_margin_deg.is_none() || self.crossover_hz.is_none() {
            (self.phase_margin_deg, self.crossover_hz) = (None, None);
        }
        self
    }
}

/// The time at which `curve` first comes up to `level`, in samples: on the
/// straight line between the sample below it and the one at or above it. A
/// curve that starts at or above the level is there at 0.
fn first_reach(curve: &[f64], level: f64) -> Option<f64> {
    if *curve.first()? >= level {
        return Some(0.0);
    }
    curve
        .windows(2)
        .position(|w| w[0] < level && w[1] >= level)
        .map(|i| i as f64 + (level - curve[i]) / (curve[i + 1] - curve[i]))
}

/// Step 1: the time from 0.1 to 0.9 of 1, ms.
fn rise_ms(curve: &[f64], fs: f64) -> Option<f64> {
    let from = first_reach(curve, RISE_LEVELS.0)?;
    let to = first_reach(curve, RISE_LEVELS.1)?;
    (to > from).then(|| (to - from) * 1000.0 / fs)
}

/// Steps 2 and 3: the closed loop of `curve` at `k·fs/N`, `k = 0 … N/2`,
/// with the step of that grid, Hz. `None` for a curve too short or one
/// that ends at zero.
fn closed_loop(curve: &[f64], fs: f64) -> Option<(Vec<Complex<f64>>, f64)> {
    let last = *curve.last()?;
    if curve.len() < MIN_SAMPLES || last.abs() < TINY || fs > MAX_FS {
        return None;
    }
    let mut impulse: Vec<f64> = curve.windows(2).map(|w| (w[1] - w[0]) / last).collect();
    let mut points = 1usize;
    while (points as f64) < fs || points < impulse.len() {
        points *= 2;
    }
    impulse.resize(points, 0.0);
    Some((Fft::new().rfft(&impulse), fs / points as f64))
}

/// Where `level_db` first comes down through `at` among the points from
/// `first` on: the index of the point at or below it and how far towards
/// that point, from the one before, the straight line between the two is
/// at `at` (0 … 1).
fn first_fall(level_db: &[f64], first: usize, at: f64) -> Option<(usize, f64)> {
    (first + 1..level_db.len())
        .find(|&k| level_db[k - 1] > at && at >= level_db[k])
        .map(|k| (k, (at - level_db[k - 1]) / (level_db[k] - level_db[k - 1])))
}

/// `20·log10` of a magnitude.
fn db(magnitude: f64) -> f64 {
    20.0 * magnitude.log10()
}

/// The phases of `values`, each a whole number of turns from its own so
/// that it is no more than half a turn from the one before.
fn unwrapped_phase(values: &[Complex<f64>]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(values.len());
    for (i, value) in values.iter().enumerate() {
        let phase = value.arg();
        let Some(before) = i.checked_sub(1) else {
            out.push(phase);
            continue;
        };
        let step = phase - values[before].arg();
        let mut within = (step + PI).rem_euclid(TAU) - PI;
        if within == -PI && step > 0.0 {
            within = PI;
        }
        out.push(out[before] + within);
    }
    out
}

/// `degrees` less the whole turns that bring it into −180° … 180° (180°
/// itself included). The phase of the open loop is unwrapped from its first
/// grid point, and a curve that wanders by a few hundredths puts that point
/// on the other side of ±180°: the margin then comes out a turn too high.
fn within_half_a_turn(degrees: f64) -> f64 {
    degrees - 360.0 * ((degrees - 180.0) / 360.0).ceil()
}

/// The performance numbers of the mean step response `mean`, sampled at
/// `fs`: the six of the module's description. The rise time needs only the
/// two levels to be reached; the other five need a curve of four samples
/// or more that does not end at zero.
pub fn performance(mean: &[f64], fs: f64) -> StepPerformance {
    let mut out = StepPerformance::default();
    if !(fs.is_finite() && fs > 0.0) || mean.iter().any(|v| !v.is_finite()) {
        return out;
    }
    out.rise_ms = rise_ms(mean, fs);
    let Some((closed, grid_hz)) = closed_loop(mean, fs) else {
        return out;
    };
    let hz = |k: usize, towards: f64| grid_hz * (k as f64 - 1.0 + towards);
    // step 4
    let closed_db: Vec<f64> = closed.iter().map(|h| db(h.norm())).collect();
    out.bandwidth_hz = first_fall(&closed_db, 0, BANDWIDTH_DB).map(|(k, w)| hz(k, w));
    // step 5
    let below = (1..closed.len()).take_while(|&k| (k as f64) * grid_hz < SENSITIVITY_BELOW_HZ);
    let peak = below.map(|k| (k, db((ONE - closed[k]).norm()))).fold(
        None,
        |best: Option<(usize, f64)>, (k, level)| match best {
            Some((_, highest)) if level <= highest || level.is_nan() => best,
            _ => Some((k, level)),
        },
    );
    if let Some((k, level)) = peak {
        out.sensitivity_db = Some(level);
        out.sensitivity_hz = Some(k as f64 * grid_hz);
    }
    // step 6: the open loop has no point at 0 Hz, where the closed loop is 1
    let open: Vec<Complex<f64>> = closed.iter().map(|h| h / (ONE - h)).collect();
    let open_db: Vec<f64> = open.iter().map(|l| db(l.norm())).collect();
    if let Some((k, w)) = first_fall(&open_db, 1, 0.0) {
        let phase = unwrapped_phase(&open[1..=k]);
        let (before, at) = (phase[k - 2], phase[k - 1]);
        out.crossover_hz = Some(hz(k, w));
        let phase = (before + w * (at - before)).to_degrees();
        out.phase_margin_deg = Some(within_half_a_turn(180.0 + phase));
    }
    out.finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample rate of the test curves, Hz.
    const FS: f64 = 2000.0;
    /// Half a second of it, as the step response has.
    const SAMPLES: usize = 1000;

    /// The step response of `ωn² / (s² + 2ζωn·s + ωn²)` times `gain`:
    /// `1 − e^(−ζωn·t)·(cos ωd·t + ζ/√(1−ζ²)·sin ωd·t)`, `ωd = ωn·√(1−ζ²)`.
    fn second_order(natural_hz: f64, damping: f64, gain: f64, fs: f64, samples: usize) -> Vec<f64> {
        let natural = TAU * natural_hz;
        let damped = natural * (1.0 - damping * damping).sqrt();
        (0..samples)
            .map(|i| {
                let t = i as f64 / fs;
                let swing = (damped * t).cos()
                    + damping / (1.0 - damping * damping).sqrt() * (damped * t).sin();
                gain * (1.0 - (-damping * natural * t).exp() * swing)
            })
            .collect()
    }

    /// The step response of `1 / (1 + s·τ)`: `1 − e^(−t/τ)`.
    fn lag(tau_s: f64) -> Vec<f64> {
        (0..SAMPLES)
            .map(|i| 1.0 - (-(i as f64) / FS / tau_s).exp())
            .collect()
    }

    fn close(got: Option<f64>, expected: f64, tolerance: f64, what: &str) {
        let got = got.unwrap_or_else(|| panic!("{what}: no number"));
        assert!(
            (got - expected).abs() <= tolerance,
            "{what}: {got} for {expected} ± {tolerance}"
        );
    }

    #[test]
    fn a_pure_delay_has_twice_the_disturbance_and_sixty_degrees() {
        // 0 before 10 ms, 1 from 10 ms on: the curve is 1 from sample 20.
        // Its only difference stands at sample 19, so the delay that is
        // read is d = 10 ms − 1/fs = 9.5 ms: H = e^(−jωd), |H| = 1.
        let tau = 0.010;
        let curve: Vec<f64> = (0..SAMPLES)
            .map(|i| if i as f64 / FS >= tau { 1.0 } else { 0.0 })
            .collect();
        let read = tau - 1.0 / FS;
        let p = performance(&curve, FS);
        // |1 − e^(−jωd)| = 2·|sin(ωd/2)|: 2 (6.02 dB) at f = 1/(2d), read at
        // the grid point next to it (the grid is fs/2048 apart)
        close(p.sensitivity_db, 20.0 * 2f64.log10(), 1e-3, "peak");
        close(
            p.sensitivity_hz,
            1.0 / (2.0 * read),
            FS / 2048.0 / 2.0,
            "peak at",
        );
        // |L| = 1 / (2·sin(ωd/2)) is 1 where ωd = π/3: f = 1/(6d); there
        // the phase of L is −(π/2 + ωd/2) = −120°, a margin of 60°
        close(p.crossover_hz, 1.0 / (6.0 * read), 0.01, "crossover");
        close(p.phase_margin_deg, 60.0, 0.01, "margin");
        // the closed loop never falls: no bandwidth
        assert_eq!(p.bandwidth_hz, None);
        // 0.1 and 0.9 are crossed on the one line from 0 to 1: 0.8 of a sample
        close(p.rise_ms, 0.8 * 1000.0 / FS, 1e-9, "rise");
    }

    #[test]
    fn a_first_order_lag_reads_its_corner_and_ninety_degrees() {
        let tau = 0.010;
        let p = performance(&lag(tau), FS);
        let corner = 1.0 / (TAU * tau);
        // |1 / (1 + jωτ)| is 1/√2 at ω = 1/τ
        close(p.bandwidth_hz, corner, 0.01, "bandwidth");
        // 1 − e^(−t/τ) is 0.1 at τ·ln(10/9) and 0.9 at τ·ln 10: τ·ln 9 apart
        close(p.rise_ms, tau * 9f64.ln() * 1000.0, 0.002, "rise");
        // |1 − H| = ωτ / √(1 + ω²τ²) grows towards 1 and never reaches it:
        // the peak is the last grid point below 150 Hz, a little under 0 dB
        let peak = p.sensitivity_db.expect("a peak");
        assert!((-0.5..0.0).contains(&peak), "peak {peak}");
        let at = p.sensitivity_hz.expect("its frequency");
        let grid = FS / 2048.0;
        assert!(
            at < SENSITIVITY_BELOW_HZ && at + grid >= SENSITIVITY_BELOW_HZ,
            "peak at {at}"
        );
        // L = 1/(sτ): 0 dB at 1/(2πτ), −90° everywhere. What is read is the
        // sampled lag, whose differences stand half a sample early:
        // |L| = (e^(1/(fs·τ)) − 1) / (2·sin(πf/fs)), 0 dB 2.6 % higher
        // (16.33 Hz for 15.92 Hz), and a lead of 180°·f/fs = 1.47°
        let sampled = FS / PI * (((1.0 / (FS * tau)).exp() - 1.0) / 2.0).asin();
        close(p.crossover_hz, sampled, 0.01, "crossover as sampled");
        close(p.crossover_hz, corner, 0.45, "crossover");
        close(
            p.phase_margin_deg,
            90.0 + 180.0 * sampled / FS,
            0.01,
            "margin as sampled",
        );
        close(p.phase_margin_deg, 90.0, 1.5, "margin");
    }

    #[test]
    fn a_second_order_plant_reads_its_textbook_numbers() {
        // ωn = 2π·20 Hz, ζ = 0.5
        let (natural_hz, damping) = (20.0, 0.5f64);
        let p = performance(&second_order(natural_hz, damping, 1.0, FS, SAMPLES), FS);
        let z2 = damping * damping;
        // |T| = 1/√2: ωb = ωn·√(1 − 2ζ² + √(4ζ⁴ − 4ζ² + 2)), 25.44 Hz
        let bandwidth =
            natural_hz * (1.0 - 2.0 * z2 + (4.0 * z2 * z2 - 4.0 * z2 + 2.0).sqrt()).sqrt();
        close(p.bandwidth_hz, bandwidth, 0.02, "bandwidth");
        // L = ωn² / (s·(s + 2ζωn)): |L| = 1 at ωc = ωn·√(√(1 + 4ζ⁴) − 2ζ²),
        // 15.72 Hz, with a margin of atan(2ζωn/ωc), 51.83°. Read from the
        // samples the crossover is 0.23 Hz higher and the margin 0.17° more
        let crossover = natural_hz * ((1.0 + 4.0 * z2 * z2).sqrt() - 2.0 * z2).sqrt();
        let margin = (2.0 * damping * natural_hz / crossover).atan().to_degrees();
        close(p.crossover_hz, crossover, 0.3, "crossover");
        close(p.phase_margin_deg, margin, 0.5, "margin");
        // 1 − T = s·(s + 2ζωn) / (s² + 2ζωn·s + ωn²). With x = (ω/ωn)² and
        // ζ = 0.5, |1 − T|² = (x² + x) / (x² − x + 1), highest at
        // x = (1 + √3)/2: 3.33 dB at 1.169·ωn, 23.38 Hz. The samples read
        // 3.22 dB, at the grid point next to it
        let x = f64::midpoint(1.0, 3f64.sqrt());
        let peak = 10.0 * ((x * x + x) / (x * x - x + 1.0)).log10();
        close(p.sensitivity_db, peak, 0.15, "peak");
        close(
            p.sensitivity_hz,
            natural_hz * x.sqrt(),
            FS / 2048.0 / 2.0,
            "peak at",
        );
        // the curve first reaches 1 a quarter of the way into its first
        // swing and overshoots by e^(−πζ/√(1−ζ²)) = 16 %: the rise ends
        // before the time to the peak, π/ωd = 28.9 ms
        let rise = p.rise_ms.expect("a rise time");
        assert!((5.0..28.9).contains(&rise), "rise {rise}");
    }

    #[test]
    fn a_curve_longer_than_a_second_keeps_all_of_its_samples() {
        // ωn = 2π·3 Hz, ζ = 0.6 at 1 kHz: 1.5 s, so the transform is of
        // 2048 points and the grid 0.49 Hz apart
        let (natural_hz, z2) = (3.0, 0.36f64);
        let p = performance(&second_order(natural_hz, 0.6, 1.0, 1000.0, 1500), 1000.0);
        let bandwidth =
            natural_hz * (1.0 - 2.0 * z2 + (4.0 * z2 * z2 - 4.0 * z2 + 2.0).sqrt()).sqrt();
        close(p.bandwidth_hz, bandwidth, 0.01, "bandwidth");
        let crossover = natural_hz * ((1.0 + 4.0 * z2 * z2).sqrt() - 2.0 * z2).sqrt();
        close(p.crossover_hz, crossover, 0.02, "crossover");
        close(
            p.phase_margin_deg,
            (2.0 * 0.6 * natural_hz / crossover).atan().to_degrees(),
            0.5,
            "margin",
        );
    }

    #[test]
    fn the_gain_of_the_curve_moves_the_rise_time_only() {
        let unit = second_order(20.0, 0.5, 1.0, FS, SAMPLES);
        let p = performance(&unit, FS);
        for gain in [0.9, 1.1] {
            let scaled: Vec<f64> = unit.iter().map(|v| v * gain).collect();
            let q = performance(&scaled, FS);
            // the differences are divided by the end of the curve
            for (what, a, b) in [
                ("bandwidth", p.bandwidth_hz, q.bandwidth_hz),
                ("peak", p.sensitivity_db, q.sensitivity_db),
                ("peak at", p.sensitivity_hz, q.sensitivity_hz),
                ("margin", p.phase_margin_deg, q.phase_margin_deg),
                ("crossover", p.crossover_hz, q.crossover_hz),
            ] {
                close(b, a.expect("a number"), 1e-9, what);
            }
        }
        // the rise time is against 1: a lower curve takes longer to 0.9
        let rise = |gain: f64| {
            let scaled: Vec<f64> = unit.iter().map(|v| v * gain).collect();
            performance(&scaled, FS).rise_ms.expect("a rise time")
        };
        assert!(rise(0.9) > rise(1.0) && rise(1.0) > rise(1.1));
        // and a lag that settles below 0.9 never gets there
        let low: Vec<f64> = lag(0.010).iter().map(|v| v * 0.85).collect();
        let q = performance(&low, FS);
        assert_eq!(q.rise_ms, None);
        close(q.bandwidth_hz, 1.0 / (TAU * 0.010), 0.01, "bandwidth");
    }

    #[test]
    fn a_wandering_curve_keeps_its_margin_within_half_a_turn() {
        // the plant of the textbook test with a slow wave of 0.05 on it, as
        // a mean of few windows has: the first grid point of the open loop
        // is then past ±180°, and the unwrapped phase a turn off (411.46°
        // for 51.46°)
        let curve: Vec<f64> = second_order(20.0, 0.5, 1.0, FS, SAMPLES)
            .iter()
            .enumerate()
            .map(|(i, v)| v + 0.05 * (TAU * 3.0 * i as f64 / FS).sin())
            .collect();
        let p = performance(&curve, FS);
        close(p.phase_margin_deg, 51.83, 0.5, "margin");
        close(p.crossover_hz, 15.72, 0.3, "crossover");
        for (degrees, within) in [
            (411.46, 51.46),
            (-190.0, 170.0),
            (180.0, 180.0),
            (-180.0, 180.0),
            (60.0, 60.0),
        ] {
            assert!((within_half_a_turn(degrees) - within).abs() < 1e-9);
        }
    }

    #[test]
    fn curves_that_say_nothing_give_no_numbers() {
        let none = StepPerformance::default();
        assert_eq!(performance(&[], FS), none);
        assert_eq!(performance(&[0.0; 1000], FS), none);
        assert_eq!(performance(&[f64::NAN; 1000], FS), none);
        assert_eq!(performance(&[0.0, 0.5, 1.0, f64::INFINITY], FS), none);
        for fs in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(performance(&lag(0.010), fs), none);
        }
        // too short for a transform: the rise time alone
        let short = performance(&[0.0, 0.5, 1.0], FS);
        close(short.rise_ms, 1.6 * 1000.0 / FS, 1e-9, "rise");
        assert_eq!(
            StepPerformance {
                rise_ms: None,
                ..short
            },
            none
        );
        // a curve that ends at zero has no gain to bring to 1
        let mut back = lag(0.010);
        back[SAMPLES - 1] = 0.0;
        let p = performance(&back, FS);
        assert!(p.rise_ms.is_some());
        assert_eq!(StepPerformance { rise_ms: None, ..p }, none);
        // a closed loop of exactly 1 has no open loop, and no bandwidth
        let p = performance(&[0.0, 1.0, 1.0, 1.0, 1.0], FS);
        assert_eq!(
            (p.bandwidth_hz, p.phase_margin_deg, p.crossover_hz),
            (None, None, None)
        );
        assert_eq!((p.sensitivity_db, p.sensitivity_hz), (None, None));
        // a curve that starts above a level is there at once; one that
        // starts above both has no rise
        assert_eq!(first_reach(&[0.2, 0.5, 1.0], 0.1), Some(0.0));
        assert_eq!(performance(&[0.95, 1.0, 1.0, 1.0], FS).rise_ms, None);
        // a rate no log has: no transform as long as the rate
        assert_eq!(performance(&lag(0.010), 2.0e6).bandwidth_hz, None);
    }

    #[test]
    fn numbers_a_file_does_not_have_are_none() {
        let p: StepPerformance = serde_json::from_str(r#"{"rise_ms": 12.5}"#).expect("json");
        assert_eq!(
            p,
            StepPerformance {
                rise_ms: Some(12.5),
                ..StepPerformance::default()
            }
        );
        let text = serde_json::to_string(&performance(&lag(0.010), FS)).expect("json");
        for key in [
            "rise_ms",
            "bandwidth_hz",
            "sensitivity_db",
            "sensitivity_hz",
            "phase_margin_deg",
            "crossover_hz",
        ] {
            assert!(text.contains(key), "{key} in {text}");
        }
    }
}
