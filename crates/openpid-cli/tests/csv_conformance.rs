//! Compare our raw decode with the official Betaflight `blackbox_decode`
//! (#49). The reference files are made by `tools/blackbox-decode-csv.sh`:
//!
//! - `tests/logs/<fixture>.<N>.csv`: the header and the first frames of flight
//!   log `N` (counted from 0; `blackbox_decode --index` counts from 1);
//! - `tests/logs/<fixture>.<N>.rows`: how many frames `blackbox_decode` wrote
//!   for that log in total;
//! - `tests/logs/blackbox_decode.version`: the decoder commit.
//!
//! Integer-valued columns (no unit, or `time (us)`) must match exactly;
//! unit-converted columns are skipped. The frame count must match the
//! `.rows` file. Once `blackbox_decode.version` exists, every Blackbox
//! fixture must have a CSV for every flight log our splitter finds, so a
//! missing or extra log is a failure, not a silent skip.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

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

/// The reference CSV of flight log `session` of `fixture`, if present.
fn reference(fixture: &Path, session: usize) -> PathBuf {
    let name = fixture.file_name().unwrap().to_str().unwrap();
    fixture.with_file_name(format!("{name}.{session}.csv"))
}

#[test]
fn raw_decode_matches_blackbox_decode_csv() {
    let strict = logs_dir().join("blackbox_decode.version").exists();
    let mut compared = 0usize;
    let mut logs_checked = 0usize;
    let mut missing = Vec::new();
    for fixture in blackbox_fixtures() {
        let data = std::fs::read(&fixture).unwrap();
        let segments = openpid_fmt_blackbox::split_logs(&data);
        for (session, (_, bytes)) in segments.iter().enumerate() {
            let csv_path = reference(&fixture, session);
            if !csv_path.exists() {
                missing.push(csv_path.file_name().unwrap().to_string_lossy().into_owned());
                continue;
            }
            let table = read_table(&std::fs::read(&csv_path).unwrap()).unwrap();
            let mine = openpid_fmt_blackbox::decode(bytes).unwrap();
            let rows_path = csv_path.with_extension("rows");
            let total = std::fs::read_to_string(&rows_path)
                .map_or(table.rows.len(), |s| s.trim().parse::<usize>().unwrap());
            assert_eq!(
                mine.len(),
                total,
                "{}: frame count, ours vs blackbox_decode",
                csv_path.display()
            );
            assert!(
                table.rows.len() <= mine.len(),
                "{}: more reference rows than frames",
                csv_path.display()
            );
            for (ci, col) in table.columns.iter().enumerate() {
                let exact =
                    col.unit.is_none() || (col.name == "time" && col.unit.as_deref() == Some("us"));
                let Some(ours) = mine.column(&col.name) else {
                    continue;
                };
                if !exact {
                    continue;
                }
                for (row, r) in table.rows.iter().enumerate() {
                    let Some(v) = r[ci] else { continue };
                    assert_eq!(
                        ours[row] as f64,
                        v,
                        "{}: row {row} field {}",
                        csv_path.display(),
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
    eprintln!("csv conformance: {logs_checked} flight logs, {compared} values compared");
    if strict {
        assert!(
            missing.is_empty(),
            "blackbox_decode.version is present but these references are missing: {missing:?}"
        );
        assert!(compared > 0, "no values compared");
    }
}
