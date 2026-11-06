//! The ArduPilot DataFlash decoder on arbitrary bytes: sniff and open.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openpid_core::{LogFormat, OpenOptions};
use openpid_fmt_ardupilot::ArdupilotFormat;

fuzz_target!(|data: &[u8]| {
    let format = ArdupilotFormat::new();
    let head = &data[..data.len().min(4096)];
    let _ = format.sniff(head, None);
    let _ = format.open(data, &OpenOptions::default());
});
