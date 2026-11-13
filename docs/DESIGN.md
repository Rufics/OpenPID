# OpenPID — design

Status: v0.4 (2026-11-04), phases 0 and 1 implemented (see §12); the development process and style are in `CONTRIBUTING.md`, `docs/STYLE.md`, `docs/RELEASING.md`.

OpenPID is a cross-platform (macOS / Windows / Linux) Rust tool for decoding flight-controller blackbox logs, navigating them and helping with PID and filter tuning. The functional reference is PIDtoolbox / PIDtoolbox Pro, but every line of code (decoder, mathematics, GUI) is our own.

---

## 1. Goals and principles

| # | Principle | What it means in practice |
|---|-----------|---------------------------|
| P1 | **Rust only, minimal dependencies** | Dependencies only where an own implementation is unreasonably expensive or dangerous (GPU / window system, FFT, serialisation). Every dependency is recorded in an ADR with its reasoning. Pulling in C/C++ transitively is forbidden (except the OS system APIs). |
| P2 | **Trustworthy mathematics** | `f64` in every computation; units in the types; explicit validity masks instead of "silent" NaN; every algorithm has reference tests against an independent implementation (NumPy/SciPy scripts in `tools/reference`, development only). |
| P3 | **Extensible without rewriting the core** | Log format, firmware, analysis, recommender (autotune), exporter: everything plugs in through traits and registries. Adding a format = adding a crate that implements `LogFormat`. The core (`openpid-core`) knows nothing about Betaflight. |
| P4 | **One engine, many front-ends** | GUI, CLI and tests use the same pipeline. The GUI only presents data; there is no mathematics in it. |
| P5 | **Honest with the user** | Corrupt frames, gaps, an unstable sample rate, unknown fields are shown, not hidden. Every plot states which parameters it was computed from. |
| P6 | **Cross-platform and touch from day one** | One binary per platform, GPU rendering, multi-touch (pinch zoom, two-finger pan), large touch targets in "touch mode". |

Non-goals (at this stage): a web version, mobile platforms, real time with the FC over USB (the architecture does not block it, see §9).

---

## 2. Functional scope

The mapping to the PIDtoolbox / Pro feature table and the phase plan.

| Feature | PTB | Pro | OpenPID phase | Note |
|---------|:---:|:---:|:---:|------|
| Log viewer (time plots, A/B) | ✓ | ✓ | 1 | Two logs A/B side by side, as in PTB |
| Step response | ✓ | ✓ | 1 | Wiener deconvolution (§6.2) |
| 2D spectral analyzer (PSD) | ✓ | ✓ | 1 | Welch |
| Freq × throttle heatmap | ✓ | ✓ | 1 | §6.4 |
| Freq × time heatmap (STFT) | ✓ | ✓ | 1 | §6.5 |
| PID slider tool | ✓ | ✓ | 1 | The firmware's own sliders through its profile (Betaflight simplified tuning), else ratio sliders over the logged gains (`RatioSliders`), as the reference tool's slider tool; every log's position on one track per slider |
| Multi-firmware support | ✓ | ✓ | 0→2 | Betaflight/EmuFlight/Rotorflight: phase 0; ArduPilot done (ADR-0010, `docs/ARDUPILOT.md`), then INAV: phase 2; KISS/Fettec/QuickSilver when samples exist (decision Q2) |
| RPM / dyn-notch overlay 2D | — | ✓ | 2 | Needs eRPM (DShot telemetry) and the pole count |
| 3D surface of freq × throttle | — | — | — | Built as a tab, then removed on 2026-11-10 at the owner's decision: the reference tool's tutorial shows no 3D view, and the row had been listed as a Pro feature without a source |
| RPM estimator | — | ✓ | 2 | Motor speed from the noise peaks when there is no telemetry |
| Filter sim | — | ✓ | 2 | `gyroUnfilt` through a model of the firmware filters |
| Test signal generator | — | ✓ | 2 | Synthetic signals to check filters and algorithms |
| Motor / prop noise | — | — | 2 | Our own, command line only (`openpid run motor-noise`). It was listed as a Pro feature by mistake; the reference tool has no such screen, and the tab was removed on 2026-11-10 |
| Dynamic spectrum player | — | ✓ | 2 | Animated playback of the STFT over time |
| Recommendations / autotune | — | — | 3 | Our own extension: `Recommend` (rules with evidence), the Advice tab, export to the Betaflight CLI |
| Export (CSV/JSON/PNG, CLI diff) | partly | partly | 1 | CSV/JSON/PNG from the GUI and CLI; `openpid diff` and `--cli` are phase 3 |
| Headless CLI | — | — | 0 | For CI, scripts and golden tests |

---

## 3. Top-level architecture

```
                 ┌──────────────────────────────────────────────┐
                 │               openpid-app (GUI)              │
                 │  egui/eframe · views · touch · state/cache   │
                 └───────────────┬──────────────────────────────┘
                                 │ only types from core/analysis
┌──────────────┐   ┌─────────────┴────────────┐   ┌──────────────────┐
│ openpid-cli  │──▶│     openpid-analysis     │◀──│  openpid-sim     │
│ (headless)   │   │ step · spectral · heat · │   │ filter sim · gen │
└──────┬───────┘   │ notch overlay · metrics  │   └────────┬─────────┘
       │           └─────────────┬────────────┘            │
       │                         │                         │
       │           ┌─────────────┴────────────┐            │
       │           │       openpid-dsp        │◀───────────┘
       │           │ fft · windows · welch ·  │
       │           │ deconv · filters · resamp│
       │           └─────────────┬────────────┘
       │                         │
       │           ┌─────────────┴────────────┐
       └──────────▶│       openpid-core       │
                   │ FlightLog · Channel ·    │
                   │ units · traits · registry│
                   └─────────────┬────────────┘
                                 │ impl LogFormat / FirmwareProfile
      ┌──────────────┬───────────┴──────────┬──────────────────┐
      │ fmt-blackbox │ fmt-blackbox-csv     │ fmt-ardupilot …  │
      │ (BF/Emu/RF/  │ (blackbox_decode     │ (phase 2)        │
      │  INAV binary)│  output, for tests)  │                  │
      └──────────────┴──────────────────────┴──────────────────┘
```

Cargo workspace:

