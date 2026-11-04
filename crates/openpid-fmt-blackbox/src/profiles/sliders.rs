//! Betaflight simplified tuning ("PID sliders"), 4.3 and later.
//!
//! Re-implements `src/main/config/simplified_tuning.c`: every gain is the
//! firmware default scaled by slider percentages, computed in `f32` and
//! truncated toward zero exactly like the firmware's `constrain(float → uint8)`.
//! (The Configurator only sends slider positions; the flight controller does
//! the arithmetic.) The defaults changed in 2025.12 (D-min became D-max), so
//! the model is selected by firmware version. Verified against real logs in
//! the tests; gains set by a preset or by hand can of course differ.

use openpid_core::settings::{PerAxis, PidGains};
use openpid_core::{
    FirmwareInfo, FirmwareVersion, RawHeaders, SliderDef, SliderModel, SliderResult, SliderValues,
};

const PID_GAIN_MAX: f64 = 250.0;
const F_GAIN_MAX: f64 = 1000.0;
const LPF_MAX_HZ: f64 = 1000.0;
const DTERM_LPF1_DYN_MIN_HZ_DEFAULT: f64 = 75.0;
const DTERM_LPF1_DYN_MAX_HZ_DEFAULT: f64 = 150.0;
const DTERM_LPF2_HZ_DEFAULT: f64 = 150.0;
const GYRO_LPF1_DYN_MIN_HZ_DEFAULT: f64 = 250.0;
const GYRO_LPF1_DYN_MAX_HZ_DEFAULT: f64 = 500.0;
const GYRO_LPF2_HZ_DEFAULT: f64 = 500.0;

/// Firmware defaults per axis: `(P, I, D, F)` plus the D-min/D-max partner.
#[derive(Debug, Clone, Copy)]
struct Defaults {
    pid: [[f64; 4]; 3],
    /// 4.3–4.5: `D_MIN_DEFAULT`; 2025.12+: `D_MAX_DEFAULT`.
    d_partner: [f64; 3],
    /// Whether the partner is D-max (2025.12+) rather than D-min.
    d_max_scheme: bool,
}

fn defaults_for(version: Option<FirmwareVersion>) -> Defaults {
    let modern = version.is_some_and(|v| v >= FirmwareVersion::new(2025, 12, 0));
    if modern {
        Defaults {
            pid: [
                [45.0, 80.0, 30.0, 120.0],
                [47.0, 84.0, 34.0, 125.0],
                [45.0, 80.0, 0.0, 120.0],
            ],
            d_partner: [40.0, 46.0, 0.0],
            d_max_scheme: true,
        }
    } else {
        Defaults {
            pid: [
                [45.0, 80.0, 40.0, 120.0],
                [47.0, 84.0, 46.0, 125.0],
                [45.0, 80.0, 0.0, 120.0],
            ],
            d_partner: [30.0, 34.0, 0.0],
            d_max_scheme: false,
        }
    }
}

/// `constrain(x, 0, max)` followed by the C float→integer truncation.
fn clamp_int(x: f32, max: f64) -> f64 {
    f64::from(x.clamp(0.0, max as f32).trunc())
}

/// Betaflight slider model built from a log's headers.
#[derive(Debug, Clone)]
pub struct BetaflightSliders {
    defaults: Defaults,
    current: SliderValues,
    logged: Option<PerAxis<PidGains>>,
    /// Whether the log's `gyro_lpf1_dyn_hz` / static values were enabled, which
    /// decides which filter outputs the sliders drive.
    gyro_dyn: bool,
    gyro_static: bool,
    gyro_lpf2: bool,
    dterm_dyn: bool,
    dterm_static: bool,
    dterm_lpf2: bool,
}

