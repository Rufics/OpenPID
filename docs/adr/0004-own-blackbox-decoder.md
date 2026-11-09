# ADR-0004: An own Blackbox decoder instead of the `blackbox-log` crate

Status: accepted (2026-11-04, updated 2026-11-09)

## Context
The crate `blackbox-log` 0.4.3 (MIT/Apache-2.0, Betaflight + INAV) exists and is pure Rust. It could have been used directly.

## Decision
Write an own decoder in `openpid-fmt-blackbox`, using `blackbox-log` and the parser of Blackbox Explorer only as **references in the tests** (ADR-0005).

Reasons:
- The user's requirement: an own functional core.
- `blackbox-log` pulls `uom`, `bitvec`, `hashbrown`, `time`, `tracing`, against ADR-0001.
- We need incremental decoding with progress, detailed corruption diagnostics, mapping onto `ChannelId` through a `FirmwareProfile`, EmuFlight/Rotorflight profiles and, later, other formats through the same trait.
- The format is publicly documented; the reference implementations (C, JS) are open for checking behaviour (not for copying code).

## Consequences
- Conformance tests against Blackbox Explorer, on every flight log of every Blackbox fixture, are mandatory (ADR-0005).
- Fuzzing of the decoder is mandatory (the input is untrusted).

## Addendum (after the implementation)
The conformance test against `blackbox-log` 0.4.3 found two deviations of the **reference** from the firmware (checked against `blackbox.c`): it ignores `P interval` when restoring `loopIteration` (`skipped = 0; // FIXME`) and after an I frame it does not copy it into both history slots (the firmware does `history[2] = history[1] = history[0]`). Our decoder follows the firmware (and Blackbox Explorer does the same); the test compares the other fields bit for bit, and the fields with the `AVERAGE_2`/`STRAIGHT_LINE` predictors on I frames only. A separate test checks that there are no jumps at the I → P boundaries.

## History
- 2026-11-04: the test references were `blackbox-log` and "the official `blackbox_decode`", and the consequence read "Conformance tests against the `blackbox_decode` CSV are mandatory from phase 0." The addendum said "Our decoder follows the firmware and the C reference". Updated on 2026-11-09 when Blackbox Explorer became the reference and `blackbox_decode` was dropped (ADR-0005, #137).
