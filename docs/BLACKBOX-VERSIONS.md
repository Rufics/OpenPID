# Blackbox: differences between Betaflight versions and test coverage

Source: `src/main/blackbox/blackbox.c` and `blackbox_fielddefs.h` at the tags
4.2.11, 4.3.2, 4.4.3, 4.5.5, 2025.12.5, 2026.6.2 (compared automatically), plus
the parser of the official Blackbox Explorer
(`blackbox-log-viewer/src/flightlog_parser.js`) and its newer TypeScript
version in `betaflight-configurator/src/blackbox-viewer/`.

## Binary format

The field encodings (0, 1, 3, 6, 7, 8, 9, 10) and predictors (0–11) **did not
change** from 4.2 to 2026.6; `Data version` is 2 everywhere. Every difference
is in the set of fields and headers, that is at the `FirmwareProfile` level,
not in the decoder.

| Version | New / changed frame fields | New / changed headers | What it means for OpenPID |
|---------|----------------------------|-----------------------|---------------------------|
| 4.2 | the base set: `axisP/I/D/F`, `rcCommand`, `setpoint`, `vbatLatest`, `amperageLatest`, `BaroAlt`, `rssi`, `gyroADC`, `accSmooth`, `debug`, `motor`, `servo`, GPS, slow | `motorOutput`, `gyro_scale`, `vbatref` (centivolts) | the base profile |
| 4.3 | — | `fields_disabled_mask`, `gyro_lpf1_*`/`dterm_lpf1_*` (renamed from `*_lowpass_*`), `ff_weight`, `dyn_notch_count`, `rpm_filter_*` | the profile reads both spellings |
| 4.4 | `BaroAlt` → `baroAlt` | `blackbox_high_resolution` (×10 for gyro, setpoint, rcCommand), `sample_rate` | the scale is divided by 10 when `blackbox_high_resolution:1` |
| 4.5 | `gyroUnfilt[0..2]`, `eRPM[0..7]` | `gyro_lpf1_dyn_expo`, `dterm_lpf1_dyn_expo`, `rc_smoothing_rx_smoothed`, `motor_poles`, `rc_command_scale` (the configurator divides rcCommand by it) | `gyroUnfilt` → `Gyro{Raw}`, `eRPM` → `Erpm` and the derived `MotorRpm` through `motor_poles` |
| 4.0–4.5, `debug_mode 6` (`GYRO_SCALED`) | `debug[0..2]` = the gyro before the software filters, deg/s | — | a derived `Gyro{Raw}` from `debug[0..2]` when `gyroUnfilt` is not logged (fixtures 4.2.0, 4.5.0, 4.5.1) |
| 2025.12 | `axisS` (wing), `imuQuaternion[0..2]` (int16 × 1/32767), `GPS_velned` | versions of the form `2025.12.x`; `d_max` instead of `d_min`; `minthrottle` no longer written (only `motorOutput`); `DeviceUID`, `motor_kv`, `gyro_debug_axis`; `fields_disabled_mask` gained the ATTITUDE bit (8) and ACC moved | the version parser accepts year.month; `FirmwareVersion` compares lexicographically, so `2025.12 > 4.5` |
| 2026.6 | `GPS_time`, `GPS_home_epoch` (G/H frames) | — | aux tables only |

The events (`E` frames) are stable too: `SYNC_BEEP=0`, `INFLIGHT_ADJUSTMENT=13`,
`LOGGING_RESUME=14`, `DISARM=15`, `FLIGHTMODE=30`, `LOG_END=255`.

## Test-log coverage

Every log lives in `tests/logs/` (Git LFS, ADR-0009) under an MIT-compatible
licence; sources and notes are in `tests/logs/README.md`.

| Version | File | Source | Licence |
|---------|------|--------|---------|
| 4.2.0 / 4.2.8 / 4.2.11 | `betaflight-4.2.*` | fc-blackbox, blackbox-log | MIT / Apache-2.0 |
| 4.3.x | **none** | no public log with an MIT-compatible licence found (GitHub search 2026-11-04: GPL-3.0 or no licence only) | — |
| 4.4.x | **none** | the same | — |
| 4.5.0 | `betaflight-4.5.0-gps-rescue.bbl` | Iteratrix/propwash | MIT |
| 4.5.1 | `betaflight-4.5.1-two-sessions.bbl` | PedroS235/blackbird | MIT |
| 2025.12.2 | `betaflight-2025.12.2-btfl_027.bbl` | Iteratrix/propwash | MIT |
| 2026.6.x | **none** | no public log found | — |

The `versions.rs` test requires 4.2, 4.5 and 2025.12 and reports the rest.
Logs without an MIT-compatible licence (GPL, non-commercial, no licence) are
not added.

The plan for 2026.6: build the Betaflight SITL from the `2026.6.x` tag
(`make TARGET=SITL`; the target has `USE_BLACKBOX_VIRTUAL`,
`blackbox_device = VIRTUAL`, `blackbox_mode = ALWAYS_ON`) and record a log
without flying: the headers and the field set will be genuine. Building
third-party code is forbidden by policy in the development environment of that
session, so this is done on the owner's machine.

## INAV

