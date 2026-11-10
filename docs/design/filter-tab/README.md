# Filter Sim tab

The owner's remark (2026-11-09, #33, PR #143): rework the analysis tabs in the
right-panel layout ([axis-layout](../axis-layout/README.md)), one tab at a
time with the owner's approval; five conceptually different designs per
change; check every screen against the reference tool's tutorial video.

## Cause

Measured at 1280 × 800 on `betaflight-2025.12.2-btfl_027.bbl` before this change:

- **Three wrapped lines of fields above the plots** (axis, segment, max,
  LPF1, LPF2, RPM notch, dynamic notch, static notch: 22 fields), then two
  lines of description and fit; the plots got 60 % of the height.
- **The filters themselves were not on screen**: only their effect on the
  log (spectra and time traces of raw, logged and simulated gyro), and a
  what-if change showed only as a moved spectrum.

The reference video (its Filter Simulation screen, 19:10–20:20) shows the
filters themselves: a column for the low-pass filters and one for the notch
filters, each with magnitude, filter delay (ms) and phase against frequency
and a chirp through the filters, the controls in a column on the right. It
has no log in it; ours runs the log's own gyro through the chain, which the
reference tool does not.

## Options

Five designs on the same data (btfl_027, roll), mocked up in the shell with
the control panel on the right:

![The five designs](variants.jpg)

| | Design | Pros | Cons | Eff | Disc | Conv | Kbd | Touch | Cost | Score |
|---|---|---|---|---|---|---|---|---|---|---|
| S1 | The reference layout: low-pass \| notch × magnitude, delay, phase, a chirp through them | the reference tool's screen | drops the log, the tab's reason to exist (the fit and the what-if on real noise) | 3 | 4 | 5 | 4 | 4 | 3 | 42 |
| **S2** | **The chain (magnitude, delay, phase per part, the total) \| the log (spectrum, time)** | **the filters and their effect on this log side by side; a what-if moves both** | **one axis at a time; dense at 1280 px** | **5** | **4** | **4** | **4** | **3** | **3** | **45** |
| S3 | R/P/Y spectra of raw, logged and sim with the chain beside them | every axis at once | three simulations per change; the chain squeezed into a narrow column | 4 | 3 | 3 | 4 | 3 | 3 | 38 |
| S4 | Low-pass \| notch × magnitude, delay, and each group on the log | separates what the low-passes and the notches do | two more simulations per change; the whole chain never shown | 4 | 4 | 4 | 4 | 3 | 2 | 41 |
| S5 | As before, the controls moved into the panel, the chain drawn on the spectrum | cheapest | the filters still not on screen | 3 | 3 | 2 | 4 | 4 | 5 | 36 |

## Decision (2026-11-10)

**S2**: the reference tool's filter curves next to what only we can show,
the log's own gyro through those filters.

## Implementation requirements

- **Analysis** (`openpid-sim`, `openpid-analysis`): `ChainModel::parts_at`
  gives the chain's parts at an `OperatingPoint` (RPM notches per motor and
  harmonic with their fade and weights, static notches, the low-passes with
  a dynamic one at the cutoff for the throttle, dynamic notches). Filter Sim
  takes the flight's typical moment (mean throttle, mean speed of each
  motor, median centre of each tracked dynamic notch over the epoch) and
  returns `FilterSimOutput::response`: per part and for the whole chain the
  magnitude (dB), the unwrapped phase and the phase delay `−φ / (2π f)` (ms,
  as the reference tool's "filter delay"), the delay at 100 Hz, and the
  chain as flown when there are what-if changes. No mathematics in the app.
- `filter_tab.rs`: `control_panel`; the docked log only. Left: "The filter
  chain", the operating point and the delay at 100 Hz, the parts' names in
  their colours (the legend), three plots sharing the frequency axis
  (magnitude −40…3 dB, delay, phase), a line per part and the total thick
  in the text colour, the chain as flown dashed. Right: "On the log", the fit
  line, the spectrum and the time trace of raw, logged filtered and sim (as
  flown dashed when changed).
- **Panel**: Data (Axis), Reset to log (with changes only), Low-pass 1
  (Type, Cut-off, Dyn min, Dyn max), Low-pass 2 (Type, Cut-off), RPM notch
  (Harmonics, Q, Min), Dynamic notch (Count, Q, Min, Max), Static notch
  (Centre, Cut-off), Model (the chain as described, Residual, Unfiltered,
  Delay, Throttle, Motors), Analysis (Segment, Max frequency). A field shows
  the log's value; an edited one turns the accent colour and becomes an
  override; the hint names the log's value.

## Tests

`parts_at_an_operating_point` (`openpid-sim`: labels, the weighted RPM
harmonics, the dynamic low-pass at half throttle, a notch out of reach left
out); `chain_response_has_delay_and_notch_depth` (`openpid-analysis`: notch
depth, PT1 delay, the notch's lead above its centre, the total's phase);
`filter_sim_tab_validates_the_model_and_applies_what_if_changes` (four
motors, the parts, the legend, the what-if label and the as-flown line,
Reset to log).

## Implementation

btfl_027 (RPM notches on four motors around 166 Hz, both low-passes, the
dynamic notch):

![btfl_027](one-log.jpg)

Betaflight 4.5.1 (two static notches as well):

![4.5.1](second-log.jpg)
