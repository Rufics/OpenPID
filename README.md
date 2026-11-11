# OpenPID

**Blackbox log analysis for tuning flight controllers.** Open a Betaflight,
EmuFlight, Rotorflight, INAV or ArduPilot log and see what the quad is doing:
noise spectra, step response, filter and motor behaviour, and rule-based
tuning advice with the evidence behind it. Desktop GUI and a scriptable CLI,
written in Rust, MIT licensed.

[![CI](https://github.com/Rufics/OpenPID/actions/workflows/ci.yml/badge.svg)](https://github.com/Rufics/OpenPID/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Rufics/OpenPID?display_name=tag)](https://github.com/Rufics/OpenPID/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](rust-toolchain.toml)

![Step response with verdicts](docs/screenshots/step.png)

## What it does

- **Step response**: setpoint-to-gyro deconvolution per axis, with verdicts
  (rise, delay, overshoot, settling, final value) judged against Freestyle,
  Racing or Cinematic targets, and a target band on the plot.
- **Noise**: Welch spectra with automatic peak markers, frequency × throttle
  and frequency × time heatmaps, a player that steps through the
  spectrum over time, and an RPM overlay of the motor harmonics.
- **Filters**: Filter Sim runs the unfiltered gyro through the modelled filter
  chain (lowpass, dynamic notch, RPM filter) so you can try new settings
  without flying; from the command line, `run motor-noise` lists every
  motor's harmonics on the gyro, and the motor speed is estimated from the
  noise when eRPM is not logged.
- **Advice**: tuning rules with explicit evidence (overshoot → more D, an
  uncovered peak → a notch, headroom above the gyro LPF → a higher cutoff),
  each proposal with its rationale and confidence, exported as Betaflight CLI
  `set …` commands. PID sliders reproduce Betaflight simplified tuning.
- **Workflow**: up to eight logs side by side (A…H), an analysis window per
  log with automatic take-off/landing trim, a log check that says what each
  log is fit for, `diff` of the settings between two logs, projects that save
  the whole view, PNG and JSON export, keyboard shortcuts, touch mode, a
  first-run tour, English and Ukrainian interface.

OpenPID is inspired by the PIDtoolbox workflow but shares no code with it:
the decoder, DSP and analyses are written from the published format and
algorithm descriptions and validated against NumPy/SciPy references and, log by log, against the parser
of Blackbox Explorer.

## Supported logs

| Firmware | Format | Notes |
|---|---|---|
| Betaflight 4.2 … 2026.x | Blackbox binary (`.bbl`, `.bfl`, `.txt`), `blackbox_decode` CSV | all encodings and predictors, every frame type, resync after corruption; `gyroUnfilt`, `eRPM`, `debug_mode GYRO_SCALED`, high-resolution logs |
| EmuFlight, Rotorflight | Blackbox binary | own firmware profiles |
| INAV | Blackbox binary | basic profile |
| ArduPilot (Copter, Plane, QuadPlane) | DataFlash (`.bin`) | IMU batch sampler for the raw gyro, ESC telemetry for motor rpm; see [docs/ARDUPILOT.md](docs/ARDUPILOT.md) |

Format details per Betaflight version are in
[docs/BLACKBOX-VERSIONS.md](docs/BLACKBOX-VERSIONS.md).

## Install

**Binaries**: download the archive for your platform from the
[latest release](https://github.com/Rufics/OpenPID/releases/latest)
(`openpid-vX.Y.Z-<target>.tar.gz` for Linux and macOS, `.zip` for Windows).
The targets are Linux x86_64, macOS arm64 (Apple silicon) and Windows x86_64;
Intel Macs are not supported.
Each one holds `openpid-gui` (desktop) and `openpid` (CLI).

**From source** (the pinned Rust toolchain is installed automatically by
`rustup` through `rust-toolchain.toml`):

```sh
git clone https://github.com/Rufics/OpenPID.git
cd OpenPID
cargo build --release
./target/release/openpid-gui         # desktop
./target/release/openpid --help      # CLI
```

The test logs under `tests/logs/` live in Git LFS; they are only needed to run
the test suite (`git lfs install && git lfs pull`).

## Quick start

**GUI.** Start `openpid-gui`, drop a log into the window (or use *Open sample
log* for a synthetic flight), and take the seven-step tour. Tabs: Log viewer,
Step response, Spectrum, Freq × throttle, Freq × time, Filter Sim, Advice,
PID sliders, Setup info, Settings. Every control is
described in the [user guide](docs/USER-GUIDE.md).

```sh
openpid-gui flight.bbl                      # one log
openpid-gui before.bbl after.bbl --tab step # two logs to compare, on a tab
openpid-gui session.openpid.json            # a saved project
```

**CLI.** Every analysis of the GUI is a command that prints JSON (or a PNG for
the heatmaps), so it fits in scripts and notebooks:

```sh
openpid info flight.bbl                                   # sessions, firmware, rates, channels, log check
openpid run step flight.bbl --axis pitch --epoch auto     # step response, take-off and landing trimmed
openpid run psd flight.bbl --channel gyro.raw.roll        # Welch spectrum
openpid run thr-heatmap flight.bbl --png throttle.png     # frequency x throttle
openpid run filter-sim flight.bbl --set lpf1_hz=120       # what if the gyro lowpass moved to 120 Hz
openpid run recommend flight.bbl --cli                    # tuning advice as Betaflight `set …` commands
openpid sliders flight.bbl --set simplified_d_gain=90     # Betaflight simplified tuning
openpid diff before.bbl after.bbl --cli                   # settings that changed, as CLI commands
openpid decode flight.bbl --session 1 --out log1.csv      # raw CSV like blackbox_decode
```

`openpid --help` lists every command and option.

## Screenshots

<details>
<summary>Log viewer, heatmaps, Filter Sim, Advice, sliders</summary>

![Log viewer](docs/screenshots/log.png)
![Several logs at once](docs/screenshots/slots-step.png)
![Freq × throttle](docs/screenshots/throttle.png)
![Freq × time](docs/screenshots/time.png)
![Filter Sim](docs/screenshots/filter.png)
![Advice](docs/screenshots/advice.png)
![PID sliders](docs/screenshots/sliders.png)
![Start screen](docs/screenshots/start.png)
![Tour](docs/screenshots/tour-3.png)

</details>

## Documentation

| Document | What is in it |
|---|---|
| [User guide](docs/USER-GUIDE.md) | every tab, parameter and workflow, with screenshots; CLI equivalents |
| [Design](docs/DESIGN.md) | architecture, crates, data model, the roadmap (§12) |
| [Architecture decisions](docs/adr/) | ADRs: minimal dependencies, own decoder, numeric validation, headless GUI tests, fuzzing, … |
| [UX design notes](docs/design/) | one note per feature: problem, options with scores, decision, requirements, acceptance |
| [Blackbox versions](docs/BLACKBOX-VERSIONS.md) | what changed between Betaflight versions and which logs cover it |
| [ArduPilot](docs/ARDUPILOT.md) | how DataFlash messages map to channels and settings |
| [Style](docs/STYLE.md) | code style (adapted Google Rust Style Guide, enforced by clippy) |
| [Process](docs/PROCESS.md) | backlog, milestones, sprints, Definition of Done |
| [Releasing](docs/RELEASING.md) | release-plz, tags, binaries |
| [Test logs](tests/logs/README.md) | where the fixtures come from |

## Contributing

Issues and pull requests are welcome. The short version of
[CONTRIBUTING.md](CONTRIBUTING.md):

- Every PR references an issue (`Closes #N` or `Part of #N`); create one first
  if there is none.
- Commits and PR titles follow [Conventional Commits](https://www.conventionalcommits.org/);
  PRs are squash-merged.
- Before pushing: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cargo deny check`. CI repeats this on Linux,
  macOS and Windows with a 90 % line-coverage gate, and it never stays red.
- New dependencies need an ADR; the PIDtoolbox code (GPLv3) is never copied.

Architecture at a glance: `openpid-core` (data model and extension traits),
`openpid-fmt-*` (decoders), `openpid-dsp` (FFT, spectra, deconvolution),
`openpid-analysis` (analyses and the recommender), `openpid-sim` (synthetic
logs and filter models), `openpid-cli`, `openpid-app` (egui desktop).

## License

[MIT](LICENSE). PIDtoolbox is a separate project under its own licence; its
name is used here only to describe the workflow OpenPID is modelled on.
