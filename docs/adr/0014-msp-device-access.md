# ADR-0014: Access to the flight controller over MSP

- Status: accepted (a decision on the boundaries; no code is added)
- Date: 2026-11-05

## Context

DESIGN §9 foresees a `Device` trait: read the settings from the controller,
write proposals (`Proposal`, the neighbouring `openpid-core::recommend`) and
download logs from the flash without a card. For Betaflight/INAV/Rotorflight
that is MSP (MultiWii Serial Protocol, v1 and v2) over USB-CDC or UART.

The problem is the serial-port dependency. The most common crate,
`serialport`, pulls `libudev` on Linux by default (a C library for port
enumeration); without it (`default-features = false`) only the libc bindings
through `nix` remain, and on some platforms the build still needs system
packages. That contradicts ADR-0001 (minimal dependencies, pure Rust, a build
without system libraries on three platforms) and the CI rule "nothing changes
without a commit".

Options considered:

| Option | Pros | Cons |
|---|---|---|
| `serialport` (default features) | the de-facto standard, port enumeration through udev | a C dependency on Linux, `pkg-config`/`libudev-dev` in CI and for users |
| `serialport` without `libudev` | no C code | port enumeration on Linux limited; the API still goes through `nix`/`winapi` |
| `serial2` | pure Rust (bindings to the system APIs without C code), BSD-2-Clause, enumeration through sysfs/IOKit/the registry, no `tokio` | a younger crate, fewer users |
| own bindings to termios / `CreateFile` | zero dependencies | volume and risk of mistakes on three platforms, not our domain |
| out of scope (log files only) | nothing changes | no "apply the advice in one click" and no logs without a card |

## Decision

1. **No device access in v1.0.** The workflow stays "log → analysis → advice →
   text for the configurator CLI" (`openpid run recommend --cli`, the Advice
   tab). That closes research question #42 without code.
2. **When we get there** (a separate milestone after v1.0), the implementation
   will be:
   - a separate crate **`openpid-msp`** behind the **`device`** feature, off by
     default; the GUI and CLI build without it as they do now;
   - the serial port through **`serial2`** (pure Rust, BSD-2-Clause, allowed in
     `deny.toml`), added through an update of this ADR and of `deny.toml`;
   - MSP v2 implemented **from the protocol documentation** (the Betaflight
     wiki / `msp_protocol.h` as the specification of the constants), without
     copying firmware code; the minimal set: `MSP_API_VERSION`,
     `MSP_FC_VARIANT`, `MSP_PID`, `MSP_FILTER_CONFIG`, `MSP_SET_*`,
     `MSP_EEPROM_WRITE`, `MSP_DATAFLASH_SUMMARY/READ`;
   - **writes only after an explicit confirmation** in the UI showing the diff
     (the same `BetaflightCli::diff_commands`), with a mandatory read-back and
     comparison; never an automatic `save`;
   - the `Device` trait lives in `openpid-core` (no dependencies) and
     `openpid-msp` implements it; headless tests on a fake port (`Read + Write`
     in memory) with recorded exchanges.
3. Downloading logs from the flash (`MSP_DATAFLASH_READ`) is part of the same
   milestone; the received stream is an ordinary `.bbl` that
   `openpid-fmt-blackbox` already opens.

## Consequences

- No new dependency and no C code now; CI and the users' builds are unchanged.
- Advice is applied through the configurator CLI: one copy step, but a
  transparent one: the user sees every `set` before `save`.
- When the `device` feature appears it does not affect the default build; the
  `serial2` risk is confined to one crate.
