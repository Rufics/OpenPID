//! Typed flight-controller settings extracted from log headers.
//!
//! Every field is optional: a firmware profile fills what it recognises and
//! leaves the rest in [`Settings::extra`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
/// PID gains of one axis in firmware units.
pub struct PidGains {
    /// Proportional gain.
    pub p: f64,
    /// Integral gain.
    pub i: f64,
    /// Derivative gain (D-max on Betaflight 4.x, D on 2025.12+).
    pub d: f64,
    /// Feed-forward gain.
    pub f: Option<f64>,
    /// D-min (Betaflight 4.x).
    pub d_min: Option<f64>,
    /// D-max (Betaflight 2025.12+).
    pub d_max: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
/// A value per rotation axis.
pub struct PerAxis<T> {
    /// Roll.
    pub roll: T,
    /// Pitch.
    pub pitch: T,
    /// Yaw.
    pub yaw: T,
}

impl<T> PerAxis<T> {
    /// Value for `axis`.
    pub fn get(&self, axis: crate::Axis) -> &T {
        match axis {
            crate::Axis::Roll => &self.roll,
            crate::Axis::Pitch => &self.pitch,
            crate::Axis::Yaw => &self.yaw,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
/// Loop and logging rates in Hz.
pub struct LoopRates {
    /// Gyro sampling rate.
    pub gyro_hz: Option<f64>,
    /// PID loop rate.
    pub pid_hz: Option<f64>,
    /// Rate at which main frames were logged.
    pub log_hz: Option<f64>,
    /// Which of the two loops the gyro's filters run in, as the firmware
    /// profile knows it of this firmware and version.
    #[serde(default)]
    pub gyro_filter_layout: GyroFilterLayout,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// How a firmware spreads the filters of its gyro over the gyro loop and the
/// PID loop. A firmware profile picks one (ADR-0015);
/// [`Settings::gyro_filter_rate`] turns it into the rate of a stage.
pub enum GyroFilterLayout {
    /// Every gyro filter runs at the gyro rate. This is also what is taken
    /// for a firmware whose arrangement is not known: an assumption, not a
    /// fact of that firmware.
    #[default]
    AllAtGyroRate,
    /// The second low-pass runs at the gyro rate, as the anti-aliasing
    /// filter before the gyro is brought down to the PID loop; every other
    /// gyro filter (RPM notches, static notches, the first low-pass, dynamic
    /// notches) runs at the PID loop rate. Betaflight 4.2 and later, by its
    /// 4.2 tuning notes.
    PidLoopAfterLowpass2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// A stage of the gyro filter chain, in the order the signal passes them.
pub enum FilterStage {
    /// The RPM notches (every motor and harmonic).
    RpmNotch,
    /// The static notches.
    StaticNotch,
    /// The first low-pass.
    Lowpass1,
    /// The second low-pass.
    Lowpass2,
    /// The dynamic notches.
    DynNotch,
}

impl FilterStage {
    /// Every stage, in signal order.
    pub const ALL: [FilterStage; 5] = [
        FilterStage::RpmNotch,
        FilterStage::StaticNotch,
        FilterStage::Lowpass1,
        FilterStage::Lowpass2,
        FilterStage::DynNotch,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// One of the two loops of a flight controller.
pub enum ControlLoop {
    /// The gyro loop (the rate the gyro is sampled at).
    Gyro,
    /// The PID loop.
    Pid,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Motor and ESC configuration.
pub struct MotorConfig {
    /// Magnetic pole count (needed to turn eRPM into RPM).
    pub poles: Option<u32>,
    /// ESC protocol name.
    pub protocol: Option<String>,
    /// Lowest motor output value.
    pub output_low: Option<f64>,
    /// Highest motor output value.
    pub output_high: Option<f64>,
    /// Motor KV rating.
    pub kv: Option<f64>,
    /// Number of motors.
    pub count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Low-pass filter topology.
pub enum LowpassKind {
    /// First-order (PT1).
    Pt1,
    /// Second-order biquad.
    Biquad,
    /// Two cascaded PT1 (PT2).
    Pt2,
    /// Three cascaded PT1 (PT3).
    Pt3,
    /// Firmware-specific type code.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// One low-pass stage.
pub struct Lowpass {
    /// Topology, when known.
    pub kind: Option<LowpassKind>,
    /// Static cut-off in Hz (0 when only dynamic).
    pub cutoff_hz: f64,
    /// Dynamic cutoff range, if the firmware moves the cutoff with throttle.
    pub dyn_min_hz: Option<f64>,
    /// Dynamic cut-off at full throttle.
    pub dyn_max_hz: Option<f64>,
    /// Curve of the dynamic cut-off over the throttle: 0 = linear, up to 1
    /// (the firmware's expo setting scaled to 0…1).
    #[serde(default)]
    pub dyn_expo: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// Static notch.
pub struct Notch {
    /// Centre frequency.
    pub center_hz: f64,
    /// Cut-off (bandwidth edge) frequency.
    pub cutoff_hz: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// Dynamic (FFT-tracked) notch configuration.
pub struct DynNotch {
    /// Lowest tracked frequency.
    pub min_hz: f64,
    /// Highest tracked frequency.
    pub max_hz: f64,
    /// Notch quality factor.
    pub q: f64,
    /// Number of notches per axis.
    pub count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// RPM (ESC telemetry) notch configuration.
pub struct RpmNotch {
    /// Harmonics filtered per motor.
    pub harmonics: u32,
    /// Notch quality factor.
    pub q: f64,
    /// Lowest filtered frequency.
    pub min_hz: f64,
    /// Width of the fade-in above `min_hz`, Hz (0 = switched hard at `min_hz`).
    #[serde(default)]
    pub fade_range_hz: f64,
    /// Weight of each harmonic, 0…1; a harmonic without one counts as 1.
    #[serde(default)]
    pub weights: Vec<f64>,
    /// Cut-off of the low-pass that smooths the motor frequency, Hz
    /// (0 = not given).
    #[serde(default)]
    pub lpf_hz: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// A complete filter chain as configured in the firmware.
pub struct FilterChain {
    /// First low-pass.
    pub lowpass1: Option<Lowpass>,
    /// Second low-pass.
    pub lowpass2: Option<Lowpass>,
    /// Static notches.
    pub notches: Vec<Notch>,
    /// Dynamic notch.
    pub dyn_notch: Option<DynNotch>,
    /// RPM notch.
    pub rpm_notch: Option<RpmNotch>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Firmware debug mode selection.
pub struct DebugMode {
    /// Numeric mode code.
    pub code: u32,
    /// Mode name, when the firmware logs it.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A setting in words: what it is and its value, as its firmware's profile
/// reads it ("TPA", "65 % above 1350, on D").
pub struct Detail {
    /// The setting, in the firmware's own terms.
    pub label: String,
    /// Its value, with units.
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Typed flight-controller settings; everything optional.
pub struct Settings {
    /// PID gains per axis.
    pub pid: Option<PerAxis<PidGains>>,
    /// The smallest step of a PID gain on this firmware, which is also the
    /// precision the gains are written with: 1 for the integer gains of the
    /// Cleanflight family, 0.0001 for `ArduPilot`'s. `None` reads as 1.
    #[serde(default)]
    pub pid_step: Option<f64>,
    /// Loop rates.
    pub loop_rates: LoopRates,
    /// Motor configuration.
    pub motor: MotorConfig,
    /// Gyro filter chain.
    pub gyro_filters: FilterChain,
    /// D-term filter chain.
    pub dterm_filters: FilterChain,
    /// Debug mode.
    pub debug_mode: Option<DebugMode>,
    /// Craft name from the headers.
    pub craft_name: Option<String>,
    /// The board the firmware was built for, as the log names it
    /// ("HBRO KAKUTEF7").
    #[serde(default)]
    pub board: Option<String>,
    /// When the log was started, as the firmware wrote it; `None` when the
    /// flight controller had no clock.
    #[serde(default)]
    pub log_start: Option<String>,
    /// Further settings the profile read and put in words, in the order
    /// to show them: what a firmware has and the typed model has no field
    /// for (rates, TPA, feedforward tuning, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Detail>,
    /// Headers the profile did not interpret, verbatim.
    pub extra: BTreeMap<String, String>,
}
impl Settings {
    /// The loop a stage of the gyro filter chain runs in on this craft, and
    /// the rate of that loop when the headers give it.
    ///
    /// This is the one place that says which stage runs at which rate:
    ///
    /// | Layout | Low-pass 2 | Every other stage |
    /// |---|---|---|
    /// | [`GyroFilterLayout::PidLoopAfterLowpass2`] (Betaflight 4.2 and later) | gyro rate | PID loop rate |
    /// | [`GyroFilterLayout::AllAtGyroRate`] (Betaflight before 4.2; every other firmware, where it is taken to be so, not known) | gyro rate | gyro rate |
    ///
    /// The rate is `None` when the headers do not give that loop's rate;
    /// whether a rate can be used (it cannot be below the rate of the log) is
    /// for the caller to judge.
    pub fn gyro_filter_rate(&self, stage: FilterStage) -> (ControlLoop, Option<f64>) {
        let rates = &self.loop_rates;
        match (rates.gyro_filter_layout, stage) {
            (GyroFilterLayout::AllAtGyroRate, _) | (_, FilterStage::Lowpass2) => {
                (ControlLoop::Gyro, rates.gyro_hz)
            }
            (GyroFilterLayout::PidLoopAfterLowpass2, _) => (ControlLoop::Pid, rates.pid_hz),
        }
    }

    /// The smallest step of a PID gain (1 when the profile gave none).
    pub fn gain_step(&self) -> f64 {
        self.pid_step
            .filter(|s| *s > 0.0 && s.is_finite())
            .unwrap_or(1.0)
    }

    /// `gain` rounded to the firmware's step.
    pub fn round_gain(&self, gain: f64) -> f64 {
        let step = self.gain_step();
        // a decimal step (0.0001) is rounded through its whole inverse, so
        // the result is the nearest number to the decimal, not 0.1546000…2
        let inverse = (1.0 / step).round();
        if step < 1.0 && (inverse * step - 1.0).abs() < 1e-9 {
            (gain * inverse).round() / inverse
        } else {
            (gain / step).round() * step
        }
    }

    /// `gain` as the firmware writes it: at the step's precision, without
    /// trailing zeros ("45" on Betaflight, "0.135" and "0.0036" on `ArduPilot`).
    pub fn gain_text(&self, gain: f64) -> String {
        let step = self.gain_step();
        let decimals = if step >= 1.0 {
            0
        } else {
            (-step.log10()).ceil().clamp(0.0, 9.0) as usize
        };
        let text = format!("{:.decimals$}", self.round_gain(gain));
        if decimals == 0 {
            return text;
        }
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text.is_empty() || text == "-" {
            "0".to_string()
        } else {
            text.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gyro_filter_runs_in_the_loop_its_firmware_puts_it_in() {
        let mut s = Settings::default();
        s.loop_rates.gyro_hz = Some(8000.0);
        s.loop_rates.pid_hz = Some(4000.0);
        // not known, or known to be so: every stage at the gyro rate
        assert_eq!(
            s.loop_rates.gyro_filter_layout,
            GyroFilterLayout::AllAtGyroRate
        );
        for stage in FilterStage::ALL {
            assert_eq!(
                s.gyro_filter_rate(stage),
                (ControlLoop::Gyro, Some(8000.0)),
                "{stage:?}"
            );
        }
        // Betaflight 4.2 and later: the second low-pass before the gyro is
        // brought down to the PID loop, everything else after
        s.loop_rates.gyro_filter_layout = GyroFilterLayout::PidLoopAfterLowpass2;
        for stage in FilterStage::ALL {
            let expected = if stage == FilterStage::Lowpass2 {
                (ControlLoop::Gyro, Some(8000.0))
            } else {
                (ControlLoop::Pid, Some(4000.0))
            };
            assert_eq!(s.gyro_filter_rate(stage), expected, "{stage:?}");
        }
        // a rate the headers do not give stays unknown, per loop
        s.loop_rates.pid_hz = None;
        assert_eq!(
            s.gyro_filter_rate(FilterStage::Lowpass1),
            (ControlLoop::Pid, None)
        );
        assert_eq!(
            s.gyro_filter_rate(FilterStage::Lowpass2),
            (ControlLoop::Gyro, Some(8000.0))
        );
    }

    #[test]
    fn gains_are_written_at_the_firmware_step() {
        let integer = Settings::default();
        assert_eq!(integer.gain_step(), 1.0);
        assert_eq!(integer.gain_text(45.0), "45");
        assert_eq!(integer.round_gain(37.4), 37.0);
        let fine = Settings {
            pid_step: Some(0.0001),
            ..Settings::default()
        };
        assert_eq!(fine.gain_text(0.135), "0.135");
        assert_eq!(fine.gain_text(0.0036), "0.0036");
        assert_eq!(fine.gain_text(0.0), "0");
        assert_eq!(fine.gain_text(0.00396), "0.004");
        assert!((fine.round_gain(0.00396) - 0.004).abs() < 1e-12);
        // exactly the decimal, without binary noise
        assert_eq!(fine.round_gain(0.1288 * 1.2), 0.1546);
        assert_eq!(fine.round_gain(0.008 * 1.2), 0.0096);
        // a step that makes no sense reads as 1
        let odd = Settings {
            pid_step: Some(0.0),
            ..Settings::default()
        };
        assert_eq!(odd.gain_step(), 1.0);
    }
}
