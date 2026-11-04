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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// RPM (ESC telemetry) notch configuration.
pub struct RpmNotch {
    /// Harmonics filtered per motor.
    pub harmonics: u32,
    /// Notch quality factor.
    pub q: f64,
    /// Lowest filtered frequency.
    pub min_hz: f64,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Typed flight-controller settings; everything optional.
pub struct Settings {
    /// PID gains per axis.
    pub pid: Option<PerAxis<PidGains>>,
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
    /// Headers the profile did not interpret, verbatim.
    pub extra: BTreeMap<String, String>,
}
