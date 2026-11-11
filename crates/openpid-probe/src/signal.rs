//! Signals whose content is known exactly, and the flight they add up to.
//!
//! A probe has to say what is true about its log before any tool looks at
//! it. So nothing here is simulated step by step: a tone is evaluated from
//! its formula at every sample time, and the response of the craft to a
//! stick step is the closed-form step response of the plant, summed over the
//! steps. The values are the same at every log rate, and a test can compute
//! them a second way.
//!
//! The three axes of a flight are independent, so one log carries three
//! experiments.

use openpid_core::session::{Channel, ChannelSet, Diagnostics, FirmwareInfo, Origin, Timebase};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage, Series, Settings, Unit};
use openpid_dsp::{Complex, Fft};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// Times closer than this count as equal, seconds: sample times and step
/// times are both whole microseconds in every probe.
const SAME_TIME_S: f64 = 1.0e-9;
/// A plant counts as settled after this many of its slowest time constants
/// (`e^-40` is below the resolution of an `f64` next to 1).
const SETTLED_AFTER: f64 = 40.0;

/// Deterministic uniform numbers in `[0, 1)`: probes must not change from
/// run to run or with the toolchain.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal (Box–Muller).
    fn normal(&mut self) -> f64 {
        let u1 = self.next().max(1e-300);
        let u2 = self.next();
        (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
    }
}

/// One component of a gyro signal, deg/s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Wave {
    /// `amplitude · sin(2π · hz · t + phase)`, between `from_s` and `to_s`
    /// when they are given (the whole flight otherwise).
    Tone {
        /// Frequency, Hz.
        hz: f64,
        /// Peak amplitude, deg/s.
        amplitude: f64,
        /// Phase at `t = 0`, degrees.
        #[serde(default)]
        phase_deg: f64,
        /// The tone starts here, seconds.
        #[serde(default)]
        from_s: Option<f64>,
        /// The tone ends here, seconds.
        #[serde(default)]
        to_s: Option<f64>,
    },
    /// White Gaussian noise of the given RMS.
    Noise {
        /// Root mean square, deg/s.
        rms: f64,
        /// Seed of the generator.
        seed: u64,
    },
    /// Noise that is flat between two frequencies and absent outside: every
    /// frequency of the band that fits the flight a whole number of times,
    /// with equal amplitude and a random phase. Its power is exactly `rms²`.
    Band {
        /// Lower edge, Hz.
        from_hz: f64,
        /// Upper edge, Hz.
        to_hz: f64,
        /// Root mean square, deg/s.
        rms: f64,
        /// Seed of the phases.
        seed: u64,
    },
    /// A constant.
    Offset {
        /// deg/s.
        value: f64,
    },
    /// A straight line from the first sample to the end of the flight.
    Ramp {
        /// Value at the start, deg/s.
        from: f64,
        /// Value at the end, deg/s.
        to: f64,
    },
    /// A sine whose frequency rises linearly over the whole flight.
    Chirp {
        /// Frequency at the start, Hz.
        from_hz: f64,
        /// Frequency at the end, Hz.
        to_hz: f64,
        /// Peak amplitude, deg/s.
        amplitude: f64,
    },
    /// A sine whose frequency follows the throttle, as motor noise does:
    /// `hz_at_zero + hz_per_percent · throttle`. With a `band` it sounds
    /// only while the throttle is inside it.
    ThrottleTone {
        /// Frequency at zero throttle, Hz.
        hz_at_zero: f64,
        /// Hz per percent of throttle.
        hz_per_percent: f64,
        /// Peak amplitude, deg/s.
        amplitude: f64,
        /// Lowest and highest throttle at which it sounds, percent.
        #[serde(default)]
        band: Option<(f64, f64)>,
    },
}

impl Wave {
    /// A tone over the whole flight with zero phase.
    pub fn tone(hz: f64, amplitude: f64) -> Wave {
        Wave::Tone {
            hz,
            amplitude,
            phase_deg: 0.0,
            from_s: None,
            to_s: None,
        }
    }

    /// The samples of the component on `timeline`.
    fn render(&self, timeline: &Timeline<'_>) -> Vec<f64> {
        let n = timeline.throttle.len();
        let fs = timeline.fs;
        let time = |i: usize| i as f64 / fs;
        match self {
            Wave::Tone {
                hz,
                amplitude,
                phase_deg,
                from_s,
                to_s,
            } => {
                let from = from_s.unwrap_or(f64::NEG_INFINITY) - SAME_TIME_S;
                let to = to_s.unwrap_or(f64::INFINITY) - SAME_TIME_S;
                let phase = phase_deg.to_radians();
                (0..n)
                    .map(|i| {
                        let t = time(i);
                        if t >= from && t < to {
                            amplitude * (TAU * hz * t + phase).sin()
                        } else {
                            0.0
                        }
                    })
                    .collect()
            }
            Wave::Noise { rms, seed } => {
                let mut rng = Rng::new(*seed);
                (0..n).map(|_| rms * rng.normal()).collect()
            }
            Wave::Band {
                from_hz,
                to_hz,
                rms,
                seed,
            } => band(n, fs, *from_hz, *to_hz, *rms, *seed),
            Wave::Offset { value } => vec![*value; n],
            Wave::Ramp { from, to } => {
                let span = timeline.duration_s;
                (0..n)
                    .map(|i| from + (to - from) * time(i) / span)
                    .collect()
            }
            Wave::Chirp {
                from_hz,
                to_hz,
                amplitude,
            } => {
                let rate = (to_hz - from_hz) / timeline.duration_s;
                (0..n)
                    .map(|i| {
                        let t = time(i);
                        amplitude * (TAU * (from_hz * t + rate * t * t / 2.0)).sin()
                    })
                    .collect()
            }
            Wave::ThrottleTone {
                hz_at_zero,
                hz_per_percent,
                amplitude,
                band,
            } => {
                let mut phase = 0.0;
                timeline
                    .throttle
                    .iter()
                    .map(|throttle| {
                        let value = amplitude * f64::sin(phase);
                        phase += TAU * (hz_at_zero + hz_per_percent * throttle) / fs;
                        let sounds = band.is_none_or(|(low, high)| {
                            *throttle >= low - SAME_TIME_S && *throttle <= high + SAME_TIME_S
                        });
                        if sounds { value } else { 0.0 }
                    })
                    .collect()
            }
        }
    }
}

