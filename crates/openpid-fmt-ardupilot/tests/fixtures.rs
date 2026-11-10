//! Real `DataFlash` logs (`tests/logs/ardupilot-*.bin`, Git LFS): decoding,
//! channel mapping, settings, and byte-level conformance against row dumps
//! produced by an independent Python decoder (`tests/golden/*.rows.json`).
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::settings::LowpassKind;
use openpid_core::{
    Axis, BatteryField, ChannelId, FirmwareVendor, FirmwareVersion, GyroStage, LogFormat,
    OpenOptions, PidTerm,
};
use openpid_fmt_ardupilot::{ArdupilotFormat, Value, Vehicle, log_starts, read};
use std::path::{Path, PathBuf};

fn logs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/logs")
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

const COPTER: &str = "ardupilot-copter-3.3-dronekit.bin";
const PLANE: &str = "ardupilot-plane-4.3.5.bin";
const QUADPLANE: &str = "ardupilot-quadplane-4.4.4-esc-telemetry.bin";

fn data(name: &str) -> Vec<u8> {
    let p = logs_dir().join(name);
    let d = std::fs::read(&p).unwrap();
    assert!(
        !d.starts_with(b"version https://git-lfs"),
        "{}: Git LFS pointer, run `git lfs pull`",
        p.display()
    );
    d
}

fn open(name: &str) -> openpid_core::FlightSession {
    let log = ArdupilotFormat::new()
        .open(&data(name), &OpenOptions::default())
        .unwrap();
    assert_eq!(log.source.format_id, "ardupilot-dataflash");
    assert_eq!(log.sessions.len(), 1);
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
fn sniffs_dataflash_only() {
    let f = ArdupilotFormat::new();
    assert_eq!(
        f.sniff(&data(COPTER)[..16], None),
        openpid_core::Confidence::HIGH
    );
    assert_eq!(
        f.sniff(b"H Product:Blackbox flight data recorder", Some("x.bbl")),
        openpid_core::Confidence::NONE
    );
    assert_eq!(f.extensions(), &["bin"]);
    assert!(
        f.open(b"not a log at all", &OpenOptions::default())
            .is_err()
    );
}

#[test]
fn copter_3_3_legacy_format() {
    let s = open(COPTER);
    assert_eq!(s.firmware.vendor, FirmwareVendor::Ardupilot);
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(3, 3, 0)));
    assert!(s.firmware.revision.starts_with("APM:Copter V3.3-dev"));
    assert_eq!(s.header("vehicle"), Some("Copter"));
    assert_eq!(s.profile_id, "ardupilot");
    // IMU (50 Hz in this log) is the timebase; the 10 Hz RATE message is held onto it
    let rate = s.timebase.sample_rate_hz();
    assert!((45.0..55.0).contains(&rate), "rate {rate}");
    assert!(s.len() > 4000);
    assert_eq!(s.diagnostics.corrupt_frames, 0);
    assert_eq!(s.diagnostics.skipped_bytes, 0);
    let gyro = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Filtered,
    };
    let (lo, hi) = range(&s, &gyro);
    assert!(
        lo < -50.0 && hi > 50.0 && lo > -1000.0 && hi < 1000.0,
        "gyro deg/s {lo}..{hi}"
    );
    assert_eq!(
        s.channel(&gyro).unwrap().unit,
        openpid_core::Unit::DegreesPerSecond
    );
    // 3.3 logged RATE in centidegrees per second: once scaled it matches the gyro magnitude
    let (lo, hi) = range(&s, &ChannelId::Setpoint { axis: Axis::Roll });
    assert!(
        lo > -200.0 && hi < 200.0 && hi > 20.0,
        "setpoint {lo}..{hi}"
    );
    let held = s
        .channel(&ChannelId::Setpoint { axis: Axis::Roll })
        .unwrap();
    assert!(
        held.data.invalid_count() >= 1,
        "nothing before the first RATE row"
    );
    for idx in 0..4 {
        let (lo, hi) = range(&s, &ChannelId::Motor { idx });
        assert!(
            lo >= 0.0 && hi <= 100.0 && hi > 30.0,
            "motor {idx}: {lo}..{hi}"
        );
    }
    assert!(s.channels.contains(&ChannelId::Throttle));
    let (_, hi) = range(&s, &ChannelId::Throttle);
    assert!(hi > 30.0 && hi <= 100.0);
    let (lo, hi) = range(&s, &ChannelId::Battery(BatteryField::Voltage));
    assert!(
        lo > 14.0 && hi < 17.0,
        "CURR.Volt centivolts → V: {lo}..{hi}"
    );
    let pid = s.settings.pid.unwrap();
    assert_eq!(pid.roll.p, 0.1288);
    assert_eq!(pid.pitch.i, 0.168);
    assert_eq!(pid.yaw.p, 0.66);
    assert_eq!(
        s.settings.gyro_filters.lowpass1.as_ref().unwrap().cutoff_hz,
        20.0
    );
    assert_eq!(
        s.settings.motor.count, None,
        "FRAME_CLASS did not exist in 3.3"
    );
    assert!(s.events.iter().any(|e| matches!(
        &e.kind,
        openpid_core::session::EventKind::Other { name, .. } if name == "MODE"
    )));
    assert!(s.diagnostics.notes[0].starts_with("timebase: IMU"));
}

