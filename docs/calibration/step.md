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
  1.2504): the response does not depend on how hard the stick moves. What
  a window needs is a mean stick distance from the centre, not a move of
  some size (the rule below). OpenPID refuses the 25 deg/s axis (its
  threshold is 80 deg/s) and keeps half the windows of the 100 deg/s one.
- **Where the sticks move (`t8`).** Moves during 5 s of the flight give 55
  windows, during 2 s give 31, one move every 2 s gives 233. Windows come
  only from where the stick moves (the rule below gives all three counts).
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
  holds. It is a property of the rule below (the model sinks the same
  way), not of the plants.

### The Peak and Latency plots

Read by eye in the first pass; the rule is in the next section.

- **Peak** is the highest point of the mean curve.
- **SR Latency** is taken per window at the level 0.5, not at half of
  where the curve settles: with a gain of 0.8 it reads 20.06 ms where the
  same plant at a gain of 1 reads 17.84 ms, and the mean curves cross 0.5
  at 20.30 and 18.08 ms. For the pure delay of 10 ms it reads 10.0, 9.5
  and 9.25 ms at 1, 2 and 4 kHz.
- **SP-Gyro Delay** reads 10.000 ms for the pure delay at all three rates,
  and 17.4 … 18.9 ms for the other plants, in steps of an eighth of a
  millisecond.

## The rule (2026-11-12, from the copies of round 4)

