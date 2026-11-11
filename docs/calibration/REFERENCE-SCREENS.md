# The reference tool, screen by screen (round 0)

Part of #167 (epic #166).

What the reference tool (Pro v0.9, on Windows) shows for a written log, taken
from the owner's 38 screenshots of round 0 (2026-11-11). It is the reference
side of [INVENTORY.md](INVENTORY.md): where a row there says "not recorded —
round 0", the screen and the controls are described here. Only what a
screenshot shows is written down; the values marked "≈" were read by eye
from the plots and are not yet measurements. The screenshots stay with the
issue; nothing of the tool is in the repository.

The log was one synthetic 30 s flight (`openpid gen`, #168) at 1, 2 and
4 kHz: stick steps on roll through an underdamped plant with a 5 ms delay,
a 150 Hz tone of 20 deg/s on every axis, white noise, motor harmonics that
follow a throttle sine of 15 … 85 %, one PT1 at 150 Hz on the gyro.

## What it opens

| Input | Result |
|---|---|
| `.bbl` written by `openpid_fmt_blackbox::write` (Betaflight 2025.12.2 headers) | opens at 1, 2 and 4 kHz; it decodes the file itself and from then on names it `<name>.01.csv` |
| a CSV made by another decoder (`openpid decode`) | refused: it reads only the CSV of its own decoding step |

So a probe is always a `.bbl`; the fallback CSV writer of #168 is not needed.

Every header we wrote is listed in its Setup Info. The eRPM fields are read
(the motor-harmonic curves of the spectrum are drawn from them). The channels
we wrote as zero show as zero or empty: P, I, D and F terms, the D term
before its filters, PID sum, debug. That was the content of the smoke log,
not a fault of the file: a probe that needs one of these screens has to
carry a signal in those fields.

## Log Viewer (the main window)

- Three rate rows (roll, pitch, yaw, "deg/s", `y scale` 500 → ± 500) and one
  row "Throttle | Motor (%)", 0 … 100, over the time in seconds from the
  first frame.
- **The first and the last second are greyed out** and left out of every
  analysis (as recorded in DESIGN §6.7).
- Check-list of what is drawn: Gyro(pf), Gyro, P-term, I-term, D-term(pf),
  D-term, F-term, Set point, PID sum, PID error, Motor 1/5 … 4/8, RPS 1/5 …
  4/8, Throttle, Test Signal, Debug. On by default: Gyro, Set point, the four
  motors, Throttle.
- Panel: Open File, Reset, the file list, R / P / Y, Period, Markup, line
  colour, line smoothing, line width, Spectral Analyzer, Step Resp Tool, PID
  slider tool, Filter Sim, "Test signal = Gyro(prefilt)" with a filter type
  (`pt1`) and a cut-off (250), Setup Info, Save Fig, Save Settings, the
  theme. Two stick boxes under the panel.
- **Throttle** is drawn 15 … 85 %, as written. **The motors read higher than
  the throttle**: ≈ 22 … 88 % for outputs that are 15 … 85 % of the range
  between the `motorOutput` limits (158 … 2047). That fits the raw output
  over 2047, not the range between the limits (OpenPID's reading). To be
  measured in #175.
- **Flight mode**: the slow-frame flags value 1 (ARM in the firmware's box
  order) is labelled `ANGLE_MODE` under the plot.

## Setup Info

One column per loaded log with every header as `name:value`, in the log's
order; "Show Differences Only"; a selector of the log.

## Spectral Analyzer

- Two columns per axis: "Full Spectrum" (0 … half the log rate: 500, 1000,
  2000 Hz for the three logs) and "Sub 100Hz". Y: "Pwr. Spec. Density (dB)",
  −50 … 20 by default (`Y min`, `Y max`).
- File list (up to 10 logs at once, one colour each) and a channel list:
  Gyro, Gyro prefilt, Dterm, Dterm prefilt, Pterm, PID error, Set point,
  Fterm, PIDsum, Motors, Test Signal, debug. Default: Gyro (solid) and Gyro
  prefilt (dotted).
- Controls: Run, Reset, Freq x Throttle, Freq x Time, Save Fig, Save
  Settings, a "Delay:" list, a smoothing list, R / P / Y, PSD, Markup, the
  four motors, "RPM Notches" (1st, 2nd, 3rd harmonic and their pairs and all
  three), "Dyn Notches", line width, "sub 100Hz", a multiplier (2.1) and
  "RPM est.".
- **The noise floor depends on the log rate as a density does**: the same
  white noise reads ≈ −27 dB at 1 kHz, ≈ −30 dB at 2 kHz and ≈ −33 dB at
  4 kHz (pitch, unfiltered, below the motor band). OpenPID reads −23.8 dB
  for the 2 kHz log: **about 6 dB higher**.
- **The steady 150 Hz tone does not stand out at the default smoothing.**
  OpenPID shows it 37 dB above the floor (13.8 dB at 148 and 152 Hz); the
  reference tool's curve has no line there, only the band of the motor
  harmonics (≈ −17 … −22 dB from 105 to 365 Hz, with a ripple). The same
  tone is the brightest line of its Freq x Throttle and Freq x Time
  screens, so it is in the data; what the curve does to a narrow line (its
  smoothing, or how it averages) is the first question of round 1.
- With RPM Notches on, each motor's harmonics are drawn over the spectra as
  curves from the eRPM.

## Freq x Throttle

- Four columns, each with its own channel and log selectors and a "<100Hz"
  switch; three rows (roll, pitch, yaw). X: "% Throttle" 0 … 100; Y: 0 … half
  the log rate, or 0 … 100 Hz.
- Presets: 1 Gyro prefilt | Gyro | Dterm prefilt | Dterm (the default); Gyro
  prefilt | Gyro | Pterm | Dterm; Gyro | Dterm | Set point | PID error; Gyro
  prefilt | Gyro and the same two below 100 Hz; Dterm prefilt | Dterm twice;
  and four that put one channel (Gyro prefilt, Gyro, Dterm, PID error) in all
  four columns, for four logs side by side ("A|B|C|D"). A channel can also be
  "NONE".
- Colour map "hot"; a `scale` box per column, 0.5 by default, with the
  colour bar 0 … 0.5 (linear). Smoothing: low, low-med, medium (default),
  med-high, high. "freq lims Hz" 20 and 60.
- **Each panel prints "mean=" and "peak="**: for the 2 kHz log 0.0689 and
  3.35 (Gyro prefilt, roll), 0.0319 and 2.15 (Gyro). The peak is the 150 Hz
  tone; 2.15 / 3.35 = 0.64 is the gain of the PT1 at 150 Hz, so the linear
  map is proportional to amplitude.
- **"PSD" switches the map to decibels**: colour bar −40 … 10, scale 10; the
  same panels then print mean −32.2, peak 7.8 and mean −39, peak 3.92. The
  two peaks differ by 3.88 dB = 20·log10(0.64), and both are
  20·log10(linear peak) − 2.7 dB.
- **"2D" replaces the maps by line plots** of "Amplitude" against frequency,
  0 … 0.5 by default, not smoothed: there the 150 Hz tone is a sharp line
  (≈ 0.28 on PID error for the filtered tone of 12.8 deg/s).
- The map has energy from ≈ 8 to ≈ 92 % throttle for a flight between 15
  and 85 %: the smoothing spreads it by several percent.

## Freq x Time

- One log and one channel (default Gyro prefilt); three rows; X: seconds
  1 … 29 (the trimmed log); Y: 0 … half the log rate, "< 100Hz" as an option.
- "Power Spectral density (dB)" with Z min −30 and Z max 10; colour map
  "hot"; smoothing in frequency and in time as two lists; the motors' RPM
  and dynamic-notch overlays as on the spectrum.
- The 150 Hz tone is a line at ≈ 0 … 5 dB, the first motor harmonic a ridge
  at ≈ −12 dB, the floor ≈ −28 dB.

## Step Resp Tool

- Left: "Step Response Functions", one row per axis, 0 … 500 ms, Y 0 … 1.5
  (`Y max`). The legend names "P, I, D, Dm, F" and prints the gains of each
  log with the number of windows: `45,80,30,40,120 (n=258)`. An axis without
  stick input says "insufficient data".
- Right: "Peak" and "Latency (ms)" per axis, one point with an error bar per
  log.
- Controls: Run, Reset, Save Fig, Save Settings, Period, Markup, a smoothing
  list, the latency list (SR Latency, SP-Gyro …), R / P / Y, Expand, Raw, BF
  sliders, Y corr.
- "Raw" draws every window's response over the mean. "BF sliders" replaces
  the right half by "Betaflight PID Sliders": Damping, Tracking, Stick Resp.,
  Dynamic D., Drift-Wobble, Pitch D., Pitch T., Master on 0 … 2, with "MODE:
  RPY".
- Readings for the roll axis. The plant has a damping of 0.5 (an overshoot
  of 16.3 % in continuous time) and a 5 ms delay; the gyro then passes a PT1
  at 150 Hz. The simulator integrates the plant step by step, so the true
  curve of these logs is not known exactly; the probes of #172 are.

  | Log | Reference tool: windows / peak / SR latency | OpenPID (`run step --epoch 1..29`): windows / peak of the mean / time to 50 % |
  |---|---|---|
  | 1 kHz | 258 / ≈ 1.15 / ≈ 15.4 ms | 209 / 1.212 (settles at 1.035) / 15.0 ms |
  | 2 kHz | 258 / ≈ 1.155 / ≈ 15.3 ms | 209 / 1.214 (settles at 1.031) / 15.5 ms |
  | 4 kHz | 255 / ≈ 1.175 / ≈ 15.2 ms | 208 / 1.200 (settles at 1.019) / 15.75 ms |

  The reference tool's "SP-Gyro" delay for the 2 kHz log is ≈ 15.8 ms;
  OpenPID's cross-correlation latency is 15.8 ms.

  - 258 windows in 28 s is one window every ≈ 0.1 s; OpenPID's step between
    windows is 0.125 s.
  - **OpenPID's peak is ≈ 0.03 … 0.06 higher**, and its mean curve settles
    2 … 3.5 % above 1 where the reference tool's settles on 1.
  - OpenPID's time to 50 % moves by 0.75 ms between 1 and 4 kHz (whole
    samples); the reference tool's by 0.2 ms.

## Filter Sim

Low-pass filters (two, type and cut-off each) and notch filters (RPM
harmonics, a static notch with Q, centre and cut-off) as four rows of plots:
magnitude, "Filter Delay (ms)", "Phase Delay (deg)" and the step response.
The titles carry the delays ("LOWPASS FILTERS | Delay 0.95492ms", "NOTCH
FILTERS | Delay 2.3933ms") and the panel the total ("Filter Delay Total
3.3483ms"). Controls: loop rate (8k), number of motors, the filter types and
values, "Step resp." / "Add noise" / "Show Both" lists, the signal's start
and end frequency and its duration, "combine lpf", "Log Frequency",
"Magnitude dB". It is a calculator of the filters typed into it; it does not
use the log.

## Not captured in round 0

- The open "smoothing" and "Delay:" lists of the Spectral Analyzer and the
  smoothing list of the Step Resp Tool (their options and defaults).
- The PID slider tool as its own window.
- "Period", "Markup", "Expand", "Y corr." and "RPM est." in use.
