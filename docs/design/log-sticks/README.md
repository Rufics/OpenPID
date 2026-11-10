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
  than its row reaches up over the row above.
- **Decoded by the format crates** (owner, 2026-11-10: the decoder turns
  the value into a fixed internal one for each log type, as the other
  fields): `openpid_core::FlightMode` (Arm, Angle, Horizon, AirMode,
  AltHold, PosHold, Loiter, ReturnHome, GpsRescue, Rescue, Failsafe, Land,
  Auto, Manual, …, `Other(name)`) and `FlightSession::flight_modes`, a
  `ModeChange` per change. The Blackbox decoder (`openpid-fmt-blackbox`,
  `modes.rs`) reads the slow frames' `flightModeFlags`, the mode-box mask:
  bit `n` is box `n` of the firmware's list, by firmware and version
  (Betaflight lists changed at 3.3, 3.4, 3.5, 4.0, 2025.12 and 2026.6;
  Cleanflight and early Betaflight the old list; INAV its own box ids;
  Rotorflight 4.2, 4.3 and 4.6 lists; EmuFlight's Explorer names none, so
  only its ARM bit is named). The `ArduPilot` decoder turns `MODE`
  messages into the vehicle's modes (Copter, Plane with its Q modes) and
  `EV` 10 / 11 into arming. The CSV reader takes `blackbox_decode`'s
  `flightModeFlags` names. `openpid_analysis::flight_modes_at` only finds
  the moment.
- **Mode changes** (owner, 2026-11-10: as the reference tool under its
  throttle row, off by default; `LogDisplay::mode_marks`): a dashed line
  where the modes change, with the modes from then on at its foot, on the
  throttle / motors row.
- `ViewerState::sticks` and `sticks_rect` keep what was drawn, for tests.

## Tests

`sticks_follow_the_rc_commands` (`openpid-analysis`, btfl_027: ranges, the
flight's throttle and roll, the scaling at a sample, past the ends);
`box_names_follow_the_firmware_and_its_version`, `bits_become_common_modes`
and `flight_modes_are_decoded_per_firmware` (`openpid-fmt-blackbox`:
the lists, btfl_027 ARM · ANGLE → ARM · POSHOLD → POSHOLD, INAV 9 ARM ·
ANGLE, Rotorflight 4.3 ARM · BLACKBOX, EmuFlight ARM · bit 21);
`modes_follow_the_vehicle_and_the_arming` and
`flight_modes_come_from_the_mode_and_arming_messages` (`openpid-fmt-ardupilot`:
Copter LOITER, Plane ARM then ARM · RTL, QuadPlane QRTL); the CSV reader's
`opens_as_session`; `modes_of_a_real_flight` (`openpid-analysis`); `log_tab_stacks_the_axes_with_the_motors_and_reads_the_cursor` (none
without a cursor, under the pointer on the last row's right edge, at the
cursor once the pointer leaves).

## Implementation

btfl_027, the pointer at 22 s (ARM · POSHOLD):

![Sticks](sticks.jpg)

With Mode changes on (ARM · ANGLE from the start, ARM · POSHOLD from 12.8 s):

![Mode changes](marks.jpg)
