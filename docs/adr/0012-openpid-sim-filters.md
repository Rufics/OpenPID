# ADR-0012: `openpid-sim`, the firmware filter models as a separate crate

- Status: accepted
- Date: 2026-11-05

## Context

Phase 2b (DESIGN §6.6) needs a model of the controller's filter chain: Filter
Sim runs `gyro.raw` through a model built from `Settings` and compares it with
the log's `gyro.filtered` and with "what-if" settings. The same models are
needed for the notch-band overlay on the spectra and, in phase 3, for the
recommendations. `openpid-dsp` deliberately holds only pure numeric primitives
(FFT, windows, spectra) without any knowledge of firmwares; mixing the discrete
forms of specific controllers into it would drag firmware semantics into the
base layer.

## Decision

- **A separate crate `openpid-sim`** without external dependencies: only
  `openpid-dsp` (the shared `Complex` type, the STFT for the dynamic-notch
  tracker) and `openpid-core` (`Settings`, from which the chain model is built).
  No new entry in `deny.toml`.
- **Two views of every filter**: `update(x) -> y` (a state machine in `f64`,
  the same discrete form as in the firmware) and `response(f) -> Complex` (the
  transfer function on the unit circle). Tests check that both agree: a sine
  run through `update` has the amplitude and phase of `response`.
- **The formulas come from the description, not the code** (the licence line,
  as with PIDtoolbox): PT1 with the coefficient `dt / (RC + dt)`; PT2/PT3 as a
  cascade of PT1 with the cutoff correction `1 / sqrt(2^(1/n) − 1)` so the
  cascade gives −3 dB at the nominal frequency; the biquad from the RBJ
  cookbook (a Butterworth LPF with `Q = 1/√2`, a notch with
  `Q = f_c·f_cut / (f_c² − f_cut²)`, the Betaflight convention), in DF2T form.
- **`Chain`** is an ordered list of stages with the product of their responses;
  **`ChainModel`** (the `model` module) builds a concrete chain from `Settings`
  and adds the moving parts (the throttle-dependent cutoff, the RPM notch, the
  dynamic notch) on top of these primitives, not inside them. The moving parts
  are approximations (DESIGN §6.6): the model runs at the log rate, the
  dynamic-notch tracker is an STFT peak picker instead of the firmware's SDFT.

## Consequences

- `openpid-analysis` gets Filter Sim (#23) as an ordinary `Analysis` that
  depends on `openpid-sim`; the GUI and CLI know nothing about the formulas.
- New filter types of other firmwares (INAV, the ArduPilot `INS_GYRO_FILTER`
  biquad) are added as stages with their own frequency-response tests.
- The discrete forms of other firmwares may differ from Betaflight (for example
  a bilinear transform instead of `dt/(RC+dt)`); when that matters for Filter
  Sim, the stage gets a variant, not a global switch.
