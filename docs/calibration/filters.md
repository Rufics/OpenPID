# Filter Sim: what the rounds have shown

Part of #166. The rows are X1–X6 of [INVENTORY.md](INVENTORY.md). The
reference tool is version 0.94. Its filter simulator is a calculator of the
filters typed into it; it does not read the log.

## Round 13 (2026-11-15): identify

The window as it comes up, a picture of each of its ten lists held open,
its three switches, and four values typed in (pack `959ad7c`): 28 vector
copies of plots and 18 pictures, none failed.

### The window

Two columns of four plots: the low-passes and the notches; magnitude,
"Filter Delay (ms)", "Phase Delay (deg)" and the step response. The lists:

| List | Items (what it comes up with in bold) |
|---|---|
| loop rate | **8k**, 4k, 3.2k, 2k, 1k |
| motors | 1 … 8 (**4**) |
| low-pass 1, low-pass 2 | off, **pt1**, pt2, pt3, biquad-butter, biquad-bessel |
| RPM filter | off, 1, 2 or **3** harmonics |
| notch | off, **on** |
| what the notch is set by | **Notch Q**, Notch Cutoff |
| the bottom row | **Step Response**, Impulse Response, Chirp Generator |
| noise | **Add noise** (none), low, med, high |
| what is shown | filtered, unfiltered, **both** |

The switches: "combine lpf" adds a dashed curve of the two low-passes
together to the magnitude, the delay and the phase; "Log Frequency" makes
the frequency axis logarithmic; "Magnitude dB" shows the magnitude in
decibels in place of a ratio.

### What a copy holds

A copied plot carries its curves as lines with their points: 251 points
from 1 to 1 000 Hz for the delay and the phase, the samples of the filter
for the step response (one every 0.125 ms at 8 kHz), and its tick marks
and labels, from which a point is read to about 0.0001 of the axis.

## The rule

At the loop rate `fs` of the list, `dt = 1 / fs`:

1. **pt1**: `y += k·(x − y)` with `k = dt / (1 / (2π·fc) + dt)`.
2. **A notch** (the RPM notches and the static one): the usual
   second-order notch, `b = [1, −2·cos w, 1]`, `a = [1 + α, −2·cos w,
   1 − α]`, `w = 2π·f0 / fs`, `α = sin w / (2·Q)`. A box holds a hundred
   times its Q (500 is a Q of 5). The RPM notches stand at the frequency
   of the box and at twice and three times it. The static notch is set by
   its "Notch Q" as the window comes up; "Notch Cutoff" then only shows
   the cut-off that goes with that Q (452 Hz for 500 Hz and a Q of 5).
3. **The four plots** of a filter `H`: `|H(f)|`; the phase delay
   `−arg H(f) / (2π·f)` in milliseconds; `arg H(f)` in degrees; and the
   response of the filter to a step, sample by sample.
4. **The printed delay** of a filter is its phase delay at the first
   point of the curve, 1 Hz. The title of a column is the sum of the
   printed delays of its filters, an RPM notch counted once per motor;
   "Filter Delay Total" is the sum of the two columns.

### How well it holds

Forty curves (the ten filters of the window as it comes up, a low-pass at
100 Hz, RPM notches with a Q of 2.5, a static notch with a Q of 3 and one
at 300 Hz; four plots each) against the formulas: the magnitude within
0.0002 for the low-passes and 0.002 at the steepest point of a notch, the
delay within 0.0003 ms (0.003 ms beside a notch), the phase within 0.1°,
the step response within 0.001. Every printed delay is the formula's to
the last digit shown:

| Filters | Printed |
|---|---|
| pt1 at 250 and 500 Hz | 0.63662 and 0.31831 ms; column 0.95492 ms |
| pt1 at 100 Hz | 1.5915 ms; column 1.9098 ms |
| RPM notches from 100 Hz, Q 5 | 0.31818, 0.15883, 0.10561 ms |
| the same, Q 2.5 | 0.63635, 0.31766, 0.21123 ms; column 4.7238 ms |
| static notch at 500 Hz, Q 5 | 0.062842 ms; column 2.3933 ms; total 3.3483 ms |
| static notch at 500 Hz, Q 3 | 0.10474 ms; column 2.4352 ms |
| static notch at 300 Hz, Q 5 | 0.10561 ms; column 2.4361 ms |

### Not settled

- pt2, pt3 and the two biquad low-passes; the loop rates other than 8 kHz.
- The notch set by its cut-off; the numbers of motors and of harmonics
  other than 4 and 3.
- "Impulse Response" and "Chirp Generator" with their noise and "show"
  lists, and the signal's start, end and duration.

## OpenPID on the same filters

OpenPID's Filter Sim (INVENTORY X1–X6) has the same first-order low-pass
and the same notch. It differs in what it is about and in what it prints:

| | The reference tool | OpenPID |
|---|---|---|
| The filters | typed into the window | those of the log's headers, with what-if changes |
| Their rate | the loop rate of a list, 8 kHz as it comes up | the rate of the log |
| The static notch's Q | typed, or from the cut-off | from the centre and the cut-off of the headers |
| Magnitude | a ratio; decibels by a switch | decibels |
| Delay | the phase delay as a curve; printed at 1 Hz per filter, summed per column and in total | the phase delay as a curve; "delay at 100 Hz" of the whole chain |
| Step response of a filter | drawn | not drawn |
| The log through the filters | none | the simulated gyro, its spectra and how well it fits |

A low-pass of 250 Hz computed at a log rate of 2 kHz in place of a loop
rate of 8 kHz reads the same 0.637 ms at 1 Hz, but 0.28 in place of 0.23
of magnitude at 1 000 Hz: the curves part towards half the rate. Whether
OpenPID should compute its filters at the loop rate, and print the delays
the reference tool prints, becomes an entry of
[DECISIONS.md](DECISIONS.md) once the remaining filter types are read.
