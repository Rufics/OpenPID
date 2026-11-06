# ADR-0007: Development process, CI checks and releases

Status: accepted (2026-11-04)

## Decision
- **Trunk-based with feature branches**: `master` is protected by a ruleset (`.github/rulesets/master.json`): PRs only, squash merge, one code-owner approval, resolved threads, a linear history, signed commits, strict status checks.
- **Conventional Commits** for commits and PR titles (`.commitlintrc.json`, the `commits` job); release-plz builds the CHANGELOG, the versions (one per workspace) and the `vX.Y.Z` tags from them; `release.yml` attaches the binaries to the GitHub Release.
- **Required checks**: fmt, clippy with the style lints (`docs/STYLE.md`), rustdoc without warnings, typos, tests on three platforms, **line coverage ≥ 90 %** over the whole workspace (`cargo llvm-cov --fail-under-lines`; the GUI is tested headless, ADR-0008), cargo-deny. The `CI OK` job aggregates them for the ruleset.
- **Artifacts**: every CI run publishes the binaries for the three platforms and the lcov report (14 days).
- **Cache**: `Swatinem/rust-cache` with separate keys per job; concurrency cancels stale runs.
- **SonarCloud** as an additional check (not blocking until the secret exists) on the same lcov; after the Quality Gate is set up it is added to the ruleset.
- **Dependabot** weekly, grouped PRs for cargo and actions.

## Why
- 90 % is the threshold for library code with golden / reference tests (≈ 92 % at the time of the decision); the GUI was excluded at first; since ADR-0008 it is tested headless with `egui_kittest` and counts towards the threshold.
- release-plz rather than release-please: native to a Cargo workspace, understands version inheritance, needs no Node.
- Squash merges make the PR title the single source of truth for the changelog and simplify bisecting.

## Consequences
- The owner adds the secrets for release-plz and Sonar (`docs/RELEASING.md`).
- Any new public item without documentation, or an `unwrap()` outside tests, breaks CI, on purpose.

## Pinned versions in CI (addendum 2026-11)

- Every GitHub Action is referenced by the **full commit SHA** with the tag in a comment (`uses: actions/checkout@3d3c42e… # v7.0.1`). A tag can be rewritten, a SHA cannot; Dependabot (`.github/dependabot.yml`, the `github-actions` ecosystem) updates the SHA and the comment together.
- Runners are dated images only (`ubuntu-24.04`, `macos-15`, `macos-15-intel`, `windows-2025`), never `*-latest`: an image change is a deliberate commit, not a surprise from GitHub.
- Jobs abandoned for lack of runners are rerun by hand (docs/RELEASING.md), without a retry action: that is a new dependency that does not cure the cause. macOS builds on both pools (`macos-15` arm64 and `macos-15-intel` x86_64), so the release does not depend on either (#44).
- Tools installed in CI carry a version too (`cargo-llvm-cov@0.9.1` through `taiki-e/install-action`; the toolchain is `RUST_TOOLCHAIN`, the same as in `rust-toolchain.toml`).
- Matrix job names do not contain the runner name (`test (linux)`, not `test (ubuntu-24.04)`), so an image update never breaks the list of required checks in the ruleset.

## The PR summary comment (addendum 2026-11)

The `PR summary comment` job (`.github/scripts/pr_summary.py`, the Python standard library only) runs after `coverage` and `sonar`, posts one comment on the PR and updates it on every push: line coverage per crate with the delta to the base branch and the least covered files, the SonarCloud quality-gate state with every condition, the bug / vulnerability / code smell / hotspot counts and a table of the open findings with links. The comment is informational: it is not part of `CI OK` and is posted even when the coverage gate failed. For PRs from forks the job is skipped (the token is read-only).
