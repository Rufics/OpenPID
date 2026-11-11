# Spectrum: what the rounds have shown

Part of #171 (epic #166). The rows are S1–S8 of [INVENTORY.md](INVENTORY.md).

## Round 1, first part (2026-11-11)

The capture script ran all 342 steps, but the reference tool had loaded only
three of the nine logs: the name field of the "open" dialog holds about 259
characters and nine full paths are longer (fixed in the script; the round is
run again for the other six). The three it loaded are the level probes
`s1-levels-2k`, `s2-levels-1k`, `s3-levels-4k`: the same flight at three log
rates, each at the five smoothing levels, 15 pictures.

Each axis of those logs is a tone (or three) over a floor of known density:
0.5 deg/s RMS of white noise plus the rounding of the log, 1/3 (deg/s)² over
half the log rate.

### Readings

Read from the pictures by a script (the curve by its colour, the axes from
the plot frame; one pixel is 0.12 dB). "Excess" is the height of a tone's
peak above the floor of its curve.

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

One model reproduces all of it: the floor to 0.1 dB at the three rates, and
the 15 tone readings of each level to 0.2 … 0.9 dB RMS.

1. **The span**: the log without its first and last second, one sample more
   or less than 28 s times the log rate (55 999 or 56 001 at 2 kHz; with
   exactly 56 000 the tones would sit on their bins and their heights would
   not depend on the frequency, as they do in the table).
2. **One transform of the whole span**, through a Hann window: no averaging
   over short segments. The resolution is 1 / 28 s = 0.036 Hz.
3. **The level**: `2·|X|² / (fs·N)`, as `10·log10`. The loss of the window is
   not put back, so noise reads 4.26 dB below its density.
4. **The smoothing**: a moving average **of the decibel values** over about
   3, 21, 77–81, 191–201 and 401 bins for low, low-med, medium, med-high and
   high. There is no unsmoothed curve.

Two consequences follow from averaging decibels and not powers:

- Noise reads a further 2.51 dB low (the mean of the logarithm of a noisy
  spectrum is below the logarithm of its mean). Together with the window:
  **6.8 dB below the density** of the noise, at every smoothing level.
- A steady tone is one or three bins among hundreds and almost vanishes: at
  the default "medium" a tone of 30 deg/s, 76 dB above the floor of its own
  transform, shows as a bump of 5.7 dB. A peak keeps its height only when it
  is as wide as the smoothing span, as motor noise that moves with the
  throttle is.

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

- Whether the smoothing span is a number of bins or a width in hertz: the
  three logs have the same length, so the two cannot be told apart (a probe
  of another length, round 2).
- The exact shape of the kernel, most of all at "low" (the model's peak is
  0.5 dB under the reading), and which of the two sample counts it is.
- The other six probes of round 1: resolution, noise and smoothing,
  averaging over the log, the lowest bins and the panel below 100 Hz, the
  two channels, bands of known density.
