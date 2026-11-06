# First run: a start screen and a tour of the interface

The owner's remark (2026-11-05): on the first start the application should
walk the user through visually and show how to use it, as Adobe and Microsoft
products do.

## Cause

The empty state is a bare plot with axes −0.5…0.5 s and a status-bar hint with
a broken arrow glyph. Twelve tabs with no explanation of where to start. A user
who does not know PIDtoolbox does not see the loop "log → noise → filters → PID
→ apply".

## What the sources say

- **Apple HIG / Material 3 (onboarding):** teach in context, in short steps,
  with a way to skip; never block the work with modal windows.
- **NN/g (coach marks):** hints attached to an element work better than a
  carousel of screens that people skip; 5–7 steps is the attention ceiling.
- **Adobe/MS:** the tour runs once and stays available from the Help menu.

## Options

| | Option | Pros | Cons | Score |
|---|---|---|---|---|
| R0 | As is | — | an empty plot, no hint at all | 18 |
| R1 | A modal welcome dialog with a carousel of 5 screens | simple to build | not tied to the UI; closed without reading | 33 |
| R2 | **Coach marks**: a dimmed screen with a window on the element plus a card with text, 7 steps over the real interface | teaches in context; the Adobe/Figma/GitHub standard | needs the widget rectangles and an overlay over egui | **48** |
| R3 | A persistent "First steps" checklist in a side panel, ticked automatically | keeps the progress, does not disturb | takes space; does not show *where* the button is | 41 |
| R4 | Tooltip bubbles on every tab at the first visit | in small portions | 12 separate hints annoy; no picture of the whole loop | 35 |
| R5 | An interactive tour on a built-in synthetic log (`openpid gen`): the tour opens the log itself and walks the tabs | the user sees real data | passive without R2 | 44 |

Recommendation: **R2 + parts of R5** (the tour may open a synthetic log to
show the steps on real plots) and a **start screen** instead of the empty plot.

## The owner's decision (2026-11-05)

R2 with the synthetic log of R5; the start screen is part of the same topic.

## Implementation requirements

**The start screen** (when no slot is loaded, on any tab): a centred 520 px
card: the logo name, "Drop a log here, or", the buttons "Open log…", "Open
sample log" (generates a `SignalSpec::default()` session with seed 1 in memory,
no file), "Start the tour", the "Recent files" list (up to 8, in `Prefs`, path
plus date), a "User guide" link. Instead of the plot with axes.

**The tour (coach marks).** The state `Tour { step: usize, active: bool }` in
`App`; `Prefs.tour_done: bool`: the tour starts by itself on the first start
after the first log (or the sample log) is opened and never again; "Help → Show
tour" starts it again. The steps (UA/EN text through `tr`):

| # | Anchor (`anchor id`) | Text |
|---|---|---|
| 1 | slot bar | "Your logs live here. Drop another one to compare" |
| 2 | the Log viewer tab plus the epoch | "Choose the analysis window: drag the bounds or press Auto" |
| 3 | the Spectrum tab | "Noise: peaks above 80 Hz are notch candidates" |
| 4 | the Freq × throttle tab | "Noise over throttle and the motor harmonics (RPM overlay)" |
| 5 | the Filter Sim tab | "Try new filters without flying" |
| 6 | the Step response tab | "Stick response: overshoot and delay" |
| 7 | the Advice tab | "Advice with evidence and ready CLI commands. Copy and apply" |

Mechanics: every step switches to its tab; the anchor widgets register their
`Rect` every frame (`app.anchors.insert("tab-spectrum", response.rect)`); over
the UI an `egui::Area` with `Order::Foreground`: a translucent dimming
(`Color32::from_black_alpha(140)`) with a cut-out (the anchor rectangle plus
6 px, radius 6), a 320 px card next to the anchor (automatically left / right /
below so it stays inside the window) with the title "Step 3 of 7", the text and
the buttons "Back", "Next", "Skip". Keys: → / Enter next, ← back, Esc skip.
Touch: 44 pt buttons. During the tour the rest of the UI takes no clicks (the
`Area` intercepts them), except the card's buttons.

**Anchor ids** (constants in `tour.rs`): `slot-bar`, `tab-log`,
`epoch-handles`, `tab-spectrum`, `tab-throttle`, `tab-filter`, `tab-step`,
`tab-advice`, `advice-cli`.

**Accessibility.** The tour card is an `egui::Window`-like container with the
accessible name "Tour step N of 7"; every button has a name.

**Settings.** Settings → "Show tour on first start" (a checkbox), "Reset
first-run state".

## Acceptance criteria

- A clean `prefs.json` plus opening a log → the tour starts; "Skip" records
  `tour_done`; the next start has no tour. Help → Show tour starts it again.
- All 7 steps show the cut-out on the right widget (test: the cut-out contains
  `anchors[id]`), switch the tabs, the keys work.
- The start screen without a log: the buttons work, "Open sample log" creates a
  slot with a synthetic session (Advice on it gives an overshoot proposal with
  `damping 0.4`).
- Headless tests: the tour from start to end, Skip, a repeated run, the start screen.
- The screenshots `start.png` and `tour-3.png` in the user guide.

## Size

M (≈ 3 days): the start screen ½, the coach-mark mechanics 1½, texts / tests / docs 1.

## Implementation (2026-11-06, #104)

- `crates/openpid-app/src/tour.rs`: `STEPS` (7 steps: tab, anchor, fallback
  anchor, text), `Tour { active, step, cutout }`, `App::{start_tour, end_tour,
  tour_keys, tour_tab, tour_overlay}`. Anchors are registered every frame in
  `App::anchors` (`slot-bar` in `slot_headers`, `epoch-handles` the strip of
  the first slot, `tab-*` the tab buttons, `advice-cli` the CLI field on
  Advice); the overlay is an `egui::Area` in `Order::Foreground` with a
  full-screen widget that swallows clicks, four dimmed rectangles around the
  cut-out, a 320 px card below or above the anchor.
- The start screen `views::start_screen` replaces the tab when every slot is
  empty (`App::start_screen_wanted`); the sample log is `App::open_sample`
  (`SignalSpec` 30 s, `damping 0.4`, Betaflight-like PIDs, no file);
  `Prefs::{tour_done, show_tour, recent}` (up to 8, the date `YYYY-MM-DD`
  without dependencies, `prefs::ymd`); the tour starts from `maybe_start_tour`
  after the first successful load (`App::auto_tour` switches it off in tests).
- Help → Show tour; Settings: the checkbox "Show the tour on the first opened
  log", "Reset first-run state", "Show tour". The env `OPENPID_TOUR_STEP=n`
  opens the application directly on step n (screenshots / demos).
- Tests: a walk through all 7 steps checking that the cut-out contains the
  anchor and the tab switched; Done/Skip/Esc/Back; once-only through
  `prefs.json`; clicks under the overlay do not pass; Advice on the sample log
  proposes D; recent files open from the start screen.

Deviations: steps 2 and 7 have fallback anchors (the tab button) when the strip
or the CLI field is off screen; a wide widget (the CLI field) is cut at the
window edge; `advice-cli` instead of a separate step for `tab-advice`, one card
on Advice.
