//! Rotorflight 4.3.0 fixture (`tests/logs/rotorflight-4.3.0-LOG246.TXT`):
//! the governor events must not break the stream, and the helicopter
//! channels must carry physical units (issues #7 and #19).
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::session::EventKind;
use openpid_core::{
    Axis, BatteryField, ChannelId, FirmwareVendor, FirmwareVersion, GyroStage, LogFormat,
    OpenOptions, Unit,
};
use openpid_fmt_blackbox::BlackboxFormat;

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
fn rotorflight_4_3_decodes_cleanly_with_helicopter_units() {
    let data = std::fs::read(common::logs_dir().join("rotorflight-4.3.0-LOG246.TXT")).unwrap();
    let log = BlackboxFormat::new()
        .open(&data, &OpenOptions::default())
        .unwrap();
    let s = &log.sessions[0];
    assert_eq!(s.firmware.vendor, FirmwareVendor::Rotorflight);
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(4, 3, 0)));
    assert_eq!(s.profile_id, "rotorflight");
    // #7: the "corrupt" frames were governor-state events the parser did not know
    assert_eq!(
        s.diagnostics.corrupt_frames, 0,
        "{:?}",
        s.diagnostics.frames
    );
    assert_eq!(s.diagnostics.desync_frames, 0);
    assert!(s.timebase.gaps.is_empty(), "gaps: {:?}", s.timebase.gaps);
    let gov: Vec<i64> = s
        .events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Other { name, data } if name == "gov_state" => Some(data["state"]),
            _ => None,
        })
        .collect();
    assert!(gov.len() >= 5, "governor state changes: {gov:?}");
    assert!(
        s.diagnostics.unmapped_fields.is_empty(),
        "unmapped: {:?}",
        s.diagnostics.unmapped_fields
    );
    // #19: motor output in percent, rotor speeds in rpm, collective and mixer in percent
    let (lo, hi) = range(s, &ChannelId::Motor { idx: 0 });
    assert!(
        lo >= 0.0 && hi > 50.0 && hi <= 100.0,
        "main motor: {lo}..{hi}"
    );
    let (lo, hi) = range(s, &ChannelId::Motor { idx: 1 });
    assert!(
        lo >= 0.0 && hi > 20.0 && hi <= 100.0,
        "tail motor: {lo}..{hi}"
    );
    let head = s.channel(&ChannelId::Custom("headspeed".into())).unwrap();
    assert_eq!(head.unit, Unit::Rpm);
    let (_, hi) = range(s, &head.id);
    assert!(hi > 3000.0 && hi < 4000.0, "headspeed {hi}");
    let tail = s.channel(&ChannelId::Custom("tailspeed".into())).unwrap();
    assert_eq!(tail.unit, Unit::Rpm);
    let (lo, hi) = range(s, &ChannelId::Custom("collective".into()));
    assert!(
        lo >= -100.0 && hi <= 100.0 && hi > 50.0,
        "collective {lo}..{hi}"
    );
    let (lo, hi) = range(s, &ChannelId::Custom("mixer.collective".into()));
    assert!(lo >= -100.0 && hi <= 100.0, "mixer {lo}..{hi}");
    let att = s
        .channel(&ChannelId::Custom("attitude.yaw".into()))
        .unwrap();
    assert_eq!(att.unit, Unit::Degrees);
    let (lo, hi) = range(s, &att.id);
    assert!(lo > 340.0 && hi < 360.0, "{lo}..{hi}");
    let (lo, hi) = range(s, &ChannelId::Battery(BatteryField::Voltage));
    assert!(
        lo > 3.0 && hi < 4.0,
        "Vbat centivolts (1S bench): {lo}..{hi}"
    );
    // throttle comes from rcCommand[4] (0…1000), not from the collective stick rcCommand[3]
    let (lo, hi) = range(s, &ChannelId::Throttle);
    assert!(lo >= 0.0 && hi > 90.0 && hi <= 100.0, "throttle {lo}..{hi}");
    assert!(s.channels.contains(&ChannelId::Gyro {
        axis: Axis::Yaw,
        stage: GyroStage::Filtered
    }));
    assert!(
        s.channels
            .contains(&ChannelId::Setpoint { axis: Axis::Roll })
    );
    let pid = s.settings.pid.unwrap();
    assert_eq!((pid.roll.p, pid.roll.i, pid.yaw.d), (60.0, 120.0, 50.0));
    assert_eq!(s.settings.motor.poles, None);
    assert_eq!(
        s.settings.gyro_filters.lowpass1.as_ref().unwrap().cutoff_hz,
        150.0
    );
    assert!(
        s.settings
            .extra
            .contains_key("gyro_rpm_filter_bank_gear_ratio")
    );
    assert!(
        !s.channels
            .contains(&ChannelId::Custom("setpoint[3]".into()))
    );
}
