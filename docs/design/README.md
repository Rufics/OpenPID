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

