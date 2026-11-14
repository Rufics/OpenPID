# Freq × time: what the rounds have shown

Part of #174 (epic #166). The rows are F1–F3 of [INVENTORY.md](INVENTORY.md).
The reference tool is version 0.94.

## Round 10 (2026-11-14): identify

Four probe logs, written as Betaflight 2025.12 logs (pack `a022b4d`). On
every axis one experiment on the unfiltered and one on the filtered gyro:

| Log | Length, rate | What it holds |
|---|---|---|
| `f1-bursts-2k` | 30 s, 2 kHz | roll: 100 Hz at 10 deg/s for 0.5, 1, 2, 0.1 and 0.02 s (unfiltered), 200 Hz at 2 deg/s all the time (filtered); pitch: a tone that climbs evenly from 50 to 450 Hz, pairs of tones 4, 8 and 16 Hz apart; yaw: white noise of 5 deg/s RMS, noise between 100 and 300 Hz only |
| `f2-steps-2k` | 30 s, 2 kHz | roll: a tone whose frequency jumps every three seconds, 300 Hz at 1, 2, 5, 10, 20 and 40 deg/s for five seconds each; pitch: an offset, a drift and a tone of 20 Hz; yaw: white noise of 20 and of 1 deg/s RMS |
| `f3-bursts-4k` | 30 s, 4 kHz | as `f1-bursts-2k` |
| `f4-short-2k` | 16.5 s, 2 kHz | as `f1-bursts-2k` |

For every log the three maps of the unfiltered and of the filtered gyro as
the window comes up; for the first log also the view below 100 Hz, the
maps with −70 typed into "Z min", and a picture of each of the two
smoothing lists held open: 30 vector copies of maps and 11 pictures, none
failed.

### The window

A list of logs and a list of channels, "Run", two lists "smooth freq axis"
and "smooth time axis" (each: off, low, med, high; both come up at "low"),
a colour map and a line width, a mark "< 100Hz", "Z min" and "Z max" (−30
and 10), marks for motor lines, lists for RPM and dynamic notch lines, a
player. Each map carries a colour bar labelled "Power Spectral density
(dB)". There is no linear colour.

### What a copy holds

A copy of one map is a picture of 2750 × 589 pixels with a palette, and
its tick labels as text. As in the throttle window the colours are the 256
steps of one colour map between the two ends of the colour bar, so a pixel
gives the cell to 40 / 256 = 0.16 dB. The columns are equal cells (14.9
pixels each for 185 columns); at some cell edges one pixel column is a
blend of the two cells. The rows are the bins from 0 to half the log rate,
the first and the last half a cell high: 301 rows at 2 kHz; at 4 kHz 601
rows on 589 pixels, so a few are not drawn.

## The rule

With both lists as the window comes up ("low"), and what the other items
of the lists change (round 11):

1. **Stretch and segments** as in the throttle window: the first and the
   last second are left out, and a segment is `L = round(0.3 · fs)`
   samples. The list "smooth time axis" does not smooth: it sets how far
   apart the segments are. "off": a segment (0.3 s); "low": half a segment
   (0.15 s); "med": a fifth (0.06 s); "high": a tenth (0.03 s). Seen on a
   stretch of 28 s at 2 kHz at all four, and at "low" also on 14.5 s and
   at 4 kHz.
2. **A cell before smoothing**: the segment under a symmetric Hann window,
   `10·log10(2·|X|² / (L·fs))`, and the bin of 0 Hz without the 2. The
   rows are the bins 0 … `L/2`. A tone of amplitude `A` on a bin reads
   `20·log10(A) − 14.3` dB on that bin and 6 dB less on the two beside it.
3. **Smoothing along the frequency** ("smooth freq axis"), on the
   decibels: a row becomes the value, at that row, of the straight line
   fitted to the nearest rows with the weights `(1 − (d / d_max)³)³`,
   where `d` is the distance in rows and `d_max` the largest among them.
   "off": nothing; "low": 5 rows; "med": 11 rows; "high": 21 rows. Away
   from the ends of the axis that is a weighted mean (at "low" of three
   rows with 0.286, 0.427 and 0.286: the outermost rows weigh nothing); at
   the two ends the rows lie to one side and the line is not level. At
   "low" a tone on a bin loses 3.4 dB by it: it reads
   `20·log10(A) − 17.7` dB.
4. **Along the time** nothing is smoothed at any setting: a column is one
   segment.
5. **The columns drawn**: column `k` (from 0) stands at the time
   `1 + hop·k` s of the axis, up to `k = floor((n − 2·fs − 1 − L) / hop)`.
   From column `L / hop` on it is the segment whose middle is at that
   time (at sample `fs + 2 + hop·k`). The first `L / hop` columns (1, 2, 5
   or 10 by the list) are the segments that *start* at their time, so
   they sit half a segment early, and the last of them is drawn again by
   the first of the others: at "low" the first segment, the second, the
   second again, the third, and so on.
6. **Colours** from −30 to 10 dB; a number typed into "Z min" moves the
   lower end (the copies made with −70 follow the same matrix), one typed
   into "Z max" the upper end (a picture with 0, round 11).
