# OpenPID code style

The base is the [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html) (adapted; the rules that only concern Chromium/Android/Fuchsia are dropped), on top of the [official Rust Style Guide](https://doc.rust-lang.org/style-guide/) and the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/). Where they disagree, this document wins.

The "Check" column: **auto** means CI rejects it automatically (rustfmt / clippy / rustdoc / typos); **review** means code review.

## 1. Formatting

| Rule | Check |
|------|-------|
| All code is formatted by `rustfmt` with `rustfmt.toml` (`max_width = 100`). `#[rustfmt::skip]` only for hand-aligned matrices or tables and generated code, with a comment saying why. | auto (`cargo fmt --check`) |
| Hex literals in lower case (`0x3f80_0000`); long numbers separated with `_`. | auto (`clippy::unreadable_literal`, `clippy::manual_range_contains`, …) |

## 2. Code organisation and imports

| Rule | Check |
|------|-------|
| Small crates with one responsibility; dependencies between them only point "down" (see `docs/DESIGN.md` §3). | review |
| A file and a leaf module are named after their main type (`series.rs` → `Series`). | review |
| **No glob imports** (`use foo::*`), except `use super::*` in `mod tests`. | auto (`clippy::wildcard_imports`) |
| One `use` per module group; types are imported and used without a path (`Series`, not `series::Series`), free functions are called with their module (`mem::take`, not `take`). Exceptions: ambiguous names (`io::Error`). | review (`imports_granularity = "Module"` in rustfmt is nightly-only) |
| Traits needed in one function are imported inside it. | review |

## 3. Ownership, parameters, return values

| Rule | Check |
|------|-------|
| Avoid `Rc`/`Arc`; use them when the ownership structure would otherwise become unreasonably complex (in the GUI a log session is an `Arc<FlightSession>` because threads share it). | review |
| `.clone()` only when a deep copy is needed, never instead of a borrow. | review (`clippy::redundant_clone`) |
| Take `&T`, `&[T]`, `&str`, `Option<&T>` rather than `&Box<T>`, `&Vec<T>`, `&String`, `&Option<T>`. | auto (`clippy::ptr_arg`, `clippy::borrowed_box`, `clippy::ref_option`) |
| In library crates prefer iterators / `impl IntoIterator`; in the applications (`openpid-app`, `openpid-cli`) `Vec`/`&[T]` are fine when they simplify the code. | review |
| An `enum` instead of a `bool` in public parameters when the meaning of the value is not obvious. | review (`clippy::fn_params_excessive_bools`) |

## 4. Errors and panics

| Rule | Check |
|------|-------|
| `Option` for plain absence; `Result` with an error type (`thiserror`) for recoverable errors with context; `panic!` only for broken invariants (a bug). | review |
| **`unwrap()` is forbidden** outside tests; when a panic is unavoidable, `expect("why this cannot happen")`. | auto (`clippy::unwrap_used`; allowed in tests through `clippy.toml`) |
| Prefer pattern matching (`if let`, `let … else`) over "check then unwrap". | review (`clippy::unnecessary_unwrap`) |
| `assert!` with a context message when the reason is not obvious or a value in the message helps. | review |
| Functions that can panic on bad input document it in a `# Panics` section. | auto (`clippy::missing_panics_doc`) |
| `todo!()`, `unimplemented!()`, `dbg!()` never reach `master`. | auto (`clippy::todo`, `clippy::unimplemented`, `clippy::dbg_macro`) |

## 5. Types and traits

| Rule | Check |
|------|-------|
| A newtype instead of a `type` alias when type safety matters; named fields instead of tuples (except newtypes). | review |
| `#[derive]` instead of hand-written impls of the standard traits; implement the standard traits (`Default`, `From`, `Display`, …) instead of own methods with the same meaning. | auto (`clippy::derivable_impls`, `clippy::should_implement_trait`) |
| Option structs implement `Default`. Constructors: `new()`, `new_with_x()`, `try_new()` for a `Result`; `from_x()` paired with `From`. | review (`clippy::new_without_default`) |
| Public enums and structs with public fields that will grow (for example `ChannelId`, `EventKind`) are `#[non_exhaustive]` when the crate is used outside the workspace. | review |
| `#[must_use]` on pure functions and result types when the type is not already `#[must_use]`. | auto (`clippy::must_use_candidate`, warn) |

## 6. Naming

| Rule | Check |
|------|-------|
| The Rust API Guidelines conventions: `as_`/`to_`/`into_` according to `self`; getters `foo()`, `foo_mut()`, `take_foo()`, `set_foo()`. | auto (`clippy::wrong_self_convention`) |
| No obscure abbreviations; the usual ones (`fft`, `psd`, `pid`, `rpm`, `csv`) are fine. | review |
| Descriptive lifetime names in types (`'data`); single letters only for simple input → output links. | review |
| Extension traits end in `Ext`. | review |

## 7. Documentation

| Rule | Check |
|------|-------|
| **Every public item is documented** (`///`), every module starts with `//!`. A comment says what the code does not; a trivial restatement of the name is worse than nothing (then `#[allow(missing_docs)]` with the reason). | auto (`missing_docs`) |
| Markdown; identifiers in backticks; sentences start with a capital and end with a full stop. | auto (`clippy::doc_markdown`) |
| No broken links in the docs; `cargo doc` without warnings. | auto (`RUSTDOCFLAGS=-D warnings`) |
| Spelling in code, comments and docs. | auto (`typos`) |

## 8. Other

| Rule | Check |
|------|-------|
| **`unsafe` is forbidden** in every crate (`#![deny(unsafe_code)]`). Should it ever become necessary: a separate module, a `// SAFETY:` comment and the reasoning in an ADR. | auto (`unsafe_code = "deny"`) |
| Stable toolchain only, no nightly features. | auto (`rust-toolchain.toml`) |
| No code runs before `main()` (no `ctor`). | review |
| Enough explicit types for the inferred ones to be clear to a reader (especially `.into()`, `.parse()`, the results of iterator chains). | review |
| A plain loop instead of a complex combinator chain when it reads better. | review |
| Mathematics in `f64`, units in types, explicit validity masks (see ADR-0003). | review + reference tests |

## How it is enforced

- `Cargo.toml` → `[workspace.lints]`: `missing_docs`, `unsafe_code`, `clippy::unwrap_used`, `clippy::wildcard_imports`, `clippy::missing_panics_doc`, `clippy::dbg_macro`, `clippy::todo`, `clippy::unimplemented` plus `clippy::pedantic`; every warning is an error in CI (`-D warnings`).
- `clippy.toml`: `unwrap`/`expect`/`dbg`/`print` are allowed in tests.
- Locally before pushing: the commands in `CONTRIBUTING.md`.
