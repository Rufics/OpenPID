# Blackbox: differences between Betaflight versions and test coverage

This is the readable form of `crates/openpid-fmt-blackbox/src/releases.rs`:
one description per Betaflight release line, chosen from the
`Firmware revision` header of a log. Everything the reader does differently
for a release is in that table and in no other place; a test
(`docs_name_every_release_line`) keeps this page and the table in step.

Sources: the firmware at the tags 4.2.11, 4.3.2, 4.4.3, 4.5.0, 4.5.5,
2025.12.5 and 2026.6.2 and at `master` `4b1520e`, which calls itself 2026.12
(`src/main/blackbox/blackbox.c`, `fc/rc_modes.h`, `build/debug.h`,
`io/gps.h`, `io/gps.c`, `sensors/*.h`), and the fixtures in `tests/logs/`.
Each fact in `releases.rs` and in the Betaflight profile names the tag and
file, or the fixture, it was read from. No tag of 4.0 or 4.1 was at hand:
those two lines are what the reader assumed before, and are marked so.

## How a description is chosen

| The log says | It is read as | The session notes |
|--------------|---------------|-------------------|
| a version of a line below (`4.5.1`, `2025.12.2`) | that line | nothing |
| a version newer than every line (`2027.6.0`) | the newest line | "is newer than the release lines this reader knows … read as 2026.12" |
| a version between two lines (`2026.9.0`) | the earlier of the two | "is not a release line this reader knows: it is read as 2026.6" |
| 4.6 or 2026.12, known from the development branch only | that line | "described from the development branch and the release may differ" |
| Betaflight before 4.0, Cleanflight | the rules before 4.0: the battery in 0.1 V, no settings in words, no GPS channels | nothing |
| Betaflight and no version | the same | "does not say which Betaflight version wrote it" |

A header of the log wins wherever it states the fact (a log may come from a
custom build): `rc_command_scale`, `blackbox_high_resolution`, `gyro_scale`,
`acc_1G`, `motorOutput`, the filter and gain headers, and the field
definitions themselves (names, signs, predictors, encodings). The
description says what holds when the header is absent, and what a header
means where that depends on the release (`gps_use_3d_speed`).

The forks (EmuFlight, Rotorflight, INAV) have profiles of their own; for the
battery unit and the three filter defaults they are read by the line of
their own version number, as the reader always assumed for them.

## Binary format

The field encodings (0, 1, 3, 6, 7, 8, 9, 10) and predictors (0–11) **did not
change** from 4.2 to `master`; `Data version` is 2 everywhere. A header block
with an encoding or a predictor outside these, or a predictor that refers to
a field the log does not define, is rejected with an error that names it.
One thing in the frames depends on the release:

| | 4.0 – 4.5 | 2025.12 on |
|---|---|---|
| Servos | one field, `servo[5]`, a plain number (tricopter) | `servo[0..7]` in the 8-value encoding, written as **a group of their own**: the group of the optional fields `vbatLatest` … `rssi` ends before `servo[0]` even when it has fewer than eight values |

## The release lines

What changed with each line. A cell says what the line brought; everything
else is as in the line before.

