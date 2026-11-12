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
- The smoothing levels: read wrongly in the trial. A plot of this window
  keeps the curves of earlier runs ("test 1, 2, …", each in its own
  colour), and the trial read the first curve of every copy, which was the
  unsmoothed one. Round 1 reads the newest; see there.

## Round 1 (2026-11-11, round 4 of the ledger): identify

Nine logs (`catalogue::round(4)` of `crates/openpid-probe`, pack `dad54b3`):

| Probe | What differs between its axes | Asks |
|---|---|---|
| `t1-plants-2k`, `t2-plants-1k`, `t3-plants-4k` | the three plants of the trial | the curve against the truth, at three log rates |
| `t4-amplitude-2k` | stick moves within 25, 100, 400 deg/s | does the response depend on how hard the stick moves; what a window must have to count |
| `t5-noise-2k` | gyro noise of 0.5, 5, 20 deg/s RMS | what noise does to the curve; what the smoothing levels do |
| `t6-holds-2k` | a new stick level every 40 … 120 ms, 300 … 900 ms, 1.5 … 3 s | which windows are taken |
| `t7-gain-2k` | a craft that reaches 0.8, 1.0, 1.25 of the rate asked for | where the curve settles |
| `t8-sparse-2k` | stick moves from 5 to 10 s only, from 20 to 22 s only, one move every 2 s | where the windows come from and how many there are |
| `t9-short-2k` | the three plants in 12 s | the number of windows against the length |

The script took all 153 copies and 20 pictures (1 169 steps, 0 failed,
17.5 min, 24 copies on a second try): for every log its nine plots, every
window's response and the other latency; for `t1` and `t5` the three
smoothing levels.

**A plot of this window keeps earlier runs.** Every Run adds a "test" to
the plots, in the next colour, until the tool clears them. A copy therefore
holds several curves and points; everything below is the newest test of
its copy, checked by the sample step of the curve (1, 0.5 and 0.25 ms for
the three log rates) and by the window count in the legend.

### The curve against the truth, and against OpenPID

"Half" is where the mean curve crosses 0.5. OpenPID is
`openpid run step --epoch 1..29` (1..11 for the short log).

| Probe, axis | True peak / half | Reference tool: windows, peak, half | OpenPID: windows, peak, half |
|---|---|---|---|
| `t1` 2 kHz, delay | 1.0000 / 10.000 ms | 258, 0.9998, 9.750 | 208, 1.0595, 10.206 |
| `t1` 2 kHz, first order | 1.0000 / 18.863 | 256, 0.9948, 18.850 | 208, 0.9927, 19.822 |
| `t1` 2 kHz, second order | 1.2538 / 18.110 | 259, 1.2520, 18.076 | 207, 1.2969, 17.628 |
| `t2` 1 kHz, delay | 1.0000 / 10.000 | 258, 0.9998, 9.500 | 208, 1.0615, 9.984 |
| `t2` 1 kHz, first order | 1.0000 / 18.863 | 257, 0.9942, 18.820 | 208, 0.9963, 19.788 |
| `t2` 1 kHz, second order | 1.2538 / 18.110 | 259, 1.2511, 18.077 | 207, 1.3009, 17.572 |
| `t3` 4 kHz, delay | 1.0000 / 10.000 | 258, 1.0001, 9.875 | 208, 1.0605, 10.160 |
| `t3` 4 kHz, first order | 1.0000 / 18.863 | 256, 0.9961, 18.784 | 208, 0.9924, 19.686 |
| `t3` 4 kHz, second order | 1.2538 / 18.110 | 259, 1.2525, 18.093 | 207, 1.2919, 17.688 |
| `t9` 12 s, second order | 1.2538 / 18.110 | 79, 1.2574, 18.067 | 65, 1.3057, 17.622 |
| `t7` gain 0.8 | 1.0031 / 20.320 | 256, 1.0019, 20.298 | 207, 1.0439, 20.045 |
| `t4` 100 deg/s | 1.2538 / 18.110 | 257, 1.2489, 18.110 | 106, 1.3061, 17.575 |
| `t4` 400 deg/s | 1.2538 / 18.110 | 259, 1.2504, 18.113 | 209, 1.2954, 17.713 |

- **The reference tool's curve is the true response to a few thousandths**:
  the peak within 0.006 on every row above, the half-way time of the two
  smooth plants within 0.08 ms. On the pure delay its step lies half a
  sample before 10 ms at every rate (9.5, 9.75, 9.875 ms).
