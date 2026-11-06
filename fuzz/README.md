# Fuzzing the decoders

Coverage-guided fuzzing with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
(libFuzzer), ADR-0015. This directory is a separate package, excluded from the
workspace: `libfuzzer-sys` and the nightly toolchain never reach the shipped
binaries, the workspace lock file or `cargo deny`.

## Targets

| Target | What it exercises |
|---|---|
| `blackbox_open` | `BlackboxFormat`: sniff, open with the firmware profile, open raw |
| `blackbox_headers` | the Blackbox container: `find_log_starts`, `split_logs`, `Headers::parse` |
| `ardupilot_open` | `ArdupilotFormat`: sniff and open |
| `csv_open` | `CsvFormat` (`blackbox_decode` CSV): sniff and open |

A panic in any of them is a finding. Every public entry point of a decoder
belongs here; add a `[[bin]]` in `Cargo.toml` and a file under `fuzz_targets/`.

## Running

```sh
rustup toolchain install nightly          # once
cargo install cargo-fuzz --locked         # once
git lfs pull                              # the test logs seed the corpus
sh fuzz/seed.sh                           # fuzz/corpus/<target>/ from tests/logs
cargo +nightly fuzz run blackbox_open -- -max_total_time=600 -timeout=10
cargo +nightly fuzz list
```

A crash is written to `fuzz/artifacts/<target>/crash-<hash>`; reproduce it with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-<hash>`, then
fix it together with a regression test in the crate (not only in the fuzzer).
`cargo +nightly fuzz cmin <target>` minimises the corpus,
`cargo +nightly fuzz coverage <target>` collects coverage.

CI (`.github/workflows/fuzz.yml`) runs every target weekly with a time budget
and for a minute on pull requests that touch a decoder or this directory;
crashing inputs are uploaded as an artifact.