/// `(key, label, default, min, max, group)` of every slider, log header name first.
const SLIDERS: [(&str, &str, f64, f64, f64, &str); 12] = [
    (
        "simplified_pids_mode",
        "PID sliders mode (0 off, 1 RP, 2 RPY)",
        2.0,
        0.0,
        2.0,
        "mode",
    ),
    (
        "simplified_master_multiplier",
        "Master multiplier",
        100.0,
        0.0,
        200.0,
        "pids",
    ),
    (
        "simplified_pi_gain",
        "P & I gain",
        100.0,
        0.0,
        200.0,
        "pids",
    ),
    ("simplified_i_gain", "I gain", 100.0, 0.0, 200.0, "pids"),
    ("simplified_d_gain", "D gain", 100.0, 0.0, 200.0, "pids"),
    (
        "simplified_feedforward_gain",
        "Feedforward gain",
        100.0,
        0.0,
        200.0,
        "pids",
    ),
    (
        "simplified_pitch_d_gain",
        "Pitch D gain (roll/pitch ratio)",
        100.0,
        0.0,
        200.0,
        "pids",
    ),
    (
        "simplified_pitch_pi_gain",
        "Pitch P & I gain",
        100.0,
        0.0,
        200.0,
        "pids",
    ),
    (
        "simplified_d_max_gain",
        "D-max gain (4.x: dmax_gain)",
        100.0,
        0.0,
        200.0,
        "pids",
    ),
    (
        "simplified_dterm_filter",
        "D-term filter sliders on",
        1.0,
        0.0,
        1.0,
        "filters",
    ),
    (
        "simplified_dterm_filter_multiplier",
        "D-term filter multiplier",
        100.0,
        10.0,
        200.0,
        "filters",
    ),
    (
        "simplified_gyro_filter_multiplier",
        "Gyro filter multiplier",
        100.0,
        10.0,
        200.0,
        "filters",
    ),
];

impl BetaflightSliders {
    /// `None` when the log predates simplified tuning (no `simplified_pids_mode` header).
    pub fn from_headers(firmware: &FirmwareInfo, h: &RawHeaders) -> Option<Self> {
        h.get("simplified_pids_mode")?;
        let mut current = SliderValues::new();
        for (key, _, default, _, _, _) in SLIDERS {
            // 4.x logs spell the D-max slider `simplified_dmax_gain`.
            let v = h.get_f64(key).or_else(|| {
                (key == "simplified_d_max_gain")
                    .then(|| h.get_f64("simplified_dmax_gain"))
                    .flatten()
            });
            current.insert(key.to_string(), v.unwrap_or(default));
        }
        current.insert(
            "simplified_gyro_filter".into(),
            h.get_f64("simplified_gyro_filter").unwrap_or(1.0),
        );
        let logged = super::common::settings(h).pid;
        let list = |k: &str| h.get_list(k).unwrap_or_default();
        let gyro_dyn_hz = list("gyro_lpf1_dyn_hz");
        let dterm_dyn_hz = list("dterm_lpf1_dyn_hz");
        Some(BetaflightSliders {
            defaults: defaults_for(firmware.version),
            current,
            logged,
            gyro_dyn: gyro_dyn_hz.first().is_some_and(|v| *v > 0.0),
            gyro_static: h.get_f64("gyro_lpf1_static_hz").is_some_and(|v| v > 0.0),
            gyro_lpf2: h.get_f64("gyro_lpf2_static_hz").is_some_and(|v| v > 0.0),
            dterm_dyn: dterm_dyn_hz.first().is_some_and(|v| *v > 0.0),
            dterm_static: h.get_f64("dterm_lpf1_static_hz").is_some_and(|v| v > 0.0),
            dterm_lpf2: h.get_f64("dterm_lpf2_static_hz").is_some_and(|v| v > 0.0),
        })
    }

    /// Slider value as the firmware sees it (`uint8_t / 100.0f`).
    fn value(&self, values: &SliderValues, key: &str) -> f32 {
        values
            .get(key)
            .or_else(|| self.current.get(key))
            .copied()
            .unwrap_or(100.0)
            .round() as f32
    }
}

/// Slider percentages as `f32` factors (`slider / 100`), as the firmware uses them.
#[derive(Debug, Clone, Copy)]
struct GainFactors {
    master: f32,
    pi: f32,
    d: f32,
    ff: f32,
    i: f32,
    d_max: f32,
    pitch_pi: f32,
    pitch_d: f32,
}

