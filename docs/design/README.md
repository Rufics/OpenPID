# Interface design decisions

Every noticeable interface change gets a document `docs/design/<topic>/README.md`:
the owner's remark → the cause (measured, not guessed) → options that differ in
approach → a score from the matrix → the decision → implementation requirements
→ acceptance criteria → tests. The document is written **before** the
implementation and is the specification for the developer; after the
implementation an "Implementation" section and a screenshot of the result are
added.

## The scoring matrix

A score of 1–5 per criterion × weight, 55 at most. The option with the highest
score is recommended; on a tie, the cheaper one. The owner's decision beats
the scores.

| Criterion | Weight | What is measured |
|---|---|---|
| Task efficiency | 3 | how many actions and seconds to the result in the "log → settings change" loop |
| Discoverability | 2 | whether the feature is noticed without documentation |
| Convention | 2 | whether PIDtoolbox, Blackbox Explorer, Adobe/MS/VS Code do it that way |
| Keyboard and accessibility | 2 | control without a mouse, accessible names (the kittest check is on) |
| Scaling / touch | 1 | 1280×800 and touch mode, 44 pt targets |
| Cost | 1 | implementation size (XS ≈ ½ day … L ≈ 5 days) |

## Sprint UX-1 (milestone v1.1, epic [#98](https://github.com/Rufics/OpenPID/issues/98))

| Topic | Document | Issue |
|---|---|---|
| N logs instead of A/B | [multi-slot](multi-slot/README.md) | [#105](https://github.com/Rufics/OpenPID/issues/105) |
| First run: tour and start screen | [onboarding-tour](onboarding-tour/README.md) | [#104](https://github.com/Rufics/OpenPID/issues/104) |
| An analysis window per log | [analysis-window](analysis-window/README.md) | [#102](https://github.com/Rufics/OpenPID/issues/102) |
| Is the log fit for use | [log-fitness](log-fitness/README.md) | [#100](https://github.com/Rufics/OpenPID/issues/100) |
| Heatmap colour scale | [colour-scale](colour-scale/README.md) | [#99](https://github.com/Rufics/OpenPID/issues/99) |
| Step response: verdicts | [step-verdicts](step-verdicts/README.md) | [#103](https://github.com/Rufics/OpenPID/issues/103) |
| Keyboard shortcuts | [hotkeys](hotkeys/README.md) | [#101](https://github.com/Rufics/OpenPID/issues/101) |

Rules shared by every topic of the sprint:

- **Language**: every new label goes through `tr(…)` with a Ukrainian translation
  in `i18n.rs`; the egui font has no glyphs for ▶ ⏸ → ● U+2011, so ASCII and the
  basic Latin/Cyrillic ranges only.
- **Accessibility**: every interactive widget has an accessible name
  (`labelled_by`), otherwise the headless test fails.
- **State**: tool parameters live in `Tools` and in `Project` (`#[serde(default)]`),
  user settings in `Prefs`; no mathematics in `openpid-app`.
- **Tests**: every topic has a headless kittest test on a real fixture plus unit
  tests of the pure logic in `openpid-analysis`/`openpid-core`; coverage stays ≥ 90 %.
- **Documentation**: the USER-GUIDE and the screenshots (`tools/screenshots.sh`)
  are updated in the same PR.
- **Control panels** (owner's rule, 2026-11-09, every tab's right-hand panel):
  - one row per field: the **label on the left, the value right-aligned** in one
    column (combo boxes `panel::VALUE_W` wide, numbers at the right edge);
  - **sections** with a small upper-case muted title (DISPLAY, ANALYSIS…);
  - labels in sentence case ("Cut-off", "Min input"); option values without
    repeating the label ("medium", not "smoothing medium");
  - **units beside the number, not inside it** (`panel::with_unit`, in a
    column of `panel::UNIT_W`): a `DragValue` suffix disappears while the
    value is typed, the unit beside it stays; this holds for every number
    field of the app, panels or not;
- **Plot grids** (owner's remark of 2026-11-09 on a trimmed epoch): every plot
  of the reworked tabs steps its grid on the 1–2–5 series with nested steps
  and labels with the decimals its step needs (`grid.rs`: `spacer_x`,
  `spacer_y`, `label`); egui_plot's powers of ten left narrow axes with one
  label and printed 1.5 as "2". Heatmaps fill their frame (no margin around
  the data), and a plot with nothing to draw says why inside it.
  - **every field explains itself on hover**, over the label and over the value:
    what it changes and, where it matters, the default and the trade-off;
  - built only with `crates/openpid-app/src/panel.rs` (`section`, `row`, `drag`,
    `choice`, `check`, `optional`), whose functions take the hint as a
    parameter, so a field without one does not compile. Test:
    `panel_rows_put_values_in_one_column_and_explain_on_hover`.

## Interface rework ([#33](https://github.com/Rufics/OpenPID/issues/33), PR [#143](https://github.com/Rufics/OpenPID/pull/143))

| Topic | Document |
|---|---|
| R/P/Y stacked, collapsible control panel | [axis-layout](axis-layout/README.md) |
| Top bar, log chips, timeline dock, look | [app-shell](app-shell/README.md) |
| Spectrum: R/P/Y × (full \| below 100 Hz), prefilt, smoothing | [spectrum-tab](spectrum-tab/README.md) |
| Freq × throttle: R/P/Y × (prefilt \| gyro) per log, flown range | [throttle-tab](throttle-tab/README.md) |
| Plot colours as Blackbox Explorer, colour-blind safe option | [plot-colours](plot-colours/README.md) |
| Freq × time: R/P/Y spectrograms in dB, spectrum at the position, throttle | [time-tab](time-tab/README.md) |
| Log viewer: R/P/Y at one scale, throttle / motors, quantities, values at the cursor | [log-tab](log-tab/README.md) |
| Filter Sim: the chain's magnitude, delay and phase per part beside the log | [filter-tab](filter-tab/README.md) |
| Motors: R/P/Y spectra with the harmonics, each motor over the mean beside them | [motors-tab](motors-tab/README.md) |
| One axis at full height on the axis-row tabs (Show, keys R / P / Y) | [axis-focus](axis-focus/README.md) |
| UI tests by control names and data units, on every screen (proof of concept) | [ui-tests](ui-tests/README.md) |
| Log viewer: the sticks and the flight modes at the pointer or the cursor, on the motors row | [log-sticks](log-sticks/README.md) |

