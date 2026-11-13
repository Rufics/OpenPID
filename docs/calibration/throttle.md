# Freq × throttle: what the rounds have shown

Part of #173 (epic #166). The rows are T1–T8 of [INVENTORY.md](INVENTORY.md).
The reference tool is version 0.94.

## Round 8 (2026-11-13): identify

Eight probe logs, written as Betaflight 2025.12 logs (pack `7038aac`). The
sticks are still; the throttle moves, and every axis carries two
experiments, one on the unfiltered and one on the filtered gyro, since the
window draws a map of either.

| Probe | Throttle | Unfiltered / filtered gyro | Asks |
|---|---|---|---|
| `z1-stairs-2k` (and `z5` at 1 kHz, `z6` at 4 kHz, `z7` at 8 kHz) | 10 … 90 % in steps of 10, 3 s each | roll: a tone at 50 Hz + 4 Hz per percent, 10 / 2 deg/s; pitch: 200 Hz, 10 deg/s, always / on the stairs of 30 and 40 % only; yaw: white noise of 5 deg/s RMS / the same density between 100 and 300 Hz only | where a throttle lands, how far it spreads, what a tone and a floor read, at four log rates |
| `z2-ramp-2k` | 0 → 100 % evenly | roll: the rising tone / 150 Hz between 20 and 40 % only; pitch: 150 Hz always / tones below 3 % and above 97 %; yaw: noise / the rising tone at 2 deg/s | the axis from end to end, columns with little data |
| `z3-hold-2k` | 50 % | roll: 100 Hz, 10 / 2 deg/s; pitch: that tone from 10 to 20 s / from 5 to 25 s; yaw: pairs of tones 4, 8 and 16 Hz apart / noise | the spread of one column, how segments are averaged |
| `z4-fine-2k` | stairs of 30.0, 30.2 … 30.8 and 60.0, 60.3, 60.5, 60.7, 61.0 % | roll: a tone of its own on every stair, 10 / 2 deg/s; pitch: 200 Hz; yaw: noise | which column a throttle value goes to |
| `z8-short-2k` | the stairs, 1 s each, 12 s in all | as `z1` | a short log |

For every log the window as it comes up (two maps per axis), with "PSD" on
and with "2D" on; for `z1` and `z3` the four other smoothing levels; for
`z3` a colour scale of 5 typed into the box and the view below 100 Hz. The
script ran 3 156 steps without a failure: 204 vector copies and 37
pictures.

### What a copy holds

A copied map is a metafile with a picture inside it, 645 × 542 pixels with
a palette, and the texts of the plot ("mean=", "peak=", the tick labels)
as texts. The picture is the matrix drawn cell by cell without
interpolation, coloured through 256 steps from black over red and yellow
to white between the limits of the colour bar. So a cell is known to one
step of 256: 0.002 on the default scale of 0 … 0.5, 0.2 dB in decibels.
The printed mean and peak have three significant digits.

The cells: 100 columns, one per percent of throttle (column `T` is drawn
from `T − 1` to `T` %); the rows are the bins of a transform of 0.3 s, from
0 Hz up, at every log rate: 150 rows at 1 kHz, 300 at 2 kHz, 600 at 4 kHz,
1 200 at 8 kHz (more rows than the picture has pixels at the two higher
rates, where it shows every other or every fourth row). The last bin
(half the log rate) is not drawn, and the rows are stretched to the top of
the axis, so a row is drawn 0.3 % above its frequency.

A copy with "2D" on is a line plot, and its line arrives thinned to the
pixel columns of the plot (about 2 700 of 28 000 points).

Typing 5 into the "scale" box changed nothing: the maps came back on
0 … 0.5. The plan types the number and presses Run; the tool seems to set
the scale back.

## The rule

Found from the copies of `z1` and `z3`, then held against all 156 copied
maps. It is written for arrays of samples, 0-based; `fs` is the log rate.

1. **The stretch.** The first and the last second of the log are left
   out; the first segment starts at sample `fs + 2` (the same two samples
   the step response window starts late by).