/// Filter cut-off as the firmware computes it (`default * slider / 100`, truncated).
fn lpf(base: f64, m: f32) -> f64 {
    (base * f64::from(m)).clamp(0.0, LPF_MAX_HZ).trunc()
}

/// `(min, max)` cut-offs of a dynamic LPF1, or the static cut-off twice.
fn lpf1_range(dynamic: bool, min_default: f64, max_default: f64, m: f32) -> (f64, f64) {
    if dynamic {
        (lpf(min_default, m), lpf(max_default, m))
    } else {
        (lpf(min_default, m), lpf(min_default, m))
    }
}

impl BetaflightSliders {
    fn logged_axis(&self, axis: usize) -> Option<PidGains> {
        self.logged.as_ref().map(|p| match axis {
            0 => p.roll,
            1 => p.pitch,
            _ => p.yaw,
        })
    }

    /// Gains of one axis from the firmware defaults and the slider factors.
    fn axis_gains(&self, axis: usize, f: &GainFactors) -> PidGains {
        let def = self.defaults.pid[axis].map(|v| v as f32);
        let partner = self.defaults.d_partner[axis] as f32;
        let (pitch_pi, pitch_d) = if axis == 1 {
            (f.pitch_pi, f.pitch_d)
        } else {
            (1.0, 1.0)
        };
        let mut gains = PidGains {
            p: clamp_int(def[0] * f.master * f.pi * pitch_pi, PID_GAIN_MAX),
            i: clamp_int(def[1] * f.master * f.pi * f.i * pitch_pi, PID_GAIN_MAX),
            f: Some(clamp_int(def[3] * f.master * pitch_pi * f.ff, F_GAIN_MAX)),
            ..PidGains::default()
        };
        // keep the firmware's left-to-right `f32` evaluation order: it decides the truncation
        if self.defaults.d_max_scheme {
            gains.d = clamp_int(def[2] * f.master * f.d * pitch_d, PID_GAIN_MAX);
            let gain = if partner > 0.0 {
                f.d_max + (1.0 - f.d_max) * def[2] / partner
            } else {
                1.0
            };
            gains.d_max = Some(clamp_int(
                partner * f.master * f.d * pitch_d * gain,
                PID_GAIN_MAX,
            ));
        } else {
            let ratio = if partner > 0.0 {
                1.0 + ((def[2] - partner) / partner) * f.d_max
            } else {
                1.0
            };
            gains.d = clamp_int(partner * f.master * f.d * pitch_d * ratio, PID_GAIN_MAX);
            gains.d_min = Some(clamp_int(partner * f.master * f.d * pitch_d, PID_GAIN_MAX));
        }
        gains
    }
}

