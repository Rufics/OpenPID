# ADR-0013: Fuzzing the decoder without `cargo fuzz`

- Status: superseded by ADR-0015 (2026-11-06): decoder fuzzing moved to cargo-fuzz; the mutators below remain as the stable smoke test `mutated_fixtures_never_panic`
- Date: 2026-11-05

## Context

DESIGN §7 and §11 call fuzzing of the blackbox decoder mandatory: the input is
untrusted, and the header and frame parsers are the largest panic surface. The
plan was `cargo fuzz` (#37). But `cargo-fuzz` needs a nightly toolchain and
`libfuzzer-sys`, which builds the C++ libFuzzer runtime. That contradicts the
project rule for dev-dependencies (pure Rust, ADR-0005) and the pinned
toolchain in `rust-toolchain.toml`. The guided-fuzzing alternatives (AFL,
honggfuzz) pull third-party binaries as well.

## Decision

- **An own mutation fuzzer** in `crates/openpid-fmt-blackbox/tests/fuzz.rs`,
  without dependencies: the corpus is segments of the logs in `tests/logs/`
  (the first 24 KiB of each, up to 6 segments per file); the mutators are bit
  flips, random bytes, truncation, removal / duplication / insertion of chunks,
  splicing between fixtures, injection of frame markers (`I P S E H G`),
  header-line edits (remove, duplicate, swap, replace a number with an
  "interesting" value such as `-1`, `256`, `2^31`, `2^64`, `NaN`, add or drop
  a comma-separated list element) and concatenation of several logs. Every
  iteration runs the input through `sniff`, `find_log_starts`, `split_logs`,
  `Headers::parse` and `BlackboxFormat::open` (normal and `raw`).
- **Reproducibility instead of coverage**: the iteration generator depends only
  on `(seed, iteration)`, so a crash is reproduced by one run of
  `OPENPID_FUZZ_REPLAY=<seed>:<iteration> cargo test -p openpid-fmt-blackbox
  --test fuzz -- --ignored fuzz_replay`; the input is also written to
  `target/tmp/fuzz-crashes/`.
- **Two modes**: `mutated_fixtures_never_panic`, 1000 iterations with a fixed
  seed in every CI run (a few seconds in a debug build); `fuzz_long`
  (`#[ignore]`), with a time budget, run weekly by the workflow
  `.github/workflows/fuzz.yml` (20 min in release, seed = the run id, so
  different inputs every week). A crash → an artifact with the inputs and the
  `OPENPID_FUZZ_REPLAY=…` text in the report.
- Every crash found is fixed together with a regression test in the relevant
  module (not only in the fuzzer).

## Consequences

- No coverage feedback, so deep parser states are reached less often than with
  libFuzzer; compensated by the structural header mutations (where the range
  logic lives) and the real fixtures as the base.
- No new entry in `deny.toml`, no nightly, everything runs on three platforms.
- Should `cargo fuzz` ever become acceptable (for example a separate crate
  outside the workspace with its own licence policy), the corpus and mutators
  stay useful as a seed corpus; this decision does not block it.
