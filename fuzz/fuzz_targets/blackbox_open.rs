//! The whole Blackbox pipeline on arbitrary bytes: sniff, open with the
//! firmware profile, open raw. A panic is the only failure.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openpid_core::{LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;

fuzz_target!(|data: &[u8]| {
    let format = BlackboxFormat::new();
    let head = &data[..data.len().min(4096)];
    let _ = format.sniff(head, None);
    let _ = format.open(data, &OpenOptions::default());
    let _ = format.open(
        data,
        &OpenOptions {
            sessions: None,
            raw: true,
        },
    );
});