impl SliderModel for BetaflightSliders {
    fn id(&self) -> &'static str {
        "betaflight-simplified-tuning"
    }

    fn sliders(&self) -> Vec<SliderDef> {
        SLIDERS
            .iter()
            .map(|(key, label, default, min, max, group)| SliderDef {
                key: (*key).to_string(),
                label: (*label).to_string(),
                value: self.current.get(*key).copied().unwrap_or(*default),
                min: *min,
                max: *max,
                default: *default,
                group: (*group).to_string(),
            })
            .collect()
    }

    fn compute(&self, values: &SliderValues) -> SliderResult {
        let v = |k: &str| self.value(values, k);
        let mode = v("simplified_pids_mode") as i32;
        let factors = GainFactors {
            master: v("simplified_master_multiplier") / 100.0,
            pi: v("simplified_pi_gain") / 100.0,
            d: v("simplified_d_gain") / 100.0,
            ff: v("simplified_feedforward_gain") / 100.0,
            i: v("simplified_i_gain") / 100.0,
            d_max: v("simplified_d_max_gain") / 100.0,
            pitch_pi: v("simplified_pitch_pi_gain") / 100.0,
            pitch_d: v("simplified_pitch_d_gain") / 100.0,
        };
        let mut out: [PidGains; 3] = [PidGains::default(); 3];
        for (axis, gains) in out.iter_mut().enumerate() {
            // axes beyond the mode keep the logged gains (the firmware leaves them alone)
            if axis as i32 > mode {
                if let Some(l) = self.logged_axis(axis) {
                    *gains = l;
                }
            } else {
                *gains = self.axis_gains(axis, &factors);
            }
        }
        let dm = v("simplified_dterm_filter_multiplier") / 100.0;
        let gm = v("simplified_gyro_filter_multiplier") / 100.0;
        let dterm_on = v("simplified_dterm_filter") >= 0.5;
        let gyro_on = v("simplified_gyro_filter") >= 0.5;
        let dterm_lpf1_hz = (dterm_on && (self.dterm_dyn || self.dterm_static)).then(|| {
            lpf1_range(
                self.dterm_dyn,
                DTERM_LPF1_DYN_MIN_HZ_DEFAULT,
                DTERM_LPF1_DYN_MAX_HZ_DEFAULT,
                dm,
            )
        });
        let gyro_lpf1_hz = (gyro_on && (self.gyro_dyn || self.gyro_static)).then(|| {
            lpf1_range(
                self.gyro_dyn,
                GYRO_LPF1_DYN_MIN_HZ_DEFAULT,
                GYRO_LPF1_DYN_MAX_HZ_DEFAULT,
                gm,
            )
        });
        let mut cli: Vec<String> = SLIDERS
            .iter()
            .filter(|(key, ..)| *key != "simplified_pids_mode")
            .map(|(key, ..)| format!("set {key} = {}", v(key) as i32))
            .collect();
        cli.insert(0, format!("set simplified_pids_mode = {mode}"));
        cli.push("simplified_tuning apply".into());
        cli.push("save".into());
        SliderResult {
            pid: PerAxis {
                roll: out[0],
                pitch: out[1],
                yaw: out[2],
            },
            gyro_lpf1_hz,
            gyro_lpf2_hz: (gyro_on && self.gyro_lpf2).then(|| lpf(GYRO_LPF2_HZ_DEFAULT, gm)),
            dterm_lpf1_hz,
            dterm_lpf2_hz: (dterm_on && self.dterm_lpf2).then(|| lpf(DTERM_LPF2_HZ_DEFAULT, dm)),
            cli,
        }
    }

    fn logged(&self) -> Option<PerAxis<PidGains>> {
        self.logged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> RawHeaders {
        RawHeaders(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn reproduces_betaflight_4_5_log() {
        // headers from betaflight-4.5.2-Run2.bbl
        let h = headers(&[
            ("simplified_pids_mode", "2"),
            ("simplified_master_multiplier", "85"),
            ("simplified_pi_gain", "100"),
            ("simplified_i_gain", "100"),
            ("simplified_d_gain", "85"),
            ("simplified_feedforward_gain", "100"),
            ("simplified_pitch_d_gain", "100"),
            ("simplified_pitch_pi_gain", "100"),
            ("simplified_dmax_gain", "100"),
            ("rollPID", "38,68,28"),
            ("pitchPID", "39,71,33"),
            ("yawPID", "38,68,0"),
            ("d_min", "21,24,0"),
            ("ff_weight", "101,106,101"),
            ("gyro_lpf1_dyn_hz", "0,500"),
            ("gyro_lpf2_static_hz", "500"),
            ("dterm_lpf1_dyn_hz", "75,150"),
            ("dterm_lpf1_static_hz", "75"),
            ("dterm_lpf2_static_hz", "150"),
        ]);
        let fw = FirmwareInfo::parse_revision("Betaflight 4.5.2 (024f8e13d) STM32F7X2");
        let m = BetaflightSliders::from_headers(&fw, &h).unwrap();
        let r = m.compute(&SliderValues::new());
        assert_eq!(
            (
                r.pid.roll.p,
                r.pid.roll.i,
                r.pid.roll.d,
                r.pid.roll.f,
                r.pid.roll.d_min
            ),
            (38.0, 68.0, 28.0, Some(102.0), Some(21.0))
        );
        assert_eq!(
            (
                r.pid.pitch.p,
                r.pid.pitch.i,
                r.pid.pitch.d,
                r.pid.pitch.f,
                r.pid.pitch.d_min
            ),
            (39.0, 71.0, 33.0, Some(106.0), Some(24.0))
        );
        assert_eq!(
            (r.pid.yaw.p, r.pid.yaw.i, r.pid.yaw.d, r.pid.yaw.f),
            (38.0, 68.0, 0.0, Some(102.0))
        );
        assert_eq!(r.gyro_lpf1_hz, None, "gyro lpf1 disabled in this log");
        assert_eq!(r.gyro_lpf2_hz, Some(500.0));
        assert_eq!(r.dterm_lpf1_hz, Some((75.0, 150.0)));
        assert!(
            r.cli
                .iter()
                .any(|c| c == "set simplified_master_multiplier = 85")
        );
        // moving the master slider scales everything
        let mut v = SliderValues::new();
        v.insert("simplified_master_multiplier".into(), 100.0);
        let r2 = m.compute(&v);
        assert_eq!((r2.pid.roll.p, r2.pid.roll.i), (45.0, 80.0));
    }

    #[test]
    fn reproduces_betaflight_2025_12_log() {
        let h = headers(&[
            ("simplified_pids_mode", "2"),
            ("simplified_master_multiplier", "100"),
            ("simplified_pi_gain", "130"),
            ("simplified_i_gain", "40"),
            ("simplified_d_gain", "130"),
            ("simplified_feedforward_gain", "100"),
            ("simplified_pitch_d_gain", "100"),
            ("simplified_pitch_pi_gain", "100"),
            ("simplified_d_max_gain", "100"),
            ("rollPID", "58,41,39"),
            ("pitchPID", "61,43,44"),
            ("yawPID", "58,41,0"),
            ("d_max", "52,59,0"),
            ("ff_weight", "120,125,120"),
        ]);
        let fw = FirmwareInfo::parse_revision("Betaflight 2025.12.0-RC4 (37800445d) STM32F7X2");
        let m = BetaflightSliders::from_headers(&fw, &h).unwrap();
        let r = m.compute(&SliderValues::new());
        assert_eq!(
            (r.pid.roll.p, r.pid.roll.i, r.pid.roll.d, r.pid.roll.d_max),
            (58.0, 41.0, 39.0, Some(52.0))
        );
        assert_eq!(
            (
                r.pid.pitch.p,
                r.pid.pitch.i,
                r.pid.pitch.d,
                r.pid.pitch.d_max
            ),
            (61.0, 43.0, 44.0, Some(59.0))
        );
        assert_eq!((r.pid.yaw.p, r.pid.yaw.i, r.pid.yaw.d), (58.0, 41.0, 0.0));
        assert_eq!(r.pid.roll.f, Some(120.0));
    }

    #[test]
    fn absent_before_4_3_and_mode_limits_axes() {
        let fw = FirmwareInfo::parse_revision("Betaflight 4.2.11 (948ba6339) STM32F7X2");
        assert!(BetaflightSliders::from_headers(&fw, &headers(&[("rollPID", "1,2,3")])).is_none());
        let h = headers(&[
            ("simplified_pids_mode", "1"),
            ("rollPID", "45,80,40"),
            ("pitchPID", "47,84,46"),
            ("yawPID", "99,98,0"),
        ]);
        let fw = FirmwareInfo::parse_revision("Betaflight 4.4.3 (738127e7e) STM32F405");
        let m = BetaflightSliders::from_headers(&fw, &h).unwrap();
        let r = m.compute(&SliderValues::new());
        assert_eq!(
            (r.pid.yaw.p, r.pid.yaw.i),
            (99.0, 98.0),
            "yaw untouched in RP mode"
        );
        assert_eq!(m.sliders().len(), 12);
    }
}
