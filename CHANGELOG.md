# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/Rufics/OpenPID/compare/v0.1.1...v0.1.2) - 2026-11-06

### Added

- *(gui)* step response verdicts — three-axis table, target band, presets, Advice line ([#118](https://github.com/Rufics/OpenPID/pull/118))
- *(gui)* analysis window — timeline strip per slot, Auto trim, --epoch auto ([#117](https://github.com/Rufics/OpenPID/pull/117))
- *(gui)* first run — start screen, sample log, recent files, 7-step coach-mark tour ([#119](https://github.com/Rufics/OpenPID/pull/119))
- *(gui)* keyboard shortcuts — tabs by number, Cmd/Ctrl keys, Ctrl+Tab, help window ([#115](https://github.com/Rufics/OpenPID/pull/115))

## [0.1.1](https://github.com/Rufics/OpenPID/compare/v0.1.0...v0.1.1) - 2026-11-05

### Added

- *(analysis)* log fitness check with badges, Diagnostics card and CLI block ([#112](https://github.com/Rufics/OpenPID/pull/112))
- *(gui)* colour scale beside every heatmap, linked scales across logs, value under the pointer ([#108](https://github.com/Rufics/OpenPID/pull/108))
- *(gui)* up to eight logs at once — slot list with colours, visibility, solo and stable letters ([#106](https://github.com/Rufics/OpenPID/pull/106))

## [0.1.0](https://github.com/Rufics/OpenPID/releases/tag/v0.1.0) - 2026-11-05

First release: phases 0–3 of `docs/DESIGN.md` — the foundation, parity with
PIDtoolbox free and Pro, and evidence-based recommendations.

### Added

- *(blackbox)* own decoder for Cleanflight-family logs (Betaflight 4.2 → 2025.12, EmuFlight, Rotorflight, INAV) with firmware profiles, conformance against `blackbox-log`, golden tests and a mutation fuzzer with a weekly CI job (#81)
- *(ardupilot)* ArduPilot DataFlash (`.bin`) decoder, IMU batch sampler as raw gyro/acc, logger restarts as sessions (#8, #63, #71)
- *(csv)* CSV logs in and out, compatible with `blackbox_decode`
- *(analysis)* step response (Wiener deconvolution, Y-offset correction, cross-correlation latency), Welch PSD, freq × throttle and freq × time heatmaps, segment counts per throttle bin (#61, #62)
- *(analysis)* Filter Sim: the unfiltered gyro through the modelled firmware filter chain with what-if overrides (#66); RPM harmonics and notch bands overlay (#67); motor speed estimate from the gyro's noise peaks (#75); motor / prop noise table (#76)
- *(analysis)* rule-based recommender with evidence — overshoot → more D, an uncovered spectrum peak → a notch, headroom above the gyro LPF → a higher cutoff — and the Betaflight CLI exporter (#86, #87)
- *(sim)* PT1/PT2/PT3 and biquad models, the filter-chain model from log settings, and a synthetic session generator with known ground truth (`openpid gen`) (#64, #65, #72)
- *(gui)* egui desktop app: log viewer with epoch handles, step response, spectrum with frequency markers, heatmaps, Surface 3D, Filter Sim, Motor noise, Advice, PID sliders (Betaflight simplified tuning), Diagnostics, Settings; two slots for A/B comparison (#57, #60, #82, #88)
- *(gui)* spectrum player on the freq × time tab synced with the log-viewer cursor (#84); touch gestures and touch mode (#78); English / Ukrainian interface (#85); dark and light themes with persisted preferences (#59)
- *(gui)* project files (`.openpid.json`), PNG and JSON export (#73, #74)
- *(cli)* `openpid info`, `decode`, `run <analysis>`, `sliders`, `gen`, `diff` and `run recommend --cli`
- *(docs)* design document, ADRs 0001–0014, user guide with screenshots, Scrum process on GitHub

### Fixed

- *(blackbox)* stop decoding at trailing garbage and validate resync frames by loop rate (#46); Rotorflight events and helicopter units (#48); an `I interval` that does not fit in 32 bits no longer divides by zero (#81)
- *(gui)* dynamic-notch band drawn as an outline, not a tint over the heatmap (#77); plain-text player buttons for the UI font (#90)

### Other

- CI on Linux, macOS (arm64 and Intel) and Windows: fmt, clippy, tests, coverage ≥ 90 %, cargo-deny, SonarCloud, conventional commits, benchmarks (report only), release binaries; release-plz as a GitHub App bot
- MIT-licensed community test logs in Git LFS (#6)
