//! `FirmwareProfile` for `ArduPilot`: maps `MESSAGE.Column` fields to `OpenPID`
//! channels and reads tuning parameters (`PARM`) into [`Settings`].
//!
//! Field names handed to [`FirmwareProfile::map_field`] are `MSG.Col`, e.g.
//! `IMU.GyrX` or `PIDR.Tar`; the raw headers carry every parameter by name
//! plus `vehicle` (`Copter`, `Plane`, ...) and `firmware`.

use openpid_core::settings::{
    DynNotch, FilterChain, Lowpass, LowpassKind, MotorConfig, Notch, PerAxis, PidGains, RpmNotch,
};
use openpid_core::{
    Axis, BatteryField, ChannelId, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor,
    GyroStage, PidTerm, RawHeaders, Settings, Unit,
};

const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;
const G: f64 = 9.806_65;

/// Vehicle family, from the firmware string (`ArduCopter`, `APM:Plane`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vehicle {
    /// Multirotor and traditional helicopter (rate loop in rad/s).
    Copter,
    /// Fixed wing, including `QuadPlane` (rate loop in deg/s).
    Plane,
    /// Rover, Sub, Tracker, Blimp: attitude channels only.
    Other,
}

impl Vehicle {
    /// Classify a firmware string.
    pub fn from_firmware(s: &str) -> Vehicle {
        let l = s.to_ascii_lowercase();
        if l.contains("copter") || l.contains("heli") {
            Vehicle::Copter
        } else if l.contains("plane") {
            Vehicle::Plane
        } else {
            Vehicle::Other
        }
    }

    /// Name as stored in the `vehicle` header.
    pub fn name(self) -> &'static str {
        match self {
            Vehicle::Copter => "Copter",
            Vehicle::Plane => "Plane",
            Vehicle::Other => "Other",
        }
    }

    fn parse(h: &RawHeaders) -> Vehicle {
        match h.get("vehicle") {
            Some("Copter") => Vehicle::Copter,
            Some("Plane") => Vehicle::Plane,
            _ => Vehicle::Other,
        }
    }
}

/// The `ArduPilot` profile.
#[derive(Debug, Default, Clone, Copy)]
pub struct ArdupilotProfile;

/// `X`/`Y`/`Z` sensor axes.
fn xyz_axis(suffix: &str) -> Option<Axis> {
    match suffix {
        "X" => Some(Axis::Roll),
        "Y" => Some(Axis::Pitch),
        "Z" => Some(Axis::Yaw),
        _ => None,
    }
}

/// `R`/`P`/`Y` controller axes.
fn rpy_axis(prefix: &str) -> Option<Axis> {
    match prefix {
        "R" => Some(Axis::Roll),
        "P" => Some(Axis::Pitch),
        "Y" => Some(Axis::Yaw),
        _ => None,
    }
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "every match arm of map_field yields an Option"
)]
fn mapping(id: ChannelId, unit: Unit, scale: f64) -> Option<FieldMapping> {
    Some(FieldMapping {
        id,
        unit,
        scale,
        offset: 0.0,
    })
}

fn custom(name: &str, unit: Unit, scale: f64) -> Option<FieldMapping> {
    mapping(ChannelId::Custom(name.to_string()), unit, scale)
}

/// `IMU.GyrX` → `(IMU, GyrX)`.
fn split(field: &str) -> Option<(&str, &str)> {
    field.split_once('.')
}

fn pid_axis(msg: &str) -> Option<Axis> {
    match msg {
        "PIDR" => Some(Axis::Roll),
        "PIDP" => Some(Axis::Pitch),
        "PIDY" => Some(Axis::Yaw),
        _ => None,
    }
}

