# ADR-0004: An own Blackbox decoder instead of the `blackbox-log` crate

Status: accepted (2026-11-04)

## Context
The crate `blackbox-log` 0.4.3 (MIT/Apache-2.0, Betaflight + INAV) exists and is pure Rust. It could have been used directly.

## Decision
Write an own decoder in `openpid-fmt-blackbox`, using `blackbox-log` and the official `blackbox_decode` only as **references in the tests**.

Reasons:
- The user's requirement: an own functional core.
- `blackbox-log` pulls `uom`, `bitvec`, `hashbrown`, `time`, `tracing`, against ADR-0001.
- We need incremental decoding with progress, detailed corruption diagnostics, mapping onto `ChannelId` through a `FirmwareProfile`, EmuFlight/Rotorflight profiles and, later, other formats through the same trait.
- The format is publicly documented; the reference implementations (C, JS) are open for checking behaviour (not for copying code).

## Consequences
- Conformance tests against the `blackbox_decode` CSV are mandatory from phase 0.
- Fuzzing of the decoder is mandatory (the input is untrusted).

## Addendum (after the implementation)
The conformance test against `blackbox-log` 0.4.3 found two deviations of the **reference** from the firmware (checked against `blackbox.c`): it ignores `P interval` when restoring `loopIteration` (`skipped = 0; // FIXME`) and after an I frame it does not copy it into both history slots (the firmware does `history[2] = history[1] = history[0]`). Our decoder follows the firmware and the C reference; the test compares the other fields bit for bit, and the fields with the `AVERAGE_2`/`STRAIGHT_LINE` predictors on I frames only. A separate test checks that there are no jumps at the I → P boundaries. *Update 2026-11-08:* the official C `blackbox_decode` has its own deviations (it reads `P interval` only as `num/denom`, drops P frames after events it does not parse, accepts a frame cut by the end of the file); they are listed in `tests/logs/README.md` and modelled by `csv_conformance`.
