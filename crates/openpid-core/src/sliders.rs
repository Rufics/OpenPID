//! Extension point for firmware "slider" tuning models (Betaflight's
//! simplified tuning): a handful of percentages that the firmware expands
//! into PID gains and filter cut-offs.

use crate::settings::{PerAxis, PidGains};
use serde::{Deserialize, Serialize};

/// One slider as the firmware defines it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SliderDef {
    /// Firmware parameter name (also the CLI `set` key).
    pub key: String,
    /// Human-readable label.
    pub label: String,
    /// Current value read from the log, or the firmware default when absent.
    pub value: f64,
    /// Lowest allowed value.
    pub min: f64,
    /// Highest allowed value.
    pub max: f64,
    /// Firmware default.
    pub default: f64,
    /// Group used for layout (e.g. `pids`, `filters`).
    pub group: String,
}

/// Slider values keyed by [`SliderDef::key`].
pub type SliderValues = std::collections::BTreeMap<String, f64>;

/// Gains and cut-offs produced from a slider set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SliderResult {
    /// Resulting PID gains per axis.
    pub pid: PerAxis<PidGains>,
    /// Gyro lowpass 1 (static, or dyn min/max) in Hz, when the sliders drive filters.
    pub gyro_lpf1_hz: Option<(f64, f64)>,
    /// Gyro lowpass 2 cut-off in Hz.
    pub gyro_lpf2_hz: Option<f64>,
    /// D-term lowpass 1 (min, max) cut-offs in Hz.
    pub dterm_lpf1_hz: Option<(f64, f64)>,
    /// D-term lowpass 2 cut-off in Hz.
    pub dterm_lpf2_hz: Option<f64>,
    /// Firmware CLI commands that would apply these sliders.
    pub cli: Vec<String>,
}

/// A firmware's slider model, created from a log's headers.
pub trait SliderModel: Send + Sync + std::fmt::Debug {
    /// Stable identifier, e.g. `betaflight-simplified-tuning`.
    fn id(&self) -> &'static str;

    /// The sliders with their current values.
    fn sliders(&self) -> Vec<SliderDef>;

    /// Expand slider values into gains. Missing keys take the current values.
    fn compute(&self, values: &SliderValues) -> SliderResult;

    /// Gains the log actually flew with, for comparison.
    fn logged(&self) -> Option<PerAxis<PidGains>>;
}
