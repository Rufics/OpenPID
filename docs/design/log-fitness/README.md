# Is the log fit for use: a check after opening

The owner's remark (2026-11-05): work out and implement the "is the log fit"
concept: the user must see at once what this log can show and what it cannot.

## Cause

The user learns that `gyroUnfilt` or eRPM is missing only on the Filter Sim or
Motor noise tab, from a grey note. That the stick activity is too low, only from
"16 / 150 windows" in Step response. Nobody says that `debug_mode` or the log
rate has to change *before* the next flight.

## Options

| | Option | Pros | Cons | Score |
|---|---|---|---|---|
| R0 | As is: notes on the tabs | — | late and scattered | 22 |
| R1 | A collapsible banner under the header on every tab | noticeable | takes space permanently | 38 |
| R2 | A card in Diagnostics only | does not get in the way | nobody visits Diagnostics | 30 |
| R3 | A pop-up after opening | guaranteed to be seen | a modal on every log is annoying | 33 |
| R4 | A separate "Log check" tab | room for detail | one more tab among 12 | 34 |
| R5 | **Badges in the slot chip** ("1 kHz · 40 s · raw ✓ · rpm ✓ · sticks !") with tooltips, plus a full card in Diagnostics and a block in `openpid info` | always visible, takes no space; details on click | needs compact marks | **49** |

Recommendation: **R5**.

## The owner's decision (2026-11-05)

R5.

## Implementation requirements

**Pure logic** `openpid-analysis::fitness::check(session: &FlightSession) -> Fitness`:
`Fitness { checks: Vec<Check>, overall: Level }`, `Check { id: &'static str,
level: Level (Ok | Warn | Fail), title, detail, fix: Option<String> }`. The
checks (thresholds are constants with comments):

| id | Ok | Warn | Fail | fix (the hint) |
|---|---|---|---|---|
| `rate` | ≥ 1 kHz | 500–999 Hz | < 500 Hz | "set blackbox_sample_rate = 1/1" |
| `duration` | ≥ 20 s | 5–20 s | < 5 s | "log a longer flight" |
| `raw_gyro` | `gyro.raw.*` present | — | missing | "set debug_mode = GYRO_SCALED" (BF < 4.3) / "enable gyroUnfilt field" |
| `erpm` | `motor_rpm.*` present | missing, but the estimator exists | — | "set dshot_telemetry = ON, blackbox log motor rpm" |
| `frames` | corrupt < 0.5 % | 0.5–5 % | > 5 % | "check SD card / flash speed" |
| `gaps` | 0 | ≤ 3 | > 3 or > 1 s | — |
| `sticks` | ≥ 10 step windows on roll and pitch | 5–9 | < 5 | "fly sharper stick inputs for step response" |
| `throttle` | ≥ 50 % of the throttle bins with ≥ 3 segments | 25–50 % | < 25 % | "vary throttle through the range" |
| `debug_mode` | known | — | — | informational: what is in debug[0..7] |

`overall` = the worst level among `rate`, `duration`, `frames`; the rest only
warn. The step-window count comes from a cheap pass (the same window selection
as `StepAnalysis`, moved to `step::count_windows`).

**GUI.**
- The slot chip: after the duration, the badges `raw ✓/✗`, `rpm ✓/✗`,
  `sticks ✓/!/✗` and an overall dot (green/yellow/red); hover lists the checks
  with their text.
- Diagnostics: a "Log check" card above the table: a row per check (an ASCII
  icon `[ok] [!] [x]`, the name, the explanation, the fix in a monospace field
  with a Copy button).
- Tabs that lack the data (Filter Sim, Motor noise) show the same `fix`.

**CLI.** `openpid info` prints a `fitness:` block with the same lines; `--json`
adds `fitness` to `SessionSummary`.

**Behaviour.** The check runs once on opening (in the worker) and is cached in
`Loaded.fitness`.

## Acceptance criteria

- 2025.12: every check Ok (raw gyro and eRPM present); 4.2.0: `erpm` Warn with
  a fix, `raw_gyro` Ok (GYRO_SCALED); 4.5.1 session 0 (3 s): `duration` Fail,
  `sticks` Fail.
- Hovering the chip shows the list; Diagnostics shows the card; `openpid info`
  and the JSON carry the block.
- Unit tests of `fitness::check` on the three fixtures and on synthetic data; a
  headless test of the badges.

## Size

S (≈ 1–1½ days).

## Implementation (2026-11-05, #100)

- `openpid-analysis::fitness::check_session` + `step::count_windows`; the result
  in `Loaded.fitness: Vec<Fitness>` (per session), computed in the worker on opening.
- GUI: badges in the slot row (`raw [ok]`, `rpm [!]`, `sticks [x]` plus the
  overall marker, hover shows the explanation and the fix), the "Log check" card
  on Diagnostics with a Copy button, Filter Sim and Motor noise show the fix of
  the `raw_gyro` check.
- CLI: `openpid info` prints the `fitness` block, `--json` adds `fitness` to the session.
- Deviations from the threshold table after checking against real logs:
  `rate` Ok from **950 Hz** (1 kHz with jitter logs 997 Hz); `throttle` counts
  the coverage with a ±3 % throttle window, as the heatmap does, Ok from **30 %**
  and Warn from 10 % (a 40 s freestyle log covers ~30 % of the range, not 50 %);
  the dots became the ASCII markers `[ok] [!] [x]` (the egui font lacks the glyphs).
