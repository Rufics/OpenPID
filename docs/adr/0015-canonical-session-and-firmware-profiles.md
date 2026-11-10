# ADR-0015: the canonical session is the only thing consumers read; firmware knowledge lives in the profiles

- Status: accepted
- Date: 2026-11-10

## Context

The Log viewer gained the sticks and the flight modes at the cursor (#33,
[log-sticks](../design/log-sticks/README.md)). The first two versions put
the interpretation outside the decoders: `openpid-analysis` scaled
`rcCommand` by ±500 and read the bits of `flightModeFlags` from tables of
its own, then a `modes.rs` in each format crate matched on the firmware
vendor beside the profiles. The owner (2026-11-10): the logic is spread
between the parser, the core and the reader; there must be one internal
structure from which a mode or any other field is simply taken and drawn,
a parser module per log type that parses and maps the data into it, and no
format handling in the analysis; modes and the like are a core function.

The spread had already produced two wrong results: the yaw stick mirrored
(the Cleanflight family stores `rcCommand` yaw inverted) and the `ArduPilot`
sticks read as full deflection (`RCIN` is in microseconds, in another
channel order).

## Decision

Three layers, each with one job.

1. **`openpid-core` owns the canonical model and every lookup on it.** A
   `FlightSession` carries channels in fixed units and conventions
   (`ChannelId`, among them `stick.roll / pitch / yaw / throttle` in percent,
   one sign convention for every firmware), `flight_modes` (`ModeChange`s of
   `FlightMode` values), settings, events. Reading it is a method of the
   session: `sample_at`, `time_s`, `flight_modes_at`, `mode_changes`,
   `sticks_at`. A consumer takes a value and draws it.
2. **A format crate turns one kind of log into that model, in three steps:**
   the *reader* (bytes to raw records: frames, messages, rows, no meaning);
   the *firmware profile* (`FirmwareProfile`, the only place that knows a
   firmware and its versions); the *session builder* (assembles the session
   and asks the profile; it holds no firmware tables).
   The profile contract: `map_field` (logged field to channel, unit, scale),
   `settings` (headers to typed settings), `derive` (channels computed from
   the logged ones: throttle, the sticks, PID error, motor rpm),
   `flight_modes` (the firmware's mode word, a box mask or a mode number, to
   `FlightMode`s), `slider_model`.
3. **`openpid-analysis`, `openpid-sim`, the CLI and the app read the
   canonical model only.** They do not branch on the firmware vendor or
   version and do not read raw headers, slow frames or firmware events.

Applied here: the mode-box tables are in the Blackbox profiles
(`betaflight.rs` by version, `inav.rs`, `rotorflight.rs`; `EmuFlight` and
unknown firmware name the three boxes every Cleanflight fork starts with),
the `ArduPilot` mode numbers and the `RCIN` channel map and calibration in
its profile; the session builders only walk the slow frames or the `MODE` /
`EV` events. `modes.rs` and `sticks.rs` left `openpid-analysis`.

The filter model (`openpid-sim`) had read four settings from raw headers
kept in `Settings::extra` and taken the firmware version for their defaults.
They are typed now (`Lowpass::dyn_expo`, `RpmNotch::fade_range_hz`,
`weights`, `lpf_hz`; `DynNotch::count` always set) and filled by the
profiles (`common::filter_details` on Blackbox logs: Betaflight's headers,
else its default of that version; the `ArduPilot` harmonic notch has no
fade-in or weights). `ChainModel::from_settings` takes the settings only.
The simulation of the Betaflight fixture is unchanged to the bit.

## Consequences

- A new firmware or version is one profile; a new consumer needs no
  firmware knowledge.
- The profile trait grows with every kind of firmware-specific meaning; that
  is where it belongs.
- **Deviations that exist today** (older code; each is to move behind the
  profile):
  - `openpid-analysis/src/fitness.rs` picks the advice "how to log the
    unfiltered gyro" by firmware vendor and version.
  - `rc.N` channels (`ChannelId::RcCommand`) keep the firmware's own units
    and channel order (±500 on the Cleanflight family, microseconds on
    `ArduPilot`, where `CTUN.ThrIn` of old Copter logs shares `rc.2` with
    `RCIN.C3`); they are the logged commands, the canonical pilot input is
    `stick.*`.