```
openpid/
├─ Cargo.toml                 # workspace, [workspace.dependencies], lints
├─ crates/
│  ├─ openpid-core/           # domain model, traits, registries, errors
│  ├─ openpid-dsp/            # pure mathematics, no I/O
│  ├─ openpid-analysis/       # analyses on top of core + dsp
│  ├─ openpid-sim/            # filter simulation, signal generator
│  ├─ openpid-fmt-blackbox/   # own binary Blackbox decoder
│  ├─ openpid-fmt-csv/        # CSV from blackbox_decode (compatibility)
│  ├─ openpid-cli/            # the `openpid` binary
│  └─ openpid-app/            # the `openpid-gui` binary
├─ tests/
│  ├─ logs/                   # reference logs (small, several firmwares)
│  └─ golden/                 # expected analysis results (JSON)
├─ tools/
│  └─ reference/              # Python scripts that generate the references (dev only)
└─ docs/
   ├─ DESIGN.md
   └─ adr/
```

Dependency rules between the crates (checked in CI through `cargo deny` and an own script):

- `core` depends on nothing in the workspace.
- `dsp` depends only on `core` (for the unit types) and `rustfft`.
- `fmt-*` depend only on `core`.
- `analysis`, `sim` → `core`, `dsp`.
- `app`, `cli` → everything above; **nothing depends on `app`**.

---

## 4. Domain model (`openpid-core`)

### 4.1 The canonical log representation

```rust
/// One file may hold several flights (sessions), like a .BBL with several logs.
pub struct LogFile {
    pub source: SourceInfo,            // path, size, hash
    pub sessions: Vec<FlightSession>,
}

pub struct FlightSession {
    pub id: SessionId,
    pub firmware: FirmwareInfo,        // vendor, version, target, date
    pub settings: Settings,            // typed settings (PID, filters, rates, …)
    pub raw_headers: Vec<(String, String)>, // every header as is
    pub timebase: Timebase,            // nominal step, actual step, jitter, gaps
    pub channels: ChannelSet,          // every data series
    pub events: Vec<Event>,            // E frames: arm/disarm, inflight adjust, logging resume, …
    pub flight_modes: Vec<ModeChange>, // flight modes over time, decoded per firmware
    pub diagnostics: Diagnostics,      // corrupt frames, resyncs, unknown fields
}
```

**The session is the only thing consumers read** (ADR-0015). The core owns
the model and every lookup on it (`sample_at`, `flight_modes_at`,
`mode_changes`, `sticks_at`); a format crate is a reader (bytes to raw
records), a firmware profile (the only place that knows a firmware: fields,
scales, settings, derived channels, mode words) and a session builder; the
analysis, the simulator and the app never branch on the firmware.

**Flight modes** are one set of values for every firmware (`FlightMode`: Arm,
Angle, Horizon, AirMode, AltHold, PosHold, ReturnHome, GpsRescue, Failsafe, …,
`Other(name)` for the rest). The profile turns the firmware's mode word into
them (`BoundProfile::flight_modes`): the mode-box mask of
`flightModeFlags` by box list and version (Betaflight, Cleanflight, INAV,
Rotorflight; EmuFlight and unknown firmware: ARM, ANGLE, HORIZON, the other
bits numbered), the `ArduPilot` mode number by vehicle (arming from its `EV`
messages); the CSV reader takes the `flightModeFlags` names.

**Sticks** are the channels `stick.roll`, `stick.pitch`, `stick.yaw` (−100 …
100 %: right, forward, right positive) and `stick.throttle` (0 … 100 %; the
collective on a helicopter), derived by the profile: `rcCommand` on the
Cleanflight family (±500 full deflection, yaw stored inverted), `RCIN` by
`RCMAP_*` and `RCn_MIN / MAX / TRIM / REVERSED` on `ArduPilot` (pitch
negated).

A **channel** is one series with one unit:

```rust
pub struct Channel {
    pub id: ChannelId,
    pub unit: Unit,                    // DegPerSec, Percent, Us, Raw, Rpm, Volt, …
    pub data: Series,                  // f64 + validity mask + time index
    pub origin: Origin,                // Logged | Derived { from: Vec<ChannelId>, by: &'static str }
}

/// A semantic identifier, independent of the firmware.
pub enum ChannelId {
    Time,
    LoopIteration,
    Gyro { axis: Axis, stage: GyroStage },     // Raw | Scaled | Filtered
    Setpoint { axis: Axis },
    PidTerm { axis: Axis, term: PidTerm },     // P | I | D | F | S(um)
    PidError { axis: Axis },                   // derived
    RcCommand { ch: u8 },
    Throttle,                                   // derived, 0..100 %
    Motor { idx: u8 },
    Erpm { idx: u8 },
    Debug { idx: u8 },                          // raw debug[n]
    Battery(BatteryField),
    Custom(Arc<str>),                           // everything the format could not map
}
```

The key idea: **a `fmt-*` crate maps the fields of a specific firmware to `ChannelId`**, and every analysis works with `ChannelId` only. For example, in Betaflight 4.x `gyroUnfilt[0]` → `Gyro{Roll, Raw}`, while in older versions the same data sits in `debug[0..2]` with `debug_mode=GYRO_SCALED`; that knowledge lives in the `FirmwareProfile`, not in the analyses.

### 4.2 Timebase

Blackbox does not guarantee a uniform step: there is jitter, dropped frames, `I`/`P` intervals, resync after corruption. The policy:

- The **actual** timestamps (`time` in µs, i64) of every frame are kept.
- `Timebase` holds the nominal step (from `looptime × pid_process_denom × P-interval`), the median actual step, the maximum jitter and the list of gaps.
- For the spectral analyses the series is **resampled** onto a uniform grid (linear interpolation within the tolerance; gaps longer than `gap_tolerance` are marked invalid and excluded from the segments). This happens once in `analysis::Prepared` and is cached.
- All of it is visible to the user on the Setup info tab ("Decoder": samples, sample step, gaps).

### 4.3 Settings

A typed structure with optional fields (`Option<T>`), filled in by the `FirmwareProfile`:

```rust
pub struct Settings {
    pub pid: PerAxis<PidGains>,          // P, I, D, D_min/D_max, F in the firmware's numbers
    pub pid_step: Option<f64>,           // their smallest step: 1 on Betaflight, 0.0001 on ArduPilot
    pub gyro_filters: FilterChain,       // lowpass1/2 (type, cutoff, dyn min/max), notch, dyn-notch, rpm-notch
    pub dterm_filters: FilterChain,
    pub rates: RatesProfile,
    pub motor: MotorConfig,              // protocol, poles, kv?
    pub loop_rates: LoopRates,           // gyro rate, pid rate, log rate
    pub extra: BTreeMap<String, String>, // everything not mapped
}
```

All mapping goes through explicit `header_name → setter` tables, so adding a parameter is a local change.

### 4.4 Extension traits

