//! Rotorflight (helicopter) profile.
//!
//! Shares the Betaflight layout, with helicopter specifics: `motor[]` is the
//! motor output in 0.1 % (0…1000, not PWM), `setpoint[3]`/`mixer[3]` are the
//! collective (±1000 = ±100 %), `mixer[0..2]` the cyclic/yaw mixer outputs,
//! `headspeed`/`tailspeed` are rotor speeds in rpm, `Vbat`/`Ibat` are
//! centi-units and `attitude[]` is in decidegrees. Governor, rescue and
//! airborne state changes arrive as events (see the parser).

use super::common::{self, Scales};
use openpid_core::{
    Axis, BatteryField, ChannelId, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor,
    FlightSession, RawHeaders, Settings, Unit,
};

#[derive(Debug, Default, Clone, Copy)]
/// Rotorflight 4.3+.
pub struct Rotorflight;

#[allow(
    clippy::unnecessary_wraps,
    reason = "every match arm of map_field yields an Option"
)]
fn custom(name: &str, unit: Unit, scale: f64) -> Option<FieldMapping> {
    Some(FieldMapping::scaled(
        ChannelId::Custom(name.to_string()),
        unit,
        scale,
    ))
}

/// Mode boxes of Rotorflight 4.2, in bit order.
const BOXES_RF_4_2: [&str; 31] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "PASSTHRU",
    "FAILSAFE",
    "RESCUE",
    "GPSRESCUE",
    "CAMSTAB",
    "BEEPER",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "SERVO1",
    "SERVO2",
    "SERVO3",
    "BLACKBOX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "PREARM",
    "BEEPGPSCOUNT",
    "VTXPITMODE",
    "PARALYZE",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
    "ACROTRAINER",
    "VTXCONTROLDISABLE",
];

/// Mode boxes of Rotorflight 4.3 to 4.5.
const BOXES_RF_4_3: [&str; 29] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "TRAINER",
    "ALTHOLD",
    "RESCUE",
    "GPSRESCUE",
    "FAILSAFE",
    "PREARM",
    "PARALYZE",
    "BEEPERON",
    "BEEPERMUTE",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "BEEPGPSCOUNT",
    "BLACKBOX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "VTXPITMODE",
    "VTXCONTROLDISABLE",
    "STICKCOMMANDDISABLE",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
];

/// Mode boxes of Rotorflight 4.6 on: the governor boxes before the user ones.
const BOXES_RF_4_6: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "TRAINER",
    "ALTHOLD",
    "RESCUE",
    "GPSRESCUE",
    "FAILSAFE",
    "PREARM",
    "PARALYZE",
    "BEEPERON",
    "BEEPERMUTE",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "BEEPGPSCOUNT",
    "BLACKBOX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "VTXPITMODE",
    "VTXCONTROLDISABLE",
    "STICKCOMMANDDISABLE",
    "GOVFALLBACK",
    "GOVSUSPEND",
    "GOVBYPASS",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
];

