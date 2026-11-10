# ArduPilot DataFlash: how a log becomes an OpenPID session

The `openpid-fmt-ardupilot` crate reads ArduPilot binary logs (`.bin` from
the SD card or downloaded over MAVLink). The format is self-describing: `FMT`
packets give the name, length, column names and types of every other packet;
`FMTU`/`UNIT`/`MULT` (since 4.0) add units and multipliers. The implementation
follows the format description (`libraries/AP_Logger/LogStructure.h`) without
borrowing code.

## Timebase

Every message is written at its own rate: `IMU` at hundreds of Hz,
`RATE`/`PIDR` at 10…400 Hz, `BAT` at 10 Hz. OpenPID keeps every channel on one
timebase, so:

- the base is the fastest of `IMU` (instance 0), `RATE`, `PIDR`;
- the other messages are held (sample-and-hold): at every base point a channel
  carries the last value logged no later than that point, and it is invalid
  before its first value (validity mask).

Consequence: the step response needs setpoint and gyro at similar rates. In
logs where `RATE`/`PIDR` are written at 10 Hz (the default `LOG_BITMASK`) the
response is stair-stepped; for tuning, enable fast logging (`LOG_BITMASK` bits
`ATTITUDE_FAST`, `PID`).

## Field mapping

| Field | Channel | Unit | Note |
|-------|---------|------|------|
| `IMU.GyrX/Y/Z` | `gyro.filtered.*` | deg/s | rad/s × 57.3; in `IMU` the gyro is already behind `INS_GYRO_FILTER` |
| `IMU.AccX/Y/Z` | `acc.*` | g | m/s² ÷ 9.80665 |
| `RATE.RDes/PDes/YDes` | `setpoint.*` | deg/s | Copter < 3.4: centidegrees/s × 0.01 |
| `RATE.R/P/Y`, `ROut/…` | `custom:rate.*`, `custom:rate_out.*` | deg/s, — | |
| `PIDR/PIDP/PIDY.Tar` | `setpoint.*`, or `custom:PIDR.Tar` … when `RATE.*Des` already gave the setpoint (the first field of a channel wins) | deg/s | Copter: rad/s × 57.3; Plane: already deg/s |
| `PID*.Err` | `pid_error.*` | deg/s | the same scaling |
| `PID*.P/I/D/FF` | `pid.p/i/d/f.*` | raw | controller output |
| `RCOU.C1…` | Copter: `motor.N` (%, by `MOT_PWM_MIN/MAX`); Plane: `custom:servo.N` (µs) | | |
| `RCIN.C1…` | `rc.N` | µs | |
| `CTUN.ThO` / `ThrOut` | `throttle` | % | Copter 4.x: 0…1 × 100; 3.x: 0…1000 × 0.1; Plane: already % |
| `BAT`/`CURR` | `battery.voltage/current/consumed` | V, A, mAh | `CURR` (3.x) in centivolts/centiamperes |
| `ESC[i].RPM` | `motor_rpm.i` | rpm | physical rpm from the ESC telemetry (not eRPM) |
| `ATT.*` | `custom:attitude.*` | deg | |
| other columns of the exported messages | `custom:MSG.Col` | from `FMTU` | the multiplier from `MULT`; without `FMTU` by the format character (`c`/`e` → ×0.01, `L` → ×1e-7) |

Exported messages: `IMU`, `RATE`, `PIDR/P/Y`, `RCOU`, `RCIN`, `CTUN`, `ATT`,
`BAT`/`CURR`, `ESC` (every instance). The rest (GPS, EKF, BARO, …) stays out of
the channels; their names and counts are in `diagnostics.notes`. `MODE` and
`EV` become events.

## Batch sampler (`ISBH`/`ISBD`): the gyro before the filter