/// The bins of an `n`-sample spectrum at `fs` that lie in `from_hz .. to_hz`:
/// first and one past the last, never the mean or the Nyquist bin.
pub fn band_bins(n: usize, fs: f64, from_hz: f64, to_hz: f64) -> (usize, usize) {
    let per_hz = n as f64 / fs;
    let first = ((from_hz * per_hz - SAME_TIME_S).ceil().max(1.0) as usize).min(n / 2);
    let end = ((to_hz * per_hz - SAME_TIME_S).ceil().max(0.0) as usize).min(n / 2);
    (first, end.max(first))
}

/// Band-limited noise of `n` samples: see [`Wave::Band`].
fn band(n: usize, fs: f64, from_hz: f64, to_hz: f64, rms: f64, seed: u64) -> Vec<f64> {
    let (first, end) = band_bins(n, fs, from_hz, to_hz);
    if end == first || n == 0 {
        return vec![0.0; n];
    }
    // K cosines of amplitude a carry K·a²/2
    let amplitude = rms * (2.0 / (end - first) as f64).sqrt();
    let mut rng = Rng::new(seed);
    let mut spectrum = vec![Complex::new(0.0, 0.0); n / 2 + 1];
    for bin in &mut spectrum[first..end] {
        let phase = TAU * rng.next();
        *bin = Complex::new(phase.cos(), phase.sin()) * (amplitude * n as f64 / 2.0);
    }
    Fft::new().irfft(&spectrum, n)
}

/// What the pilot does with the stick of one axis: the rate asked for,
/// deg/s. Every profile is piecewise constant, so the response of a plant to
/// it is exact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stick {
    /// The stick stays centred.
    Silence,
    /// The levels in turn, each held for `every_s`, from `start_s` on, and
    /// again from the first when the list ends.
    Steps {
        /// Time of the first level, seconds.
        start_s: f64,
        /// Time between two levels, seconds.
        every_s: f64,
        /// Rates asked for, deg/s.
        levels: Vec<f64>,
    },
    /// Random whole levels in `±amplitude`, each held for a random whole
    /// number of milliseconds.
    Random {
        /// Seed of the generator.
        seed: u64,
        /// Shortest hold, ms.
        hold_min_ms: u32,
        /// Longest hold, ms.
        hold_max_ms: u32,
        /// Largest rate asked for, deg/s.
        amplitude: f64,
    },
}

impl Stick {
    /// The moments the stick moves and the level it moves to, in time
    /// order: `(seconds, deg/s)`. Before the first one the level is zero.
    pub fn breakpoints(&self, duration_s: f64) -> Vec<(f64, f64)> {
        match self {
            Stick::Silence => Vec::new(),
            Stick::Steps {
                start_s,
                every_s,
                levels,
            } => {
                if levels.is_empty() || *every_s <= 0.0 {
                    return Vec::new();
                }
                let count = ((duration_s - SAME_TIME_S - start_s) / every_s)
                    .ceil()
                    .max(0.0) as usize;
                (0..count)
                    .map(|k| (start_s + k as f64 * every_s, levels[k % levels.len()]))
                    .collect()
            }
            Stick::Random {
                seed,
                hold_min_ms,
                hold_max_ms,
                amplitude,
            } => {
                let mut rng = Rng::new(*seed);
                let span = f64::from(hold_max_ms.saturating_sub(*hold_min_ms) + 1);
                let mut out = Vec::new();
                let mut at_ms = 0u64;
                while (at_ms as f64) / 1000.0 < duration_s - SAME_TIME_S {
                    let level = ((rng.next() * 2.0 - 1.0) * amplitude).round();
                    out.push((at_ms as f64 / 1000.0, level));
                    let hold = hold_min_ms + (rng.next() * span) as u32;
                    at_ms += u64::from(hold.max(1));
                }
                out
            }
        }
    }
}

/// How the craft follows the stick: the transfer from the rate asked for to
/// the rate flown.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Plant {
    /// The craft does not move: the gyro carries only what is added to it.
    None,
    /// The stick, `delay_ms` later.
    Delay {
        /// Transport delay, ms.
        delay_ms: f64,
    },
    /// `gain / (1 + s·τ)` after the delay.
    FirstOrder {
        /// Time constant, ms.
        tau_ms: f64,
        /// Transport delay, ms.
        delay_ms: f64,
        /// Steady-state gain.
        gain: f64,
    },
    /// `gain · ω² / (s² + 2ζω·s + ω²)` after the delay.
    SecondOrder {
        /// Natural frequency `ω / 2π`, Hz.
        natural_hz: f64,
        /// Damping ratio ζ: below 1 the response overshoots.
        damping: f64,
        /// Transport delay, ms.
        delay_ms: f64,
        /// Steady-state gain.
        gain: f64,
    },
}

