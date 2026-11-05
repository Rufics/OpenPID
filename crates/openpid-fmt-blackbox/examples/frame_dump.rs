//! Temporary diagnostic: where in the file does the parser lose sync?
#![allow(clippy::unwrap_used, clippy::print_stdout, missing_docs)]
use openpid_fmt_blackbox::headers::Headers;
use openpid_fmt_blackbox::parser::Parser;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let data = std::fs::read(&path).unwrap();
    let headers = Headers::parse(&data).unwrap();
    let body_start = headers.data_start;
    let body = &data[body_start..];
    let mut p = Parser::new(body, &headers);
    let block = 65_536usize;
    let mut blocks: std::collections::BTreeMap<usize, (u64, u64)> =
        std::collections::BTreeMap::new(); // (frames, skipped)
    let mut last_pos = 0usize;
    let mut last_skipped = 0u64;
    let mut last_corrupt = 0u64;
    let mut junk_regions: Vec<(usize, usize)> = Vec::new();
    while let Some(_f) = p.next_frame() {
        let pos = p.position();
        let st = &p.stats;
        let b = blocks.entry(pos / block).or_default();
        b.0 += 1;
        let sk = st.skipped_bytes - last_skipped;
        let co = st.corrupt_total() - last_corrupt;
        b.1 += sk;
        if sk + co > 0 && pos - last_pos > 200 {
            junk_regions.push((body_start + last_pos, body_start + pos));
        }
        last_pos = pos;
        last_skipped = st.skipped_bytes;
        last_corrupt = st.corrupt_total();
    }
    println!("blocks of 64 KiB: frames / skipped bytes");
    for (k, (f, s)) in &blocks {
        if *s > 0 {
            println!(
                "  {:>8}: {f:>6} frames, {s:>6} skipped",
                k * block + body_start
            );
        }
    }
    println!(
        "junk regions (> 200 B between accepted frames): {}",
        junk_regions.len()
    );
    for (a, b) in junk_regions.iter().take(40) {
        println!("  {a}..{b} ({} B)", b - a);
    }
}
