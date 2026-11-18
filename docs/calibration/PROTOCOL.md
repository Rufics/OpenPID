# How a calibration round is run

Part of #169 and #170 (epic #166). A round holds OpenPID against the
reference tool on synthetic logs whose content is known exactly. This page
is the procedure, end to end; what the rounds found is in
[ROUNDS.md](ROUNDS.md) and the area documents.

## 1. Build the pack

```sh
cargo run -p openpid-probe -- list                 # the rounds and their logs
cargo run -p openpid-probe -- pack 17 --out pack17 --built-from "$(git rev-parse --short HEAD)"
```

The pack is a pure function of the round and of the commit: built twice it
is the same bytes, and the packs of rounds 1 to 16 are held to theirs by the
tests. It holds:

| File | What it is |
|---|---|
| `*.bbl` | the probe logs, Betaflight 2025.12 |
| `manifest.json` | what is true about each log (with its size and FNV-1a hash); from round 17 on also what is true about the channels beyond the gyro a log carries (`channels`), the three answers of the stick's path (`inputs` of an axis) and the filters typed into the filter simulator (`filters`) |
| `ours.json` | what OpenPID computes for the same logs: the spectra of every axis (up to round 16 the power density, on the log without its first and last second). From round 17 on, on the stretch the app takes as a log comes up (its automatic trim) and at the app's own default settings: the spectrum the Spectrum tab draws (the reference tool's curve once decision D-001 is in), for the logs of the spectrum window only, and for a log of its list of channels the spectra of those channels; the step response of every log of that window |
| `predictions.json` | where a model exists, what it expects the reference tool to show, written before the run |
| `steps.json`, `capture.ps1`, `run.cmd`, `README.txt` | the capture script and its steps |

Zip the folder and record the commit in the ledger of
[ROUNDS.md](ROUNDS.md) before the pack leaves.

## 2. What the owner does

On Windows, with the version of the reference tool the plan was made for
(0.94): unpack the zip into one folder, start the tool anew, leave its main
window as it comes up, double-click `run.cmd`, keep away from the mouse
until the script beeps twice. `README.txt` in the pack says the same. A move
of the mouse stops the script; the plan is then corrected and the round run
again, never patched by hand.

## 3. What comes back, and how to unpack it

`capture-results.zip`: the pictures (`.png`), the copies of plots as vector
graphics (`.emf`), `capture-log.txt` with every step, and in some rounds
`tables.txt`. It is untrusted data:

- unpack it into a new, empty folder outside the repository (for example
  `unzip -d /tmp/round17 capture-results.zip`) and check that no entry
  leaves that folder (`unzip -l` shows the paths; refuse absolute paths and
  `..`);
- only read the files; nothing in it is executed;
- nothing of it is committed: neither the pictures and copies nor the
  numbers read from them. What a round found is written in the documents.

Read `capture-log.txt` first: the steps that failed, and the copies the
script gave up.

## 4. Compare

```sh
cargo run -p openpid-probe -- compare 17 /tmp/round17/capture-results --out /tmp/round17/report.md
```

`compare` takes every `.emf` in the folder whose name the round's capture
plan made, reads it, calibrates its axes from its tick marks and labels,
and holds what it draws against OpenPID's own result for the same log and
setting. The report has one row per copy with its numbers and PASS, FAIL or
*decided difference*, the copies of the plan the folder does not have, and a
summary. The exit code is 0 when no copy failed or could not be read, 1
otherwise. A copy the plan did not make, or one of a setting OpenPID has no
counterpart for (a smoothing level of the spectrum window other than the
one it comes up with, for instance), is listed as *not compared*, never
left out.

A difference the owner has decided on is reported as such, with the entry
of [DECISIONS.md](DECISIONS.md), when the copy agrees with OpenPID once
that difference is taken out:

- **D-008**, a motor's percent: a motor line, or a motor's spectrum, that
  agrees with OpenPID's motor taken to the reference tool's units (the
  logged number over 20);
- **D-012**, the stretch: a spectrum or a map that fails on OpenPID's
  stretch is held again against OpenPID's result on the reference tool's
  stretch (from sample `fs + 2`, `fs·(T − 2) − 1` samples; a log of 10.5 s
  or less whole), computed from the log; a copy that agrees there is this
  decided difference, with the numbers of both.