impl Plant {
    /// Transport delay, seconds.
    fn delay_s(&self) -> f64 {
        match self {
            Plant::None => 0.0,
            Plant::Delay { delay_ms }
            | Plant::FirstOrder { delay_ms, .. }
            | Plant::SecondOrder { delay_ms, .. } => delay_ms / 1000.0,
        }
    }

    /// Value the response to a unit step settles at.
    pub fn gain(&self) -> f64 {
        match self {
            Plant::None => 0.0,
            Plant::Delay { .. } => 1.0,
            Plant::FirstOrder { gain, .. } | Plant::SecondOrder { gain, .. } => *gain,
        }
    }

    /// Time after a step from which the response is its final value to the
    /// last bit, seconds.
    fn settled_s(&self) -> f64 {
        let slowest = match self {
            Plant::None | Plant::Delay { .. } => 0.0,
            Plant::FirstOrder { tau_ms, .. } => tau_ms / 1000.0,
            Plant::SecondOrder {
                natural_hz,
                damping,
                ..
            } => {
                let w = TAU * natural_hz;
                if *damping > 1.0 {
                    // the slow pole of the two real ones
                    1.0 / (w * (damping - (damping * damping - 1.0).sqrt()))
                } else {
                    1.0 / (w * damping.max(1e-3))
                }
            }
        };
        self.delay_s() + SETTLED_AFTER * slowest
    }

    /// The response `t` seconds after a unit step of the stick, in closed
    /// form.
    pub fn step_response(&self, t: f64) -> f64 {
        let t = t - self.delay_s();
        if t < -SAME_TIME_S {
            return 0.0;
        }
        let t = t.max(0.0);
        match self {
            Plant::None => 0.0,
            Plant::Delay { .. } => 1.0,
            Plant::FirstOrder { tau_ms, gain, .. } => {
                if *tau_ms <= 0.0 {
                    return *gain;
                }
                gain * (1.0 - (-t / (tau_ms / 1000.0)).exp())
            }
            Plant::SecondOrder {
                natural_hz,
                damping,
                gain,
                ..
            } => {
                let w = TAU * natural_hz;
                let z = *damping;
                let shape = if z < 1.0 {
                    let wd = w * (1.0 - z * z).sqrt();
                    let decay = (-z * w * t).exp();
                    1.0 - decay * ((wd * t).cos() + z * w / wd * (wd * t).sin())
                } else if z > 1.0 {
                    let root = (z * z - 1.0).sqrt();
                    let (s1, s2) = (-w * (z - root), -w * (z + root));
                    1.0 - (s2 * (s1 * t).exp() - s1 * (s2 * t).exp()) / (s2 - s1)
                } else {
                    1.0 - (-w * t).exp() * (1.0 + w * t)
                };
                gain * shape
            }
        }
    }

    /// Overshoot of the step response as a fraction of its final value
    /// (0 when it does not overshoot).
    pub fn overshoot(&self) -> f64 {
        match self {
            Plant::SecondOrder { damping, .. } if *damping < 1.0 && *damping > 0.0 => {
                (-PI * damping / (1.0 - damping * damping).sqrt()).exp()
            }
            _ => 0.0,
        }
    }
}

/// What the filtered gyro carries, next to the unfiltered one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Filtered {
    /// The same samples: no filter.
    #[default]
    Same,
    /// The response of the craft plus other components than the unfiltered
    /// gyro has, to tell the two channels apart in a tool.
    Separate {
        /// Components of the filtered gyro.
        waves: Vec<Wave>,
    },
}

/// One axis of a probe flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisSpec {
    /// The stick.
    pub stick: Stick,
    /// How the craft follows it.
    pub plant: Plant,
    /// What is added to the response on the unfiltered gyro.
    pub waves: Vec<Wave>,
    /// The filtered gyro.
    #[serde(default)]
    pub filtered: Filtered,
}

impl AxisSpec {
    /// An axis at rest: centred stick, nothing on the gyro.
    pub fn quiet() -> AxisSpec {
        AxisSpec {
            stick: Stick::Silence,
            plant: Plant::None,
            waves: Vec::new(),
            filtered: Filtered::Same,
        }
    }

    /// An axis whose gyro carries `waves` and nothing else.
    pub fn of(waves: Vec<Wave>) -> AxisSpec {
        AxisSpec {
            waves,
            ..AxisSpec::quiet()
        }
    }

    /// An axis that follows `stick` through `plant`, with a clean gyro.
    pub fn following(stick: Stick, plant: Plant) -> AxisSpec {
        AxisSpec {
            stick,
            plant,
            ..AxisSpec::quiet()
        }
    }
}

/// Throttle over the flight, percent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Throttle {
    /// Fixed.
    Constant {
        /// Percent.
        percent: f64,
    },
    /// The levels in turn, each for its own time, and again from the first.
    Stairs {
        /// `(percent, seconds)` of every stair.
        steps: Vec<(f64, f64)>,
    },
    /// A straight line over the whole flight.
    Ramp {
        /// Percent at the start.
        from: f64,
        /// Percent at the end.
        to: f64,
    },
}

impl Throttle {
    fn render(&self, fs: f64, n: usize, duration_s: f64) -> Vec<f64> {
        match self {
            Throttle::Constant { percent } => vec![*percent; n],
            Throttle::Ramp { from, to } => (0..n)
                .map(|i| from + (to - from) * (i as f64 / fs) / duration_s)
                .collect(),
            Throttle::Stairs { steps } => {
                let period: f64 = steps.iter().map(|(_, s)| s.max(0.0)).sum();
                if period <= 0.0 {
                    return vec![0.0; n];
                }
                (0..n)
                    .map(|i| {
                        let mut t = (i as f64 / fs) % period;
                        for (percent, seconds) in steps {
                            if t < seconds - SAME_TIME_S {
                                return *percent;
                            }
                            t -= seconds;
                        }
                        steps.last().map_or(0.0, |s| s.0)
                    })
                    .collect()
            }
        }
    }
}

