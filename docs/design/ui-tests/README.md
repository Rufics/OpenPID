# UI tests that do not depend on the layout (proof of concept)

The owner's remark (2026-11-10, #33, PR #143): a field moving in the right
panel must not mean rewriting tests; run a proof of concept of unified UI
tests that (A) are simpler to write and (B) do not depend on simple shifts
or on the display size.

## Cause

Measured on the 55 headless tests of `crates/openpid-app/src/tests.rs`
(ADR-0008, `egui_kittest`):

- **Pointer clicks at a widget's centre.** `node.click()` moves the pointer
  to the middle of the widget's rectangle and presses. A control that is
  scrolled out of view, covered or elsewhere than expected is not clicked;
  the test then fails far from the cause. The Sticks check box at the end
  of the Log viewer's panel needed `scroll_to_me()` and twelve frames.
- **One window size.** Every test runs at 1280 × 800.
- **Frame counting.** 352 calls of `h.step()`, `h.run_steps(n)` and
  `run_until(...)` say how many frames an action needs.
- **Pixels for plots.** 64 places compute positions from frames the tabs
  keep "for tests" (`viewer.plot_frame`, `spectrum.plot_frame`,
  `player.plot_frame`, `heatmap_frames`), each with its own helper.

Two experiments on the unchanged tests:

| Change | Old tests failing |
|---|---|
| window 1024 × 640 instead of 1280 × 800 | 9 |
| every panel's fields 260 px lower | 9 |

Four tests fail in both, so 14 of the 55 depend on the layout or the size.

## The proof of concept

`crates/openpid-app/src/testkit.rs`, a driver over the same harness:

- **Controls by their accessible name, through AccessKit actions**
  (`click_accesskit`, `set_value_accesskit`): `ui.click("all")`,
  `ui.check("Sticks", false)`, `ui.set("Y scale ±", 250.0)`,
  `ui.choose("session", "session 0")`, `ui.key(Key::P)`. The action reaches
  the widget wherever it is, also scrolled out of view.
- **Plots in data units.** Every plot that takes pointer input registers
  its frame and visible range under a name each frame (`probe.rs`,
  `App::probes`: `log/roll`, `log/motors`, ...). `ui.plot("log/roll").click(14.2)`,
  `.hover(20.0)`, `.drag(from, to)`, `.height()`, `.x_range()`,
  `.x_tolerance()` (two screen points in data units) find the pixel
  themselves. One registry replaces the per-tab frames kept for tests.
- **No frame counting.** Every action ends with `settle()`: frames until no
  log is loading, no analysis is pending and three quiet frames passed.
- **Every screen.** `at_every_size(|screen| ...)` runs the body at
  1280 × 800, 1024 × 640, 1920 × 1080 and 1280 × 800 in touch mode.

Two tests of `tests.rs` redone with it (`tests_kit.rs`):

| Test | Before | After | Runs on |
|---|---|---|---|
| Log viewer: rows, quantities, scale, cursor, sticks, modes, one axis | 113 lines, 15 frame / scroll calls | 63 lines, none | 4 screens instead of 1 |
| Epoch bounds dragged on the plot, a drag elsewhere pans | 82 lines, 27 frame / pixel references | 48 lines, none | 4 screens instead of 1 |

Both pass in the two experiments above (the smaller window is one of their
screens; with the fields 260 px lower they pass unchanged).

## Limits

- A control is found by its exact name: two controls with one name on a
  screen need a scope (`ui.within("Low-pass 1").set("Cut-off", ...)`), not
  written yet.
- Pointer input on plots is still a pointer (it has to be: drags, hover);
  it is only computed from the probe instead of written down.
- AccessKit actions do not test that a control can be reached with the
  mouse (covered, off screen): that stays the job of a few layout tests,
  as `panel_rows_put_values_in_one_column_and_explain_on_hover`.
- The texts of labels still couple tests to the wording; that is the
  contract the screen reader has too.

## Options for the rest

| | Option | Pros | Cons |
|---|---|---|---|
| U1 | Keep `tests.rs`, write new tests with the driver | no churn | two styles for good; the 14 fragile tests stay fragile |
| **U2** | **Move tab by tab: when a tab's tests are touched, redo them with the driver and register its plots in `probes`; delete the per-tab frames kept for tests as they lose their readers** | **the fragile tests go first; each step is small and reviewable** | **two styles for a while** |
| U3 | Rewrite all 55 tests at once | one style at once | a large diff that changes every test with no product change |

Recommended: **U2**, starting with the 14 tests the two experiments broke
(four of them broke in both).
