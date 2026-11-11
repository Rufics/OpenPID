# Step response: what the rounds have shown

Part of #172 (epic #166). The rows are the step-response rows of
[INVENTORY.md](INVENTORY.md).

## Trial of the capture on this window (2026-11-11)

One log, `t1-plants-2k` (30 s at 2 kHz): on each axis random stick moves of
up to 200 deg/s, a new level every 150 … 700 ms, flown through a plant
whose step response is known in closed form.

| Axis | Plant | True response |
|---|---|---|
| roll | a pure delay of 10 ms | a step at 10 ms; no overshoot |
| pitch | first order, time constant 20 ms, after 5 ms | no overshoot; half way at 18.863 ms |
| yaw | second order, 15 Hz, damping 0.4, after 5 ms | a peak of 1.2538; half way at 18.110 ms |

The script (189 steps, 0 failed, 1.7 min) copied the nine plots of the
window as vector graphics, then the responses with "Raw" on, at the three
smoothing levels and the latency plots with "SP-Gyro Delay": 21 copies,
three of them on a second try. A copy of this window holds what the
spectrum's did: the points of a curve with fractional coordinates, and the
tick labels that place it.

### What a copy holds

- **The response curve**: 1 001 points from 0 to 500 ms, one per sample of
  the log (0.5 ms at 2 kHz).
- **"Raw"**: one curve per window, 258 on roll; each can be read on its
  own. A curve is drawn only inside the plot (0 … 1.5).
- **Peak and latency**: the value as a point and its spread as a bar, both
  as numbers (the ends of two short lines).

### First readings

None is a measurement of a rule yet: one log, three plants.

| | True | Reference tool | OpenPID (`run step --epoch 1..29`) |
|---|---|---|---|
| Windows: roll / pitch / yaw | — | 258 / 256 / 259 | 208 / 208 / 207 (of 209) |
| Peak, pure delay | 1.0000 | 0.9998 ± 0.0166 | 1.0595 |
| Peak, first order | 1.0000 | 0.9946 ± 0.0397 | 0.9927 |
| Peak, second order | 1.2538 | 1.2521 ± 0.0531 | 1.2969 |
| Half way, pure delay | 10.000 ms | 9.750 ms | 10.206 ms |
| Half way, first order | 18.863 ms | 18.849 ms | 19.822 ms |
| Half way, second order | 18.110 ms | 18.075 ms | 17.628 ms |
| Value at 500 ms (true: 1) | 1.0000 | 0.9890 / 0.9848 / 0.9853 | 0.9613 / 0.9823 / 1.0154 |

("Half way" is where the mean curve crosses 0.5, read the same way from
both tools' curves; OpenPID's own "time to 50 %" metrics differ from it, an
open point of the inventory.)

- **On plants of known response the reference tool's curve is the truer
  one.** Its peak is within 0.002 of the truth on all three; OpenPID draws
  an overshoot of 6 % on a pure delay, which has none, and 0.04 too much on
  the second-order plant.
- **Its curve sinks slowly**: 0.997 at 100 ms, 0.989 at 500 ms on the pure
  delay, and about 0.985 at 500 ms on the other two. OpenPID's ends between
  0.96 and 1.02.
- **Its "Peak" value is the highest point of the mean curve** (0.9998,
  0.9946 and 1.2521 on the three axes, equal to four decimals). What the
  bar is, is not known yet: on yaw it is close to the spread of the single
  windows' peaks (0.0526 against 0.0531 on the plot), on roll it is not
  (0.0356 against 0.0166).
- **Its "SR Latency"** is not where the mean curve crosses 0.5: it reads
  9.5 ± 0, 18.63 ± 0.64 and 17.86 ± 0.31 ms where the crossings are 9.75,
  18.85 and 18.08 ms. **"SP-Gyro Delay"** reads 10.0, 18.625 and 18.25 ms:
  exact for the pure delay, and stepped by an eighth of a millisecond.
- **The three smoothing levels changed nothing** on this log: the curves
  are equal to the unsmoothed ones to four decimals, the sharp step of the
  pure delay included. The list shows "smoothing…" whatever is chosen, so
  the picture cannot confirm that a level was set; round 1 asks again on a
  log with noise.

## Round 1 (sent 2026-11-11, round 4 of the ledger)

Nine logs (`catalogue::round(4)` of `crates/openpid-probe`):

| Probe | What differs between its axes | Asks |
|---|---|---|
| `t1-plants-2k`, `t2-plants-1k`, `t3-plants-4k` | the three plants above | the curve against the truth, at three log rates |
| `t4-amplitude-2k` | stick moves within 25, 100, 400 deg/s | does the response depend on how hard the stick moves; what a window must have to count |
| `t5-noise-2k` | gyro noise of 0.5, 5, 20 deg/s RMS | what noise does to the curve; what the smoothing levels do |
| `t6-holds-2k` | a new stick level every 40 … 120 ms, 300 … 900 ms, 1.5 … 3 s | which windows are taken |
| `t7-gain-2k` | a craft that reaches 0.8, 1.0, 1.25 of the rate asked for | where the curve settles |
| `t8-sparse-2k` | stick moves from 5 to 10 s only, from 20 to 22 s only, one move every 2 s | where the windows come from and how many there are |
| `t9-short-2k` | the three plants in 12 s | the number of windows against the length |

For every log: the nine plots, every window's response, the other latency;
for `t1` and `t5` the three smoothing levels. 153 copies.
