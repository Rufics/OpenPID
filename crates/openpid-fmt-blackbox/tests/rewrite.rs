//! Real logs written back: a flight log is decoded into the canonical
//! session and the session is written for the release that logged it. The
//! result must be the log: the same header lines with the same values in
//! the same order, and after them the same bytes, frame for frame.
//!
//! This is the check of the writer that is not our own reader: every byte
//! of the frames was put there by a flight controller.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::{FlightSession, LogFormat, OpenOptions};
use openpid_fmt_blackbox::parser::Parser;
use openpid_fmt_blackbox::write::{Target, WriteOptions, betaflight};
use openpid_fmt_blackbox::{BlackboxFormat, Headers, decode, split_logs};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(common::logs_dir().join(name)).unwrap()
}

fn session(log: &[u8]) -> FlightSession {
    let mut file = BlackboxFormat::new()
        .open(log, &OpenOptions::default())
        .unwrap();
    assert_eq!(file.sessions.len(), 1);
    file.sessions.remove(0)
}

fn written(session: &FlightSession, target: Target) -> Vec<u8> {
    let options = WriteOptions {
        target,
        ..WriteOptions::default()
    };
    betaflight(session, &options).unwrap()
}

/// The frames of a log, each with its letter, as the decoder delimits them.
fn frames(log: &[u8]) -> Vec<&[u8]> {
    let headers = Headers::parse(log).unwrap();
    let mut parser = Parser::new(log, &headers);
    let mut out = Vec::new();
    let mut start = headers.data_start;
    while parser.next_frame().is_some() {
        let end = parser.position();
        out.push(&log[start..end]);
        start = end;
    }
    out
}

/// Asserts that `mine` is `original` up to the padding of the storage.
fn assert_same_log(name: &str, original: &[u8], mine: &[u8]) {
    let (theirs, ours) = (
        Headers::parse(original).unwrap(),
        Headers::parse(mine).unwrap(),
    );
    assert_eq!(theirs.raw.0.len(), ours.raw.0.len(), "{name}: header lines");
    for (a, b) in theirs.raw.0.iter().zip(&ours.raw.0) {
        assert_eq!(a, b, "{name}: header line");
    }
    assert_eq!(
        &original[..theirs.data_start],
        &mine[..ours.data_start],
        "{name}: the header block"
    );
    let (theirs, ours) = (&original[theirs.data_start..], &mine[ours.data_start..]);
    if let Some(at) = theirs.iter().zip(ours).position(|(a, b)| a != b) {
        panic!("{name}: the frames differ at byte {at} of {}", ours.len());
    }
    assert!(
        ours.len() <= theirs.len(),
        "{name}: more frames than the log"
    );
    // what follows the last frame is the erased flash between two logs
    let rest = &theirs[ours.len()..];
    assert!(
        rest.iter().all(|b| *b == 0xff),
        "{name}: {} bytes of the log were not written",
        rest.len()
    );
}

/// Betaflight 2025.12.2 (40 s at 1 kHz with GPS frames, a wing build) and
/// both sessions of the 4.5.1 log (eRPM, unfiltered gyro, a debug mode):
/// release builds, clean logs. Every header line and every byte of the
/// frames comes back.
#[test]
fn logs_of_release_builds_are_written_back_byte_for_byte() {
    let mut frames_checked = 0;
    for (file, target, logs) in [
        ("betaflight-2025.12.2-btfl_027.bbl", Target::V2025_12, 1),
        ("betaflight-4.5.1-two-sessions.bbl", Target::V4_5, 2),
    ] {
        let data = fixture(file);
        let segments = split_logs(&data);
        assert_eq!(segments.len(), logs, "{file}");
        for (i, (_, original)) in segments.into_iter().enumerate() {
            let name = format!("{file}, log {i}");
            let session = session(original);
            let d = &session.diagnostics;
            assert_eq!(
                (d.corrupt_frames, d.desync_frames, d.skipped_bytes),
                (0, 0, 0),
                "{name}: the fixture is a clean log"
            );
            let mine = written(&session, target);
            assert_same_log(&name, original, &mine);
            frames_checked += frames(&mine).len();
        }
    }
    // I, P, S, G, H and E frames of three flights
    assert!(frames_checked > 60_000, "{frames_checked}");
}

