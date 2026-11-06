# An analysis window per log (trim the start and the end)

The owner's remark (2026-11-05): as in PIDtoolbox, every recording needs a
window chosen on the plot that is analysed instead of all N seconds, cutting
the start and the end where there is nothing of interest.

## Cause

The epoch already exists in the model (`Loaded.epoch: Option<Epoch>`, one per
slot) and is driven only from the Log viewer: the "A start = cursor" buttons
after a click on the plot, or dragging the bounds (`draw_epochs`). On the other
tabs the epoch is visible only as two numbers in the header ("0.0 s … 39.596 s")
that cannot be edited. The user does not realise that the analysis runs over the
whole log, take-off and landing included.

## Options

| | Option | Pros | Cons | Score |
|---|---|---|---|---|
| R0 | As is: cursor plus buttons in the Log viewer | works | invisible from the other tabs; two clicks plus a tab switch | 28 |
| R1 | Editable start/end fields in the slot header (as the PTB "start/end sec") | familiar from PTB; cheap | hard to guess the seconds without a picture | 40 |
| R2 | **A timeline strip** under every slot header on every tab: a throttle/gyro thumbnail, the range with handles, the trimmed part visible | always visible; one gesture; as in video editors | 36 px more per tab; needs a thumbnail (decimation exists) | **50** |
| R3 | Range selection by Shift-drag on any plot with a time axis | handy on Freq × time | a hidden gesture; conflicts with pan | 37 |
| R4 | An "Auto" button: trim take-off and landing by throttle and rpm; "Most active 20 s" | zero manual work | needs the strip to show what was trimmed | 44 |
| R5 | Several windows per log (segments) analysed together | flexible | a complex model and UI; a separate topic | 31 |

Recommendation: **R2 + R1 + R4** in one widget; R5 later, if asked for.

## The owner's decision (2026-11-05)

R2 with the numeric fields (R1) and the Auto button (R4).

## Implementation requirements

**The widget `timeline_strip(ui, app, slot)`** under the slot row on every tab
(except Settings), 36 px high, the width of the tab:
- the background is the min/max envelope of the throttle (%) over the whole
  session (decimated to 2 points per pixel; `views::decimate` exists), grey;
  without throttle, |gyro.filtered.roll|;
- on top, the selected epoch range (the slot colour, alpha 60) with two 8 px
  handles (44 pt in touch); dragging a handle moves a bound, dragging inside
  shifts the window, a double-click is the whole log;
- right of the strip: `start` and `end` (DragValue, s, 0.1 s steps), the
  "Auto" and "All" buttons; the label "12.0–32.0 s · 20.0 s" (window length);
- the state is the same `Loaded.epoch`, so the Log viewer handles and the strip
  always agree; an epoch change invalidates the cache through `JobKey` (already so).

**Auto-trim** (`openpid-analysis::epoch::auto(session) -> Option<Epoch>`, a pure
function with tests):
1. "Airborne": throttle ≥ 5 % and (when present) `motor_rpm` ≥ 1000 rpm for ≥ 1 s;
   the first and last such moments, ± 1 s inwards.
2. If less than 5 s remains after trimming, return `None` and show "log too short to trim".
3. "Most active 20 s" (the second entry of the Auto menu): the 20 s window with
   the most |setpoint| energy over roll + pitch; for the step response that gives
   the most windows.

**Project / CLI.** The epoch is already in `ProjectSlot.epoch` and `--epoch A..B`;
add `openpid run … --epoch auto`.

**Default behaviour.** On opening a log the epoch is Auto when Auto gives a
result; otherwise the whole log. The header shows that the epoch is automatic
(an "auto" badge), so the user knows the log was trimmed.

## Acceptance criteria

- Every tab shows every slot's strip; dragging a handle changes the analysis
  results (the number of step windows changes).
- Auto on the 2025.12 fixture cuts the first ~1 s (take-off) and leaves ≥ 30 s;
  on 4.5.1 session 0 (3 s) it says "too short" and the epoch stays whole.
- The start/end fields accept typed values and are clamped to the session length.
- Headless tests: handle drag, Auto, All, agreement with the Log viewer.
- Unit tests of `epoch::auto` on the generator's synthetic data (a throttle ramp with pauses).

## Size

M (≈ 2–3 days): the strip 1, auto-trim ½, integration and tests 1.

## Implementation (2026-11-06, #102)

- `openpid-analysis::epoch::{auto, most_active}`: pure functions with unit tests
  on synthetic data (`SignalSpec` with "ground" at the start and the end) and a
  check on real logs: 2025.12 → 1.43–38.6 s of 39.6 s; 4.2.0 → 1.8–16.0 s;
  4.5.1 session 0 (3 s) → `None`.
- The strip `views::timeline_strip` is called from `slot_header`, so it is on
  every tab with slots; the envelope is computed once per (slot, session, width)
  and cached in `App::strip`. Handles and the window shift go through
  `Sense::click_and_drag` with the hit test at the press origin
  (`pointer.press_origin`), because egui recognises a drag a few pixels later.
- The epoch on opening is `auto`; `Loaded.epoch_auto` gives the "auto" badge,
  which disappears after any manual change. A project (`pending` with a session)
  is not trimmed and keeps its epoch. A session switch applies Auto as well.
- CLI: `--epoch auto | active`; the chosen window is printed to stderr; an error
  with an explanation when there is no window.
- A side fix: the shaded epoch on the Log viewer is now painted over the plot
  instead of being a `Polygon` among the items; a full-height polygon fed the
  auto-bounds and grew the Y axis by the margin (5 %) every frame.

Deviations: "Most active 20 s" is not marked as auto (it is an explicit user
choice); the strip handles are 4 px at rest and 8 px under the pointer or while
dragging, the grab zone is 8 px (22 px in touch), as in the Log viewer.
