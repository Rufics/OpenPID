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
| 4.5 | `gyroUnfilt[0..2]`, `eRPM[0..7]`, `debug[4..7]` | `gyro_lpf1_dyn_expo`, `dterm_lpf1_dyn_expo`, `rc_smoothing_rx_smoothed`, `motor_poles` | `gyroUnfilt` → `Gyro{Raw}`, `eRPM` → `Erpm` and the derived `MotorRpm` through `motor_poles` |
| 4.0–4.5, `debug_mode 6` (`GYRO_SCALED`) | `debug[0..2]` = the gyro before the software filters, deg/s | — | a derived `Gyro{Raw}` from `debug[0..2]` when `gyroUnfilt` is not logged (fixtures 4.2.0, 4.5.0, 4.5.1) |
| 2025.12 | `axisS` (wing), `imuQuaternion[0..2]` (int16 × 1/32767), `GPS_velned` | versions of the form `2025.12.x`; `d_max` instead of `d_min`; `minthrottle` no longer written (only `motorOutput`); `DeviceUID`, `motor_kv`, `gyro_debug_axis`; `fields_disabled_mask` gained the ATTITUDE bit (8) and ACC moved | the version parser accepts year.month; `FirmwareVersion` compares lexicographically, so `2025.12 > 4.5` |
| 2026.6 | `GPS_time`, `GPS_home_epoch` (G/H frames) | — | aux tables only |
| after 2026.6 (`master` at `4b1520e`, version 2026.12) | `rcCommand` is a fraction of full deflection (−1 … 1; the throttle 0 … 1) times 1000, where it was ±500 and 1000 … 2000 (`fc/rc_controls.c`, `blackbox.c`); `pitot[0..1]` | `rc_command_scale` (1000); `debug_mode_name`, `Build key`, `Release name`; no `magPID` | with `rc_command_scale` the profile brings `rcCommand` back to ±500 and 1000 … 2000, so the `RcCommand` channels and the sticks mean the same for every version. No log of such a build is among the fixtures: checked against the firmware source and with the header added to the 2025.12 fixture |

The events (`E` frames) are stable too: `SYNC_BEEP=0`, `INFLIGHT_ADJUSTMENT=13`,
`LOGGING_RESUME=14`, `DISARM=15`, `FLIGHTMODE=30`, `LOG_END=255`.

## Writing: what differs between the target releases

The writer (`openpid_fmt_blackbox::write`, `docs/DESIGN.md` §5) writes a log
for one of seven targets. This is everything that depends on the target, with
where it was read. All paths are in the Betaflight repository; "the tag" is
`4.2.11`, `4.3.2`, `4.4.3`, `4.5.0`…`4.5.5`, `2025.12.1`…`2025.12.5`,
`2026.6.1`, `2026.6.2`, and `master` at `4b1520e`. The patch releases of 4.5,
2025.12 and 2026.6 were compared and do not differ in any of it, so a target
is a version (of 4.2, 4.3 and 4.4 one tag was read).

