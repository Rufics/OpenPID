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

## Round 16 (2026-11-17): the open ends

Three more logs for this window, among the fourteen of round 16 (pack
`b8d035e`, the same bytes as `379bd18`), all written as Betaflight 2025.12
logs:

| Log | What it holds |
|---|---|
| `v3-samp-1k` | 7 s at 1 kHz. Pitch: a gyro equal to its sample number (0, 1, 2, … 6999 deg/s). Roll: 0 with pulses of 500 deg/s, one sample long at 2.000 s, five at 4.000 s, fifty at 5.000 s. Yaw: noise of 20 deg/s RMS and a step to 200 deg/s at 3.500 s |
| `v1-deriv-2k` | 20 s at 2 kHz. Roll: the filtered gyro a triangle of ±200 deg/s every 2 s (±400 deg/s per second), the unfiltered one a triangle of ±300 deg/s every 1.2 s (±1000 per second). Pitch: tones of 5 Hz (filtered) and 2 Hz (unfiltered), 100 deg/s. Yaw: an unfiltered tone of 150 Hz. The stick's throttle at 50 % (`rcCommand[3]` 1500), the mixer's at 30 % (`setpoint[3]` 300). Motor speeds of 12 000 … 30 000 rpm (200 … 500 Hz) |
| `v2-out-2k` | 20 s at 2 kHz. Motors at 1070, 1300, 1535 and 2000 with `motorOutput:1070,2000` and a protocol that is not digital (`motor_pwm_protocol:1`) |

All 874 steps of the round ran, none failed, in six minutes. The script
chose each log in the window's list of files, which holds them in the
order of their names, and every picture shows the log it was meant to.

### What came back

| Asked | Came back |
|---|---|
| Which sample a line starts with | **The first sample of a log is not drawn**: a copied line of `v3-samp-1k` has 6 999 points for its 7 000 samples, and every pulse is drawn 1 ms early (the one-sample pulse of sample 2000 at 1.999 s). The tool's time zero is the second sample of the log |
| Which sample the read-out shows | the one after the time it prints: "time: 0.25 sec" with "gyro P: 251 deg/s", 0.751 s with 752, 1.2 s with 1201 (the pitch gyro is its own sample number). It is the same offset: the printed time counts from the second sample |
| What "line smoothing" does | each level is a local quadratic regression with the weights `(1 − (d/d_max)³)³` over a span of 11 ("low"), 21 ("med"), 41 ("med-high") and 81 samples ("high"), centred on the sample: the one-sample pulse comes back as that kernel to 0.0002 of its height (peaks 0.317, 0.159, 0.079 and 0.040 of the pulse, with side lobes below zero). "Off" is the samples as they are. The log runs at 1 kHz, so whether the span is counted in samples or in milliseconds was not told apart |
| D-term(pf), which OpenPID does not have | a line the tool computes from the **filtered** gyro: minus its change from one sample to the next, times a factor. On roll every step of the gyro by 1 deg/s is a spike of ∓198, negative while the gyro rises (2 166 against 21); on pitch the values are 0, ±40 and ±80. The factor (198 on roll, 40 on pitch) is not the D gain (30 and 34) nor `d_max`; it was not found |
| Which throttle is drawn | the mixer's: 30 %, where the stick says 50 % (OpenPID's L7 reads the same field) |
| Motors that are not digital | 53.48, 65.00, 76.74 and 100.00 % for 1070, 1300, 1535 and 2000: the logged number over 20 again, whatever the protocol and the `motorOutput` range (D-008) |
| Motor speeds above 150 Hz | with the four speed lines ticked the lower plot reaches 250; the speed of 200 Hz is drawn, those of 300, 400 and 500 Hz are not drawn at all. The grey ends stay at 0 … 100 |
| "Markup" | a line drawn between two clicks in a plot |
| "Period" | a red mark at each of two clicked times and the time between them with its frequency, "6681ms, 0.1496Hz" |
| Flight modes | "ARM" written under the lower plot at 0 s for these logs, which are armed from their first frame; round 15's log, armed at 2 s, showed no name |

## Not settled

- **The factor of D-term(pf)**: 198 on roll and 40 on pitch in one log; it
  is neither the D gain nor `d_max`. OpenPID has no such line.
- **The span of the line smoothing** in samples or in milliseconds: seen at
  1 kHz only.
- **Where the top of the lower plot comes from** with the speed lines
  ticked (250 for speeds of 200 … 500 Hz).
- **What the reference tool has and OpenPID has not**: the line smoothing,
  D-term(pf), "Markup" and "Period". Whether OpenPID should have any of them
  is entry D-011 of [DECISIONS.md](DECISIONS.md).
