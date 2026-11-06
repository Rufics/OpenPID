# ADR-0010: ArduPilot DataFlash as a separate crate `openpid-fmt-ardupilot`

- Status: accepted
- Date: 2026-11-04

## Context

Phase 2 (decision Q2) adds ArduPilot, then INAV. DataFlash is not Blackbox:
another container (self-describing `FMT` packets), several independent message
streams instead of one frame per iteration, parameters in `PARM` instead of
headers, other units (rad/s, m/s², centidegrees in old versions).

## Decision

- **A separate crate** `openpid-fmt-ardupilot` with its own `LogFormat` and
  `FirmwareProfile` (`ArdupilotProfile`), without new dependencies (only
  `openpid-core`). The packet decoder (`reader`) does not interpret the data;
  `session` builds the `FlightSession`; `profile` maps `MSG.Col` → channel and
  `PARM` → `Settings`. The same split as in `openpid-fmt-blackbox`.
- **One timebase**: the fastest of `IMU[0]`, `RATE`, `PIDR`; the other messages
  are sample-and-hold with a validity mask. No interpolation: it invents values
  the controller never saw; the analyses need an honest mask.
- **Export by an allow-list** of the messages relevant to tuning; the rest is
  only counted in the diagnostics. Unknown columns of the exported messages
  become `custom:` with the unit and multiplier from `FMTU`/`UNIT`/`MULT`.
- **Units** are brought to the OpenPID core (deg/s, g, %, V, A, mAh); the
  differences between Copter and Plane (rad/s vs deg/s in the PID messages,
  motors vs servos in `RCOU`) are chosen by the firmware string.
- **Fixtures**: three MIT logs (Copter 3.3, Plane 4.3.5, QuadPlane 4.4.4) in
  Git LFS; byte-level agreement is checked against an independent Python
  decoder (`tests/golden/*.rows.json`) written from the same format description.

## Consequences

- `openpid-cli`/`openpid-app` register the format; a `.bin` opens like a `.bbl`.
- The step-response quality depends on the `RATE`/`PIDR` rate in the log (see
  `docs/ARDUPILOT.md`); that is a property of ArduPilot logs, not of the decoder.
- Filter Sim for ArduPilot waits for the batch-sampler reader (`ISBH/ISBD`), a
  separate phase-2 task.
