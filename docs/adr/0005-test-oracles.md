# ADR-0005: Oracles for checking the decoder

Status: accepted (2026-11-04)

## Context
The official `blackbox_decode` (C) could not be built in the development environment; the decoder needs independent sources of truth.

## Decision
Three layers of checks, independent of each other:
1. **The specification**: unit tests of the encodings on the examples from "Blackbox Internals" (the UVB, ZigZag and Elias-delta tables) and round trips through own encoders that mirror the firmware logic.
2. **An independent decoder**: `blackbox-log` 0.4.3 as a *dev-dependency* (it never reaches the binaries), comparing the raw frame values aligned by timestamp. The known deviations of the reference are documented in ADR-0004 and excluded from the comparison.
3. **The official tool**: the `csv_conformance` test compares our raw decode with the `blackbox_decode` CSV when such files sit next to the fixtures (`<log>.<session>.csv`). The owner or CI generates them on a machine where the tool is built.

The golden files (`tests/golden/*.json`) pin the summary of every fixture; an algorithm change requires their deliberate update.

## Consequences
- Dev-dependencies are not bound by ADR-0001 as strictly as runtime dependencies, but they must be pure Rust with a compatible licence.
- Fuzzing through `cargo fuzz` is replaced for now by a deterministic robustness test (random corruption of the fixtures in CI); `cargo fuzz` is added when it becomes available in the environment. *Update 2026-11-05:* instead of `cargo fuzz`, an own mutation fuzzer was adopted (ADR-0013).
