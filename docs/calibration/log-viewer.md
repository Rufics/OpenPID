# Log viewer: what the rounds have shown

Part of #175 (epic #166). The rows are L1–L14 of
[INVENTORY.md](INVENTORY.md). The reference tool is version 0.94; its main
window is the log viewer.

## Round 15 (2026-11-16): identify

One probe log, written as a Betaflight 2025.12 log (pack `fdfb71e`):
`q1-values-2k`, 20 s at 2 kHz, in which every channel holds a constant or a
step of its own.

| Channel | Roll | Pitch | Yaw |
|---|---|---|---|
| Gyro, deg/s | 100, and 1000 in the one sample at 15.000 s | 200 | −150 |
| Unfiltered gyro, deg/s | 120 | 230 | −170 |
| Setpoint, deg/s | 0; 100 from 4 s, 200 from 8 s, 400 from 12 s, 0 from 16 s | 60 | −40 |
| P, I, D, F term | 100, 20, −30, 5 | −50, 10, 15, 0 | 25, −5, none, 0 |
| `debug[0..3]` (debug mode 1) | 123 | −45 | 6 (and 0) |

Motors 1 to 4 at 158, 630, 1102 and 2047 with the header
`motorOutput:158,2047` (0, 25, 50 and 100 % of the range between the two),
turning at 6 000, 12 000, 18 000 and 24 000 rpm (`eRPM` 420 … 1680 with 14
poles); the throttle at 50 %, at 20 % from 10 to 12 s; the sticks follow
the setpoint (`rcCommand` 30 on pitch, −20 on yaw); armed at 2.000 s, angle
mode from 5.000 to 9.000 s.

Asked: the four plots copied as the window comes up; every line of the
check-list ticked, a picture and the four plots copied again; "y scale"
typed as 1000; the list of the line smoothing held open; a click in the
roll plot and one on ">" of the player; a click on "Markup" and on
"Period". 8 pictures and 8 vector copies.

**What came back**: all 173 steps, the 8 pictures and the 4 copies of the
window as it comes up. With every line ticked the tool copied nothing, at
three tries, and the script went on; what those plots show is read from
the pictures (to about 2 deg/s, and 0.5 on the lower plot). After "y scale"
was typed the script saw the window change for 205 s before it went on;
the text cursor left in the box may be all that changed.

### The lines

From the copies (a copy places a line to about 0.25 deg/s on the rate plots
and to 0.02 % on the lower one) and, where marked *px*, from the pictures:

| Line of the check-list | The log holds | The reference tool draws |
|---|---|---|
| Gyro | 100, 200, −150 deg/s | 99.8, 199.9, −150.1 |
| Gyro(pf) | 120, 230, −170 | 120, 230, −170 *px* |
| Set point | roll 0, 100, 200, 400, 0; pitch 60; yaw −40 | 0, 99.8, 199.9, 400.0, 0, the steps at 4.000, 8.000, 12.000 and 16.000 s; 59.8; −40.2 |
| P-term, I-term, D-term, F-term | the table above | the logged numbers, on the rate axis: 100, 20, −30, 5; −50, 10, 15, 0; 25, −5, no D line, 0 *px* |
| PID sum | — | 95, −25, 20: P + I + D + F *px* |
| PID error | — | roll 100, 0, −100, −300, 100; pitch 140; yaw −110: **the gyro less the setpoint** *px*. In the sample of the spike it reads 600 (seen with "y scale" 1000) |
| D-term(pf) | no such field | 0 on every axis, and one stroke down to the edge of the plot at the gyro's spike: a line the tool computes from the gyro. Its scale cannot be read from one spike |
| Throttle | 50 %, 20 % from 10 to 12 s | 50.00 and 20.00, the steps at 10.00 and 12.00 s |
| Motor 1/5 … 4/8 | 158, 630, 1102, 2047 | 7.88, 31.48, 55.08 %, and the fourth off the plot, which ends at 100 %; 102.7 % *px* once the axis is longer. **The logged number over 20** (7.9, 31.5, 55.1, 102.35), not the share of the range between the `motorOutput` limits (0, 25, 50, 100). Four thin dashed lines at 0 in the same colours: motors 5 to 8, which the log does not have |
| RPS 1/5 … 4/8 | 6 000 … 24 000 rpm | the axis becomes "Motor (%) \| RPS (Hz)", 0 … 150; RPS 1 at 100.0 *px*: **revolutions per second**. The other three (200, 300, 400) are off the plot |
| Debug | 123, −45, 6, 0 | `debug[0]`, `[1]` and `[2]` as logged, on the roll, pitch and yaw plots (123, −46, 6 *px*); the fourth is not drawn. The lines are not yet in the picture taken a second after the tick, and are in all the later ones |

