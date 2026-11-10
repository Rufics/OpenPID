//! Extension point for firmware "slider" tuning models (Betaflight's
//! simplified tuning): a handful of percentages that the firmware expands
//! into PID gains and filter cut-offs.

use crate::settings::{PerAxis, PidGains, Settings};
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
    /// The names of the positions `min..=max` when the slider is a choice in
    /// whole steps and not a quantity (`off`, `RP`, `RPY`); empty for a
    /// quantity. Two names make it a switch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
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

    /// What [`SliderResult::cli`] is written for, for the user: `Betaflight CLI`.
    fn cli_name(&self) -> &'static str;

    /// The sliders with their current values.
    fn sliders(&self) -> Vec<SliderDef>;

    /// Expand slider values into gains. Missing keys take the current values.
    fn compute(&self, values: &SliderValues) -> SliderResult;

    /// Gains the log actually flew with, for comparison.
    fn logged(&self) -> Option<PerAxis<PidGains>>;
}

/// Ratio sliders over the gains a log flew with, for a log whose firmware
/// has no slider model of its own: the gains keep their proportions and are
/// scaled together.
///
/// Positions are in percent, 100 leaves the logged gains as they are:
///
/// - **master**: every P, I and D of every axis;
/// - **P:D ratio**: D of every axis (more damping for the same P);
/// - **P:I ratio**: I of roll and pitch;
/// - **roll:pitch ratio**: P, I and D of pitch;
/// - **yaw ratio**: P, I and D of yaw.
///
/// Feedforward is not part of it and stays as logged; D-min / D-max follow D.
/// The results are rounded to the firmware's gain step
/// ([`Settings::round_gain`]). There are no commands to apply: the gains are
/// typed into the firmware's own configurator.
#[derive(Debug, Clone, PartialEq)]
pub struct RatioSliders {
    settings: Settings,
    logged: PerAxis<PidGains>,
}

/// The ratio sliders: key and label.
const RATIO_SLIDERS: [(&str, &str); 5] = [
    ("master_multiplier", "Master multiplier"),
    ("pd_ratio", "P:D ratio"),
    ("pi_ratio", "P:I ratio"),
    ("roll_pitch_ratio", "Roll:pitch ratio"),
    ("yaw_ratio", "Yaw ratio"),
];

impl RatioSliders {
    /// Identifier of the model.
    pub const ID: &'static str = "ratio-sliders";

    /// The ratio sliders of a log, when it has PID gains.
    pub fn from_settings(settings: &Settings) -> Option<Self> {
        let logged = settings.pid?;
        Some(RatioSliders {
            settings: settings.clone(),
            logged,
        })
    }

    fn scaled(&self, g: &PidGains, p: f64, i: f64, d: f64) -> PidGains {
        let round = |v: f64| self.settings.round_gain(v);
        PidGains {
            p: round(g.p * p),
            i: round(g.i * i),
            d: round(g.d * d),
            f: g.f,
            d_min: g.d_min.map(|v| round(v * d)),
            d_max: g.d_max.map(|v| round(v * d)),
        }
    }
}

impl SliderModel for RatioSliders {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn cli_name(&self) -> &'static str {
        ""
    }

    fn sliders(&self) -> Vec<SliderDef> {
        RATIO_SLIDERS
            .iter()
            .map(|(key, label)| SliderDef {
                key: (*key).to_string(),
                label: (*label).to_string(),
                value: 100.0,
                min: 0.0,
                max: 200.0,
                default: 100.0,
                group: "pids".to_string(),
                options: Vec::new(),
            })
            .collect()
    }

    fn compute(&self, values: &SliderValues) -> SliderResult {
        let v = |key: &str| values.get(key).copied().unwrap_or(100.0).clamp(0.0, 200.0) / 100.0;
        let (m, pd, pi, rp, yaw) = (
            v("master_multiplier"),
            v("pd_ratio"),
            v("pi_ratio"),
            v("roll_pitch_ratio"),
            v("yaw_ratio"),
        );
        SliderResult {
            pid: PerAxis {
                roll: self.scaled(&self.logged.roll, m, pi * m, pd * m),
                pitch: self.scaled(&self.logged.pitch, rp * m, pi * rp * m, pd * rp * m),
                yaw: self.scaled(&self.logged.yaw, yaw * m, yaw * m, pd * yaw * m),
            },
            gyro_lpf1_hz: None,
            gyro_lpf2_hz: None,
            dterm_lpf1_hz: None,
            dterm_lpf2_hz: None,
            cli: Vec::new(),
        }
    }

    fn logged(&self) -> Option<PerAxis<PidGains>> {
        Some(self.logged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gains(p: f64, i: f64, d: f64) -> PidGains {
        PidGains {
            p,
            i,
            d,
            f: Some(100.0),
            d_min: None,
            d_max: Some(d * 1.2),
        }
    }

    fn values(pairs: &[(&str, f64)]) -> SliderValues {
        pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
    }

    #[test]
    fn ratio_sliders_scale_the_logged_gains() {
        // no gains, no sliders
        assert!(RatioSliders::from_settings(&Settings::default()).is_none());
        let settings = Settings {
            pid: Some(PerAxis {
                roll: gains(4.0, 0.05, 15.0),
                pitch: gains(4.0, 0.05, 15.0),
                yaw: gains(4.0, 0.05, 2.0),
            }),
            pid_step: Some(0.001),
            ..Settings::default()
        };
        let m = RatioSliders::from_settings(&settings).unwrap();
        assert_eq!(m.id(), "ratio-sliders");
        assert_eq!(m.sliders().len(), 5);
        assert!(
            m.sliders()
                .iter()
                .all(|d| d.value == 100.0 && d.options.is_empty())
        );
        // at 100 % everything is as logged
        let same = m.compute(&SliderValues::new());
        assert_eq!(Some(same.pid), m.logged());
        assert!(same.cli.is_empty() && same.gyro_lpf1_hz.is_none());
        // the numbers of the reference tool's slider tool for these start
        // values and ratios 1.35, 1.20, 1.30, 1.05 and master 1.45
        let out = m
            .compute(&values(&[
                ("pd_ratio", 135.0),
                ("pi_ratio", 120.0),
                ("roll_pitch_ratio", 130.0),
                ("yaw_ratio", 105.0),
                ("master_multiplier", 145.0),
            ]))
            .pid;
        let close = |a: f64, b: f64| (a - b).abs() < 0.006;
        assert!(close(out.roll.p, 5.8) && close(out.roll.i, 0.087) && close(out.roll.d, 29.36));
        assert!(close(out.pitch.p, 7.54) && close(out.pitch.i, 0.113) && close(out.pitch.d, 38.17));
        assert!(close(out.yaw.p, 6.09) && close(out.yaw.i, 0.076) && close(out.yaw.d, 4.11));
        // feedforward stays, D-max follows D
        assert_eq!(out.roll.f, Some(100.0));
        assert!(close(out.roll.d_max.unwrap(), 18.0 * 1.35 * 1.45));
        // an integer firmware gets integers
        let whole = Settings {
            pid_step: Some(1.0),
            ..settings
        };
        let out = RatioSliders::from_settings(&whole)
            .unwrap()
            .compute(&values(&[("master_multiplier", 145.0)]))
            .pid;
        assert_eq!((out.roll.p, out.roll.d), (6.0, 22.0));
    }
}