/// The kinds of frame the fixtures exercise, so the test above cannot pass
/// on main frames alone.
#[test]
fn the_fixtures_have_every_kind_of_frame() {
    let data = fixture("betaflight-2025.12.2-btfl_027.bbl");
    let session = session(&data);
    let mine = written(&session, Target::V2025_12);
    let count = |letter: u8| frames(&mine).iter().filter(|f| f[0] == letter).count();
    let counts = (*b"IPSGHE").map(count);
    assert_eq!(counts, [1234, 38_247, 7, 398, 1, 5]);
}

/// `betaflight-4.5.0-gps-rescue.bbl` calls itself 4.5.0 but was built from
/// the development branch before that release (revision 3aabaf365): it has
/// no `ez_landing_speed` header, and it logs the GPS altitude as an unsigned
/// number, which the release (tag `4.5.0`, `blackboxGpsGFields`) logs
/// signed. The writer writes the release's format. So this log comes back
/// with exactly those two differences and no other: the definition of
/// `GPS_altitude`, and the altitude bytes of the `G` frames, which decode
/// to the same values.
#[test]
fn a_build_from_before_the_release_differs_only_where_its_format_did() {
    let original = fixture("betaflight-4.5.0-gps-rescue.bbl");
    let session = session(&original);
    assert_eq!(session.diagnostics.corrupt_frames, 0);
    let mine = written(&session, Target::V4_5);

    let (theirs, ours) = (
        Headers::parse(&original).unwrap(),
        Headers::parse(&mine).unwrap(),
    );
    let mut different = Vec::new();
    assert_eq!(theirs.raw.0.len(), ours.raw.0.len());
    for (a, b) in theirs.raw.0.iter().zip(&ours.raw.0) {
        assert_eq!(a.0, b.0, "the same lines in the same order");
        if a.1 != b.1 {
            different.push((a.0.as_str(), a.1.as_str(), b.1.as_str()));
        }
    }
    assert_eq!(
        different,
        [
            ("Field G signed", "0,0,1,1,0,0,0", "0,0,1,1,1,0,0"),
            ("Field G encoding", "1,1,0,0,1,1,1", "1,1,0,0,0,1,1"),
        ]
    );
    assert!(session.header("ez_landing_speed").is_none());

    let (theirs, ours) = (frames(&original), frames(&mine));
    assert_eq!(theirs.len(), ours.len(), "frame for frame");
    let mut fixes = 0;
    for (a, b) in theirs.iter().zip(&ours) {
        assert_eq!(a[0], b[0], "the same kind of frame");
        if a[0] == b'G' {
            fixes += 1;
        } else {
            assert_eq!(a, b, "every frame but the fixes, byte for byte");
        }
    }
    assert!(fixes > 5, "{fixes} fixes");
    // and the fixes hold the same values
    let (theirs, ours) = (decode(&original).unwrap(), decode(&mine).unwrap());
    assert_eq!(theirs.gps.unwrap().rows, ours.gps.unwrap().rows);
    assert_eq!(theirs.gps_home.unwrap().rows, ours.gps_home.unwrap().rows);
    assert_eq!(theirs.columns, ours.columns);
}