| Line | Source | Frame fields | Units and scales | Mode boxes (`flightModeFlags`, 32 bits) | Headers the reader uses |
|------|--------|--------------|------------------|------------------------------------------|-------------------------|
| 4.0 | previous reader; no tag, no fixture | the base set: `axisP/I/D/F`, `rcCommand`, `setpoint`, `vbatLatest`, `amperageLatest`, `BaroAlt`, `rssi`, `gyroADC`, `accSmooth`, `debug[0..3]`, `motor`, `servo[5]`, GPS, slow | `vbatLatest` 0.01 V, `amperageLatest` 0.01 A (before 4.0: 0.1 V); `rcCommand` ±500 and 1000…2000; `debug_mode 6` (`GYRO_SCALED`): `debug[0..2]` is the gyro before the software filters, deg/s | ARM, ANGLE, HORIZON, MAG, HEADFREE, PASSTHRU, FAILSAFE, GPSRESCUE, ANTIGRAVITY, … VTXPITMODE (30), USER1 (31, not verified) | `gyro_lowpass_hz` and the other pre-4.3 filter names; `d_min`; `motorOutput`, `gyro_scale`, `vbatref`; defaults when absent: no dynamic low-pass curve, no RPM-notch fade, one dynamic notch |
| 4.1 | previous reader; no tag, no fixture | — | — | — | — |
| 4.2 | tag 4.2.11; fixtures 4.2.0, 4.2.8, 4.2.11 | — | confirmed: 0.01 V, 0.01 A, `GYRO_SCALED` is 6 | confirmed, and bit 31 is **PARALYZE** (the user boxes are boxes 32 on and never logged) | dynamic low-pass curve 5 when not logged |
| 4.3 | tag 4.3.2 | — | — | — | filter names `gyro_lpf1_*`, `dterm_lpf1_*` (4.2: `gyro_lowpass_*`, `dterm_lowpass_*`); `ff_weight` (4.2: `feedforward_weight`); `rpm_filter_*` (4.2: `gyro_rpm_notch_*`); new: `fields_disabled_mask`, `dyn_notch_count`, `motor_poles`, `simplified_*`, `dyn_idle_*`, `vbat_sag_compensation`; defaults when absent: RPM-notch fade 50 Hz, three dynamic notches |
| 4.4 | tag 4.4.3 | `BaroAlt` → `baroAlt` | `blackbox_high_resolution`: gyro, `setpoint[0..2]` and `rcCommand` ten times finer when 1 | — | `blackbox_high_resolution`; `gps_use_3d_speed` is logged, but `GPS_speed` stays the speed over ground |
| 4.5 | tags 4.5.0, 4.5.5; fixtures 4.5.0, 4.5.1 | `gyroUnfilt[0..2]`, `eRPM[0..7]`, `debug[4..7]`; `GPS_altitude` signed in both tags (the 4.5.0 fixture still logs it unsigned: the field definition of the log decides) | — | — | `gyro_lpf1_dyn_expo`, `dterm_lpf1_dyn_expo`, `rpm_filter_weights` |
| 4.6 | previous reader; the working name of 2025.12, no tag, no fixture | as 4.5 | no `GYRO_SCALED` debug mode | as 4.5 | as 4.5 |
| 2025.12 | tag 2025.12.5; fixture 2025.12.2 | `axisS` (wing), `imuQuaternion[0..2]` (int16 × 1/32767), `servo[0..7]`, `GPS_velned[0..2]`, `GPS_home[2]` | `debug_mode 6` is `RC_INTERPOLATION`: no gyro from `debug` | ALTHOLD (4), CHIRP (6), POSHOLD (9) come in: ARM, ANGLE, HORIZON, MAG, ALTHOLD, HEADFREE, CHIRP, PASSTHRU, FAILSAFE, POSHOLD, GPSRESCUE, ANTIGRAVITY, … FLIPOVERAFTERCRASH (30), PREARM (31) | `d_max` instead of `d_min`; no `minthrottle` (only `motorOutput`); `motor_idle` for `dshot_idle_value`; `gps_use_3d_speed:1` now makes `GPS_speed` the **3D speed** |
| 2026.6 | tag 2026.6.2; no fixture | `GPS_time`, `GPS_home_epoch` | — | AUTOPILOT (11) after GPSRESCUE; PREARM leaves the 32 bits | — |
| 2026.12 | `master` at `4b1520e`; not released, no fixture | `pitot[0..1]` | `rcCommand` is a **fraction of full deflection** (−1 … 1; the throttle 0 … 1) times `rc_command_scale` (1000), also when the header is absent | as 2026.6 | `rc_command_scale`, `debug_mode_name` |

Whatever the line, the `RcCommand` channels come out as ±500 and 1000…2000
and the sticks in percent, so they mean the same for every version.

## What the frames beside the main ones become

From 4.0 on the GPS frames, the home frames and the slow frames are channels
on the timeline of the main frames (each value held until the next; no value
before the first), and columns of `openpid decode --scaled`. The tables stay
in the session as they were.