#[test]
fn plane_4_3_5_pid_messages() {
    let s = open(PLANE);
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(4, 3, 5)));
    assert_eq!(s.header("vehicle"), Some("Plane"));
    assert_eq!(Vehicle::from_firmware(&s.firmware.revision), Vehicle::Plane);
    // units from FMTU/UNIT/MULT: attitude in degrees, battery in volts
    let att = s
        .channel(&ChannelId::Custom("attitude.roll".into()))
        .unwrap();
    assert_eq!(att.unit, openpid_core::Unit::Degrees);
    let (lo, hi) = range(&s, &att.id);
    assert!(lo > -5.0 && hi < 5.0, "bench log: {lo}..{hi}");
    // the plane rate loop is already in deg/s: PIDR.Tar is the setpoint unscaled
    let (lo, hi) = range(&s, &ChannelId::Setpoint { axis: Axis::Roll });
    assert!(lo < -30.0 && hi < -15.0, "{lo}..{hi}");
    assert!(s.channels.contains(&ChannelId::PidTerm {
        axis: Axis::Pitch,
        term: PidTerm::F
    }));
    assert!(
        s.channels
            .contains(&ChannelId::PidError { axis: Axis::Yaw })
    );
    // servos, not motors
    assert!(s.channels.contains(&ChannelId::Custom("servo.1".into())));
    assert!(!s.channels.contains(&ChannelId::Motor { idx: 0 }));
    let pid = s.settings.pid.unwrap();
    assert_eq!(pid.roll.p, 0.04);
    assert_eq!(pid.pitch.i, 0.075);
    assert_eq!(pid.yaw.p, 0.0, "YAW_RATE_ENABLE is 0");
    assert_eq!(
        s.settings
            .dterm_filters
            .lowpass1
            .as_ref()
            .unwrap()
            .cutoff_hz,
        12.0
    );
    assert_eq!(s.settings.loop_rates.pid_hz, Some(50.0));
    assert!(s.settings.gyro_filters.notches.is_empty());
    let generic = s.channel(&ChannelId::Custom("BAT[0].Res".into())).unwrap();
    assert_eq!(generic.unit, openpid_core::Unit::Custom("Ohm".into()));
    assert!(
        s.diagnostics
            .unmapped_fields
            .contains(&"BAT[0].Res".to_string())
    );
}

#[test]
fn quadplane_4_4_4_esc_telemetry_and_notch() {
    let s = open(QUADPLANE);
    assert_eq!(s.firmware.version, Some(FirmwareVersion::new(4, 4, 4)));
    let rate = s.timebase.sample_rate_hz();
    assert!((290.0..310.0).contains(&rate), "IMU at 300 Hz: {rate}");
    for idx in 0..5 {
        let c = s
            .channel(&ChannelId::MotorRpm { idx })
            .unwrap_or_else(|| panic!("ESC instance {idx}"));
        assert_eq!(c.unit, openpid_core::Unit::Custom("rpm".into()));
    }
    let volt = s.channel(&ChannelId::Custom("ESC[1].Volt".into())).unwrap();
    let (lo, hi) = range(&s, &volt.id);
    assert!(
        lo > 28.0 && hi < 29.0,
        "ESC.Volt is e*100 with multiplier B: {lo}..{hi}"
    );
    let chain = &s.settings.gyro_filters;
    assert_eq!(chain.lowpass1.as_ref().unwrap().cutoff_hz, 40.0);
    assert_eq!(
        chain.lowpass1.as_ref().unwrap().kind,
        Some(LowpassKind::Biquad)
    );
    // INS_HNTCH_MODE 1 (throttle based) is described as a static notch at INS_HNTCH_FREQ
    assert_eq!(chain.notches.len(), 1);
    assert_eq!(chain.notches[0].center_hz, 160.0);
    assert_eq!(
        s.settings.extra.get("INS_HNTCH_HMNCS").map(String::as_str),
        Some("3")
    );
    assert_eq!(s.settings.loop_rates.pid_hz, Some(300.0));
    assert_eq!(s.settings.motor.protocol.as_deref(), None);
    assert_eq!(
        s.settings.loop_rates.gyro_hz,
        Some(2000.0),
        "INS_GYRO_RATE 1"
    );
    assert!(
        s.raw_headers
            .iter()
            .any(|(k, v)| k == "MSG" && v.contains("ArduPlane V4.4.4"))
    );
}

