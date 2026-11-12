//! Damage a real log the ways a flight controller and its storage do, and
//! check that everything that is still a frame of the log is read
//! (issue #183, findings B to F). The inputs are made here from the fixtures
//! in `tests/logs/`.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::{LogFormat, OpenOptions};
use openpid_fmt_blackbox::headers::Headers;
use openpid_fmt_blackbox::parser::{Frame, MAX_ITERATION_JUMP, MainKind, Parser};
use openpid_fmt_blackbox::{BlackboxFormat, DecodedLog, decode};

/// A clean 40 s flight: no corrupt frame, no stray byte.
const CLEAN_LOG: &str = "betaflight-2025.12.2-btfl_027.bbl";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(common::logs_dir().join(name)).unwrap()
}

/// Where a main frame of a log lies and what it counts.
#[derive(Debug, Clone, Copy)]
struct MainFrame {
    /// Byte offset of its frame letter.
    start: usize,
    /// Row of the decoded log it becomes.
    row: usize,
    intra: bool,
    iteration: u32,
}

/// The main frames of a log without stray bytes, in order.
fn main_frames(data: &[u8]) -> Vec<MainFrame> {
    let headers = Headers::parse(data).unwrap();
    let mut parser = Parser::new(data, &headers);
    let mut out = Vec::new();
    let mut start = headers.data_start;
    loop {
        let frame = match parser.next_frame() {
            Some(Frame::Main {
                kind, iteration, ..
            }) => Some((kind == MainKind::Intra, iteration)),
            Some(_) => None,
            None => break,
        };
        if let Some((intra, iteration)) = frame {
            out.push(MainFrame {
                start,
                row: out.len(),
                intra,
                iteration,
            });
        }
        start = parser.position();
    }
    assert_eq!(parser.stats.skipped_bytes, 0, "the log has stray bytes");
    assert_eq!(
        parser.stats.corrupt_total(),
        0,
        "the log has corrupt frames"
    );
    out
}

/// The rows `rows` of every main-frame column.
fn rows(log: &DecodedLog, rows: std::ops::Range<usize>) -> Vec<&[i64]> {
    log.columns.iter().map(|c| &c[rows.clone()]).collect()
}

/// Two I frames of the clean log: one a quarter into it, and the first one
/// at least `iterations` loop iterations later.
fn two_intra_frames(frames: &[MainFrame], iterations: u32) -> (MainFrame, MainFrame) {
    let intra: Vec<&MainFrame> = frames.iter().filter(|f| f.intra).collect();
    let a = *intra[intra.len() / 4];
    let b = **intra
        .iter()
        .find(|f| f.iteration >= a.iteration + iterations)
        .unwrap();
    (a, b)
}

/// B: a stretch of frames never reached the storage (it was busy; the
/// firmware drops frames then and writes no "logging resume" event). The
/// gap is longer than the window a decoder accepts the next frame in, and
/// the rest of the log was lost: 30 s of this flight.
#[test]
fn the_log_is_read_on_after_frames_were_lost() {
    let data = fixture(CLEAN_LOG);
    let whole = decode(&data).unwrap();
    let frames = main_frames(&data);
    let (a, b) = two_intra_frames(&frames, 2 * MAX_ITERATION_JUMP);
    let mut cut = data[..a.start].to_vec();
    cut.extend_from_slice(&data[b.start..]);
    let log = decode(&cut).unwrap();
    assert_eq!(log.len(), whole.len() - (b.row - a.row));
    assert!(
        rows(&log, 0..a.row) == rows(&whole, 0..a.row),
        "before the gap"
    );
    assert!(
        rows(&log, a.row..log.len()) == rows(&whole, b.row..whole.len()),
        "after the gap"
    );
    assert_eq!(log.stats.resumed_gaps.len(), 1);
    assert!(log.stats.resumed_gaps[0].iterations > 2 * MAX_ITERATION_JUMP);
    assert_eq!(log.stats.trailing_bytes, 0);
    assert_eq!(log.stats.desync_total(), 0);
    // the session says what happened
    let file = BlackboxFormat::new()
        .open(&cut, &OpenOptions::default())
        .unwrap();
    let notes = &file.sessions[0].diagnostics.notes;
    assert!(
        notes.iter().any(|n| n.contains("goes on after 1 gap")),
        "{notes:?}"
    );
    assert_eq!(file.sessions[0].len(), log.len());
}

/// Pseudo-random bytes, a frame letter every twelfth.
fn junk(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if x.is_multiple_of(12) {
                b"IPSGHE"[(x as usize / 12) % 6]
            } else {
                (x >> 8) as u8
            }
        })
        .collect()
}

/// C: 20 KiB in the middle of the log are not frames: a block of the
/// storage that was never written (zeros) and bytes of something else
/// spliced in. More than 16 KiB without a frame used to end the log there.
#[test]
fn the_log_is_read_on_after_a_hole_in_the_storage() {
    let data = fixture(CLEAN_LOG);
    let whole = decode(&data).unwrap();
    let frames = main_frames(&data);
    let a = two_intra_frames(&frames, 0).0;
    let check = |damaged: &[u8], next: MainFrame, what: &str| {
        let log = decode(damaged).unwrap();
        // the frame before the hole goes with it: no frame follows it
        let kept = a.row - 1;
        assert_eq!(log.len(), kept + whole.len() - next.row, "{what}");
        assert!(
            rows(&log, 0..kept) == rows(&whole, 0..kept),
            "{what}: before the hole"
        );
        assert!(
            rows(&log, kept..log.len()) == rows(&whole, next.row..whole.len()),
            "{what}: after the hole"
        );
        assert_eq!(log.stats.trailing_bytes, 0, "{what}");
        assert_eq!(log.stats.resumed_gaps.len(), 1, "{what}");
        assert!(log.stats.resumed_gaps[0].hole_bytes >= 20 * 1024, "{what}");
        let file = BlackboxFormat::new()
            .open(damaged, &OpenOptions::default())
            .unwrap();
        let notes = &file.sessions[0].diagnostics.notes;
        assert!(
            notes
                .iter()
                .any(|n| n.contains("not blackbox data lie inside the log")),
            "{what}: {notes:?}"
        );
        assert!(
            !notes.iter().any(|n| n.contains("trailing bytes")),
            "{what}: {notes:?}"
        );
    };
    // bytes of something else between two frames: no frame is lost with them
    let mut spliced = data[..a.start].to_vec();
    spliced.extend_from_slice(&junk(20 * 1024));
    spliced.extend_from_slice(&data[a.start..]);
    check(&spliced, a, "junk spliced in");
    // a block of zeros where frames were: they are lost, up to the next I frame
    let end = a.start + 20 * 1024;
    let next = *frames.iter().find(|f| f.intra && f.start >= end).unwrap();
    let mut zeroed = data.clone();
    zeroed[a.start..end].fill(0);
    check(&zeroed, next, "a block of zeros");
}
