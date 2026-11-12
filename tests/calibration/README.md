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
| `r3-<probe>-medium-roll-sub100.csv`, four files | the flown flight of round 3 at 2, 1, 4 and 8 kHz | roll, "Sub 100Hz", smoothing "medium", the upper limit of the plot raised to 70 dB | the points of the copy from 0 to 100 Hz (all of them at 1 and 2 kHz) |
| `r3-h2-flight-1k-low-roll-sub100.csv` | `h2-flight-1k` (round 3) | the same plot at "low" | the 917 points the copy keeps of 1 601 |
| `r3-h5-bands-2k-medium-yaw-sub100.csv` | `h5-bands-2k` (round 3), the flight of `s9-bands-2k` | yaw, "Sub 100Hz", smoothing "medium" | every point from 0 to 100 Hz |

The first two were taken on 2026-11-11 in the second trial of the capture
script, with the log loaded alone; the others the same day in rounds 2 and
3, with the logs of the round loaded together. A point where a curve
leaves its plot is not a point of the curve and is left out.

## step/

The step response window, from plots copied as vector graphics in round 4
(2026-11-11). A curve file is `sample,value`: the index of the point (one
per sample of the log, from 0 at 0 ms to half a second) and the response
drawn there. A point within 0.0005 of the plot's limits (0 and 1.5) is left
out: where a curve leaves its plot, the copy holds the limit and not the
curve.

| File | Probe | What |
|---|---|---|
| `r4-<probe>-<axis>-mean.csv`, twelve files | nine probes of round 4 | the mean curve of the axis (the newest "test" of the plot), smoothing off |
| `r4-t1-plants-2k-roll-mean-<level>.csv`, `r4-t5-noise-2k-yaw-mean-<level>.csv` | a pure delay; the second-order plant under 20 deg/s RMS of gyro noise | the mean curve at "low", "medium" and "high" smoothing |
| `r4-t1-plants-2k-yaw-window-0.csv`, `…-window-116.csv` | `t1-plants-2k`, yaw | two of the 259 single-window responses the tool draws with "Raw" on: the first and the 117th |
| `r4-points.csv` | all nine probes, three axes | the number of windows in the legend, the point and the bar of the "Peak" plot, of "SR Latency" and the point of "SP-Gyro Delay"; no Peak where the point is outside its plot |
