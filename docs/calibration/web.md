# The web edition 1.0 of the reference tool

Part of #166. On 2026-11-27 the owner gave access to the web edition of the
reference tool (header "version 1.0"). From then on OpenPID is calibrated
against it (decision D-015 = A of [DECISIONS.md](DECISIONS.md)); the rules
found for version 0.94 in the other documents of this folder stay as
history.

The web edition runs in the browser: a log is decoded and analysed on the
page, nothing is sent to a server, and every plot is drawn by a plotting
library whose traces hold the numbers. A run reads those numbers directly,
to the precision of a 32-bit float, instead of vector copies and pictures.
The rules below were found from those numbers on probe logs only; no code
of the reference tool was read.

## Round W1 (2026-11-27): exploration

Four logs of round 17 (`e1-spec-1k`, `e3-step-2k`, `e5-maps-2k`,
`e6-view-2k`), loaded in the owner's browser by a local session; every
window at its defaults, every list, the four smoothing levels of the
spectrum and the five window lengths of the step response. 30 pictures and
110 plots as numbers. All seven windows are open on the owner's account.

### The stretch

Every analysis takes the samples from `fs` to `fs·(T − 1)`, both ends
included (`fs·(T − 2) + 1` samples of a log of `n = fs·T`): time 0 is the
first sample, and one second is left out at each end. Version 0.94 started
two samples later and took two samples fewer.

### What is the same as in version 0.94

