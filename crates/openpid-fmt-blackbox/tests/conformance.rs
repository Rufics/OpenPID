//! Cross-check our decoder against the independent `blackbox-log` crate
//! (a Rust port of the official tools).
//!
//! The reference crate has two known deviations from the firmware (verified
//! against Betaflight's `blackbox.c`): it ignores P-frame sub-sampling when
//! reconstructing `loopIteration` (`skipped = 0; // FIXME`), and after an
//! intra frame it keeps the pre-intra frame as "previous-previous" history,
//! whereas the firmware sets both history slots to the intra frame. The second
//! point changes every `AVERAGE_2`/`STRAIGHT_LINE` predicted field for the
//! rest of the I-frame interval, so those fields are compared on intra frames
//! only. Everything else must match bit for bit.

mod common;

use blackbox_log::frame::Frame as _;
use blackbox_log::prelude::*;
use openpid_fmt_blackbox::{decode, split_logs};
use std::collections::HashMap;

const HISTORY_PREDICTORS: [u8; 3] = [2, 3, 6];

#[test]
fn main_frames_match_reference_decoder() {
    let mut compared_values = 0u64;
    for path in common::all_logs() {
        let data = std::fs::read(&path).unwrap();
        let reference = blackbox_log::File::new(&data);
        let ours = split_logs(&data);
        assert_eq!(
            reference.log_count(),
            ours.len(),
            "{}: log count",
            path.display()
        );
        for (index, (offset, bytes)) in ours.into_iter().enumerate() {
            let name = format!("{} log {index} @ {offset}", path.display());
            let Some(Ok(headers)) = reference.parse(index) else {
                continue; // reference cannot read this firmware; covered by golden tests instead
            };
            let mine = decode(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            let def = mine.headers.main_def();
            let p_def = mine.headers.def(b'P');
            let ref_names: Vec<String> = headers
                .main_frame_def()
                .iter()
                .map(|f| f.name.to_string())
                .collect();
            let my_index: Vec<usize> = ref_names
                .iter()
                .map(|n| {
                    def.index_of(n)
                        .unwrap_or_else(|| panic!("{name}: field {n} missing"))
                })
                .collect();
            let compare_in_p: Vec<bool> = my_index
                .iter()
                .map(|&i| {
                    def.names[i] != "loopIteration"
                        && p_def.is_none_or(|p| !HISTORY_PREDICTORS.contains(&p.predictors[i]))
                })
                .collect();
            let time_idx = def.index_of("time").unwrap();
            let mut by_time: HashMap<i64, usize> = HashMap::new();
            for (row, t) in mine.columns[time_idx].iter().enumerate() {
                by_time.entry(*t).or_insert(row);
            }
            let mut parser = headers.data_parser();
            let (mut ref_frames, mut unmatched) = (0usize, 0usize);
            let mut first_mismatch: Option<String> = None;
            while let Some(event) = parser.next() {
                if let ParserEvent::Main(frame) = event {
                    ref_frames += 1;
                    // Accepted frame sets may differ on corrupted input; align by timestamp.
                    let Some(&row) = by_time.get(&(frame.time_raw() as i64)) else {
                        unmatched += 1;
                        continue;
                    };
                    let intra = mine.intra[row];
                    for (j, value) in frame.iter_raw().enumerate() {
                        if ref_names[j] == "loopIteration" || (!intra && !compare_in_p[j]) {
                            continue;
                        }
                        let ours = mine.columns[my_index[j]][row] as u32;
                        compared_values += 1;
                        if ours != value && first_mismatch.is_none() {
                            first_mismatch = Some(format!(
                                "row {row} ({}) field {}: ours {} vs reference {}",
                                if intra { "I" } else { "P" },
                                ref_names[j],
                                ours as i32,
                                value as i32
                            ));
                        }
                    }
                }
            }
            if let Some(m) = first_mismatch {
                panic!("{name}: {m}");
            }
            if ref_frames >= 100 {
                assert!(
                    unmatched * 100 <= ref_frames,
                    "{name}: {unmatched} of {ref_frames} reference frames not found by time"
                );
                let extra = mine.len().saturating_sub(ref_frames - unmatched);
                assert!(
                    extra * 100 <= mine.len(),
                    "{name}: {extra} of our {} frames unknown to the reference",
                    mine.len()
                );
            }
        }
    }
    assert!(
        compared_values > 100_000,
        "compared only {compared_values} values"
    );
}
