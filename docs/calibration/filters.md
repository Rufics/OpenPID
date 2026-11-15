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

1. **pt1**: `y += k·(x − y)` with `k = dt / (1 / (2π·fc) + dt)`. **pt2**
   and **pt3**: two and three of them in a row, each at
   `fc / sqrt(2^(1/n) − 1)`. **biquad-butter** and **biquad-bessel**: the
   usual second-order low-pass, `b = [(1 − cos w)/2, 1 − cos w,
   (1 − cos w)/2]` over the denominator of the notch below, with a Q of
   `1/√2` and of `1/√3`.
2. **A notch** (the RPM notches and the static one): the usual
   second-order notch, `b = [1, −2·cos w, 1]`, `a = [1 + α, −2·cos w,
   1 − α]`, `w = 2π·f0 / fs`, `α = sin w / (2·Q)`. A box holds a hundred
   times its Q (500 is a Q of 5). The RPM notches stand at the frequency
   of the box and at twice and three times it. The static notch is set by
   its "Notch Q" as the window comes up, and the cut-off shown is the one
   that goes with that Q (452 Hz for 500 Hz and a Q of 5); with the list
   on "Notch Cutoff" its Q is `fc·fcut / (fc² − fcut²)`.
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

## Round 14 (2026-11-15): the other types and rates, and a set to hold the rule against

The lists set for the first time (pack `b7990bb`): low-pass 1 as each of
its other four types and at the other four loop rates with its plots
copied; one motor, one harmonic, the RPM filter off, the notch off; the
notch set by its cut-off; the impulse response and the chirp; and a set
of filters made for no question, whose delays had been written down
before the run (2026-11-15, 21:06 UTC). 46 vector copies and 19 pictures,
none failed.

| Asked | Came back |
|---|---|
| pt2 and pt3 at 250 Hz | two and three first-order low-passes in a row, each at `fc / sqrt(2^(1/n) − 1)` (388 and 490 Hz): 0.81945 and 0.97369 ms; the four curves of each within the reading of a copy |
| biquad-butter and biquad-bessel at 250 Hz | the usual second-order low-pass with a Q of `1/√2` and of `1/√3`: 0.89743 and 1.0991 ms; the four curves of each within the reading of a copy |
| Loop rates of 4, 3.2, 2 and 1 kHz | every printed delay as written down (the notch column 2.379, 2.3682, 2.321 and 2.0724 ms); the magnitude, the delay and the phase of the low-pass at each rate as the formulas give them. At 1 kHz the static notch at 500 Hz stands on half the rate, does nothing, and is not labelled |
| One motor; the RPM filter off; the notch off | the columns 0.64546, 0.062842 and 2.3305 ms, as written down |
| The notch set by its cut-off | the notch then has the Q that its centre and cut-off give, `fc·fcut / (fc² − fcut²)`: 4.95 for 500 and 452 Hz, 0.063532 ms in place of 0.062842. The box of the cut-off kept its 452 when 400 was typed |
| The set made for no question: 4 kHz, 6 motors, 2 harmonics from 130 Hz with a Q of 3, low-passes at 180 and 320 Hz, a notch at 240 Hz with a Q of 3.5 | all seven printed numbers as written down (0.88418 and 0.49736 ms, column 1.3815; 0.40669 and 0.2012 ms, 0.18722 ms, column 3.8346), and the magnitude, delay and phase of its five filters as the formulas give them |
| "Impulse Response" | the response to one sample of 1, divided by the sample step (1 313 for the low-pass at 250 Hz and 8 kHz, `k · fs`) |
| "Chirp Generator" | the bottom row becomes a tone that climbs from "Signal Hz start" to "Signal Hz end" over "Duration sec.", before and after the filters of the column; the noise list adds noise to it, the "show" list hides one of the two |

One pick did not take: with "1 harmonic" asked, the picture still shows
three. Two harmonics, in the set made for no question, came out as two.
The step response of a low-pass at a loop rate other than 8 kHz is drawn
on a time axis that does not start at 0 (its first sample stands at
0.08 ms at 4 kHz); its values are the filter's.

### Not settled

- Why the step response's time axis moves with the loop rate.
- The chirp of the notch column shows the dip of the static notch and
  none at the RPM notches.
- low-pass 2 as another type than pt1 (taken to be as low-pass 1).

## OpenPID on the same filters

OpenPID's Filter Sim (INVENTORY X1–X6) has the same filters: the
first-order low-pass, pt2 and pt3 with the same cut-off correction, the
second-order low-pass with a Q of `1/√2`, and the notch with its Q from
the centre and the cut-off. It has no "bessel" low-pass (the firmware has
none). It differs in what it is about and in what it prints:

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
of magnitude at 1 000 Hz: the curves part towards half the rate, and a
notch at 500 Hz reads 0.050 ms in place of 0.063. Whether OpenPID should
compute its curves at the loop rate, and print the delays the reference
tool prints, is entry D-007 of [DECISIONS.md](DECISIONS.md).
