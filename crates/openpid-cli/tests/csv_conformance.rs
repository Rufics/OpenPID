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
//! Integer-valued columns (no unit, or `time (us)`) must match exactly;
//! unit-converted columns are skipped. The frame count must match the
//! `.rows` file. Once `blackbox_decode.version` exists, every Blackbox
//! fixture must have a CSV for every flight log our splitter finds, so a
//! missing or extra log is a failure, not a silent skip; the only exception
//! is a fixture listed in `blackbox_decode.skip`, which must have no CSV.
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

#[test]
fn raw_decode_matches_blackbox_decode_csv() {
    let strict = logs_dir().join("blackbox_decode.version").exists();
    let skip = skipped();
    let mut compared = 0usize;
    let mut logs_checked = 0usize;
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