#[test]
fn raw_mode_and_session_filter() {
    let f = ArdupilotFormat::new();
    let raw = f
        .open(
            &data(PLANE),
            &OpenOptions {
                sessions: None,
                raw: true,
            },
        )
        .unwrap();
    let s = &raw.sessions[0];
    assert_eq!(s.profile_id, "raw");
    assert!(s.settings.pid.is_none());
    assert!(s.channels.iter().all(|c| matches!(
        c.id,
        ChannelId::Custom(_) | ChannelId::Time | ChannelId::LoopIteration
    )));
    assert!(
        s.channels
            .iter()
            .all(|c| !matches!(c.unit, openpid_core::Unit::Degrees))
    );
    let none = f
        .open(
            &data(PLANE),
            &OpenOptions {
                sessions: Some(vec![1]),
                raw: false,
            },
        )
        .unwrap();
    assert!(none.sessions.is_empty());
}

/// Every value of the first rows of the key messages must match an independent decoder.
#[test]
fn rows_match_the_reference_decoder() {
    for name in [COPTER, PLANE, QUADPLANE] {
        let raw = read(&data(name)).unwrap();
        let golden: serde_json::Value = serde_json::from_slice(
            &std::fs::read(golden_dir().join(format!("{name}.rows.json"))).unwrap(),
        )
        .unwrap();
        for (msg, spec) in golden.as_object().unwrap() {
            let (idx, def) = raw.def(msg).unwrap_or_else(|| panic!("{name}: no {msg}"));
            let columns: Vec<&str> = spec["columns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap())
                .collect();
            assert_eq!(def.columns, columns, "{name} {msg} columns");
            let count = raw.messages.iter().filter(|m| m.def == idx).count();
            assert_eq!(
                count as u64,
                spec["count"].as_u64().unwrap(),
                "{name} {msg} count"
            );
            let ours: Vec<_> = raw
                .messages
                .iter()
                .filter(|m| m.def == idx)
                .take(20)
                .collect();
            for (r, row) in spec["rows"].as_array().unwrap().iter().enumerate() {
                for (c, expected) in row.as_array().unwrap().iter().enumerate() {
                    let got = &ours[r].values[c];
                    let what = format!("{name} {msg} row {r} col {}", columns[c]);
                    match (got, expected) {
                        (Value::Text(s), e) => assert_eq!(e.as_str(), Some(s.as_str()), "{what}"),
                        (Value::Array(_), e) => assert!(e.is_null(), "{what}"),
                        (Value::Int(i), e) => assert_eq!(e.as_i64(), Some(*i), "{what}"),
                        (Value::Float(v), e) => {
                            let e = e.as_f64().unwrap();
                            assert!(
                                (v - e).abs() <= 1e-6 * v.abs().max(1.0),
                                "{what}: {v} vs {e}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn quadplane_batch_sampler_gives_raw_gyro() {
    use openpid_fmt_ardupilot::batch::{BatchKind, batches};
    let raw = read(&data(QUADPLANE)).unwrap();
    let (all, orphans) = batches(&raw);
    assert_eq!(all.len(), 11, "ISBH:11");
    assert_eq!(orphans, 0);
    assert!(all.iter().all(|b| b.samples.len() == 1024));
    // the fixture is cut at 1 MiB: 339 ISBD packets of 11 × 32, the last batch is short
    assert!(all[..10].iter().all(|b| b.missing() == 0));
    assert_eq!(all[10].missing(), (11 * 32 - 339) * 32);
    assert!(all.iter().all(|b| (3270.0..3280.0).contains(&b.rate_hz)));
    // ISBH N=0: type 1 (gyro), instance 0, mul 938; its first ISBD row is x=2, y=-1, z=1
    let first = &all[0];
    assert_eq!(first.kind, BatchKind::Gyro);
    assert_eq!(first.instance, 0);
    let s0 = first.samples[0].unwrap();
    assert!((s0[0] - 2.0 / 938.0).abs() < 1e-12);
    assert!((s0[1] + 1.0 / 938.0).abs() < 1e-12);
    assert!((s0[2] - 1.0 / 938.0).abs() < 1e-12);
    // INS_LOG_BAT_OPT 4 = pre and post filter, alternating; the post-filter
    // copy is logged with instance + IMU count (3) and is visibly smoother
    let hf = |b: &openpid_fmt_ardupilot::batch::Batch| {
        let v: Vec<f64> = b.samples.iter().map(|s| s.unwrap()[0]).collect();
        v.windows(3)
            .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
            .sum::<f64>()
            / v.len() as f64
    };
    let pre = &all[0];
    let post = &all[2];
    assert_eq!((post.kind, post.instance), (BatchKind::Gyro, 3));
    assert!(
        hf(post) < hf(pre) * 0.5,
        "post {} vs pre {}",
        hf(post),
        hf(pre)
    );

    let s = open(QUADPLANE);
    let raw_roll = s
        .channel(&ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Raw,
        })
        .expect("gyro.raw.roll from the batch sampler");
    assert_eq!(raw_roll.unit, openpid_core::Unit::DegreesPerSecond);
    // batches N=4 and N=8 (0.31 s each) fall inside the 8.5 s timebase; N=0 is before it
    let valid = raw_roll.data.valid_count();
    assert!((150..=230).contains(&valid), "valid raw samples: {valid}");
    for axis in [Axis::Pitch, Axis::Yaw] {
        assert!(
            s.channel(&ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw
            })
            .is_some()
        );
    }
    assert!(
        s.channel(&ChannelId::Custom("acc.raw.roll".into()))
            .is_some()
    );
    assert!(
        s.channel(&ChannelId::Custom("isb[0].gyro.post.roll".into()))
            .is_some()
    );
    assert!(
        s.channel(&ChannelId::Custom("isb[0].acc.post.roll".into()))
            .is_some()
    );
    // the unfiltered gyro carries more high-frequency content than IMU.GyrX
    // (after INS_GYRO_FILTER 40 Hz) over the same samples
    let filtered = s
        .channel(&ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Filtered,
        })
        .unwrap();
    let idx: Vec<usize> = (0..s.len())
        .filter(|&i| raw_roll.data.is_valid(i))
        .collect();
    let hf_of = |c: &openpid_core::Channel| {
        idx.windows(3)
            .filter(|w| w[2] - w[0] == 2)
            .map(|w| {
                let v = c.data.values();
                (v[w[0]] - 2.0 * v[w[1]] + v[w[2]]).abs()
            })
            .sum::<f64>()
            / idx.len() as f64
    };
    assert!(
        hf_of(raw_roll) > hf_of(filtered) * 1.5,
        "raw {} vs filtered {}",
        hf_of(raw_roll),
        hf_of(filtered)
    );
    assert!(
        s.diagnostics
            .notes
            .iter()
            .any(|n| n.starts_with("batch sampler: 11 batches at 3276 Hz")),
        "{:?}",
        s.diagnostics.notes
    );
    assert!(
        !s.diagnostics.notes.iter().any(|n| n.contains("ISBH:")),
        "ISBH/ISBD are consumed, not listed as skipped"
    );
}

#[test]
fn concatenated_logs_open_as_separate_sessions() {
    let (a, b) = (data(PLANE), data(QUADPLANE));
    assert_eq!(log_starts(&a), vec![0]);
    let mut joined = a.clone();
    joined.extend_from_slice(&b);
    assert_eq!(log_starts(&joined), vec![0, a.len()]);
    let log = ArdupilotFormat::new()
        .open(&joined, &OpenOptions::default())
        .unwrap();
    assert_eq!(log.sessions.len(), 2);
    let (s0, s1) = (&log.sessions[0], &log.sessions[1]);
    assert_eq!((s0.index, s0.offset_bytes), (0, 0));
    assert_eq!((s1.index, s1.offset_bytes), (1, a.len() as u64));
    assert_eq!(s0.firmware.version, Some(FirmwareVersion::new(4, 3, 5)));
    assert_eq!(s1.firmware.version, Some(FirmwareVersion::new(4, 4, 4)));
    assert_eq!(s0.len(), open(PLANE).len(), "the first log is untouched");
    assert_eq!(
        s1.len(),
        open(QUADPLANE).len(),
        "the second decodes on its own"
    );
    assert_eq!(s0.diagnostics.total_bytes, a.len() as u64);
    // the session filter picks by index
    let second = ArdupilotFormat::new()
        .open(
            &joined,
            &OpenOptions {
                sessions: Some(vec![1]),
                raw: false,
            },
        )
        .unwrap();
    assert_eq!(second.sessions.len(), 1);
    assert_eq!(second.sessions[0].index, 1);
    // a restart that only wrote its header is skipped, not fatal
    let mut header_only = a.clone();
    header_only.extend_from_slice(&b[..89 * 40]);
    let log = ArdupilotFormat::new()
        .open(&header_only, &OpenOptions::default())
        .unwrap();
    assert_eq!(log.sessions.len(), 1);
}

#[test]
fn flight_modes_come_from_the_mode_and_arming_messages() {
    use openpid_core::FlightMode;
    let names = |name: &str| -> Vec<Vec<FlightMode>> {
        open(name)
            .flight_modes
            .into_iter()
            .map(|c| c.modes)
            .collect()
    };
    // Copter 3.3 flew LOITER (mode 5), Plane RTL (11), QuadPlane QRTL (21)
    let copter = names("ardupilot-copter-3.3-dronekit.bin");
    assert!(
        copter.iter().any(|m| m.contains(&FlightMode::Loiter)),
        "{copter:?}"
    );
    let plane = names("ardupilot-plane-4.3.5.bin");
    assert!(
        plane.iter().any(|m| m.contains(&FlightMode::ReturnHome)),
        "{plane:?}"
    );
    let quad = names("ardupilot-quadplane-4.4.4-esc-telemetry.bin");
    assert!(
        quad.iter().any(|m| m.contains(&FlightMode::ReturnHome)),
        "{quad:?}"
    );
    // the plane log records its arming: ARM, then ARM with RTL
    assert_eq!(plane[0], [FlightMode::Arm]);
}

#[test]
fn sticks_come_from_the_rc_inputs() {
    use openpid_core::StickAxis;
    let mut seen = 0;
    for name in [
        "ardupilot-copter-3.3-dronekit.bin",
        "ardupilot-plane-4.3.5.bin",
        "ardupilot-quadplane-4.4.4-esc-telemetry.bin",
    ] {
        let s = open(name);
        if s.channel(&ChannelId::RcCommand { ch: 0 }).is_none() {
            // no RCIN in this log: no sticks either
            assert!(s.sticks_at(1.0).is_none(), "{name}");
            continue;
        }
        // a log whose RC input stays at 0 (no receiver: an autonomous flight)
        // has no stick positions, the others have them from the first pulse on
        let roll = s
            .channel(&ChannelId::Stick {
                axis: StickAxis::Roll,
            })
            .unwrap();
        let has_rc = roll.data.iter().flatten().next().is_some();
        let at_end = s.sticks_at(s.timebase.duration_s());
        assert_eq!(at_end.is_some(), has_rc, "{name}");
        if !has_rc {
            continue;
        }
        seen += 1;
        for axis in StickAxis::ALL {
            let (lo, hi) = range(&s, &ChannelId::Stick { axis });
            let floor = if axis == StickAxis::Throttle {
                0.0
            } else {
                -100.0
            };
            assert!(lo >= floor && hi <= 100.0, "{name} {axis:?}: {lo}..{hi}");
        }
        // the two Plane logs never got a throttle pulse (RCIN.C3 is 0): no
        // throttle stick there, the Copter log has one
        let throttle = s.channel(&ChannelId::RcCommand { ch: 2 }).unwrap();
        let pulsed = throttle.data.iter().flatten().any(|pwm| pwm > 0.0);
        assert_eq!(at_end.unwrap().throttle.is_some(), pulsed, "{name}");
    }
    assert_eq!(seen, 3, "every fixture logs RCIN");
}
