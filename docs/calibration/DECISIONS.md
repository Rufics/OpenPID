# Calibration decisions

Part of #167 (epic #166).

Every disagreement between the reference tool, OpenPID and the exact value
is recorded here, one entry each. **OpenPID's behaviour changes only after
the owner has chosen**: an entry without a decision changes no code.

An entry compares three things for one synthetic probe:

- what **the reference tool** shows, as read from the owner's screenshots
  and copies (neither they nor the numbers read from them are committed:
  what a round found is written in these documents);
- what **OpenPID** shows for the same probe;
- **the exact value**, which is known because the probe is synthetic.

The quantities are the rows of [INVENTORY.md](INVENTORY.md); the rounds that
produce the evidence are in [ROUNDS.md](ROUNDS.md).

## Process decisions

Taken by the owner on 2026-11-11:

1. The probe tool is a workspace crate, `crates/openpid-probe`, not a Python
   tool.
2. A pack reaches the owner as a zip and is reproducible from the
   repository.
3. Screenshots of the reference tool are not committed. Since 2026-11-13
   neither are the numbers read from them, nor any other material of a
   round: the repository holds the program's code and its tests, and the
   comparison of this epic is done once. What a round found is written in
   these documents; what it changes in OpenPID arrives as a pull request
   of its own from `master`, with tests that need no data of a round.
4. The order of the areas: Spectrum, Step response, Freq × throttle,
   Freq × time, the rest.

## How an entry is written

- The id is `D-001`, `D-002`, … in the order the entries are opened; an id
  is not reused.
- "The exact value" is what the probe was built to give, with its
  derivation in one line. Where no exact value exists (a display choice,
  such as a default colour range), the field says so.
- An entry is finished when it names the owner's decision with its date and
  the pull request that carried it out.

## Entry template

```markdown
### D-000 Short title

| Field | |
|---|---|
| Id | D-000 |
| Area | the area and the row of INVENTORY.md, for example "Spectrum, S1" |
| Probe | the probe (name, round, pack commit) and what it contains |
| The reference tool shows | the number or curve read from the screenshot or the copy |
| OpenPID shows | the number or curve for the same probe, with the test or command that gives it |
| The exact value | the value the probe was built to give and why; or "none: a display choice" |
| Options | A. … B. … C. … (what a user would see with each) |
| Recommendation | the option proposed and the reason |
| The owner's decision | the option chosen, the date (YYYY-MM-DD) |
| Carried out by | the pull request |
```

## Register

### D-001 What the Spectrum curve is

