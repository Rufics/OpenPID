# ADR-0015: Decoder fuzzing with cargo-fuzz; fuzz-only dependencies allowed

- Status: accepted
- Date: 2026-11-06
- Supersedes: ADR-0013 (the own mutation fuzzer)

## Context

ADR-0013 chose an own mutation fuzzer because the dev-dependency rule (pure
Rust, ADR-0005) ruled out `libfuzzer-sys` (the C++ libFuzzer runtime) and the
pinned stable toolchain ruled out nightly. That fuzzer has no coverage feedback,
so deep parser states are reached only by luck, and its mutators, corpus
handling and crash reproduction are code we maintain ourselves.

`cargo-fuzz` is the maintained standard of the Rust ecosystem: coverage-guided
mutation (libFuzzer), sanitizers, corpus minimisation, crash files that
reproduce with one command, and OSS-Fuzz compatibility. Its costs are exactly
the two things the rules forbade: a C++ runtime in the build and a nightly
toolchain. Both can be confined.

## Decision

1. **Fuzz targets live in a separate package `fuzz/`, excluded from the
   workspace** (`[workspace] exclude = ["fuzz"]`). It has its own `Cargo.toml`
   and lock file and depends on the format crates by path. Nothing in it reaches
   the shipped binaries, the workspace lock, `cargo deny` or the MSRV.
2. **The dependency rule gets one exception**: the `fuzz/` package may depend on
   `libfuzzer-sys` (MIT OR Apache-2.0 OR NCSA, builds the libFuzzer runtime with
   a C++ compiler) and is built with a nightly toolchain
   (`cargo +nightly fuzz …`). No other package may; ADR-0001 and ADR-0005 stay
   in force for the workspace.
3. **Targets**: every public entry point of every decoder. Now: `blackbox_open`
   (sniff, open, open raw), `blackbox_headers` (`find_log_starts`,
   `split_logs`, `Headers::parse`), `ardupilot_open`, `csv_open`. A new decoder
   adds its target in the same PR.
4. **Corpus**: seeded from the committed test logs by `fuzz/seed.sh` (the first
   kilobytes of each log, where the headers and the first frames are); the grown
   corpus and the artifacts are not committed.
5. **CI** (`fuzz.yml`): weekly, every target with a time budget on nightly plus
   `cargo-fuzz` installed at a pinned version; a one-minute run of every target
   on pull requests that touch a decoder crate or `fuzz/`; crashing inputs are
   uploaded as an artifact. The toolchain is a dated nightly, bumped
   deliberately like the stable one (ADR-0007).
6. **The stable smoke test stays**: `mutated_fixtures_never_panic` in
   `crates/openpid-fmt-blackbox/tests/fuzz.rs` runs 1000 structure-aware
   mutations in every CI build on stable in a few seconds, so a regression is
   caught in the PR itself, not a week later. Its long mode and weekly job are
   retired in favour of cargo-fuzz.
7. Every crash found is fixed together with a regression test in the relevant
   module, as before.

## Consequences

- Coverage feedback and the libFuzzer tooling, at no cost to the workspace:
  `cargo build`, `cargo test`, `cargo deny check` and the release are unchanged.
- Two toolchains in CI (stable for everything, nightly for `fuzz.yml` only) and
  a C++ compiler on the fuzz runner (present on the GitHub images).
- The structure-aware mutators of ADR-0013 remain as the stable smoke test; if
  they ever prove valuable to libFuzzer they can be ported as a custom mutator.
- `fuzz/Cargo.lock` is committed for reproducible CI builds and updated by
  Dependabot like the workspace lock.
