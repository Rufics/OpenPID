# UI tests that do not depend on the layout

The owner's remark (2026-11-10, #33, PR #143): a field moving in the right
panel must not mean rewriting tests; run a proof of concept of unified UI
tests that (A) are simpler to write and (B) do not depend on simple shifts
or on the display size.

## Cause

Measured on the 55 headless tests of the old `crates/openpid-app/src/tests.rs`
(ADR-0008, `egui_kittest`):

- **Pointer clicks at a widget's centre.** `node.click()` moves the pointer
  to the middle of the widget's rectangle and presses. A control that is
  scrolled out of view, covered or elsewhere than expected is not clicked;
  the test then fails far from the cause. The Sticks check box at the end
  of the Log viewer's panel needed `scroll_to_me()` and twelve frames.
- **One window size.** Every test ran at 1280 × 800.
- **Frame counting.** 352 calls of `h.step()`, `h.run_steps(n)` and
  `run_until(...)` said how many frames an action needs.
- **Pixels for plots.** 64 places computed positions from frames the tabs
  kept "for tests" (`viewer.plot_frame`, `spectrum.plot_frame`,
  `player.plot_frame`, `heatmap_frames`, `strip.rects`, `surface.frame`),
  each with its own helper.

Two experiments on the unchanged tests:

| Change | Old tests failing |
|---|---|
| window 1024 × 640 instead of 1280 × 800 | 9 |
| every panel's fields 260 px lower | 9 |

Four tests failed in both, so 14 of the 55 depended on the layout or the size.

## The driver

`crates/openpid-app/src/testkit.rs`, over the same harness:

- **Controls by their accessible name, through AccessKit actions**
  (`click_accesskit`, `set_value_accesskit`): `ui.click("All")`,
  `ui.check("Sticks", false)`, `ui.set("Y scale ±", 250.0)`,
  `ui.choose("session", "session 0 (")`, `ui.slide("Master multiplier", 150.0)`,
  `ui.menu("Help")`, `ui.key(Key::P)`. The action reaches the widget
  wherever it is, also scrolled out of view.
- **Plots in data units.** Every plot that takes pointer input registers
  its frame and visible range under a name each frame (`probe.rs`,
  `App::probes`). `ui.plot("log/roll").click(14.2)`, `.hover(20.0)`,
  `.drag(from, to)`, `.grab(..)` / `.release(..)` (to look at the state
  while the button is down), `.double_click(x)`, `.hold(x, frames)`,
  `.pinch_x(100.0, 300.0)`, `.height()`, `.x_range()`, `.x_tolerance()`
  (two screen points in data units) find the pixel themselves; a
  logarithmic frequency axis is still addressed in Hz. One registry
  replaced the per-tab frames kept for tests, which are gone.
- **No frame counting.** Every action ends with `settle()`: frames until no
  log is loading, no analysis is pending and three quiet frames passed.
  `ui.advance(n)` is left for what advances with time (the player).
- **Every screen.** `at_every_size(|screen| ...)` runs a body at
  1280 × 800, 1024 × 640, 1920 × 1080 and in touch mode;
  `OPENPID_UI_SCREEN=1024x640 cargo test -p openpid-app` (or `1920x1080`,
  `touch`) runs every other test on that screen.
- **The pointer only where it is the subject**: `ui.press(label)` for what
  an overlay must swallow, `ui.hover(..)` for tooltips, `ui.rect(..)` for
  the two layout tests.

The names of the plots:

| Name | Plot |
|---|---|
| `log/roll`, `log/pitch`, `log/yaw`, `log/motors`, `log/rpm`, `log/debug`, `log/other` | rows of the Log viewer, seconds |
| `spectrum/<axis>` | the full-range spectrum of an axis, Hz |
| `throttle/<slot>/<axis>`, `.../prefilt` | a Freq × throttle map, throttle % × Hz |
| `time/<axis>` | a spectrogram row, seconds × Hz |
| `strip/<slot>` | the timeline strip of a log, seconds |
| `surface/<slot>` | a 3D surface panel, fractions of its size |

## The rewrite

The proof of concept redid two tests; the owner then asked for all of them
(option U3 below). `crates/openpid-app/src/tests/` holds the 59 tests (four
of them need no window) by subject (`shell`, `slots`, `analysis_window`, `log_viewer`, `step`,
`spectra`, `tools`, `layout`, `worker`), written with the driver.

| | Before | After |
|---|---|---|
| Calls that count frames | 352 | 3 (`advance`, while the player plays) |
| Places that compute a pixel from a frame kept for tests | 64 | 0 |
| Fields of the app kept only so tests can find a plot | 10 | 0 (`App::probes`) |
| Tests failing at 1024 × 640 | 9 | 0 |
| Tests failing with the panel's fields 260 px lower | 9 | 0 |
| Screens the whole suite passes on | 1 | 4 (1280 × 800, 1024 × 640, 1920 × 1080, touch) |

Tests that used to poke the state now use the control where one exists (the
cursor is placed by a click on the plot, a log is hidden with the dock's
eye, the Details table is opened with its button); `app_mut()` stays for
what has no control (an invalid parameter the field would clamp, targets
set from a project).

## Limits

- A control is found by its exact name; of several fields with one name
  (the start and end of every log's window) `set_first` takes the first.
- Pointer input on plots is still a pointer (it has to be: drags, hover);
  it is only computed from the probe instead of written down.
- AccessKit actions do not test that a control can be reached with the
  mouse (covered, off screen): that stays the job of the layout tests in
  `tests/layout.rs`.
- The texts of labels still couple tests to the wording; that is the
  contract the screen reader has too.
- CI runs the suite on the design size only; the other screens are run by
  hand with `OPENPID_UI_SCREEN` (45 s each).

## Options that were considered

| | Option | Pros | Cons |
|---|---|---|---|
| U1 | Keep `tests.rs`, write new tests with the driver | no churn | two styles for good; the 14 fragile tests stay fragile |
| U2 | Move tab by tab as tests are touched | each step small | two styles for a while |
| **U3** | **Rewrite all tests at once** (chosen by the owner) | **one style at once; the test-only fields go in the same change** | **a large diff that changes every test with no product change** |
