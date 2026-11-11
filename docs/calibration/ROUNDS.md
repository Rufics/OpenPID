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
| 0 (smoke) | pack sent 2026-11-11 | does the reference tool open a written log; one screenshot of every screen (the reference side of the inventory) | `e525e7b` (#177): `openpid gen --out smoke-{1k,2k,4k}.bbl` of one 30 s flight at 1, 2 and 4 kHz, and `openpid decode` of the 2 kHz log as CSV | with the owner | — |

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
"Reference tool" column of the inventory, where a cell now says
"not recorded — round 0".
