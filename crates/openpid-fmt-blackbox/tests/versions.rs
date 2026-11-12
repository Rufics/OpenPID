//! One real log per Betaflight release line must decode cleanly and expose the
//! channels that release introduced. Logs live in `tests/logs/` (Git LFS); a
//! release line listed in `REQUIRED_LINES` must be present, the others are
//! reported when absent (see docs/BLACKBOX-VERSIONS.md for what is still missing).
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::{
    Axis, ChannelId, FirmwareVendor, FirmwareVersion, GyroStage, LogFormat, OpenOptions,
};
use openpid_fmt_blackbox::BlackboxFormat;
use std::collections::BTreeMap;

/// Release lines with a committed fixture: the test fails if one stops decoding.
const REQUIRED_LINES: [(u32, u32); 3] = [(4, 2), (4, 5), (2025, 12)];

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
            "no sample log for: {} (see docs/BLACKBOX-VERSIONS.md)",
            missing.join(", ")
        );
    }
    for line in REQUIRED_LINES {
        assert!(
            seen.contains_key(&line),
            "no decoded session for the committed {}.{} fixture (git lfs pull?)",
            line.0,
            line.1
        );
    }
}

#[test]
fn debug_gyro_scaled_becomes_gyro_raw() {
    use openpid_core::session::Origin;
    use openpid_core::{Axis, ChannelId, GyroStage, Unit};
    // (log, raw gyro present, derived from debug[0..2])
    for (name, present, derived) in [
        ("betaflight-4.2.0.BFL", true, true), // debug_mode 6, no gyroUnfilt field
        ("betaflight-4.5.1-two-sessions.bbl", true, false), // gyroUnfilt logged
        ("betaflight-2025.12.2-btfl_027.bbl", true, false),
        ("betaflight-4.2.11-two-logs.bbl", false, false), // debug_mode 0
    ] {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/logs")
            .join(name);
        let log = BlackboxFormat::new()
            .open(&std::fs::read(&path).unwrap(), &OpenOptions::default())
            .unwrap();
        for s in &log.sessions {
            let raw = s.channel(&ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Raw,
            });
            assert_eq!(raw.is_some(), present, "{name} session {}", s.index);
            let is_derived = raw.is_some_and(|c| matches!(c.origin, Origin::Derived { .. }));
            assert_eq!(is_derived, derived, "{name} session {}", s.index);
            if derived {
                let c = raw.unwrap();
                assert_eq!(c.unit, Unit::DegreesPerSecond);
                assert!(c.name.starts_with("debug[0]"));
                for axis in [Axis::Pitch, Axis::Yaw] {
                    assert!(
                        s.channel(&ChannelId::Gyro {
                            axis,
                            stage: GyroStage::Raw
                        })
                        .is_some()
                    );
                }
            }
        }
    }
}

/// The first session with data of a fixture.
fn first_session(name: &str) -> openpid_core::FlightSession {
    let data = std::fs::read(common::logs_dir().join(name)).unwrap();
    BlackboxFormat::new()
        .open(&data, &OpenOptions::default())
        .unwrap()
        .sessions
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap()
}