impl FirmwareProfile for ArdupilotProfile {
    fn id(&self) -> &'static str {
        "ardupilot"
    }

    fn score(&self, firmware: &FirmwareInfo, _headers: &RawHeaders) -> Option<u8> {
        (firmware.vendor == FirmwareVendor::Ardupilot).then_some(100)
    }

    fn map_field(
        &self,
        field: &str,
        firmware: &FirmwareInfo,
        headers: &RawHeaders,
    ) -> Option<FieldMapping> {
        let vehicle = Vehicle::parse(headers);
        let (msg, col) = split(field)?;
        let rate_scale = match vehicle {
            Vehicle::Copter => RAD_TO_DEG,
            Vehicle::Plane | Vehicle::Other => 1.0,
        };
        // ArduCopter before 3.4 logged RATE in centidegrees per second
        let centi = firmware
            .version
            .is_some_and(|v| v < openpid_core::FirmwareVersion::new(3, 4, 0));
        let rate_log_scale = if centi { 0.01 } else { 1.0 };
        match (msg, col) {
            ("IMU", "TimeUS" | "TimeMS") => None,
            ("IMU", "GyrX" | "GyrY" | "GyrZ") => mapping(
                ChannelId::Gyro {
                    axis: xyz_axis(&col[3..])?,
                    stage: GyroStage::Filtered,
                },
                Unit::DegreesPerSecond,
                RAD_TO_DEG,
            ),
            ("IMU", "AccX" | "AccY" | "AccZ") => mapping(
                ChannelId::Acc {
                    axis: xyz_axis(&col[3..])?,
                },
                Unit::Gs,
                1.0 / G,
            ),
            ("RATE", "RDes" | "PDes" | "YDes") => mapping(
                ChannelId::Setpoint {
                    axis: rpy_axis(&col[..1])?,
                },
                Unit::DegreesPerSecond,
                rate_log_scale,
            ),
            ("RATE", "R" | "P" | "Y") => custom(
                &format!("rate.{}", rpy_axis(col)?.name()),
                Unit::DegreesPerSecond,
                rate_log_scale,
            ),
            ("RATE", "ROut" | "POut" | "YOut") => custom(
                &format!("rate_out.{}", rpy_axis(&col[..1])?.name()),
                Unit::Unitless,
                1.0,
            ),
            ("PIDR" | "PIDP" | "PIDY", "Tar") => {
                let axis = pid_axis(msg)?;
                // copter logs RATE.*Des as well; the PID target then stays a secondary channel
                if vehicle == Vehicle::Copter && headers.get("has_rate_msg") == Some("1") {
                    custom(
                        &format!("pid_target.{}", axis.name()),
                        Unit::DegreesPerSecond,
                        rate_scale,
                    )
                } else {
                    mapping(
                        ChannelId::Setpoint { axis },
                        Unit::DegreesPerSecond,
                        rate_scale,
                    )
                }
            }
            ("PIDR" | "PIDP" | "PIDY", "Act") => custom(
                &format!("pid_actual.{}", pid_axis(msg)?.name()),
                Unit::DegreesPerSecond,
                rate_scale,
            ),
            ("PIDR" | "PIDP" | "PIDY", "Err") => mapping(
                ChannelId::PidError {
                    axis: pid_axis(msg)?,
                },
                Unit::DegreesPerSecond,
                rate_scale,
            ),
            ("PIDR" | "PIDP" | "PIDY", "P" | "I" | "D" | "FF") => mapping(
                ChannelId::PidTerm {
                    axis: pid_axis(msg)?,
                    term: match col {
                        "P" => PidTerm::P,
                        "I" => PidTerm::I,
                        "D" => PidTerm::D,
                        _ => PidTerm::F,
                    },
                },
                Unit::Raw,
                1.0,
            ),
            ("RCOU", c) if c.starts_with("Ch") || c.starts_with('C') => {
                let n: u8 = c
                    .trim_start_matches("Ch")
                    .trim_start_matches('C')
                    .parse()
                    .ok()?;
                if vehicle == Vehicle::Copter {
                    let (low, high) = motor_range(headers);
                    Some(FieldMapping {
                        id: ChannelId::Motor {
                            idx: n.checked_sub(1)?,
                        },
                        unit: Unit::Percent,
                        scale: 100.0 / (high - low),
                        offset: -low,
                    })
                } else {
                    custom(&format!("servo.{n}"), Unit::Microseconds, 1.0)
                }
            }
            ("RCIN", c) if c.starts_with('C') => {
                let n: u8 = c[1..].parse().ok()?;
                mapping(
                    ChannelId::RcCommand {
                        ch: n.checked_sub(1)?,
                    },
                    Unit::Microseconds,
                    1.0,
                )
            }
            ("CTUN", "ThO") => mapping(
                ChannelId::Throttle,
                Unit::Percent,
                if vehicle == Vehicle::Copter {
                    100.0
                } else {
                    1.0
                },
            ),
            ("CTUN", "ThrOut") => mapping(ChannelId::Throttle, Unit::Percent, 0.1),
            ("CTUN", "ThrIn") => mapping(ChannelId::RcCommand { ch: 2 }, Unit::Raw, 1.0),
            ("BAT", "Volt") => mapping(ChannelId::Battery(BatteryField::Voltage), Unit::Volts, 1.0),
            ("BAT", "Curr") => mapping(
                ChannelId::Battery(BatteryField::Current),
                Unit::Amperes,
                1.0,
            ),
            ("BAT", "CurrTot") => mapping(
                ChannelId::Battery(BatteryField::Consumed),
                Unit::MilliampHours,
                1.0,
            ),
            ("CURR", "Volt") => {
                mapping(ChannelId::Battery(BatteryField::Voltage), Unit::Volts, 0.01)
            }
            ("CURR", "Curr") => mapping(
                ChannelId::Battery(BatteryField::Current),
                Unit::Amperes,
                0.01,
            ),
            ("CURR", "CurrTot") => mapping(
                ChannelId::Battery(BatteryField::Consumed),
                Unit::MilliampHours,
                1.0,
            ),
            ("ATT", "Roll" | "Pitch" | "Yaw" | "DesRoll" | "DesPitch" | "DesYaw") => custom(
                &format!("attitude.{}", col.to_ascii_lowercase()),
                Unit::Degrees,
                0.01,
            ),
            _ => None,
        }
    }

    fn settings(&self, _firmware: &FirmwareInfo, h: &RawHeaders) -> Settings {
        let vehicle = Vehicle::parse(h);
        let (roll, pitch, yaw) = match vehicle {
            Vehicle::Copter => ("ATC_RAT_RLL", "ATC_RAT_PIT", "ATC_RAT_YAW"),
            Vehicle::Plane => ("RLL_RATE", "PTCH_RATE", "YAW_RATE"),
            Vehicle::Other => ("", "", ""),
        };
        let gains = |prefix: &str| -> Option<PidGains> {
            let get = |suffix: &str| h.get_f64(&format!("{prefix}_{suffix}"));
            // ArduCopter < 3.6 used RATE_RLL_P etc.
            let legacy = |suffix: &str| {
                h.get_f64(&format!(
                    "RATE_{}_{suffix}",
                    prefix.trim_start_matches("ATC_RAT_")
                ))
            };
            let p = get("P").or_else(|| legacy("P"))?;
            Some(PidGains {
                p,
                i: get("I").or_else(|| legacy("I")).unwrap_or(0.0),
                d: get("D").or_else(|| legacy("D")).unwrap_or(0.0),
                f: get("FF"),
                d_min: None,
                d_max: None,
            })
        };
        // a plane without a yaw rate controller (YAW_RATE_ENABLE 0) has no yaw gains
        let pid = if roll.is_empty() {
            None
        } else {
            match (gains(roll), gains(pitch), gains(yaw)) {
                (Some(roll), Some(pitch), yaw) => Some(PerAxis {
                    roll,
                    pitch,
                    yaw: yaw.unwrap_or_default(),
                }),
                _ => None,
            }
        };
        let gyro_hz = h.get_f64("INS_GYRO_RATE").map(|v| match v as i64 {
            1 => 2000.0,
            2 => 4000.0,
            3 => 8000.0,
            _ => 1000.0,
        });
        let pid_hz = h.get_f64("SCHED_LOOP_RATE").filter(|v| *v > 0.0);
        let gyro_filters = FilterChain {
            lowpass1: h
                .get_f64("INS_GYRO_FILTER")
                .filter(|v| *v > 0.0)
                .map(|cutoff_hz| Lowpass {
                    kind: Some(LowpassKind::Biquad),
                    cutoff_hz,
                    dyn_min_hz: None,
                    dyn_max_hz: None,
                }),
            lowpass2: None,
            notches: Vec::new(),
            dyn_notch: None,
            rpm_notch: None,
        };
        let gyro_filters = harmonic_notch(h, gyro_filters);
        let dterm_cutoff = h
            .get_f64(&format!("{roll}_FLTD"))
            .filter(|v| *v > 0.0 && !roll.is_empty());
        let dterm_filters = FilterChain {
            lowpass1: dterm_cutoff.map(|cutoff_hz| Lowpass {
                kind: Some(LowpassKind::Pt1),
                cutoff_hz,
                dyn_min_hz: None,
                dyn_max_hz: None,
            }),
            ..FilterChain::default()
        };
        let (low, high) = motor_range(h);
        let motor = MotorConfig {
            poles: None,
            protocol: h.get_f64("MOT_PWM_TYPE").map(|v| {
                match v as i64 {
                    0 => "Normal",
                    1 => "OneShot",
                    2 => "OneShot125",
                    3 => "Brushed",
                    4 => "DShot150",
                    5 => "DShot300",
                    6 => "DShot600",
                    7 => "DShot1200",
                    _ => "unknown",
                }
                .to_string()
            }),
            output_low: Some(low),
            output_high: Some(high),
            kv: None,
            count: h.get_f64("FRAME_CLASS").and_then(|v| match v as i64 {
                1 => Some(4),
                2 | 5 => Some(6),
                3 | 4 => Some(8),
                7 => Some(3),
                12 => Some(12),
                _ => None,
            }),
        };
        let mut extra = std::collections::BTreeMap::new();
        for key in [
            "INS_HNTCH_ENABLE",
            "INS_HNTCH_MODE",
            "INS_HNTCH_FREQ",
            "INS_HNTCH_BW",
            "INS_HNTCH_HMNCS",
            "INS_HNTCH_REF",
            "INS_HNTCH_OPTS",
            "INS_GYRO_FILTER",
            "INS_ACCEL_FILTER",
            "SCHED_LOOP_RATE",
            "FRAME_CLASS",
            "FRAME_TYPE",
            "LOG_BITMASK",
        ] {
            if let Some(v) = h.get(key) {
                extra.insert(key.to_string(), v.to_string());
            }
        }
        if let Some(v) = h.get("vehicle") {
            extra.insert("vehicle".into(), v.to_string());
        }
        Settings {
            pid,
            loop_rates: openpid_core::settings::LoopRates {
                gyro_hz,
                pid_hz,
                log_hz: None,
            },
            motor,
            gyro_filters,
            dterm_filters,
            debug_mode: None,
            craft_name: h.get("craft_name").map(str::to_string),
            extra,
        }
    }
}