/// What a component needs to know about the flight.
struct Timeline<'a> {
    /// Sample rate, Hz.
    fs: f64,
    /// Length of the flight, seconds.
    duration_s: f64,
    /// Throttle at every sample, percent.
    throttle: &'a [f64],
}

/// A whole probe flight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightSpec {
    /// Log rate, Hz.
    pub fs: f64,
    /// Length, seconds.
    pub duration_s: f64,
    /// Roll.
    pub roll: AxisSpec,
    /// Pitch.
    pub pitch: AxisSpec,
    /// Yaw.
    pub yaw: AxisSpec,
    /// Throttle; the four motors follow it.
    pub throttle: Throttle,
}

/// The samples of one axis.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisSamples {
    /// Rate asked for, deg/s.
    pub setpoint: Vec<f64>,
    /// The response of the plant alone, deg/s.
    pub response: Vec<f64>,
    /// Unfiltered gyro: the response plus the components of the axis.
    pub unfiltered: Vec<f64>,
    /// Filtered gyro.
    pub filtered: Vec<f64>,
}

impl FlightSpec {
    /// A flight of `duration_s` at `fs` with every axis at rest and the
    /// throttle at half.
    pub fn quiet(fs: f64, duration_s: f64) -> FlightSpec {
        FlightSpec {
            fs,
            duration_s,
            roll: AxisSpec::quiet(),
            pitch: AxisSpec::quiet(),
            yaw: AxisSpec::quiet(),
            throttle: Throttle::Constant { percent: 50.0 },
        }
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        (self.fs * self.duration_s).round().max(0.0) as usize
    }

    /// Whether the flight has no sample.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The axis specification of `axis`.
    pub fn axis(&self, axis: Axis) -> &AxisSpec {
        match axis {
            Axis::Roll => &self.roll,
            Axis::Pitch => &self.pitch,
            Axis::Yaw => &self.yaw,
        }
    }

    /// Throttle at every sample, percent.
    pub fn throttle_samples(&self) -> Vec<f64> {
        self.throttle.render(self.fs, self.len(), self.duration_s)
    }

    /// The exact samples of `axis`, before anything is rounded for a log.
    pub fn samples(&self, axis: Axis) -> AxisSamples {
        let spec = self.axis(axis);
        let n = self.len();
        let fs = self.fs;
        let throttle = self.throttle_samples();
        let timeline = Timeline {
            fs,
            duration_s: self.duration_s,
            throttle: &throttle,
        };
        let steps = spec.stick.breakpoints(self.duration_s);
        let mut setpoint = vec![0.0; n];
        let mut response = vec![0.0; n];
        // what the steps that have settled add from each sample on
        let mut settled = vec![0.0; n + 1];
        let mut level = 0.0;
        for (k, (at, to)) in steps.iter().enumerate() {
            let change = to - level;
            level = *to;
            let first = ((at - SAME_TIME_S) * fs).ceil().max(0.0) as usize;
            let until = steps
                .get(k + 1)
                .map_or(n, |(next, _)| ((next - SAME_TIME_S) * fs).ceil() as usize);
            for value in &mut setpoint[first.min(n)..until.min(n)] {
                *value = *to;
            }
            let done = (((at + spec.plant.settled_s()) * fs).ceil() as usize + 1).min(n);
            for (i, value) in response.iter_mut().enumerate().take(done).skip(first) {
                *value += change * spec.plant.step_response(i as f64 / fs - at);
            }
            settled[done] += change * spec.plant.gain();
        }
        let mut sum = 0.0;
        for (value, add) in response.iter_mut().zip(&settled) {
            sum += add;
            *value += sum;
        }
        let add = |waves: &[Wave]| -> Vec<f64> {
            let mut out = response.clone();
            for wave in waves {
                for (value, w) in out.iter_mut().zip(wave.render(&timeline)) {
                    *value += w;
                }
            }
            out
        };
        let unfiltered = add(&spec.waves);
        let filtered = match &spec.filtered {
            Filtered::Same => unfiltered.clone(),
            Filtered::Separate { waves } => add(waves),
        };
        AxisSamples {
            setpoint,
            response,
            unfiltered,
            filtered,
        }
    }

