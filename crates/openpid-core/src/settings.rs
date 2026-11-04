//! Typed flight-controller settings extracted from log headers.
//!
//! Every field is optional: a firmware profile fills what it recognises and
//! leaves the rest in [`Settings::extra`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PidGains {
    pub p: f64,
    pub i: f64,
    pub d: f64,
    pub f: Option<f64>,
    pub d_min: Option<f64>,
    pub d_max: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PerAxis<T> {
    pub roll: T,
    pub pitch: T,
    pub yaw: T,
}

impl<T> PerAxis<T> {
    pub fn get(&self, axis: crate::Axis) -> &T {
        match axis {
            crate::Axis::Roll => &self.roll,
            crate::Axis::Pitch => &self.pitch,
            crate::Axis::Yaw => &self.yaw,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LoopRates {
    pub gyro_hz: Option<f64>,
    pub pid_hz: Option<f64>,
    /// Rate at which main frames were logged.
    pub log_hz: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MotorConfig {
    pub poles: Option<u32>,
    pub protocol: Option<String>,
    pub output_low: Option<f64>,
    pub output_high: Option<f64>,
    pub kv: Option<f64>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LowpassKind {
    Pt1,
    Biquad,
    Pt2,
    Pt3,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lowpass {
    pub kind: Option<LowpassKind>,
    pub cutoff_hz: f64,
    /// Dynamic cutoff range, if the firmware moves the cutoff with throttle.
    pub dyn_min_hz: Option<f64>,
    pub dyn_max_hz: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Notch {
    pub center_hz: f64,
    pub cutoff_hz: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DynNotch {
    pub min_hz: f64,
    pub max_hz: f64,
    pub q: f64,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RpmNotch {
    pub harmonics: u32,
    pub q: f64,
    pub min_hz: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FilterChain {
    pub lowpass1: Option<Lowpass>,
    pub lowpass2: Option<Lowpass>,
    pub notches: Vec<Notch>,
    pub dyn_notch: Option<DynNotch>,
    pub rpm_notch: Option<RpmNotch>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DebugMode {
    pub code: u32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub pid: Option<PerAxis<PidGains>>,
    pub loop_rates: LoopRates,
    pub motor: MotorConfig,
    pub gyro_filters: FilterChain,
    pub dterm_filters: FilterChain,
    pub debug_mode: Option<DebugMode>,
    pub craft_name: Option<String>,
    /// Headers the profile did not interpret, verbatim.
    pub extra: BTreeMap<String, String>,
}
