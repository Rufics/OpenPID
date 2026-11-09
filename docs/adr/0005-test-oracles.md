# ADR-0005: Oracles for checking the decoder

Status: accepted (2026-11-04, rewritten 2026-11-09)

## Context
The decoder needs sources of truth that do not share its code. Two official
tools exist for Betaflight logs: the C `blackbox_decode`
(`betaflight/blackbox-tools`) and the parser of Blackbox Explorer
(`betaflight/blackbox-log-viewer`). Checked against both on the fixtures
(#49): `blackbox_decode` lags behind the format the firmware writes (last
release 2017; it does not parse the flight-mode and disarm events and drops
the P frames after them, reads `P interval:N` as 1/1, accepts a frame cut by
the end of the file; betaflight/blackbox-tools#73 has had no answer since
August 2026). Explorer is the tool the project develops and users run, handles
all three like the firmware, and matched our decoder on every Betaflight log
value for value. Rotorflight maintains its own Explorer fork
(`rotorflight/rotorflight-blackbox`), which knows its events.

## Decision
Four layers of checks, independent of each other:
1. **The specification**: unit tests of the encodings on the examples from "Blackbox Internals" (the UVB, ZigZag and Elias-delta tables) and round trips through own encoders that mirror the firmware logic.
2. **An independent decoder**: `blackbox-log` 0.4.3 as a *dev-dependency* (it never reaches the binaries), comparing the raw frame values aligned by timestamp. The known deviations of the reference are documented in ADR-0004 and excluded from the comparison.
3. **Blackbox Explorer, the regression reference**: `explorer_conformance` compares whole flight logs (every main, slow, GPS and GPS-home frame and every event) with the output of Explorer's parser, stored as per-field digests in `tests/logs/explorer/<fixture>.json`. Betaflight's Explorer decodes the Betaflight, INAV and EmuFlight fixtures, Rotorflight's fork the Rotorflight ones. `tools/blackbox-explorer/run.sh` makes the files from Explorer checkouts (Node.js); every Blackbox fixture must have one. One line of Explorer is reverted while loading it: PR #862 lost the 32-bit wrap of the `MINTHROTTLE`/`MINMOTOR` predictors that its own comment asks for (`tools/blackbox-explorer/README.md`); Explorer before that PR agrees with the result unmodified.
4. **The C `blackbox_decode`**: `csv_conformance` compares our raw decode with its CSV when such files sit next to the fixtures (`<log>.<session>.csv`). It is a second opinion, not the reference, because of the gaps above.

The golden files (`tests/golden/*.json`) pin the summary of every fixture; an algorithm change requires their deliberate update.

## Consequences
- Dev-dependencies are not bound by ADR-0001 as strictly as runtime dependencies, but they must be pure Rust with a compatible licence.
- Making the Explorer reference needs Node.js and the npm package `semver` (Explorer's own dependency, pinned in `tools/blackbox-explorer/package-lock.json`). They are tools for producing test data, like the C compiler for `blackbox_decode`: not Rust dependencies, not needed by `cargo test` or CI.
- Explorer is GPL-3.0: it runs as an external program when the reference is made; no code of it is copied into the repository or linked into OpenPID.
- A decoder change that alters a value now fails against Explorer on the whole log, not only on the first rows; the failure names the field and the range of frames.
- Fuzzing through `cargo fuzz` is replaced for now by a deterministic robustness test (random corruption of the fixtures in CI); `cargo fuzz` is added when it becomes available in the environment. *Update 2026-11-06:* cargo-fuzz in a separate `fuzz/` package outside the workspace that may depend on `libfuzzer-sys` and nightly (ADR-0013, rewritten; it first chose an own mutation fuzzer).

## History
- 2026-11-04, first decision: three layers (the specification, `blackbox-log`, the official `blackbox_decode` CSV as the third). Its context: "The official `blackbox_decode` (C) could not be built in the development environment; the decoder needs independent sources of truth." Layer 3 read: "**The official tool**: the `csv_conformance` test compares our raw decode with the `blackbox_decode` CSV when such files sit next to the fixtures (`<log>.<session>.csv`). The owner or CI generates them on a machine where the tool is built." Rewritten on 2026-11-09 when the comparison with both official tools (#49) showed that `blackbox_decode` lags behind the format and Explorer does not.