```rust
/// A log file format.
pub trait LogFormat: Send + Sync {
    fn id(&self) -> &'static str;                       // "blackbox-binary"
    fn sniff(&self, head: &[u8], name: &str) -> Confidence;
    fn open(&self, src: &dyn ByteSource, opts: &OpenOptions) -> Result<LogFile, FormatError>;
}

/// A firmware family (and its versions): which logs are its own, and the profile of one of them.
pub trait FirmwareProfile: Send + Sync {
    fn id(&self) -> &'static str;                       // "betaflight"
    fn score(&self, fw: &FirmwareInfo, headers: &RawHeaders) -> Option<u8>;
    /// Reads the firmware version and the headers once (ADR-0015, #144).
    fn bind<'a>(&self, fw: &'a FirmwareInfo, headers: &'a RawHeaders) -> Box<dyn BoundProfile + 'a>;
}

/// The profile of one log: field mapping, units, settings, what its firmware can be told.
pub trait BoundProfile {
    fn map_field(&self, field: &str) -> Option<FieldMapping>;        // channel, unit, scale, offset
    fn settings(&self) -> Settings;
    fn derive(&self, session: &mut FlightSession);      // throttle %, pid error, gyro stage, the sticks, …
    fn flight_modes(&self, word: i64) -> Vec<FlightMode>;            // a box mask or a mode number
    fn slider_model(&self) -> Option<Box<dyn SliderModel>>;          // for the PID slider tool
    fn exporter(&self) -> Option<Box<dyn Exporter>>;                 // proposals / a diff as the firmware's commands
}

/// A computation over a session. A pure function of (data, parameters).
pub trait Analysis: Send + Sync {
    type Params: Clone + Hash + Serialize + Default;
    type Output: Serialize;
    fn id(&self) -> &'static str;
    fn requires(&self) -> &'static [Requirement];      // which ChannelIds are needed
    fn run(&self, ctx: &AnalysisCtx, p: &Self::Params) -> Result<Self::Output, AnalysisError>;
}

/// Recommendations of setting changes (autotune and the like), phase 3.
pub trait Recommender: Send + Sync {
    fn id(&self) -> &'static str;
    fn recommend(&self, s: &FlightSession, results: &ResultStore) -> Vec<Proposal>;
}

/// Proposals and setting diffs as text for the firmware of one log; created
/// by that log's `FirmwareProfile`, so it knows the version's setting names.
pub trait Exporter: Send + Sync {
    fn id(&self) -> &'static str;                       // "betaflight-cli"
    fn label(&self) -> &'static str;                    // "Betaflight CLI", shown to the user
    fn export(&self, proposals: &[Proposal]) -> Result<String, ExportError>;
    fn diff(&self, diff: &[HeaderDiff]) -> String;
}
```

The registries (`FormatRegistry`, `ProfileRegistry`, `AnalysisRegistry`, …) are filled statically in `openpid-app`/`openpid-cli` through an own `inventory`-like mechanism (a plain `fn builtin() -> Vec<Box<dyn …>>`, no macros, no dynamic loading). Dynamic plugins (WASM / cdylib) are deliberately postponed: they add an ABI surface and dependencies; when needed they can be added later as one more `LogFormat`/`Analysis` adapter without changing the traits.

### 4.5 Why this is "OOP" in Rust

We follow the principles, not the inheritance syntax:

- **Dependency inversion**: the core depends on abstractions (traits), the concrete firmwares and formats depend on the core.
- **Open/closed**: new formats, analyses and recommenders are new types; existing code does not change.
- **Single responsibility**: a crate is one responsibility (format, mathematics, analysis, GUI).
- **Composition over inheritance**: the `FirmwareProfile` for EmuFlight/Rotorflight *delegates* to the Betaflight profile and overrides only the differences.
- **Types as documentation**: units, axes and filter stages are encoded in enums; a raw gyro cannot be passed where a filtered one is expected by accident.

---

## 5. The Blackbox decoder (`openpid-fmt-blackbox`)

