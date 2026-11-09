//! Compare our raw decode with the official Betaflight `blackbox_decode`
//! (#49). The reference files are made by `tools/blackbox-decode-csv.sh`:
//!
//! - `tests/logs/<fixture>.<N>.csv`: the header and the first frames of flight
//!   log `N` (counted from 0; `blackbox_decode --index` counts from 1);
//! - `tests/logs/<fixture>.<N>.rows`: how many frames `blackbox_decode` wrote
//!   for that log in total;
//! - `tests/logs/blackbox_decode.version`: the decoder commit;
//! - `tests/logs/blackbox_decode.skip`: fixtures the official decoder cannot
//!   process (it hangs or writes nothing), `<fixture>\t<reason>` per line.
//!
//! The official decoder lags behind the format the firmware writes in three
//! known ways (checked in its `src/parser.c`, see `tests/logs/README.md`);
//! the test models them on top of our decode instead of hiding them:
//!
//! 1. an event it does not parse (anything but sync beep, in-flight
//!    adjustment, logging resume and log end: e.g. flight mode, disarm, the
//!    Rotorflight events) leaves its payload unread, so the stream is invalid
//!    and the P frames until the next I frame are dropped
//!    (betaflight/blackbox-tools#73);
//! 2. `P interval` is read only as `num/denom`; the Betaflight form `P
//!    interval:N` leaves 1/1, so `loopIteration` of P frames is wrong there
//!    and is compared on I frames only;
//! 3. a frame cut by the end of the file is accepted (the "next byte starts a
//!    frame" check looks at the frame's own marker), so a log that ends
//!    without "End of log" may have one more frame.
//!
//! Integer-valued columns (no unit, or `time (us)`) must match exactly on
//! every reference row; unit-converted columns are skipped. Once
//! `blackbox_decode.version` exists, every Blackbox fixture must have a CSV
//! for every flight log our splitter finds, so a missing or extra log is a
//! failure, not a silent skip; the only exception is a fixture listed in
//! `blackbox_decode.skip`, which must have no CSV.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::session::EventKind;
use openpid_fmt_blackbox::DecodedLog;
use openpid_fmt_csv::read_table;
use std::path::{Path, PathBuf};

fn logs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/logs")
}

/// Blackbox fixtures (`.bbl`, `.bfl`, `.txt` in any case).
fn blackbox_fixtures() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(logs_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "bbl" | "bfl" | "txt"))
        })
        .collect();
    v.sort();
    v
}

/// Fixtures the official decoder cannot process, with the reason.
fn skipped() -> Vec<(String, String)> {
    std::fs::read_to_string(logs_dir().join("blackbox_decode.skip"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (name, reason) = l.split_once('\t').unwrap_or((l, ""));
            (name.trim().to_string(), reason.trim().to_string())
        })
        .collect()
}

/// The reference CSV of flight log `session` of `fixture`, if present.
fn reference(fixture: &Path, session: usize) -> PathBuf {
    let name = fixture.file_name().unwrap().to_str().unwrap();
    fixture.with_file_name(format!("{name}.{session}.csv"))
}

/// Whether `blackbox_decode` parses this event (its `parseEventFrame`).
fn official_parses(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::SyncBeep { .. }
            | EventKind::InflightAdjustment { .. }
            | EventKind::LoggingResume { .. }
            | EventKind::LogEnd
    )
}

/// Our frames as `blackbox_decode` sees them: the indices it keeps (deviation 1).
fn official_frames(mine: &DecodedLog) -> Vec<usize> {
    let mut dropped = vec![false; mine.len()];
    for e in mine.events.iter().filter(|e| !official_parses(&e.kind)) {
        let mut i = e.sample_index;
        while i < mine.len() && !mine.intra[i] {
            dropped[i] = true;
            i += 1;
        }
    }
    (0..mine.len()).filter(|&i| !dropped[i]).collect()
}

