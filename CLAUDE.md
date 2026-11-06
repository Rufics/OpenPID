# OpenPID — rules for working in this repository

- **CI is never left red.** After every push check the GitHub Actions status; if it is red, fix it before anything else. Before pushing run locally: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` (install with `cargo install cargo-deny --locked`).
- The toolchain is pinned in `rust-toolchain.toml`; CI uses the same version. Bump it deliberately, in its own commit, together with the fixes for any new lints.
- New dependencies only through an ADR (`docs/adr/`); dev-dependencies must be pure Rust with a licence allowed by `deny.toml`.
- **The repository language is English** for everything: code, comments, documents, README, PR and issue text. Documents that are still in Ukrainian are translated step by step (separate issues); new documents are written in English from the start. The interface translation (`i18n.rs`) is a product feature and stays.
- Never copy PIDtoolbox code (GPLv3); implement the algorithms from their descriptions.
- **Every PR references an issue** (`Closes #N` or `Part of #N` in the description). No issue yet — create it before the PR. CI checks this (job `conventional commits`, step "PR references an issue").
- **No PR is closed silently.** When a PR is replaced by another one (for example the commit header exceeded the commitlint limit and force-push is not allowed) or abandoned, leave a comment with the reason and a link to the replacement PR or issue before closing it. The trail belongs in the PR itself, not only in a report.