2. **Segments** of 0.3 s (`L = 0.3·fs` samples). In a log of 30 s the next
   segment starts 0.1 s later; in the log of 12 s, 0.06 s later (see "Not
   settled"). With `hop` samples between two, there are
   `floor((len − 2·fs − 1 − L) / hop)` of them, as the step response
   window counts its windows: the last one that would still fit is not
   taken.
3. **The spectrum of a segment**: the samples times a symmetric Hann
   window, transformed, `|X| / L`. Nothing makes up for the window, so a
   tone of amplitude `A` on a bin reads `A / 4` there and `A / 8` on the
   bin on either side.
4. **The throttle of a segment** is the mean of the throttle over its
   samples, in percent.
5. **Columns.** Column `T` (1 … 100) holds every segment with
   `T − 1 < throttle ≤ T + 1`: a window of two percent, so a segment is in
   two columns. A cell is the mean of its segments' spectra; a column
   without segments is 0.
6. **Smoothing**: the matrix is convolved with a Gaussian of `5n` columns
   by `n` rows, `σ = 4` cells both ways, summing to 1, with zeros outside
   the matrix. `n` is 1 … 5 for "low", "low-med", "medium" (the default),
   "med-high", "high": 5 × 1, 10 × 2, 15 × 3, 20 × 4, 25 × 5 cells. With
   `σ = 4` the taps of so short a kernel are nearly equal, so along the
   frequency it is all but a moving average of `n` rows. The even kernels
   are placed half a cell towards the lower throttle and the lower
   frequency.
7. **The map** shows that matrix between 0 and the number in "scale"
   (0.5 as the window comes up).
8. **"PSD"** is another matrix, smoothed the same way: per segment
   `10·log10(2·|X|² / (L·fs))` (the formula of the spectrum window, here
   for a segment), a cell is the mean of its segments' decibels, a column
   without segments is −40 dB, and so is everything outside the matrix. It
   is shown between −40 and 10 dB. Averaging decibels reads noise 2.5 dB
   below its density, as in the spectrum window.
9. **"mean=" and "peak="** are the mean and the highest value of the
   smoothed matrix over all columns and the rows from a tenth of the way
   up (row `rows/10 − 1`: 97 Hz at 2 kHz, 397 Hz at 8 kHz) to the top. In
   the view below 100 Hz they are taken over a short range of rows that
   fits the "freq lims" boxes (20 and 60 Hz); that range was not pinned.

### How well it holds

| | |
|---|---|
| Copied maps compared | 156 (all but the 48 "2D" copies): 8 logs, both gyro channels, three axes; linear and decibels; five smoothing levels for two logs; 1, 2, 4 and 8 kHz; 12 and 30 s |
| Pixels within one colour step of the model | median 99.98 % of a picture, worst 97.7 % (a noise map, where a rounding of half a step shows) |
| Pixels within two steps | median 99.99 %, worst 98.6 % |
| Printed mean | the same three digits on all 150 maps outside the view below 100 Hz (step 9) |
| Printed peak | the same three digits on 149 of those 150; the last is −0.0112 dB for −0.0110 dB |

Numbers read on the way, each as the rule gives it:

- a tone of 2 deg/s at a held throttle fills two columns with 0.50 before
  smoothing (`A / 4`), and white noise of 5 deg/s RMS at 2 kHz with 0.111;
- at "low" the spot of that tone reads 0.094, 0.197, 0.209, 0.209, 0.197,
  0.094 along the throttle (two columns of 0.50 under a kernel of five)
  and 0.5 : 1 : 0.5 along the frequency (the Hann window, no smoothing);
- along the frequency the levels give 1 : 3 : 3 : 1 at "low-med",
  1 : 3 : 4 : 3 : 1 at "medium", and so on;
- on the stairs every stair fills three columns and every segment that
  straddles two stairs two columns of its own, at a third and two thirds
  of the way: three segments per 0.3 s;
- on the even ramp every column has a handful of segments, and one
  segment more or less at the end of the stretch moves the printed peak
  by 6 % (0.517 with 276 segments, 0.486 with 277): that is what gave the
  count of step 2;
- in decibels the peak of the 2 deg/s tone is −34.2 dB and that of the
  10 deg/s tone −31.3 dB at "medium": 14 dB of signal are 2.9 dB on the
  map, because most cells under the kernel hold the floor of −40 dB.

### Not settled

- **The distance between segments** is 0.1 s for the logs of 30 s and
  0.06 s for the log of 12 s (pixel for pixel: 99.9 % with 120 samples
  between two segments, 74 % at best with any other distance). Two
  lengths do not give the rule.
- **Which throttle** the tool takes. In a probe the stick, the throttle
  after the mixer and the four motors say the same.
- **The numbers of the view below 100 Hz** (step 9).
- **"2D"**: the line is a spectrum of the whole stretch (points 1/28 Hz
  apart), not a cut through the map, and it reads a tone higher the
  shorter it sounds (0.219 for 28 s, 0.245 for 20 s, 0.320 for 10 s of the
  same tone): an amplitude spectrum that is smoothed. OpenPID has no such
  view; it was not followed further.
- Other channels than the two gyro ones, and the presets of the window.

## OpenPID on the same logs

OpenPID's map (`openpid run thr-heatmap`, INVENTORY T1) is built
differently at every step:

| | The reference tool | OpenPID |
|---|---|---|
| Segments | 0.3 s, overlapping by two thirds (30 s) | 0.2 s, side by side |
| A tone of amplitude `A` | `A / 4` | `A` |
| Columns | 100, each the segments within ±1 % | 101, each the segments within ±6 % |
| Smoothing | a Gaussian, five levels, 15 × 3 cells by default | a box of 9 columns by 1 % of the segment's bins, fixed |
| Rows | to half the log rate | to 1 000 Hz |
| Decibels | a second matrix ("PSD") | none |
| Printed numbers | mean and peak of the map | none (the value under the pointer) |
| Colour scale | 0 … 0.5 unless typed | the 99th percentile of the map |

On `z3-hold-2k` (100 Hz, 10 deg/s, throttle held at 50 %) the reference
tool's map peaks at 0.35 in a spot 16 columns wide, OpenPID's at 4.03 in
one 21 columns wide; white noise of 5 deg/s RMS reads 0.064 as the mean of
the reference tool's map and about 0.5 in OpenPID's. Neither is wrong
about the signal: they are different quantities on different grids. What
OpenPID should show is entry D-005 of [DECISIONS.md](DECISIONS.md).