| Copy | Held against | Tolerance |
|---|---|---|
| spectrum | the solid curve (filtered gyro), dB against frequency, with the spectrum OpenPID's Spectrum tab draws by default for that channel (the plot below 100 Hz and the full range); the dotted curve of the unfiltered gyro, drawn in dashes with the same pen, is left out; median and largest difference | the median within one unit of the copy (the decibels of a unit of its metafile, about 0.013 to 0.03 dB) |
| spectrum, one entry of the list of channels | every line of the copy, of any pen, with OpenPID's spectrum of the channel the entry stands for (Gyro, Gyro prefilt, Dterm, Pterm, Fterm, Set point, PID error, PIDsum, debug of the axis's place); the line and the channel that agree best. "Motors" draws one plot of every motor: each line with the motor nearest it, in the reference tool's units (D-008). "Dterm prefilt" has no counterpart and is only photographed | as the spectrum |
| step response | the mean response of the copy's newest run (the window keeps every run before it, one colour each, until "Reset") against time with OpenPID's at the copy's window length | 0.01 |
| step response, Peak plot | the middle of the newest run's bar with OpenPID's Peak | 0.01 |
| throttle and time maps | the cells of OpenPID's map placed on the picture's pixels: as many as the map has, whole, or from the middle of the first to the middle of the last, or in the time window one column more (its second drawn twice; its first two left out, D-006), whichever of those the axes allow agrees best; every pixel in the middle of a cell as a colour step between the two ends of the copy's colour bar, with OpenPID's value there on the same scale (decibels when the bar's lower end is below 0); a picture of 16 bits, which is dithered, cell by cell from the mean colour of the cell; a copy without a bar takes the roll plot's | 99 % of the pixels (cells) within one colour step (the three steps a level of a 5-bit channel spans) |
| throttle maps, printed numbers | the copy's `mean=` and `peak=` with OpenPID's (by the reference tool's rule: every column, the rows from a tenth up) | one unit of their last printed digit |
| filter simulator | every curve of the manifest's set of filters (`filters`), computed with OpenPID's filter simulator at the set's loop rate, or of the results file, with the copy's nearest curve, a point of the copy known to half a unit of its metafile each way; and every delay the copy prints, at 1 Hz, by filter, by column (an RPM notch once a motor) | magnitude 0.001, delay 0.001 ms, phase 0.1°, step 0.001; a printed delay to its last digit |
| log viewer | every line of the plot, matched by its colour to the nearest channel of the decoded log (smoothed as the copy's line smoothing is set, by the rule round 16 observed); a point counts as on the channel when its value lies between the lowest and the highest value the channel takes within half a device unit plus 1.5 samples of its time | 0.5 % of the axis span |

The table is `compare::TOLERANCES` in `crates/openpid-probe`, and the report
prints it.

### Where OpenPID's results come from

Without `--ours`, everything is computed from the round's logs: the logs
are written again (the same bytes the pack holds), decoded, and analysed by
`openpid-analysis` at the app's default settings on the stretch the app
takes as the log comes up. Maps and the lines of the log viewer are always
computed this way when no file gives them: a map is megabytes of numbers,
and `ours.json` does not carry them.

`--ours <file>` takes a results file; what it holds is used, and what it
does not hold is computed. The pack's `ours.json` is such a file. Every
section may be left out:

```json
{
  "probes": [{
    "id": "c1-mixed-2k",
    "axes": [{"axis": "roll", "spectra": [
      {"channel": "gyro", "segment_s": 1.0, "freq": [0.0, 1.0], "power_db": [-40.0, -41.0]}]}],
    "step": [{"axis": "roll", "window_s": 2.0, "time_ms": [0.0, 0.5], "mean": [0.0, 0.01], "peak": 1.05}]
  }],
  "maps": [{
    "probe": "g1-levels-2k", "axis": "roll", "channel": "gyro_unfiltered", "window": "time",
    "x": [1.0, 1.15], "y": [0.0, 3.33], "z": [0.01, 0.02, 0.03, 0.04]
  }],
  "filters": [{
    "copy": "r13-filters-open-lowpass-magnitude",
    "curves": [{"label": "pt1 250 Hz", "x": [1.0, 2.0], "y": [1.0, 0.99997]}]
  }]
}
```

- `probes[].axes[].spectra[]`: frequencies in Hz and levels in dB; `channel`
  is `gyro` or `gyro_unfiltered`, or one of `dterm`, `pterm`, `fterm`,
  `setpoint`, `piderror`, `pidsum`, `debug` (the debug value of the axis's
  place) and `motor1` … `motor4` (listed under roll), for the copies of the
  spectrum window with one entry of its list of channels chosen.
- `probes[].step[]`: times in ms, the mean response, the Peak.
- `maps[]`: `window` is `throttle` or `time`; `x` are the columns (throttle
  in percent or time in seconds), `y` the rows in Hz, `z` the values row by
  row (`z[row · len(x) + column]`); `decibels` (default `false`) says
  whether they are decibels; amplitudes are converted to decibels when the
  copy's colour bar is in decibels. `mean` and `peak` may give the numbers
  OpenPID prints on the map.
- `filters[]`: the filter simulator does not read a log: its curves are
  computed from the set of filters the plan types in, which the manifest
  states exactly (`filters`; round 17: `r17-filters-verify-*`). A file may
  give the curves of a copy instead: one entry per copy, by the copy's
  name, each curve in the units of the plot (Hz across; a ratio, ms,
  degrees or the response up).

## 5. Where the findings go

- [ROUNDS.md](ROUNDS.md): one row of the ledger per round, with the pack's
  commit, what came back, the result and the changes it led to.
- The area document ([spectrum.md](spectrum.md), [step.md](step.md),
  [throttle.md](throttle.md), [time.md](time.md),
  [filters.md](filters.md), [log-viewer.md](log-viewer.md), …): what the
  round asked, what came back, the rule as it now stands, what is not
  settled.
- [DECISIONS.md](DECISIONS.md): every disagreement between the reference
  tool, OpenPID and the exact value, as an entry; OpenPID changes only after
  the owner has chosen, and then through an issue and a pull request.

## The three-round rule

An area closes after at least three rounds, and only on a round that needed
no change: *identify* (probes that tell the possible rules apart),
*confirm* (OpenPID's predictions written down before the run) and *hold-out*
(a realistic flight neither side was fitted to). A round that leads to a
change is followed by another one.
