//! Compare our raw decode with the Blackbox Explorer parser, the reference
//! for the decoder's regression tests (ADR-0005). The reference files
//! `tests/logs/explorer/<fixture>.json` are made by
//! `tools/blackbox-explorer/run.sh`: Betaflight's Explorer for every fixture
//! except the Rotorflight ones, which go through Rotorflight's fork.
//!
//! Every flight log is compared in full: the main frames field by field
//! (FNV-1a 64 digests per chunk of rows, so a difference is located to a field
//! and a range of frames), the frame kinds, the slow, GPS and GPS-home frames
//! with the main frame they follow, and every event with its payload. Every
//! Blackbox fixture must have a reference, with as many flight logs as our
//! splitter finds.
//!
//! One case is allowed to differ: when the header block is cut in the middle of
//! a line (`emuflight-3.7.0-truncated.BFL`), Explorer reads that line on to the next
//! newline inside the data and loses the frames there. Our frames from
//! Explorer's first one on must then match it exactly, and the slow frames and
//! events before that point move to its first frame.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::session::AuxRow;
use openpid_core::session::{AdjustmentValue, EventKind};
use openpid_fmt_blackbox::DecodedLog;
use serde_json::{Value, json};
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

/// FNV-1a 64 of every value as a little-endian i64, the digest
/// `tools/blackbox-explorer/reference.mjs` writes.
fn fnv(values: impl Iterator<Item = i64>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in values {
        for b in v.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Digests of `column` per chunk of `chunk` values.
fn digests(column: &[i64], chunk: usize) -> Vec<String> {
    column
        .chunks(chunk)
        .map(|c| fnv(c.iter().copied()))
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

/// Explorer's event number and payload for one of ours.
fn explorer_event(kind: &EventKind) -> (i64, Value) {
    match kind {
        EventKind::SyncBeep { time_us } => (0, json!({ "time": time_us })),
        // Explorer scales the value for display; the function id is compared
        EventKind::InflightAdjustment { function, value } => {
            let raw = match value {
                AdjustmentValue::Int(_) => 0,
                AdjustmentValue::Float(_) => 128,
            };
            (13, json!({ "func": i64::from(*function) + raw }))
        }
        EventKind::LoggingResume { iteration, time_us } => (
            14,
            json!({ "logIteration": iteration, "currentTime": time_us }),
        ),
        EventKind::Disarm { reason } => (15, json!({ "reason": reason })),
        EventKind::FlightMode { flags, last_flags } => {
            (30, json!({ "newFlags": flags, "lastFlags": last_flags }))
        }
        EventKind::LogEnd => (255, json!({})),
        EventKind::Other { name, data } => {
            let state = data.get("state").copied().unwrap_or_default();
            match name.as_str() {
                "gov_state" => (50, json!({ "govState": state })),
                "rescue_state" => (51, json!({ "rescueState": state })),
                "airborne_state" => (52, json!({ "airborneState": state })),
                "custom_data" => (100, json!(null)),
                "custom_string" => (101, json!(null)),
                other => panic!("event {other} has no Explorer counterpart"),
            }
        }
        EventKind::Unknown { code } => (i64::from(*code), json!(null)),
    }
}

/// The part of an Explorer event payload we compare with `ours`.
fn comparable(theirs: &Value, ours: &Value) -> Value {
    match ours {
        Value::Object(keys) => Value::Object(
            keys.keys()
                .map(|k| (k.clone(), theirs.get(k).cloned().unwrap_or(Value::Null)))
                .collect(),
        ),
        _ => Value::Null,
    }
}

/// Compare one auxiliary table (slow, GPS, GPS home), its rows following
/// main frame `at - offset`.
fn check_table(what: &str, reference: &Value, ours: &[AuxRow], offset: usize, chunk: usize) {
    let rows = usize::try_from(reference["rows"].as_u64().unwrap()).unwrap();
    assert_eq!(ours.len(), rows, "{what}: row count, ours vs Explorer");
    let fields = strings(&reference["fields"]);
    let theirs = &reference["digests"];
    let at: Vec<i64> = ours
        .iter()
        .map(|r| (r.sample_index.max(offset) - offset) as i64)
        .collect();
    assert_eq!(
        digests(&at, chunk),
        strings(&theirs["(at)"]),
        "{what}: main frame each row follows"
    );
    for (c, field) in fields.iter().enumerate() {
        let column: Vec<i64> = ours.iter().map(|r| r.values[c]).collect();
        assert_eq!(
            digests(&column, chunk),
            strings(&theirs[field]),
            "{what}: field {field}"
        );
    }
}

/// Compare one flight log; returns the number of main-frame values compared.
fn check_log(
    shown: &str,
    bytes: &[u8],
    mine: &DecodedLog,
    reference: &Value,
    chunk: usize,
) -> usize {
    assert!(
        reference["error"].is_null(),
        "{shown}: Explorer failed: {}",
        reference["error"]
    );
    let frames = usize::try_from(reference["frames"].as_u64().unwrap()).unwrap();
    // a header line cut by the data (we stop the header before it) costs
    // Explorer the frames up to the next newline
    let header_cut = bytes[mine.headers.data_start..].starts_with(b"H ");
    let offset = mine.len().saturating_sub(frames);
    if offset > 0 {
        assert!(
            header_cut,
            "{shown}: Explorer has {frames} frames, ours {}",
            mine.len()
        );
        let time = mine.column("time").unwrap();
        assert_eq!(
            Some(time[offset]),
            reference["first_time"].as_i64(),
            "{shown}: Explorer's first frame is not our frame {offset}"
        );
    }
    assert_eq!(
        mine.len() - offset,
        frames,
        "{shown}: frame count, ours vs Explorer"
    );

    let fields = strings(&reference["fields"]);
    assert_eq!(fields, mine.field_names(), "{shown}: main-frame fields");
    let theirs = &reference["digests"];
    let intra: Vec<i64> = mine.intra[offset..].iter().map(|&i| i64::from(i)).collect();
    assert_eq!(
        digests(&intra, chunk),
        strings(&theirs["(intra)"]),
        "{shown}: frame kinds (I/P)"
    );
    for (c, field) in fields.iter().enumerate() {
        let ours = digests(&mine.columns[c][offset..], chunk);
        let expected = strings(&theirs[field]);
        if let Some(k) = ours.iter().zip(&expected).position(|(a, b)| a != b) {
            panic!(
                "{shown}: field {field} differs in frames {}..{} (our frames from {offset} on); \
                 `node tools/blackbox-explorer/betaflight.mjs ... --csv <dir>` writes Explorer's frames",
                k * chunk,
                ((k + 1) * chunk).min(frames)
            );
        }
        assert_eq!(
            ours.len(),
            expected.len(),
            "{shown}: field {field}, chunk count"
        );
    }

    check_table(
        &format!("{shown}: slow"),
        &reference["slow"],
        &mine.slow.rows,
        offset,
        chunk,
    );
    let none = Vec::new();
    let gps = mine.gps.as_ref().map_or(&none, |t| &t.rows);
    check_table(
        &format!("{shown}: GPS"),
        &reference["gps"],
        gps,
        offset,
        chunk,
    );
    let home = mine.gps_home.as_ref().map_or(&none, |t| &t.rows);
    check_table(
        &format!("{shown}: GPS home"),
        &reference["gps_home"],
        home,
        offset,
        chunk,
    );

    let events = reference["events"].as_array().unwrap();
    assert_eq!(
        mine.events.len(),
        events.len(),
        "{shown}: event count, ours vs Explorer"
    );
    for (i, (e, t)) in mine.events.iter().zip(events).enumerate() {
        let (code, payload) = explorer_event(&e.kind);
        let at = e.sample_index.max(offset) - offset;
        assert_eq!(
            (at as u64, code),
            (t["at"].as_u64().unwrap(), t["event"].as_i64().unwrap()),
            "{shown}: event {i} (frame, number), ours vs Explorer"
        );
        assert_eq!(
            payload,
            comparable(&t["data"], &payload),
            "{shown}: event {i} payload"
        );
    }
    frames * (fields.len() + 1)
}

#[test]
fn raw_decode_matches_blackbox_explorer() {
    let dir = logs_dir().join("explorer");
    let mut compared = 0usize;
    let mut logs = 0usize;
    for fixture in blackbox_fixtures() {
        let name = fixture.file_name().unwrap().to_str().unwrap().to_string();
        let path = dir.join(format!("{name}.json"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            panic!("{name}: no Explorer reference, run tools/blackbox-explorer/run.sh");
        };
        let reference: Value = serde_json::from_str(&text).unwrap();
        let chunk = usize::try_from(reference["chunk"].as_u64().unwrap()).unwrap();
        let theirs = reference["logs"].as_array().unwrap();
        let data = std::fs::read(&fixture).unwrap();
        let segments = openpid_fmt_blackbox::split_logs(&data);
        assert_eq!(
            segments.len(),
            theirs.len(),
            "{name}: flight logs, ours vs Explorer"
        );
        for (session, ((_, bytes), log)) in segments.iter().zip(theirs).enumerate() {
            let mine = openpid_fmt_blackbox::decode(bytes).unwrap();
            compared += check_log(&format!("{name}.{session}"), bytes, &mine, log, chunk);
            logs += 1;
        }
    }
    eprintln!("explorer conformance: {logs} flight logs, {compared} main-frame values compared");
    assert!(compared > 0, "no values compared");
}
