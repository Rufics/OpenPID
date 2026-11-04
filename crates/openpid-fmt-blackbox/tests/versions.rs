//! One real log per Betaflight release line must decode cleanly and expose the
//! channels that release introduced. Logs live in `tests/logs/` (committed) and
//! `tests/logs/external/` (fetched by `tools/fetch-community-logs.sh`); a
//! release whose log is absent is reported, not failed, except for the
//! committed 4.2 fixture.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::{
    Axis, ChannelId, FirmwareVendor, FirmwareVersion, GyroStage, LogFormat, OpenOptions,
};
use openpid_fmt_blackbox::BlackboxFormat;
use std::collections::BTreeMap;

/// Release lines we track, as (major, minor).
const RELEASE_LINES: [(u32, u32); 7] = [
    (4, 2),
    (4, 3),
    (4, 4),
    (4, 5),
    (2025, 12),
    (2026, 6),
    (2026, 12),
];

#[test]
fn one_log_per_betaflight_release_line() {
    let format = BlackboxFormat::new();
    let mut seen: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
    for path in common::all_logs() {
        let data = std::fs::read(&path).unwrap();
        let Ok(log) = format.open(&data, &OpenOptions::default()) else {
            continue;
        };
        for s in log.sessions.iter().filter(|s| s.len() >= 50) {
            if s.firmware.vendor != FirmwareVendor::Betaflight {
                continue;
            }
            let Some(v) = s.firmware.version else {
                continue;
            };
            let name = format!(
                "{} session {} ({})",
                path.file_name().unwrap().to_string_lossy(),
                s.index,
                v
            );
            assert_eq!(s.profile_id, "betaflight", "{name}");
            assert_eq!(s.diagnostics.corrupt_frames, 0, "{name}: corrupt frames");
            assert_eq!(s.diagnostics.skipped_bytes, 0, "{name}: skipped bytes");

            let gyro = ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Filtered,
            };
            assert!(s.channels.contains(&gyro), "{name}: no filtered gyro");
            assert!(
                s.channels
                    .contains(&ChannelId::Setpoint { axis: Axis::Roll }),
                "{name}"
            );
            assert!(s.channels.contains(&ChannelId::Motor { idx: 0 }), "{name}");
            assert!(
                s.channels.contains(&ChannelId::Throttle),
                "{name}: no derived throttle"
            );
            assert!(s.settings.pid.is_some(), "{name}: PID gains not parsed");
            assert!(
                s.settings.loop_rates.log_hz.is_some(),
                "{name}: log rate unknown"
            );
            let fields: Vec<&str> = s
                .raw_headers
                .iter()
                .find(|(k, _)| k == "Field I name")
                .map(|(_, v)| v.split(',').collect())
                .unwrap_or_default();

            // 4.5+: gyroUnfilt and eRPM exist as fields (unless disabled by the pilot) and must map.
            if v >= FirmwareVersion::new(4, 5, 0) {
                if fields.contains(&"gyroUnfilt[0]") {
                    assert!(
                        s.channels.contains(&ChannelId::Gyro {
                            axis: Axis::Roll,
                            stage: GyroStage::Raw
                        }),
                        "{name}: gyroUnfilt unmapped"
                    );
                }
                if fields.contains(&"eRPM[0]") {
                    assert!(
                        s.channels.contains(&ChannelId::Erpm { idx: 0 }),
                        "{name}: eRPM unmapped"
                    );
                    if s.settings.motor.poles.is_some() {
                        assert!(
                            s.channels.contains(&ChannelId::MotorRpm { idx: 0 }),
                            "{name}: motor RPM not derived"
                        );
                    }
                }
            }
            // 2025.12+: year-based versions, d_max instead of d_min, imuQuaternion field.
            if v >= FirmwareVersion::new(2025, 12, 0) {
                assert!(s.header("d_max").is_some(), "{name}: d_max header missing");
                if fields.contains(&"imuQuaternion[0]") {
                    assert!(
                        s.channels
                            .contains(&ChannelId::Custom("imuQuaternion[0]".into())),
                        "{name}"
                    );
                }
            }
            // Fields we do not know yet must be rare and must still be present as raw channels.
            for f in &s.diagnostics.unmapped_fields {
                assert!(
                    s.channels.contains(&ChannelId::Custom(f.clone())),
                    "{name}: unmapped field {f} lost"
                );
            }
            assert!(
                s.diagnostics.unmapped_fields.len() <= 3,
                "{name}: unmapped {:?}",
                s.diagnostics.unmapped_fields
            );
            seen.entry((v.major, v.minor)).or_default().push(name);
        }
    }
    let covered: Vec<String> = seen
        .iter()
        .map(|(k, v)| format!("{}.{} ({} sessions)", k.0, k.1, v.len()))
        .collect();
    eprintln!("Betaflight release lines covered: {}", covered.join(", "));
    let missing: Vec<String> = RELEASE_LINES
        .iter()
        .filter(|k| !seen.contains_key(k))
        .map(|k| format!("{}.{}", k.0, k.1))
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "no sample log for: {} (run tools/fetch-community-logs.sh or add one)",
            missing.join(", ")
        );
    }
    assert!(
        seen.contains_key(&(4, 2)),
        "the committed 4.2 fixtures must always be present"
    );
}
