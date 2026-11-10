//! INAV profile (3.x … 9.x).
//!
//! INAV shares the Blackbox container and most field names with Betaflight.
//! Differences handled here: `axisRate` is the rate setpoint, `vbat`/`amperage`
//! are centi-units, `attitude[]` is in decidegrees, the navigation and
//! fixed-wing controller fields (`nav*`, `fwAlt*`, `fwPos*`, `servo[]`,
//! `rcData[]`, `AirSpeed`) get units, and the filter headers are named
//! `gyro_lpf_hz`/`dterm_lpf_hz`/`dynamicGyroNotch*`/`rpm_gyro_*`.

use super::common::{self, Log};
use openpid_core::settings::{DynNotch, FilterChain, Lowpass, LowpassKind, Notch, RpmNotch};
use openpid_core::{
    Axis, BatteryField, BoundProfile, ChannelId, FieldMapping, FirmwareInfo, FirmwareProfile,
    FirmwareVendor, FlightSession, RawHeaders, Settings, Unit,
};

#[derive(Debug, Default, Clone, Copy)]
/// INAV multirotor and fixed-wing logs.
pub struct Inav;

#[allow(
    clippy::unnecessary_wraps,
    reason = "every match arm of map_field yields an Option"
)]
fn custom(name: String, unit: Unit, scale: f64) -> Option<FieldMapping> {
    Some(FieldMapping::scaled(ChannelId::Custom(name), unit, scale))
}

fn xyz(idx: Option<usize>) -> Option<&'static str> {
    Some(match idx? {
        0 => "x",
        1 => "y",
        2 => "z",
        _ => return None,
    })
}

/// Headers INAV writes in `value * 100`.
fn centi(h: &RawHeaders, key: &str) -> Option<f64> {
    h.get_f64(key).map(|v| v / 100.0)
}

fn lowpass_kind(h: &RawHeaders, key: &str) -> Option<LowpassKind> {
    Some(match h.get_f64(key)? as i64 {
        0 => LowpassKind::Pt1,
        1 => LowpassKind::Biquad,
        2 => LowpassKind::Pt2,
        3 => LowpassKind::Pt3,
        other => LowpassKind::Other(other.to_string()),
    })
}

fn lowpass(h: &RawHeaders, hz_key: &str, type_key: &str) -> Option<Lowpass> {
    let cutoff_hz = h.get_f64(hz_key).filter(|v| *v > 0.0)?;
    Some(Lowpass {
        kind: lowpass_kind(h, type_key),
        cutoff_hz,
        dyn_min_hz: None,
        dyn_max_hz: None,
        dyn_expo: 0.0,
    })
}

fn gyro_filters(h: &RawHeaders) -> FilterChain {
    let notches = match (h.get_f64("gyro_notch_hz"), h.get_f64("gyro_notch_cutoff")) {
        (Some(center_hz), cutoff) if center_hz > 0.0 => vec![Notch {
            center_hz,
            cutoff_hz: cutoff.unwrap_or(0.0),
        }],
        _ => Vec::new(),
    };
    let dyn_enabled = h.get_f64("dynamicGyroNotchEnabled").is_none_or(|v| v > 0.5);
    let dyn_notch = h
        .get_f64("dynamicGyroNotchMinHz")
        .filter(|v| *v > 0.0 && dyn_enabled)
        .map(|min_hz| DynNotch {
            min_hz,
            // INAV tracks up to the gyro Nyquist band; the header only carries the floor
            max_hz: h.get_f64("dynamicGyroNotchMaxHz").unwrap_or(500.0),
            q: centi(h, "dynamicGyroNotchQ").unwrap_or(0.0),
            count: None,
        });
    let rpm_notch = h
        .get_f64("rpm_gyro_filter_enabled")
        .filter(|v| *v > 0.5)
        .map(|_| RpmNotch {
            harmonics: h.get_f64("rpm_gyro_harmonics").map_or(1, |v| v as u32),
            q: centi(h, "rpm_gyro_q").unwrap_or(0.0),
            min_hz: h.get_f64("rpm_gyro_min_hz").unwrap_or(0.0),
            fade_range_hz: 0.0,
            weights: Vec::new(),
            lpf_hz: 0.0,
        });
    FilterChain {
        lowpass1: lowpass(h, "gyro_lpf_hz", "gyro_lpf_type"),
        lowpass2: lowpass(
            h,
            "gyro_anti_aliasing_lpf_hz",
            "gyro_anti_aliasing_lpf_type",
        ),
        notches,
        dyn_notch,
        rpm_notch,
    }
}

