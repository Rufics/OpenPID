# Reference data from Blackbox Explorer

The decoder's regression reference (ADR-0005) is the parser of **Blackbox
Explorer**, the log viewer the firmware projects maintain:

| Logs | Explorer | Loader |
|------|----------|--------|
| Betaflight, INAV, EmuFlight | [`betaflight/blackbox-log-viewer`](https://github.com/betaflight/blackbox-log-viewer) | `betaflight.mjs` |
| Rotorflight | [`rotorflight/rotorflight-blackbox`](https://github.com/rotorflight/rotorflight-blackbox) | `rotorflight.mjs` |

`run.sh` decodes every Blackbox fixture in `tests/logs/` with the matching
Explorer and writes `tests/logs/explorer/<fixture>.json`;
`crates/openpid-cli/tests/explorer_conformance.rs` compares our raw decode
with it.

## Updating the reference

Needs Node.js ≥ 20.6 and npm. From the repository root:

```sh
git clone https://github.com/betaflight/blackbox-log-viewer ../blackbox-log-viewer
git clone https://github.com/rotorflight/rotorflight-blackbox ../rotorflight-blackbox
sh tools/blackbox-explorer/run.sh ../blackbox-log-viewer ../rotorflight-blackbox
cargo test -p openpid-cli --test explorer_conformance
git add tests/logs/explorer
```

Do it after a new Blackbox fixture is added, or to move to a newer Explorer
(each reference file names the Explorer commit it came from). Nothing of
Explorer is built or installed: the loaders take its parser modules straight
from the checkout, so its own dependencies are not needed apart from `semver`,
which `npm ci` installs here (`package-lock.json`).

## What is recorded

The loaders drive Explorer's `FlightLogParser` the way its `FlightLogIndex`
does: `parseHeader`, then `parseLogData` for every log in the file. Every valid
main frame, every slow, GPS and GPS-home frame (each with the number of main
frames before it) and every event are recorded. Tables are stored as FNV-1a 64
digests per field and per 1024 rows, so whole logs are compared without
committing megabytes, and a difference is still located to a field and a range
of frames; events are stored in full.

To see the frames themselves, add `--csv <dir>` to a loader:

```sh
node tools/blackbox-explorer/betaflight.mjs ../blackbox-log-viewer label /tmp/out.json \
  tests/logs/<fixture> --csv /tmp
```

## One line of Explorer is reverted

Betaflight's Explorer PR #862 (2025-09-29, commit `4f60b5e`, "Review sonar")
replaced `(value | 0)` with `Math.trunc(value)` in the `MINTHROTTLE` and
`MINMOTOR` predictors of `flightlog_parser.js`. The comment right above both
lines still says the value must be forced to a *signed* 32-bit integer: the
firmware writes `motor[0] - minthrottle` with unsigned encoding, so a motor
below minthrottle arrives as a large unsigned number. Without the wrap,
`motor[0]` of a disarmed INAV quad reads 4 294 968 296 instead of 1000, and the
error carries into the following P frames through the predictors.

`hooks.mjs` puts the two `| 0` back in memory while loading the module (the
checkout is not changed) and stops with an error if the lines are not found,
so a fix or a rewrite upstream is noticed. With the revert, Explorer master
matches our decoder; Explorer before #862 (`4f60b5e^`) matches it unmodified on
the Betaflight, INAV and EmuFlight fixtures (EmuFlight apart from the frames lost
to its cut header, see the test); the Rotorflight fork still has the `| 0`.

## Licence

Blackbox Explorer and its Rotorflight fork are GPL-3.0. They are used here only
as external programs at the time the reference data is made: no Explorer code
is copied into this repository or linked into OpenPID. The scripts in this
directory are OpenPID's own (MIT).