With "Raw" on, the tool draws the response of every window it used: 5 610
curves on the 27 plots of the round. One rule gives every one of them, the
45 mean curves (27 without smoothing, 18 at the three levels), the window
counts of all 45 legends, and the Peak and SR Latency plots, to the
precision of the copies (0.0000004 on a curve that runs from 0 to 1.25).
Round 5 confirmed it on logs it had not seen and corrected item 6. The
rule is `openpid_analysis::step_windows` (#188); the copies and the
scripts that read them were one-off material and are not in the repository.

1. **Smoothing** works on the filtered gyro only, before anything else: a
   local regression with tricube weights (the one of the Spectral
   Analyzer) over 5, 11 and 21 samples for "low", "medium" and "high":
   samples, the same number at 1, 2 and 4 kHz (round 5). The setpoint is
   not smoothed. The logged setpoint is used, not
   one computed from the sticks.
2. **Windows**: the log without its first and last second, starting two
   samples after the first second (as the spectrum does), is cut into
   windows of 2 s, a tenth of a second apart: 259 for 30 s, 219 for 26 s,
   139 for 18 s, 79 for 12 s, at every log rate. On two of the three
   short logs (one of 12 s, the one of 18 s) the first window starts one
   sample later; why is not known, and the curve moves by less than 0.001.
3. **One window**: the setpoint and the gyro each go through a Hann window
   and are followed by 200 zeros (200 at 1, 2 and 4 kHz alike). Their
   transforms `X` and `Y`, divided by their length, give
   `H = Y·conj(X) / (|X|² + 0.0001)`. Back in time this is the impulse
   response; its running sum is the window's step response, of which the
   first 500 ms are kept, at the log's own rate.
4. **A window counts** when the stick was off its centre by 20 deg/s or
   more on average (the mean of `|setpoint|` over the window; exactly 20
   is enough, 19 is not), and when its response stays between 0.5 and 1.5
   from 150 ms to 500 ms (150 ms is a time: sample 150, 300 and 600 at 1,
   2 and 4 kHz).
5. **The curve** is the mean of the windows that count; the `n` of the
   legend is their number.
6. **Peak** is the highest value of the curve before 150 ms (the samples
   before the one from which item 4 looks); a response that is still
   rising there reads lower than its final overshoot. Its bar is the
   spread (sample standard deviation) over the windows of each window's
   highest value among its first 150 samples (150 samples at every rate:
   150, 75 and 37.5 ms).
7. **SR Latency** is, per window, the time of the first sample above 0.5,
   counted from one and less a millisecond (`(i + 1) / fs − 1 ms` for
   sample `i` counted from zero): a pure delay of 10 ms reads 10.0, 9.5 and
   9.25 ms at 1, 2 and 4 kHz. The plot shows the mean over the windows and
   their spread.

What this explains of the round:

- **Why the windows' count follows the stick.** 25 deg/s moves give a mean
  distance of 12 deg/s: two windows of 259 reach 20.06 and 20.08 and are
  the two the tool drew. Moves in 5 s of the flight: the 55 windows that
  hold enough of them.
- **Why noise drops windows and smoothing brings them back.** A noisy
  response leaves 0.5 … 1.5 somewhere after 150 ms: 58 of 259 windows at
  20 deg/s RMS. Smoothing the gyro keeps 231, 249 and 256 of them inside.
- **Why long holds are worse.** A window with one stick move divides by a
  setpoint with little in it: its response wanders (the bar of 0.18).
- **Why a gain of 1.25 has a curve at all**: its tail stays below 1.5.
  A gain above 1.5 or below 0.5 would leave no window.
- **Why the curve sinks.** The Hann window weighs the setpoint and the
  gyro by where they are in the window, not by where the response to a
  move is: the division of the two is slightly off for the late part of a
  response. The zeros and the constant in the denominator keep single
  windows from going far wrong (without them the same windows are up to
  0.4 off).

Three details differ from what a reader might assume, and each is as the
tool does it: the 200 zeros and the first 150 samples are numbers of
samples, the same at every rate; and the first sample above 0.5 is taken,
with no reading between two samples.

### What round 5 settled, and what is still open

| Question after round 4 | Answer of round 5 |
|---|---|
| The smoothing windows at other log rates | 5, 11 and 21 samples at 1 and 4 kHz too: all curves of the three levels as predicted |
| "From 150 ms on": a time or a number of samples | a time: 192 windows on the noisy axis at 1 kHz and 241 on the creeping plant at 4 kHz, the counts of that reading |
| Where the Peak stops looking | at 150 ms: for a plant of 2.5 Hz, whose curve tops at 226 ms, the Peak is the curve's value at 149.5 ms (0.95254); for the creeping plant it is the value just before 150 ms (0.774) |
| "20 deg/s or more": is exactly 20 enough | yes: 257 windows at 20 and at 21, none at 19 |
| Where the first window starts | two samples after the first second on the logs of 26 and 30 s and on one of 12 s, three on the other of 12 s and on the one of 18 s. Not the length alone, then, and not the first second (two of the logs begin alike). **Open**; the effect is below 0.001 on the curve |
| SP-Gyro Delay | the same straight-line reading at 8 kHz is right on 39 of the 47 plots of both rounds, one step of 0.125 ms off on seven and 10 ms off on one (the creeping plant at 1 kHz: 62.5 against 72.75, where the agreement of the two signals has a flat top). Trying other ways of bringing the signals to 8 kHz, other ends of the stretch, with and without the means: none reads more plots. **Open** |

## OpenPID after decision D-002 (2026-11-12)

The owner chose option A: OpenPID computes the step response as the
reference tool does. The rule above is now `openpid_analysis::step_windows`
and the default method of the Step response tab, of `openpid run step` and
of the advice (#188, on its own from `master`; it replaces #184, which
stood on the branches of the test tools); the estimator OpenPID had is kept
as a second method.

**Computed.** OpenPID's own code was held to the reference tool's readings
of rounds 4 and 5, once the whole way a user goes: the log as a file, the
reader, an epoch, the analysis with its defaults. For the epoch that is
the reference tool's stretch of the log (1.001 s to one second before the
end) the result is its curve to 0.000002, with its window count, Peak, SR
latency and both bars. Those comparisons were one-off; what stays in the
repository are the tests on plants whose response is known in closed form
and on the flown logs below.

