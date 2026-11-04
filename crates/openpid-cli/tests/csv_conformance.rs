//! Compare our raw decode with CSV files produced by the official
//! `blackbox_decode` tool, when such files are present next to the fixtures
//! (`tests/logs/<name>.<session>.csv`, generated with
//! `blackbox_decode --index <session> <log>`). Integer-valued columns must
//! match exactly; unit-converted columns are skipped.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_fmt_csv::read_table;
use std::path::{Path, PathBuf};

fn logs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/logs")
}

#[test]
fn raw_decode_matches_blackbox_decode_csv() {
    let mut compared = 0usize;
    let entries: Vec<PathBuf> = std::fs::read_dir(logs_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    for csv_path in entries
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "csv"))
    {
        let stem = csv_path.file_stem().unwrap().to_str().unwrap();
        let Some((log_name, session)) = stem.rsplit_once('.') else {
            continue;
        };
        let Ok(session) = session.parse::<usize>() else {
            continue;
        };
        let log_path = csv_path.with_file_name(log_name);
        let Ok(data) = std::fs::read(&log_path) else {
            continue;
        };
        let table = read_table(&std::fs::read(csv_path).unwrap()).unwrap();
        let segments = openpid_fmt_blackbox::split_logs(&data);
        let (_, bytes) = segments[session];
        let mine = openpid_fmt_blackbox::decode(bytes).unwrap();
        assert_eq!(
            mine.len(),
            table.rows.len(),
            "{}: frame count",
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
    }
    eprintln!("csv conformance: compared {compared} values");
}
