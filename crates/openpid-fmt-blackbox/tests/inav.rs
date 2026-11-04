//! INAV fixtures (3.0.1 and 9.0.1, `tests/logs/`): channel mapping, units
//! and settings of the `inav` profile.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::settings::LowpassKind;
use openpid_core::{
    Axis, BatteryField, ChannelId, FirmwareVendor, FirmwareVersion, GyroStage, LogFormat,
    OpenOptions, PidTerm, Unit,
};
use openpid_fmt_blackbox::BlackboxFormat;

fn open(name: &str) -> openpid_core::FlightSession {
    let data = std::fs::read(common::logs_dir().join(name)).unwrap();
    let log = BlackboxFormat::new()
        .open(&data, &OpenOptions::default())
        .unwrap();
    log.sessions.into_iter().next().unwrap()
}

fn range(s: &openpid_core::FlightSession, id: &ChannelId) -> (f64, f64) {
    let c = s
        .channel(id)
        .unwrap_or_else(|| panic!("missing {}", id.key()));
    c.data
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        })
}

#[test]
fn inav_9_fixed_wing() {
    let s = open("inav-9.0.1-truncated.TXT");
    assert_eq!(s.firmware.vendor, FirmwareVendor::Inav);
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(9, 0, 1)));
    assert_eq!(s.profile_id, "inav");
    assert!(
        s.diagnostics.unmapped_fields.len() <= 3,
        "unmapped: {:?}",
        s.diagnostics.unmapped_fields
    );
    assert_eq!(s.diagnostics.corrupt_frames, 1);
    // setpoint - gyro == pidError on every sample (INAV logs the error directly)
    let sp = &s
        .channel(&ChannelId::Setpoint { axis: Axis::Pitch })
        .unwrap()
        .data;
    let gyro = &s
        .channel(&ChannelId::Gyro {
            axis: Axis::Pitch,
            stage: GyroStage::Filtered,
        })
        .unwrap()
        .data;
    let err = &s
        .channel(&ChannelId::PidError { axis: Axis::Pitch })
        .unwrap()
        .data;
    let worst = (0..s.len())
        .map(|i| ((sp.get(i).unwrap() - gyro.get(i).unwrap()) - err.get(i).unwrap()).abs())
        .fold(0.0, f64::max);
    assert!(
        worst <= 1.0,
        "axisRate is the setpoint in deg/s: worst residual {worst}"
    );
    let att = s
        .channel(&ChannelId::Custom("attitude.yaw".into()))
        .unwrap();
    assert_eq!(att.unit, Unit::Degrees);
    let (lo, hi) = range(&s, &att.id);
    assert!(
        lo > 270.0 && hi < 280.0,
        "decidegrees → degrees: {lo}..{hi}"
    );
    let (lo, hi) = range(&s, &ChannelId::Custom("servo.0".into()));
    assert!(lo > 1300.0 && hi < 1450.0, "servo µs: {lo}..{hi}");
    assert_eq!(
        s.channel(&ChannelId::Custom("nav.vel.z".into()))
            .unwrap()
            .unit,
        Unit::Custom("cm/s".into())
    );
    assert!(s.channels.contains(&ChannelId::Custom("fw_pos.out".into())));
    assert!(s.channels.contains(&ChannelId::PidTerm {
        axis: Axis::Roll,
        term: PidTerm::F
    }));
    let (lo, hi) = range(&s, &ChannelId::Battery(BatteryField::Voltage));
    assert!(lo > 12.0 && hi < 12.3, "centivolts: {lo}..{hi}");
    let (lo, hi) = range(&s, &ChannelId::Throttle);
    assert!(lo >= 0.0 && hi <= 100.0);

    let pid = s.settings.pid.unwrap();
    assert_eq!(
        (pid.roll.p, pid.roll.i, pid.roll.d, pid.roll.f),
        (5.0, 7.0, 0.0, Some(50.0))
    );
    assert_eq!(pid.pitch.f, Some(91.0));
    assert_eq!(s.settings.loop_rates.gyro_hz, Some(2000.0));
    let log_hz = s.settings.loop_rates.log_hz.unwrap();
    assert!(
        (990.0..1010.0).contains(&log_hz),
        "P interval 1/2 of 2 kHz: {log_hz}"
    );
    let g = &s.settings.gyro_filters;
    assert_eq!(g.lowpass1.as_ref().unwrap().cutoff_hz, 90.0);
    assert_eq!(g.dyn_notch.unwrap().min_hz, 60.0);
    assert_eq!(g.dyn_notch.unwrap().q, 2.5);
    assert!(g.rpm_notch.is_none());
    let d = s.settings.dterm_filters.lowpass1.as_ref().unwrap();
    assert_eq!(
        (d.cutoff_hz, d.kind.clone()),
        (85.0, Some(LowpassKind::Pt2))
    );
    assert_eq!(s.settings.motor.protocol.as_deref(), Some("DSHOT150"));
    assert_eq!(s.settings.motor.output_low, Some(1050.0));
    assert_eq!(s.settings.craft_name.as_deref(), Some("FLIK WING"));
    assert_eq!(
        s.settings.extra.get("Firmware type").map(String::as_str),
        Some("Cleanflight")
    );
}

#[test]
fn inav_3_multirotor_era() {
    let s = open("inav-3.0.1-truncated.TXT");
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(3, 0, 1)));
    assert_eq!(s.profile_id, "inav");
    assert!(
        s.diagnostics.unmapped_fields.len() <= 3,
        "unmapped: {:?}",
        s.diagnostics.unmapped_fields
    );
    assert!(s.channels.contains(&ChannelId::PidTerm {
        axis: Axis::Roll,
        term: PidTerm::D
    }));
    assert!(s.channels.contains(&ChannelId::Custom("servo.15".into())));
    assert!(s.channels.contains(&ChannelId::Custom("nav.acc.x".into())));
    assert_eq!(
        s.channel(&ChannelId::Custom("nav.surface".into()))
            .unwrap()
            .unit,
        Unit::Centimeters
    );
    let (lo, hi) = range(&s, &ChannelId::Motor { idx: 0 });
    // disarmed idle (1000 µs) sits just below motorOutput's 1042 floor
    assert!(
        lo >= -10.0 && hi <= 100.0,
        "motorOutput 1042..1850: {lo}..{hi}"
    );
    let pid = s.settings.pid.unwrap();
    assert_eq!(
        (pid.pitch.p, pid.pitch.i, pid.pitch.d, pid.pitch.f),
        (13.0, 28.0, 0.0, None)
    );
    let g = &s.settings.gyro_filters;
    let lp = g.lowpass1.as_ref().unwrap();
    assert_eq!(
        (lp.cutoff_hz, lp.kind.clone()),
        (25.0, Some(LowpassKind::Biquad))
    );
    assert_eq!(g.dyn_notch.unwrap().min_hz, 30.0);
    let d = &s.settings.dterm_filters;
    assert_eq!(d.lowpass1.as_ref().unwrap().cutoff_hz, 40.0);
    assert!(d.lowpass2.is_none(), "dterm_lpf2_hz 0");
    assert_eq!(s.settings.loop_rates.gyro_hz, Some(1000.0));
    let log_hz = s.settings.loop_rates.log_hz.unwrap();
    assert!((495.0..505.0).contains(&log_hz), "{log_hz}");
    assert_eq!(s.settings.motor.protocol.as_deref(), Some("PWM"));
}