/// Motor PWM range: `MOT_PWM_MIN/MAX` when set, else the usual 1000–2000 µs.
fn motor_range(h: &RawHeaders) -> (f64, f64) {
    let low = h
        .get_f64("MOT_PWM_MIN")
        .filter(|v| *v > 0.0)
        .unwrap_or(1000.0);
    let high = h
        .get_f64("MOT_PWM_MAX")
        .filter(|v| *v > low)
        .unwrap_or(2000.0);
    (low, high)
}

/// `INS_HNTCH_*` as the nearest `OpenPID` notch description.
fn harmonic_notch(h: &RawHeaders, mut chain: FilterChain) -> FilterChain {
    if h.get_f64("INS_HNTCH_ENABLE").unwrap_or(0.0) < 0.5 {
        return chain;
    }
    let freq = h.get_f64("INS_HNTCH_FREQ").unwrap_or(0.0);
    let bw = h.get_f64("INS_HNTCH_BW").unwrap_or(0.0);
    let harmonics = h
        .get_f64("INS_HNTCH_HMNCS")
        .map_or(1, |m| (m as u32).count_ones());
    let q = if bw > 0.0 { freq / bw } else { 0.0 };
    match h.get_f64("INS_HNTCH_MODE").unwrap_or(0.0) as i64 {
        // 2 = RPM sensor, 3 = ESC telemetry: tracks motor speed
        2 | 3 => {
            chain.rpm_notch = Some(RpmNotch {
                harmonics,
                q,
                min_hz: freq,
            });
        }
        // 4 = in-flight FFT
        4 => {
            chain.dyn_notch = Some(DynNotch {
                min_hz: freq,
                max_hz: h
                    .get_f64("INS_HNTCH_FM_RAT")
                    .map_or(freq * 4.0, |r| freq * r.max(1.0) * 4.0),
                q,
                count: Some(harmonics),
            });
        }
        // 0 = fixed, 1 = throttle-based: a static description of the centre
        _ => {
            chain.notches.push(Notch {
                center_hz: freq,
                cutoff_hz: (freq - bw / 2.0).max(0.0),
            });
        }
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(vehicle: &str, pairs: &[(&str, &str)]) -> RawHeaders {
        let mut v = vec![("vehicle".to_string(), vehicle.to_string())];
        v.extend(
            pairs
                .iter()
                .map(|(k, x)| ((*k).to_string(), (*x).to_string())),
        );
        RawHeaders(v)
    }

    #[test]
    fn copter_fields_and_gains() {
        let h = headers(
            "Copter",
            &[
                ("ATC_RAT_RLL_P", "0.135"),
                ("ATC_RAT_RLL_I", "0.135"),
                ("ATC_RAT_RLL_D", "0.0036"),
                ("ATC_RAT_RLL_FF", "0"),
                ("ATC_RAT_PIT_P", "0.135"),
                ("ATC_RAT_YAW_P", "0.18"),
                ("ATC_RAT_RLL_FLTD", "20"),
                ("INS_GYRO_FILTER", "20"),
                ("INS_HNTCH_ENABLE", "1"),
                ("INS_HNTCH_MODE", "3"),
                ("INS_HNTCH_FREQ", "80"),
                ("INS_HNTCH_BW", "40"),
                ("INS_HNTCH_HMNCS", "7"),
                ("SCHED_LOOP_RATE", "400"),
                ("MOT_PWM_MIN", "1100"),
                ("MOT_PWM_MAX", "1900"),
                ("FRAME_CLASS", "1"),
                ("has_rate_msg", "1"),
            ],
        );
        let fw = FirmwareInfo::unknown();
        let p = ArdupilotProfile;
        let g = p.map_field("IMU.GyrY", &fw, &h).unwrap();
        assert_eq!(
            g.id,
            ChannelId::Gyro {
                axis: Axis::Pitch,
                stage: GyroStage::Filtered
            }
        );
        assert!((g.scale - 57.295_78).abs() < 1e-4);
        let sp = p.map_field("RATE.PDes", &fw, &h).unwrap();
        assert_eq!(sp.id, ChannelId::Setpoint { axis: Axis::Pitch });
        assert_eq!(sp.scale, 1.0);
        assert_eq!(
            p.map_field("PIDR.Tar", &fw, &h).unwrap().id,
            ChannelId::Custom("pid_target.roll".into())
        );
        let m = p.map_field("RCOU.C3", &fw, &h).unwrap();
        assert_eq!(m.id, ChannelId::Motor { idx: 2 });
        assert!((1500.0 + m.offset) * m.scale - 50.0 < 1e-9);
        assert_eq!(p.map_field("CTUN.ThO", &fw, &h).unwrap().scale, 100.0);
        assert!(p.map_field("GPS.Lat", &fw, &h).is_none());
        assert!(p.map_field("IMU.TimeUS", &fw, &h).is_none());
        let s = p.settings(&fw, &h);
        let pid = s.pid.unwrap();
        assert_eq!(pid.roll.p, 0.135);
        assert_eq!(pid.roll.d, 0.0036);
        assert_eq!(pid.yaw.p, 0.18);
        assert_eq!(s.loop_rates.pid_hz, Some(400.0));
        assert_eq!(s.gyro_filters.lowpass1.unwrap().cutoff_hz, 20.0);
        let rpm = s.gyro_filters.rpm_notch.unwrap();
        assert_eq!(rpm.harmonics, 3);
        assert_eq!(rpm.q, 2.0);
        assert_eq!(s.dterm_filters.lowpass1.unwrap().cutoff_hz, 20.0);
        assert_eq!(s.motor.count, Some(4));
        assert_eq!(s.motor.output_low, Some(1100.0));
    }

    #[test]
    fn plane_and_legacy_copter() {
        let h = headers(
            "Plane",
            &[
                ("RLL_RATE_P", "0.35"),
                ("PTCH_RATE_P", "0.48"),
                ("YAW_RATE_P", "0.5"),
                ("INS_HNTCH_ENABLE", "1"),
                ("INS_HNTCH_MODE", "1"),
                ("INS_HNTCH_FREQ", "160"),
                ("INS_HNTCH_BW", "80"),
            ],
        );
        let fw = FirmwareInfo::unknown();
        let p = ArdupilotProfile;
        let t = p.map_field("PIDP.Tar", &fw, &h).unwrap();
        assert_eq!(t.id, ChannelId::Setpoint { axis: Axis::Pitch });
        assert_eq!(t.scale, 1.0, "plane rate loop is already in deg/s");
        assert_eq!(
            p.map_field("RCOU.C1", &fw, &h).unwrap().id,
            ChannelId::Custom("servo.1".into())
        );
        assert_eq!(p.map_field("CTUN.ThO", &fw, &h).unwrap().scale, 1.0);
        let s = p.settings(&fw, &h);
        assert_eq!(s.pid.unwrap().pitch.p, 0.48);
        assert_eq!(s.gyro_filters.notches.len(), 1);
        assert_eq!(s.gyro_filters.notches[0].center_hz, 160.0);

        let h = headers(
            "Copter",
            &[
                ("RATE_RLL_P", "0.1288"),
                ("RATE_PIT_P", "0.168"),
                ("RATE_YAW_P", "0.66"),
            ],
        );
        let s = p.settings(&fw, &h);
        assert_eq!(s.pid.unwrap().roll.p, 0.1288);
        assert_eq!(
            p.map_field("CTUN.ThrOut", &fw, &h).unwrap().scale,
            0.1,
            "old copter throttle is 0..1000"
        );
        assert_eq!(p.map_field("CURR.Volt", &fw, &h).unwrap().scale, 0.01);
        assert_eq!(
            Vehicle::from_firmware("APM:Copter V3.3-dev"),
            Vehicle::Copter
        );
        assert_eq!(Vehicle::from_firmware("ArduPlane V4.4.4"), Vehicle::Plane);
        assert_eq!(Vehicle::from_firmware("ArduRover V4.4.4"), Vehicle::Other);
        assert_eq!(p.score(&FirmwareInfo::unknown(), &h), None);
    }
}