The same binary format and the same encodings/predictors as Betaflight; the
differences are in the field set (`axisRate`, `fwAlt*`, `fwPos*`, `nav*`,
`servo[]`, `attitude[]`, `accVib`) and in the `S` frames (predictor 1 for
`rxUpdateRate`/`escTemperature`). Fixtures: 3.0.1 and 9.0.1 (the first MiB).

Investigation #11 (2026-11-04): the full INAV 9.0.1 log (7.8 MB) showed 1590
"corrupt" frames and 211 KB of skipped bytes. The sync-loss map
(`cargo run -p openpid-fmt-blackbox --example frame_dump -- <log>`) showed two
zero-filled gaps of ~0.5 KB in the middle (the SD buffer) and **~150 KB after
the real end of the log with bytes of another file** (the `A3 95` pairs are
ArduPilot DataFlash headers left on the card). The decoder read correctly; the
"corrupt" count was the attempts to decode garbage byte by byte. The parser now
stops after 16 KiB without a single accepted frame (`GIVE_UP_BYTES`) and
records the tail in `diagnostics.notes` as `trailing bytes`. The truncated
fixture has no such tail and decodes with one corrupt frame.

### INAV field mapping (the `inav` profile)

| Field | Channel | Unit |
|-------|---------|------|
| `axisRate[i]` | `setpoint.*` | deg/s (checked on 9.0.1: `axisRate − gyroADC == pidError`) |
| `gyroADC[i]`, `accSmooth[i]` | `gyro.filtered.*`, `acc.*` | deg/s, g (`acc_1G`) |
| `axisP/I/D/F[i]`, `pidSum[i]`, `pidError[i]` | `pid.*`, `pid_error.*` | raw, deg/s |
| `vbat`, `amperage` | `battery.voltage/current` | V, A (centi-units × 0.01) |
| `motor[i]` | `motor.i` | % by `motorOutput` |
| `rcCommand[i]` | `rc.i` | raw (±500, throttle 1000…2000) |
| `attitude[i]` | `custom:attitude.roll/pitch/yaw` | deg (decidegrees × 0.1) |
| `servo[i]`, `rcData[i]` | `custom:servo.i`, `custom:rc_raw.i` | µs |
| `BaroAlt`, `AirSpeed` | `custom:BaroAlt`, `custom:airspeed` | cm, cm/s |
| `navPos/navVel/navAcc/navTgtPos/navTgtVel[i]`, `navEPH/EPV`, `navSurf` | `custom:nav.*` | cm, cm/s, cm/s² |
| `navState`, `navFlags`, `navTgtHdg` | `custom:nav.state/flags/target_heading` | flags / raw |
| `fwAlt*`, `fwPos*` | `custom:fw_alt.*`, `custom:fw_pos.*` | raw (fixed-wing controllers) |

Settings: `rollPID` = P,I,D[,FF]; `looptime` → gyro/pid Hz, `P interval` → the
log rate; `gyro_lpf_hz`/`gyro_lpf_type`, `dterm_lpf_hz`/`dterm_lpf_type`,
`dterm_lpf2_hz`, `gyro_notch_hz/cutoff`; `dynamicGyroNotchMinHz`/`dynamicGyroNotchQ`
(Q × 0.01) → dyn notch; `rpm_gyro_*` → RPM notch; `motor_pwm_protocol` (the
same codes as Betaflight), `motorOutput`.

## Rotorflight

The same container as Betaflight, plus helicopter events and fields.
Investigation #7 (2026-11-04): all 11 "corrupt" frames of the 4.3.0 fixture
were **`E` events with code 50** (`FLIGHT_LOG_EVENT_GOVSTATE`, one byte of
governor state) the parser did not know; after each one the P frames up to the
next I frame were lost (82 desyncs, 9 time gaps). The parser now reads the
Rotorflight events 50/51/52 (governor / rescue / airborne state, 1 byte), 100
(custom data: length + bytes) and 101 (custom string, `\0`-terminated) and
returns them as `EventKind::Other { name, data }`. The fixture decodes without
corrupt frames, desyncs or gaps.

| Field | Channel | Unit |
|-------|---------|------|
| `motor[i]` | `motor.i` | % (logged in 0.1 %: 0…1000, **not** PWM) |
| `setpoint[3]`, `mixer[3]` | `custom:collective`, `custom:mixer.collective` | % (±1000 → ±100) |
| `mixer[0..2]` | `custom:mixer.roll/pitch/yaw` | % |
| `headspeed`, `tailspeed` | `custom:headspeed`, `custom:tailspeed` | rpm |
| `attitude[i]` | `custom:attitude.*` | deg (× 0.1) |
| `Vbat`, `Ibat` | `battery.voltage/current` | V, A (× 0.01) |
| `rcCommand[3]` | `rc.3`, the collective stick (±500) | raw |
| `rcCommand[4]` | → the derived `throttle` (0…1000 → %) | % |

`motor_poles` in 4.3 holds a memory address (`0x2000xxxx`) instead of the pole
count; the profile rejects values outside 2…64. The headers
`gyro_rpm_filter_bank_*`, `gyro_decimation_hz`, `rates_type`, `rate_limits`,
`P ratio`, `Board information` go to `settings.extra`.
