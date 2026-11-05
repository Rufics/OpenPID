//! Mutation fuzzing of the Blackbox decoder and header parser (ADR-0013).
//!
//! `cargo fuzz` needs a nightly toolchain and the C++ libFuzzer runtime, which
//! the project's pure-Rust rule for dev-dependencies rules out. This harness
//! does the same job without either: every iteration takes a fixture segment
//! from the corpus, applies one to four structure-aware mutations (bit flips,
//! chunk deletion / duplication / splicing, frame-marker injection, header
//! line edits with interesting numbers, multi-log concatenation) and feeds
//! the result to every public entry point. A panic is the only failure.
//!
//! Every iteration is seeded independently from `(seed, iteration)`, so a
//! crash reported as `OPENPID_FUZZ_REPLAY=<seed>:<iteration>` is reproduced
//! in one step; the input is also written to `target/tmp/fuzz-crashes/`.
//!
//! Knobs (environment): `OPENPID_FUZZ_SEED`, `OPENPID_FUZZ_ITERS`,
//! `OPENPID_FUZZ_SECONDS` (time budget; overrides the iteration count),
//! `OPENPID_FUZZ_REPLAY=<seed>:<iteration>`.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use openpid_core::{LogFormat, OpenOptions};
use openpid_fmt_blackbox::{BlackboxFormat, Headers, find_log_starts, split_logs};

/// Longest slice of a fixture segment kept in the corpus: the headers plus a
/// few hundred frames are where the parsing logic lives, and short inputs
/// keep iterations fast.
const SEGMENT_LIMIT: usize = 24 * 1024;
/// Segments taken from one fixture (an SD-card dump holds dozens of near copies).
const SEGMENTS_PER_FIXTURE: usize = 6;
/// Iterations of the always-on test (a few seconds in a debug build).
const QUICK_ITERS: u64 = 1000;
/// Seed of the always-on test; the weekly job varies it.
const DEFAULT_SEED: u64 = 0x5EED_0B1A_CB0F_0001;