| | 4.2 | 4.3 | 4.4 | 4.5 | 2025.12 | 2026.6 | master (2026.12-dev) | Read from |
|---| --- |---|---|---|---|---|---|---|
| `Firmware type` | Cleanflight | Cleanflight | Cleanflight | Cleanflight | Betaflight | Betaflight | Betaflight | `blackbox.c`, `blackboxWriteSysinfo` |
| Version in `Firmware revision` | `4.2.11` | `4.3.2` | `4.4.3` | `4.5.5` | `2025.12.2` | `2026.6.2` | `2026.12.0` | `build/version.h` |
| System-information lines a build with every option can write | 112 | 136 | 172 | 191 | 242 | 233 | 275 | `blackboxWriteSysinfo`, `fc/parameter_names.h` |
| `minthrottle` line | yes | yes | yes | yes | — | — | — | the same |
| `DeviceUID`, `motor_kv` lines | — | — | — | — | yes | yes | yes | the same |
| D range line | `d_min` | `d_min` | `d_min` | `d_min` | `d_max` | `d_max` | `d_max` | the same |
| `blackbox_high_resolution` (×10 on gyro, setpoint, rcCommand) | — | — | yes | yes | yes | yes | yes | `blackboxInit`, `loadMainState` |
| `rcCommand` units | ±500, 1000…2000 | ±500, 1000…2000 | the same | the same | the same | the same | fraction × 1000, with `rc_command_scale:1000` | `loadMainState`, `BLACKBOX_RC_COMMAND_SCALE` |
| `debug_mode_name` line | — | — | — | — | — | — | yes | `blackboxWriteSysinfo` |
| Main-frame fields (all conditions true) | 49 | 49 | 49 | 64 | 77 | 77 | 79 | `blackboxMainFields` |
| Barometer field | `BaroAlt` | `BaroAlt` | `baroAlt` | `baroAlt` | `baroAlt` | `baroAlt` | `baroAlt` | the same |
| `gyroUnfilt[0..2]`, `eRPM[0..7]` | — | — | — | yes | yes | yes | yes | the same |
| `debug[]` slots | 4 | 4 | 4 | 8 | 8 | 8 | 8 | `DEBUG16_VALUE_COUNT` |
| `axisS[0..2]` (wing, S gain ≠ 0), `imuQuaternion[0..2]` | — | — | — | — | yes | yes | yes | `blackboxMainFields` |
| Servos | `servo[5]` of a tricopter, signed VB | `servo[5]` of a tricopter, signed VB | the same | the same | `servo[0..7]`, TAG8_8SVB, predictor 1500 | the same | the same | the same, `writeIntraframe` |
| Servo fields obey their mask bit | (no mask) | (no bit) | (no bit) | (no bit) | no: `hasServos() && (FIELD_SELECT(SERVO))` tests the number of the bit | no | yes | `testBlackboxConditionUncached` |
| `pitot[0..1]` | — | — | — | — | — | — | yes | `blackboxMainFields` |
| `fields_disabled_mask` bits | no mask: every field whose sensor or gain is there is logged | PID 0, RC 1, SETPOINT 2, BATTERY 3, MAG 4, ALTITUDE 5, RSSI 6, GYRO 7, ACC 8, DEBUG 9, MOTOR 10, GPS 11 | the same | + RPM 12, GYROUNFILT 13 | ATTITUDE 8 inserted (ACC 9, DEBUG 10, MOTOR 11, GPS 12, RPM 13, GYROUNFILT 14), + SERVO 15 | the same | + PITOT 16 | `blackbox.h` (to 4.5), `blackbox_fielddefs.h` |
| `G` frame fields | 7 | 7 | 7 | 7 | 10 (+ `GPS_velned[0..2]`) | 11 (+ `GPS_time`) | 11 | `blackboxGpsGFields` |
| `GPS_altitude` | unsigned VB | unsigned VB | unsigned VB | signed VB | signed VB | signed VB | signed VB | the same, `writeGPSFrame` |
| `H` frame fields | 2 | 2 | 2 | 2 | 3 (+ `GPS_home[2]`) | 4 (+ `GPS_home_epoch`) | 4 | `blackboxGpsHFields` |
| `P ratio` of a log without P frames | 0 (the setting itself) | undefined (a division by zero; the writer writes 0) | the same | the same | the same | the same | 0 | `blackboxGetPRatio` |
| `P interval` | `I interval` / `P ratio`, any ratio (the setting `blackbox_p_ratio`; 0 for none) | one of 1, 2, 4, 8, 16 (`blackbox_sample_rate`), 0 when that exceeds the `I interval` | the same | the same | the same | the same | the same | `blackboxInit` |
| `P ratio` line | the setting | `I interval` / `P interval` | the same | the same | the same | the same | the same | `blackboxWriteSysinfo` |
| `G` frame has `time` | unless `P ratio` is 1 | unless `P interval` equals `I interval` | the same | the same | the same | the same | the same | `testBlackboxConditionUncached` |
| Low-pass, feedforward and RPM-filter lines | `gyro_lowpass_hz`, `dterm_filter_type`, `feedforward_weight`, `gyro_rpm_notch_*` | `gyro_lpf1_static_hz`, `dterm_lpf1_type`, `ff_weight`, `rpm_filter_*` | the same | the same | the same | the same | the same | `blackboxWriteSysinfo` |

