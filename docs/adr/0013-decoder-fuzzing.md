# ADR-0013: Decoder fuzzing with cargo-fuzz; fuzz-only dependencies allowed

- Status: accepted
- Date: 2026-11-06 (rewritten; the first version of 2026-11-05 is under History)

## Context

DESIGN §5 and §11 call fuzzing of the decoders mandatory: the input is
untrusted, and the header and frame parsers are the largest panic surface.

The first version of this decision rejected `cargo fuzz` because it needs a
nightly toolchain and `libfuzzer-sys`, which builds the C++ libFuzzer runtime,
against the pure-Rust rule for dev-dependencies (ADR-0005) and the pinned
toolchain. It chose an own mutation harness instead (structure-aware mutators
in a stable test). That harness had no coverage feedback, so deep parser states
were reached only by luck, and its mutators, corpus handling and crash
reproduction were code we maintained ourselves.

`cargo-fuzz` is the maintained standard of the Rust ecosystem: coverage-guided
mutation (libFuzzer), sanitizers, dictionaries, corpus minimisation, crash
files that reproduce with one command, OSS-Fuzz compatibility. Its two costs
are exactly what the rules forbade, and both can be confined to one package.

## Decision

1. **Fuzz targets live in a separate package `fuzz/`, excluded from the
   workspace** (`[workspace] exclude = ["fuzz"]`). It has its own `Cargo.toml`
   and lock file and depends on the format crates by path. Nothing in it reaches
   the shipped binaries, the workspace lock, `cargo deny` or the MSRV.
2. **The dependency rule gets one exception**: the `fuzz/` package may depend on
   `libfuzzer-sys` (MIT OR Apache-2.0 OR NCSA; builds libFuzzer with a C++
   compiler) and is built with a nightly toolchain (`cargo +nightly fuzz …`).
   No other package may; ADR-0001 and ADR-0005 stay in force for the workspace.
3. **Targets**: every public entry point of every decoder. Now: `blackbox_open`
   (sniff, open, open raw), `blackbox_headers` (`find_log_starts`,
   `split_logs`, `Headers::parse` per segment and whole), `ardupilot_open`,
   `csv_open`. A new decoder adds its target in the same PR.
4. **Structural knowledge in dictionaries, not in mutators**: the frame
   markers, header keys, field names, message names and interesting numbers
   that the old mutators injected are libFuzzer dictionaries
   (`fuzz/dictionaries/{blackbox,ardupilot,csv}.dict`), passed with `-dict`.
5. **Corpus**: `fuzz/seed.sh` seeds every target from the committed test logs
   the way the old harness built its corpus (a 24 KiB segment at each of the
   first six log starts of every Blackbox fixture plus a 4 KiB head; 64 KiB
   and 8 KiB heads of the ArduPilot logs; two CSV exports from the CLI). The
   grown corpus and the artifacts are not committed.
6. **CI** (`fuzz.yml`): weekly, every target with a 20-minute budget on a dated
   nightly plus `cargo-fuzz` at a pinned version (bumped deliberately,
   ADR-0007); a one-minute run of every target on pull requests that touch a
   decoder crate or `fuzz/`; crashing inputs are uploaded as an artifact.
7. **The own harness is removed** (`crates/openpid-fmt-blackbox/tests/fuzz.rs`
   and its weekly job). Every crash it ever found stays covered by the
   regression tests in the modules (for example `headers.rs`, "an interval
   that does not fit"). Every crash found from now on is fixed the same way:
   together with a regression test in the relevant module.

## Comparison before the removal

The old harness exercised `sniff`, `find_log_starts`, `split_logs`,
`Headers::parse` (per segment and whole), `open` and `open --raw`; the two
Blackbox targets call exactly these. Line coverage of
`crates/openpid-fmt-blackbox/src/` was measured with the same instrumentation
(`-C instrument-coverage`, release profile) for the old test
(`mutated_fixtures_never_panic`, 1000 iterations, ≈ 4 s on stable) and for the
union of the two cargo-fuzz targets after 4 minutes each on the seeded corpus
with the dictionary (one laptop core):

COMPARISON_TABLE

## Consequences

- Coverage feedback and the libFuzzer tooling at no cost to the workspace:
  `cargo build`, `cargo test`, `cargo deny check` and the release are unchanged.
- Two toolchains in CI (stable for everything, nightly for `fuzz.yml` only) and
  a C++ compiler on the fuzz runner (present on the GitHub images).
- Pull requests that touch a decoder get a minute of fuzzing per target on top
  of the unit, golden and conformance tests; everything else runs weekly.
- `fuzz/Cargo.lock` is committed for reproducible CI builds.

## History

**2026-11-05, first version ("Fuzzing the decoder without `cargo fuzz`").**
An own mutation fuzzer in `tests/fuzz.rs` without dependencies: a corpus of
fixture segments (the first 24 KiB, up to six segments per file), mutators (bit
flips, random bytes, truncation, chunk removal / duplication / insertion,
splicing between fixtures, frame-marker injection, header-line edits with
interesting numbers, concatenation of logs), every iteration seeded from
`(seed, iteration)` for one-step replay, 1000 iterations in every CI build and
a weekly 20-minute run. Replaced by cargo-fuzz on 2026-11-06 once the
dependency and toolchain costs could be confined to the `fuzz/` package; its
structural mutations live on as the dictionaries.
