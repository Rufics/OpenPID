# Calibration inventory: every number OpenPID computes and shows

Part of #167 (epic #166).

The epic finds out how the reference tool computes what it shows, by feeding
it synthetic probe logs, and makes OpenPID's mathematics agree where the owner
decides so. This file is the starting point: one row for every number, curve
or map a user can read in OpenPID, with how it is computed today, what fixes
its value, and what the repository records about the reference tool.

Companion files: [DECISIONS.md](DECISIONS.md) (the register of
disagreements) and [ROUNDS.md](ROUNDS.md) (the ledger of probe rounds).

## How to read it

- **One row per quantity.** The bold id in the first column (`S1`, `R8`, …)
  is how the other calibration documents refer to a row.
- **How OpenPID computes it** is written from the code, not from the design
  documents. Where the code and `docs/DESIGN.md` disagree, the code is
  described here and the disagreement is listed in
  [Open points found while writing](#open-points-found-while-writing).
- **Code** gives `path:line` at commit `0a61b63` (the parent of the commit
  that added this file). The paths are shortened:
  `analysis/` = `crates/openpid-analysis/src/`,
  `dsp/` = `crates/openpid-dsp/src/`,
  `sim/` = `crates/openpid-sim/src/`,
  `core/` = `crates/openpid-core/src/`,
  `bbx/` = `crates/openpid-fmt-blackbox/src/`,
  `app/` = `crates/openpid-app/src/`,
  `cli/` = `crates/openpid-cli/src/`.
- **Pinned by** names the tests that fix the value today. Unit tests are in
  the `#[cfg(test)]` module of the file in the Code column; the others are in
  `crates/*/tests/*.rs` and `crates/openpid-app/src/tests/*.rs`. A test named
  with a remark in brackets fixes only that part of the row.
- **Reference tool** holds only what this repository already records:
  `docs/DESIGN.md` §6.2 and §6.7, the `docs/design/*/README.md` notes and
  `docs/USER-GUIDE.md`. No code of the reference tool was read and nothing
  was looked up elsewhere. "not recorded — round 0" means the repository said
  nothing when the row was written; what round 0 then showed of that screen
  is in [REFERENCE-SCREENS.md](REFERENCE-SCREENS.md).
- **Status** is one of:
  - `matched on evidence`: the repository holds a comparison with the
    reference tool (its numbers in a test, or a comparison recorded in
    `docs/DESIGN.md`) and the values agree within the tolerance the row
    states;
  - `differs`: the repository records that the two tools do not show the
    same; the row says how;
  - `never compared`: no value of the reference tool is recorded for this
    quantity, whether or not the reference tool has it;
  - `OpenPID only`: the repository records that the reference tool has no
    such quantity or screen.
- **Probe idea** is the synthetic signal that would settle how the reference
  tool computes the quantity; "—" where the quantity is OpenPID only.

Three different decibel scales are in use; the rows say which:

| Where | Value | 0 dB is |
|---|---|---|
| Spectrum tab, Filter Sim spectra, `psd`, `motor-noise` | `10·log10` of a power density | 1 unit²/Hz (1 (deg/s)²/Hz for a gyro) |
| Freq × time | `20·log10` of an amplitude | an amplitude of 1 unit (1 deg/s for a gyro) |
| Filter Sim chain | `20·log10` of a gain | a gain of 1 |

The mapping from raw log fields to channels is described for the
Betaflight family (`bbx/profiles/`); the ArduPilot profile
(`crates/openpid-fmt-ardupilot/src/profile.rs`) and the CSV reader
(`crates/openpid-fmt-csv/src/lib.rs`) produce the same canonical channels by
their own rules, which are not listed here.

## 0. Common to every area

What every analysis does before its own mathematics, and what the dock at
the bottom of the window shows on every tab.

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **C1** Sample rate `fs` (Hz) of every analysis | `1e6 / dt`; `dt` is the configured log step when the headers give one, else the median observed step (note C-1) | `Timebase::nominal_dt_us`, `Timebase::median_dt_us`; no user parameter | `analysis/prepare.rs:79`, `:127`; `bbx/session.rs:170`; `bbx/profiles/common.rs:361`, `:413` | `frame_rate_from_intervals` (the header ratio only); no test with a nominal step that differs from the median | not recorded — round 0 | never compared | a tone of known frequency in a log whose header rate is 2 % off its time stamps: where the peak sits |
| **C2** Samples on a uniform grid, gaps filled | linear interpolation of (time, value) onto a grid that starts at the first sample of the window with step `dt`; a grid point between two samples more than `1.5·dt` apart, or beside an invalid sample, is invalid; every invalid run is then replaced by a straight line between its neighbours (held flat at the ends) | gap limit `1.5·dt` (constant) | `analysis/prepare.rs:96` (`:122`), `:35`; `dsp/resample.rs:46` | `resample_matches_reference`, `interpolates_and_flags_gaps`, `resample_edges`, `fills_gaps` | not recorded — round 0 | never compared | a tone with 5 % of the frames dropped: the noise floor with and without the drops |
| **C3** Analysis window, start – end (s) | the samples with `first + start ≤ t ≤ first + end`; no window = the whole session. The GUI opens a log on C4, the CLI on the whole log unless `--epoch` is given | `Epoch { start_s, end_s }` | `analysis/prepare.rs:66`; `app/app.rs:147`, `:882`; `cli/main.rs:514`; text `app/views.rs:1263` | `a_log_opens_on_its_automatic_trim_and_all_restores_the_whole_log`, `cursor_buttons_set_the_epoch`, `strip_handles_fields_and_the_log_viewer_share_the_epoch` | "start/end sec" fields (docs/design/analysis-window); the trimmed-off parts are greyed in its log viewer (docs/design/log-tab) | never compared | a marker tone only in second 3–4 with start = 4 s: gone or not |
| **C4** Automatic trim "Auto" (s) | from the first to the last airborne sample (throttle ≥ 5 % and, when motor rpm is logged, the fastest motor ≥ 1000 rpm) of the airborne runs that last ≥ 1 s, then 1 s cut inside each end; nothing when the result is shorter than 5 s | `AIR_THROTTLE_PCT 5`, `AIR_RPM 1000`, `AIR_MIN_RUN_S 1`, `AIR_MARGIN_S 1`, `MIN_WINDOW_S 5` (constants) | `analysis/epoch.rs:42` (constants `:9`-`:17`) | `auto_trims_the_ground_time_one_second_inside`, `auto_ignores_a_blip_and_refuses_a_short_flight`, `rpm_vetoes_the_throttle`, `auto_epoch_of_the_real_logs` | leaves out the first and the last second of the log (DESIGN §6.2, §6.7) | differs: OpenPID trims by throttle and rpm, the reference tool by a fixed second at each end | a marker tone in the first and in the last second, and 5 s of idle throttle after arming |
| **C5** "Most active 20 s" window (s) | the 20 s window with the largest sum over its samples of `abs(setpoint roll) + abs(setpoint pitch)`; one candidate start per sample | `ACTIVE_WINDOW_S 20` | `analysis/epoch.rs:85`; `app/views.rs:1240` | `most_active_finds_the_stick_moves` | not recorded — round 0 | never compared | whether it has such a control at all (round 0) |
| **C6** Dock: length (s), rate (Hz), timeline envelope | length = last − first time stamp; rate = `1e6 / median step` (not C1); envelope = per pixel column the minimum and maximum of throttle / 100, or without a throttle of `abs(gyro roll)` over its maximum | — | `core/session.rs:250`, `:259`; `app/views.rs:637`-`:638`, `:1004` | `timebase_gaps`; the envelope has no test | not recorded — round 0 | never compared | the length and rate it prints for a 30.000 s log at 1000 Hz |

Notes

- **C-1.** For the Betaflight family the configured step comes from three
  headers: gyro rate = `1e6 / looptime`, PID rate = gyro rate /
  `pid_process_denom`, log rate = PID rate / frames ratio, where the frames
  ratio is `denominator / numerator` of `P interval` written as a fraction,
  the number itself when it is a whole number, and `I interval` (32 when
  absent) when it is zero or negative. A CSV log, an ArduPilot log and a
  Blackbox log without `looptime` have no configured step and use the
  median.

## 1. Log viewer

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **L1** Gyro (deg/s) | `gyroADC[axis] × gyro_scale / hires`; `gyro_scale` is the header of that name read as IEEE-754 bits (1 when absent or not above 1e-9), `hires` is 10 when `blackbox_high_resolution = 1`, else 1 | header-driven; display unit `Prefs::rate_unit` (deg/s, or rad/s = × π/180) | `bbx/profiles/common.rs:48`, `:119`, `:161`; drawn `app/log_tab.rs:509`-`:523`, `app/views.rs:1356`, `app/prefs.rs:51` | `golden_summaries`, `gyro_scale_header`, `main_frames_match_reference_decoder`, `raw_decode_matches_blackbox_explorer`, `log_viewer_uses_display_units` | Gyro on roll, pitch, yaw rows at ± 500 °/s (DESIGN §6.7); the values at the cursor (docs/design/log-tab) | never compared | a constant gyro of 100, 200 and 300 raw units on roll, pitch, yaw: the values at the cursor |
| **L2** Gyro prefilt (deg/s) | `gyroUnfilt` or `gyroRAW` with the scale of L1; without them `debug[0..2]` taken as deg/s when Betaflight 4.0–4.5 flew `debug_mode = GYRO_SCALED` (code 6) | — | `bbx/profiles/common.rs:169`, `:563` | `debug_gyro_scaled_becomes_gyro_raw`, `golden_summaries` | "Gyro prefilt" in its checklist (docs/design/log-tab) | never compared | a log with `debug_mode GYRO_SCALED` and no `gyroUnfilt`: does it draw a prefilt line, and at what scale |
| **L3** Setpoint (deg/s) | `setpoint[0..2] / hires`; INAV: `axisRate` as logged | — | `bbx/profiles/common.rs:177`; `bbx/profiles/inav.rs:173` | `golden_summaries`, `inav_9_fixed_wing` | "Set point", on by default (DESIGN §6.7); drawn under the other lines (the comment at `app/log_tab.rs:273`) | never compared | a setpoint staircase of 100, 200, 400 raw units |
| **L4** P, I, D, F term and PID sum (firmware units) | `axisP/I/D/F` as logged (unit "raw"); sum = P + I + D + F (+ S) sample by sample when at least two terms are logged; drawn on the axis rows, whose axis is labelled in the rate unit | — | `bbx/profiles/common.rs:187`, `:733`-`:757`; `app/log_tab.rs:268`-`:288` | `golden_summaries` | P, I, D prefilt, D, F and PID sum in its checklist (docs/design/log-tab); their units are not recorded | never compared | constant P = 100, I = 20, D = −30, F = 5 raw units: the reading and the unit of each line and of the sum |
| **L5** PID error (deg/s) | setpoint − filtered gyro, sample by sample, when both carry the same unit | — | `bbx/profiles/common.rs:713`-`:732` | `golden_summaries` | "PID error" in its checklist; the formula is not recorded | never compared | setpoint 100, gyro 60, both constant: 40 or −40 |
| **L6** Motors (%) | `(motor[i] − low) × 100 / (high − low)`; `low` and `high` from the `motorOutput` header, else `minthrottle` and `maxthrottle` (1150 and 1850 when absent); not limited to 0–100; raw when `high ≤ low`. Rotorflight: `motor[i] × 0.1` | header-driven | `bbx/profiles/common.rs:128`, `:140`; `bbx/profiles/rotorflight.rs:180` | `golden_summaries`, `rotorflight_4_3_decodes_cleanly_with_helicopter_units` | Motors 1–4 with the throttle in a row of their own (DESIGN §6.7); the scale is not recorded | never compared | the motors held at `low`, at mid-range and at `high`: 0, 50 and 100 % or not |
| **L7** Throttle (%) | `setpoint[3] / 10` (the mixer throttle × 1000) when the log has it, not limited; else `(rcCommand[3] − 1000) / 10` limited to 0–100, when `rcCommand[3]` exceeds 900 somewhere in the log. Rotorflight: `rcCommand[4] / 10`, limited | — | `bbx/profiles/common.rs:184`, `:688`-`:712`; `bbx/profiles/rotorflight.rs:227` | `golden_summaries` | Throttle in the motors row (DESIGN §6.7); which field it reads is not recorded | never compared | `rcCommand[3]` at 1500 with `setpoint[3]` at 300: 50 % or 30 % |
| **L8** Motor RPM (rpm) | `eRPM[i] × 100 × 2 / motor_poles`; no channel when `motor_poles` is not logged | — | `bbx/profiles/common.rs:761`-`:778` | `golden_summaries`, `rpm_overlay_follows_the_motor_ridge` | RPM 1–4 in its checklist (docs/design/log-tab); the scale is not recorded | never compared | `eRPM` = 100 with 14 poles: 1429 rpm or not |
| **L9** Debug (raw) | `debug[i]` as logged | — | `bbx/profiles/common.rs:208` | `golden_summaries` | "Debug" and "Test signal" in its checklist (docs/design/log-tab) | never compared | `debug[0]` = 123 |
| **L10** Values at the cursor | per line, the value of the first sample at or after the cursor time (not the nearest one), × the display scale of L1, printed without decimals | — | `app/log_tab.rs:813` (`:825`, `:839`) | `log_viewer_rows_quantities_cursor_and_sticks` | the values at the cursor (docs/design/log-tab); the motor outputs and the gyro at the cursor under its stick boxes (docs/design/log-sticks) | never compared | a ramp of one unit per sample: which sample is read and how many decimals are shown |
| **L11** The lines as drawn | when the visible span holds more samples than twice the plot width in pixels: per bucket of samples the minimum and the maximum, in time order; invalid samples left out | 2 points per pixel | `app/views.rs:717`; `app/log_tab.rs:513` | `decimation_keeps_extremes` | not recorded — round 0 | never compared | a one-sample spike of 1000 in 60 s of zeros: visible at full zoom-out or not |
| **L12** Sticks card (−1…1, throttle 0…1) | roll and pitch = `rcCommand / 500`, yaw = `−rcCommand[2] / 500`, throttle = `(rcCommand[3] − 1000) / 1000` (when `rcCommand[3]` exceeds 900 in the log), each limited to its range; read from the last valid sample at or before the sample of L10; the dot sits at value × half the box | `RC_FULL 500` | `bbx/profiles/common.rs:614`; `core/session.rs:587`; `app/log_tab.rs:718` | `sticks_are_mapped_per_firmware`, `modes_and_sticks_are_read_by_time`, `log_viewer_rows_quantities_cursor_and_sticks` | two boxes, "T / Y" and "P / R", beside its plots (docs/design/log-sticks); the scaling and the sign of yaw are not recorded | never compared | full right yaw and half right roll: where the dots are |
| **L13** Flight modes at the cursor; mode-change marks (s) | the modes of the last change at or before the sample of L10; a mark at the time of every change, labelled "t.t s · modes" | `LogDisplay { modes true, mode_marks false }` | `core/session.rs:569`, `:578`; `app/log_tab.rs:635` | `flight_modes_are_decoded_per_firmware`, `changes_are_looked_up_by_sample`, `mode_changes_of_a_log_without_modes` | a dashed line through the panels where the modes change, the names under the throttle row (DESIGN §6.7) | never compared | arming at 2.000 s and angle mode at 5.000 s |
| **L14** Time axis (s) and Y scale | time = (time stamp − first time stamp) / 1e6; the axis rows span ± `y_scale`, the motors row 0–100 %, the other rows follow their data | `LogDisplay::y_scale 500` (deg/s) | `app/log_tab.rs:387`, `:288`, `:323` | `log_viewer_rows_quantities_cursor_and_sticks` (± 500, ± 250) | ± 500 by default, a "y scale" field (DESIGN §6.7; docs/design/log-tab) | matched on evidence (the default only) | an event 10.000 s after the first frame, with 3 s logged before arming: where its time zero is |

## 2. Spectrum

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **S1** Spectrum, full range: power density of the chosen quantity per axis (dB re 1 unit²/Hz, or linear) | Welch (note S-1): segments of `nperseg` samples, each minus its own mean, × a periodic Hann window, FFT; the mean of `abs(X)²` over the segments × `1 / (fs · Σw²)`, × 2 for every bin except 0 Hz and Nyquist | `PsdParams { segment_s 0.25, overlap 0.5, window Hann, max_hz None }` | `analysis/psd.rs:104` (`:111`, `:122`); `dsp/spectrum.rs:46`; `dsp/window.rs:25` | `welch_matches_scipy`, `sine_amplitude_and_psd_power`, `windows_match_scipy`, `psd_finds_the_injected_tone` (the frequency only), `spectra_of_a_real_flight` (the shape only) | roll, pitch and yaw rows; window Hann; "its own dB scale", whose absolute levels differ from OpenPID's (DESIGN §6.7; docs/design/axis-layout) | differs: the dB levels are not the same; by how much and why is not recorded | a sine of 10 deg/s at an exact bin plus white noise of known density: the dB at the peak and of the floor give its reference, its window gain and whether it shows density or amplitude |
| **S2** Spectrum below 100 Hz | S1 with `segment_s = max(segment_s, 1 s)` and `max_hz = 100`: 1 Hz bins | `LOW_SEGMENT_S 1.0`, `LOW_MAX_HZ 100` | `app/spectrum_tab.rs:328`-`:333` | `spectrum_controls_change_what_is_drawn` (the number of jobs) | a "Sub 100Hz" panel per axis (DESIGN §6.7); its segment length is not recorded | never compared | two sines at 40 and 42 Hz: resolved in its sub-100 Hz panel or not |
| **S3** Smoothing of the curve | in the app, on the linear power: a centred moving average over `2·h + 1` bins, `h = floor((width / df) / 2)`, `df` the bin spacing; the window is cut, not mirrored, at the ends of the axis; the dB conversion comes after (note S-2) | `SMOOTHING`: off 0, low 2, low-med 4, medium 8, med-high 16, high 32 Hz; default medium (`SpectrumDisplay::smoothing 3`) | `app/spectrum_tab.rs:37`, `:167`, `:398` | `smoothing_averages_over_the_width` | smoothing, "medium" by default (DESIGN §6.7; docs/design/axis-layout); what its levels do is not recorded | never compared (the name of the default is the same, its effect is unknown) | a single-bin tone on a flat floor at each of its smoothing levels: the width and height of the peak, and whether it averages dB or power |
| **S4** Value in dB; the power axis | `10·log10` of the smoothed power (floor: the smallest positive `f64`); linear: the power itself. Axis: the fixed "Y min" / "Y max", else the lowest and highest drawn point at ≥ 2 Hz of all six plots, − 3 and + 3 dB, rounded outwards to 10 dB (linear: 0 to 1.05 × the maximum) | `Tools::psd_db true`; `SpectrumDisplay { y_min None, y_max None }`; `LOG_MIN_HZ 2` | `app/spectrum_tab.rs:398`, `:414` | `spectrum_controls_change_what_is_drawn` (dB on, Y min back to automatic); no test of the range values | "its own dB scale" (DESIGN §6.7) | differs (see S1) | white noise of 1 (deg/s)²/Hz: the level it reads |
| **S5** Automatic peaks (Hz labels), "top peak" | on the unsmoothed `10·log10(power)` of the solid full-range curve: local maxima (`y[i] > y[i−1]` and `y[i] ≥ y[i+1]`) with a topographic prominence of at least the threshold, at least `2·df` apart, most prominent first; the first `n + 8` are taken, those below 20 Hz dropped, the first `n` kept. The label is the bin frequency to 0.1 Hz, placed at the height of the smoothed curve | `SpectrumState { auto_peaks 3, min_prominence_db 6.0 }`; `PEAK_MIN_HZ 20` | `app/spectrum_tab.rs:347`, `:600`; `analysis/psd.rs:75`; `dsp/peaks.rs:52` (`:22`) | `two_tones_over_a_noise_floor`, `shoulder_peaks_and_distance`, `psd_finds_the_injected_tone`, `spectrum_controls_change_what_is_drawn` | not recorded — round 0 | never compared | two tones, 12 dB and 4 dB above the floor: which one gets a label and at what frequency |
| **S6** Manual markers (Hz), Δ, pointer read-out | a click snapped to the nearest bin of the first curve of the first row; Δ = the distance between the last two markers; the pointer shows frequency and the value of the nearest drawn point with one decimal | — | `app/spectrum_tab.rs:303`, `:369`, `:540`, `:770` | `spectrum_markers_are_placed_by_clicks` | not recorded — round 0 | never compared | whether its cursor reads a bin or interpolates (round 0) |
| **S7** Prefilt curve (dotted) | S1–S4 on the unfiltered gyro (L2) | `SpectrumDisplay::prefilt true` | `app/spectrum_tab.rs:110`, `:317` | `spectrum_controls_change_what_is_drawn` | gyro solid, gyro prefilt dotted (DESIGN §6.7) | matched on evidence (which curves are drawn, not their levels) | a tone present only in `gyroUnfilt`: only the dotted curve shows it |
| **S8** D-term, P-term, PID sum, PID error, setpoint | S1–S4 on the channels of L3–L5 (raw² per Hz for the PID terms) | `Quantity::ALL`, default Gyro | `app/spectrum_tab.rs:86` | `quantities_map_to_channels_and_back` | not recorded — round 0 (the channel list of its spectrum screen) | never compared | a tone present only in `axisD` |

Notes

- **S-1.** `nperseg` is `round(segment_s · fs)`, at least 16, raised to the
  next number whose prime factors are 2, 3 and 5 (250 at 1 kHz, 400 at
  1562.5 Hz, 2000 at 8 kHz). Consecutive segments start
  `nperseg − floor(overlap · nperseg)` samples apart; a last partial segment
  is dropped; the bins are at `k · fs / nperseg`, kept up to `max_hz`. The
  scaling is that of `scipy.signal.welch(…, "hann", detrend="constant",
  scaling="density", average="mean")`: a sine of amplitude `A` at a bin
  centre reads `A² / (3 · df)` at its bin, white noise reads its density.
- **S-2.** With `h = floor((width / df) / 2)` the averaged span is
  `(2h + 1) · df`, not `width`. At the default 0.25 s segment (`df` = 4 Hz)
  the levels give h = 0, 0, 0, 1, 2, 4: "low" and "low-med" leave the curve
  unchanged, "medium" averages 3 bins (12 Hz). In the column below 100 Hz
  (`df` = 1 Hz) the same levels give h = 0, 1, 2, 4, 8, 16: "medium" averages
  9 bins (9 Hz).

## 3. Freq × throttle

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **T1** Map: mean amplitude per throttle bin and frequency (the channel's unit; drawn without a unit) | segments, an amplitude spectrum and a mean throttle per segment, the mean of the spectra within the throttle window of each of the 101 bins, a box filter (note T-1) | `ThrottleHeatmapParams { segment_s 0.2, half_window 6.0, window Hann, max_hz 1000, smooth_throttle 8, smooth_freq_frac 0.01 }` | `analysis/throttle.rs:137` (`:152`, `:175`, `:194`, `:252`); `dsp/spectrum.rs:104`; `dsp/smooth.rs:27` | `throttle_heatmap_ridge_follows_the_motor_speed` (where the ridge is), `rfft_matches_numpy` (the amplitude spectrum), `smoothing_matches_reference` (the box filter), `spectra_of_a_real_flight` (the shape); no test of a cell value | gyro prefilt beside gyro per axis (its preset 1), throttle 0–100 % (DESIGN §6.7; docs/design/throttle-tab); how the map is computed is not recorded | differs until #195 (D-005: as the reference tool; rounds 8 and 9, [throttle.md](throttle.md)) | a throttle staircase in 10 % steps, every step with its own tone frequency and amplitude: which columns show each tone (window, smoothing) and what its cells read (normalisation) |
| **T2** Segments per bin; dimmed bins; the summary line | `counts[t]` = the number of segments with `abs(throttle − t) ≤ half_window`, before the smoothing; a bin with fewer than `min_segments` is dimmed; "n of 101 throttle bins have fewer than m segments" | `Tools::min_segments 3` | `analysis/throttle.rs:196`-`:207`; `app/throttle_tab.rs:143`; `app/views.rs:1804` | `throttle_heatmap_reuses_its_textures_and_names_thin_bins` | not recorded — round 0 | differs until #195 (D-005: as the reference tool; rounds 8 and 9, [throttle.md](throttle.md)) | a flight between 20 and 40 % throttle only: what it draws at 80 % |
| **T3** Colour scale: the maximum and the bar | automatic maximum = the 99th percentile of the positive cells of the rows at ≥ 20 Hz (the sorted cell at index `floor((n − 1) · 0.99)`); per log the larger of its maps; with "Link scales" the largest of the logs; a fixed "Colour max" wins. Colour = `value / maximum` through the colour map, ≤ 0 transparent, above 1 the top colour; the picture is interpolated between cells. Bar: five ticks from 0 to the maximum | `Tools { heatmap_max 0 (automatic), heatmap_link true }`; `SCALE_MIN_HZ 20` | `analysis/render.rs:82`, `:103`, `:199`; `app/throttle_tab.rs:203`; `app/views.rs:1430`, `:1531` | `palette_and_grid_rendering`, `colour_maximum_and_value_lookup`, `heatmaps_show_a_colour_scale_with_linked_maxima_and_a_hover_value` | colour map MATLAB "hot"; its colour limit is not recorded (DESIGN §6.7) | differs in the colour map (the owner's choice); the limit differs until #195 (D-005; [throttle.md](throttle.md)) | one tone of known amplitude, raised step by step: the amplitude at which its map saturates |
| **T4** Value under the pointer | the cell nearest to the pointer in throttle and in frequency: "t %, f Hz: z" with three decimals, no unit | — | `analysis/render.rs:215`; `app/views.rs:1503`, `:1514` | `colour_maximum_and_value_lookup`, `heatmaps_show_a_colour_scale_with_linked_maxima_and_a_hover_value` | not recorded — round 0 | differs until #195 (D-005: as the reference tool; rounds 8 and 9, [throttle.md](throttle.md)) | whether it shows a value under the pointer at all (round 0) |
| **T5** RPM overlay lines (Hz against throttle) | per motor and throttle bin: the median (for an even count the upper of the two middle values) of the segment means of the motor rpm (L8; a segment counts when at least half of its rpm samples are valid) over the segments of T2's window; a line at `k · rpm / 60`. Drawn by default as one line per harmonic at the mean of the motors; the number of harmonics is the RPM filter's (3 when the log has none) unless set | `OverlayState { show false, per_motor false, harmonics 0, notches false }` | `analysis/throttle.rs:181`-`:184`, `:214`-`:238`; `app/views.rs:1632`, `:1685`, `:1702` | `rpm_overlay_follows_the_motor_ridge`, `throttle_heatmap_ridge_follows_the_motor_speed`, `rpm_overlay_on_the_throttle_heatmap_has_per_motor_toggles` | draws no line from the data on this screen, only fixed 20 and 60 Hz guides in its "<100Hz" view (docs/design/throttle-tab) | OpenPID only | — |
| **T6** Notch bands and limits (Hz) | static notch: a band of centre ± `abs(centre − cutoff)`; dynamic notch: its minimum–maximum band; the RPM filter minimum as a line | from `Settings::gyro_filters` | `analysis/throttle.rs:239`-`:249`; `app/views.rs:1750` | `rpm_overlay_follows_the_motor_ridge` | none on this screen (as T5) | OpenPID only | — |
| **T7** Throttle axis | 0–100 %; with "Zoom to the flown range" from the first to the last bin with at least `min_segments` segments in any map, to the edges of those bins | `ThrottleDisplay::zoom_flown false` | `app/throttle_tab.rs:227` | `throttle_tab_stacks_the_axes_with_prefilt_and_zooms_to_the_flown_range` | 0–100 % (DESIGN §6.7) | matched on evidence (the default axis) | a tone at 0–5 % throttle only: is the edge of its axis 0 % |
| **T8** Prefilt map | T1 on the unfiltered gyro, on the colour scale of T3 | `ThrottleDisplay::prefilt true` | `app/throttle_tab.rs:166` | `throttle_tab_stacks_the_axes_with_prefilt_and_zooms_to_the_flown_range` | gyro prefilt beside gyro (its preset 1) (DESIGN §6.7) | matched on evidence (the layout only) | a tone present only in `gyroUnfilt` |

Notes

- **T-1.** In full: (1) `nperseg` as in note S-1 from `segment_s`; the
  segments do not overlap; a segment counts when at least 90 % of its samples
  are valid. (2) Per segment the single-sided amplitude spectrum
  `|X_k| / Σw`, doubled for `0 < k < n/2`, with a periodic Hann window, so a
  sine of amplitude `A` at a bin centre reads `A`; the mean of the segment is
  **not** removed. (3) Per segment the mean of the throttle (L7). (4) For
  each bin `t` = 0, 1, …, 100 % the plain mean of the spectra of the segments
  with `|throttle − t| ≤ half_window`; a bin without segments stays 0.
  (5) A box filter with zero padding over the grid: along throttle
  `smooth_throttle` bins made odd (8 becomes 9), along frequency
  `round(smooth_freq_frac · nperseg)` bins made odd (3 at 1 kHz, 9 at 4 kHz);
  the sum is always divided by the full box, also at the edges of the grid.
  (6) Rows up to `max_hz`.

## 4. Freq × time

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **F1** Spectrogram: amplitude per time column and frequency (the channel's unit) | segments of `nperseg` samples (note S-1, from `segment_s`) every `floor(hop · nperseg)` samples; per segment the amplitude spectrum of note T-1 (2) (Hann, mean not removed); column time = segment centre + start of the window; rows up to `max_hz`; a last partial segment is dropped; invalid samples enter as filled by C2, without a validity check | `StftParams { segment_s 0.2, hop 0.25, window Hann, max_hz Some(1000) }`; channel: the unfiltered gyro (`TimeDisplay::prefilt true`) | `analysis/stft.rs:82` (`:89`, `:90`, `:105`); `dsp/spectrum.rs:150` | `stft_matches_reference`, `spectra_of_a_real_flight` (the shape), `time_tab_stacks_the_axes_in_db_with_the_throttle_under_them` | roll, pitch and yaw spectrograms of the unfiltered gyro (DESIGN §6.7; docs/design/time-tab); how they are computed is not recorded | never compared | a tone switched on at exactly 10.000 s: the width of its edge gives the segment length and the hop; a chirp gives the frequency axis |
| **F2** Colour in dB and its range | `20·log10(max(amplitude, 1e-9))`. Automatic range, shared by the three axes: per axis, floor = the 30th percentile of the dB values of its cells at ≥ 20 Hz, rounded down, and top = the 99.5th percentile, rounded up (at least floor + 1); the lowest floor and the highest top of the axes are used; "Z min" / "Z max" fix the ends; below the range is drawn black | `TimeDisplay { db true, z_min None, z_max None }`; `DB_LOW_PCT 0.30`, `DB_HIGH_PCT 0.995`, `SCALE_MIN_HZ 20` | `app/time_tab.rs:190`, `:234`, `:276`, `:379` | `time_tab_stacks_the_axes_in_db_with_the_throttle_under_them` (a floor below 0; fixed −30 / 10); no test of the automatic values | colour in dB, Z min −30, Z max 10 (DESIGN §6.7; docs/design/time-tab); what its 0 dB is, is not recorded | differs: "the dB levels differ" (DESIGN §6.7), which is why OpenPID's range is automatic | a sine of amplitude 10 deg/s: the dB value its map shows on the tone |
| **F3** Linear colour | 0 to the maximum of T3's rule, the largest of the rows | `Tools::heatmap_max 0` | `app/time_tab.rs:260`-`:266` | `heatmaps_show_a_colour_scale_with_linked_maxima_and_a_hover_value` | not recorded — round 0 | never compared | whether it has a linear colour option (round 0) |
| **F4** Spectrum at the position (beside each row) | the column of F1 nearest to the player position: amplitude, or the dB of F2, against frequency, with the colour range as its value axis | — | `app/time_tab.rs:440`; `app/views.rs:221` | `time_heatmap_player_plays_scrubs_and_syncs_the_cursor` | not on its screen; added by OpenPID (DESIGN §6.7) | OpenPID only | — |
| **F5** Throttle strip (%) | L7 against time, every `max(1, n / 2000)`-th sample (a plain stride, not the minimum and maximum of L11) | — | `app/time_tab.rs:488` | `time_tab_stacks_the_axes_in_db_with_the_throttle_under_them` | not on its screen; added by OpenPID (DESIGN §6.7) | OpenPID only | — |
| **F6** Position (s) and "column i of n" | the position limited to the first and last column time; the column is the nearest one; playing adds wall-clock time × speed and wraps to the start | `PlayerState::speed 1.0` | `app/time_tab.rs:197`, `:699` | `time_heatmap_player_plays_scrubs_and_syncs_the_cursor`, `player_keys_work_on_the_time_tab_only` | not recorded — round 0 | never compared | whether it has a position or a player (round 0) |
| **F7** Value under the pointer | the nearest cell: "t s, f Hz: z", in dB when the colour is, three decimals | — | `app/time_tab.rs:178`-`:181`; `app/views.rs:1514` | `heatmaps_show_a_colour_scale_with_linked_maxima_and_a_hover_value` | not recorded — round 0 | never compared | whether it shows a value under the pointer (round 0) |

## 5. Step response

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **R1** Step windows: "n=" in the legend, the "windows" verdict | windows of `round(window_s · fs)` samples, one every `floor(window / superpos)` samples. Used: the largest `abs(setpoint)` of the window is ≥ `min_input`, at least 95 % of its setpoint samples and of its gyro samples are valid, and its response settles (the mean of its last quarter) within `settle_range`. Rejected: failed only the last test | `StepParams { window_s 2.0, superpos 16, min_input 80, settle_range (0.5, 1.5) }` | `analysis/step.rs:360` (`:388`-`:389`, `:409`-`:431`), `:270` | `step_response_matches_the_reference_tool` (within a factor of two), `step_response_of_a_real_flight`, `latency_equals_the_plant_delay_for_a_white_input` | its rule is not public; `btfl_027`: 78 / 13 / 30 windows (roll / pitch / yaw); 4.5.1 yaw: 32; "insufficient data" on 4.5.1 roll and pitch (DESIGN §6.2) | differs: 65 / 14 / 27, and 27; "not enough data" in the same two cases | isolated steps 3 s apart at 40, 60, 80, 100 and 150 deg/s: which of them it counts, and how many windows per step |
| **R2** Mean response curve (1 = the setpoint) | Wiener deconvolution of setpoint → filtered gyro per used window, the running sum of the impulse response, the mean over the windows weighted by each window's largest `abs(setpoint)` (note R-1) | `StepParams { cutoff_hz 25, response_s 0.5 }`; `DeconvParams { reg_low 0.1, reg_high 1e8, transition 0.2, window Hann }` | `analysis/step.rs:400`-`:426`, `:250`, `:463`; `dsp/deconv.rs:62`, `:42`, `:112` | `deconvolution_matches_reference` (against the same formula written again in Python), `recovers_first_order_plant`, `recovers_a_fir_plant_and_its_step_is_the_cumulative_sum`, `step_response_recovers_the_plant`, `step_response_matches_the_reference_tool` | the mean response per log; peaks 1.15 / 1.12 / 1.18 (`btfl_027`) and 0.87 (4.5.1 yaw) (DESIGN §6.2); its method is not public | matched on evidence: peaks 1.14 / 1.14 / 1.24 / 0.88, within the 0.08 the test allows | a second-order plant of known overshoot and delay, the same stick pattern at 100, 300 and 1000 deg/s: the same curve at every amplitude or not |
| **R3** Low-input and high-input curves | R2 over the used windows whose largest `abs(setpoint)` is below `high_input`, and over those at or above it; drawn only when both groups exist | `StepParams::high_input 500` | `analysis/step.rs:453`-`:462`, `:497`-`:498`; `app/axis_layout.rs:775` | `validation_and_variants` (that they exist) | not recorded — round 0 | never compared | steps of 200 and of 800 deg/s through a plant whose gain drops at high rates |
| **R4** Raw responses (a density) | a histogram of the unsmoothed window responses: per time sample, amplitude bins of width `(hi − lo) / bins` over `hist_range` (values outside are dropped), every response adding its weight / the total weight; colour from 0 to the 99th percentile of the positive cells; the first visible log only | `StepParams { hist_range (−0.5, 2.5), hist_bins 150 }`; `StepDisplay::raw false` | `analysis/step.rs:468`-`:491`; `app/axis_layout.rs:741`; `app/views.rs:1292` | `step_response_of_a_real_flight` (the weight per column) | "Raw", off by default (DESIGN §6.7); what it draws is not recorded | never compared (the default is the same) | two plants that alternate (gain 0.8 and 1.2): two bands or a cloud |
| **R5** Smoothing | a centred moving average of the mean curve and of R3 over `floor(smooth_ms / 1000 · fs)` samples made odd, the window shrinking symmetrically at the ends; applied before the metrics R7–R13 | `StepParams::smooth_ms 0`; panel: off 0, low 10, medium 20, high 40 ms | `analysis/step.rs:446`-`:452`; `dsp/smooth.rs:5`; `app/axis_layout.rs:195` | `smoothing_matches_reference` and `averages` (the average); no test of a smoothed response | several levels, the default is not recorded (DESIGN §6.7); DESIGN §6.2 names them "1/20/40/60" | never compared | a plant with a sharp 30 % overshoot at each of its smoothing levels: the peak against the level |
| **R6** Y correction | every window's response ÷ the mean of its last quarter (when that is above 0.2), before the averaging | `StepParams::y_offset false` | `analysis/step.rs:270`-`:282` | `stick_input_latency_and_y_offset_correction` | a control exists, its default is not recorded (DESIGN §6.7) | never compared | a plant of gain 0.8: its curve ends at 0.8 or at 1.0 by default |
| **R7** Peak plot: point and bar | point = the maximum of the mean curve (R2 after R5), not divided by the final value; bar = ± the population standard deviation of the used windows' own maxima | — | `app/axis_layout.rs:827` (`:840`-`:841`); `analysis/step.rs:176`-`:194` | `step_response_matches_the_reference_tool` (the point); no test of the bar | a "peak" per log and axis (DESIGN §6.2, the table) | matched on evidence (the point, as R2); the bar: never compared | as R2; its error bar on the two-plant log of R4 |
| **R8** Latency plot, "SR latency (50 %)" (ms) | point = the time of the first sample of the mean curve with `value / final ≥ 0.5`; bar = ± the standard deviation over the windows of the time of their first sample with `value ≥ 0.5`; whole samples, no interpolation | `StepDisplay::latency HalfRise` | `analysis/step.rs:312`-`:313`, `:330`, `:181`-`:185`; `app/axis_layout.rs:868` | `step_response_matches_the_reference_tool` (within 2 ms), `latency_equals_the_plant_delay_for_a_white_input` and `step_response_recovers_the_plant` (within 4 ms) | "SR Latency", the time the mean response takes to reach 50 %: 22 / 22.5 / 12.3 ms (`btfl_027`), 11.7 ms (4.5.1 yaw) (DESIGN §6.2) | matched on evidence: 22 / 21 / 12 / 12 ms, within 2 ms; OpenPID reads whole samples, the reference tool shows tenths of a millisecond | a plant that is a pure delay of 12.3 ms, logged at 1 kHz: 12, 13 or 12.3 ms |
| **R9** Latency plot, "SP-gyro delay (xcorr)" (ms) | the lag, between 0 and 100 ms, at which the cross-correlation of setpoint and gyro over the whole window C3 is largest: means removed, every lag divided by its overlap, the peak refined by a parabola through its neighbours; nothing when no lag correlates positively | `LATENCY_MAX_LAG_S 0.1` | `analysis/step.rs:294`, `:465`-`:466`; `dsp/xcorr.rs:11` | `pure_delay_and_first_order_plant`, `degenerate_inputs`, `latency_equals_the_plant_delay_for_a_white_input`, `stick_input_latency_and_y_offset_correction` | "SP-Gyro Delay" ("Xcorr Latency" in v0.8) as its other option, which "most likely corresponds" (DESIGN §6.2); no value is recorded | never compared | a white-noise setpoint through a pure delay of 9.4 ms |
| **R10** Rise time (ms) | the time between the first samples at ≥ 10 % and at ≥ 90 % of the final value | target `StepTargets::rise_ms_max 60` | `analysis/step.rs:312`-`:317` | `metrics_of_ideal_responses` (within 2 ms) | not recorded — round 0 | never compared | a first-order plant of 20 ms: 44 ms |
| **R11** Overshoot (%) | `(maximum / final − 1) × 100`; 0 when the maximum is not above the final value | targets `overshoot_warn_pct 15`, `overshoot_fail_pct 30` | `analysis/step.rs:318`-`:323` | `metrics_of_ideal_responses` (within 5 points) | shows the peak, not a percentage (DESIGN §6.2, the table) | never compared | as R2 |
| **R12** Settling time (ms) | the time of the sample after the last one that is more than 5 % away from the final value; nothing when the last sample still is | target `settling_ms_max 120` | `analysis/step.rs:324`-`:337` | `metrics_of_ideal_responses` (within 2 ms) | not recorded — round 0 | never compared | a first-order plant of 20 ms: 60 ms |
| **R13** Final value | the mean of the last quarter of the curve (the last 125 ms of 0.5 s) | target `final_tol 0.10` | `analysis/step.rs:284`, `:301` | `metrics_of_ideal_responses`, `latency_equals_the_plant_delay_for_a_white_input` (the plant gain within 0.08) | not recorded — round 0 | never compared | a plant of gain 0.8 |
| **R14** Verdicts, the target band and lines | every metric judged ok / warn / fail / unknown against `StepTargets` (note R-2); the band runs from `min(settling_ms_max, response length)` between `1 ± final_tol`; lines at `1 + overshoot_warn_pct / 100` and `1 + overshoot_fail_pct / 100` | presets: Freestyle (default) 15 / 30 %, 60 ms, 120 ms, 0.10, 10 / 5 windows; Racing 20 %, 40 ms; Cinematic 8 %, 100 ms | `analysis/targets.rs:80`, `:165`; `app/axis_layout.rs:341`-`:345`, `:402`, `:682` | `verdicts_follow_the_thresholds`, `too_few_windows_make_everything_unknown`, `presets_and_defaults`, `verdicts_of_the_real_logs`, `step_tab_judges_three_axes_and_targets_change_badges_without_new_jobs` | no verdicts: its users judge the curve by eye (docs/design/step-verdicts) | OpenPID only | — |
| **R15** Gains in the legend, "P,I,D,Dm,F" | the logged gains of the axis at the firmware's gain step; Dm = D-max, else D-min, else 0; F = 0 when absent | — | `app/axis_layout.rs:308`; `core/settings.rs:247` | `the_legend_shows_gains_at_the_firmwares_step` | per log "A: P,I,D,Dm,F = 45,80,30,40,120 (n=65)" (docs/design/axis-layout) | never compared | a log with D-min and D-max both logged: which one it prints as Dm |
| **R16** Advice line | the first `pid.*` proposal of K12 for the first visible log | — | `app/axis_layout.rs:599` | `step_tab_judges_three_axes_and_targets_change_badges_without_new_jobs` | no advice of its own (docs/design/advice-tab) | OpenPID only | — |

Notes

- **R-1.** Per used window: setpoint and gyro (both filled by C2) are each
  multiplied by a periodic Hann window and zero-padded to the smallest
  2-3-5-smooth length of at least 1.5 × the window (3000 samples for 2 s at
  1 kHz). `H = Y · conj(X) / (|X|² + λ(f))` with the unnormalised FFT;
  `λ = reg_low` at or below `0.8 · cutoff_hz`, `reg_high` at or above
  `1.2 · cutoff_hz`, and between them a raised cosine in `ln λ`. The impulse
  response is the inverse FFT, its first `round(response_s · fs)` samples;
  the step response is its running sum. Neither signal has its mean removed.
  `λ` is added to `|X|²` as it is: it is not scaled by the input power, the
  window length or the sample rate.
- **R-2.** Windows: ok at ≥ `min_windows_ok`, warn at ≥ `min_windows_weak`,
  else fail and every other verdict "unknown". Rise: warn above
  `rise_ms_max`. Delay and latency: informational. Overshoot: warn above
  `overshoot_warn_pct`, fail above `overshoot_fail_pct`. Settling: warn above
  `settling_ms_max` or when it never settles. Final: warn when
  `|final − 1| > final_tol`. Values are printed as whole milliseconds and
  whole percent, latency with one decimal, final with two.

## 6. Filter Sim

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **X1** Filter chain: magnitude per part and in total (dB) | `20·log10(max(abs(H(f)), 1e-9))` at 400 frequencies `top · i / 400`, `top = min(max_hz or fs/2, fs/2)`; `H` is the discrete transfer function of each stage at the log's sample rate (X6); a part is the product over its stages of `w · H + (1 − w)`, the total the product of the parts; all at the operating point X5 | `RESPONSE_POINTS 400`; `FilterSimParams::max_hz None`; axis −40…3 dB | `analysis/filter_sim.rs:372`, `:404`, `:647`-`:652`; `sim/model.rs:157`, `:323`; drawn `app/filter_tab.rs:139` | `chain_response_has_delay_and_notch_depth`, `parts_at_an_operating_point`, `pt1_matches_the_analytic_first_order_response`, `chain_multiplies_responses_and_runs_stages_in_order` | a column for the low-pass filters and one for the notch filters, each with magnitude against frequency (docs/design/filter-tab); no value is recorded | never compared | its filter screen set to PT1 100 Hz: the −3 dB frequency and the level at 400 Hz (analogue or discrete, and at which rate) |
| **X2** Phase (deg) | the argument of `H(f)`, unwrapped along the frequency axis (each point moved by the multiple of 360° that brings it nearest to the previous one) | — | `analysis/filter_sim.rs:385`-`:391` | `chain_response_has_delay_and_notch_depth` | phase against frequency (docs/design/filter-tab) | never compared | a biquad low-pass at 100 Hz: the phase at 100 Hz (−90°) |
| **X3** Delay (ms) | the phase delay `−φ / (2π f) × 1000` of the unwrapped phase of X2 | — | `analysis/filter_sim.rs:392` | `chain_response_has_delay_and_notch_depth` | "filter delay (ms)" against frequency (docs/design/filter-tab); whether it is a phase delay or a group delay is not recorded | never compared | PT1 100 Hz: the delay it reads at 100 Hz (1.25 ms is the phase delay, 0.80 ms the group delay) |
| **X4** "delay at 100 Hz" (ms) | X3 of the whole chain at `min(100 Hz, 0.45 · fs)` | — | `analysis/filter_sim.rs:416`, `:424`; `app/filter_tab.rs:153`-`:164` | `chain_response_has_delay_and_notch_depth` | not recorded — round 0 | never compared | whether it prints one delay figure for the chain (round 0) |
| **X5** Operating point: "throttle %", "motors Hz", the dynamic-notch centres | throttle = the mean of L7 / 100 over the window; per motor the mean of rpm / 60 over the samples with rpm > 0 (the text shows the mean of the motors); per dynamic notch the median over the samples of its tracked centre (X12) | — | `analysis/filter_sim.rs:438`; `app/filter_tab.rs:141`-`:148` | `filter_sim_tab_validates_the_model_and_applies_what_if_changes` | has no log on this screen (docs/design/filter-tab) | OpenPID only | — |
| **X6** The filter stages | PT1: `y += k (x − y)`, `k = dt / (1 / (2π fc) + dt)`; PT2 and PT3: two and three PT1 in series at `fc / sqrt(2^(1/n) − 1)`; biquad low-pass: the RBJ form with Q = 1/√2; notch: the RBJ form with `Q = fc · fcut / (fc² − fcut²)`; `dt` is the log's sample step, not the gyro loop's; cut-offs and centres are limited to `0.48 · fs` | from `Settings::gyro_filters`; an unknown low-pass type is taken as PT1; `MAX_NOTCH_FRACTION 0.48` | `sim/filter.rs:48`, `:106`, `:122`, `:169`, `:221`, `:247`; `sim/model.rs:579`, `:171` | `pt1_matches_the_analytic_first_order_response`, `pt2_and_pt3_are_at_minus_3_db_at_the_cutoff`, `biquad_lowpass_is_butterworth`, `notch_is_deep_at_the_centre_and_minus_3_db_at_the_cutoff`, `every_filter_is_stable_across_the_parameter_range` | which filter types it offers is not recorded | never compared | as X1, for PT2, PT3, a biquad and a notch at 200 Hz with a 150 Hz cut-off |
| **X7** Simulated gyro, the time trace (deg/s) | the unfiltered gyro, filled by C2, sample by sample through RPM notches (X11) → static notches → low-pass 1 → low-pass 2 → dynamic notches (X12); "sim" with the what-if overrides, "sim as flown" without | `FilterOverrides` (all unset) | `sim/model.rs:424`; `analysis/filter_sim.rs:597`-`:610`; drawn `app/filter_tab.rs:299` | `static_run_equals_the_static_chain`, `filter_sim_reproduces_the_synthetic_filter_exactly`, `betaflight_2025_12_simulated_gyro_matches_the_logged_filtered_gyro`, `overrides_edit_every_stage` | runs a chirp through its filters, not a log (docs/design/filter-tab) | OpenPID only | — |
| **X8** Spectra of raw, logged, sim and sim as flown (dB) | S1 with this tab's `segment_s`, half overlap and Hann; `10·log10(max(power, 1e-30))`; not smoothed | `FilterSimParams::segment_s 0.25` | `analysis/filter_sim.rs:612`-`:644`; drawn `app/filter_tab.rs:272` | `welch_matches_scipy`, `filter_sim_tab_validates_the_model_and_applies_what_if_changes` | none | OpenPID only | — |
| **X9** Fit: residual %, unfiltered %, delay (ms) | after the first `min(fs, n / 4)` samples: residual = RMS(sim as flown − logged) / RMS(logged); unfiltered = RMS(raw − logged) / RMS(logged); delay = the lag of the cross-correlation maximum (the method of R9) of the sim behind the logged gyro within ± 20 ms, negative when the sim leads | — | `analysis/filter_sim.rs:660`-`:680`, `:531` | `filter_sim_reproduces_the_synthetic_filter_exactly`, `signed_lag_has_a_direction`, `betaflight_2025_12_simulated_gyro_matches_the_logged_filtered_gyro` (residual below 25 %) | none | OpenPID only | — |
| **X10** Dynamic low-pass cut-off (Hz) | `min + (max − min) · (t (1 − t) · expo + t)`, `t` = the throttle 0…1 rounded to 0.01, recomputed every `round(0.005 · fs)` samples | `DYN_LPF_THROTTLE_STEPS 100`, `DYN_LPF_UPDATE_S 0.005`; the range and expo from the log | `sim/model.rs:41`, `:513`-`:527` | `dynamic_lowpass_curve_follows_betaflight`, `throttle_opens_the_dynamic_lowpass` | not recorded — round 0 | never compared | whether its filter screen has a throttle-driven low-pass (round 0) |
| **X11** RPM notches | per motor and harmonic `k`: a notch at `k · f`, `f` = rpm / 60 through a PT1 at `rpm_lpf_hz`, the centre not below `min_hz`; weight = `clamp((k · f − min_hz) / fade_range_hz, 0, 1)` × the harmonic's weight; output = `w · notched + (1 − w) · input`; no notch at or above `0.48 · fs` | from the log (`harmonics`, `q`, `min_hz`, `fade_range_hz`, `weights`, `lpf_hz`); what-if defaults 3, Q 5, 100 Hz, 50 Hz, 150 Hz | `sim/model.rs:361`, `:491`-`:507`, `:560`; `analysis/filter_sim.rs:194`-`:201` | `rpm_notches_remove_the_motor_tone_and_fade_near_the_minimum`, `betaflight_4_5_0_model_builds_from_the_headers` | not recorded — round 0 | never compared | whether its notch column includes RPM notches (round 0) |
| **X12** Dynamic notch centres (Hz) | an STFT of the input (Hann, `round(0.1 · fs)` samples made 2-3-5-smooth, hop `round(0.016 · fs)`); the `count` largest local maxima of the amplitude between `min_hz` and `max_hz`, each refined by a parabola, sorted by frequency; every slot through a PT1 of 4 Hz, held between hops, starting at `max_hz` | from the log; what-if defaults: 1 notch, Q 3, 150–600 Hz; `DYN_NOTCH_SMOOTH_HZ 4` | `sim/model.rs:606`, `:536`-`:549` | `dynamic_notch_tracks_a_moving_tone` | not recorded — round 0 | never compared | whether its notch column includes dynamic notches (round 0) |
| **X13** "Model" lines, "Throttle", "Motors" | the chain in words (cut-offs without decimals, Q with one); whether a throttle channel drove X10; the number of motor rpm channels | — | `sim/model.rs:248`; `app/filter_tab.rs:393` | `filter_sim_reproduces_the_synthetic_filter_exactly`, `betaflight_4_5_0_model_builds_from_the_headers` | none | OpenPID only | — |

## 7. Sliders

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **P1** Betaflight sliders: the gains "from the sliders" | Betaflight's simplified tuning (4.3 and later) in `f32`, limited to 0–250 (F: 0–1000) and truncated to whole numbers; the axes above the slider mode keep the logged gains (note P-1) | the 12 sliders of the log's headers (the firmware default where a header is absent); firmware defaults P/I/D/F roll 45/80/40/120, pitch 47/84/46/125, yaw 45/80/0/120 with D-min 30/34/0; from 2025.12: D 30/34/0 with D-max 40/46/0 | `bbx/profiles/sliders.rs:36`, `:62`, `:251`, `:319` | `reproduces_betaflight_4_5_log`, `reproduces_betaflight_2025_12_log`, `absent_before_4_3_and_mode_limits_axes`, `sliders_command`, `sliders_tab_tracks_every_log_and_gives_the_gains_and_commands` | a "Betaflight PID Sliders" plot of the slider positions (docs/design/sliders-tab); whether it computes gains from them is not recorded | never compared (the gains are pinned against the gains two firmware versions logged) | whether it shows gains for moved Betaflight sliders (round 0) |
| **P2** Filter cut-offs from the sliders (Hz) | `default × multiplier / 100`, limited to 0–1000 and truncated; gyro low-pass 1: 250–500 (the lower value twice when it is static), low-pass 2: 500; D-term low-pass 1: 75–150, low-pass 2: 150; only for the filters the log had switched on | `simplified_gyro_filter_multiplier 100`, `simplified_dterm_filter_multiplier 100` | `bbx/profiles/sliders.rs:228`, `:233`, `:343`-`:380`; `app/sliders_tab.rs:424` | `reproduces_betaflight_4_5_log`, `filters_are_summed_up_a_line_per_chain` | not recorded — round 0 | never compared | whether it shows filter cut-offs for the filter sliders (round 0) |
| **P3** As-flown positions (the marks, %) | the slider headers of every visible log that has the same slider model | — | `bbx/profiles/sliders.rs:170`; `app/sliders_tab.rs:143` | `sliders_tab_tracks_every_log_and_gives_the_gains_and_commands` | one line per slider with a dot per log, positions 0…2 (docs/design/sliders-tab) | differs in the unit only: percent against 0…2 | two logs with a master multiplier of 85 and of 120 |
| **P4** Ratio sliders: the gains | on the logged gains, with every slider as percent / 100: roll P × m, I × pi · m, D × pd · m; pitch the same × rp; yaw P and I × yaw · m, D × pd · yaw · m; D-min and D-max scale as D; F is unchanged; rounded to the firmware's gain step | 5 sliders (`master_multiplier`, `pd_ratio`, `pi_ratio`, `roll_pitch_ratio`, `yaw_ratio`), 0–200, default 100 | `core/sliders.rs:114`, `:152`; `core/settings.rs:233` | `ratio_sliders_scale_the_logged_gains` (the reference tool's numbers, within 0.006), `ratio_sliders_keep_the_firmwares_gain_step` | its slider tool: start gains 4 / 0.05 / 15 and ratios 1.35, 1.20, 1.30, 1.05, master 1.45 (its tutorial video; docs/design/sliders-tab); the resulting gains are in the test: roll 5.8 / 0.087 / 29.36, pitch 7.54 / 0.113 / 38.17, yaw 6.09 / 0.076 / 4.11 | matched on evidence | a second set of start gains and ratios, to confirm which ratios act on yaw I and D |
| **P5** The logged gains table | the PID headers at the firmware's gain step: the decimals of the step, trailing zeros cut | `Settings::pid_step` (1 when absent) | `core/settings.rs:247`; `app/sliders_tab.rs:362` | `gains_are_written_at_the_firmware_step`, `the_legend_shows_gains_at_the_firmwares_step` | not recorded — round 0 | never compared | a log with a gain of 0.1546: how it prints it |
| **P6** Commands | `set <slider> = <whole number>` for every slider, then `simplified_tuning apply` and `save` | — | `bbx/profiles/sliders.rs:363`-`:370` | `reproduces_betaflight_4_5_log`, `sliders_command` | not recorded — round 0 | never compared | whether it prints commands (round 0) |

Notes

- **P-1.** With the slider values as factors (`value / 100`) and the firmware
  defaults `P0`, `I0`, `D0`, `F0` of the axis: `P = P0 · master · pi`,
  `I = I0 · master · pi · i`, `F = F0 · master · ff`, each × `pitch_pi` on
  pitch. Up to 4.5 (D-min scheme, `Dmin0` = 30 / 34 / 0):
  `D = Dmin0 · master · d · pitch_d · (1 + (D0 − Dmin0) / Dmin0 · d_max)` and
  `D-min = Dmin0 · master · d · pitch_d`. From 2025.12 (D-max scheme,
  `Dmax0` = 40 / 46 / 0): `D = D0 · master · d · pitch_d` and
  `D-max = Dmax0 · master · d · pitch_d · (d_max + (1 − d_max) · D0 / Dmax0)`.
  The products are evaluated left to right in `f32`, as the firmware does,
  because the order decides the truncation.

## 8. Setup info

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **U1** Log check: the verdict | the worst of U2, U3 and U6; computed on the whole session, whatever the analysis window | — | `analysis/fitness.rs:339`; `app/setup_tab.rs:159` | `a_good_synthetic_log_passes_and_a_short_silent_one_fails`, `missing_channels_are_reported_with_a_fix`, `fitness_of_the_real_logs`, `log_check_badges_and_card_follow_the_session`, `setup_info_has_a_column_per_log_and_marks_what_differs` | no log check: its Setup Info lists headers only (docs/design/setup-tab) | OpenPID only | — |
| **U2** rate: "n Hz logged" | `1e6 / median step` (C6, not C1); ok at ≥ 950 Hz, warn at ≥ 500 Hz | `RATE_OK_HZ 950`, `RATE_WARN_HZ 500` | `analysis/fitness.rs:152` (`:73`-`:74`) | as U1 | none | OpenPID only | — |
| **U3** duration (s) | last − first time stamp; ok at ≥ 20 s, warn at ≥ 5 s | `DURATION_OK_S 20`, `DURATION_WARN_S 5` | `analysis/fitness.rs:168` | as U1 | none | OpenPID only | — |
| **U4** raw: is the unfiltered gyro logged | any of the three channels of L2 exists | — | `analysis/fitness.rs:184` | `missing_channels_are_reported_with_a_fix` | none | OpenPID only | — |
| **U5** rpm: is the motor rpm logged | any channel of L8 exists | — | `analysis/fitness.rs:210` | `missing_channels_are_reported_with_a_fix` | none | OpenPID only | — |
| **U6** frames: "c corrupt of n frames (x %)" | `corrupt / (valid + corrupt)`; ok below 0.5 %, warn up to 5 % | `CORRUPT_WARN 0.005`, `CORRUPT_FAIL 0.05` | `analysis/fitness.rs:226` | as U1 | none | OpenPID only | — |
| **U7** gaps: "n gap(s) in the timebase" | the number of steps above 1.5 × the median step; ok at 0, warn up to 3 | `GAPS_WARN 3` | `analysis/fitness.rs:252`; `core/session.rs:211` | `timebase_gaps`, as U1 | none | OpenPID only | — |
| **U8** sticks: "a roll / b pitch step windows" | the windows of R1 that pass the input and validity tests (not the settle test), with the default `StepParams`, on the whole session; judged on the smaller of the two: ok at ≥ 10, warn at ≥ 5 | `STICKS_OK 10`, `STICKS_WARN 5` | `analysis/fitness.rs:268`; `analysis/step.rs:201` | `a_good_synthetic_log_passes_and_a_short_silent_one_fails` | none | OpenPID only | — |
| **U9** throttle: "x % of the throttle range has 3+ segments" | segments of `round(0.2 × the rate of U2)` logged samples, at least 90 % valid; the mean throttle of each rounded to a whole percent; a bin is covered when the bins within ± 3 % hold at least 3 segments; the share of the 101 bins; ok at ≥ 30 %, warn at ≥ 10 % | `THROTTLE_SEGMENT_S 0.2`, `THROTTLE_HALF_WINDOW 3`, `THROTTLE_MIN_SEGMENTS 3`, `THROTTLE_OK 0.3`, `THROTTLE_WARN 0.1` | `analysis/fitness.rs:116`, `:296` | `fitness_of_the_real_logs`; no test of a coverage value | none | OpenPID only | — |
| **U10** debug: the debug mode | the mode's name and code as logged | — | `analysis/fitness.rs:322` | `a_good_synthetic_log_passes_and_a_short_silent_one_fails` | none | OpenPID only | — |
| **U11** Decoder: samples and length, sample step and gaps, frames by kind, corrupt and desynchronised frames, skipped bytes | the decoder's counts; the step is the median step in µs | — | `core/describe.rs:212` | `decoder_rows_count_what_was_read_and_what_was_not`, `setup_info_shows_every_fixture` | not in its Setup Info (docs/design/setup-tab) | OpenPID only | — |
| **U12** Setup in words: the rates (Hz), the PID gains, the filters, the motors | from the typed settings: gyro rate = `1e6 / looptime`, PID rate = gyro rate / `pid_process_denom`, logging rate as in note C-1; the gains at the gain step (P5); the filters as "PT1 500 Hz" or "biquad 75–150 Hz by throttle" | — | `core/describe.rs:147`; `bbx/profiles/common.rs:404` | `setup_reads_the_typed_settings_in_plain_words`, `settings_in_words_come_from_the_betaflight_profile_only`, `setup_info_shows_every_fixture` | no setup in plain words (docs/design/setup-tab) | OpenPID only | — |
| **U13** All headers; the rows that differ | every header as `name: value` as logged; a row differs when the logs' values are not all the same (a missing row counts as different) | `SetupDisplay::differences_only false` | `core/describe.rs:264`; `app/setup_tab.rs:248` | `compare_matches_rows_by_label_and_marks_the_differences`, `setup_info_has_a_column_per_log_and_marks_what_differs` | the headers of the loaded files as `name:value` rows side by side, the differing rows on pink, "Show Differences Only" (docs/design/setup-tab) | never compared (the same idea; no screen was compared row by row) | two logs that differ in one header and list two others in another order |

## 9. CLI only

`openpid run <analysis>` prints the result structure as JSON. By default it
analyses the whole log (C3) and the filtered gyro of `--axis`. The rows
below list what the command line shows beyond the tabs: the fields no tab
draws, the two analyses that have no tab (`motor-noise`, `rpm`) and the
rules of `recommend`, which the Advice tab draws.

| Quantity | How OpenPID computes it | Parameters and defaults | Code | Pinned by | Reference tool | Status | Probe idea |
|---|---|---|---|---|---|---|---|
| **K1** `step`: `StepOutput` and `verdicts` | R1–R13 and R14; in the JSON only: `windows_total` and `windows_rejected` (R1), `spread.peak_mean` (the mean of the windows' own maxima), `spread.half_ms_mean` (the mean of the windows' times to 0.5), `fs`, the histogram of R4 | `StepParams` defaults; the targets of a `--project`, else Freestyle | `cli/main.rs:555`; `analysis/step.rs:119`, `:153` | `run_analyses`, `json_export_equals_the_cli_serialisation_and_names_the_file` | as R1–R13 | differs (R1), matched on evidence (R2, R7, R8), otherwise never compared | as R1–R13 |
| **K2** `psd`: `freq`, `power`, `power_db`, `nperseg`, `segments`, `unit` | S1 without the app's smoothing; `power_db = 10·log10(max(power, 1e-30))` | `PsdParams` defaults; `--channel` | `cli/main.rs:567`; `analysis/psd.rs:133` | `run_analyses`, `validation_and_variants` (linear and dB agree) | as S1 | differs (as S1) | as S1 |
| **K3** `thr-heatmap`: `grid`, `counts`, `overlay`; `--png` | T1, T2 and the data of T5 and T6; the picture has one pixel per cell, viridis, the automatic maximum of T3 | `ThrottleHeatmapParams` defaults | `cli/main.rs:578`, `:679`-`:685`; `analysis/render.rs:98` | `run_analyses`, `png_of_a_heatmap` | as T1 | never compared (as T1) | as T1 |
| **K4** `stft`: `grid`; `--png` | F1 as linear amplitude (no dB); the default channel is the filtered gyro, not the tab's unfiltered one | `StftParams` defaults | `cli/main.rs:589` | `run_analyses` | as F1 | never compared (as F1) | as F1 |
| **K5** `filter-sim`: everything of X1–X13, and `response_db` | `response_db` = `20·log10` of the gain of the static part of the what-if chain (the static notches and the low-passes at their static cut-off, or at the dynamic minimum) at the frequencies of X8; no tab draws it | `--set key=value` fills `FilterOverrides` | `cli/main.rs:662`; `analysis/filter_sim.rs:645`-`:646`; `sim/model.rs:299` | `filter_sim_command` | none | OpenPID only | — |
| **K6** `motor-noise`: per motor and harmonic `amplitude` (the channel's unit) and `mean_hz` | an STFT of the unfiltered gyro (note S-1 for the segment, hop half a segment, Hann, the amplitude of note T-1 (2)); per frame the motor's rpm at the frame centre; harmonic `k` sits at bin `round(k · rpm / 60 / df)`; amplitude = the largest bin within ± `bins`; the mean over the frames with rpm > 0 whose band is inside the spectrum. Without logged rpm the estimate of K9 is used, as one row for all motors | `MotorNoiseParams { segment_s 0.25, harmonics 3, bins 1, max_hz None }` | `cli/main.rs:600`; `analysis/motor_noise.rs:127` (`:149`, `:196`-`:219`) | `motor_noise_table_shows_the_bent_prop`, `motor_noise_command` | no such screen (docs/USER-GUIDE.md §8; docs/design/motors-tab) | OpenPID only | — |
| **K7** `motor-noise`: `ratio`, `total`, `loudest`, `mean_rpm` | ratio = the amplitude over the mean of the motors at that harmonic (1 when that mean is 0); total = the sum of a motor's harmonic amplitudes; loudest = the motor with the largest total; mean_rpm = the mean over the frames | — | `analysis/motor_noise.rs:220`-`:244` | `motor_noise_table_shows_the_bent_prop` | none | OpenPID only | — |
| **K8** `motor-noise`: `psd_db`, `frames`, `rpm_source` | S1 (0.25 s, half overlap) as `10·log10(max(power, 1e-30))`; the number of STFT frames; `logged` or `estimated` | — | `analysis/motor_noise.rs:246`-`:273` | `motor_noise_command` | none | OpenPID only | — |
| **K9** `rpm`: the estimated `rpm` per frame | an STFT of the unfiltered gyro (0.25 s, hop `round(0.25 · nperseg)`, Hann); per frame up to 6 most prominent peaks at least 2 bins apart between `rpm_min / 60` and `harmonics · rpm_max / 60`; every peak is a candidate as the 1st…3rd harmonic of a speed inside the range; a dynamic programme picks the path with the largest sum of `ln(strength) − 0.5 · (h − 1)` minus `continuity · abs(ln(rpm / previous rpm))`; a frame without a peak carries the middle of the range | `RpmEstimateParams { rpm_min 1500, rpm_max 35000, segment_s 0.25, hop 0.25, harmonics 3, continuity 8.0 }` | `cli/main.rs:645`; `analysis/rpm.rs:120` (`:172`-`:242`) | `rpm_estimate_follows_the_synthetic_motors`, `rpm_estimate_tracks_the_logged_motor_speed`, `rpm_command` | not recorded — round 0 | never compared | whether it estimates motor speed without telemetry (round 0) |
| **K10** `rpm`: `confidence`, `harmonic` | confidence = `(s − 1) / (s + 1)` limited to 0–1, `s` = the chosen peak's amplitude over the median amplitude of the band; harmonic = which multiple the chosen peak was taken as | — | `analysis/rpm.rs:177`, `:243`-`:255` | `rpm_estimate_follows_the_synthetic_motors` (mean confidence above 0.5) | not recorded — round 0 | never compared | as K9 |
| **K11** `rpm`: `logged_rpm`, `median_error` | the mean of the logged motors at each frame centre; the median (for an even count the upper middle value) of `abs(estimate − logged) / logged` over the frames with a logged speed above 0 | — | `analysis/rpm.rs:257`-`:282` | `rpm_estimate_follows_the_synthetic_motors` (below 3 %), `rpm_estimate_tracks_the_logged_motor_speed` | not recorded — round 0 | never compared | as K9 |
| **K12** `recommend`, overshoot rule: a new D, its confidence | per axis, on R11 with at least 5 used windows: above `overshoot_warn_pct` (and not above 200 %), with the D-term quiet (the mean dB of S1 on the D-term in 150–400 Hz minus that in 5–30 Hz is at most −15 dB): new D = the larger of `round(D × 1.10)` and `round(D + one gain step)`; confidence = `(overshoot − limit) / limit` limited to 0.3–0.9 | `RecommendParams { d_step_pct 10, dterm_noise_max_db −15 }`, `targets` (R14) | `analysis/recommend.rs:178`, `:132`, `:143` | `overshoot_raises_d_when_the_dterm_is_quiet`, `recommend_command_reports_evidence_and_cli_commands`, `advice_tab_lists_proposals_evidence_and_cli` | no advice of its own (docs/design/advice-tab) | OpenPID only | — |
| **K13** `recommend`, peak rule: notch settings | per axis, on the unsmoothed dB of S1 of the unfiltered gyro (the filtered one without it): the 5 most prominent peaks with a prominence of at least 8 dB, at least 10 Hz apart; the most prominent of them at ≥ 80 Hz; fine when a static notch (`abs(f − centre) ≤ abs(centre − cutoff)`), the dynamic-notch range or the RPM filter (`f ≥ min_hz`) covers it; else, with a dynamic notch in the log, its minimum = `0.8 f` (a peak below the range) or its maximum = `1.2 f` (above it), and without one a static notch at `f` with cut-off `0.8 f`; each rounded to 5 Hz; confidence = `prominence / 20` limited to 0.3–0.9 | `peak_min_prominence_db 8`, `peak_min_hz 80` | `analysis/recommend.rs:357` | `an_uncovered_peak_gets_a_notch` | none | OpenPID only | — |
| **K14** `recommend`, gyro low-pass rule: a new cut-off | for a static low-pass 1 at `c` with `2c ≤ 0.45 · fs`: headroom = the larger over roll and pitch of the mean dB of S1 in 10 Hz…`c / 2`, minus the larger over roll and pitch of the mean dB in `c`…`2c`; at ≥ 10 dB the cut-off becomes `1.25 · c` rounded to 10 Hz; the text gives the PT1 delay `1000 / (2π c)` ms; confidence = `(headroom − 10) / 10` limited to 0.3–0.9 | `lpf_headroom_db 10`, `lpf_step 1.25` | `analysis/recommend.rs:498` | `quiet_band_above_the_lpf_raises_the_cutoff` | none | OpenPID only | — |

The Advice tab and the Advice line of the Step tab (R16) draw K12–K14: the
proposals with their confidence as a whole percent
(`app/advice_tab.rs:291`) and the findings of every rule.

## Completeness check

The public result structures of `openpid-analysis` and the rows that cover
their fields:

| Structure | Rows |
|---|---|
| `StepMetrics`, `StepOutput`, `StepSpread` (`analysis/step.rs:98`, `:119`, `:153`) | R1–R13, K1 |
| `StepTargets`, `Verdict` (`analysis/targets.rs:11`, `:138`) | R14 |
| `PsdOutput` (`analysis/psd.rs:53`) | S1–S8, K2 |
| `RpmOverlay`, `ThrottleHeatmapOutput` (`analysis/throttle.rs:78`, `:102`) | T1–T8, K3 |
| `StftOutput` (`analysis/stft.rs:53`) | F1–F7, K4 |
| `FilterFit`, `PartResponse`, `ChainResponse`, `FilterSimOutput` (`analysis/filter_sim.rs:319`, `:333`, `:351`, `:471`) | X1–X13, K5 |
| `HarmonicNoise`, `MotorNoise`, `MotorNoiseOutput` (`analysis/motor_noise.rs:61`, `:74`, `:87`) | K6–K8 |
| `RpmEstimateOutput` (`analysis/rpm.rs:71`) | K9–K11 |
| `Finding`, `RecommendOutput` (`analysis/recommend.rs:85`, `:120`) and `Proposal` (`core/recommend.rs:15`) | K12–K14, R16 |
| `Check`, `Fitness` (`analysis/fitness.rs:34`, `:51`) | U1–U10 |

`Prepared` (`analysis/prepare.rs:9`) is rows C1–C3; `Project` and
`ProjectSlot` (`analysis/project.rs`) hold parameters, not results.

## Open points found while writing

Things in OpenPID's own mathematics that look inconsistent, untested or
different from what the documents say. None of them was changed: they are
input for the decisions of [DECISIONS.md](DECISIONS.md). Every point comes
from reading the code, not from running it; the one numeric illustration
(point 1) is a NumPy re-implementation of the formula, not a run of OpenPID.

### The same quantity computed in two ways

1. **The step response depends on how hard the sticks were moved and on the
   logging rate.** The regularisation of the deconvolution is added to
   `|X|²` as an absolute number (`dsp/deconv.rs:103`), although the field is
   documented as "relative to the input power" (`dsp/deconv.rs:15`). `|X|²`
   grows with the square of the stick amplitude and of the sample rate, so
   `reg_high = 1e8` suppresses the band above the cut-off for gentle inputs
   at 1 kHz and hardly at all for hard inputs at 8 kHz. The NumPy
   re-implementation of `dsp/deconv.rs:62`-`:109` on one 2 s window, with the
   same stick pattern through the same linear plant (true overshoot 20.5 %),
   gives an overshoot of 21.2 % (100 deg/s, 1 kHz), 22.1 % (1000 deg/s,
   1 kHz), 22.5 % (100 deg/s, 8 kHz) and 26.5 % (1000 deg/s, 8 kHz), and a
   50 % time of 12 to 14.25 ms; at 40 Hz the factor `|X|² / (|X|² + λ)` was
   0.001 in the first case and 0.76 in the last. The fixture that pins the
   function (`deconvolution_matches_reference`) is the same formula written
   again in Python (`tools/reference/make_fixtures.py:95`-`:144`), so it
   cannot show this.
2. **The time to 50 % has three definitions.** `delay_ms` divides the mean
   curve by its final value (`analysis/step.rs:312`-`:313`, `:330`); the
   spread of the Latency plot takes each window's first sample at ≥ 0.5
   without dividing (`analysis/step.rs:181`-`:185`); the comparison with the
   reference tool takes the first sample of the mean at ≥ 0.5 without
   dividing (`crates/openpid-analysis/tests/real_logs.rs:553`-`:554`). The
   Latency plot draws the point from the first and the bar from the second
   (`app/axis_layout.rs:879`-`:882`). They agree only when the response
   settles at 1.
3. **The Peak plot mixes two statistics, and "peak" and "overshoot"
   normalise differently.** The point is the maximum of the mean curve
   (`app/axis_layout.rs:840`), the bar the standard deviation of the windows'
   own maxima (`analysis/step.rs:177`-`:180`), whose mean (`peak_mean`,
   `analysis/step.rs:186`) is computed and drawn nowhere. The peak is not
   divided by the final value, the overshoot is
   (`analysis/step.rs:318`-`:323`): a curve that settles at 0.9 with a peak
   of 1.05 shows "peak 1.05" and "overshoot 17 %".
4. **Two sample rates.** The analyses use the configured log rate
   (`analysis/prepare.rs:79`), the dock, the "rate" check and the throttle
   coverage use the median step (`core/session.rs:250`; `app/views.rs:638`;
   `analysis/fitness.rs:118`, `:152`). The fixture test of the filter model
   runs it at the median rate
   (`crates/openpid-sim/tests/betaflight_fixtures.rs:37`), Filter Sim at the
   configured one (`analysis/filter_sim.rs:574`, `:597`). When the two rates
   differ, every frequency axis is scaled by their ratio; no test checks
   the frequency of a known tone in a session where they differ.
5. **The GUI and the CLI analyse different parts of the same log by
   default.** The GUI opens a log on the automatic trim (`app/app.rs:147`,
   `:882`), `openpid run` on the whole log (`cli/main.rs:514`-`:530`), and
   the log check always on the whole session
   (`analysis/fitness.rs:268`-`:272`). The step windows of the Step tab, of
   `openpid run step` and of the "sticks" check are three different counts
   for one log.
6. **Two decibel scales for the spectrum of one channel.** The Spectrum tab
   shows `10·log10` of a power density with every segment's mean removed
   (`app/spectrum_tab.rs:403`; `dsp/spectrum.rs:70`-`:76`); the spectrum
   beside each Freq × time row shows `20·log10` of an amplitude without
   removing the mean (`app/time_tab.rs:190`, `:460`;
   `dsp/spectrum.rs:104`-`:126`). The lowest frequency rows of the
   Freq × throttle and Freq × time maps therefore carry the mean rate of
   each segment (the stick input), which the Spectrum tab removes.
7. **A tone reads 6 dB higher in the column below 100 Hz than in the full
   column of the same Spectrum row.** Both columns are densities and share
   one power axis (`app/spectrum_tab.rs:251`, `:328`-`:333`); a density
   reads a tone as `A² / (3 · df)`, and `df` is 4 Hz in one column and 1 Hz
   in the other. `docs/DESIGN.md` §6.1 says the normalisation makes
   "amplitudes comparable between segment lengths"; that holds for
   broadband noise only.
8. **Spectrum smoothing: two of the six levels do nothing on the full
   column, and the two columns are smoothed over different spans.** See note
   S-2 (`app/spectrum_tab.rs:167`-`:183` with `:37`-`:44`). The hint in the
   panel promises an average "over 0, 2, 4, 8, 16 or 32 Hz"
   (`app/spectrum_tab.rs:693`).
9. **The automatic peaks are found on the unsmoothed spectrum and drawn on
   the smoothed curve.** `auto_peaks` reads `PsdOutput::power_db`
   (`app/spectrum_tab.rs:354`-`:355`; `analysis/psd.rs:80`), the curve is
   `smooth_power` of the power (`app/spectrum_tab.rs:399`); the label is
   placed at the smoothed height of the peak's bin
   (`app/spectrum_tab.rs:605`-`:607`). A labelled frequency need not be a
   maximum of the curve on screen, and the prominence threshold is not that
   of the curve on screen.
10. **Two peak pickers with different constants.** Spectrum tab: prominence
    6 dB, at least `2 · df` apart, from 20 Hz (`app/views.rs:170`-`:171`;
    `analysis/psd.rs:83`; `app/spectrum_tab.rs:30`). Advice: prominence
    8 dB, at least 10 Hz apart, from 80 Hz
    (`analysis/recommend.rs:55`-`:56`, `:365`-`:372`). The Advice rule also
    takes the 5 most prominent peaks of the whole spectrum first and drops
    those below 80 Hz afterwards, so five strong low-frequency peaks hide a
    noise peak from it.
11. **The throttle coverage of the log check does not count what the map
    counts.** The check bins the rounded mean throttle and looks ± 3 bins
    (`analysis/fitness.rs:88`, `:133`, `:140`-`:142`); the map takes the
    segments within ± 6 % of the unrounded mean (`analysis/throttle.rs:54`,
    `:201`). The comment says "as the heatmap's throttle window averages
    them" (`analysis/fitness.rs:86`-`:87`). The segment length also differs:
    `round(0.2 · median rate)` logged samples
    (`analysis/fitness.rs:118`-`:119`) against a 2-3-5-smooth number of
    resampled samples at the configured rate (`analysis/throttle.rs:152`).
12. **The thresholds "10 windows / 5 windows" exist three times.**
    `StepTargets::min_windows_ok` / `min_windows_weak`
    (`analysis/targets.rs:87`-`:88`), `STICKS_OK` / `STICKS_WARN`
    (`analysis/fitness.rs:80`-`:81`) and a literal 5 in the overshoot rule
    (`analysis/recommend.rs:223`), which ignores the targets the user set.
13. **The throttle is two different signals, depending on the log.**
    `setpoint[3] / 10`, not limited, when the log has it, else
    `(rcCommand[3] − 1000) / 10` limited to 0–100
    (`bbx/profiles/common.rs:688`-`:712`); the throttle stick always comes
    from `rcCommand[3]` (`bbx/profiles/common.rs:640`-`:656`). The motor
    outputs are not limited to 0–100 % either
    (`bbx/profiles/common.rs:140`-`:148`).
14. **Decimation.** The Log viewer keeps the minimum and maximum of every
    bucket (`app/views.rs:717`); the throttle strip of Freq × time takes
    every n-th sample (`app/time_tab.rs:494`-`:500`), which can miss a short
    throttle blip.
15. **Hop and overlap are rounded three ways.** Truncated in `stft`
    (`analysis/stft.rs:90`) and `psd` (`analysis/psd.rs:122`), rounded in
    `rpm` (`analysis/rpm.rs:150`), an integer half in `motor-noise`
    (`analysis/motor_noise.rs:148`).
16. **Validity is checked differently per analysis.** Step windows need
    95 % valid samples (`analysis/step.rs:422`), throttle-map segments 90 %
    (`analysis/throttle.rs:175`), the spectrogram and the Welch spectrum
    take every filled sample (`analysis/stft.rs:91`; `analysis/psd.rs:112`).
17. **Three copies of "Welch, then dB".** `analysis/psd.rs:120`-`:133`,
    `analysis/motor_noise.rs:246`-`:272` and
    `analysis/filter_sim.rs:613`-`:633`; the first takes the overlap from a
    parameter, the other two fix it. The floors of the dB conversions
    differ: 1e-30 in those three, the smallest positive `f64` in the
    Spectrum tab (`app/spectrum_tab.rs:403`), an amplitude of 1e-9 in
    Freq × time (`app/time_tab.rs:191`), a gain of 1e-9 for the filter
    curves (`analysis/filter_sim.rs:384`) and none for `response_db`
    (`sim/filter.rs:19`-`:21`).
18. **The median is the upper middle value.** `v[len / 2]` of the sorted
    values, for an even count the upper of the two middle ones rather than
    their mean: `analysis/throttle.rs:233`, `analysis/filter_sim.rs:434`,
    `analysis/rpm.rs:177`, `:281`, and `core/session.rs:217` for the median
    step.

### Likely defects

19. **`StepParams::settle_range` is not part of the parameter hash.**
    `impl Hash for StepParams` lists every field but `settle_range`
    (`analysis/step.rs:75`-`:94`), and the GUI identifies the parameters of
    a cached result by that hash alone (`app/worker.rs:77`, `:83`-`:96`).
    Two step jobs on the same log and window that differ only in
    `settle_range` share a cache entry. No control edits the
    field today; a project file can carry it (`analysis/project.rs:67`).
    `RecommendParams` hashes its `step` the same way
    (`analysis/recommend.rs:78`).
20. **The throttle map is darker at its edges and next to unsampled bins.**
    The box filter divides by the full box also where the box leaves the
    grid (`dsp/smooth.rs:29`-`:51`): a uniform map reads 5/9 of its value at
    0 % and at 100 % throttle, and less than its value in the lowest and
    highest frequency rows where the box is wider than one bin. Bins without
    segments stay 0 (`analysis/throttle.rs:195`, `:208`-`:212`) and are
    averaged into their sampled neighbours. The default
    `smooth_throttle = 8` acts as 9 bins (`dsp/smooth.rs:29`), while the
    documentation says 8 (`analysis/throttle.rs:57`; `docs/DESIGN.md` §6.4).
21. **Values at the cursor have no decimals, also in rad/s.**
    `format!("{} {:.0}", …)` (`app/log_tab.rs:839`): with the rate unit set
    to rad/s a gyro of 500 deg/s reads "9" and one of 20 deg/s reads "0".
22. **PID terms are drawn on an axis labelled in the rate unit.** The axis
    rows are labelled "roll (deg/s)" and span ± `y_scale` × the rate scale
    (`app/log_tab.rs:272`, `:288`), while P, I, D, F and the PID sum are in
    raw firmware units and are not scaled (`app/log_tab.rs:509`). With rad/s
    chosen the row spans ± 8.7 and the PID terms, in the hundreds, leave it.
23. **The cursor reads the sample at or after it, the sticks the valid
    sample at or before that one.** `app/log_tab.rs:825` against
    `core/session.rs:592`-`:596`; neither is the nearest sample.
24. **EmuFlight: one low-pass cut-off for three axes.** Where the common
    headers give no gyro low-pass 1, the profile takes
    `gyro_lowpass_hz_roll` as the cut-off of the chain
    (`bbx/profiles/emuflight.rs:44`-`:58`), so Filter Sim on pitch and yaw
    uses roll's.
25. **The step tab's hint describes another algorithm.** "Cut-off: low-pass
    on setpoint and gyro before the deconvolution"
    (`app/axis_layout.rs:268`); the code applies no such filter, the cut-off
    is where the regularisation of note R-1 changes.
26. **The Y-correction guard cannot trigger at the default settings.**
    `tail.abs() > 0.2` (`analysis/step.rs:276`) is tested after the window
    has passed `settle_range`, 0.5 … 1.5 by default
    (`analysis/step.rs:64`, `:272`).

### Untested numbers

27. **Constants no test fixes the value of.** The percentiles of the
    automatic dB range, 0.30 and 0.995 (`app/time_tab.rs:38`-`:39`, `:276`);
    the padding and rounding of the Spectrum power axis
    (`app/spectrum_tab.rs:414`-`:439`); the throttle range of the zoom
    (`app/throttle_tab.rs:227`); the throttle coverage
    (`analysis/fitness.rs:116`); the 6 peaks, the harmonic penalty 0.5 and
    the continuity 8 of the rpm estimate (`analysis/rpm.rs:178`, `:203`,
    `:48`); in the recommender the bands 150–400 Hz and 5–30 Hz
    (`analysis/recommend.rs:274`), the 200 % limit (`:246`), the factors 0.8
    and 1.2 (`:455`, `:466`) and the confidence limits 0.3 and 0.9 (`:144`).
28. **No test fixes an amplitude or a level above the DSP layer.**
    `psd_finds_the_injected_tone` checks the frequency of the tone, not its
    level; `throttle_heatmap_ridge_follows_the_motor_speed` checks where the
    ridge is, not the cell values; the time-tab test checks that the floor
    is below 0 dB. The segment rounding of note S-1, the bin averaging and
    the smoothing of note T-1 and the dB conversions are fixed only through
    `welch_matches_scipy`, `stft_matches_reference` and
    `smoothing_matches_reference`, which test the functions under them.
29. **Smoothing of the step response is not pinned.** The metrics are
    computed on the smoothed curve (`analysis/step.rs:463`-`:464`), the
    histogram and the spread on the unsmoothed windows
    (`analysis/step.rs:472`, `:503`); the width rule
    `((smooth_ms / 1000 · fs) as usize) | 1` (`analysis/step.rs:448`) gives
    11 samples for 10 ms at 1 kHz and 15 at 1562.5 Hz. The only test that
    sets `smooth_ms` checks the length of the result
    (`crates/openpid-analysis/tests/params.rs:188`).
30. **The comparison with the reference tool is loose.**
    `step_response_matches_the_reference_tool` accepts a window count
    between half and twice the reference, a peak within 0.08 and a 50 % time
    within 2 ms (`crates/openpid-analysis/tests/real_logs.rs:555`-`:566`),
    on four curves of two logs. `min_input = 80` and
    `settle_range = (0.5, 1.5)` were chosen to fit those
    (`analysis/step.rs:31`-`:39`).
31. **Time metrics are whole samples.** Rise, delay and settling are sample
    indices × `1000 / fs` (`analysis/step.rs:300`, `:312`): 1 ms steps at
    1 kHz, while the recorded values of the reference tool have tenths
    (22.5, 12.3, 11.7 ms).
32. **Results that are computed and shown nowhere.** `response_db`
    (`analysis/filter_sim.rs:645`-`:646`), `StepSpread::peak_mean` and
    `half_ms_mean` (`analysis/step.rs:186`-`:187`), `windows_total` and
    `windows_rejected` outside an error message
    (`analysis/step.rs:500`-`:502`).

### Code against the documents

33. `docs/DESIGN.md` §6.1: "every `dsp` function has a test against a
    reference generated by `tools/reference/*.py`". `tests/reference/` has
    no fixture for `dsp/peaks.rs` or `dsp/xcorr.rs` (they have analytic
    tests), and the deconvolution fixture is not independent (point 1).
34. `docs/DESIGN.md` §6.2, step 5, gives the smoothing levels as
    "1/20/40/60"; §6.7 and the code have off / 10 / 20 / 40 ms
    (`app/axis_layout.rs:195`-`:200`).
35. `docs/DESIGN.md` §6.2 lists the parameters `resp_len_s` and
    `smoothing`; the fields are `response_s` and `smooth_ms`, and
    `hist_range` and `hist_bins` are not listed
    (`analysis/step.rs:42`-`:53`).
36. `docs/DESIGN.md` §6.2, step 4: "windows with gaps or corruption
    excluded"; the code keeps a window with up to 5 % invalid samples
    (`analysis/step.rs:422`).
37. `docs/DESIGN.md` §6.3: "default 1024 at 4 kHz"; the default is 0.25 s,
    which is 1000 samples at 4 kHz (`analysis/psd.rs:33`, `:111`). It also
    lists motor and debug channels; the tab offers six quantities
    (`app/spectrum_tab.rs:65`-`:72`), the CLI any channel.
38. `docs/DESIGN.md` §6.4, step 1: the amplitude "`|Y|/N·2`"; the code
    divides by the sum of the window, `|X| / Σw · 2`
    (`dsp/spectrum.rs:102`-`:126`), which is twice as much for a Hann
    window.
39. `docs/design/axis-layout/README.md` describes a "mean" and a "peak" of
    the amplitude per panel on Freq × throttle; `app/throttle_tab.rs` has no
    such read-out.
40. `docs/design/spectrum-tab/README.md` names the test
    `spectrum_and_heatmap_controls_change_parameters`; the test is
    `spectrum_controls_change_what_is_drawn`
    (`crates/openpid-app/src/tests/spectra.rs:14`).
41. Doc comments in the wrong place or twice: the comment of
    `lowpass_stage` stands over `rpm_fade` (`sim/model.rs:557`); the
    paragraphs at `dsp/smooth.rs:20`-`:23`, `dsp/resample.rs:35`-`:42` and
    `dsp/fft.rs:90`-`:91` are repeated.