#[test]
fn raw_decode_matches_blackbox_decode_csv() {
    let strict = logs_dir().join("blackbox_decode.version").exists();
    let skip = skipped();
    let mut compared = 0usize;
    let mut logs_checked = 0usize;
    let mut dropped_total = 0usize;
    let mut missing = Vec::new();
    for fixture in blackbox_fixtures() {
        let name = fixture.file_name().unwrap().to_str().unwrap().to_string();
        if let Some((_, reason)) = skip.iter().find(|(n, _)| *n == name) {
            assert!(
                !reference(&fixture, 0).exists(),
                "{name} is listed in blackbox_decode.skip ({reason}) but has a reference CSV"
            );
            eprintln!("csv conformance: {name} skipped: {reason}");
            continue;
        }
        let data = std::fs::read(&fixture).unwrap();
        let segments = openpid_fmt_blackbox::split_logs(&data);
        for (session, (_, bytes)) in segments.iter().enumerate() {
            let csv_path = reference(&fixture, session);
            let shown = csv_path.file_name().unwrap().to_string_lossy().into_owned();
            if !csv_path.exists() {
                missing.push(shown);
                continue;
            }
            let table = read_table(&std::fs::read(&csv_path).unwrap()).unwrap();
            let mine = openpid_fmt_blackbox::decode(bytes).unwrap();
            let kept = official_frames(&mine);
            dropped_total += mine.len() - kept.len();
            let total = std::fs::read_to_string(csv_path.with_extension("rows"))
                .map_or(table.rows.len(), |s| s.trim().parse::<usize>().unwrap());
            // deviation 3: one extra frame only where the log is cut
            let cut = !mine.events.iter().any(|e| e.kind == EventKind::LogEnd);
            let expected = if cut && total == kept.len() + 1 {
                kept.len() + 1
            } else {
                kept.len()
            };
            assert_eq!(
                total,
                expected,
                "{shown}: frame count of blackbox_decode vs ours ({} decoded, {} after the \
                 frames it drops after unparsed events, log cut: {cut})",
                mine.len(),
                kept.len()
            );
            assert!(
                table.rows.len() <= total,
                "{shown}: more reference rows than its .rows file says"
            );
            // deviation 2
            let p_interval_fraction = mine
                .headers
                .raw
                .get("P interval")
                .is_some_and(|v| v.contains('/'));
            for (ci, col) in table.columns.iter().enumerate() {
                let exact =
                    col.unit.is_none() || (col.name == "time" && col.unit.as_deref() == Some("us"));
                let Some(ours) = mine.column(&col.name) else {
                    continue;
                };
                if !exact {
                    continue;
                }
                let iteration = col.name == "loopIteration";
                for (row, r) in table.rows.iter().enumerate() {
                    let Some(v) = r[ci] else { continue };
                    let Some(&frame) = kept.get(row) else {
                        // only the accepted cut frame of deviation 3 has no counterpart
                        assert!(
                            cut && row == kept.len(),
                            "{shown}: row {row} has no frame of ours"
                        );
                        continue;
                    };
                    if iteration && !p_interval_fraction && !mine.intra[frame] {
                        continue;
                    }
                    assert_eq!(
                        ours[frame] as f64, v,
                        "{shown}: row {row} (our frame {frame}) field {}",
                        col.name
                    );
                    compared += 1;
                }
            }
            logs_checked += 1;
        }
        // a reference for a log our splitter does not see is a disagreement too
        let extra = reference(&fixture, segments.len());
        assert!(
            !extra.exists(),
            "{}: blackbox_decode found more flight logs than our splitter ({})",
            extra.display(),
            segments.len()
        );
    }
    eprintln!(
        "csv conformance: {logs_checked} flight logs, {compared} values compared, \
         {dropped_total} frames dropped by blackbox_decode after events it does not parse"
    );
    if strict {
        assert!(
            missing.is_empty(),
            "blackbox_decode.version is present but these references are missing: {missing:?}"
        );
        assert!(compared > 0, "no values compared");
    }
}
