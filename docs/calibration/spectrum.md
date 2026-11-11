# Spectrum: what the rounds have shown

Part of #171 (epic #166). The rows are S1–S8 of [INVENTORY.md](INVENTORY.md).

## Round 1 (2026-11-11)

Nine probe logs of 30 s, the spectrum of each at the five smoothing levels:
45 pictures, taken by the capture script (342 steps, 3.6 min). The round
was run twice. In the first run the reference tool loaded three logs of
nine: the name field of its "open" dialog holds about 259 characters and
nine full paths are longer. The script now enters the folder first and then
the bare names; the second run (pack `a82756a`) loaded all nine. What is
written here is read from the second run; the first gave the same numbers
for the three logs it had.

### The probes

Every axis carries its signal over the same floor: 0.5 deg/s RMS of white
noise plus the rounding of the log to whole deg/s, a density of
1/3 (deg/s)² spread over half the log rate. The probes are in
`crates/openpid-probe` (`catalogue::round(1)`).

| Probe | Roll | Pitch | Yaw | Asks |
|---|---|---|---|---|
| `s1-levels-2k`, `s2-levels-1k`, `s3-levels-4k` | 100 Hz, 3 deg/s | 100 Hz, 30 deg/s | 48, 200, 400 Hz, 10 deg/s | the level of a tone and of the floor, at three log rates |
| `s4-resolution-2k` | 100 and 104 Hz | 100 and 101 Hz | 100 and 302.3 Hz | the resolution; a tone between two bins |
| `s5-noise-2k` | white noise, 10 deg/s RMS | the same with a 152 Hz tone of 20 deg/s | white noise, 3 deg/s RMS | the level of noise of a known density |
| `s6-averaging-2k` | 100 Hz, 20 deg/s, first half of the log | the same, the whole log | the same, 10 … 11 s only | whether the log is averaged in pieces |
| `s7-low-2k` | offset 30 deg/s, 20 Hz | ramp −50 … 50 deg/s, 20 Hz | 5, 20, 60, 61 Hz | the mean, a drift, the lowest bins |
| `s8-channels-2k` | 100 Hz before the filters, 248 Hz after | the same on both | 20 deg/s before, 10 after | the two channels of one picture |
| `s9-bands-2k` | band 100 … 200 Hz, density 1 | bands of 4, 20 and 80 Hz at 100, 200, 400 Hz | density 0.1 at 20 … 150 and 200 … 900 Hz | the level of a band and what its edge looks like |

### How the pictures are read

By a script: the curve by its colour, the axes from the plot frames. One
pixel is 0.12 dB. A pixel column of the "Full Spectrum" panel covers 0.7 Hz
at 2 kHz, which is 19.5 points of the reference tool's curve; a column of
the "Sub 100Hz" panel covers 0.07 Hz, two points. So the panel below 100 Hz
shows the curve almost point by point, and the comparison leans on it.

A model of the reference tool is compared with a picture as a picture: its
curve is drawn as a line of the tool's width on the same pixel grid, and the
top and the bottom of the line in every column are compared with the top and
the bottom of the red in the screenshot. "RMS" below is over those columns.

After the round the owner pointed out that every plot can be copied as a
vector graphic. A trial of the capture script on `s9-bands-2k` (2026-11-11,
52 steps, three copies) showed what such a copy holds: a metafile whose
curve is a path of points with fractional coordinates, next to the axis
lines and the tick labels that place it. The panel below 100 Hz carries
every point of the curve (2 802 for 0 … 100 Hz); the Full Spectrum panel
carries 3 636 of its 28 000, each of them a true point. A point is known to
about 0.00001 dB. The section "Checked on the points of the curve" below is
read that way; the readings of the round itself are from pixels.

### Readings of the level probes

"Excess" is the height of a tone's peak above the floor of its curve; the
model in brackets is the moving-average one of the first run, which is why
it is up to 0.9 dB off.

Floor of the smoothed curve, dB:

| Log rate | low-med | medium | high | Model below |
|---|---|---|---|---|
| 1 kHz | −38.6 | −38.6 | −38.7 | −38.62 |
| 2 kHz | −41.7 | −41.6 | −41.6 | −41.56 |
| 4 kHz | −44.7 | −44.6 | −44.6 | −44.57 |

Peak of the 100 Hz tone of 3 deg/s at "low": 11.5, 11.4 and 11.5 dB at 1, 2
and 4 kHz. The tones of 10 and 30 deg/s leave the top of the default axis at
"low".

Excess of the tones over the floor at 2 kHz, dB (in brackets: the model):