- **OpenPID's curve is biased the same way on every log**: an overshoot of
  6 % on a pure delay, which has none; a peak 0.04 … 0.05 too high on the
  second-order plant; the half-way time 0.5 ms early on the second-order
  plant and 1 ms late on the first-order one. This is the first of the
  inventory's open points on this tab, now measured against a known truth.
- **The curve is drawn at the log's own rate**: 501, 1 001 and 2 001 points
  for 500 ms at 1, 2 and 4 kHz. Nothing is resampled.

### What the reference tool's result depends on

- **Windows.** 256 … 259 for 30 s of lively sticks and 78 … 79 for 12 s,
  at every log rate: about one window every 0.1 s of the trimmed log, less
  two seconds. OpenPID takes 209 and 65 (one every 0.125 s).
- **Stick size (`t4`).** Moves within 25 deg/s give 2 windows; within 100
  and 400 deg/s give 257 and 259, and the same curve (peaks 1.2489 and
  1.2504): the response does not depend on how hard the stick moves, and a
  window needs a stick move of more than about 25 deg/s. OpenPID refuses
  the 25 deg/s axis (its threshold is 80 deg/s) and keeps half the windows
  of the 100 deg/s one.
- **Where the sticks move (`t8`).** Moves during 5 s of the flight give 55
  windows, during 2 s give 31, one move every 2 s gives 233. Windows come
  only from where the stick moves; the exact rule needs single steps at
  known times (round 2).
- **Hold time (`t6`).** A new level every 40 … 120 ms or 300 … 900 ms: 259
  and 257 windows, peaks 1.2518 and 1.2532. Every 1.5 … 3 s: 216 windows,
  peak 1.2079 ± 0.18 and a curve that sinks to 0.94: long rests make it
  worse, not better.
- **Gain (`t7`).** A craft that reaches 0.8 of the rate asked for is drawn
  settling at 0.79, with its peak at 1.0019 (true 1.0031): the curve is
  not scaled to end at 1. At a gain of 1.25 the curve leaves the plot,
  whose upper limit is 1.5.
- **Noise (`t5`).** At 0.5 and 5 deg/s RMS the curve holds (peaks 1.2512
  and 1.2571, 257 and 259 windows). At 20 deg/s RMS windows are dropped
  (201) and the peak rises to 1.2833 ± 0.12.
- **Smoothing.** It works on the signals before the response is computed,
  not on the curve. On the noisy axis each level brings windows back: off
  201, low 231, medium 249, high 256, with peaks 1.2833, 1.2639, 1.2573,
  1.2491. On the pure delay it blurs the step: from 0.1 to 0.9 in 0.5 ms
  (off), 1.3 ms (low), 2.7 ms (medium), 5.1 ms (high) at 2 kHz. "Low" is a
  three-point kernel of weights 0.29, 0.43, 0.29, the tricube again, as in
  the Spectral Analyzer.
- **The curve sinks**: to 0.985 … 0.989 at 500 ms on the 30 s logs with
  holds of 150 … 700 ms, to 0.99 … 1.00 on the 12 s log and with short
  holds. Its cause is not known.

### The Peak and Latency plots

- **Peak** is the highest point of the mean curve (equal to four decimals
  on 25 of 26 rows; on one it is 0.0004 lower). The bar is not yet
  understood.
- **SR Latency** is taken per window at the level 0.5, not at half of
  where the curve settles: with a gain of 0.8 it reads 20.06 ms where the
  same plant at a gain of 1 reads 17.84 ms, and the mean curves cross 0.5
  at 20.30 and 18.08 ms. For the pure delay of 10 ms it reads 10.0, 9.5
  and 9.25 ms at 1, 2 and 4 kHz.
- **SP-Gyro Delay** reads 10.000 ms for the pure delay at all three rates,
  and 17.4 … 18.9 ms for the other plants, in steps of an eighth of a
  millisecond.

### Not settled yet

- How the response of a window is computed (the 258 single responses of
  every axis are in the "Raw" copies and have not been analysed yet), and
  why the mean sinks.
- The rule that takes or drops a window: the stick threshold, the length
  and the step of the windows, what drops a noisy window.
- What the bar of the Peak plot is, and how the latency of a window is
  read to the fraction of a sample.
- The kernels of "medium" and "high" smoothing, and whether they follow
  the log rate.