**On flown logs** (not probes; the reference tool's numbers are from the
owner's screenshots for #33, read by eye to two or three digits):

| Log, axis | Reference tool: windows / peak / SR latency | OpenPID, division |
|---|---|---|
| `betaflight-2025.12.2-btfl_027`, roll | 78 / 1.15 / 22 ms | 77 / 1.162 / 22.07 ms |
| the same, pitch | 13 / 1.12 / 22.5 ms | 13 / 1.127 / 22.62 ms |
| the same, yaw | 30 / 1.18 / 12.3 ms | 30 / 1.179 / 12.30 ms |
| `betaflight-4.5.1-two-sessions` (21 s), roll and pitch | insufficient data | not enough data |
| the same, yaw | 32 / 0.87 / 11.7 ms | 31 / 0.875 / 11.29 ms |

These logs run at 997 and 1013 Hz; OpenPID puts a log on an even grid at
its nominal rate before any analysis, which the reference tool does not:
the window that differs on two axes and the 0.4 ms on the last row are of
that size. Nothing was fitted to these logs.

**Drawn.** The Step response tab for `t1-plants-2k` shows a step at 10 ms
on roll, the first-order rise on pitch and the overshoot of 1.25 on yaw,
with `n=258`, `255`, `259` in the legends (the tab's automatic epoch
starts at 1.000 s, one sample before the reference tool's, which costs
pitch one window), Peak points at 1.00, 0.99 and 1.25 with their bars, and
latencies of 9.5, 18.6 and 17.9 ms: the reference tool's plots of the same
log. A test reads what the Peak and Latency plots are given to draw for a
flown log and holds it to the table above. The Latency plot showed the
half-way time of the mean curve before; with the division it shows the SR
latency of the windows, as the reference tool does.

## Round 2 (round 5 of the ledger): confirm

Seven logs (`catalogue::round(5)`, pack `407c2cd`), every one with stick
moves the model has not seen. Its predictions for every plot are in the pack
(`predictions.json`): the window count, the curve at 18 times, the Peak
and the latency with their bars, the delay; where the table above leaves a
choice, both readings.

| Probe | What differs between its axes | Asks |
|---|---|---|
| `u1-plants-2k` (26 s) | the three plants of round 1, other stick moves | the rule as it stands |
| `u2-rates-1k`, `u3-rates-4k` | a pure delay, a creeping plant, the second-order plant under 20 deg/s RMS of noise; the three smoothing levels | what follows the log rate: the smoothing windows, the limit at 150 ms |
| `u4-slow-2k` | a creeping plant; second-order plants of 2.5 and 4 Hz | where the Peak stops looking |
| `u5-stick-2k` | a stick always 19, 21, 20 deg/s from its centre | what a window needs: predicted 0, 257 and 257 windows |
| `u6-short-2k` (12 s), `u7-mid-2k` (18 s) | the three plants | where the first window starts |

The script takes 123 copies and 16 pictures, in about 15 minutes.

### What came back (2026-11-13)

945 steps of the script, one failed; 122 vector copies and 16 pictures.

- **Curves and counts.** All 49 mean curves are the predicted ones, to
  0.0000005 where the first window starts as predicted and to 0.0006 on
  `u7-mid-2k`, which starts a sample later; all window counts are the
  predicted ones. The stick that is always 19 deg/s from its centre has no
  curve ("insufficient data"), as predicted.
- **SR Latency**: every mean as predicted. Every bar too, once the plot is
  read as it is drawn: see below.
- **Peak**: as predicted on every plot whose curve tops before 150 ms. On
  the slow plants it is the highest value before 150 ms, the reading the
  prediction had listed as the alternative: item 6 of the rule now says so.
- **Drawn.** The Peak plot's axis runs from 0.8 to 1.5 whatever the
  values, and the Latency plot's axis is set around the points; a bar is
  cut where it leaves the axis, and a point outside is not drawn at all.
  On the creeping plant only the upper end of a Peak bar is visible (the
  point is at 0.774), and the latency bars of the slow plants (8 … 9 ms)
  are cut at both ends. Read with that in mind, every bar is the sample
  standard deviation the rule gives. OpenPID fits both axes to the points
  with their bars, so nothing is cut.

- **Against the truth.** The plants that are over within 150 ms are read
  as before, to a few thousandths. The slow ones are read low: the plant of
  2.5 Hz overshoots to 1.163 at 236 ms; the tool's curve tops at 1.122,
  and its Peak, read before 150 ms, is 0.953. Where the Peak should be
  read is entry D-003 of [DECISIONS.md](DECISIONS.md).

The rule stands; the third round holds it out on logs made for no
question in particular.

## Round 3 (round 6 of the ledger): hold-out

Five flights (`catalogue::round(6)`, pack `3dbab4a`), none made to ask a
question: on every axis another plant behind a delay, a little gyro noise,
stick moves of other seeds, lengths no earlier round had, and 8 kHz for
the first time. The pack carries what the rule expects for each of the 15
axes without smoothing and for one log at the three levels: 95 … 369
windows, peaks between 0.96 and 1.37, SR latencies between 10 and 35 ms.
The rule is not changed for this round: what the copies show is either
what it predicted or a finding.

### The reference tool is version 0.94 from here on (2026-11-13)

The owner moved from version 0.9 to 0.94 before this round was run. Its
list of changes names nothing of the step response but new numbers; the
copies show three changes:

| | 0.9 | 0.94 |
|---|---|---|
| Distance between two windows | 0.1 s | a fortieth of the window: 0.05 s for the default of 2 s. Twice the windows, the same curve to the third decimal |
| Length of a window | 2 s | a list of 1 … 5 s, 2 s by default. Everything else of the rule is as for 2 s: the 200 zeros, the first window, the limit at 150 ms, the first 500 ms kept |
| What the two right columns of plots show | Peak and one of two latencies | a list of five pairs: SR Peak and SR Latency; SR Latency and SP-Gyro Delay; Rise Time and Bandwidth; Sensitivity Peak and Sensitivity Freq; Phase Margin and Crossover Freq |

The window, the toolbar of a plot and the place of the controls changed
too (a newer MATLAB); the capture script was moved to them (#180). A
tick label of a copied plot no longer sits on its tick: the axes are read
from their grid lines.

### What came back (2026-11-13, version 0.94)

2 952 steps of the script, six failed; 186 of 192 vector copies and 29
pictures in 17.5 minutes. The six are "Raw" copies: a plot of 600 curves
takes longer to copy than the script waited.

- **The hold-out.** With the windows 0.05 s apart, which one picture had
  shown before the run, every plot of the four logs at 2, 1, 4 and 8 kHz
  is the predicted one: the twelve mean curves to 0.0000007, the window
  counts (196 … 595), Peak, SR Latency and both bars to the last digit
  read, and the SP-Gyro Delay on eleven of twelve plots (one step of
  0.125 ms off on the twelfth). 8 kHz had not been in a round before.
- **The fifth log** (41 s) is the predicted one too, to 0.0000005, when
  its first window starts three samples after the first second, not two:
  the late start of rounds 4 and 5 again, on four logs of 21 now (12, 18
  and 41 s here, and one of 30 s in the spectrum rounds). Its cause is
  still not found; a log of the same length and rate can start either
  way.
- **The smoothing levels** on that log: curves and counts as the rule
  gives them with that start.
- **Windows of 1 s and of 3 s** on the first log: 1 163 / 1 188 / 1 067
  and 385 / 386 / 381 windows, their curves, Peak, latency and bars, all
  as the rule gives them with the window that long and a fortieth of it
  between two windows. Nothing else of the rule follows the length.
- **Peak** is still read before 150 ms and its bar from the first 150
  samples of every window, at 8 kHz too (19 ms): entry D-003 stands for
  0.94.

**The rule is settled** for the mean curve, the window count, Peak and SR
Latency with their bars, at 1 to 8 kHz, for windows of 1 to 3 s and the
four smoothing levels. Open: the late start (below 0.001 on a curve) and
the SP-Gyro Delay on a few plots.

### The new numbers of 0.94

Read from the copies for all fifteen axes; one value per axis, no bar.
The plants are known, so each has a true value to be held to.

| Number | What it is, as far as found |
|---|---|
| Rise Time (ms) | the time the mean curve takes from 0.1 to 0.9, read between two samples along a straight line. Exact on all fifteen axes (15.3703 ms for a second-order plant of 22 Hz, damping 0.7, whose closed form gives 15.4) |
| Bandwidth (Hz) | near the frequency at which the response of the mean curve has fallen by 3 dB (22.34 Hz read, 22.31 by that definition; 5.56 for a first-order plant of 30 ms, whose true value is 5.31). The exact rule is not found |
| Sensitivity Peak (dB), Sensitivity Freq (Hz) | near the highest value of `|1 − H|` and its frequency, on a grid of 0.977 Hz at every log rate (a transform of the next power of two above the log rate). Within 0.2 dB and a grid step; not exact. Where the peak is small the plot shows no point |
| Phase Margin (deg), Crossover Freq (Hz) | near the margin of `H / (1 − H)` where its gain crosses 1. Within 2 … 4 degrees and 0.4 Hz; not exact |

Whether OpenPID shows these numbers is the owner's to say; they would be
a round of their own to identify (the last three rows).
