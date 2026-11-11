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
| Probe | `s1-levels-2k`, `s2-levels-1k`, `s3-levels-4k`, round 1 (pack `68b855e`): tones of 3, 10 and 30 deg/s over a floor of 1/3 (deg/s)² spread over half the log rate |
| The reference tool shows | at its default "medium": a floor of −38.6 / −41.6 / −44.6 dB at 1 / 2 / 4 kHz; the 30 deg/s tone as a bump of 5.7 dB, the 3 deg/s tone as one of 1.7 dB. It computes one Hann-windowed transform of the whole log, `10·log10(2·\|X\|² / (fs·N))`, and smooths the decibel values by a moving average ([spectrum.md](spectrum.md)) |
| OpenPID shows | a floor of −31.8 / −34.8 / −37.8 dB; the 30 deg/s tone 53.5 dB above it. Welch: 0.25 s segments, the mean of their powers, the window's loss put back, smoothing of the power (`our_spectrum_is_a_power_density` in `crates/openpid-probe`) |
| The exact value | the floor: `10·log10((0.25 + 1/12) / (fs/2))` = −31.8 / −34.8 / −37.8 dB, the density of the noise. A tone has no density: its height depends on the resolution in any spectrum |
| Options | **A.** The reference tool's curve by default (one transform, its level, decibel smoothing with its five levels), and OpenPID's density as a second mode. A user sees the same numbers as in the reference tool and its tutorials; narrow lines are hidden unless the mode is switched. **B.** OpenPID's density by default, the reference tool's curve as a second mode. Noise reads 6.8 dB higher than in the reference tool, tones stand out; numbers from tutorials do not carry over unless the mode is switched. **C.** OpenPID's curve only, the offset documented. |
| Recommendation | A: the epic's aim is that a user who knows the reference tool reads the same numbers. The density stays one switch away because it is the exact quantity, the only one the probes can check to a tenth of a decibel, and the one that shows a steady resonance. To be confirmed once the other six probes of round 1 are in |
| The owner's decision | open |
| Carried out by | — |

The open points at the end of [INVENTORY.md](INVENTORY.md) concern OpenPID's
own mathematics. Whether one of them becomes an entry here is the owner's
call.