| Logged field | Channel | Unit | Since | Source |
|--------------|---------|------|-------|--------|
| G `time` | `gps.fix_time` | µs, the log's clock | 4.0 | `writeGPSFrame`: the time since the last main frame; the 32-bit wrap between the two is handled |
| `GPS_numSat` | `gps.satellites` | — | 4.0 | `io/gps.h` |
| `GPS_coord[0..1]` | `gps.latitude`, `gps.longitude` | deg (÷ 1e7) | 4.0 | logged against the home point, absolute once decoded |
| `GPS_altitude` | `gps.altitude` | m (÷ 10) | 4.0 | `altCm / 10`, tags 4.2.11 on; unsigned up to 4.4, as the log's field definition says |
| `GPS_speed` | `gps.ground_speed`, or `gps.speed_3d` from 2025.12 with `gps_use_3d_speed:1` | m/s (÷ 100) | 4.0 | `io/gps.c`: cm/s (the comment in `io/gps.h` of 4.x says 0.1 m/s; the assignments say cm/s) |
| `GPS_ground_course` | `gps.course` | deg (÷ 10) | 4.0 | `io/gps.h` |
| `GPS_velned[0..2]` | `gps.velocity_north/east/down` | m/s (÷ 100) | 2025.12 | `io/gps.h` |
| `GPS_time` | `gps.time_of_week` | s (÷ 1000) | 2026.6 | `gpsSol.time`, "GPS msToW" |
| `GPS_home[0..1]` | `gps.home_latitude`, `gps.home_longitude` | deg | 4.0 | `writeGPSHomeFrame` |
| `GPS_home[2]` | `gps.home_altitude` | m (÷ 10) | 2025.12 | |
| `GPS_home_epoch` | `gps.home_time` | s since 1970 UTC | 2026.6 | `gpsDateTimeToEpoch` |
| S `stateFlags` | `custom:stateFlags` | flags | 4.0 | `fc/runtime_config.h`: bit 0 home point set, bit 1 GPS fix, bit 2 GPS fix ever |
| S `failsafePhase` | `custom:failsafePhase` | — | 4.0 | `flight/failsafe.h`: 0 idle, 1 signal loss detected, 2 landing, 3 landed, 4 monitoring, 5 recovered, 6 GPS rescue, 7 autopilot (`master`) |
| S `rxSignalReceived`, `rxFlightChannelsValid` | `custom:…` | 0 or 1 | 4.0 | `loadSlowState` |
| S `flightModeFlags` | the session's flight modes | | | the mode boxes of the line |

Main-frame fields of Betaflight with a unit of their own (the channel keys are
the logged names, `custom:<name>`): `pitot[0]` cm/s and `pitot[1]` Pa
(`sensors/pitot.h`), `servo[n]` µs (`flight/servos.c`), `surfaceRaw` cm, −1
out of range (`sensors/rangefinder.c`), `magADC[n]` sensor counts
(`sensors/compass.c`), `baroAlt` cm.

The other firmwares keep their GPS and slow tables without channels: their
units were not checked against their sources.

## Headers in words

Setup info puts into words, besides the tuning settings: **Logging** (the
resolution, what `rcCommand` holds) and **GPS** (which speed is logged, how
the home point is taken), the two groups that decide how the numbers of the
log are to be taken. The other groups a Betaflight log can carry (GPS rescue,
altitude and position hold, autopilot, wing, chirp, RC smoothing, hardware
and receiver ids, features; about 170 of the 277 names of `master`) change
nothing about how the data is read and are shown verbatim under "All
headers".

## Reading on through damage

- **A gap without a "logging resume" event** (the firmware drops frames while
  the storage is busy): the I frame after it is outside the window of 5000
  iterations and 10 s. It becomes the new reference when (1) its iteration
  is a multiple of `I interval` and after the last frame, (2) iteration and
  time moved together across the gap at the rate of the log (within a factor
  of 1.25), and (3) the frames after it read on cleanly to the next I frame,
  which agrees with it at the same rate. Junk and the remains of another
  flight do not pass.
- **A hole of more than 16 KiB** inside a log: the same three rules find the
  I frame the log goes on with; only when there is none is the rest of the
  file "trailing bytes".
- **The 32-bit time counter** wraps every 71.6 minutes: main frames go on
  from the last accepted main frame; a GPS frame and an event take the time
  nearest to it and change nothing; a "logging resume" event takes the time
  its iteration count puts it at, so a pause may be longer than a round.

The session notes every gap and hole it read across.

The events (`E` frames) are stable: `SYNC_BEEP=0`, `INFLIGHT_ADJUSTMENT=13`,
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
"corrupt" count was the attempts to decode garbage byte by byte. After 16 KiB
without a single accepted main frame (`GIVE_UP_BYTES`) the parser stops
trying every byte: it looks for an I frame the log goes on with (see "Reading
on through damage") and, when there is none, records the tail in
`diagnostics.notes` as `trailing bytes`. The truncated fixture has no such
tail and decodes with one corrupt frame.

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
