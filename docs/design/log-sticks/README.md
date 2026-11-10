# Sticks and flight modes on the Log viewer

The owner's remark (2026-11-10, #33, PR #143): the Log viewer does not show
where the sticks were as the cursor moves; the reference tool does (the
owner's screenshot of it: two boxes, "T / Y" and "P / R", beside its plots,
with the motor outputs and the gyro at the cursor under them).

## Cause

The Log viewer showed the values of its lines at the cursor (gyro,
setpoint, throttle, motors) but not the RC commands; reading the stick
inputs took adding `rc.0`…`rc.3` as other channels, on a row of their own,
in raw units.

## Options

Five places for the sticks, mocked up on the Log viewer (btfl_027, cursor
at 14.6 s):

![The five designs](variants.jpg)

| | Design | Pros | Cons | Eff | Disc | Conv | Kbd | Touch | Cost | Score |
|---|---|---|---|---|---|---|---|---|---|---|
| S1 | In the panel, under the cursor time | where the reference tool has them, beside its panel | gone with the panel folded; far from the plots | 4 | 4 | 5 | 4 | 4 | 4 | 46 |
| **S2** | **A card floating at the right edge of the throttle / motors row** | **on the plots, with the panel folded too; next to the motors they drive** | **covers the last seconds of that row** | **4** | **4** | **3** | **3** | **3** | **4** | **39** |
| S3 | A sticks row: the commands over time, values at the cursor | the inputs over the whole flight | a row more; no stick picture | 4 | 4 | 3 | 4 | 4 | 4 | 42 |
| S4 | A column beside the motors row | never covers data | narrows the motors row only, misaligning its time axis | 4 | 4 | 4 | 3 | 4 | 3 | 41 |
| S5 | A card beside the cursor line | where the eye is | covers the data around the cursor | 5 | 4 | 2 | 3 | 2 | 3 | 38 |

## Decision (2026-11-10)

**S2**, the owner's choice ("Ні, варіант S2"): S1 scored higher, the
owner's decision beats the score.

## Implementation requirements

- `openpid_analysis::sticks_at(session, t)`: roll, pitch and yaw from
  `rcCommand` scaled to −1…1 (±500 is full deflection on the Betaflight
  family), the throttle channel to 0…1, at the sample at or after `t`;
  `None` without RC commands. No mathematics in the app.
- `log_tab.rs`: the card at the bottom right of the throttle / motors row
  (the last row when it is hidden), two 54 px boxes in mode 2 (left:
  throttle up, yaw across; right: pitch up, roll across) with a dot in the
  accent colour, the labels "T / Y" and "P / R"; a translucent background
  in the theme. It follows the pointer while it is over the rows, else the
  placed cursor; with neither it is not drawn.
- **Sticks** (`LogDisplay::sticks`, on by default; owner, 2026-11-10: "a
  way to hide them"): a check box at the end of the panel, after the
  cursor rows, so the fields above it keep their place.
- **Flight modes** (owner, 2026-11-10: the flight-mode labels, following
  the cursor; `LogDisplay::modes`, on by default, a check box after
  Sticks): the modes active at the same moment, on a line over the sticks
  (owner: over the sticks, not beside them), "ARM · ANGLE"; a card taller
  than its row reaches up over the row above. `openpid_analysis::flight_modes_at` reads the last
  slow frame's `flightModeFlags` at or before that moment. On the
  Betaflight family the field is the mode-box activation mask, so bit `n`
  is box `n` of the firmware's box list, which changed at 3.3, 3.4, 3.5,
  4.0, 2025.12 and 2026.6 (`flight_mode_names`, the names Betaflight and
  Blackbox Explorer print); Cleanflight, EmuFlight and Betaflight before 3.3
  use the older list; INAV, Rotorflight and `ArduPilot` give the field
  another meaning and show no modes. On btfl_027: ARM · ANGLE (0x3) from the start,
  ARM · POSHOLD (0x201) from 12.8 s; the reference tool prints the same
  bits as "ANGLE_MODE|HORIZON_MODE" and "ANGLE_MODE|SONAR", an older,
  shifted list.
- `ViewerState::sticks` and `sticks_rect` keep what was drawn, for tests.

## Tests

`sticks_follow_the_rc_commands` (`openpid-analysis`, btfl_027: ranges, the
flight's throttle and roll, the scaling at a sample, past the ends);
`box_names_follow_the_firmware_version` and `modes_of_a_real_flight`
(`openpid-analysis`: the lists per version, btfl_027's ARM, ANGLE,
POSHOLD); `log_tab_stacks_the_axes_with_the_motors_and_reads_the_cursor` (none
without a cursor, under the pointer on the last row's right edge, at the
cursor once the pointer leaves).

## Implementation

btfl_027, the pointer at 22 s (ARM · POSHOLD):

![Sticks](sticks.jpg)
