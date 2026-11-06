//! The CSV reader (`blackbox_decode` output) on arbitrary bytes: sniff and open.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openpid_core::{LogFormat, OpenOptions};
use openpid_fmt_csv::CsvFormat;

fuzz_target!(|data: &[u8]| {
    let format = CsvFormat::new();
    let head = &data[..data.len().min(4096)];
    let _ = format.sniff(head, None);
    let _ = format.open(data, &OpenOptions::default());
});
