# Sliders: what the rounds have shown

Part of #166. The row is P4 of [INVENTORY.md](INVENTORY.md) (the ratio
sliders); P1–P3 and P5–P6 are OpenPID's own. The reference tool is
version 0.94.

## Round 13 (2026-11-15)

The slider tool is a small window of its own with no plot (round 12). In
round 13 (pack `959ad7c`) the start gains 45, 80 and 30 were typed in and
the sliders moved by their arrows, with a picture after each move: ten
pictures.

| Moved | "PID test values": roll, pitch, yaw |
|---|---|
| start gains typed, every slider at 1.00 | 45 80 30; 45 80 30; 45 80 0 |
| PD ratio 1.25 | 45 80 38; 45 80 38; 45 80 0 |
| PI ratio 0.85 | 45 68 38; 45 68 38; 45 80 0 |
| Roll-Pitch ratio 1.10 | 45 68 38; 50 75 41; 45 80 0 |
| Yaw ratio 0.90 | 45 68 38; 50 75 41; 40 72 0 |
| Master Multiplier 1.25 | 56 85 47; 62 94 52; 51 90 0 |

## The rule

With the start gains `P`, `I`, `D` and the sliders `pd`, `pi`, `rp`, `yaw`
and `m`:

- roll: `P·m`, `I·pi·m`, `D·pd·m`;
- pitch: the same, each times `rp`;
- yaw: `P·yaw·m`, `I·yaw·m` (the PI ratio does not act on yaw), and a D of
  0 whatever the start D;
- every value is the product rounded to a whole number at the end, not
  step by step: pitch D is `30 · 1.25 · 1.10 · 1.25 = 51.56` and reads 52.

It gives every number of the table. One press of an arrow moves a slider
by 0.05, and the slider's value is the sum of those steps: after two
presses down the yaw ratio is a hair under 0.9, so `45 · 0.9` reads 40,
not 41. The list of the window names Betaflight, INAV, FETTEC, KISS_ULTRA,
QuickSilver and Ardupilot; only Betaflight was asked. "Reset" empties the
start gains and puts the sliders back to 1; the test values stay on
screen.

## OpenPID

OpenPID's ratio sliders (P4) apply the same products to the gains of the
log, axis by axis, and round to the firmware's gain step. Two things
differ by design: the start gains are the log's, with a D of yaw of its
own, where the reference tool takes one typed triple for the three axes
and gives yaw no D; and a slider holds the value it shows, so the hair
under 0.9 does not arise.

### Not settled

- The other five entries of the list: whether they change the step of a
  gain or the rounding.
