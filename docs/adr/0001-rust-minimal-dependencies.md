# ADR-0001: Rust and the minimal-dependency policy

Status: accepted (2026-11-04)

## Context
The project must be reliable, auditable and cross-platform. Every external dependency is a risk (correctness, security, licence, the build on three platforms).

## Decision
- The language is Rust only (stable, edition 2024, the MSRV pinned in `rust-toolchain.toml`).
- Dependencies are allowed in three categories only: (1) platform / GPU / window-system APIs, (2) algorithms where an own implementation carries a higher risk of mistakes than a proven pure-Rust crate (FFT), (3) serialisation / CLI infrastructure.
- Forbidden: crates with C/C++ (`*-sys`) outside category (1); dynamic plugin loading; async runtimes (threads are enough for a desktop tool).
- Every new dependency is added only through an ADR; `cargo deny` blocks unknown licences.
- The actual list (phase 1): `eframe`/`egui`/`egui_plot` (the `glow` backend, transitively `winit`, `glutin`), `rfd` (file dialogs), `rustfft`, `serde`/`serde_json`, `thiserror`. Not needed after all: `memmap2` (reading the file whole is enough), `clap` (a hand-written argument parser). Dev: `blackbox-log` (a reference), `serde_json`.

## Consequences
- A slower start (own decoder, own windows / Welch / deconvolution), but full control over the mathematics.
- An own FFT was deliberately rejected: `rustfft` is pure Rust, deterministic, reference-tested and SIMD; that matches the "trustworthy mathematics" requirement.
