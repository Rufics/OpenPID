# ADR-0009: Test logs only under an MIT-compatible licence, in Git LFS

- Status: accepted
- Date: 2026-11-04

## Context

Some tests (the 4.3+ sliders, Rotorflight, newer Betaflight versions) worked only with logs that `tools/fetch-community-logs.sh` downloaded into a git-ignored directory, because their sources had no MIT-compatible licence. CI had none of those files, so the tests were silently skipped: the coverage of `rotorflight.rs` was 9 % in CI and 95 % locally.

## Decision

- The download script and the `tests/logs/external/` directory are removed entirely. Tests read `tests/logs/` only.
- **Only** logs under an MIT-compatible licence (MIT, Apache-2.0, BSD, CC0/CC-BY) are added to the repository. The source and licence of every file are recorded in `tests/logs/README.md`; GPL, non-commercial and unlicensed files are not accepted, even when another MIT project has already copied them.
- The logs are stored in **Git LFS** (`.gitattributes`: `tests/logs/*.{bbl,bfl,txt}` in any case). History was not rewritten: the old fixtures were moved into LFS by a new commit.
- Large files are cut to the first 1–1.5 MiB or to a session boundary (`betaflight-4.5.1-two-sessions.bbl` is the first two complete sessions of eight): the decoder needs the headers and a few thousand frames, not minutes of flight.
- CI pulls the LFS objects only in the jobs that run tests (`test` ×3, `coverage`), through `actions/cache` keyed by the list of OIDs; `build`/`sonar` do without them. The free GitHub LFS quota is 1 GB of traffic per month, so the fixtures are deliberately small (≈ 9 MB in total).

## Consequences

- After cloning: `git lfs install && git lfs pull`. Without it the files are pointers and `versions.rs` fails with a hint.
- Betaflight 4.3, 4.4, 2026.6 and EmuFlight 0.4 still have no fixtures; `docs/BLACKBOX-VERSIONS.md` keeps the list. New logs come only through a PR with the source table filled in.