/// Numbers that trip range checks, conversions and allocation sizes.
const INTERESTING: &[&str] = &[
    "",
    "0",
    "1",
    "-1",
    "2",
    "7",
    "8",
    "31",
    "32",
    "63",
    "64",
    "127",
    "128",
    "255",
    "256",
    "32767",
    "32768",
    "65535",
    "65536",
    "2147483647",
    "2147483648",
    "-2147483648",
    "-2147483649",
    "4294967295",
    "4294967296",
    "9223372036854775807",
    "18446744073709551615",
    "18446744073709551616",
    "1e9",
    "0x10",
    "NaN",
    "inf",
    "-0",
    "1.5",
    "a",
];

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n` (`0` when `n == 0`).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}

/// Fixture segments the mutations start from.
fn corpus() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for path in common::committed_logs() {
        let data = std::fs::read(&path).unwrap();
        for (_, seg) in split_logs(&data).into_iter().take(SEGMENTS_PER_FIXTURE) {
            out.push(seg[..seg.len().min(SEGMENT_LIMIT)].to_vec());
        }
    }
    assert!(!out.is_empty(), "empty corpus");
    out
}

/// Byte range of the header block (`H ` lines) at the start of `data`.
fn header_block(data: &[u8]) -> usize {
    let mut pos = 0;
    while data.len() >= pos + 2 && data[pos] == b'H' && data[pos + 1] == b' ' {
        match data[pos..].iter().position(|b| *b == b'\n') {
            Some(nl) => pos += nl + 1,
            None => return data.len(),
        }
    }
    pos
}

/// Start offsets of the header lines.
fn header_lines(data: &[u8]) -> Vec<(usize, usize)> {
    let end = header_block(data);
    let mut lines = Vec::new();
    let mut start = 0;
    while start < end {
        let nl = data[start..end]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(end, |n| start + n + 1);
        lines.push((start, nl));
        start = nl;
    }
    lines
}

/// Runs of ASCII digits (with an optional leading `-`) inside `line`.
fn numbers(line: &[u8]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if line[i].is_ascii_digit() {
            let start = if i > 0 && line[i - 1] == b'-' {
                i - 1
            } else {
                i
            };
            let mut end = i;
            while end < line.len() && line[end].is_ascii_digit() {
                end += 1;
            }
            runs.push((start, end));
            i = end;
        } else {
            i += 1;
        }
    }
    runs
}

/// Edit one header line: drop or duplicate it, swap it with another, replace
/// one of its numbers with an interesting value, or add / remove a
/// comma-separated item.
fn mutate_header(rng: &mut Rng, data: &mut Vec<u8>, desc: &mut String) {
    let lines = header_lines(data);
    if lines.is_empty() {
        desc.push_str(" header:none");
        return;
    }
    let (start, end) = lines[rng.below(lines.len())];
    match rng.below(6) {
        0 => {
            data.drain(start..end);
            desc.push_str(" header:drop");
        }
        1 => {
            let line = data[start..end].to_vec();
            data.splice(start..start, line);
            desc.push_str(" header:dup");
        }
        2 => {
            let (s2, e2) = lines[rng.below(lines.len())];
            if s2 != start {
                let (a, b) = if s2 < start {
                    ((s2, e2), (start, end))
                } else {
                    ((start, end), (s2, e2))
                };
                let first = data[a.0..a.1].to_vec();
                let second = data[b.0..b.1].to_vec();
                data.splice(b.0..b.1, first);
                data.splice(a.0..a.1, second);
            }
            desc.push_str(" header:swap");
        }
        3 | 4 => {
            let runs = numbers(&data[start..end]);
            if runs.is_empty() {
                desc.push_str(" header:nonum");
                return;
            }
            let (a, b) = runs[rng.below(runs.len())];
            let value = if rng.below(4) == 0 {
                format!("{}", rng.next() >> rng.below(64))
            } else {
                INTERESTING[rng.below(INTERESTING.len())].to_string()
            };
            data.splice(start + a..start + b, value.bytes());
            desc.push_str(" header:num");
        }
        _ => {
            let commas: Vec<usize> = data[start..end]
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b',')
                .map(|(i, _)| start + i)
                .collect();
            if commas.is_empty() {
                desc.push_str(" header:nocomma");
                return;
            }
            let c = commas[rng.below(commas.len())];
            if rng.below(2) == 0 {
                // drop the item after the comma
                let item_end = data[c + 1..end]
                    .iter()
                    .position(|b| *b == b',' || *b == b'\n')
                    .map_or(end, |n| c + 1 + n);
                data.drain(c..item_end);
                desc.push_str(" header:item-");
            } else {
                let item = format!(",{}", INTERESTING[rng.below(INTERESTING.len())]);
                data.splice(c..c, item.bytes());
                desc.push_str(" header:item+");
            }
        }
    }
}

/// Apply one random mutation to `data`.
fn mutate_once(rng: &mut Rng, corpus: &[Vec<u8>], data: &mut Vec<u8>, desc: &mut String) {
    let len = data.len();
    match rng.below(10) {
        0 => {
            let k = 1 + rng.below(64);
            for _ in 0..k {
                if len > 0 {
                    let i = rng.below(len);
                    data[i] ^= 1 << rng.below(8);
                }
            }
            let _ = write!(desc, " flip{k}");
        }
        1 => {
            let k = 1 + rng.below(32);
            for _ in 0..k {
                if len > 0 {
                    let i = rng.below(len);
                    data[i] = rng.byte();
                }
            }
            let _ = write!(desc, " set{k}");
        }
        2 => {
            data.truncate(rng.below(len + 1));
            desc.push_str(" trunc");
        }
        3 => {
            if len > 0 {
                let a = rng.below(len);
                let n = rng.below((len - a).min(4096) + 1);
                data.drain(a..a + n);
            }
            desc.push_str(" del");
        }
        4 => {
            if len > 0 {
                let a = rng.below(len);
                let n = rng.below((len - a).min(2048) + 1);
                let chunk = data[a..a + n].to_vec();
                let b = rng.below(len + 1);
                data.splice(b..b, chunk);
            }
            desc.push_str(" dup");
        }
        5 => {
            let n = 1 + rng.below(256);
            let bytes: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
            let i = rng.below(len + 1);
            data.splice(i..i, bytes);
            let _ = write!(desc, " ins{n}");
        }
        6 => {
            let other = &corpus[rng.below(corpus.len())];
            let a = rng.below(other.len());
            let n = rng.below((other.len() - a).min(4096) + 1);
            let chunk = other[a..a + n].to_vec();
            let i = rng.below(len + 1);
            let j = (i + rng.below(n + 1)).min(len);
            data.splice(i..j, chunk);
            desc.push_str(" splice");
        }
        7 => {
            const MARKERS: &[u8] = b"IPSEHG\x00\xFF\n";
            let k = 1 + rng.below(16);
            for _ in 0..k {
                if len > 0 {
                    let i = rng.below(len);
                    data[i] = MARKERS[rng.below(MARKERS.len())];
                }
            }
            let _ = write!(desc, " mark{k}");
        }
        8 => mutate_header(rng, data, desc),
        _ => {
            let other = &corpus[rng.below(corpus.len())];
            data.extend_from_slice(&other[..other.len().min(8 * 1024)]);
            desc.push_str(" concat");
        }
    }
}

/// Build the input of one iteration; the description names the mutations.
fn build_input(seed: u64, iteration: u64, corpus: &[Vec<u8>]) -> (Vec<u8>, String) {
    let mut rng = Rng::new(seed ^ iteration.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let base = rng.below(corpus.len());
    let mut data = corpus[base].clone();
    let mut desc = format!("base{base}");
    let ops = 1 + rng.below(4);
    for _ in 0..ops {
        mutate_once(&mut rng, corpus, &mut data, &mut desc);
    }
    (data, desc)
}

/// Every public entry point, on the whole input and on each log segment.
fn exercise(format: &BlackboxFormat, data: &[u8]) {
    let head = &data[..data.len().min(4096)];
    let _ = format.sniff(head, None);
    let _ = find_log_starts(data);
    for (_, seg) in split_logs(data) {
        let _ = Headers::parse(seg);
    }
    let _ = Headers::parse(data);
    let _ = format.open(data, &OpenOptions::default());
    let _ = format.open(
        data,
        &OpenOptions {
            sessions: None,
            raw: true,
        },
    );
}

fn crash_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fuzz-crashes")
}

struct Failure {
    seed: u64,
    iteration: u64,
    desc: String,
    path: PathBuf,
}

/// Run `iters` iterations (or until `budget` elapses) and collect the crashes.
fn run(seed: u64, iters: u64, budget: Option<Duration>) -> Vec<Failure> {
    let corpus = corpus();
    let format = BlackboxFormat::new();
    let started = Instant::now();
    let mut failures = Vec::new();
    let mut done = 0;
    for iteration in 0..iters {
        if budget.is_some_and(|b| started.elapsed() >= b) {
            break;
        }
        let (data, desc) = build_input(seed, iteration, &corpus);
        let outcome = catch_unwind(AssertUnwindSafe(|| exercise(&format, &data)));
        if outcome.is_err() {
            let dir = crash_dir();
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{seed:016x}-{iteration}.bin"));
            std::fs::write(&path, &data).unwrap();
            std::fs::write(path.with_extension("txt"), &desc).unwrap();
            failures.push(Failure {
                seed,
                iteration,
                desc,
                path,
            });
        }
        done = iteration + 1;
    }
    eprintln!(
        "fuzz: seed {seed:#x}, {done} iterations in {:.1} s, {} crash(es)",
        started.elapsed().as_secs_f64(),
        failures.len()
    );
    failures
}

fn report(failures: &[Failure]) {
    assert!(
        failures.is_empty(),
        "{} crash(es):\n{}",
        failures.len(),
        failures
            .iter()
            .map(|f| {
                format!(
                    "  OPENPID_FUZZ_REPLAY={}:{}  ({}; input {})",
                    f.seed,
                    f.iteration,
                    f.desc,
                    f.path.display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// Always on: a short run on every CI build, a fixed seed.
#[test]
fn mutated_fixtures_never_panic() {
    let seed = env_u64("OPENPID_FUZZ_SEED").unwrap_or(DEFAULT_SEED);
    let iters = env_u64("OPENPID_FUZZ_ITERS").unwrap_or(QUICK_ITERS);
    report(&run(seed, iters, None));
}

/// The weekly job: a time budget and a fresh seed per run
/// (`cargo test -p openpid-fmt-blackbox --release --test fuzz -- --ignored fuzz_long`).
#[test]
#[ignore = "long; run by the weekly fuzz workflow"]
fn fuzz_long() {
    let seed = env_u64("OPENPID_FUZZ_SEED").unwrap_or(DEFAULT_SEED);
    let seconds = env_u64("OPENPID_FUZZ_SECONDS").unwrap_or(600);
    let iters = env_u64("OPENPID_FUZZ_ITERS").unwrap_or(u64::MAX);
    report(&run(seed, iters, Some(Duration::from_secs(seconds))));
}

/// Reproduce one crash with the panic and backtrace intact:
/// `OPENPID_FUZZ_REPLAY=<seed>:<iteration> cargo test … -- --ignored fuzz_replay`.
#[test]
#[ignore = "needs OPENPID_FUZZ_REPLAY"]
fn fuzz_replay() {
    let spec =
        std::env::var("OPENPID_FUZZ_REPLAY").expect("OPENPID_FUZZ_REPLAY=<seed>:<iteration>");
    let (seed, iteration) = spec.split_once(':').expect("<seed>:<iteration>");
    let seed: u64 = seed.parse().expect("seed");
    let iteration: u64 = iteration.parse().expect("iteration");
    let corpus = corpus();
    let (data, desc) = build_input(seed, iteration, &corpus);
    eprintln!(
        "replaying {seed}:{iteration} ({desc}, {} bytes)",
        data.len()
    );
    exercise(&BlackboxFormat::new(), &data);
}

/// The generator itself: deterministic per `(seed, iteration)` and not a no-op.
#[test]
fn inputs_are_reproducible_and_varied() {
    let corpus = corpus();
    let (a, da) = build_input(7, 3, &corpus);
    let (b, db) = build_input(7, 3, &corpus);
    assert_eq!(a, b);
    assert_eq!(da, db);
    let distinct: std::collections::HashSet<Vec<u8>> =
        (0..20).map(|i| build_input(7, i, &corpus).0).collect();
    assert!(
        distinct.len() > 10,
        "mutations collapse: {}",
        distinct.len()
    );
    // header editing finds lines and numbers on real fixtures
    let base = &corpus[0];
    assert!(!header_lines(base).is_empty());
    assert!(!numbers(&base[..header_block(base)]).is_empty());
}
