# Test logs

Real Blackbox logs used by the conformance, golden, version and GUI tests.
They are stored in **Git LFS** (`.gitattributes`); after cloning run
`git lfs install && git lfs pull` (CI does this itself, with a cache keyed on
the file contents). Every file is redistributable under an MIT-compatible
licence; the table names the source so the attribution stays with the data.

| File | Firmware | Source | Licence | Notes |
|------|----------|--------|---------|-------|
| `betaflight-4.2.0.BFL` | Betaflight 4.2.0 | [fc-blackbox](https://github.com/ilya-epifanov/fc-blackbox) `LOG00037.BFL` | MIT / Apache-2.0 | single log |
| `betaflight-4.2.8-44-logs.bbl` | Betaflight 4.2.8 | fc-blackbox `btfl_all.bbl` | MIT / Apache-2.0 | 44 short logs (arming sessions) |
| `betaflight-4.2.11-error-recovery.bbl` | Betaflight 4.2.11 | [blackbox-log](https://github.com/blackbox-log/blackbox-log) `tests/logs/error-recovery.bbl` | MIT / Apache-2.0 | deliberately corrupted frames |
| `betaflight-4.2.11-two-logs.bbl` | Betaflight 4.2.11 | fc-blackbox `btfl_001.bbl` | MIT / Apache-2.0 | two logs in one file |
| `betaflight-4.5.0-gps-rescue.bbl` | Betaflight 4.5.0 | [propwash](https://github.com/Iteratrix/propwash) `tests/fixtures/fc-blackbox/btfl_gps_rescue.bbl` | MIT | 1 s, simplified-tuning headers (sliders), GPS rescue |
| `betaflight-4.5.1-two-sessions.bbl` | Betaflight 4.5.1 | [blackbird](https://github.com/PedroS235/blackbird) `tests/fixtures/eight_logs_in_one.bbl`, first two sessions | MIT | 3 s + 21 s at 1 kHz, `gyroUnfilt`, eRPM |
| `betaflight-2025.12.2-btfl_027.bbl` | Betaflight 2025.12.2 | propwash `tests/fixtures/fc-blackbox/btfl_027.bbl` | MIT | 40 s, D-max scheme, sliders |
| `rotorflight-4.3.0-LOG246.TXT` | Rotorflight 4.3.0 | propwash `tests/fixtures/rotorflight/LOG246.TXT` | MIT | helicopter, 500 Hz, a few corrupt frames in the source |
| `emuflight-3.7.0-truncated.BFL` | EmuFlight 3.7.0 | fc-blackbox `crashing-LOG00002.BFL`, first 1 MiB | MIT / Apache-2.0 | truncated mid-frame on purpose |
| `inav-3.0.1-truncated.TXT` | INAV 3.0.1 | fc-blackbox `LOG00004.TXT`, first 1 MiB | MIT / Apache-2.0 | `P interval:1/2`, GPS and slow frames; INAV is a phase-2 target |
| `inav-9.0.1-truncated.TXT` | INAV 9.0.1 | propwash `tests/fixtures/inav/LOG00005.TXT`, first 1 MiB | MIT | phase-2 target; the decoder still reports corrupt frames on it |
| `ardupilot-copter-3.3-dronekit.bin` | ArduCopter 3.3-dev | propwash `tests/fixtures/ardupilot/dronekit-copter-log171.bin` (originally dronekit-la, Apache-2.0), first 1 MiB | MIT / Apache-2.0 | legacy DataFlash (`TimeMS`, no `FMTU`), IMU 50 Hz, RATE 10 Hz |
| `ardupilot-plane-4.3.5.bin` | ArduPlane 4.3.5 | propwash `tests/fixtures/ardupilot/pyflightcoach-plane-v4.3.5.bin` | MIT | bench log, `PIDR/P/Y`, `FMTU` units |
| `ardupilot-quadplane-4.4.4-esc-telemetry.bin` | ArduPlane 4.4.4 (QuadPlane) | propwash `tests/fixtures/ardupilot/esc-telem-quadplane-v4.4.4.bin`, first 1 MiB | MIT | IMU 300 Hz, ESC telemetry (5 instances), harmonic notch, `VER` |

Still wanted (no permissively licensed public log found as of 2026-11-04):
Betaflight 4.3.x, 4.4.x, 2026.6.x and EmuFlight 0.4.x. See
`docs/BLACKBOX-VERSIONS.md`. Logs from repositories without a licence, or under
GPL / non-commercial terms, are not accepted, even when an MIT project has
already copied them.

The project owner's own logs are reserved for final verification and are not
part of this directory.