### The rest of the window

- **Time**: from the first frame of the log (the lines run from 0.000 to
  20.000 s). The first and the last second are greyed.
- **"y scale"** is half the span of the rate plots: 500 gives ± 500, 1000
  gives ± 1000. A line that leaves the plot is cut at its edge.
- **A spike of one sample is drawn** with the whole log in view: the gyro
  line goes to the edge of the plot at 15.000 s. A copied line of the
  40 000 samples has 7 253 points.
- **The player**: a click in a plot placed no cursor. ">" put a line
  through all plots at "time: 0.2 sec" and filled the read-out under the
  panel: "M1: 8%", "M2: 32%", "M3: 55%", "M4: 102%" and "gyro R: 100
  deg/s", "gyro P: 200 deg/s", "gyro Y: −150 deg/s" — the motors and the
  gyro in whole units, nothing of the other lines.
- **The stick boxes** ("T / Y" and "P / R"): the throttle of 50 % at the
  middle of its box; the pitch stick 6 % of half the box above the middle
  for 6 % of stick; the yaw stick 4 % to the right for 4 % of stick the
  other way: yaw is mirrored. Read to a pixel, which is 1 % here.
- **Flight modes**: no mark and no name of a mode anywhere, though the
  log arms at 2 s and changes mode at 5 and 9 s.
- **Line smoothing**: "off" (as the window comes up), "low", "med",
  "med-high", "high". What they do was not asked.
- **"Markup" and "Period"**, clicked once each, changed nothing in the
  plots; the time of the player read 0.45 s after them.

## OpenPID on the same log

`openpid decode --scaled` on `q1-values-2k`:

| | The reference tool | OpenPID |
|---|---|---|
| Gyro, unfiltered gyro, setpoint, P, I, D, F, PID sum, throttle, debug | as logged | the same numbers |
| PID error | gyro − setpoint: 100, 0, −100, −300; 140; −110 | setpoint − gyro: −100, 0, 100, 300; −140; 110 |
| Motors | 7.9, 31.5, 55.1, 102.35 % | 0, 25.0, 50.0, 100 % |
| Motor speed | 100, 200, 300, 400 "RPS (Hz)" | 6 000, 12 000, 18 000, 24 000 rpm |
| Sticks | pitch up for a positive `rcCommand[1]`, yaw mirrored | the same (6 % and 4 %) |
| At the cursor | the motors and the gyro | every line drawn |
| Flight modes | not shown | the modes at the cursor, marks where they change |

The three differences are entries D-008, D-009 and D-010 of
[DECISIONS.md](DECISIONS.md). The owner's decisions of 2026-11-16: the
motors' percent and the sign of the PID error stay OpenPID's; the motor
speed is shown in revolutions per second, as the reference tool (#198).

## Not settled

- **D-term(pf)**: what the tool computes it from and at what scale. A gyro
  that rises evenly would tell; it is asked only if the owner wants that
  line in OpenPID.
- **The motors of a log that is not DShot** (outputs of 1000 … 2000): the
  rule of the tool was seen on DShot numbers only.
- **Which field the throttle is read from**: the stick and the mixer's
  throttle said the same in this log.
- **Which sample the read-out shows**, the line smoothing, "Markup" and
  "Period".
- **The motor speed above 150**: whether the lower plot ever grows past it.