    /// The flight as a session, ready for the log writer: time, the three
    /// axes, the throttle and four motors that follow it.
    pub fn build(&self) -> FlightSession {
        let n = self.len();
        let dt_us = 1.0e6 / self.fs;
        let time_us: Vec<i64> = (0..n).map(|i| (i as f64 * dt_us).round() as i64).collect();
        let mut channels = ChannelSet::new();
        let mut add = |id: ChannelId, name: String, unit: Unit, values: Vec<f64>| {
            channels.insert(Channel {
                id,
                name,
                unit,
                data: Series::from_vec(values),
                origin: Origin::Derived {
                    from: Vec::new(),
                    by: "openpid-probe".into(),
                },
            });
        };
        add(
            ChannelId::Time,
            "time".into(),
            Unit::Microseconds,
            time_us.iter().map(|t| *t as f64).collect(),
        );
        add(
            ChannelId::LoopIteration,
            "loopIteration".into(),
            Unit::Unitless,
            (0..n).map(|i| i as f64).collect(),
        );
        for axis in Axis::ALL {
            let i = axis.index();
            let samples = self.samples(axis);
            add(
                ChannelId::Setpoint { axis },
                format!("setpoint[{i}]"),
                Unit::DegreesPerSecond,
                samples.setpoint,
            );
            let raw = ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            };
            add(
                raw,
                format!("gyroUnfilt[{i}]"),
                Unit::DegreesPerSecond,
                samples.unfiltered,
            );
            let filtered = ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            };
            add(
                filtered,
                format!("gyroADC[{i}]"),
                Unit::DegreesPerSecond,
                samples.filtered,
            );
        }
        let throttle = self.throttle_samples();
        for idx in 0..4u8 {
            add(
                ChannelId::Motor { idx },
                format!("motor[{idx}]"),
                Unit::Percent,
                throttle.clone(),
            );
        }
        add(
            ChannelId::Throttle,
            "throttle".into(),
            Unit::Percent,
            throttle,
        );
        let mut settings = Settings {
            loop_rates: openpid_fmt_blackbox::write::typical_rates(self.fs),
            ..Settings::default()
        };
        settings.motor.count = Some(4);
        let mut firmware = FirmwareInfo::parse_revision("Synthetic (openpid-probe)");
        firmware.revision = "openpid-probe".into();
        FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware,
            profile_id: "synthetic".into(),
            settings,
            raw_headers: Vec::new(),
            timebase: Timebase::from_times(time_us, Some(dt_us)),
            channels,
            events: Vec::new(),
            aux: Vec::new(),
            flight_modes: Vec::new(),
            diagnostics: Diagnostics::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Amplitude of the `hz` component of `x` (the length holds whole periods).
    fn amplitude_at(x: &[f64], hz: f64, fs: f64) -> f64 {
        let n = x.len() as f64;
        let (mut s, mut c) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let phase = TAU * hz * i as f64 / fs;
            s += v * phase.sin();
            c += v * phase.cos();
        }
        2.0 * (s * s + c * c).sqrt() / n
    }

    fn rms(x: &[f64]) -> f64 {
        (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn flight_of(fs: f64, duration_s: f64, roll: AxisSpec) -> FlightSpec {
        FlightSpec {
            roll,
            ..FlightSpec::quiet(fs, duration_s)
        }
    }

    #[test]
    fn a_tone_has_its_amplitude_at_every_log_rate() {
        for fs in [1000.0, 2000.0, 4000.0, 8000.0] {
            let spec = flight_of(
                fs,
                2.0,
                AxisSpec::of(vec![Wave::tone(100.0, 10.0), Wave::tone(250.0, 3.0)]),
            );
            let x = spec.samples(Axis::Roll).unfiltered;
            assert_eq!(x.len(), (2.0 * fs) as usize);
            assert!((amplitude_at(&x, 100.0, fs) - 10.0).abs() < 1e-9, "{fs}");
            assert!((amplitude_at(&x, 250.0, fs) - 3.0).abs() < 1e-9, "{fs}");
            assert!(amplitude_at(&x, 150.0, fs) < 1e-9, "{fs}");
            // the other axes are at rest, and the filtered gyro is the same
            assert!(
                spec.samples(Axis::Pitch)
                    .unfiltered
                    .iter()
                    .all(|v| *v == 0.0)
            );
            assert_eq!(spec.samples(Axis::Roll).filtered, x);
        }
    }

    #[test]
    fn a_tone_can_have_a_phase_and_a_span() {
        let wave = Wave::Tone {
            hz: 50.0,
            amplitude: 4.0,
            phase_deg: 90.0,
            from_s: Some(1.0),
            to_s: Some(2.0),
        };
        let x = flight_of(1000.0, 3.0, AxisSpec::of(vec![wave]))
            .samples(Axis::Roll)
            .unfiltered;
        assert!(x[..1000].iter().all(|v| *v == 0.0));
        assert!(x[2000..].iter().all(|v| *v == 0.0));
        // a cosine: its first sample is the amplitude
        assert!((x[1000] - 4.0).abs() < 1e-9);
        assert!((amplitude_at(&x[1000..2000], 50.0, 1000.0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn noise_has_its_rms_and_depends_only_on_the_seed() {
        let noise = |seed| {
            flight_of(
                4000.0,
                30.0,
                AxisSpec::of(vec![Wave::Noise { rms: 5.0, seed }]),
            )
            .samples(Axis::Roll)
            .unfiltered
        };
        let a = noise(1);
        assert!((rms(&a) - 5.0).abs() < 0.05, "{}", rms(&a));
        let mean = a.iter().sum::<f64>() / a.len() as f64;
        assert!(mean.abs() < 0.05, "{mean}");
        assert_eq!(a, noise(1));
        assert_ne!(a, noise(2));
        // white: neighbouring samples are not correlated
        let lag1: f64 = a.windows(2).map(|w| w[0] * w[1]).sum::<f64>() / a.len() as f64;
        assert!(lag1.abs() < 0.3, "{lag1}");
    }

    #[test]
    fn a_band_has_its_power_between_its_edges_and_nowhere_else() {
        let fs = 2000.0;
        let wave = Wave::Band {
            from_hz: 100.0,
            to_hz: 200.0,
            rms: 10.0,
            seed: 5,
        };
        let spec = flight_of(fs, 30.0, AxisSpec::of(vec![wave.clone()]));
        let x = spec.samples(Axis::Roll).unfiltered;
        assert!((rms(&x) - 10.0).abs() < 1e-9, "{}", rms(&x));
        // 100 Hz of band at 1/30 Hz per bin
        assert_eq!(band_bins(x.len(), fs, 100.0, 200.0), (3000, 6000));
        let spectrum = Fft::new().rfft(&x);
        let power = |bins: std::ops::Range<usize>| -> f64 {
            let n = x.len() as f64;
            spectrum[bins]
                .iter()
                .map(|c| 2.0 * c.norm_sqr() / (n * n))
                .sum()
        };
        assert!((power(3000..6000) - 100.0).abs() < 1e-6);
        assert!(power(1..3000) < 1e-12 && power(6000..30000) < 1e-12);
        // flat: every bin of the band carries the same power
        let one = 2.0 * spectrum[4000].norm_sqr() / (x.len() as f64).powi(2);
        assert!((one - 100.0 / 3000.0).abs() < 1e-9);
        // noise-like: no sample far beyond a few RMS, and not a repeating pattern
        assert!(x.iter().all(|v| v.abs() < 60.0));
        assert!((x[0] - x[2000]).abs() > 1e-6);
        // the same seed, the same samples; another seed, others
        let again = flight_of(fs, 30.0, AxisSpec::of(vec![wave]));
        assert_eq!(again.samples(Axis::Roll).unfiltered, x);
        let other = Wave::Band {
            from_hz: 100.0,
            to_hz: 200.0,
            rms: 10.0,
            seed: 6,
        };
        let y = flight_of(fs, 30.0, AxisSpec::of(vec![other]))
            .samples(Axis::Roll)
            .unfiltered;
        assert!((rms(&y) - 10.0).abs() < 1e-9);
        assert_ne!(x, y);
        // a band with no bin in it, or beyond the Nyquist frequency, is silent
        let none = Wave::Band {
            from_hz: 100.0,
            to_hz: 100.0,
            rms: 10.0,
            seed: 1,
        };
        let z = flight_of(fs, 1.0, AxisSpec::of(vec![none]))
            .samples(Axis::Roll)
            .unfiltered;
        assert!(z.iter().all(|v| *v == 0.0));
        assert_eq!(band_bins(2000, fs, 1500.0, 1800.0), (1000, 1000));
        assert_eq!(band_bins(2000, fs, 0.0, 5.0), (1, 5));
    }

    #[test]
    fn offset_ramp_and_chirp_follow_their_formulas() {
        let spec = flight_of(
            1000.0,
            2.0,
            AxisSpec::of(vec![
                Wave::Offset { value: 7.0 },
                Wave::Ramp {
                    from: -10.0,
                    to: 10.0,
                },
            ]),
        );
        let x = spec.samples(Axis::Roll).unfiltered;
        assert!((x[0] + 3.0).abs() < 1e-12);
        assert!((x[1000] - 7.0).abs() < 1e-12);
        assert!((x[1999] - (7.0 + 10.0 - 0.01)).abs() < 1e-9);

        let chirp = Wave::Chirp {
            from_hz: 10.0,
            to_hz: 110.0,
            amplitude: 2.0,
        };
        let x = flight_of(8000.0, 2.0, AxisSpec::of(vec![chirp]))
            .samples(Axis::Roll)
            .unfiltered;
        // the frequency at t is 10 + 50·t: count the zero crossings per 100 ms
        let crossings = |from: usize| {
            x[from..from + 800]
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count() as f64
                * 10.0
        };
        assert!((crossings(0) - 12.5).abs() <= 10.0, "{}", crossings(0));
        assert!(
            (crossings(8000) - 62.5).abs() <= 10.0,
            "{}",
            crossings(8000)
        );
        assert!(
            (crossings(15200) - 107.5).abs() <= 10.0,
            "{}",
            crossings(15200)
        );
        assert!(x.iter().all(|v| v.abs() <= 2.0));
    }

    #[test]
    fn a_throttle_tone_follows_the_stairs() {
        let spec = FlightSpec {
            throttle: Throttle::Stairs {
                steps: vec![(20.0, 1.0), (60.0, 2.0)],
            },
            roll: AxisSpec::of(vec![Wave::ThrottleTone {
                hz_at_zero: 100.0,
                hz_per_percent: 2.0,
                amplitude: 6.0,
                band: None,
            }]),
            pitch: AxisSpec::of(vec![Wave::ThrottleTone {
                hz_at_zero: 100.0,
                hz_per_percent: 2.0,
                amplitude: 6.0,
                band: Some((50.0, 70.0)),
            }]),
            ..FlightSpec::quiet(2000.0, 6.0)
        };
        let throttle = spec.throttle_samples();
        assert_eq!(throttle[0], 20.0);
        assert_eq!(throttle[1999], 20.0);
        assert_eq!(throttle[2000], 60.0);
        assert_eq!(throttle[5999], 60.0);
        assert_eq!(throttle[6000], 20.0, "the stairs repeat");
        let roll = spec.samples(Axis::Roll).unfiltered;
        // 140 Hz on the first stair, 220 Hz on the second
        assert!((amplitude_at(&roll[..2000], 140.0, 2000.0) - 6.0).abs() < 1e-6);
        assert!(amplitude_at(&roll[..2000], 220.0, 2000.0) < 1e-6);
        assert!((amplitude_at(&roll[2000..6000], 220.0, 2000.0) - 6.0).abs() < 1e-6);
        // with a band the tone is silent outside it
        let pitch = spec.samples(Axis::Pitch).unfiltered;
        assert!(pitch[..2000].iter().all(|v| *v == 0.0));
        assert!((amplitude_at(&pitch[2000..6000], 220.0, 2000.0) - 6.0).abs() < 1e-6);
        // the motors carry the throttle
        let session = spec.build();
        let motor = session.channel(&ChannelId::Motor { idx: 3 }).unwrap();
        assert_eq!(motor.data.values(), throttle.as_slice());
    }

    #[test]
    fn throttle_profiles() {
        let ramp = Throttle::Ramp {
            from: 10.0,
            to: 90.0,
        }
        .render(1000.0, 2000, 2.0);
        assert_eq!(ramp[0], 10.0);
        assert!((ramp[1000] - 50.0).abs() < 1e-12);
        assert_eq!(
            Throttle::Constant { percent: 33.0 }.render(1000.0, 3, 1.0),
            [33.0; 3]
        );
        let none = Throttle::Stairs { steps: Vec::new() }.render(1000.0, 3, 1.0);
        assert_eq!(none, [0.0; 3]);
    }

    #[test]
    fn step_responses_are_the_closed_forms() {
        let first = Plant::FirstOrder {
            tau_ms: 20.0,
            delay_ms: 5.0,
            gain: 0.8,
        };
        assert_eq!(first.step_response(0.004), 0.0);
        assert_eq!(first.step_response(0.005), 0.0);
        let one_tau = first.step_response(0.025);
        assert!((one_tau - 0.8 * (1.0 - (-1.0f64).exp())).abs() < 1e-12);
        assert!((first.step_response(5.0) - 0.8).abs() < 1e-12);
        assert_eq!(first.overshoot(), 0.0);
        let instant = Plant::FirstOrder {
            tau_ms: 0.0,
            delay_ms: 0.0,
            gain: 2.0,
        };
        assert_eq!(instant.step_response(0.0), 2.0);

        for damping in [0.3, 0.5, 0.7] {
            let plant = Plant::SecondOrder {
                natural_hz: 20.0,
                damping,
                delay_ms: 0.0,
                gain: 1.0,
            };
            // the first maximum is at π / ω_d and is 1 + exp(-πζ / sqrt(1 - ζ²))
            let wd = TAU * 20.0 * (1.0 - damping * damping).sqrt();
            let peak = plant.step_response(PI / wd);
            assert!(
                (peak - (1.0 + plant.overshoot())).abs() < 1e-12,
                "{damping}"
            );
            let around = [PI / wd - 1e-4, PI / wd + 1e-4].map(|t| plant.step_response(t));
            assert!(around.iter().all(|v| *v < peak), "{damping}");
            assert!((plant.step_response(10.0) - 1.0).abs() < 1e-12);
            assert_eq!(plant.step_response(0.0), 0.0);
        }
        assert!(
            (Plant::SecondOrder {
                natural_hz: 20.0,
                damping: 0.5,
                delay_ms: 0.0,
                gain: 1.0
            }
            .overshoot()
                - 0.163_033_534_821_580_2)
                .abs()
                < 1e-12
        );

        // critical and over-damped: monotone, no overshoot, and continuous in ζ
        let at = |damping: f64, t: f64| {
            Plant::SecondOrder {
                natural_hz: 20.0,
                damping,
                delay_ms: 0.0,
                gain: 1.0,
            }
            .step_response(t)
        };
        for damping in [1.0, 2.0] {
            let curve: Vec<f64> = (0..2000)
                .map(|i| at(damping, f64::from(i) * 1e-4))
                .collect();
            assert!(curve.windows(2).all(|w| w[1] >= w[0]), "{damping}");
            assert!(curve.iter().all(|v| *v <= 1.0), "{damping}");
            assert!((at(damping, 10.0) - 1.0).abs() < 1e-12, "{damping}");
        }
        assert!((at(1.0, 0.01) - at(1.0 - 1e-7, 0.01)).abs() < 1e-6);
        assert!((at(1.0, 0.01) - at(1.0 + 1e-7, 0.01)).abs() < 1e-6);

        let delay = Plant::Delay { delay_ms: 10.0 };
        assert_eq!(delay.step_response(0.0099), 0.0);
        assert_eq!(delay.step_response(0.01), 1.0);
        assert_eq!((delay.gain(), delay.overshoot()), (1.0, 0.0));
        assert_eq!(Plant::None.step_response(1.0), 0.0);
        assert_eq!(Plant::None.gain(), 0.0);
    }

    /// The response as the plain sum over every step, without the shortcut
    /// for settled steps.
    fn response_the_long_way(spec: &FlightSpec, axis: Axis) -> Vec<f64> {
        let a = spec.axis(axis);
        let steps = a.stick.breakpoints(spec.duration_s);
        (0..spec.len())
            .map(|i| {
                let t = i as f64 / spec.fs;
                let mut level = 0.0;
                let mut sum = 0.0;
                for (at, to) in &steps {
                    sum += (to - level) * a.plant.step_response(t - at);
                    level = *to;
                }
                sum
            })
            .collect()
    }

    #[test]
    fn the_gyro_is_the_sum_of_the_step_responses() {
        let plants = [
            Plant::SecondOrder {
                natural_hz: 25.0,
                damping: 0.4,
                delay_ms: 6.0,
                gain: 1.0,
            },
            Plant::SecondOrder {
                natural_hz: 8.0,
                damping: 3.0,
                delay_ms: 0.0,
                gain: 0.7,
            },
            Plant::FirstOrder {
                tau_ms: 30.0,
                delay_ms: 2.5,
                gain: 1.3,
            },
            Plant::Delay { delay_ms: 10.0 },
        ];
        for fs in [1000.0, 4000.0] {
            for plant in plants {
                let stick = Stick::Random {
                    seed: 9,
                    hold_min_ms: 20,
                    hold_max_ms: 400,
                    amplitude: 500.0,
                };
                let spec = flight_of(fs, 12.0, AxisSpec::following(stick, plant));
                let got = spec.samples(Axis::Roll);
                let want = response_the_long_way(&spec, Axis::Roll);
                for (i, (a, b)) in got.response.iter().zip(&want).enumerate() {
                    assert!(
                        (a - b).abs() < 1e-9,
                        "{plant:?} at {fs}: sample {i}: {a} vs {b}"
                    );
                }
                assert_eq!(got.unfiltered, got.response);
            }
        }
    }

    #[test]
    fn sticks_move_when_they_say() {
        let steps = Stick::Steps {
            start_s: 1.0,
            every_s: 0.5,
            levels: vec![200.0, 0.0, -300.0],
        };
        assert_eq!(
            steps.breakpoints(2.6),
            [(1.0, 200.0), (1.5, 0.0), (2.0, -300.0), (2.5, 200.0)]
        );
        let spec = flight_of(
            2000.0,
            2.6,
            AxisSpec::following(steps, Plant::Delay { delay_ms: 3.0 }),
        );
        let s = spec.samples(Axis::Roll);
        assert_eq!(s.setpoint[1999], 0.0);
        assert_eq!(s.setpoint[2000], 200.0);
        assert_eq!(s.setpoint[2999], 200.0);
        assert_eq!(s.setpoint[3000], 0.0);
        assert_eq!(s.setpoint[4000], -300.0);
        assert_eq!(s.setpoint[5199], 200.0);
        // the gyro is the stick 3 ms (6 samples) later
        assert_eq!(s.response[2005], 0.0);
        assert_eq!(s.response[2006], 200.0);
        assert_eq!(s.response[3006], 0.0);
        assert_eq!(s.response[4006], -300.0);

        assert!(Stick::Silence.breakpoints(10.0).is_empty());
        let nothing = Stick::Steps {
            start_s: 0.0,
            every_s: 0.0,
            levels: vec![1.0],
        };
        assert!(nothing.breakpoints(10.0).is_empty());

        let random = Stick::Random {
            seed: 4,
            hold_min_ms: 50,
            hold_max_ms: 250,
            amplitude: 300.0,
        };
        let points = random.breakpoints(60.0);
        assert!(points.len() > 200, "{}", points.len());
        assert_eq!(points[0].0, 0.0);
        for pair in points.windows(2) {
            let hold_ms = ((pair[1].0 - pair[0].0) * 1000.0).round();
            assert!((50.0..=250.0).contains(&hold_ms), "{hold_ms}");
        }
        assert!(
            points
                .iter()
                .all(|(_, l)| l.fract() == 0.0 && l.abs() <= 300.0)
        );
        assert!(points.iter().any(|(_, l)| *l > 200.0));
        assert!(points.iter().any(|(_, l)| *l < -200.0));
        assert_eq!(points, random.breakpoints(60.0));
    }

    #[test]
    fn the_filtered_gyro_can_carry_other_components() {
        let axis = AxisSpec {
            stick: Stick::Steps {
                start_s: 0.5,
                every_s: 1.0,
                levels: vec![100.0],
            },
            plant: Plant::Delay { delay_ms: 0.0 },
            waves: vec![Wave::tone(200.0, 8.0)],
            filtered: Filtered::Separate {
                waves: vec![Wave::tone(80.0, 5.0)],
            },
        };
        let spec = flight_of(1000.0, 2.0, axis);
        let s = spec.samples(Axis::Roll);
        let tone = |x: &[f64], hz| amplitude_at(&x[1000..2000], hz, 1000.0);
        assert!((tone(&s.unfiltered, 200.0) - 8.0).abs() < 1e-9);
        assert!(tone(&s.unfiltered, 80.0) < 1e-9);
        assert!((tone(&s.filtered, 80.0) - 5.0).abs() < 1e-9);
        assert!(tone(&s.filtered, 200.0) < 1e-9);
        // both carry the response: at 1.5 s the two tones cross zero
        assert!((s.unfiltered[1500] - 100.0).abs() < 1e-9);
        assert!((s.filtered[1500] - 100.0).abs() < 1e-9);
        assert_eq!(s.response[499], 0.0);
        assert_eq!(s.response[500], 100.0);
    }

    #[test]
    fn a_flight_becomes_a_session_with_every_channel() {
        let spec = FlightSpec {
            pitch: AxisSpec::of(vec![Wave::tone(60.0, 12.0)]),
            ..FlightSpec::quiet(2000.0, 1.0)
        };
        assert!(!spec.is_empty());
        assert!(FlightSpec::quiet(1000.0, 0.0).is_empty());
        let session = spec.build();
        assert_eq!(session.len(), 2000);
        assert_eq!(session.settings.loop_rates.log_hz, Some(2000.0));
        assert_eq!(session.settings.loop_rates.pid_hz, Some(4000.0));
        let time = session.channel(&ChannelId::Time).unwrap();
        assert_eq!(time.data.values()[1999], 999_500.0);
        for axis in Axis::ALL {
            for id in [
                ChannelId::Setpoint { axis },
                ChannelId::Gyro {
                    axis,
                    stage: GyroStage::Raw,
                },
                ChannelId::Gyro {
                    axis,
                    stage: GyroStage::Filtered,
                },
            ] {
                assert_eq!(session.channel(&id).unwrap().data.len(), 2000, "{id:?}");
            }
        }
        let pitch = session
            .channel(&ChannelId::Gyro {
                axis: Axis::Pitch,
                stage: GyroStage::Raw,
            })
            .unwrap();
        assert_eq!(pitch.data.values(), spec.samples(Axis::Pitch).unfiltered);
        assert_eq!(
            session.channel(&ChannelId::Throttle).unwrap().data.values(),
            [50.0; 2000]
        );
        // a spec survives being written down and read again
        let text = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<FlightSpec>(&text).unwrap(), spec);
    }
}
