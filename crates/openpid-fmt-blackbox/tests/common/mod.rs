#![allow(dead_code, clippy::unwrap_used, reason = "test helpers")]

use blackbox_log::frame::Frame as _;
use blackbox_log::prelude::*;
use std::path::{Path, PathBuf};

pub fn logs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/logs")
}

pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

fn is_log(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("bbl" | "bfl" | "txt")
    )
}

/// Fixtures in `tests/logs/` (Git LFS; `git lfs pull` if they are pointer files).
pub fn committed_logs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(logs_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_log(p))
        .collect();
    v.sort();
    assert!(!v.is_empty(), "no fixtures in {}", logs_dir().display());
    v
}

/// Every log fixture (all of them are committed, in Git LFS).
pub fn all_logs() -> Vec<PathBuf> {
    committed_logs()
}

/// What the independent decoder read of a log.
pub struct ReferenceRead {
    /// Main frames.
    pub frames: usize,
    /// Field values that were compared with ours.
    pub values: u64,
    /// GPS frames.
    pub fixes: usize,
}

/// Reads the log `bytes` with our decoder and with the independent one
/// (ADR-0005) and asserts that every main frame has the same value in every
/// field. `name` says which log it is when they differ.
pub fn assert_reference_decoder_agrees(bytes: &[u8], name: &str) -> ReferenceRead {
    let mine = openpid_fmt_blackbox::decode(bytes).unwrap();
    let def = mine.headers.main_def();
    let inter = mine.headers.def(b'P').unwrap();

    let reference = blackbox_log::File::new(bytes);
    assert_eq!(reference.log_count(), 1, "{name}");
    let headers = reference.parse(0).unwrap().unwrap();
    let names: Vec<String> = headers
        .main_frame_def()
        .iter()
        .map(|f| f.name.to_string())
        .collect();
    // the reference lists every field but the time, which it keeps apart
    let index: Vec<usize> = names.iter().map(|n| def.index_of(n).unwrap()).collect();
    assert_eq!(names.len(), def.len() - 1, "{name}: the same fields");
    let time = def.index_of("time").unwrap();

    let mut parser = headers.data_parser();
    let mut read = ReferenceRead {
        frames: 0,
        values: 0,
        fixes: 0,
    };
    while let Some(event) = parser.next() {
        let frame = match event {
            ParserEvent::Main(frame) => frame,
            ParserEvent::Gps(_) => {
                read.fixes += 1;
                continue;
            }
            _ => continue,
        };
        let row = read.frames;
        assert_eq!(
            mine.columns[time][row],
            frame.time_raw() as i64,
            "{name}: row {row}"
        );
        for (j, value) in frame.iter_raw().enumerate() {
            // the reference keeps the wrong history after an I frame and
            // ignores the sub-sampling (see `conformance.rs`): fields
            // predicted from two frames are compared on I frames only
            let from_history = [2, 3, 6].contains(&inter.predictors[index[j]]);
            if names[j] == "loopIteration" || (!mine.intra[row] && from_history) {
                continue;
            }
            assert_eq!(
                mine.columns[index[j]][row] as u32, value,
                "{name}: row {row}, field {}",
                names[j]
            );
            read.values += 1;
        }
        read.frames += 1;
    }
    read
}