fn dterm_filters(h: &RawHeaders) -> FilterChain {
    FilterChain {
        lowpass1: lowpass(h, "dterm_lpf_hz", "dterm_lpf_type"),
        lowpass2: lowpass(h, "dterm_lpf2_hz", "dterm_lpf2_type"),
        ..FilterChain::default()
    }
}

/// INAV's RC modes by box id, the first 32 `flightModeFlags` holds.
const BOXES_INAV: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "NAV ALTHOLD",
    "HEADING HOLD",
    "HEADFREE",
    "HEADADJ",
    "CAMSTAB",
    "NAV RTH",
    "NAV POSHOLD",
    "MANUAL",
    "BEEPER",
    "LEDS OFF",
    "LIGHTS",
    "NAV LAUNCH",
    "OSD OFF",
    "TELEMETRY",
    "BLACKBOX",
    "FAILSAFE",
    "NAV WP",
    "AIR MODE",
    "HOME RESET",
    "GCS NAV",
    "KILLSWITCH",
    "SURFACE",
    "FLAPERON",
    "TURN ASSIST",
    "SERVO AUTOTRIM",
    "AUTO TUNE",
    "CAMERA CONTROL 1",
    "CAMERA CONTROL 2",
    "CAMERA CONTROL 3",
];

impl FirmwareProfile for Inav {
    fn id(&self) -> &'static str {
        "inav"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        (fw.vendor == FirmwareVendor::Inav).then_some(100)
    }

    fn bind<'a>(&self, fw: &'a FirmwareInfo, h: &'a RawHeaders) -> Box<dyn BoundProfile + 'a> {
        Box::new(Bound(Log::new(fw, h, &BOXES_INAV[..])))
    }
}

/// The profile of one INAV log.
struct Bound<'a>(Log<'a>);