The same in every target, and so not in the table: the encodings and
predictors; the `S` frame (five fields); the events and their layout
(`blackboxLogEvent`); the schedule of the frames (`blackboxAdvanceIterationTimers`,
`blackboxShouldLogPFrame`, `blackboxShouldLogIFrame`: an `I` frame at every
multiple of the `I interval`, `P` frames every `P interval` counted from it,
whatever the two intervals are); the rounding of the rates (`sensors/gyro_init.c`: the gyro and PID
periods are whole microseconds, rounded down; `blackboxInit`:
`I interval = 32000 / PID period`, rounded down, so 102 at 3.2 kHz and 205 at
6.4 kHz); the slow-frame rules (`writeSlowFrameIfNeeded`: before a `P` frame
when the state changed or 256 `I` intervals passed, forced at the start and
after a resume; before `I` frames only with `P interval:0`); the home
frame before the fix when the home point moved (`blackboxShouldLogGpsHomeFrame`).

How the tables were verified against bytes a flight controller wrote:

| Target | Field tables | Header lines | Frames |
|---|---|---|---|
| 4.2 | source, and the 4.2 fixtures | the 4.2.0, 4.2.8 and 4.2.11 fixtures have exactly the 112 lines of the table (4.2.8 without `currentSensor`: no ADC current meter) | 46 logs of the three fixtures come back byte for byte; the 47th ends after its headers and comes back as headers |
| 4.3, 4.4 | the firmware source only (no fixture with an MIT-compatible licence) | source only | round trips through our reader and the independent decoder |
| 4.5 | source, and the 4.5.1 fixture | the 4.5.1 fixture has exactly the 191 lines of the table, in order (an official build with every option) | both sessions of the 4.5.1 fixture come back byte for byte |
| 2025.12 | source, and the 2025.12.2 fixture | the 2025.12.2 fixture has exactly the lines the table gives a wing build with GPS, compass, barometer and chirp | the 2025.12.2 fixture (39 481 main frames, 398 GPS frames, 7 slow frames, 5 events) comes back byte for byte |
| 2026.6, master | source only (no log exists yet) | source only | round trips through our reader |

`betaflight-4.5.0-gps-rescue.bbl` names itself 4.5.0 but was built from the
development branch before the release: it lacks `ez_landing_speed` and logs
`GPS_altitude` unsigned. Written for 4.5 it comes back with exactly those
differences (`tests/rewrite.rs`). `betaflight-4.2.11-error-recovery.bbl` is
corrupted on purpose and is not written back.

`tools/betaflight-blackbox/extract.py` reads the field definitions and the
header lines out of a Betaflight checkout at a tag; its README says how a
release is added.

The header lines are one table, `write/sysinfo/lines.txt`: every line some
release writes, once, in the firmware's order, with the releases that write
it (`all`, `4.3-`, `4.4-4.5`, …) and the build options it depends on. A new
release that writes what the last one wrote needs no edit there.

The table holds no values, and the writer has none of its own: a header line
is written when its value is known (from the release, from the headers a
decoded session carries, from the session's settings, from what the frames
need, or from the caller) and left out otherwise. `docs/DESIGN.md` §5 lists
which lines that is. A caller that knows more passes the lines in
`WriteOptions::headers`; each lands where the table has it
(`Target::header_names`).

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
