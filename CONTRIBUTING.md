# Contributing to OpenPID

## Workflow (trunk-based with feature branches)

1. `master` is protected: no direct pushes, pull requests only.
2. One branch per change: `feat/<topic>`, `fix/<topic>`, `docs/<topic>`, `ci/<topic>`.
3. Commits and the **PR title** follow [Conventional Commits](https://www.conventionalcommits.org/):
   `type(scope): description`, where `type` ∈ `feat | fix | perf | refactor | docs | test | build | ci | chore | revert`
   and `scope` is a crate or an area (`dsp`, `fmt-blackbox`, `analysis`, `app`, `cli`, `core`, `ci`, `docs`).
   `feat` bumps the minor version, `fix`/`perf` the patch, `BREAKING CHANGE:` in the footer or `!` after the type the major.
   release-plz builds the CHANGELOG and the versions from these messages, so they must describe the change for a user.
   The header is at most 100 characters (commitlint).
4. A PR is **squash-merged** (its title becomes the commit message) only when:
   - every required CI check is green (`CI OK` and the individual jobs),
   - a code owner approved it (`.github/CODEOWNERS`),
   - every review thread is resolved,
   - the branch is up to date with `master` (strict status checks).
5. Releases: release-plz opens (and keeps updating) a `chore: release vX.Y.Z` PR after every merge into `master`;
   merging that PR creates the `vX.Y.Z` tag and the GitHub Release and starts the binary build (`release.yml`).
   See [`docs/RELEASING.md`](docs/RELEASING.md).

## Before you push

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check                                   # cargo install cargo-deny --locked
git lfs pull                                       # test logs (tests/logs) live in Git LFS
cargo llvm-cov --workspace --fail-under-lines 90   # cargo install cargo-llvm-cov --locked
```

CI (`.github/workflows/ci.yml`) does the same: fmt, clippy with every style lint
(`docs/STYLE.md`), rustdoc without warnings, spelling (`typos`), tests on
Linux/macOS/Windows, line coverage ≥ 90 % over the whole workspace (the GUI is
tested headless with `egui_kittest`), cargo-deny, binary artifacts for the three
platforms, SonarCloud (when the secret is present), commit-message and PR-title
checks. CI posts a summary of the coverage and the Sonar findings as a PR comment
and updates it on every push. The project rule: **CI never stays red**.

## Code style

See [`docs/STYLE.md`](docs/STYLE.md), an adapted Google Rust Style Guide. Most
rules are enforced automatically (rustfmt plus the clippy lints in `Cargo.toml`
and `clippy.toml`); the rest is for code review.

## Architecture and decisions

- `docs/DESIGN.md`: architecture, data model, mathematics.
- `docs/adr/`: decision records; a new dependency or a format change means a new ADR.
- `docs/BLACKBOX-VERSIONS.md`: format differences between firmware versions and the test-log coverage.
- `docs/design/`: one UX note per feature (problem, options with scores, decision, requirements, acceptance).

## Test data

Committed logs come only from MIT/Apache-compatible sources
(`tests/logs/README.md`); the rest is fetched by `tools/fetch-community-logs.sh`.
Golden files are updated deliberately:
`OPENPID_UPDATE_GOLDEN=1 cargo test -p openpid-fmt-blackbox --test fixtures`;
the numeric reference fixtures with `python3 -I tools/reference/make_fixtures.py`;
the reference CSVs of the official `blackbox_decode` with
`sh tools/blackbox-decode-csv.sh <betaflight/blackbox-tools checkout>`
(after a new Blackbox fixture is added; see `tests/logs/README.md`).

## Fuzzing

The decoders are fuzzed with cargo-fuzz in the separate `fuzz/` package
(`fuzz/README.md`, ADR-0013): `cargo +nightly fuzz run blackbox_open` after
`sh fuzz/seed.sh`. CI runs every target weekly and for a minute on PRs that
touch a decoder. A new decoder entry point gets a target in the same PR.

## Process

The backlog, milestones per phase, sprints and the Definition of Ready/Done are
described in [`docs/PROCESS.md`](docs/PROCESS.md).

**Every PR references an issue**: `Closes #N` (closes it) or `Part of #N` (part
of an epic) in the PR description. No issue yet — create one first (Context /
Scope / Acceptance). CI checks this in the "PR references an issue" step of the
`conventional commits` job; PRs from bots (release-plz, Dependabot) are exempt.

**No PR is closed silently.** When a PR is replaced by a new one (the usual
case: the commit header exceeded the 100-character commitlint limit and
force-push is not allowed, so the change is rebuilt in a new branch) or
abandoned, leave a comment with the reason and a link to the replacement PR or
issue before closing it. That keeps the history in the PR itself.

## Language

English for everything in the repository: code, comments, documents, PR and
issue text. The interface translation (`crates/openpid-app/src/i18n.rs`) is a
product feature and stays.
