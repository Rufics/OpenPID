# Open questions for the product owner

## Decisions taken (2026-11-04)

| # | Decision |
|---|----------|
| Q1 | Licence **MIT**. |
| Q2 | Firmware order: phase 0 Betaflight/EmuFlight/Rotorflight; phase 2 **ArduPilot, then INAV**. |
| Q3 | Development logs come from the community (see `tests/logs/README.md`); the owner's own logs are for the final check only. |
| Q4 | "Touch support" means touch screens on the desktop; mobile platforms are not blocked but not planned. |
| Q5 | `rustfft` is acceptable. |
| Q6 | PID slider tool: Betaflight only (simplified tuning 4.3+). Implemented through `BoundProfile::slider_model`. |

The rest (Q6–Q12) stay open until the relevant phase.

## Full list

| # | Question | Options / recommendation |
|---|----------|--------------------------|
| Q1 | **Project licence.** PIDtoolbox (the public fork) is GPLv3; the old files are Beerware. We copy no code, so we are free to choose. | MIT **or** Apache-2.0 recommended (allows any use, compatible with every dependency). GPLv3 if forks must stay open. |
| Q2 | **Firmware priority for the MVP.** Betaflight/EmuFlight/Rotorflight share one binary format (phase 0). INAV: the same format, other scales. ArduPilot (DataFlash .bin), KISS Ultra, Fettec, QuickSilver: separate formats without a public spec. | Recommended: phase 0 BF/Emu/RF; phase 1 INAV; phase 2 the rest as sample logs arrive. Confirm or reorder. |
| Q3 | **Sample logs.** Conformance and golden tests need short (5–30 s) real logs from every firmware and version, ideally with `gyroUnfilt`, eRPM and various `debug_mode` values. Can you provide them (or allow collecting them from the community) and put them in the repository under the project licence? | Yes/no; if yes, which firmwares are at hand. |
| Q4 | **"Touch support"**: touch screens (laptops and tablets with Windows/Linux, iPad-like scenarios) or something else (for example mobile platforms later)? | Multi-touch gestures are in the desktop GUI (egui supports them natively) and future iOS/Android builds are not blocked. Confirm. |
| Q5 | **The "minimal dependencies" line.** Is `rustfft` (pure Rust) acceptable for the FFT, or do you want an own FFT? `wgpu`/`winit` are unavoidable for the GUI. | `rustfft` recommended (see ADR-0001). |
| Q6 | **PID slider tool**: the Betaflight sliders (Configurator 10.9+ / 4.3+ "simplified tuning") that turn positions into P/I/D/F/D_min? Other firmwares? | Implemented through `BoundProfile::slider_model`, Betaflight first. Your access to the PTB UI will help to compare the behaviour. |
| Q7 | **A/B comparison** (two logs at once, as in PTB): needed in phase 1 or can it wait? | Phase 1 recommended: it is the core of the PTB workflow. |
| Q8 | **Scope of the "Pro" features.** All seven rows of the table (notch overlay 2D/3D, RPM estimator, filter sim, generator, motor noise, spectrum player) in phase 2, or are some priorities? | Suggested order: filter sim → notch/RPM overlay 2D → RPM estimator → motor noise → overlay 3D → generator → player. |
| Q9 | **Interface and documentation language.** | UI EN + UA (a switch); code, comments and documents in English. |
| Q10 | **Autotune / recommendations.** Is there a vision of the algorithm (rules like the PTB guides, or a model), or is it R&D for phase 3? | The `Recommender` trait is in the architecture; the content is phase 3. |
| Q11 | **Name / brand.** Repository `OpenPID`; binaries `openpid` (CLI) and `openpid-gui`. | Confirm or propose another name. |
| Q12 | **Access to the PTB UI.** For phase 1 I will need screenshots or descriptions: the step-response tab (smoothing parameters, Y-offset, thresholds), the spectral tab (ranges, clim), the slider tool. I will ask for specific screens when we get there. | — |

## What could not be checked in that session
- The PIDtoolbox Patreon page and `betaflight.com` could not be opened (network restrictions of the session); the Blackbox specification was taken from the `betaflight/betaflight.com` repository on GitHub.
- The original `bw1129/PIDtoolbox` repository is no longer available; the algorithms were compared against the public fork (an old GPLv3 version) and PID-Analyzer (deconvolution). The current PTB 0.9x logic (deconvolution in `PTstepcalc`) is reproduced from its description and from PID-Analyzer, so the details (default values) should be compared with your UI.
