# Calibration decisions

Part of #167 (epic #166).

Every disagreement between the reference tool, OpenPID and the exact value
is recorded here, one entry each. **OpenPID's behaviour changes only after
the owner has chosen**: an entry without a decision changes no code.

An entry compares three things for one synthetic probe:

- what **the reference tool** shows, as read from the owner's screenshots
  (the numbers are kept in `tests/calibration/`, the screenshots are not
  committed);
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
3. Screenshots of the reference tool are not committed, only the numbers
   read from them (`tests/calibration/`).
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
| The reference tool shows | the number or curve read from the screenshot, with the file in `tests/calibration/` |
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
| Carried out by | #181 |

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
| Carried out by | #184 |

The open points at the end of [INVENTORY.md](INVENTORY.md) concern OpenPID's
own mathematics. Whether one of them becomes an entry here is the owner's
call.
