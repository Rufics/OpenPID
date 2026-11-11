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