7. **The view below 100 Hz** is the rows 0 … 100 Hz of the same matrix.

### How well it holds

All 30 copies, pixel by pixel: a median of 99.9 % of the pixels of a map
within one colour step (worst 97.3 %, the noise in the view below 100 Hz);
what is left are the blended pixel columns at the cell edges. Read at the
cell centres, the 24 copies at 2 kHz are 1 024 050 cells, and every one of
them is the very colour step the rule gives, once the first segment of
`f4-short-2k` starts at `fs + 3` instead of `fs + 2`.

What the probes were built to show, as read from the copies:

| Asked | Came back |
|---|---|
| 300 Hz at 1, 2, 5, 10, 20 and 40 deg/s | about −17.7 (the log holds whole deg/s, and a tone of 1 deg/s wavers between −17.1 and −18.7), −11.8, −3.7, 2.3, 8.4 dB and the top of the scale (the rule for a clean tone: −17.7, −11.7, −3.7, 2.3, 8.3, 14.3) |
| 100 Hz for 0.5 s | full height (2.3 dB) in three columns, and 11 dB less in one column on each side |
| 100 Hz for 0.1 s and for 0.02 s | one column at 0.7 dB with −25 dB beside it; two columns at −15 and −21 dB |
| White noise of 5 deg/s RMS (−16 dB per Hz) | every cell as the rule gives it; no trace of a second column in any cell (no smoothing along the time) |
| The same at 4 kHz and in 16.5 s | the same rule: 0.3 s and 0.15 s in seconds, 184 and 94 segments |

## Round 11 (2026-11-14): the two lists

Seven logs (pack `a2cf0c2`): the first log of round 10 again
(`g1-levels-2k`), the same in flights of 8, 45 and 84 s and at 1 and
8 kHz, and a flight made for no question. Planned: for every log the maps
as the window comes up, for the first also at eight other settings of the
two lists and with 0 typed into "Z max", for the flight the filtered gyro
and two settings more; 57 vector copies.

**What came back is the first log only**, at all nine settings of the
lists (27 copies), and a picture with "Z max" at 0. The script stood still
for a minute after several of its Runs, the owner moved the mouse, and a
move of the mouse stops the script (step 504 of 1 033). The cause is in
the plan, not in the tool: this window redraws as soon as a list or a box
is set, and the Run after it changes nothing; the script took its
reference picture at the Run and then waited its full minute for a change
that had already happened. The plan now takes the reference picture
before the first change and waits 25 s at most.

| Asked | Came back |
|---|---|
| "smooth freq axis" off, med, high | no smoothing; the line through 11 rows; through 21 rows. With each, every cell of the noise map (55 685 cells) is the very colour step |
| "smooth time axis" off, med, high | 93, 462 and 924 columns in place of 185: segments 0.3, 0.06 and 0.03 s apart in place of 0.15 s. No smoothing along the time at any setting: every column is one segment of 0.3 s |
| Both lists off, both high | the two settings together, nothing else |

With the rule as it stands above, all 27 copies are the rule's, cell by
cell: 2 922 108 cells, every one the very colour step.
| "Z max" 0 | the colour bar ends at 0 dB |

### Not settled

- **Other lengths and rates**: the throttle window puts its segments
  0.06, 0.1 or 0.3 s apart by the length of the stretch; here the
  distances of the list were seen on a stretch of 28 s (and "low" on
  14.5 s and at 4 kHz) only. The logs for it (8, 45 and 84 s; 1 and 8 kHz) and the
  flight made for no question are in the pack of round 11 and were not
  reached.
- **The first sample**: `fs + 3` in one log of four in round 10, `fs + 2`
  in the others (the throttle window does the same on two logs of
  eighteen, see [throttle.md](throttle.md)); what decides it was not
  found. It moves a segment by half a millisecond.
- The motor, RPM and notch lines, the player, other channels.

## OpenPID on the same logs

OpenPID's map (`openpid run stft`, INVENTORY F1 and F2) is another
quantity on another grid:

| | The reference tool | OpenPID |
|---|---|---|
| Segments | 0.3 s, every 0.15 s (or 0.3, 0.06, 0.03 s); the first and the last second left out | 0.2 s, every 0.05 s; the whole log |
| A cell | a power density in decibels: a tone of amplitude `A` reads `20·log10(A) − 14.3` dB before smoothing | an amplitude, shown as `20·log10(A)` |
| Smoothing | along the frequency, a line through 5 rows on the decibels (or none, 11, 21) | none |
| Rows | to half the log rate | to 1 000 Hz |
| Colour scale | −30 … 10 dB unless typed | from the map: the 30th and the 99.5th percentile; or typed |

The tone of 300 Hz in `f2-steps-2k` at 1, 2, 5, 10, 20 and 40 deg/s reads
−17.7, −11.8, −3.7, 2.3, 8.4 dB and the top of the scale in the reference
tool, and −0.1, 6.1, 14.2, 20.0, 26.0 and 32.0 dB in OpenPID: 17.7 dB
apart, all the way. What OpenPID should show was entry D-006 of
[DECISIONS.md](DECISIONS.md): the owner chose the reference tool's map
(2026-11-14), with every column at the middle of its segment.