| Tone | low-med | medium | med-high | high |
|---|---|---|---|---|
| 100 Hz, 3 deg/s | 9.5 (10.2) | 1.7 (1.9) | 0.5 (0.7) | 0.3 (0.4) |
| 100 Hz, 30 deg/s | 19.8 (19.7) | 5.7 (5.4) | 2.6 (2.3) | 1.4 (1.2) |
| 48 Hz, 10 deg/s | 12.9 (13.4) | 3.5 (3.7) | 1.4 (1.7) | 1.0 (1.0) |
| 200 Hz, 10 deg/s | 15.8 (15.7) | 4.9 (5.5) | 2.3 (2.1) | 1.4 (1.2) |
| 400 Hz, 10 deg/s | 18.6 (18.4) | 5.0 (4.6) | 1.8 (1.8) | 1.1 (1.0) |

### What the reference tool computes

One model reproduces the 45 pictures, on all six panels of each.

1. **The span**: the log without its first and last second, `28·fs − 1`
   samples (27 999, 55 999 and 111 999 at 1, 2 and 4 kHz). With exactly
   `28·fs` samples every tone of the probes would sit
   on a bin; with one less it sits a little off, its skirts rise, and that
   is what the pictures show: around the 48 Hz tone of the 1 kHz log the
   model with `28·fs − 1` samples is 0.10 dB RMS from the picture, with
   `28·fs` it is 0.35 dB and with `28·fs + 1` it is 0.20 dB.
2. **Nothing is removed first**: no mean, no trend. The offset and the ramp
   of `s7-low-2k` stand at 0 Hz as a rise of 7 and 17 dB at "medium". A
   model that removes the mean misses the roll picture there by 2.9 dB RMS;
   without it the miss is 0.3 dB.
3. **One transform of the whole span**, through a Hann window (the
   symmetric one, zero at both ends): no averaging over shorter pieces. The
   resolution is 1 / 28 s = 0.036 Hz.
4. **The level**: `2·|X|² / (fs·N)`, as `10·log10`; the point at 0 Hz is
   not doubled. The loss of the window is not put back, so noise reads
   4.26 dB below its density.
5. **The smoothing** works on the decibel values and is a local regression:
   at every point a straight line is fitted to the nearest `m` points with
   tricube weights, and its value at that point is the curve. Away from the
   ends this is a weighted mean with a bell-shaped kernel. At 0 Hz and at
   half the log rate the window cannot be symmetric, the line is fitted to
   the `m` points that exist, and the curve keeps its slope into the end of
   the plot. This is how the pictures of `s7-low-2k` look below 3 Hz: at
   "medium" the fitted line is 0.3 and 0.7 dB RMS off on roll and pitch,
   where a weighted mean (renormalised, or mirrored at 0 Hz) is 1.0 … 2.2 dB
   off; at "high" it is 0.10 and 0.13 dB against 0.3 … 0.7 dB.
6. **Every channel of a picture** goes through the same steps
   (`s8-channels-2k`: the solid and the dotted curve).

The spans, as the number of points with a weight above zero (the window of
the regression is two points longer: the weight falls to zero at its ends):

| Level | Points | Half-width, points | Half-width for these 28 s logs |
|---|---|---|---|
| low | 3 | 2 | 0.07 Hz |
| low-med | 33 | 17 | 0.6 Hz |
| medium (the default) | 119, exact | 60 | 2.1 Hz |
| med-high | 283 | 142 | 5.1 Hz |
| high | 551 | 276 | 9.9 Hz |

"Medium" is exact (below); the other four are fitted to pixels and can be a
few points off. There is no unsmoothed curve.

### Checked on the points of the curve

The vector copies of the trial are the "medium" curves of `s9-bands-2k`:
yaw below 100 Hz (2 801 points on the grid of the transform) and pitch over
the whole range (3 636 points, from 0 Hz to half the log rate). The model
above gives every one of them:

| Curve | Points | Largest difference | RMS |
|---|---|---|---|
| yaw, 0 … 100 Hz | 2 801 | 0.000016 dB | 0.000005 dB |
| pitch, 0 … 1000 Hz | 3 636 | 0.000009 dB | 0.000004 dB |

That is the precision of the coordinates in the copy. What it fixes:

- **The samples**: 55 999 of them, the first being sample 2003 of the log
  as OpenPID decodes it (counting from 0; 1.0015 s after the first). One
  sample earlier or later is 0.0006 dB off. Why three samples past the
  second is not known; a log of another rate or length will tell whether it
  is a count or a time.
- **The window**: the symmetric Hann. The periodic one is 0.0001 dB off.
- **The point at 0 Hz is not doubled.** With it doubled the first 60 points
  are up to 0.14 dB off, the rest unchanged.
- **The kernel**: solved from the points alone (2 651 equations, 151 free
  weights), it is the tricube with zeros at ±60 points to 0.000001 in every
  weight, and it sums to 1.000000.
- **The ends**: a line fitted to the 121 nearest points, the weights scaled
  to the farthest of them. Exact at 0 Hz and at half the log rate.

