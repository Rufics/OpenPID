//! The Blackbox container and header parser: log boundaries, the split into
//! logs and `Headers::parse` on every segment and on the whole input.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openpid_fmt_blackbox::{Headers, find_log_starts, split_logs};

fuzz_target!(|data: &[u8]| {
    let _ = find_log_starts(data);
    for (_, segment) in split_logs(data) {
        let _ = Headers::parse(segment);
    }
    let _ = Headers::parse(data);
});