/// Every Betaflight fixture is read with the description of its release
/// line, and its own headers bear out what that description says about the
/// line.
#[test]
fn each_fixture_is_read_with_the_description_of_its_release_line() {
    use openpid_fmt_blackbox::releases;
    for (name, line) in [
        ("betaflight-4.2.0.BFL", "4.2"),
        ("betaflight-4.2.8-44-logs.bbl", "4.2"),
        ("betaflight-4.2.11-error-recovery.bbl", "4.2"),
        ("betaflight-4.2.11-two-logs.bbl", "4.2"),
        ("betaflight-4.5.0-gps-rescue.bbl", "4.5"),
        ("betaflight-4.5.1-two-sessions.bbl", "4.5"),
        ("betaflight-2025.12.2-btfl_027.bbl", "2025.12"),
    ] {
        let s = first_session(name);
        let selection = releases::select(&s.firmware);
        let release = selection.release;
        assert_eq!(release.name, line, "{name}");
        assert_eq!(selection.note, None, "{name}: a released line");
        assert!(
            !s.diagnostics.notes.iter().any(|n| n.contains("read as")),
            "{name}: {:?}",
            s.diagnostics.notes
        );
        let has = |header: &str| s.header(header).is_some();
        // the names of the filter settings: 4.3 renamed them
        assert_eq!(has("gyro_lpf1_static_hz"), release.lpf1_names, "{name}");
        assert_eq!(has("gyro_lowpass_hz"), !release.lpf1_names, "{name}");
        // the headers of 4.3, 4.4 and 4.5 are not in a 4.2 log
        assert_eq!(has("dyn_notch_count"), release.since >= (4, 3), "{name}");
        assert_eq!(
            has("blackbox_high_resolution"),
            release.high_resolution,
            "{name}"
        );
        assert_eq!(has("gps_use_3d_speed"), release.since >= (4, 4), "{name}");
        assert_eq!(has("gyro_lpf1_dyn_expo"), release.since >= (4, 5), "{name}");
        // D-min up to 4.5, D-max from 2025.12
        assert_eq!(has("d_max"), release.d_max_scheme, "{name}");
        assert_eq!(has("d_min"), !release.d_max_scheme, "{name}");
        // the battery in centivolts: the voltage at arming is that of a pack
        // of whole cells between 3.3 V and 4.3 V each (`vbatcellvoltage`)
        let volts = s.header("vbatref").unwrap().parse::<f64>().unwrap() * release.vbat_volts;
        assert!(
            (1..=8).any(|cells| (3.3..=4.3).contains(&(volts / f64::from(cells)))),
            "{name}: {volts} V"
        );
        let battery = s
            .channel(&ChannelId::Battery(openpid_core::BatteryField::Voltage))
            .unwrap();
        let mean = battery.data.values().iter().sum::<f64>() / battery.data.len() as f64;
        assert!((mean - volts).abs() < 0.25 * volts, "{name}: {mean} V");
        // the mode boxes are the line's
        assert_eq!(&release.mode_boxes[..4], ["ARM", "ANGLE", "HORIZON", "MAG"]);
    }
}

/// A log of a Betaflight newer than every release line the reader knows is
/// read with the newest description, and its session says so.
#[test]
fn a_newer_betaflight_is_read_as_the_newest_known_line_and_says_so() {
    let name = "betaflight-2025.12.2-btfl_027.bbl";
    let known = first_session(name);
    let data = std::fs::read(common::logs_dir().join(name)).unwrap();
    let old = b"H Firmware revision:Betaflight 2025.12.2 ";
    let at = data.windows(old.len()).position(|w| w == old).unwrap();
    let reopen = |revision: &[u8]| {
        let mut newer = data[..at].to_vec();
        newer.extend_from_slice(revision);
        newer.extend_from_slice(&data[at + old.len()..]);
        BlackboxFormat::new()
            .open(&newer, &OpenOptions::default())
            .unwrap()
            .sessions
            .remove(0)
    };
    let s = reopen(b"H Firmware revision:Betaflight 2027.6.0 ");
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(2027, 6, 0)));
    assert_eq!(s.profile_id, "betaflight");
    assert_eq!(s.len(), known.len(), "the log is read all the same");
    let notes = &s.diagnostics.notes;
    assert!(
        notes.iter().any(|n| n.contains(
            "Betaflight 2027.6.0 is newer than the release lines this reader knows (up to \
             2026.12): it is read as 2026.12"
        )),
        "{notes:?}"
    );
    let gyro = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Filtered,
    };
    assert_eq!(s.channel(&gyro), known.channel(&gyro));
    // read as the newest line: the fifth mode box of 2026.6 on (bit 11) is
    // AUTOPILOT, and the sticks are fractions without a header that says
    // otherwise
    let rc = ChannelId::RcCommand { ch: 0 };
    let (new, old) = (s.channel(&rc).unwrap(), known.channel(&rc).unwrap());
    assert!(
        new.data
            .values()
            .iter()
            .zip(old.data.values())
            .all(|(n, o)| (n - o / 2.0).abs() < 1e-9)
    );
    // a line described from the development branch only says that
    let s = reopen(b"H Firmware revision:Betaflight 2026.12.0 ");
    assert!(
        s.diagnostics
            .notes
            .iter()
            .any(|n| n.contains("described from the development branch")),
        "{:?}",
        s.diagnostics.notes
    );
    // and a released line has nothing to say
    assert!(
        !known
            .diagnostics
            .notes
            .iter()
            .any(|n| n.contains("read as"))
    );
}