The first run took the smoothing for a plain moving average
of 3, 21, 79, 197 and 401 points. The panel below 100 Hz tells the two
apart; RMS in dB on the three axes of `s5-noise-2k` (white noise):

| Level | Moving average, best span | Local line with tricube weights |
|---|---|---|
| low-med | 0.46 (23 points) | 0.18 |
| medium | 0.23 (79) | 0.07 |
| med-high | 0.15 (185) | 0.06 |
| high | 0.11 (353) | 0.06 |

At "low" both are three points wide and cannot be told apart (1.1 dB RMS
either way: next to a line that jumps by several decibels from one point to
the next the pixels say little).

Agreement of the model with all nine probes, RMS in dB, the lowest and the
highest of the three axes over the nine logs:

| Level | Sub 100Hz panels | Full Spectrum panels |
|---|---|---|
| low | 1.06 … 1.81 | 0.83 … 2.54 |
| low-med | 0.20 … 0.42 | 0.50 … 1.16 |
| medium | 0.06 … 0.16 (0.36 on `s8`, two curves in one plot) | 0.19 … 0.67 |
| med-high | 0.04 … 0.13 (0.29 on `s8`) | 0.10 … 0.39 |
| high | 0.04 … 0.11 (0.24 on `s8`) | 0.06 … 0.25 |

From "medium" up the model is within one pixel of the picture. The Full
Spectrum panels are coarser because a column holds twenty points.

### What follows from averaging decibels

- **Noise reads 6.8 dB below its density** at every smoothing level: 4.26 dB
  from the window and 2.51 dB because the mean of the logarithm of a noisy
  spectrum lies below the logarithm of its mean. `s5-noise-2k`: white noise
  of 10 and of 3 deg/s RMS has a density of −10.0 and −20.4 dB and reads
  −16.8 and −27.2 dB.
- **A band reads by its density, whatever its width**, once it is wider
  than the span: the bands of `s9-bands-2k` built at a density of 1 and of
  0.1 read −5.96, −6.38 and −16.36 dB at "high" (the model: −5.96, −6.38,
  −16.37). The band of 4 Hz is narrower than the span of "high" and sinks
  to −28.8 dB there.
- **A steady tone almost vanishes.** It is three points among hundreds: at
  "medium" a tone of 30 deg/s, 76 dB above the floor of its own transform,
  is a bump of 5.7 dB.
- **A tone that is not steady stands tall** (`s6-averaging-2k`, "medium"):
  the 100 Hz tone of 20 deg/s that lasts the whole log is a bump of under
  4 dB;
  the same tone present for the first half of the log is a peak of 40 dB
  with skirts 50 Hz wide, and present for one second a peak of 38 dB.
  Switching a tone on or off spreads it over many points of the one long
  transform, and what is wide survives the smoothing.
- **A tone between two points of the transform reads higher than one
  near a point** (`s4-resolution-2k`, yaw, "low-med"): 302.3 Hz reads
  −14 dB next to 100 Hz at −22 dB, both of 30 deg/s.
- **Two tones are told apart when they are further apart than the span**:
  100 and 104 Hz are two lines at "low-med" (span 1.2 Hz), 100 and 101 Hz
  are one.

So the height of a peak in this curve measures how wide the peak is at
least as much as how strong it is. Motor noise that moves with the throttle
is wide and is shown well; a resonance at a fixed frequency is narrow and is
shown weakly.

### OpenPID on the same logs

Welch's method: 0.25 s Hann segments, half overlapping, the mean of their
powers, the window's loss put back, 4 Hz bins; the curve is smoothed by a
moving average of the power over 8 Hz. It reads the density exactly (the
floor of the 2 kHz log at −34.8 dB, checked in `crates/openpid-probe`) and a
tone as its power over 1.5 bins (15.2 dB for 20 deg/s).

| On the 2 kHz log | Reference tool, medium | OpenPID | Exact |
|---|---|---|---|
| Floor (density 1/3000 (deg/s)²/Hz) | −41.6 dB | −34.8 dB | −34.8 dB |
| Tone of 30 deg/s above the floor | 5.7 dB | 53.5 dB | a line: its height is a matter of resolution |

The two tools draw different things under one axis label. This is decision
D-001 of [DECISIONS.md](DECISIONS.md).

### Not settled yet

- **Points or hertz.** All logs so far are 28 s long after the trim, so a
  span of 119 points and a span of 4.25 Hz are the same thing. A probe of
  another length tells them apart (round 2).
- **The spans of the other four levels**, exactly, and whether they are the
  same at every log rate. Fitted to pixels on one tone, the span of "medium"
  came out as 107, 119 and 127 points at 1, 2 and 4 kHz; on the exact points
  of the 2 kHz log it is 119. Round 2 reads all levels at three rates from
  vector copies.
- **The start of the span** at other rates and lengths (the three samples
  above).
