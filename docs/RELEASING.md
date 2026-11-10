# Releases and repository setup

## One-time GitHub setup (done by the owner)

1. **Ruleset for `master`.** Settings → Rules → Rulesets → New ruleset → Import a ruleset → the file `.github/rulesets/master.json`. It requires PRs with squash merge, one code-owner approval, resolved threads, a linear history, signed commits, no force-push or deletion, and green checks: `CI OK`, `fmt + clippy + docs + typos`, `conventional commits`, `test (ubuntu/macos/windows)`, `coverage (>= 90% lines)`, `cargo deny`. The checks are "strict": the PR branch must be up to date with `master`.
   - If signed commits are not wanted, remove the `required_signatures` rule from the JSON before importing.
   - The field `integration_id: 15368` is GitHub Actions; GitHub fills in the right value on import.
2. **Secrets and the release bot.**
   - `SONAR_TOKEN` is already set; the `SonarCloud` job is active.
   - **The token for release-plz comes from a bot, never from a person.** The `release-plz` workflow authenticates as a GitHub App and mints itself a one-hour installation token (`actions/create-github-app-token`); no personal access tokens. Why a token at all: PRs and tags created with the default `GITHUB_TOKEN` do not start other workflows (GitHub's recursion guard), so a release PR would get no CI checks and could not be merged under the ruleset, and the tag would not start the binary build. An App token has no such restriction.
     Creating it (five minutes, once):
     1. Settings → Developer settings → GitHub Apps → *New GitHub App*. A name such as `openpid-release-bot`; Homepage: the repository URL; *Webhook* off.
     2. Permissions → Repository permissions: **Contents: Read and write**, **Pull requests: Read and write**, **Metadata: Read-only** (added automatically). Nothing else. *Where can this App be installed*: *Only on this account*.
     3. Create → note the **App ID** on the App page; below it *Generate a private key* downloads a `.pem`.
     4. Install App → install it on your account, selecting **only the OpenPID repository**.
     5. In the repository: Settings → Secrets and variables → Actions: the variable `RELEASE_APP_ID` = App ID; the secret `RELEASE_APP_PRIVATE_KEY` = the whole `.pem`. The `release-plz` workflow then enables itself (without the variable it is skipped and does not go red).
     Access can be revoked at any time: delete the App installation or generate a new key.
     Checking the setup: the next push to `master` (any merge) must give a `release-plz` run with status *success* (not *skipped*) and a `chore: release vX.Y.Z` PR from `<App name>[bot]` on which the usual CI checks run. Typical failures: a *skipped* run means the `RELEASE_APP_ID` variable is missing (or was created as a secret); a red "Mint a short-lived token" step means a truncated `.pem` or an App not installed on the repository; a refused push means *Contents: Read and write* is missing.
   - The "forbid" alternative: do not create the App; releases are then made by hand (`git tag vX.Y.Z && git push --tags` from a workstation starts `release.yml`) and the CHANGELOG is kept manually. The automation is off, nothing breaks.
3. **Settings → General → Pull Requests**: leave only *Allow squash merging* on, set *Default commit message: Pull request title and description*, enable *Automatically delete head branches*.
4. **Settings → Actions → General**: *Workflow permissions: Read repository contents*. The option *Allow GitHub Actions to create and approve pull requests* is **not** needed: the App token is not subject to it.
5. **Dependabot** is configured (`.github/dependabot.yml`): weekly grouped PRs for cargo and GitHub Actions. Settings → Code security: enable Dependabot alerts.
6. **SonarCloud**: after the first analysis set the Quality Gate to "Sonar way" and enable *Pull request decoration*; Sonar becomes one more check that can be added to the ruleset (`SonarCloud Code Analysis`).

## How a release happens

```
feature branch ──PR──▶ master ──release-plz──▶ PR "chore: release vX.Y.Z" ──merge──▶ tag vX.Y.Z
                                                                                     └─▶ GitHub Release + CHANGELOG
                                                                                     └─▶ release.yml: binaries for
                                                                                         Linux x86_64, macOS arm64, Windows x86_64
```

One version for the whole workspace (`[workspace.package].version`), the tag
is `v<version>`, the log is `CHANGELOG.md` built from Conventional Commits.
Nothing is published to crates.io (`publish = false`). The tag and the GitHub
release (named "OpenPID v<version>", the body is the CHANGELOG entry) are
created by the `openpid-cli` crate alone; every other crate has
`git_tag_enable = false`, otherwise each would try to create the same tag and
the second one would fail with 422 (#113). The other crates count as released
under that shared tag.

## CI artifacts

Every CI run builds `openpid` and `openpid-gui` for the three platforms (the
`build` job) and the coverage report (`coverage-lcov`); they can be downloaded
from the run page (Actions → run → Artifacts) and are kept for 14 days.

## Cache

`Swatinem/rust-cache` caches `~/.cargo` and `target/` keyed by `Cargo.lock`
plus the toolchain, separately for the `check`, `test`, `coverage` and
`build-<os>` jobs; repeated runs rebuild only the changed crates.

## Abandoned macOS jobs (arm64 runner shortage)

Now and then GitHub does not allocate a `macos-15` (arm64) runner: the job
ends as *abandoned* ("The job was not acquired by Runner of type hosted even
after multiple attempts"). `CI OK` counts that as red on purpose (#9), so a
merge never goes through with an untested platform. The policy:

1. **Push nothing just to rerun**: no empty commits, no closing and reopening
   the PR. Rerun from Actions → run → *Re-run failed jobs*, or
   `gh run rerun <run-id> --failed` (needs the `repo` scope). Only the
   abandoned or failed jobs rerun; the green ones are not recounted.
2. **Two attempts, then wait.** If the runner still does not appear after the
   second rerun, the shortage is global: check https://www.githubstatus.com
   and retry later; CI is not weakened.
3. **No automatic retry.** `nick-fields/retry` or the like is a new workflow
   dependency (ADR-0007), and a repeated `cargo test` does not help when the
   problem is machine allocation, not the test.
4. **One macOS architecture.** macOS is built for Apple silicon (arm64) on
   `macos-15` alone; the Intel leg on `macos-15-intel` was removed (#164). A
   release therefore waits for that pool: when the `aarch64-apple-darwin` job
   of `release.yml` is abandoned, rerun it the same way, and the archive is
   attached to the existing GitHub release.
