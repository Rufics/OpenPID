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
| `manifest.json` | what is true about each log (with its size and FNV-1a hash) |
| `ours.json` | what OpenPID computes for the same logs: the spectra of every axis; from round 17 on also the step response of every log of that window |
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
setting. The report has one row per copy with its numbers and PASS or FAIL,
the copies of the plan the folder does not have, and a summary. The exit
code is 0 when no copy failed or could not be read, 1 otherwise. A copy the
plan did not make, or one of a setting OpenPID has no counterpart for (a
smoothing level of the reference tool, for instance), is listed as *not
compared*, never left out.

| Copy | Held against | Tolerance |
|---|---|---|
| spectrum | the solid curve (filtered gyro), dB against frequency, with OpenPID's spectrum of that channel (1 s segments for the plot below 100 Hz, 0.25 s for the full range); largest and median difference | 0.05 dB |
| step response | the mean response against time with OpenPID's at the copy's window length | 0.01 |
| step response, Peak plot | the middle of the bar with OpenPID's Peak | 0.01 |
| throttle and time maps | every pixel inside a cell of OpenPID's map, as a colour step between the two ends of the copy's colour bar, with OpenPID's value there on the same scale (decibels when the bar's lower end is below 0); a copy without a bar takes the roll plot's | 99 % of the pixels within one step |
| throttle maps, printed numbers | the copy's `mean=` and `peak=` with those of OpenPID's map over the same cells | 1 % |
| filter simulator | every curve of the results file with the copy's nearest curve | magnitude 0.001, delay 0.001 ms, phase 0.1°, step 0.001 |
| log viewer | every line of the plot, matched by its colour to the nearest channel of the decoded log; a point counts as on the channel when its value lies between the lowest and the highest value the channel takes within half a device unit plus 1.5 samples of its time | 0.5 % of the axis span |

The table is `compare::TOLERANCES` in `crates/openpid-probe`, and the report
prints it.

### Where OpenPID's results come from

Without `--ours`, everything is computed from the round's logs as `pack`
computes them: the logs are written again (the same bytes the pack holds),
decoded, and analysed by `openpid-analysis` with its default settings on
the log without its first and last second. Maps and the lines of the log
viewer are always computed this way when no file gives them: a map is
megabytes of numbers, and `ours.json` does not carry them.

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
  is `gyro` (the only one compared) or `gyro_unfiltered`.
- `probes[].step[]`: times in ms, the mean response, the Peak.
- `maps[]`: `window` is `throttle` or `time`; `x` are the columns (throttle
  in percent or time in seconds), `y` the rows in Hz, `z` the amplitudes row
  by row (`z[row · len(x) + column]`); compare converts them to decibels
  when the copy's colour bar is in decibels.
- `filters[]`: the filter simulator does not read a log, so its curves can
  only come from a file: one entry per copy, by the copy's name, each curve
  in the units of the plot (Hz across; a ratio, ms, degrees or the response
  up).

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
