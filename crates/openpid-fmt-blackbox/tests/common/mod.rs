#![allow(dead_code, clippy::unwrap_used, reason = "test helpers")]

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
