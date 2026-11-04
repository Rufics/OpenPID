#![allow(dead_code)]

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

/// Committed fixtures (always present).
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

/// Community logs fetched by tools/fetch-community-logs.sh (optional).
pub fn external_logs() -> Vec<PathBuf> {
    let dir = logs_dir().join("external");
    let mut v: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_log(p))
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

pub fn all_logs() -> Vec<PathBuf> {
    let mut v = committed_logs();
    v.extend(external_logs());
    v
}
