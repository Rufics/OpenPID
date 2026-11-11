# Calibration readings

Numbers read from the reference tool for the probe logs of the calibration
rounds (`docs/calibration/ROUNDS.md`, epic #166). Nothing of the tool itself
is here: only what it showed for logs that `openpid-probe` writes, so that a
test can hold a model of it, or OpenPID, against those numbers.

## spectrum/

The curve of the Spectral Analyzer, from plots copied as vector graphics
(the coordinates of the copy converted to hertz and decibels with the axis
lines of the same copy). A file is `bin,db`: the index of the point in the
one-sided transform of the analysed span, and the level drawn there.

| File | Probe | Plot | Points |
|---|---|---|---|
| `r1-s9-bands-2k-medium-yaw-sub100.csv` | `s9-bands-2k` (round 1) | yaw, "Sub 100Hz", smoothing "medium" | every point from 0 to 100 Hz |
| `r1-s9-bands-2k-medium-pitch-full.csv` | `s9-bands-2k` (round 1) | pitch, "Full Spectrum", smoothing "medium" | the 3 636 points the copy keeps of the 28 000 |

| `r2-<probe>-medium-yaw-sub100.csv`, six files | the six probes of round 2 | yaw, "Sub 100Hz", smoothing "medium" | every point from 0 to 100 Hz, but for `c5-long-2k`, of which the copy keeps 3 192 of 4 500 |
| `r2-c1-mixed-2k-<level>-yaw-sub100.csv`, four files | `c1-mixed-2k` (round 2) | the same plot at "low", "low-med", "med-high" and "high" | every point from 0 to 100 Hz |

The first two were taken on 2026-11-11 in the second trial of the capture
script, with the log loaded alone; the others the same day in round 2, six
logs loaded together.