`IMU.Gyr*` has already passed `INS_GYRO_FILTER`. The unfiltered samples are
written by the batch sampler (`INS_LOG_BAT_MASK` ≠ 0): `ISBH` opens a batch
(`N`, `type` 0 = accelerometer / 1 = gyro, `instance`, the multiplier `mul`,
`smp_cnt`, `SampleUS`, `smp_rate`), then `ISBD` carries 32 `int16` samples
each (`x`, `y`, `z`, `seqno` = position in the batch); a value is `int16 / mul`
in rad/s or m/s². The batches come in bursts (in the QuadPlane fixture 1024
samples at 3276 Hz, that is 0.31 s, every ~0.7 s), and with `INS_LOG_BAT_OPT`
bit 2 the pre- and post-filter copies alternate; the "post" ones are logged
with `instance + number of IMUs` (the ArduPilot tools tell them apart the same
way). A batch whose `N` has no `ISBD` keeps invalid samples; a truncated log
leaves an incomplete last batch.

The timebase decision: the batches are **resampled onto the session base**,
not held: every base point gets the mean of the batch samples within ± half a
base period (box anti-aliasing one period wide; when the base is faster than
the batch, the nearest sample), and outside the batches the channel is
invalid. So `gyro.raw` can be compared with `gyro.filtered` on the same samples
(the PSD shows the roll-off of `INS_GYRO_FILTER`) at the cost of bandwidth: on
a 300 Hz base only up to 150 Hz remains. The full batch rate (a separate
timebase) will be needed by Filter Sim; that is a separate decision in phase 2b.

| Batch | Channel | Unit |
|-------|---------|------|
| gyro, pre-filter, the timebase IMU | `gyro.raw.*` | deg/s |
| accelerometer, pre-filter, the timebase IMU | `custom:acc.raw.*` | g |
| the rest (another IMU, or post-filter) | `custom:isb[i].{gyro,acc}.{pre,post}.*` | deg/s, g |

`diagnostics.notes` records the number of batches, the rate, the dropped
samples and the base coverage of every channel in percent.

## Settings from `PARM`

| What | Parameters |
|------|------------|
| PID per axis | Copter: `ATC_RAT_{RLL,PIT,YAW}_{P,I,D,FF}` (3.x: `RATE_*`); Plane: `{RLL,PTCH,YAW}_RATE_*` (without a yaw controller: zeros) |
| Rates | `SCHED_LOOP_RATE` → pid_hz; `INS_GYRO_RATE` → gyro_hz (1/2/4/8 kHz); log_hz is the timebase rate |
| Gyro filter | `INS_GYRO_FILTER` → lowpass1 (second-order biquad) |
| D-term | `*_FLTD` → dterm lowpass1 (PT1) |
| Harmonic notch | `INS_HNTCH_*`: mode 2/3 (RPM/ESC) → `rpm_notch`; 4 (FFT) → `dyn_notch`; 0/1 (static / by throttle) → a static `notch`; every parameter in `settings.extra` |
| Motors | `MOT_PWM_MIN/MAX`, `MOT_PWM_TYPE`, `FRAME_CLASS` → count |

Every `PARM` and every `MSG` line is available as a raw header (the Diagnostics
tab).

## Test logs

`tests/logs/ardupilot-*.bin` (MIT, sources in `tests/logs/README.md`): Copter
3.3 (the legacy format with `TimeMS`, no `FMTU`), Plane 4.3.5, QuadPlane 4.4.4
with ESC telemetry and the harmonic notch. The first 20 rows of the key
messages are compared with an independent Python decoder
(`tests/golden/*.rows.json`).

## Several sessions in one file

Every log starts with a block of `FMT` packets whose first packet describes
`FMT` itself (type 128, length 89). The file is split into sessions at these
headers: a logger restart or two concatenated files give two sessions with
their own `index` and `offset_bytes`; `OpenOptions.sessions` selects the ones
wanted. A segment with only a header and no stabilisation-loop messages is
skipped. Backward jumps of `TimeUS` without a new header do not count as a
session.

## Known limitations

- `gyro.raw` from the batch sampler covers only the bursts (a few percent of
  the time in a typical setup) and is limited to the timebase bandwidth; the
  full batch rate is not exported yet.
- A QuadPlane in VTOL mode uses `Q_A_RAT_*`; the mapping is chosen by the
  vehicle type (Plane), so in VTOL segments the `PIDR.Tar` scale may be in rad/s.
