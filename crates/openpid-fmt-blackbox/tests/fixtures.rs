//! Decode every sample log: sanity checks, golden summaries and robustness
//! against corrupted input.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::summary::SessionSummary;
use openpid_core::{ChannelId, LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;

#[test]
fn every_fixture_decodes_sanely() {
    let format = BlackboxFormat::new();
    for path in common::all_logs() {
        let data = std::fs::read(&path).unwrap();
        let log = format
            .open(&data, &OpenOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!log.sessions.is_empty(), "{}", path.display());
        let decoded: Vec<_> = log.sessions.iter().filter(|s| !s.is_empty()).collect();
        assert!(
            !decoded.is_empty(),
            "{}: no session with data",
            path.display()
        );
        for s in decoded {
            let name = format!("{} session {}", path.display(), s.index);
            assert!(
                s.channels.contains(&ChannelId::Time),
                "{name}: no time channel"
            );
            assert!(s.channels.contains(&ChannelId::LoopIteration), "{name}");
            // time must be monotonic (validated by the parser)
            assert!(
                s.timebase.time_us.windows(2).all(|w| w[1] >= w[0]),
                "{name}: time not monotonic"
            );
            let it = &s.channel(&ChannelId::LoopIteration).unwrap().data;
            assert!(
                it.values().windows(2).all(|w| w[1] > w[0]),
                "{name}: iteration not increasing"
            );
            // corruption must stay rare on these real logs
            let total = s
                .diagnostics
                .frames
                .values()
                .map(|f| f.valid)
                .sum::<u64>()
                .max(1);
            // the error-recovery fixture is tiny and corrupted on purpose: judge real flights only
            assert!(
                total < 1000 || s.diagnostics.corrupt_frames * 100 < total,
                "{name}: {} corrupt of {total} frames",
                s.diagnostics.corrupt_frames
            );
            assert!(
                total < 1000 || s.timebase.sample_rate_hz() > 100.0,
                "{name}: rate {}",
                s.timebase.sample_rate_hz()
            );
            // every Cleanflight-family log has a filtered gyro in deg/s
            let g = s
                .channel(&ChannelId::Gyro {
                    axis: openpid_core::Axis::Roll,
                    stage: openpid_core::GyroStage::Filtered,
                })
                .unwrap_or_else(|| panic!("{name}: no gyro"));
            assert_eq!(g.unit, openpid_core::Unit::DegreesPerSecond);
            assert_eq!(g.data.len(), s.len());
        }
    }
}

#[test]
fn golden_summaries() {
    let format = BlackboxFormat::new();
    let update = std::env::var_os("OPENPID_UPDATE_GOLDEN").is_some();
    let mut mismatches = Vec::new();
    for path in common::committed_logs() {
        let data = std::fs::read(&path).unwrap();
        let log = format.open(&data, &OpenOptions::default()).unwrap();
        let summaries: Vec<SessionSummary> = log.sessions.iter().map(SessionSummary::of).collect();
        let actual = serde_json::to_value(&summaries).unwrap();
        let golden = common::golden_dir().join(format!(
            "{}.json",
            path.file_name().unwrap().to_str().unwrap()
        ));
        if update {
            std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
            std::fs::write(
                &golden,
                serde_json::to_string_pretty(&actual).unwrap() + "\n",
            )
            .unwrap();
            continue;
        }
        let expected: serde_json::Value = if let Ok(bytes) = std::fs::read(&golden) {
            serde_json::from_slice(&bytes).unwrap()
        } else {
            mismatches.push(format!(
                "{}: missing golden file (run with OPENPID_UPDATE_GOLDEN=1)",
                golden.display()
            ));
            continue;
        };
        if expected != actual {
            mismatches.push(format!(
                "{}: summary differs from golden file",
                path.display()
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// Deterministic xorshift so the test is reproducible without dependencies.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn corrupted_input_never_panics() {
    let format = BlackboxFormat::new();
    let path = common::logs_dir().join("betaflight-4.2.11-two-logs.bbl");
    let original = std::fs::read(path).unwrap();
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    for round in 0..40 {
        let mut data = original.clone();
        match round % 4 {
            0 => {
                // flip random bytes in the frame data
                for _ in 0..64 {
                    let i = (rng.next() as usize) % data.len();
                    data[i] ^= (rng.next() as u8) | 1;
                }
            }
            1 => {
                // truncate anywhere
                let n = (rng.next() as usize) % data.len();
                data.truncate(n);
            }
            2 => {
                // damage the header block
                for _ in 0..8 {
                    let i = (rng.next() as usize) % 4000;
                    data[i] = (rng.next() as u8) & 0x7F;
                }
            }
            _ => {
                // delete a random chunk
                let a = (rng.next() as usize) % data.len();
                let b = (a + (rng.next() as usize) % 5000).min(data.len());
                data.drain(a..b);
            }
        }
        // Any outcome is fine except a panic.
        let _ = format.open(&data, &OpenOptions::default());
    }
    // Pure garbage and tiny inputs.
    let _ = format.open(&[], &OpenOptions::default());
    let _ = format.open(b"H ", &OpenOptions::default());
    let _ = format.open(b"H Data version:2\n", &OpenOptions::default());
    let garbage: Vec<u8> = (0..100_000).map(|_| rng.next() as u8).collect();
    let _ = format.open(&garbage, &OpenOptions::default());
}

#[test]
fn raw_mode_keeps_logged_names() {
    let format = BlackboxFormat::new();
    let data = std::fs::read(common::logs_dir().join("betaflight-4.2.0.BFL")).unwrap();
    let log = format
        .open(
            &data,
            &OpenOptions {
                sessions: None,
                raw: true,
            },
        )
        .unwrap();
    let s = &log.sessions[0];
    assert_eq!(s.profile_id, "raw");
    assert!(s.channels.contains(&ChannelId::Custom("gyroADC[0]".into())));
    assert!(!s.channels.contains(&ChannelId::Throttle));
    let log = format
        .open(
            &data,
            &OpenOptions {
                sessions: Some(vec![5]),
                raw: false,
            },
        )
        .unwrap();
    assert_eq!(log.sessions.len(), 0);
}

/// A wrong history rotation after intra frames would show up as a jump in
/// every `AVERAGE_2`-predicted field (gyro, motors) on the first P frame after
/// each I frame. Check that steps across I->P boundaries look like any other
/// step.
#[test]
fn no_discontinuity_at_intra_frame_boundaries() {
    for path in common::all_logs() {
        let data = std::fs::read(&path).unwrap();
        for (offset, bytes) in openpid_fmt_blackbox::split_logs(&data) {
            let Ok(log) = openpid_fmt_blackbox::decode(bytes) else {
                continue;
            };
            if log.len() < 1000 {
                continue;
            }
            let name = format!("{} @ {offset}", path.display());
            for field in ["gyroADC[0]", "gyroADC[1]", "gyroADC[2]", "motor[0]"] {
                let Some(col) = log.column(field) else {
                    continue;
                };
                let (mut boundary, mut nb, mut other, mut no) = (0.0f64, 0u64, 0.0f64, 0u64);
                for i in 1..col.len() {
                    let step = (col[i] - col[i - 1]).abs() as f64;
                    if log.intra[i - 1] {
                        boundary += step;
                        nb += 1;
                    } else {
                        other += step;
                        no += 1;
                    }
                }
                if nb < 20 || no < 100 {
                    continue;
                }
                let (b, o) = (boundary / nb as f64, other / no as f64);
                assert!(
                    b <= 3.0 * o + 2.0,
                    "{name}: {field} mean step after I frames {b:.2} vs elsewhere {o:.2}"
                );
            }
        }
    }
}

#[test]
fn flight_modes_are_decoded_per_firmware() {
    use openpid_core::FlightMode::{self, Angle, Arm, PosHold};
    let format = BlackboxFormat::new();
    let modes = |name: &str| -> Vec<Vec<FlightMode>> {
        let path = common::all_logs()
            .into_iter()
            .find(|p| p.file_name().is_some_and(|n| n == name))
            .unwrap();
        let log = format
            .open(&std::fs::read(path).unwrap(), &OpenOptions::default())
            .unwrap();
        log.sessions[0]
            .flight_modes
            .iter()
            .map(|c| c.modes.clone())
            .collect()
    };
    // Betaflight 2025.12: armed in ANGLE, then POSHOLD (bit 9), then disarmed
    assert_eq!(
        modes("betaflight-2025.12.2-btfl_027.bbl"),
        [vec![Arm, Angle], vec![Arm, PosHold], vec![PosHold]]
    );
    // INAV numbers its boxes its own way
    assert_eq!(modes("inav-9.0.1-truncated.TXT")[0], [Arm, Angle]);
    // Rotorflight 4.3 has a list of its own too
    assert_eq!(
        modes("rotorflight-4.3.0-LOG246.TXT")[0],
        [Arm, FlightMode::Other("BLACKBOX".into())]
    );
    // EmuFlight's boxes are not named anywhere: ARM, the others numbered
    assert_eq!(
        modes("emuflight-3.7.0-truncated.BFL")[0],
        [Arm, FlightMode::Other("bit 21".into())]
    );
}

#[test]
fn sticks_are_mapped_per_firmware() {
    use openpid_core::StickAxis;
    let format = BlackboxFormat::new();
    let open = |name: &str| {
        let path = common::all_logs()
            .into_iter()
            .find(|p| p.file_name().is_some_and(|n| n == name))
            .unwrap();
        format
            .open(&std::fs::read(path).unwrap(), &OpenOptions::default())
            .unwrap()
            .sessions
            .remove(0)
    };
    let stick = |s: &openpid_core::FlightSession, axis: StickAxis, i: usize| {
        s.channel(&ChannelId::Stick { axis }).unwrap().data.values()[i]
    };
    let rc = |s: &openpid_core::FlightSession, ch: u8, i: usize| {
        s.channel(&ChannelId::RcCommand { ch })
            .unwrap()
            .data
            .values()[i]
    };
    // Betaflight: ±500 is full deflection; rcCommand yaw is stored inverted
    // (its Blackbox Explorer negates it to draw the stick)
    let s = open("betaflight-2025.12.2-btfl_027.bbl");
    let yaw = s.channel(&ChannelId::RcCommand { ch: 2 }).unwrap();
    let i = (0..s.len())
        .max_by(|a, b| {
            yaw.data.values()[*a]
                .abs()
                .total_cmp(&yaw.data.values()[*b].abs())
        })
        .unwrap();
    assert!(rc(&s, 2, i).abs() > 100.0, "a yaw input in this flight");
    assert!((stick(&s, StickAxis::Yaw, i) + rc(&s, 2, i) / 5.0).abs() < 1e-9);
    assert!((stick(&s, StickAxis::Roll, i) - rc(&s, 0, i) / 5.0).abs() < 1e-9);
    assert!((stick(&s, StickAxis::Pitch, i) - rc(&s, 1, i) / 5.0).abs() < 1e-9);
    assert!((stick(&s, StickAxis::Throttle, i) - (rc(&s, 3, i) - 1000.0) / 10.0).abs() < 1e-9);
    let st = s.sticks_at(s.time_s(i).unwrap()).unwrap();
    assert!((st.yaw - stick(&s, StickAxis::Yaw, i) / 100.0).abs() < 1e-9);
    // Rotorflight: the vertical stick is the collective, rcCommand[3] ±500
    let s = open("rotorflight-4.3.0-LOG246.TXT");
    let i = s.len() / 2;
    assert!((stick(&s, StickAxis::Throttle, i) - (rc(&s, 3, i) + 500.0) / 10.0).abs() < 1e-9);
    // INAV logs rcCommand as Betaflight does
    let s = open("inav-9.0.1-truncated.TXT");
    assert!(s.sticks_at(0.0).is_some());
}

/// The exporter and the slider model of a log come from its firmware
/// profile, found through the format registry: Betaflight has both, the
/// other firmwares of the family have their own setting names and get none.
#[test]
fn exporter_and_sliders_come_from_the_logs_profile() {
    let mut registry = openpid_core::FormatRegistry::new();
    registry.register(Box::new(BlackboxFormat::new()));
    let first = |name: &str| {
        let data = std::fs::read(common::logs_dir().join(name)).unwrap();
        let log = registry
            .open(&data, Some(name), &OpenOptions::default())
            .unwrap();
        log.sessions.into_iter().find(|s| !s.is_empty()).unwrap()
    };
    let new = first("betaflight-4.5.1-two-sessions.bbl");
    assert_eq!(registry.profile_for(&new).unwrap().id(), "betaflight");
    let exporter = registry.exporter(&new).unwrap();
    assert_eq!(
        (exporter.id(), exporter.label()),
        ("betaflight-cli", "Betaflight CLI")
    );
    let lpf = openpid_core::Proposal {
        path: "gyro_filters.lpf1.cutoff_hz".into(),
        from: None,
        to: "300".into(),
        rationale: String::new(),
        confidence: 1.0,
    };
    let text = exporter.export(std::slice::from_ref(&lpf)).unwrap();
    assert!(text.contains("set gyro_lpf1_static_hz = 300\n"), "{text}");
    assert_eq!(
        registry.slider_model(&new).unwrap().cli_name(),
        "Betaflight CLI"
    );
    // before 4.3 the same setting had another name, and there were no sliders
    let old = first("betaflight-4.2.0.BFL");
    let text = registry.exporter(&old).unwrap().export(&[lpf]).unwrap();
    assert!(text.contains("set gyro_lowpass_hz = 300\n"), "{text}");
    assert!(registry.slider_model(&old).is_none());
    for name in [
        "inav-9.0.1-truncated.TXT",
        "rotorflight-4.3.0-LOG246.TXT",
        "emuflight-3.7.0-truncated.BFL",
    ] {
        let s = first(name);
        assert!(registry.profile_for(&s).is_some(), "{name}");
        assert!(registry.exporter(&s).is_none(), "{name}");
        assert!(registry.slider_model(&s).is_none(), "{name}");
    }
}
