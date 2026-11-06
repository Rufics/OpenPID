# Step response: verdicts instead of raw metrics

The owner's remark (2026-11-05): improve Step response; propose how a user is
to understand the result.

## Cause

The monospace metrics line ("16 / 150 windows · rise 18 ms · delay 11 ms ·
latency 11.1 ms · overshoot 31 % · settling — ms · final 0.83") does not say
whether that is good. On the short 4.5.1 log the mean curve is noisy and "16
windows" gives no warning about unreliability. A PTB user is used to judging by
eye ("the curve near 1 without a hump"), not to numbers.

## Options

| | Option | Pros | Cons | Score |
|---|---|---|---|---|
| R0 | As is | — | numbers without a judgement | 24 |
| R1 | **Coloured badges** under the plot: `overshoot 31 % [too high]`, `rise 18 ms [ok]`… | instantly clear; cheap | no explanation of the thresholds | 46 |
| R2 | **A target band on the plot**: green 0.95–1.05 after 100 ms, a red line at 1.15 | the "hump" is seen by eye; the PTB canon ("near 1") | only for overshoot / settling | 45 |
| R3 | A verdict sentence with a link to Advice ("Roll overshoots 31 %: Advice proposes D 40 → 44") | ties it to the action | text fades without badges | 40 |
| R4 | **A three-axis table** roll/pitch/yaw with badges, no axis switching | everything at once; A/B in rows | three analyses instead of one (cached; 3 × 22 ms) | 47 |
| R5 | A single "goodness 0–100" score as in some tuners | one glance | hides the cause; a disputable formula | 30 |

Recommendation: **R1 + R2 + R4**, R3 as a line under the table when Advice has a proposal.

## The owner's decision (2026-11-05)

R1 + R2 + R4 + the R3 line.

## Implementation requirements

**Thresholds**, the single source for Step, Advice and the future A/B comparison:
`openpid-analysis::targets::StepTargets { overshoot_warn: 15, overshoot_fail: 30,
rise_ms_max: 60, settling_ms_max: 120, final_tol: 0.10, min_windows_ok: 10,
min_windows_weak: 5 }` plus the presets `Freestyle` (these), `Racing` (rise 40,
overshoot 20), `Cinematic` (overshoot 8, settling 100).
`RecommendParams.overshoot_max_pct` comes from `StepTargets` (remove the
duplication). The preset and the edits live in Settings → "Targets", in `Prefs`
and in `Project`.

**Verdicts** `step::verdicts(metrics, windows_used, targets) -> Vec<Verdict>`:
`Verdict { metric, value_text, level: Ok|Warn|Fail|Unknown, note }`.
Reliability: `windows_used ≥ 10` ok, 5–9 "weak: N windows", < 5 "unreliable"
(every other verdict becomes Unknown, in grey). `final_value` outside 1 ± tol →
"gain mismatch: enable Y-offset or check setpoint scaling".

**GUI.**
- Above the plot, instead of the metrics line, a table: rows roll / pitch / yaw
  (all three computed, the active axis highlighted and plotted; a click on a row
  switches the axis), columns windows · rise · delay · latency · overshoot ·
  settling · final; a cell = the value plus a coloured badge (green / yellow /
  red / grey, with the ASCII mark `ok`/`!`/`x` next to it for colour safety).
  With several slots, a row per slot and axis.
- On the plot: the band 1 ± tol (green, alpha 30) from `settling_ms_max` to the
  end; a horizontal line `1 + overshoot_warn/100` (yellow, dashed) and
  `1 + overshoot_fail/100` (red, dashed); labels on the left.
- A line under the table: when Advice for this session has a PID proposal on
  the axis, "Advice: roll D 40 -> 44 (overshoot 31 %)" with a "Go to Advice" button.
- Hovering a badge shows the threshold and the explanation.

**CLI.** `openpid run step` adds `verdicts` to the JSON.

## Acceptance criteria

- 4.2.0 roll: overshoot 35 % → Fail, pitch 37 % → Fail; 2025.12 pitch 6 % →
  Ok, roll 21 % → Warn; 4.5.1 session 1 (16 windows) → reliability Ok, yaw (0
  windows) → Unknown.
- Changing the preset in Settings changes the badges without recomputing the
  analysis (the verdicts are a pure function of the metrics).
- A headless test: the three-axis table, a row click switches the axis, the
  badges, the band on the plot (checked through the plot state), the Advice line.
- Unit tests of `verdicts` on edge values and `None`.

## Size

M (≈ 2 days).

## Implementation (2026-11-06, #103)

- `openpid-analysis::targets`: `StepTargets` (plus `Hash`, serde default), the
  presets `TargetPreset::{Freestyle, Racing, Cinematic}` and `matching()`,
  `verdicts()` as a pure function (7 verdicts in column order), `overall()`.
  Unit tests on edge values, `None` and too few windows; a check on real logs
  in `tests/real_logs.rs`.
- `RecommendParams.overshoot_max_pct` replaced by `targets: StepTargets` (the D
  rule fires above `overshoot_warn_pct`); old projects still load (serde
  default, the extra field is ignored).
- The thresholds live in `Prefs.targets` (Settings → Targets: presets, fields,
  "Custom" when no preset matches) and in `Project.recommend.targets`; at run
  time in `tools.recommend.targets`. The "overshoot limit" field on Advice
  edits the same threshold.
- The Step tab: an `egui::Grid` table of three axes × slots (all three axes
  computed through the cache); `App.step_view` keeps the verdicts, the band and
  the Advice line for the tests; the band 1 ± tol from `settling_ms_max` to the
  end of the response (a `Polygon` with fixed coordinates, so it does not affect
  the auto-bounds), dashed `HLine`s warn/fail; the Advice line reads the cached
  recommender result (it starts the job for the first visible slot).
- CLI: `run step --json` adds `verdicts` (the thresholds from `--project`,
  otherwise Freestyle).

Deviations: `delay` and `latency` are informational (green without a threshold,
with a note); `settling = None` is Warn ("never settles"), not Unknown; in 4.5.1
session 1 yaw has 55 windows (not 0, as the document assumed); the criterion was
checked on roll (16 windows → Ok by count, overshoot 30.5 % → Fail, final 0.8 →
gain mismatch).