| Field | |
|---|---|
| Id | D-001 |
| Area | Spectrum, S1, S2, S4 |
| Probe | the nine probes of round 1 (pack `a82756a`); the numbers below are of `s1-levels-2k`, `s2-levels-1k`, `s3-levels-4k`: tones of 3, 10 and 30 deg/s over a floor of 1/3 (deg/s)² spread over half the log rate |
| The reference tool shows | at its default "medium": a floor of −38.6 / −41.6 / −44.6 dB at 1 / 2 / 4 kHz; the 30 deg/s tone as a bump of 5.7 dB, the 3 deg/s tone as one of 1.7 dB. It computes one Hann-windowed transform of the whole log, `10·log10(2·\|X\|² / (fs·N))`, and smooths the decibel values by a local regression whose window is `L³·log10(points)` for level L = 1 … 5, so 5 … 555 points for a 30 s log at 2 kHz ([spectrum.md](spectrum.md)); the same level is a wider band in hertz for a short log than for a long one. A model built this way is within a pixel of all 45 pictures of round 1, gives the curves of round 2, read from vector copies, to 0.00002 dB, and gave the flown flight of round 3 (the hold-out: four logs, 72 plots) with nothing changed. In this curve noise reads 6.8 dB below its density; a steady tone of 20 deg/s is a bump of under 4 dB, the same tone switched on for half the log a peak of 40 dB |
| OpenPID shows | a floor of −31.8 / −34.8 / −37.8 dB; the 30 deg/s tone 53.5 dB above it. Welch: 0.25 s segments, the mean of their powers, the window's loss put back, smoothing of the power (`our_spectrum_is_a_power_density` in `crates/openpid-probe`) |
| The exact value | the floor: `10·log10((0.25 + 1/12) / (fs/2))` = −31.8 / −34.8 / −37.8 dB, the density of the noise. A tone has no density: its height depends on the resolution in any spectrum |
| Options | **A.** The reference tool's curve by default (one transform, its level, decibel smoothing with its five levels), and OpenPID's density as a second mode. A user sees the same numbers as in the reference tool and its tutorials; narrow lines are hidden unless the mode is switched. **B.** OpenPID's density by default, the reference tool's curve as a second mode. Noise reads 6.8 dB higher than in the reference tool, tones stand out; numbers from tutorials do not carry over unless the mode is switched. **C.** OpenPID's curve only, the offset documented. |
| Recommendation | A: the epic's aim is that a user who knows the reference tool reads the same numbers. The density stays one switch away because it is the exact quantity, the only one the probes can check to a tenth of a decibel, and the one that shows a steady resonance. The other six probes of round 1 are in and do not change it. They add a caution for either order of the two modes: in the reference tool's curve the height of a peak follows its width, so a fixed resonance is shown weakly and anything that changes during the flight strongly |
| The owner's decision | **A**, 2026-11-11: the reference tool's curve by default, OpenPID's density as a second mode |
| Carried out by | #187 (on its own from `master`; it replaces #181, which stood on the branches of the test tools) |

### D-002 What the Step response curve is

| Field | |
|---|---|
| Id | D-002 |
| Area | Step response: the mean curve, its peak, the time to 50 %, the number of windows |
| Probe | `t1-plants-2k`, `t2-plants-1k`, `t3-plants-4k`, `t9-short-2k` of round 4 (pack `dad54b3`): random stick moves through a pure delay of 10 ms, a first-order plant (20 ms after 5 ms) and a second-order plant (15 Hz, damping 0.4, after 5 ms), whose step responses are known in closed form |
| The reference tool shows | the true response to a few thousandths: a peak of 0.9998 on the delay and 1.2511 … 1.2525 on the second-order plant at 1, 2 and 4 kHz; the half-way time within 0.08 ms on the two smooth plants; 256 … 259 windows for 30 s ([step.md](step.md)). How it computes them is identified on this round (2026-11-12): windows of 2 s every 0.1 s, the gyro's transform divided by the setpoint's after a Hann window, 200 zeros and a constant of 0.0001, the running sum of the result; a window counts when the stick was 20 deg/s from its centre on average and the response stays between 0.5 and 1.5 after 150 ms. A model built this way gives all 45 mean curves and 5 610 single windows of the round to 0.0000004 |
| OpenPID shows | the same bias on every log: a peak of 1.06 on the pure delay, 1.29 … 1.30 on the second-order plant; the half-way time 0.5 ms early on the second-order plant and 1 ms late on the first-order one; 207 … 209 windows (`openpid run step`) |
| The exact value | the delay: no overshoot, a step at 10 ms. The second-order plant: a peak of 1.2538 (`exp(−π·ζ/√(1−ζ²))` over 1), half way at 18.110 ms. The first-order plant: no overshoot, half way at 18.863 ms |
| Options | **A.** OpenPID computes the response as the reference tool does (identified on round 4, to be confirmed on rounds 5 and 6): the same curve, peak and window count as the reference tool, and on these probes the truth. **B.** OpenPID keeps its own estimator and it is corrected against the closed-form responses (its smoothing of the spectrum and its regularisation are what bias it); numbers stay close to the reference tool's without being the same. **C.** No change; the bias is documented. |
| Recommendation | A, if the identified method stays as close to the truth on the confirming and the hold-out rounds as it is here: then matching the reference tool and being right are the same change. To be put to the owner when the method is identified |
| The owner's decision | **A**, 2026-11-12: OpenPID computes the response as the reference tool does. Decided before rounds 5 and 6: what they correct in the rule is corrected in OpenPID with it |
| Carried out by | #188 (on its own from `master`; it replaces #184), with the Peak of round 5 and the distance between windows of version 0.94 (0.05 s; round 6) |