An own implementation from the public specification (Betaflight "Blackbox Logging Internals"; the reference implementation is Blackbox Explorer's `flightlog_parser.js`, the test oracle of ADR-0005).

- **Input**: `&[u8]` (through `memmap2` for big files, or read whole), zero-copy, no intermediate strings.
- **Container**: one file → N logs, separated by the header marker `H Product:Blackbox flight data recorder by Nicholas Sherlock`. Supports `.BBL/.BFL/.TXT` and SD-card dumps with garbage between the logs.
- **Headers**: `H Field I name/signed/predictor/encoding`, `H Field P predictor/encoding`, `H Field S …`, `H Field G/H …`, `I interval`, `P interval`, `P ratio`, `Data version`, `minthrottle`, `vbatref`, `gyro_scale`, `motorOutput` and every other one are kept raw and handed to the `FirmwareProfile`.
- **Frames**: `I` (intra), `P` (inter), `S` (slow), `G`/`H` (GPS), `E` (events). Predictors 0–10, encodings 0–10 (SignedVB, UnsignedVB, Neg14, EliasDelta u/s, TAG8_8SVB, TAG2_3S32, TAG8_4S16, Null, TAG2_3SVariable). Every encoding is a separate function with property tests (an encoder in the tests → decoder → identity).
- **Robustness**: frame validation by the next byte (it must start a known frame), monotonicity checks of `loopIteration`/`time`, resync at the next `I` frame, corruption statistics in `Diagnostics`. Fuzzing of the decoder is mandatory because the input is untrusted (cargo-fuzz in the separate `fuzz/` package with format dictionaries, ADR-0013).
- **Events**: sync beep, inflight adjustment, logging resume, disarm, flight-mode flags → `Event`.
- **Output**: `i64` columns that become `Channel`s with `f64` values and units after the profile mapping (the scale is applied once, in `f64`, without intermediate rounding).
- **Writer** (`openpid_fmt_blackbox::write::betaflight`, #168, #182): the inverse path, a canonical session → the bytes of a Betaflight log of a chosen release. `WriteOptions::target` names the release: 4.2, 4.3, 4.4, 4.5, 2025.12 (the default: the calibration probes of #166 are written for it), 2026.6 and the unreleased `master` (2026.12-dev). Everything that differs between releases comes from one description per release, read from the firmware source at the release's tag and never guessed: the fields of the `I`/`P`, `G`, `H` and `S` frames with predictor, encoding and the condition each is logged on, and the bits of `fields_disabled_mask` (`write/version.rs`); the system-information header lines in the firmware's order with the build option each depends on (`write/sysinfo/lines.txt`: one table for all releases, each line written once with the releases that write it; `write/sysinfo/crafts.txt`: the values of the example craft of each release). The table of differences is in `docs/BLACKBOX-VERSIONS.md`.
  - **Values.** The channels go back through the decoder's own field mapping, inverted, with the scales of the headers written, so what the reader makes of a log is what the writer takes back: gyro and setpoint in whole deg/s (tenths with `blackbox_high_resolution:1`), `rcCommand` in the units of the release (±500 and 1000…2000, or thousandths of full deflection with `rc_command_scale` on `master`), throttle in tenths of a percent, motors in DShot units between the `motorOutput` limits, eRPM / 100, the battery in centi-units against a `vbatref` that is the voltage of the first frame, the accelerometer in counts of `acc_1G`, the attitude quaternion × 32767. Fields without a `ChannelId` of their own are the `Custom(<field name>)` channels the reader makes of them (`magADC[i]`, `baroAlt`, `surfaceRaw`, `servo[i]`, `pitot[i]`, `imuQuaternion[i]`, `setpoint[3]`). Each value is limited to the integer type the firmware keeps it in; the time alone wraps, as the firmware's 32-bit clock does after 71.6 minutes.
  - **The headers decide what is logged**, as on a flight controller: a field is in the frames when the release's condition holds for the configuration the headers describe (`fields_disabled_mask`, `debug_mode`, the D and S gains, `dshot_bidir`, the GPS feature). The session supplies the values. Where it has none for a group the configuration would log, the group is switched off in `fields_disabled_mask` (unfiltered gyro, debug values of a set debug mode, eRPM with telemetry on, stick commands, setpoints, motors), or the craft is one without that sensor and loses its header lines (no voltage: no `vbat_scale`/`vbatcellvoltage`/`vbatref`; no current: no `currentSensor`); nothing is filled in with made-up readings. The PID terms alone are written as zero when the session has none, which is what the firmware logs while the controller is idle. The header lines come, later ones winning, from the example craft of the release (a stock quadcopter build; a wing build when the session has an S term), from the headers of a session decoded from a Betaflight log, from the typed settings, and from the caller's overrides.
  - **Frames** in the firmware's order and by its rules (`blackboxLogIteration`, the same from 4.2 to `master`): an `I` frame at every multiple of the `I interval` and a `P` frame every `P interval` iterations counted from it (so P intervals that do not divide the I interval, the default on 3.2 kHz gyros, and `P interval:0` logs of `I` frames only, are written as the firmware writes them); `I interval` and `looptime` rounded as the firmware rounds them (whole microseconds, rounded down, 32 ms of PID loops, rounded down); a slow frame before the first `P` frame, on a change of the flight modes, after a pause and every 256 `I` intervals; the `FLIGHTMODE` event with it; `G` and `H` frames from the session's GPS tables, predicted from the last home frame and the time of the last main frame; the arming beep, in-flight adjustments, "logging resumed", disarm and end-of-log events. A sample without a gyro value pauses the log, which resumes on the next `I` frame.
  - **Two kinds of session.** One decoded from a Blackbox log is written back as that log had it (its loop counter, its events, its slow, GPS and home frames, its own header list), and for the release that logged it the result is the log, header line for header line and byte for byte (`tests/rewrite.rs`: the 2025.12.2 and 4.5.1 fixtures, 60 000 frames of all six kinds, and 46 logs of the 4.2 fixtures). Written for another release it becomes a log of that release: the craft and the flight are kept, the fields, units, mask bits and mode boxes are the target's. Any other session (`openpid gen`, the probes) is logged as a flight by the rules above.
  - **Checks**: the firmware-derived byte vectors of every encoder and event; the field tables against the lines extracted from the firmware (`write/field-tables.txt`); the header tables against the header lists of the release-build fixtures; round trips through the reader for every field, frame and event kind at five schedules (P interval 4, 1, one that does not divide the I interval, `I` frames only, P = I) for every release, in normal and high resolution (`tests/write_releases.rs`); the independent decoder of ADR-0005 on the main frames (`tests/write.rs`, `tests/write_releases.rs`). Not modelled: the storage layer (`blackbox_io.c`: padding between logs, behaviour on overrun), the names of the debug modes of each release (a session keeps its debug mode number), and the conversion of the D-min scheme of 4.5 into the D-max scheme of 2025.12 when a log changes release.

Profiles (`openpid-fmt-blackbox::profiles`): `betaflight` (4.0–4.6+, the various debug modes), `emuflight`, `rotorflight` (delegating to BF with overrides), `inav` (other gyro scales, other names). The profile is chosen by `Firmware revision`, because `Firmware type` is always `Cleanflight`.

The separate crate `openpid-fmt-csv` reads CSV exports in the `blackbox_decode` column layout, as a fallback for exotic logs; `openpid decode` writes the same layout.

---

## 6. Mathematics (`openpid-dsp`, `openpid-analysis`)

### 6.1 Number policy

- Every series is `f64`. Raw counters (`loopIteration`, `time`) stay `i64`/`u64` until the last moment.
- `Series` = `Vec<f64>` + a validity `BitMask`; operations go through the mask, no NaN sentinels.
- FFT: `rustfft` (pure Rust, deterministic, MIT/Apache). The `dsp::fft::Planner` wrapper caches plans by length. Lengths are arbitrary (rustfft does not need powers of two), but for speed the standard segments use the nearest "smooth" number (2^a·3^b·5^c).
- Windows: Hann (the default, as in PTB), Hamming, Blackman-Harris, Tukey(α). The window normalisation (coherent gain / ENBW) is applied **explicitly** in the PSD, so amplitudes are comparable between segment lengths.
- Every `dsp` function has a test against a reference generated by `tools/reference/*.py` (NumPy/SciPy) and stored as a fixture; the tolerance is a relative error ≤ 1e-9 for linear operations and ≤ 1e-6 for iterative ones.
- Also property tests (Parseval for the PSD, linearity of the deconvolution, impulse → step through cumsum).

### 6.2 Step response

The method is **Wiener deconvolution** from the input (setpoint) to the output (filtered gyro), as in the modern PTB and PID-Analyzer, not the "end of manoeuvre" heuristic of the old PTB.

1. The epoch (the user's time range) is cut into windows of length `L` (default 2 s) with a hop of `L/superpos` (superpos = 16 → 125 ms).
2. Every window: `x = setpoint·w`, `y = gyro·w` (Hann), zero-padded to a smooth length.
3. `H = Y·conj(X) / (|X|² + λ(f))`, where `λ(f)` is the regularisation: `reg_low` (0.1) below `cutoff` (default 25 Hz), `reg_high` (1e8) above, with a raised-cosine transition of ±20 % on a log scale between them. Both signals are multiplied by Hann before the FFT: without it the unobserved tail of the response beyond the record understates the gain (≈ 10 % for a 4 s record and τ = 20 ms, checked on a synthetic model). The FFT length is a 5-smooth number ≥ 1.5·len so the circular convolution does not wrap. The impulse response is `h = IFFT(H)`, the step response `s = cumsum(h)` over the first 500 ms.
4. Window selection: `max|setpoint| ≥ threshold` (default **80 °/s**, with separate "low/high input" as in PID-Analyzer at 500 °/s), windows with gaps or corruption excluded, and windows whose response **does not settle** between 0.5 and 1.5 (mean of the last quarter, `settle_range`) dropped as failed deconvolutions; the output counts them (`windows_rejected`). Both values are calibrated against PIDtoolbox Pro v0.9 (#33, table below). The earlier 20 °/s took every gentle stick movement, and a few windows whose deconvolution failed pulled the weighted mean: on `btfl_027` the pitch curve ended at 1.26 and the yaw curve, with the automatic trim, at 2.7.
5. Aggregation: a weighted mean (weight = input amplitude) **and** a 2D histogram (time × amplitude) to show the spread; optional Y-offset correction (normalising on the settled tail value) and smoothing (PTB "smoothing 1/20/40/60").
6. Metrics: rise time (10–90 %), overshoot %, settling time (±5 %), delay (the first crossing of 50 % of the mean curve) and **latency**: the lag of the cross-correlation peak of setpoint ↔ gyro on the raw signals of the epoch (`openpid_dsp::xcorr::lag_of_max`, searched up to 100 ms, parabolic peak refinement). For a white input this is the transport delay of the axis; with smooth stick input the peak also catches part of the plant's own lag (on real logs 10–17 ms against a 50 % delay of 18–22 ms), so the value is read next to delay. A "whitened" variant through first differences was rejected: on real logs the gyro noise dominates the derivative (r ≈ 0.1) and the peak jumps between 4 and 37 ms depending on the smoothing. Both values are shown side by side: delay depends on the filters and PIDs, latency is the transport delay of the axis. Y-offset: every window is normalised on the mean of the last quarter of its response when that is > 0.2 (otherwise the window is left as is).

Parameters (`StepParams`): `window_s, superpos, cutoff_hz, min_input, settle_range, high_input, resp_len_s, y_offset, smoothing, axis`.

**Calibration against PIDtoolbox Pro v0.9** (#33; the owner's screenshots of the same logs; PTB leaves out the first and the last second of the log, so the comparison uses that epoch; `step_response_matches_the_reference_tool` in `crates/openpid-analysis/tests/real_logs.rs` keeps it). PTB's selection rule itself is not public: the only public PTB code (2019, GPLv3) still uses the old "end of manoeuvre" heuristic and no deconvolution, so the rule was matched on the results, not read.

| Log, axis | PTB windows / peak / latency | OpenPID windows / peak / 50 % |
|---|---|---|
| `betaflight-2025.12.2-btfl_027`, roll | 78 / 1.15 / 22 ms | 65 / 1.14 / 22 ms |
| `btfl_027`, pitch | 13 / 1.12 / 22.5 ms | 14 / 1.14 / 21 ms |
| `btfl_027`, yaw | 30 / 1.18 / 12.3 ms | 27 / 1.24 / 12 ms |
| `betaflight-4.5.1-two-sessions` (21 s session), roll / pitch | insufficient data | not enough data |
| same, yaw | 32 / 0.87 / 11.7 ms | 27 / 0.88 / 12 ms |

PTB's "SR Latency" is the time the mean response takes to reach 50 % (our *delay*); PTB's other latency option, "SP-Gyro Delay" ("Xcorr Latency" in v0.8), most likely corresponds to our cross-correlation *latency*. With the old defaults (20 °/s, no settle check) the same logs gave 172/133/32 windows and peaks of 1.26/1.35/1.24 on `btfl_027`. A log with only gentle stick inputs (`betaflight-4.2.0.BFL`: 48 windows above 20 °/s, none above 80) now gives "not enough data", as PTB does; the threshold is a step-tab control for such logs.

### 6.3 2D spectral analyzer (PSD)

Welch: `nfft` segments (default 1024 at 4 kHz; parameterised in seconds), 50 % overlap, Hann, averaged periodograms, units `(°/s)²/Hz` with a dB or linear option. Channels: gyro raw/filtered, D-term, PID error, setpoint, motor, debug[n]. A/B comparison on one axis.

### 6.4 Freq × throttle heatmap

As the PTB `PTthrSpec`, without the MATLAB artifacts:

1. Segments of 200 ms (a parameter), Hann, FFT, the one-sided amplitude `|Y|/N·2`.
2. The mean throttle (%) of every segment.
3. For every throttle bin `t ∈ 0..100` the spectra of the segments with `|thr − t| ≤ half_window` (default 6 %) are averaged: a sliding window over throttle.
4. 2D box smoothing (parameters `thr_smooth=8`, `freq_smooth=segment_len/100`), cut at `max_freq`.
5. The result is `Grid2D { x: throttle, y: freq, z: amp }` plus the number of segments per bin (shown so that empty bins do not look like silence).

### 6.5 Freq × time heatmap (STFT)

An STFT with `nfft`, hop, Hann → `Grid2D { x: time, y: freq, z: amp/dB }`. It is also the source of the "Dynamic spectrum player" (phase 2): the player simply walks the columns.

### 6.6 Filters and overlays (phase 2)

- `openpid-sim` (ADR-0012): PT1/PT2/PT3, biquad LPF/notch (Betaflight/RBJ formulas) as state machines `update(x)` with an analytic `response(f)`, assembled into a `Chain`. All in `f64`, with tests of the frequency response against the analytic formulas and of the consistency of `update` ↔ `response`.
- `ChainModel::from_settings` builds the gyro or D-term chain from the typed `Settings` alone (the firmware profile fills the dynamic low-pass curve, the RPM-notch fade, weights and smoothing and the dynamic-notch count, with the version's defaults; ADR-0015) and runs the signal under a `Drive` (throttle, motor rpm, dynamic-notch centres): lpf1 with a throttle-dependent cutoff `min + (max−min)·(t(1−t)·expo + t)` recomputed every 5 ms in 1 % throttle steps; an RPM notch on the harmonics `k·rpm/60` with a smoothed frequency, a fade-in above `min_hz`, harmonic weights and skipping of harmonics above 0.48·fs; a dynamic notch that tracks the strongest STFT peaks (≈ 100 ms, 16 ms hop, PT1 4 Hz) in the `min..max` band. The order: RPM → static notches → lpf1 → lpf2 → dynamic notch. Limits of the model: it runs at the log rate, not the gyro rate (at 1 kHz only the band up to 500 Hz remains, higher RPM harmonics are not modelled), the dynamic-notch tracker approximates the firmware's SDFT, the expo-0 curve for < 4.2 is linear. On the Betaflight 2025.12 fixture (`gyroUnfilt` in the log) the residual `gyro.sim − gyro.filtered` is 5–13 % RMS against 27–75 % without filters (a test in the crate).
- **Filter Sim** (`FilterSimAnalysis`, the Filter Sim tab, `openpid run filter-sim --set …`): `gyro.raw → ChainModel(settings) → gyro.sim`, compared with the log's `gyro.filtered` (validation metrics: residual in % RMS, delay by cross-correlation) and with a "what-if" chain (`FilterOverrides`: LPF1/LPF2 cutoffs and types, the dynamic range, RPM and dynamic notches, a static notch; 0 switches a stage off). Outputs: both series over time, PSD raw/logged/sim/as-flown, a description of the stages. It needs the unfiltered gyro in the log (`gyroUnfilt`, `debug_mode GYRO_SCALED`, the ArduPilot batch sampler).
- **RPM / dyn-notch overlay** (`RpmOverlay` in the freq × throttle output): from `motor_rpm.*` (eRPM and `motor_poles`), the median rpm of every motor per throttle bin (the same segments and window as the spectra); the GUI draws the harmonics `k·rpm/60` (toggles per motor, number of harmonics, by default as many as the RPM filter removes), the static notch bands, the dynamic-notch band and the RPM-filter minimum; the CLI JSON always has `overlay` when rpm exists. Checked on the 4.5.1 fixture: the fundamental line sits on the ridge of the unfiltered-gyro spectrum in 93 % of the bins.
- **RPM estimator** (`RpmEstimator`, `openpid run rpm`): without eRPM, the candidates are the strongest STFT peaks of the unfiltered gyro in the `rpm_min..rpm_max` band (as the 1st…3rd harmonic), dynamic programming picks a path continuous in time (penalties for a relative jump and for a higher harmonic), the confidence is the peak's excess over the band median. With four motors at similar speed the estimate follows their mean: a median error of 2.9 % on the 4.5.1 fixture and 2.0 % on 2025.12 (against the logged eRPM, reported as `median_error`). `gyro.filtered` is unusable: the RPM filter has already removed the peaks.
- **Motor noise** (`MotorNoiseAnalysis`, `openpid run motor-noise`; no tab): per motor, the gyro amplitude at its harmonics `k·rpm/60` in every STFT frame (averaged), the ratio to the mean over the motors, the sum; the speeds come from eRPM or the RPM estimator (then one row for all motors). A "bent prop" shows as a row with a ratio > 1 and the loudest motor highlighted; checked on the generator's synthetic data with one motor amplified 2.5×.

### 6.7 Defaults against the reference tool (#33)

What the reference tool shows by default, from the owner's PIDtoolbox Pro v0.9 screenshots (#33) and its v0.8 tutorial video, against what OpenPID does and why. "Not recorded" means neither source shows the value; nothing is guessed, and no code of the reference tool was read.

| Screen | Setting | Reference tool | OpenPID default | Why |
|---|---|---|---|---|
| Step | Window selection | rule not public; on `btfl_027` 78 / 13 / 30 windows (roll / pitch / yaw) | stick input ≥ 80 °/s and a response that settles between 0.5 and 1.5: 65 / 14 / 27 windows | matched on the results (§6.2); 20 °/s gave 172 / 133 / 32 and curves that ended off 1 |
| Step | Part of the log analysed | leaves out the first and the last second | the automatic trim of take-off and landing ("Auto"), or the whole log ("All") | the trim follows the throttle and the motor rpm, so a long idle before take-off is left out too; the calibration of §6.2 used the reference tool's epoch |
| Step | Latency shown | "SR Latency": the time to 50 % of the mean response; "SP-Gyro Delay" as the other option | the same two, "SR latency" first | same quantity, same name |
| Step | Raw responses | "Raw", off | "Raw" (the response histogram), off | the mean is what is read; the spread is one click away |
| Step | Y axis | not recorded | 0 … 1.5, fixed ("Y max") | a fixed axis makes two logs comparable by eye |
| Step | Smoothing | several levels; the default is not recorded | off (0 ms); low / medium / high = 10 / 20 / 40 ms | smoothing hides a little of the overshoot the verdicts measure, so it is opt-in |
| Step | Y correction | a control exists; the default is not recorded | off | it rescales every window to end at 1, which would hide a response that really settles off 1 |
| Spectrum | Layout | roll, pitch, yaw, each with a "Sub 100Hz" panel | the same: three rows × (full range, below 100 Hz) | the low range is where propwash and frame resonances are |
| Spectrum | Unfiltered gyro | gyro solid, gyro prefilt dotted | the same, on | the effect of the filters is on screen without a second plot |
| Spectrum | Smoothing | medium | medium (a moving average over 8 Hz of the power) | same |
| Spectrum | Window | Hann | Hann; 0.25 s segments, 1 s for the column below 100 Hz | the longer segment gives 1 Hz bins where 4 Hz bins leave 25 points |
| Spectrum | Level | its own dB scale | (°/s)²/Hz in dB, the range automatic | the absolute levels of the two tools differ, so a copied Y range would cut our curves |
| Freq × throttle | Columns | gyro prefilt beside gyro (its preset 1) | the same, per log | before and after the filters side by side |
| Freq × throttle | Throttle axis | 0–100 % | 0–100 %; "Zoom to the flown range" as an option | a zoomed axis stretched 25 bins into blocks |
| Freq × throttle | Lines over the maps | none from the data; fixed 20 / 60 Hz guides in its "<100Hz" view | none; "RPM overlay" and "Notches" as options | the owner: a line from the data reads as part of the map |
| Freq × throttle | Colour map | MATLAB "hot" (black, red, yellow, white) | Blackbox Explorer's black, red, white; a colour-blind safe option | the owner's choice (docs/design/plot-colours) |
| Freq × throttle | Colour limit | not recorded | automatic, one scale per log | two logs of different noise each keep their contrast |
| Freq × time | Layout and channel | roll, pitch, yaw spectrograms of the unfiltered gyro, in dB | the same; the spectrum at the position and the throttle are added | what the noise did when, without leaving the tab |
| Freq × time | Colour range | Z min −30, Z max 10 dB | automatic (the 30th and 99.5th percentile of the dB values above 20 Hz); Z min / Z max can be fixed | the dB levels differ, as on Spectrum |
| Log viewer | Rows | roll, pitch, yaw at ± 500 °/s, throttle and motors in a row of their own | the same; "Y scale ±" in the panel | same |
| Log viewer | Quantities shown | Gyro, Set point, Motors, Throttle | the same | same |
| Log viewer | Sticks | two boxes beside the plots | a card on the throttle / motors row, with the flight modes | the owner's choice (docs/design/log-sticks) |
| Log viewer | Flight-mode changes | a dashed line through the panels, the names under the throttle row | the same, as an option that is off | the owner: off by default |
| Sliders | Slider tool | start gains and five ratios (P:D, P:I, roll:pitch, yaw, master) | the firmware's own sliders when the log has them, else the same five ratios over the logged gains | Betaflight's sliders give commands to paste; the ratios keep every other firmware from a dead end |

### 6.8 Cache and incrementality

`ResultStore` caches an `Output` by the key `(session_id, epoch, analysis_id, hash(params))`. Heavy computations run on a worker thread (`std::thread` plus a channel; `rayon` only if measurements show the need); the GUI receives progress and never blocks.

---

## 7. GUI (`openpid-app`)

### 7.1 Technology

**egui + eframe (wgpu backend) + egui_plot**, ADR-0002. Pure Rust, one binary per platform (macOS/Windows/Linux), GPU rendering, built-in multi-touch (`MultiTouchInfo`: pinch zoom, two-finger pan, rotation), an active community, stable releases.

Alternatives considered: `iced` 0.14 (Elm architecture, fewer ready plot components), `slint` 1.18 (its own markup DSL, licence GPLv3 / royalty-free with restrictions / commercial), `tauri` (a web stack, against P1).

### 7.2 Application structure

```
AppState
├─ Workspace { logs: [LoadedLog], active: {A, B}, epoch: {A, B} }
├─ ResultStore (analysis cache)
├─ ViewState (current tab, the parameters of every tool, zoom/pan)
└─ Prefs (theme, touch mode, units, language)

Views (each a struct implementing trait View { fn ui(&mut self, ctx, state) })
├─ LogViewer      — time plots, epoch selection, A/B cursor sync
├─ StepResponse   — curves, metrics, spread histogram
├─ Spectral2D     — PSD, A/B overlay, frequency markers
├─ HeatmapThr     — freq × throttle, notch/RPM overlay
├─ HeatmapTime    — freq × time, player (phase 2)
├─ SliderTool     — PID slider ↔ gains
├─ FilterSim      — (phase 2)
├─ SetupInfo      — log check, frame statistics, setup and headers of every log side by side
└─ Settings
```

- A heatmap is rendered into a texture (`ColorImage` → `TextureHandle`) once per data change; axes and overlays are drawn on top through `egui_plot`. That keeps 60 fps even on 100 × 1000 grids.
- Every tool parameter is a plain `struct`, serialised into a "project" (`.openpid.json`, the `openpid_analysis::Project` v1 schema with a default for every field): logs with paths relative to the project file, the session and epoch of every slot, the active tab, touch mode, every tool parameter. Project → Open / Save as, drag-and-drop of a `.openpid.json`, or a command-line argument; `openpid run <analysis> --project p.openpid.json` takes the log of slot A, its session, epoch and parameters from the project (the `--axis/--channel/--epoch` flags win).
- **Touch mode**: switched on automatically at the first touch event or by hand; it enlarges the controls (≥ 44 pt) and adds gestures: pinch zooms an axis (egui classifies a horizontal or vertical pinch, egui_plot scales the matching axis), two fingers pan, a long press (≥ 0.6 s without moving > 8 px) places the measuring cursor in the log viewer, a double tap resets the zoom. The channel list of the log viewer is height-limited and scrolls, so in touch mode it does not push the plot away.
- Themes: dark (the default, PTB-like) and light; UA/EN localisation through a simple string table.
- **Settings** (phase 1): theme, touch mode and units (deg/s ↔ rad/s, g ↔ m/s²) are saved in `prefs.json` in the platform's configuration directory (`~/.config/openpid`, `~/Library/Application Support/openpid`, `%APPDATA%\openpid`; override with `OPENPID_CONFIG_DIR`). Unknown fields are ignored, missing ones take their defaults, so versions can share the file. The units apply to the log-viewer traces; the analyses stay in deg/s.

### 7.3 The user's workflow

1. Open a file (the `rfd` dialog or drag-and-drop) → the list of sessions in the file → pick one.
2. Log viewer: see the whole flight, pick the epoch (in PTB the "start/end sec" fields; here also by dragging the bounds on the plot).
3. Go to a tool; parameters change and the result is recomputed in the background.
4. Load log B, compare.
5. Export: a PNG of the plot, CSV/JSON of the results, (phase 3) a settings diff.

---

## 8. CLI (`openpid-cli`)

```
openpid info  <log>                      # sessions, firmware, diagnostics
openpid decode <log> --session 1 --csv   # blackbox_decode-compatible CSV
openpid run step <log> --axis roll --epoch 5..60 --json
openpid run psd  <log> --channel gyro.filtered.roll
openpid run thr-heatmap <log> --png out.png
```

`openpid gen [<spec.json>] [--out synthetic.csv]` (`openpid-sim::SignalSpec`): a synthetic session with known ground truth: a throttle profile (constant / ramp / sine), stick input (steps / chirp / sine), a first-order plant with delay, motor harmonics tied to the throttle, white/pink noise and tones, a PT1 "firmware" filter, jitter and dropouts. Written as a scaled CSV with the canonical column keys, which `openpid` opens like any log (`gen --example` prints the default spec), or, with `--out <name>.bbl`, as a Betaflight Blackbox log through the writer of §5 (`--firmware` names the release whose format is written), for any tool that reads such logs. The generator tests of the analyses (`crates/openpid-analysis/tests/synthetic.rs`) are built on it.

The CLI uses the same registries as the GUI and is the basis of the golden tests in CI (the output is compared with `tests/golden/*.json`).

---

## 9. Autotune / device integration (phase 3, the architectural groundwork)

- A `Recommender` receives a `FlightSession` plus the analysis results and returns `Vec<Proposal>` (`{ path: "pid.roll.d", from, to, rationale, confidence }`).
- `Exporter("betaflight-cli")` turns `Proposal`s into `set …` commands / a diff.
- Implemented (v1.0): `openpid-core::recommend` (`Proposal`, `Exporter`, `diff_headers`), `openpid-analysis::recommend::Recommend` with three rules and explicit evidence (overshoot → D, an uncovered peak → notch, headroom above the gyro LPF → cutoff), `openpid-fmt-blackbox::BetaflightCli`, `openpid diff` and `openpid run recommend --cli`.
- `FormatRegistry::slider_model` falls back to `openpid_core::RatioSliders` (firmware-agnostic: master, P:D, P:I, roll:pitch and yaw ratios over the logged gains, rounded to the firmware's gain step, no commands) when the profile has no slider model, so a log with PID gains always has sliders.
- The exporter and the slider model of a log are asked from its firmware profile through the format registry (`FormatRegistry::exporter(&session)`, `slider_model(&session)`; ADR-0015): the app and the CLI name no firmware. Betaflight (and a log that does not name its firmware) has the CLI exporter; INAV, Rotorflight, `EmuFlight` and `ArduPilot` have none yet, so the Advice tab shows their proposals without commands and `--cli` reports that there is no export.
- Later: a `Device` trait (MSP over serial) to read and write settings and to download logs from the FC. The decision is ADR-0014: no device access in v1.0; later a separate `openpid-msp` crate behind the `device` feature on `serial2` (pure Rust), writes only with confirmation and a read-back.

None of these parts needs a change in `core`: all of them are new implementations of existing traits.

---

## 10. Dependencies (the initial list, ADR-0001)

| Crate | Why | Why not our own |
|-------|-----|-----------------|
| `eframe`/`egui`/`egui_plot` | GUI, windows, GPU | An own UI toolkit is years of work |
| `wgpu`, `winit` | transitively through eframe | the same |
| `rustfft` | FFT | an own FFT is possible, but rustfft is reference-validated, pure Rust, SIMD; the risk of mistakes in our own outweighs the benefit |
| `serde` + `serde_json` | projects, golden tests, export | the de-facto standard, pure Rust |
| ~~`memmap2`~~ | zero-copy of big logs | not needed: logs are read whole into memory (audit 2026-11-05, removed from the workspace) |
| `rfd` | native file dialogs | platform APIs |
| `thiserror` | error types | only a derive macro; possible without it |
| `clap` (CLI only) | argument parsing | optional; replaceable by a hand-written parser |

Dev only: `proptest`, `criterion`, `insta` (snapshots); `cargo-fuzz` with `libfuzzer-sys` only in the separate `fuzz/` package (ADR-0013). Everything else is added only through an ADR. `cargo deny` checks licences (allow: MIT/Apache-2.0/BSD/Zlib/MPL-2.0, aligned with Q1) and duplicates.

---

## 11. Quality and testing

- **Unit**: the blackbox encodings (round trip), predictors, windows, the FFT wrapper, filters (frequency response against the analytic formulas).
- **Reference**: `tools/reference/*.py` generates fixtures (JSON) for step / PSD / heatmap on synthetic signals and on the reference logs; the Rust tests compare with tolerances.
- **Conformance**: our decoder against the parser of Blackbox Explorer on `tests/logs/*` (every flight log in full: every frame, table and event; ADR-0005).
- **Golden**: the CLI output on the reference logs is pinned in `tests/golden`; an algorithm change is a deliberate golden update described in the PR.
- **Fuzz**: every decoder entry point with cargo-fuzz / libFuzzer (`fuzz/`, ADR-0013): weekly with a time budget and for a minute on decoder PRs (`fuzz.yml`), with dictionaries of the format tokens.
- **Bench**: `criterion` for the decoder (MB/s) and for step / heatmap (ms per 60 s of log at 8 kHz).
- **CI**: a GitHub Actions matrix macOS/Windows/Linux: `fmt`, `clippy -D warnings`, tests, `cargo deny`, release binaries. Performance targets: an 8 kHz × 3 min log decodes in < 1 s, every phase-1 analysis in < 2 s on a laptop.
- **Lints**: `#![deny(unsafe_code)]` in every crate except `fmt-blackbox` (mmap) with documented `unsafe` blocks; `clippy::pedantic` selectively.

---

## 12. Phase plan

| Phase | Content | Done when |
|-------|---------|-----------|
| 0 — Foundation ✅ | workspace, the `core` model and traits, `fmt-blackbox` (BF/Emu/RF), `fmt-csv`, `cli info/decode`, conformance and robustness tests, CI | Conformance with the independent `blackbox-log` decoder on every test log; since 2026-11-09 also whole-log conformance with Blackbox Explorer (#137) |
| 1 — PTB (free) parity ✅ | the `dsp` basis, step, PSD, heatmaps, a GUI with 7 tabs, A/B, touch, the PID slider tool (Betaflight simplified tuning, checked on 4.5 and 2025.12 logs). Audit 2026-11-05, moved to the backlog (milestone v0.4 unless stated otherwise): projects (#31), PNG/CSV export (#32), PTB default comparison (#33), touch gestures §7.2 (#35), theme / localisation (#34), the Settings tab and saved Prefs (#53), frequency markers in the spectrum (#51), dragging the epoch bounds on the plot (#52), the segment counter on the heatmap (#54), Y-offset / latency in the step response (#50), deconvolution property tests (#55, v0.3), `blackbox_decode` CSV fixtures from phase 0 (#49, v0.2; replaced by the Blackbox Explorer reference, #137) | The user reproduces the whole usual PTB workflow in OpenPID |
| 2 — Pro parity ✅ | filters / sim (ADR-0012), notch / RPM overlay 2D (a 3D surface tab was built and removed on 2026-11-10, §2), the RPM estimator, the generator (`openpid gen`), motor noise (in the CLI; its tab was removed on 2026-11-10), the spectrum player, benchmarks (#36), fuzzing (ADR-0013), touch gestures, UA/EN localisation, the user guide; the ArduPilot (ADR-0010) and INAV formats done, KISS/Fettec/QuickSilver wait for sample logs (#18) | Every row of the §2 table is "yes" (except the formats without samples) |
| 3 — Extensions ✅ | a `Recommender` with three rules and evidence, the Advice tab, `openpid run recommend --cli`, `openpid diff`, the Betaflight CLI exporter; MSP postponed (ADR-0014) | The first recommendation a user really applies: the `set …` text from the Advice tab |

---

## 13. Risks

| Risk | Mitigation |
|------|------------|
| Format details between firmware versions (field names, scales, debug modes) | Profiles plus conformance with Blackbox Explorer; collect a library of test logs from the community |
| No public specification for KISS/Fettec/QuickSilver/ArduPilot | Separate `fmt-*` crates, each after samples arrive; ArduPilot (the self-describing DataFlash `FMT`) is the best documented |
| Our plots differ from PTB "by eye" because of other smoothing parameters | The parameters are in the UI with PTB-compatible defaults; the difference is documented |
| egui performance on large point arrays | Decimation (min/max per pixel) for line plots, textures for heatmaps |
| Licence cleanliness | No code copied from GPL sources; only own implementations from the algorithm descriptions |

---

## 14. Open questions

See `docs/OPEN-QUESTIONS.md`. The answers to Q1–Q5 were needed before phase 0.
