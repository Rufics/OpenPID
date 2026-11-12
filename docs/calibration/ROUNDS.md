# Calibration rounds

Part of #167 (epic #166).

A round is one pack of synthetic probe logs, built from the repository at
one commit, that the owner runs through the reference tool. What the
reference tool shows is read from the owner's screenshots into
`tests/calibration/` and compared with what OpenPID shows for the same logs
and with the exact values the probes were built to give. A disagreement
becomes an entry of [DECISIONS.md](DECISIONS.md); the quantities are the
rows of [INVENTORY.md](INVENTORY.md).

## The three-round rule

An area closes after **at least three rounds**, and only on a round that
needed no change:

1. **Identify.** Probes that tell apart the possible ways of computing a
   quantity (the "Probe idea" column of the inventory).
2. **Confirm.** OpenPID's predictions of what the reference tool will show
   are written down before the owner runs the round.
3. **Hold-out.** A realistic synthetic flight that neither side was fitted
   to.

A round that leads to a change is followed by another round: the count of
three is a minimum, and the last round of an area is always one after which
nothing was changed.

The areas are taken in the order the owner set on 2026-11-11: Spectrum, Step
response, Freq × throttle, Freq × time, the rest.

## Ledger

| Round | Date | Areas | Pack (commit) | Result | Changes it led to |
|---|---|---|---|---|---|
| 0 (smoke) | 2026-11-11 | does the reference tool open a written log; one screenshot of every screen | `e525e7b` (#177): `openpid gen --out smoke-{1k,2k,4k}.bbl` of one 30 s flight at 1, 2 and 4 kHz, and `openpid decode` of the 2 kHz log as CSV | done: the `.bbl` opens at all three rates, a foreign CSV is refused; 38 screenshots, written up in [REFERENCE-SCREENS.md](REFERENCE-SCREENS.md) | none to OpenPID; the questions below for the rounds |
| 1 | 2026-11-11 | Spectrum (#171) | `a82756a`: nine logs, the spectrum of each at the five smoothing levels, by the capture script (342 steps, 3.6 min). Run twice: the first pack (`68b855e`) got three logs of nine into the reference tool (its "open" dialog cut the list of paths) | 45 pictures. One model of the reference tool's curve reproduces all of them, within a pixel (0.12 dB) from "medium" up ([spectrum.md](spectrum.md)): one Hann-windowed transform of the log without its first and last second, no mean removed, the level without the window's loss, a local regression of the decibel values. It differs from OpenPID's density by construction: entry D-001 | none yet: D-001 is with the owner. Open for round 2: the span in points or in hertz, the spans of four levels exactly and against the log rate, the start of the analysed span |
| 2 | 2026-11-11 | Spectrum (#171), confirm | `cf9d9a9`: one flight at 1, 2, 4 and 8 kHz and at 12, 30 and 47 s, six logs; the plots below 100 Hz at the five smoothing levels and the plots of the whole range at "medium" as vector copies, 32 pictures; the model's predictions in the pack | 102 of 108 copies (879 steps, 10.6 min). The prediction held for the log shaped as round 1's (to 0.006 dB) and missed the other five by up to 0.1 … 2.4 dB at single points: the smoothing window follows the number of points of the curve, `L³·log10(points)`, and the analysed span starts two samples after the first second, not three. With both, the model gives all thirty curves checked to 0.00002 dB ([spectrum.md](spectrum.md)) | the model (`crates/openpid-probe`, #169); none to OpenPID. A round that changes the model is followed by another |
| 3 | 2026-11-11 | Spectrum (#171), hold-out | `f7a4a3e`: a flight with stick moves, motor noise and a resonance at 2, 1, 4 and 8 kHz (25, 18, 36, 21 s) and `s9-bands-2k` of round 1 again; 90 vector copies, 27 pictures; predictions in the pack | all 90 copies (736 steps, 8.6 min). The model gave the four flown logs unchanged: 72 plots, 245 790 points, 0.00003 dB at worst. The repeated probe of round 1 starts its span one sample later than every other log, as it did before: a known detail of 0.01 dB, not a change | none to the model: **the reference side of the Spectrum area is settled**. D-001 decided by the owner (option A); the change to OpenPID follows in #171 |
| 4 | 2026-11-11 | Step response (#172), identify | `dad54b3`: nine logs of plants with a known step response (a delay, a first-order and a second-order one at three log rates and two lengths; one plant under three stick sizes, noise levels, hold times and gains; a stick that moves in parts of the flight only); 153 vector copies of the step response window, 20 pictures | all 153 copies (1 169 steps, 17.5 min). The reference tool's mean curve is the true response to a few thousandths on every plant and rate; OpenPID's is biased the same way on every log (6 % of overshoot on a pure delay, a peak 0.04 … 0.05 too high on the second-order plant). What the result depends on (stick size, noise, hold time, gain, smoothing) is in [step.md](step.md); how a window's response is computed is not identified yet | none yet. The single-window responses of the "Raw" copies are next, then round 5 |

The columns:

- **Round**: its number.
- **Date**: the day the owner ran the pack through the reference tool.
- **Areas**: the areas of the inventory the pack probes.
- **Pack (commit)**: the commit the pack was built from, so that the same
  zip can be built again.
- **Result**: what agreed and what did not, with the ids of the entries
  opened in DECISIONS.md.
- **Changes it led to**: the pull requests that carried out the decisions;
  "none" on the round that closes an area.

Round 0 is the smoke round of #168. Its screenshots also complete the
"Reference tool" column of the inventory: where a cell there says
"not recorded — round 0", the screen is described in
[REFERENCE-SCREENS.md](REFERENCE-SCREENS.md).

## What round 0 leaves for the rounds

Seen on one realistic log, by eye; none is a measurement yet, and none
changes OpenPID before a probe has settled it and the owner has chosen.

| # | Seen | Goes to |
|---|---|---|
| 1 | A steady tone that OpenPID shows 37 dB above the floor does not stand out in the reference tool's spectrum at its default smoothing, while its Freq x Throttle and Freq x Time screens show it as their brightest line | #171, round 1: the spectrum at every smoothing level; broad-band probes next to the tones |
| 2 | The reference tool's noise floor follows the log rate as a density does (−3 dB per doubling) and reads about 6 dB below OpenPID's | #171, round 1 |
| 3 | Its Freq x Throttle map is linear in amplitude by default, with a decibel mode that is 20·log10 of the linear value − 2.7 dB, and an unsmoothed "2D" amplitude view | #173; the "2D" view is a way to read a tone's amplitude without smoothing |
| 4 | Step response: 258 windows where OpenPID takes 209; a peak of ≈ 1.155 where OpenPID shows 1.21; its curve settles on 1, OpenPID's 3 % above | #172 |
| 5 | Motors read as a share of the highest output, not of the range between the two `motorOutput` limits | #175 |
| 6 | Its map has energy 7 % of throttle beyond the flown range on both sides | #173 |
| 7 | The flags value of an armed craft in acro is labelled `ANGLE_MODE` | #175; whether a mode changes what it analyses is not known |

For the owner the round took about an hour of clicking. From round 1 on the
clicks and the screenshots are scripted (#170): `tools/capture/capture.ps1`
travels in every pack with the steps of the round. Its trial on the owner's
machine on 2026-11-11 (one log, the spectrum, the step response, four lists)
ran 42 steps without a failure and saved 10 correct pictures in 44 s.

A second trial the same day (one log, 52 steps, 47 s) taught the script the
toolbar of a plot: it copied three plots as vector graphics and zoomed into
one. From round 2 on a curve is read from the points of such a copy, which
are known to 0.00001 dB, and not from the pixels of a screenshot (0.12 dB).

A third trial (one log, 189 steps, 1.7 min) did the same for the step
response window: its nine plots, every window's response, the smoothing
levels and the other latency, 21 copies.