### D-003 Where the Peak of a step response is read

| Field | |
|---|---|
| Id | D-003 |
| Area | Step response: the Peak plot and its bar |
| Probe | `u4-slow-2k` of round 5 (pack `407c2cd`): second-order plants of 2.5 Hz and of 4 Hz, damping 0.5, after 5 ms; and the plants of 15 Hz of rounds 4 and 5 at 1, 2 and 4 kHz |
| The reference tool shows | Peak 0.953 for the plant of 2.5 Hz: the value of its curve just before 150 ms, where it stops looking; the curve it draws goes on to 1.122 at 226 ms. Peak 1.137 for the plant of 4 Hz, which tops before 150 ms. The bar is the spread over the windows of each window's highest value among its first 150 samples, whatever the log rate: 150 ms at 1 kHz, 75 ms at 2 kHz, 37.5 ms at 4 kHz, where it ends before the top of a response that peaks at 40 ms |
| OpenPID shows | the same, since D-002 (#188), until #193. Version 0.94 of the reference tool reads both as 0.9 did (round 6, at 8 kHz too, where the 150 samples are 19 ms) |
| The exact value | both plants overshoot to 1.163, at 236 and at 149 ms |
| Options | **A.** As the reference tool (what OpenPID does now): a craft whose response tops after 150 ms shows a Peak below the overshoot its own curve draws, and the bar covers a stretch that depends on the log rate. **B.** Peak is the highest value of the whole curve (500 ms) and the bar the spread of the windows' highest values over the same stretch: 1.122 for the plant of 2.5 Hz (the method itself reads a slow plant low; the truth is 1.163); the same point as now for every response that tops before 150 ms; the bar differs from the reference tool's at every rate. **C.** The point as the reference tool, the bar over 150 ms instead of 150 samples: the reference tool's Peak, and a bar that means the same at every log rate |
| Recommendation | B. The number is called Peak and is read as the overshoot; on a slow craft the reference tool's reading is not the overshoot, and its bar is over a stretch nobody chose. The point changes on slow crafts only, where the reference tool's value misleads |
| The owner's decision | **B**, 2026-11-13: the Peak is the highest value of the whole curve, its bar the spread of the windows' highest values over the same 500 ms |
| Carried out by | #193 (on top of #188, where the code it changes is) |

### D-004 The performance numbers of a step response

| Field | |
|---|---|
| Id | D-004 |
| Area | Step response: rise time, bandwidth, sensitivity peak and its frequency, phase margin, crossover frequency (new in version 0.94 of the reference tool; the owner decided on 2026-11-13 that OpenPID shows them) |
| Probe | the eight logs of round 7 (pack `7066f96`) and the five of round 6: plants with analytic answers |
| The reference tool shows | six numbers per axis, read off the mean curve after it is brought to a gain of 1 (the rise time excepted). Its arithmetic for five of them is not found: some 2 000 plain definitions come near and none is exact. It reads the phase margin at a grid point of 0.977 Hz, so the same plant shows 32.5° … 35.4° at different log rates |
| OpenPID shows | until #194 the rise time only (10 % → 90 % of where the mean curve settles) |
| The exact value | known for every plant of round 7. The tool is within 0.10 Hz (bandwidth), 0.06 dB (sensitivity), 1.0° (margin) and 0.19 Hz (crossover) of it in the median, and up to 1.0 Hz, 0.27 dB, 9.2° and 0.57 Hz off on slow or sharp responses |
| Options | **A.** A plain, documented computation on the same mean curve: differences of the curve over its last value, the tool's frequency grid, bandwidth at −3 dB, the highest `\|1 − T\|`, crossover and margin of `T / (1 − T)` with the phase read between grid points. Within 0.13 Hz, 0.03 dB, 0.03 Hz and 1.4° of the tool in the median, and as near the truth as the tool or nearer. A user comparing the two programs sees the same numbers to the first decimal on an ordinary craft. **B.** The same, but the phase margin read at the grid point as the tool reads it: within 0.3° of the tool in the median, and its jump of up to 3° between log rates with it. **C.** Wait until the tool's arithmetic is found exactly (more rounds, with no promise that it can be), and show nothing until then |
| Recommendation | A. The numbers are engineering estimates with a spread larger than the difference between the two programs; A has a definition one can write down and test against the analytic plants, and no artefact of the grid. If the tool's arithmetic is found later, the difference is a small change |
| The owner's decision | **A**, 2026-11-13: the plain, documented computation, with the phase margin read between grid points |
| Carried out by | #194 (on top of #188) |

### D-005 How the map of frequency against throttle is computed

| Field | |
|---|---|
| Id | D-005 |
| Area | Freq × throttle, T1–T4: the cells of the map, its smoothing, its colour scale, the numbers printed on it |
| Probe | the eight logs of round 8 (pack `7038aac`): a throttle that climbs stairs, rises evenly, is held and climbs stairs a few tenths of a percent apart, with tones and noise of known size on both gyro channels |
| The reference tool shows | a map whose rule was found and holds on 156 copied maps to one colour step ([throttle.md](throttle.md)): segments of 0.3 s that overlap; a tone of amplitude `A` reads `A / 4`; 100 columns, each the mean of the segments within ±1 % of throttle; a Gaussian over 15 × 3 cells by default, with five levels to choose from; a second map in decibels; "mean=" and "peak=" printed on every map; the colour scale 0 … 0.5 unless typed. For a tone of 10 deg/s at a held throttle: a peak of 0.35 in a spot 16 columns wide; white noise of 5 deg/s RMS: a mean of 0.064 |
| OpenPID shows | another quantity on another grid: segments of 0.2 s side by side; a tone reads `A`; 101 columns, each the mean of the segments within ±6 %; a fixed box of 9 columns; no decibels, no printed numbers; the colour scale from the map itself. The same tone: a peak of 4.03 in a spot 21 columns wide; the same noise: about 0.5 |
| The exact value | the tone has an amplitude of 10 deg/s at one throttle and one frequency; neither map shows that number, and neither claims to: both smooth it over cells. The reference tool's is a quarter of the amplitude before smoothing because nothing makes up for its window |
| Options | **A.** As the reference tool, all of it: its segments, columns, value (`A / 4`), the five smoothing levels with "medium" as the default, the map in decibels as a second view, mean and peak printed on the map, and its colour scale of 0 … 0.5 as the default with OpenPID's automatic scale as a choice. A user sees the picture and the numbers the reference tool shows, and can compare a log in both; the user guide says that a cell is not a tone's amplitude. **B.** Keep OpenPID's map as it is and say in the user guide how it differs. Nothing to compare with the reference tool: neither the picture nor a number. **C.** The reference tool's grid and smoothing, but a cell multiplied by 4 so that a tone on a bin reads its amplitude before smoothing; the decibel view and the printed numbers follow. The picture is the reference tool's with a colour scale four times as long; its printed mean and peak are four times the tool's |
| Recommendation | A. The printed mean and peak are what people quote when they compare noise between flights and between crafts, and they are only worth quoting if they are the tool's numbers; after the smoothing a cell is not an amplitude in either unit, so the factor of C buys little. Two parts of the rule are not settled and need round 9 before the code: the distance between segments in logs of other lengths, and which throttle the tool reads. The RPM lines and the notch bands OpenPID draws over the map (T5, T6) stay |
| The owner's decision | **A**, 2026-11-14: as the reference tool, all of it |
| Carried out by | #195 (on its own from `master`). Round 9 settled which throttle is read (not the motors) and gave the distance between segments for seven lengths of stretch; round 10 put the two places where it changes inside (19, 21.25] and (57.25, 64] s, and 20 and 60 s are the round numbers there |

### D-006 How the map of frequency against time is computed

| Field | |
|---|---|
| Id | D-006 |
| Area | Freq × time, F1 and F2: the cells of the map, its smoothing, its colour scale |
| Probe | the four logs of round 10 (pack `a022b4d`): tones that come and go, a tone that climbs, a tone of 300 Hz at 1 … 40 deg/s, close pairs of tones and noise of known density, on both gyro channels, at 2 and 4 kHz and in a flight of 16.5 s |
| The reference tool shows | a map whose rule was found and holds on 30 copied maps, a million cells of them to the colour step ([time.md](time.md)): segments of 0.3 s every 0.15 s, without the first and the last second of the log; a power density in decibels, in which a tone of amplitude `A` reads `20·log10(A) − 14.3` dB; along the frequency a line through five rows, fitted to the decibels, after which the tone reads `20·log10(A) − 17.7` dB; two lists of four smoothing levels, one for each axis; colours from −30 to 10 dB unless typed. The tone of 300 Hz at 10 deg/s: 2.3 dB |
| OpenPID shows | another quantity on another grid: segments of 0.2 s every 0.05 s over the whole log; an amplitude shown as `20·log10(A)`; no smoothing; rows to 1 000 Hz; the colour range from the map itself (its 30th and 99.5th percentile) unless typed. The same tone: 20.0 dB |
| The exact value | the tone has an amplitude of 10 deg/s, which is 20 dB re 1 deg/s: OpenPID's number. The reference tool's 2.3 dB is not wrong either: it is a density per Hz, lowered by its window and its smoothing, and it does not claim to be the amplitude |
| Options | **A.** As the reference tool: its segments, its decibels, its smoothing along the frequency with the four levels of each list ("low" as the default), its colour scale of −30 … 10 dB as the default with OpenPID's automatic range as a choice. A user sees the picture and the levels the reference tool shows and can compare a log in both; the user guide says that a cell is a density, not an amplitude. Not copied: the reference tool draws its second column twice, which puts its first two columns 0.15 s early; OpenPID keeps every column at the middle of its segment, where the tool has all the others. **B.** Keep OpenPID's map and say in the user guide how it differs. Nothing to compare with the reference tool: the levels are 17.7 dB apart on a tone and the pictures differ in grain. **C.** The reference tool's segments and smoothing, in OpenPID's unit (14.3 dB more, so that a tone on a bin reads its amplitude before smoothing). The picture is the reference tool's with a colour scale moved by 14.3 dB; no level is the tool's |
| Recommendation | A, as for the throttle map (D-005): the levels people quote from this window, and the −30 … 10 dB scale they know it by, are only worth having if they are the tool's. The spectrum beside each row and the throttle strip (F4, F5) stay, in the new decibels. Before the code a round has to read the other three levels of the two smoothing lists and the segments in logs of other lengths and rates (round 11 read the lists: the one for the frequency is the line through 5, 11 or 21 rows or none, the one for the time sets the distance between segments to 0.3, 0.15, 0.06 or 0.03 s) |
| The owner's decision | **A**, 2026-11-14: as the reference tool, every column at the middle of its segment |
| Carried out by | #196 (on its own from `master`). Its map is the reference tool's, cell by cell, on a log of round 11 at nine settings of the two lists (2.9 million cells, each the very colour step), but for the reference tool's first columns, which are not copied. Other lengths and rates have not been seen in the reference tool yet |

The open points at the end of [INVENTORY.md](INVENTORY.md) concern OpenPID's
own mathematics. Whether one of them becomes an entry here is the owner's
call.