impl BoundProfile for Bound<'_> {
    fn map_field(&self, field: &str) -> Option<FieldMapping> {
        let (base, idx) = common::split_indexed(field);
        let axis = idx.and_then(Axis::from_index);
        match base {
            "axisRate" => Some(FieldMapping::new(
                ChannelId::Setpoint { axis: axis? },
                Unit::DegreesPerSecond,
            )),
            "vbat" => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Voltage),
                Unit::Volts,
                0.01,
            )),
            "amperage" => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Current),
                Unit::Amperes,
                0.01,
            )),
            "attitude" => custom(format!("attitude.{}", axis?.name()), Unit::Degrees, 0.1),
            "servo" => custom(format!("servo.{}", idx?), Unit::Microseconds, 1.0),
            "rcData" => custom(format!("rc_raw.{}", idx?), Unit::Microseconds, 1.0),
            "AirSpeed" => custom("airspeed".into(), Unit::Custom("cm/s".into()), 1.0),
            "accVib" => custom("acc_vibration".into(), Unit::Raw, 1.0),
            "navPos" => custom(format!("nav.pos.{}", xyz(idx)?), Unit::Centimeters, 1.0),
            "navVel" => custom(
                format!("nav.vel.{}", xyz(idx)?),
                Unit::Custom("cm/s".into()),
                1.0,
            ),
            "navAcc" => custom(
                format!("nav.acc.{}", xyz(idx)?),
                Unit::Custom("cm/s²".into()),
                1.0,
            ),
            "navTgtPos" => custom(
                format!("nav.target_pos.{}", xyz(idx)?),
                Unit::Centimeters,
                1.0,
            ),
            "navTgtVel" => custom(
                format!("nav.target_vel.{}", xyz(idx)?),
                Unit::Custom("cm/s".into()),
                1.0,
            ),
            "navEPH" => custom("nav.eph".into(), Unit::Centimeters, 1.0),
            "navEPV" => custom("nav.epv".into(), Unit::Centimeters, 1.0),
            "navSurf" => custom("nav.surface".into(), Unit::Centimeters, 1.0),
            "navTgtHdg" => custom("nav.target_heading".into(), Unit::Raw, 1.0),
            "navState" => custom("nav.state".into(), Unit::Flags, 1.0),
            "navFlags" => custom("nav.flags".into(), Unit::Flags, 1.0),
            "fwAltP" | "fwAltI" | "fwAltD" | "fwAltOut" | "fwPosP" | "fwPosI" | "fwPosD"
            | "fwPosOut" => {
                let (ctl, term) = base.split_at(5);
                custom(
                    format!(
                        "fw_{}.{}",
                        if ctl == "fwAlt" { "alt" } else { "pos" },
                        term.to_ascii_lowercase()
                    ),
                    Unit::Raw,
                    1.0,
                )
            }
            _ => self.0.map_field(field),
        }
    }

    fn settings(&self) -> Settings {
        let (fw, h) = (self.0.fw, self.0.h);
        let mut s = self.0.settings();
        s.gyro_filters = gyro_filters(h);
        s.dterm_filters = dterm_filters(h);
        // INAV logs none of Betaflight's detail headers: the defaults apply
        common::filter_details(&mut s.gyro_filters, "gyro", fw, h);
        common::filter_details(&mut s.dterm_filters, "dterm", fw, h);
        for key in [
            "gyro_lpf_hz",
            "gyro_lpf_type",
            "dterm_lpf_hz",
            "dterm_lpf_type",
            "dterm_lpf2_hz",
            "dynamicGyroNotchMinHz",
            "dynamicGyroNotchQ",
            "dynamicGyroNotchRange",
            "rpm_gyro_filter_enabled",
            "rpm_gyro_harmonics",
            "rpm_gyro_min_hz",
            "rpm_gyro_q",
            "yaw_lpf_hz",
            "motor_pwm_rate",
            "rates",
            "rc_rate",
            "rc_expo",
            "tpa_rate",
            "tpa_breakpoint",
            "levelPID",
            "altPID",
            "posPID",
            "Firmware type",
        ] {
            if let Some(v) = h.get(key) {
                s.extra.insert(key.to_string(), v.trim().to_string());
            }
        }
        s
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }

    fn flight_modes(&self, word: i64) -> Vec<openpid_core::FlightMode> {
        self.0.flight_modes(word)
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
    fn maps_inav_specific_fields() {
        let fw = FirmwareInfo::parse_revision("INAV 9.0.1 (d44f2cf6) MICOAIR743V2");
        let h = headers(&[("acc_1G", "2048"), ("motorOutput", "1050,2000")]);
        let p = Inav;
        assert_eq!(p.score(&fw, &h), Some(100));
        let log = p.bind(&fw, &h);
        let att = log.map_field("attitude[2]").unwrap();
        assert_eq!(att.id, ChannelId::Custom("attitude.yaw".into()));
        assert_eq!(att.unit, Unit::Degrees);
        assert_eq!(att.scale, 0.1);
        assert_eq!(
            log.map_field("servo[3]").unwrap().id,
            ChannelId::Custom("servo.3".into())
        );
        assert_eq!(
            log.map_field("navVel[1]").unwrap().id,
            ChannelId::Custom("nav.vel.y".into())
        );
        assert_eq!(
            log.map_field("fwAltOut").unwrap().id,
            ChannelId::Custom("fw_alt.out".into())
        );
        assert_eq!(
            log.map_field("fwPosP").unwrap().id,
            ChannelId::Custom("fw_pos.p".into())
        );
        assert_eq!(
            log.map_field("axisRate[0]").unwrap().id,
            ChannelId::Setpoint { axis: Axis::Roll }
        );
        assert_eq!(log.map_field("vbat").unwrap().scale, 0.01);
        assert!(log.map_field("navPos[3]").is_none());
        // shared fields still go through the common table
        assert_eq!(
            log.map_field("motor[0]").unwrap().id,
            ChannelId::Motor { idx: 0 }
        );
    }

    #[test]
    fn filters_from_inav_headers() {
        let h = headers(&[
            ("looptime", "500"),
            ("rollPID", "5,7,0,50"),
            ("pitchPID", "5,7,0,91"),
            ("yawPID", "6,10,0,60"),
            ("gyro_lpf_hz", "90"),
            ("dterm_lpf_hz", "85"),
            ("dterm_lpf_type", "2"),
            ("dterm_lpf2_hz", "0"),
            ("dynamicGyroNotchQ", "250"),
            ("dynamicGyroNotchMinHz", "60"),
            ("rpm_gyro_filter_enabled", "1"),
            ("rpm_gyro_harmonics", "3"),
            ("rpm_gyro_min_hz", "100"),
            ("rpm_gyro_q", "500"),
            ("gyro_notch_hz", "0"),
            ("motor_pwm_protocol", "5"),
        ]);
        let s = Inav.bind(&FirmwareInfo::unknown(), &h).settings();
        let pid = s.pid.unwrap();
        assert_eq!((pid.roll.p, pid.roll.i, pid.roll.f), (5.0, 7.0, Some(50.0)));
        let g = s.gyro_filters;
        let lp = g.lowpass1.unwrap();
        assert_eq!(lp.cutoff_hz, 90.0);
        assert_eq!(lp.kind, None, "no gyro_lpf_type header");
        assert!(g.lowpass2.is_none());
        assert!(g.notches.is_empty());
        let dn = g.dyn_notch.unwrap();
        assert_eq!((dn.min_hz, dn.q), (60.0, 2.5));
        let rpm = g.rpm_notch.unwrap();
        assert_eq!((rpm.harmonics, rpm.q, rpm.min_hz), (3, 5.0, 100.0));
        let d = s.dterm_filters.lowpass1.unwrap();
        assert_eq!((d.cutoff_hz, d.kind), (85.0, Some(LowpassKind::Pt2)));
        assert!(s.dterm_filters.lowpass2.is_none());
        assert_eq!(s.motor.protocol.as_deref(), Some("DSHOT150"));
        assert_eq!(s.loop_rates.gyro_hz, Some(2000.0));
        assert_eq!(s.extra.get("rpm_gyro_q").map(String::as_str), Some("500"));

        let h = headers(&[
            ("gyro_lpf_hz", "25"),
            ("gyro_lpf_type", "1"),
            ("gyro_notch_hz", "200"),
            ("gyro_notch_cutoff", "150"),
            ("dynamicGyroNotchEnabled", "0"),
            ("dynamicGyroNotchMinHz", "30"),
            ("rpm_gyro_filter_enabled", "0"),
        ]);
        let g = Inav
            .bind(&FirmwareInfo::unknown(), &h)
            .settings()
            .gyro_filters;
        assert_eq!(g.lowpass1.unwrap().kind, Some(LowpassKind::Biquad));
        assert_eq!(g.notches.len(), 1);
        assert_eq!(g.notches[0].cutoff_hz, 150.0);
        assert!(g.dyn_notch.is_none(), "explicitly disabled");
        assert!(g.rpm_notch.is_none());
    }
}