impl FirmwareProfile for Rotorflight {
    fn id(&self) -> &'static str {
        "rotorflight"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        (fw.vendor == FirmwareVendor::Rotorflight).then_some(100)
    }

    fn map_field(&self, field: &str, fw: &FirmwareInfo, h: &RawHeaders) -> Option<FieldMapping> {
        let (base, idx) = common::split_indexed(field);
        let axis = idx.and_then(Axis::from_index);
        match (base, idx) {
            ("Vbat", _) => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Voltage),
                Unit::Volts,
                0.01,
            )),
            ("Ibat", _) => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Current),
                Unit::Amperes,
                0.01,
            )),
            ("headspeed", _) => custom("headspeed", Unit::Rpm, 1.0),
            ("tailspeed", _) => custom("tailspeed", Unit::Rpm, 1.0),
            // motor output is logged as 0.1 % of full output, not as PWM
            ("motor", Some(i)) if i < 8 => Some(FieldMapping::scaled(
                ChannelId::Motor { idx: i as u8 },
                Unit::Percent,
                0.1,
            )),
            ("setpoint", Some(3)) => custom("collective", Unit::Percent, 0.1),
            ("mixer", Some(i)) => custom(
                &format!(
                    "mixer.{}",
                    match i {
                        0 => "roll",
                        1 => "pitch",
                        2 => "yaw",
                        3 => "collective",
                        _ => return None,
                    }
                ),
                Unit::Percent,
                0.1,
            ),
            ("attitude", _) => custom(&format!("attitude.{}", axis?.name()), Unit::Degrees, 0.1),
            _ => common::map_field(field, &Scales::betaflight(fw, h)),
        }
    }

    fn settings(&self, _fw: &FirmwareInfo, h: &RawHeaders) -> Settings {
        let mut s = common::settings(h);
        for key in [
            "gyro_rpm_filter_bank_motor_index",
            "gyro_rpm_filter_bank_gear_ratio",
            "gyro_rpm_filter_bank_notch_q",
            "gyro_rpm_filter_bank_min_hz",
            "gyro_rpm_filter_bank_max_hz",
            "gyro_decimation_hz",
            "rates_type",
            "rate_limits",
            "P ratio",
            "Board information",
        ] {
            if let Some(v) = h.get(key) {
                s.extra.insert(key.to_string(), v.trim().to_string());
            }
        }
        s
    }

    fn derive(&self, session: &mut FlightSession) {
        // rcCommand[3] is the collective stick (±500); the throttle is rcCommand[4], 0…1000
        let rc4 = ChannelId::RcCommand { ch: 4 };
        if let Some(c) = session.channels.get(&rc4) {
            let data = c.data.map(|v| (v / 10.0).clamp(0.0, 100.0));
            session.channels.insert(openpid_core::session::Channel {
                id: ChannelId::Throttle,
                name: "throttle".into(),
                unit: Unit::Percent,
                data,
                origin: openpid_core::session::Origin::Derived {
                    from: vec![rc4],
                    by: "rcCommand[4] / 10".into(),
                },
            });
        }
        common::derive(session);
        common::derive_collective_stick(session);
    }

    fn flight_modes(&self, fw: &FirmwareInfo, word: i64) -> Vec<openpid_core::FlightMode> {
        let names: &[&str] = if fw.is_at_least(4, 6) {
            &BOXES_RF_4_6
        } else if fw.is_at_least(4, 3) {
            &BOXES_RF_4_3
        } else if fw.is_at_least(4, 2) {
            &BOXES_RF_4_2
        } else {
            &common::BOXES_COMMON
        };
        openpid_core::modes::modes_from_mask(word, names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helicopter_fields() {
        let fw = FirmwareInfo::parse_revision("Rotorflight 4.3.0 (norevision) STM32F411");
        let h = RawHeaders(vec![
            ("minthrottle".into(), "1070".into()),
            ("maxthrottle".into(), "2000".into()),
            ("motor_poles".into(), "536878740".into()),
            ("acc_1G".into(), "2048".into()),
            ("gyro_rpm_filter_bank_gear_ratio".into(), "1000,2000".into()),
        ]);
        let p = Rotorflight;
        assert_eq!(p.score(&fw, &h), Some(100));
        let m = p.map_field("motor[0]", &fw, &h).unwrap();
        assert_eq!(m.id, ChannelId::Motor { idx: 0 });
        assert_eq!((m.unit, m.scale, m.offset), (Unit::Percent, 0.1, 0.0));
        assert_eq!(
            p.map_field("setpoint[3]", &fw, &h).unwrap().id,
            ChannelId::Custom("collective".into())
        );
        assert_eq!(
            p.map_field("mixer[2]", &fw, &h).unwrap().id,
            ChannelId::Custom("mixer.yaw".into())
        );
        assert!(p.map_field("mixer[4]", &fw, &h).is_none());
        assert_eq!(p.map_field("tailspeed", &fw, &h).unwrap().unit, Unit::Rpm);
        assert_eq!(
            p.map_field("attitude[1]", &fw, &h).unwrap().id,
            ChannelId::Custom("attitude.pitch".into())
        );
        assert_eq!(
            p.map_field("setpoint[1]", &fw, &h).unwrap().id,
            ChannelId::Setpoint { axis: Axis::Pitch }
        );
        let s = p.settings(&fw, &h);
        assert_eq!(s.motor.poles, None, "a RAM address is not a pole count");
        assert_eq!(
            s.extra
                .get("gyro_rpm_filter_bank_gear_ratio")
                .map(String::as_str),
            Some("1000,2000")
        );
    }
}