/// A log written for another release than its own is a log of that
/// release: the craft and the flight are kept, the format is the target's.
#[test]
fn a_log_is_carried_to_every_other_release() {
    let data = fixture("betaflight-4.5.1-two-sessions.bbl");
    let (_, first) = split_logs(&data)[0];
    let original = session(first);
    for target in Target::ALL {
        let mine = written(&original, target);
        let back = session(&mine);
        let name = target.to_string();
        let d = &back.diagnostics;
        assert_eq!(
            (d.corrupt_frames, d.desync_frames, d.skipped_bytes),
            (0, 0, 0),
            "{name}"
        );
        assert_eq!(back.len(), original.len(), "{name}");
        let (version, wanted) = (back.firmware.version.unwrap(), target.version());
        assert_eq!(
            (version.major, version.minor),
            (wanted.major, wanted.minor),
            "{name}: {}",
            back.firmware.revision
        );
        // the craft: its gains, its loop rates, its name
        assert_eq!(back.header("rollPID"), original.header("rollPID"), "{name}");
        assert_eq!(back.settings.loop_rates, original.settings.loop_rates);
        assert_eq!(back.settings.craft_name, original.settings.craft_name);
        // the flight: every channel both releases log
        let mut compared = 0;
        for channel in original.channels.iter() {
            let Some(other) = back.channel(&channel.id) else {
                continue;
            };
            let sticks = matches!(
                channel.id,
                openpid_core::ChannelId::RcCommand { .. } | openpid_core::ChannelId::Stick { .. }
            );
            if sticks && target == Target::Master {
                // other units on master: see the round trips of `write.rs`
                continue;
            }
            assert_eq!(
                channel.data.values().len(),
                other.data.values().len(),
                "{name}: {:?}",
                channel.id
            );
            for (i, (a, b)) in channel
                .data
                .values()
                .iter()
                .zip(other.data.values())
                .enumerate()
            {
                assert!(
                    (a - b).abs() < 1e-9,
                    "{name}: {:?} sample {i}: {a} became {b}",
                    channel.id
                );
            }
            compared += 1;
        }
        assert!(compared >= 30, "{name}: {compared} channels");
        assert_eq!(back.events, original.events, "{name}");
        assert_eq!(back.flight_modes, original.flight_modes, "{name}");
    }
}

/// Betaflight 4.2: the 47 logs of the three clean fixtures, among them one
/// with GPS frame definitions and a debug mode.
#[test]
fn the_logs_of_4_2_are_written_back_byte_for_byte() {
    let mut logs = 0;
    for (file, expected) in [
        ("betaflight-4.2.0.BFL", 1),
        ("betaflight-4.2.8-44-logs.bbl", 44),
        ("betaflight-4.2.11-two-logs.bbl", 2),
    ] {
        let data = fixture(file);
        let segments = split_logs(&data);
        assert_eq!(segments.len(), expected, "{file}");
        for (i, (_, original)) in segments.into_iter().enumerate() {
            let name = format!("{file}, log {i}");
            let session = session(original);
            let d = &session.diagnostics;
            let mine = written(&session, Target::V4_2);
            if (file, i) == ("betaflight-4.2.8-44-logs.bbl", 43) {
                // the last log of the dump ends after its headers: what
                // follows them (536 bytes) is not a frame. It is written
                // back as it was read: the headers, and no frame.
                assert_eq!((session.len(), d.skipped_bytes), (0, 536), "{name}");
                let (theirs, ours) = (
                    Headers::parse(original).unwrap(),
                    Headers::parse(&mine).unwrap(),
                );
                assert_eq!(&original[..theirs.data_start], &mine[..], "{name}");
                assert_eq!(ours.data_start, mine.len(), "{name}");
                continue;
            }
            assert_eq!(
                (d.corrupt_frames, d.desync_frames, d.skipped_bytes),
                (0, 0, 0),
                "{name}: the fixture is a clean log"
            );
            assert_same_log(&name, original, &mine);
            logs += 1;
        }
    }
    assert_eq!(logs, 46);
}

/// A log that is not Betaflight's (or older than 4.2) has no release of its
/// own among the targets: it is written as a flight of the craft for the
/// release asked for.
#[test]
fn a_log_of_another_firmware_is_written_for_the_release_asked_for() {
    let data = fixture("emuflight-3.7.0-truncated.BFL");
    let (_, first) = split_logs(&data)[0];
    let original = session(first);
    assert_eq!(Target::from_name("3.7"), None);
    let back = session(&written(&original, Target::V4_3));
    assert_eq!(back.len(), original.len());
    assert_eq!(back.diagnostics.corrupt_frames, 0);
    assert!(back.firmware.is_at_least(4, 3) && !back.firmware.is_at_least(4, 4));
    for axis in openpid_core::Axis::ALL {
        let id = openpid_core::ChannelId::Gyro {
            axis,
            stage: openpid_core::GyroStage::Filtered,
        };
        assert_eq!(
            back.channel(&id).unwrap().data,
            original.channel(&id).unwrap().data
        );
    }
}