| Window | The same |
|---|---|
| Spectrum | the level `10·log10(2·|X|²/(fs·n))` through a symmetric Hann window, 0 Hz and the Nyquist bin undoubled; nothing removed from the signal |
| Freq × time | segments of 600 samples (0.3 s at 2 kHz) through a Hann window, a power density in dB, −30 … 10 dB; which segment each of the first columns holds |
| Freq × throttle | segments of 600 samples every 0.1 s, amplitude `|X|/600`, the throttle of a segment the mean of `setpoint[3]`, the same Gaussian smoothing (15 × 3 cells, σ 4) |
| Filter simulator | every curve of the default set to 10⁻¹³: first-order low-pass, the notch at 8 kHz, magnitude, delay, phase, step |
| Step response | windows a fortieth of their length apart; Peak the highest value of the mean curve before 150 ms (D-003) |
| Main window | the motors as the logged number over 20 (D-008 kept OpenPID's percent); the throttle from `setpoint[3]` |

### What changed

| Window | Web edition 1.0 | Found to |
|---|---|---|
| Spectrum | the stretch above, zero-padded to the next power of two (32 768 points for 24 001 samples); smoothing a moving average of the dB values over 51, 151, 301 and 601 points ("Low", "Medium", "High", "Max"), over the points that exist at the ends; four levels where 0.94 had five | all 24 curves to 2.4·10⁻⁶ dB |
| Freq × time | segments zero-padded to 1 024 points (513 rows); 0.03 s between columns at the default "Max" of its time list (0.94: 0.15 s); "Medium" of its frequency list a moving average of 5 rows of dB, cut at both ends | every cell to the 0.0005 dB of the file |
| Freq × throttle | segments zero-padded to 1 024; the column of throttle `j` averages the segments within 1.5 % of `j + 0.5` (0.94: (T − 1, T + 1]); the last segment that fits is kept; mean and peak printed over rows 50 … 511 (52 … 512 fits as well) | six maps to 5·10⁻⁶ and the twelve printed numbers to their digits |
| Filter simulator | the delays printed at 25 Hz (0.94: at the lowest frequency, 1 Hz): 0.6339 ms for a first-order low-pass at 250 Hz where 0.94 printed 0.63662 | every printed number |
| Main window | time from the first sample; the smoothing list "Off, 2, 5, 10, 20" | — |
| Spectrum, a new column | "Motor Noise (dB)" at the first three motor harmonics, ±20 Hz, from 0.3 s segments every 0.15 s and an estimate of the motor speed | not identified |

### Not settled

- **Step response**: the web edition keeps other windows than 0.94 (469,
  462, 460 at 2 s against 476, 471, 465), at 1 s more of them; its mean
  curve differs by 0.003 … 0.006 at most and Peak by up to 0.0016. Which
  windows it keeps, and how a window's response is computed exactly, were
  not found from the mean curves alone.
- The motor noise column, the short-log rule, other log rates and the other
  smoothing levels of both maps were not asked yet.

### OpenPID against it

OpenPID as decided for 0.94 differs from the web edition by 0.23 … 0.35 dB
in the median of a spectrum (up to 1.6 dB below 100 Hz, 2 … 3 dB over the
full range), 0.27 dB on the map against time (tones 1.1 dB lower without the
padding), 1.5 … 2 % on the printed numbers of the throttle map, and in the
filter simulator's printed delays. Decision D-015 moves OpenPID to the web
edition's rules.

## Round W2 (2026-11-27): the rules

Sixteen logs of earlier rounds (spectrum at 1, 2, 4 and 8 kHz, logs of 10
and 11 s, the step response plants of round 4 and the windows of round 18,
both maps at every level of their lists, a log of 61 s, every channel of
`i1-chan-2k`, a log with motor speeds), the filter simulator at five
settings and the main window's smoothing on one-sample pulses. 114
pictures and 820 plots as numbers; the step response with every accepted
window's trace ("Raw"). Every rule below reproduces the web edition's
numbers to the precision of a 32-bit float unless it says otherwise.

### The stretch

The samples `i` with `In ≤ i/fs ≤ Out`, time counted from the first sample:
from `ceil(In·fs)` to `floor(Out·fs)`. By default `In` = 1 s and
`Out` = T − 1 s: samples `fs … fs·(T − 1)`. The same at 1, 2, 4 and 8 kHz
and on logs of 10 and 11 s: there is no rule that takes a short log whole.
Moving In and Out moves every analysis at once.

### Step response

- Windows of `W = window·fs` samples, `W/40` apart, from the stretch's
  first sample, while a window fits.
- Per window: setpoint and filtered gyro through a symmetric Hann window,
  200 zeros after them (`M = W + 200`), **zero-padded further to the next
  power of two**; both transforms over `M`; `H = Y·conj(X)/(|X|² + 0.0001)`;
  back to time, the running sum, 0 … 500 ms. Only the padding differs from
  0.94.
- A window counts when the mean of `|setpoint|` is at least 20 deg/s, its
  response stays within 1 ± (SS width)/2 from 150 ms to 500 ms, and
  ("Dist. to median", on by default) the mean of its response from 150 to
  500 ms lies within 3 scaled median absolute deviations (×1.4826) of the
  median over the windows that passed the first two tests. All 24 window
  counts of the round agree.
- The curve is the mean of those windows. Peak is its highest value before
  150 ms; the bar is the standard deviation of each window's highest value
  in its first **150 ms** (0.94: 150 samples). The latency is read per
  window at 0.5, as in 0.94.

### Spectrum

The transform length is the next power of two of the stretch's sample
count, at every rate and length; the moving average spans 51, 151, 301 or
601 **points** whatever the point spacing (a width in hertz is 4 … 12 dB
off on the 10 s log). The level as in W1.

### Freq × time

- "Time smoothing" 1, 2, 5, 10 (Low … Max): segments of 600 samples
  `600/F` samples apart (0.3, 0.15, 0.06, 0.03 s at 2 kHz). The first `F`
  columns hold the segment that starts at their time, the later ones the
  segment centred at it, at every level.
- "Freq smoothing" 2, 5, 10, 20: a centred moving average of the dB rows
  over 3, 5, 11, 21 rows, cut at the ends.
- Its channels: Gyro (filt) `gyroADC`, Gyro (unfilt) `gyroUnfilt`, D term
  `axisD`, P term, I term, Setpoint, RC cmd (the logged `rcCommand`), PID
  error (setpoint − filtered gyro), PID sum (P + I + D + F), and D term raw:
  minus the change of the filtered gyro from one sample to the next times a
  factor that was not found.

### Freq × throttle

- 600-sample segments zero-padded to 1 024, 0.06 s apart for a stretch of
  20 s or less and 0.1 s up to 60 s (above 60 s not asked), the last one
  that fits kept; the throttle of a segment the mean of `setpoint[3]`/10.
- **The column of throttle `j` (drawn at `j + 0.5` %) holds the segments
  with `j − 1 ≤ T < j + 2`**: three percent, half open.
- Smoothing level `n` (1 … 5, Low … High): a Gaussian of `n` rows by `5n`
  columns, σ 4, as in 0.94.
- "PSD": the mean of the segments' density in dB (0 Hz and Nyquist halved),
  −40 dB for an empty column.
- The printed mean and peak: over the rows of 100 Hz and up; with
  "< 100 Hz" on, over the rows between the two frequency limits.

### Filter simulator

As in 0.94, at the chosen loop rate, with the delays printed at the chosen
frequency (25 Hz by default, 1 Hz on request). The RPM notches fade:
weight `min(1, (H1 − min)/fade)` times the harmonic's weight, the notch
acting as `(1 − w) + w·H`; below the minimum frequency there are no RPM
notches at all. Every one of the 89 printed delays agrees.

### Main window

Smoothing 2, 5, 10, 20: a centred moving average over 3, 5, 11, 21
samples, cut at the log's ends. PID error = setpoint − filtered gyro (as
OpenPID shows it: the difference of D-009 is gone). Motors as the logged
number over 20 (D-008 keeps OpenPID's percent). Motor speeds from `eRPM`:
`eRPM·100/(poles/2)/60` Hz.

### Not settled after W2

- The motor noise column's level per segment (its segments, 0.3 s every
  0.15 s, and their counts are found), and the spectrum's "Filt. delay"
  column.
- The factor of "D term raw".
- "Max start" of the step response (never active on these logs) and the
  step response's smoothing levels.
- The throttle map above 60 s and at other log rates; a log whose length is
  not a whole number of seconds.

