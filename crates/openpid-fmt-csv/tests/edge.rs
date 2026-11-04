//! CSV reader edge cases: weak sniffing, raw mode, malformed input.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::{ChannelId, Confidence, FormatError, LogFormat, OpenOptions, Unit};
use openpid_fmt_csv::{CsvFormat, read_table};

#[test]
fn sniff_levels_and_errors() {
    let f = CsvFormat::new();
    assert_eq!(f.id(), "blackbox-csv");
    assert!(f.extensions().contains(&"csv"));
    assert_eq!(f.sniff(b"time,foo\n1,2\n", None), Confidence::LOW);
    assert_eq!(f.sniff(b"hello world", None), Confidence::NONE);
    assert_eq!(f.sniff(b"loopIteration,time\n", None), Confidence::HIGH);
    assert!(matches!(read_table(b""), Err(FormatError::Unrecognized)));
    assert!(
        matches!(read_table(b"a,b\n1,2\n"), Err(FormatError::Unrecognized)),
        "no time column"
    );
    assert!(matches!(
        f.open(b"nothing", &OpenOptions::default()),
        Err(FormatError::Unrecognized)
    ));
}

#[test]
fn raw_mode_and_blank_lines() {
    let f = CsvFormat::new();
    let data = b"loopIteration,time,gyroADC[0]\n0,0,1\n\n8,1000,2\nbroken\n";
    let log = f
        .open(
            data,
            &OpenOptions {
                sessions: None,
                raw: true,
            },
        )
        .unwrap();
    let s = &log.sessions[0];
    assert_eq!(s.profile_id, "raw");
    assert_eq!(s.len(), 2);
    let g = s.channel(&ChannelId::Custom("gyroADC[0]".into())).unwrap();
    assert_eq!(g.unit, Unit::Raw);
    assert_eq!(g.data.values(), &[1.0, 2.0]);
    assert!(s.diagnostics.notes[0].contains("1 malformed"));
    let t = read_table(data).unwrap();
    assert_eq!(t.bad_rows, 1);
    assert!(t.column("nope").is_none());
}
