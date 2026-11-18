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
use openpid_core::settings::DebugMode;
use openpid_core::{
    Axis, BatteryField, ChannelId, FlightMode, FlightSession, GyroStage, ModeChange, PidTerm,
    Series, Settings, Unit,
};
use openpid_dsp::{Complex, Fft};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// Battery voltage every probe flight logs, V.
const RESTING_VOLTS: f64 = 16.0;
/// Signal strength every probe flight logs, in the firmware's units
/// (0 … 1023).
const RESTING_RSSI: f64 = 1000.0;

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
    /// One sample that stands out: `value` at the sample at `at_s`, nothing
    /// at any other.
    Spike {
        /// The time of the sample, seconds.
        at_s: f64,
        /// What the sample carries, deg/s.
        value: f64,
    },
    /// A triangle: `from` at the first sample, a straight line to `to` over
    /// half a period, a straight line back over the other half, and so on.
    Triangle {
        /// Value at the start of every period, deg/s.
        from: f64,
        /// Value half a period later, deg/s.
        to: f64,
        /// Length of a period, seconds.
        period_s: f64,
    },
    /// `value` in `samples` samples in a row, the first of them the sample
    /// at `at_s`, and nothing in any other.
    Pulse {
        /// The time of the first sample, seconds.
        at_s: f64,
        /// How many samples it lasts.
        samples: u32,
        /// What those samples carry, deg/s.
        value: f64,
    },
    /// The stick of the axis the wave belongs to, `delay_ms` later and
    /// times `scale`: the rate asked for, as a channel that is not the
    /// setpoint may hold it. Nothing on a channel of no axis.
    Echo {
        /// How much later, ms.
        delay_ms: f64,
        /// The factor.
        scale: f64,
    },
    /// At every move of the stick of the axis the wave belongs to, `gain`
    /// times the size of the move for `ms` milliseconds from the sample the
    /// setpoint moves at: what a feedforward does with a step. Nothing on a
    /// channel of no axis.
    Kicks {
        /// How long each kick lasts, ms.
        ms: f64,
        /// The factor on the size of the move.
        gain: f64,
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
            Wave::Spike { at_s, value } => {
                let mut out = vec![0.0; n];
                let at = (at_s * fs).round();
                if at >= 0.0 && (at as usize) < n {
                    out[at as usize] = *value;
                }
                out
            }
            Wave::Triangle { from, to, period_s } => triangle(n, period_s * fs, *from, *to),
            Wave::Echo { delay_ms, scale } => echo(timeline, delay_ms / 1000.0, *scale),
            Wave::Kicks { ms, gain } => kicks(timeline, ms / 1000.0, *gain),
            Wave::Pulse {
                at_s,
                samples,
                value,
            } => {
                let mut out = vec![0.0; n];
                let first = (at_s * fs).round() as i64;
                let end = (first + i64::from(*samples)).clamp(0, n as i64) as usize;
                let first = first.clamp(0, n as i64) as usize;
                for sample in &mut out[first..end.max(first)] {
                    *sample = *value;
                }
                out
            }
        }
    }
}

/// The first sample at or after `at_s` on a timeline at `fs`: where the
/// setpoint takes a move of the stick at `at_s`.
fn first_sample(at_s: f64, fs: f64) -> usize {
    ((at_s - SAME_TIME_S) * fs).ceil().max(0.0) as usize
}

/// The stick of the timeline `delay_s` later, times `scale`: see
/// [`Wave::Echo`].
fn echo(timeline: &Timeline<'_>, delay_s: f64, scale: f64) -> Vec<f64> {
    let n = timeline.throttle.len();
    let mut out = vec![0.0; n];
    let moves = timeline.stick;
    for (k, (at, level)) in moves.iter().enumerate() {
        let first = first_sample(at + delay_s, timeline.fs).min(n);
        let until = moves
            .get(k + 1)
            .map_or(n, |(next, _)| first_sample(next + delay_s, timeline.fs))
            .min(n);
        for value in &mut out[first..until.max(first)] {
            *value = scale * level;
        }
    }
    out
}

/// `gain` times every move of the stick of the timeline, for `width_s`
/// from the sample the setpoint moves at: see [`Wave::Kicks`].
fn kicks(timeline: &Timeline<'_>, width_s: f64, gain: f64) -> Vec<f64> {
    let n = timeline.throttle.len();
    let width = (width_s * timeline.fs).round().max(0.0) as usize;
    let mut out = vec![0.0; n];
    let mut level = 0.0;
    for (at, to) in timeline.stick {
        let first = first_sample(*at, timeline.fs).min(n);
        for value in &mut out[first..(first + width).min(n)] {
            *value += gain * (to - level);
        }
        level = *to;
    }
    out
}

/// A triangle of `n` samples with a period of `period` samples: see
/// [`Wave::Triangle`]. It is counted in samples, so that a period of a
/// whole number of them repeats to the bit, and a slope of half a unit a
/// sample lands on the halves exactly.
fn triangle(n: usize, period: f64, from: f64, to: f64) -> Vec<f64> {
    // a period a rounding error away from a whole number of samples is one
    let period = if (period - period.round()).abs() < SAME_TIME_S {
        period.round()
    } else {
        period
    };
    if period <= 0.0 {
        return vec![from; n];
    }
    let half = period / 2.0;
    (0..n)
        .map(|i| {
            let at = i as f64 % period;
            let along = if at <= half { at } else { period - at };
            from + (to - from) * along / half
        })
        .collect()
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
    /// The second-order plant of [`Plant::SecondOrder`] whose gain depends
    /// on how far the stick moves: `gain` for a move smaller than
    /// `large_from` deg/s, `large_gain` for one at least that large, as a
    /// craft whose motors run out of thrust on large moves. The response
    /// to a small move is the plant's in every other respect too.
    Graded {
        /// Natural frequency `ω / 2π`, Hz.
        natural_hz: f64,
        /// Damping ratio ζ.
        damping: f64,
        /// Transport delay, ms.
        delay_ms: f64,
        /// Steady-state gain of a small move.
        gain: f64,
        /// The smallest move that is large, deg/s.
        large_from: f64,
        /// Steady-state gain of a large move.
        large_gain: f64,
    },
}

impl Plant {
    /// The plant a small move of the stick sees: for [`Plant::Graded`] the
    /// second-order plant of its small gain, the plant itself otherwise.
    pub fn small(&self) -> Plant {
        match *self {
            Plant::Graded {
                natural_hz,
                damping,
                delay_ms,
                gain,
                ..
            } => Plant::SecondOrder {
                natural_hz,
                damping,
                delay_ms,
                gain,
            },
            other => other,
        }
    }

    /// The plant a move of the stick by `change` deg/s sees: for
    /// [`Plant::Graded`] the second-order plant of the gain of a move that
    /// size, the plant itself otherwise.
    pub fn for_move(&self, change: f64) -> Plant {
        match *self {
            Plant::Graded {
                natural_hz,
                damping,
                delay_ms,
                large_from,
                large_gain,
                ..
            } if change.abs() >= large_from => Plant::SecondOrder {
                natural_hz,
                damping,
                delay_ms,
                gain: large_gain,
            },
            other => other.small(),
        }
    }

    /// Transport delay, seconds.
    fn delay_s(&self) -> f64 {
        match self {
            Plant::None => 0.0,
            Plant::Graded { .. } => self.small().delay_s(),
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
            Plant::Graded { .. } => self.small().gain(),
        }
    }

    /// Time after a step from which the response is its final value to the
    /// last bit, seconds.
    fn settled_s(&self) -> f64 {
        let slowest = match self {
            Plant::None | Plant::Delay { .. } => 0.0,
            Plant::Graded { .. } => return self.small().settled_s(),
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
        if let Plant::Graded { .. } = self {
            return self.small().step_response(t);
        }
        let t = t - self.delay_s();
        if t < -SAME_TIME_S {
            return 0.0;
        }
        let t = t.max(0.0);
        match self {
            Plant::None => 0.0,
            Plant::Delay { .. } => 1.0,
            Plant::Graded { .. } => unreachable!("taken as the plant of a small move above"),
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
    /// Straight lines between points of the flight, `(seconds, percent)` in
    /// time order: a throttle that wanders as a pilot's does. Before the
    /// first point it holds the first value, after the last the last.
    Path {
        /// The points.
        points: Vec<(f64, f64)>,
    },
}

impl Throttle {
    fn render(&self, fs: f64, n: usize, duration_s: f64) -> Vec<f64> {
        match self {
            Throttle::Constant { percent } => vec![*percent; n],
            Throttle::Ramp { from, to } => (0..n)
                .map(|i| from + (to - from) * (i as f64 / fs) / duration_s)
                .collect(),
            Throttle::Path { points } => (0..n).map(|i| along(points, i as f64 / fs)).collect(),
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

/// The value at `t` of straight lines between `points` (`(t, value)` in
/// time order), held at the ends.
fn along(points: &[(f64, f64)], t: f64) -> f64 {
    let after = points.partition_point(|(at, _)| *at <= t);
    match (
        after.checked_sub(1).map(|i| points[i]),
        points.get(after).copied(),
    ) {
        (Some((t0, v0)), Some((t1, v1))) if t1 > t0 => v0 + (v1 - v0) * (t - t0) / (t1 - t0),
        (Some((_, v)), _) | (None, Some((_, v))) => v,
        (None, None) => 0.0,
    }
}

/// A channel beyond the gyro that carries waves of its own over a flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Carrier {
    /// The setpoint of an axis, deg/s: the waves are added to the stick.
    /// The craft does not follow them: the gyro follows the stick alone.
    Setpoint {
        /// The axis.
        axis: Axis,
    },
    /// The RC command of an axis, as the rate it asks for through the
    /// craft's rates, deg/s ([`RATE_AT_FULL_STICK`] at full stick, in a
    /// straight line). Without it a log holds the setpoint's image there.
    RcCommand {
        /// The axis.
        axis: Axis,
    },
    /// A term of the controller, in the firmware's units: the waves are
    /// added to the constant of [`FlightSpec::pid_terms`], or stand alone.
    Term {
        /// The axis.
        axis: Axis,
        /// The term.
        term: PidTerm,
    },
    /// The output of a motor, percent: added to what it holds otherwise.
    Motor {
        /// Its place, 0 to 3.
        idx: u8,
    },
    /// A debug value, in the firmware's units: added to its constant. A
    /// log holds it only while the flight names a debug mode.
    Debug {
        /// Its slot.
        idx: u8,
    },
}

impl Carrier {
    /// The axis whose stick the waves of the channel follow, if any.
    pub fn axis(&self) -> Option<Axis> {
        match self {
            Carrier::Setpoint { axis }
            | Carrier::RcCommand { axis }
            | Carrier::Term { axis, .. } => Some(*axis),
            Carrier::Motor { .. } | Carrier::Debug { .. } => None,
        }
    }

    /// The channel of the session.
    pub fn id(&self) -> ChannelId {
        match *self {
            Carrier::Setpoint { axis } => ChannelId::Setpoint { axis },
            Carrier::RcCommand { axis } => ChannelId::RcCommand {
                ch: axis.index() as u8,
            },
            Carrier::Term { axis, term } => ChannelId::PidTerm { axis, term },
            Carrier::Motor { idx } => ChannelId::Motor { idx },
            Carrier::Debug { idx } => ChannelId::Debug { idx },
        }
    }

    /// The name of its field in a log.
    pub fn field(&self) -> String {
        match *self {
            Carrier::Setpoint { axis } => format!("setpoint[{}]", axis.index()),
            Carrier::RcCommand { axis } => format!("rcCommand[{}]", axis.index()),
            Carrier::Term { axis, term } => {
                let letter = term.name().to_ascii_uppercase();
                format!("axis{letter}[{}]", axis.index())
            }
            Carrier::Motor { idx } => format!("motor[{idx}]"),
            Carrier::Debug { idx } => format!("debug[{idx}]"),
        }
    }
}

/// What a channel beyond the gyro carries: see [`Carrier`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Carried {
    /// The channel.
    pub channel: Carrier,
    /// What it carries, in the unit of the channel.
    pub waves: Vec<Wave>,
}

/// The rate the craft's rates ask for at full stick on roll, pitch and
/// yaw, deg/s: the probes' craft has "actual" rates of 100 with no expo
/// (`craft.txt`), a straight line to 1000 deg/s, and a log holds the RC
/// command of those axes as ±500 at full stick.
pub const RATE_AT_FULL_STICK: f64 = 1000.0;
/// The RC command of roll, pitch or yaw at full stick, in a log.
const RC_AT_FULL_STICK: f64 = 500.0;

/// The four terms of the controller on roll, pitch and yaw, each constant
/// over the flight, in the firmware's units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PidTerms {
    /// The P term of roll, pitch and yaw.
    pub p: [f64; 3],
    /// The I term.
    pub i: [f64; 3],
    /// The D term.
    pub d: [f64; 3],
    /// The feedforward.
    pub f: [f64; 3],
}

impl PidTerms {
    /// The terms as the session names them, in the order of a log.
    pub fn terms(&self) -> [(PidTerm, char, [f64; 3]); 4] {
        [
            (PidTerm::P, 'P', self.p),
            (PidTerm::I, 'I', self.i),
            (PidTerm::D, 'D', self.d),
            (PidTerm::F, 'F', self.f),
        ]
    }
}

/// The debug values of a flight: a firmware logs them only while a debug
/// mode is set, so the flight names one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugValues {
    /// The number of the debug mode the header of the log states
    /// (`debug_mode`); not zero, which is "none".
    pub mode: u32,
    /// The values of the first slots, each constant over the flight. The
    /// slots a log has beyond them hold zero.
    pub values: Vec<f64>,
}

/// The outputs of the motors of a flight whose ESCs are not driven
/// digitally: the range the header of the log states (`motorOutput`) and
/// the protocol (`motor_pwm_protocol`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MotorOutput {
    /// The output of a motor that stands still.
    pub low: u32,
    /// The highest output.
    pub high: u32,
    /// The number of the protocol in the firmware's list: 0 for plain PWM,
    /// 1 for `OneShot125`; 5 and above are the digital ones.
    pub protocol: u32,
}

impl MotorOutput {
    /// The share of the range a logged `output` is, percent: what a flight
    /// states its motors in.
    pub fn percent(&self, output: f64) -> f64 {
        100.0 * (output - f64::from(self.low))
            / f64::from(self.high.saturating_sub(self.low).max(1))
    }
}

/// The flight modes of a flight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Modes {
    /// The craft is armed from here on, seconds; before, it has no mode.
    pub armed_from_s: f64,
    /// Angle mode from the first of these times to the second, seconds.
    #[serde(default)]
    pub angle_s: Option<(f64, f64)>,
}

impl Modes {
    /// The modes in force at `t`, in the order of the firmware's flags.
    fn at(&self, t: f64) -> Vec<FlightMode> {
        let mut modes = Vec::new();
        if t >= self.armed_from_s - SAME_TIME_S {
            modes.push(FlightMode::Arm);
        }
        let in_angle = |(from, to): (f64, f64)| t >= from - SAME_TIME_S && t < to - SAME_TIME_S;
        if self.angle_s.is_some_and(in_angle) {
            modes.push(FlightMode::Angle);
        }
        modes
    }

    /// The changes of mode of a flight of `n` samples at `fs`: the modes of
    /// the first sample, and those of every sample they change at.
    fn changes(&self, fs: f64, n: usize) -> Vec<ModeChange> {
        let (from, to) = self.angle_s.unwrap_or((0.0, 0.0));
        let mut samples: Vec<usize> = [0.0, self.armed_from_s, from, to]
            .into_iter()
            .map(|t| ((t - SAME_TIME_S) * fs).ceil().max(0.0) as usize)
            .filter(|sample| *sample < n)
            .collect();
        samples.sort_unstable();
        samples.dedup();
        let mut out: Vec<ModeChange> = Vec::new();
        for sample_index in samples {
            let modes = self.at(sample_index as f64 / fs);
            if out.last().is_none_or(|last| last.modes != modes) {
                out.push(ModeChange {
                    sample_index,
                    modes,
                });
            }
        }
        out
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
    /// The moves of the stick of the axis, `(seconds, deg/s)`: none for a
    /// channel of no axis.
    stick: &'a [(f64, f64)],
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
    /// What the four motors do when they do not follow the throttle: to
    /// tell which of the two a tool reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motors: Option<Throttle>,
    /// The output of each of the four motors, percent, when each holds a
    /// level of its own over the whole flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motor_levels: Option<[f64; 4]>,
    /// The terms of the controller, when the flight logs other than zeros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_terms: Option<PidTerms>,
    /// The speed of each of the four motors, revolutions per minute, each
    /// constant over the flight, when the motors report one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motor_rpm: Option<[f64; 4]>,
    /// The debug values, when the flight logs any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugValues>,
    /// The flight modes, when the craft is not simply armed throughout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<Modes>,
    /// The throttle of the stick, percent, constant over the flight, when
    /// it is not the throttle the mixer works with (`throttle`): to tell
    /// which of the two a tool reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stick_throttle: Option<f64>,
    /// The outputs of the motors, when they are not the digital ones every
    /// other flight has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motor_output: Option<MotorOutput>,
    /// The channels beyond the gyro that carry waves of their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carried: Vec<Carried>,
    /// What its log does that the logs of the other flights do not.
    #[serde(default, skip_serializing_if = "Quirks::is_none")]
    pub quirks: Quirks,
}

/// What the log of a flight may do beyond what a log of the probes' craft
/// does (round 18 on): other header lines, time stamps that run off the
/// rate its header states, frames that are not logged, the unfiltered gyro
/// in the debug values in place of its own field. A flight has none of it
/// unless it says so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Quirks {
    /// Header lines, `(name, value)`: each replaces the line of the
    /// probes' craft of that name, or is added after the craft's lines in
    /// this order (the writer puts a line the firmware writes in the
    /// firmware's place, any other after them all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    /// The time stamps advance by this factor times the step of the rate
    /// the header states: a clock that runs off the loop's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<f64>,
    /// Frames that are not logged, as when the storage of a flight
    /// controller falls behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pauses: Option<Pauses>,
    /// The log has no field of the unfiltered gyro; its debug values 0 to
    /// 2 hold the unfiltered gyro of roll, pitch and yaw in deg/s, as the
    /// firmware's debug mode `GYRO_SCALED` logs it (the flight names that
    /// mode, [`FlightSpec::debug`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prefilt_in_debug: bool,
}

impl Quirks {
    /// Whether the log does nothing the craft's logs do not.
    pub fn is_none(&self) -> bool {
        *self == Quirks::default()
    }
}

/// How long the firmware stays paused once a frame was not logged,
/// seconds: it resumes on the next `I` frame, and logs one every 32 ms.
pub const PAUSE_S: f64 = 0.032;

/// Frames a log leaves out: whole stretches of [`PAUSE_S`], chosen at
/// random, never the first or the last.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pauses {
    /// The share of the stretches left out.
    pub share: f64,
    /// Seed of the choice.
    pub seed: u64,
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
            motors: None,
            motor_levels: None,
            pid_terms: None,
            motor_rpm: None,
            debug: None,
            modes: None,
            stick_throttle: None,
            motor_output: None,
            carried: Vec::new(),
            quirks: Quirks::default(),
        }
    }

    /// Whether each sample is logged: all are, but on a flight whose log
    /// pauses ([`Quirks::pauses`]).
    pub fn kept(&self) -> Vec<bool> {
        let n = self.len();
        let Some(pauses) = self.quirks.pauses else {
            return vec![true; n];
        };
        let block = ((PAUSE_S * self.fs).round() as usize).max(1);
        let blocks = n.div_ceil(block);
        let inner = blocks.saturating_sub(2);
        let wanted = ((pauses.share * blocks as f64).round() as usize).min(inner);
        // a fixed shuffle of the inner stretches; the first `wanted` go
        let mut order: Vec<usize> = (1..=inner).collect();
        let mut rng = Rng::new(pauses.seed);
        for i in (1..order.len()).rev() {
            let j = ((rng.next() * (i + 1) as f64) as usize).min(i);
            order.swap(i, j);
        }
        let mut kept = vec![true; n];
        for b in order.into_iter().take(wanted) {
            for k in kept.iter_mut().skip(b * block).take(block) {
                *k = false;
            }
        }
        kept
    }

    /// The number of samples the log holds.
    pub fn logged_len(&self) -> usize {
        self.kept().iter().filter(|k| **k).count()
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

    /// The output of the motors at every sample, percent: the throttle
    /// unless the flight says otherwise.
    pub fn motor_samples(&self) -> Vec<f64> {
        let motors = self.motors.as_ref().unwrap_or(&self.throttle);
        motors.render(self.fs, self.len(), self.duration_s)
    }

    /// The output of motor `idx` (0 … 3) at every sample, percent: its own
    /// level when the flight gives each motor one, else what all four do.
    pub fn motor_samples_of(&self, idx: usize) -> Vec<f64> {
        let mut out = match self.motor_levels {
            Some(levels) => vec![levels[idx % 4]; self.len()],
            None => self.motor_samples(),
        };
        let carrier = Carrier::Motor {
            idx: (idx % 4) as u8,
        };
        if let Some(add) = self.carried_samples(carrier) {
            out.iter_mut().zip(add).for_each(|(v, a)| *v += a);
        }
        out
    }

    /// What the flight's carried channels `channel` hold: the sum of their
    /// waves, `None` when it carries nothing there.
    pub fn carried_samples(&self, channel: Carrier) -> Option<Vec<f64>> {
        let mut waves = self
            .carried
            .iter()
            .filter(|c| c.channel == channel)
            .flat_map(|c| &c.waves)
            .peekable();
        waves.peek()?;
        let throttle = self.throttle_samples();
        let stick = channel
            .axis()
            .map(|axis| self.axis(axis).stick.breakpoints(self.duration_s))
            .unwrap_or_default();
        let timeline = Timeline {
            fs: self.fs,
            duration_s: self.duration_s,
            throttle: &throttle,
            stick: &stick,
        };
        let mut out = vec![0.0; self.len()];
        for wave in waves {
            out.iter_mut()
                .zip(wave.render(&timeline))
                .for_each(|(v, w)| *v += w);
        }
        Some(out)
    }

    /// The changes of flight mode, for the session: none for a flight that
    /// names no modes, which a log writer takes as armed throughout.
    pub fn mode_changes(&self) -> Vec<ModeChange> {
        self.modes
            .map_or_else(Vec::new, |modes| modes.changes(self.fs, self.len()))
    }

    /// The exact samples of `axis`, before anything is rounded for a log.
    pub fn samples(&self, axis: Axis) -> AxisSamples {
        let spec = self.axis(axis);
        let n = self.len();
        let fs = self.fs;
        let throttle = self.throttle_samples();
        let steps = spec.stick.breakpoints(self.duration_s);
        let timeline = Timeline {
            fs,
            duration_s: self.duration_s,
            throttle: &throttle,
            stick: &steps,
        };
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
            let plant = spec.plant.for_move(change);
            let done = (((at + plant.settled_s()) * fs).ceil() as usize + 1).min(n);
            for (i, value) in response.iter_mut().enumerate().take(done).skip(first) {
                *value += change * plant.step_response(i as f64 / fs - at);
            }
            settled[done] += change * plant.gain();
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
        if let Some(more) = self.carried_samples(Carrier::Setpoint { axis }) {
            setpoint.iter_mut().zip(more).for_each(|(v, m)| *v += m);
        }
        AxisSamples {
            setpoint,
            response,
            unfiltered,
            filtered,
        }
    }

    /// The channels of the parts only some flights have: the terms of the
    /// controller, the speeds of the motors (mechanical, which the writer
    /// turns into the electrical speed a log holds), the debug values and
    /// the throttle of the stick (in the firmware's units, 1000 … 2000: a
    /// writer takes it for `rcCommand[3]` in place of the throttle), each
    /// constant over the flight, and the waves of the carried channels on
    /// top of them or as channels of their own (the RC command of an axis
    /// in the units of a log, ±500 at full stick).
    fn extra_channels(&self) -> Vec<(ChannelId, String, Unit, Vec<f64>)> {
        let n = self.len();
        let mut out = Vec::new();
        if let Some(percent) = self.stick_throttle {
            let id = ChannelId::RcCommand { ch: 3 };
            let units = 1000.0 + 10.0 * percent;
            out.push((id, "rcCommand[3]".into(), Unit::Raw, vec![units; n]));
        }
        if let Some(pid) = &self.pid_terms {
            for (term, letter, values) in pid.terms() {
                for axis in Axis::ALL {
                    let i = axis.index();
                    let id = ChannelId::PidTerm { axis, term };
                    let name = format!("axis{letter}[{i}]");
                    out.push((id, name, Unit::Raw, vec![values[i]; n]));
                }
            }
        }
        for (idx, rpm) in self.motor_rpm.iter().flatten().enumerate() {
            let id = ChannelId::MotorRpm { idx: idx as u8 };
            out.push((id, format!("motorRPM[{idx}]"), Unit::Rpm, vec![*rpm; n]));
        }
        let debug = self.debug.iter().flat_map(|d| d.values.iter().take(8));
        for (idx, value) in debug.enumerate() {
            let id = ChannelId::Debug { idx: idx as u8 };
            out.push((id, format!("debug[{idx}]"), Unit::Raw, vec![*value; n]));
        }
        let mut carriers: Vec<Carrier> = Vec::new();
        for carried in &self.carried {
            let elsewhere = matches!(
                carried.channel,
                Carrier::Setpoint { .. } | Carrier::Motor { .. }
            );
            if !elsewhere && !carriers.contains(&carried.channel) {
                carriers.push(carried.channel);
            }
        }
        for carrier in carriers {
            let Some(mut add) = self.carried_samples(carrier) else {
                continue;
            };
            if let Carrier::RcCommand { .. } = carrier {
                for v in &mut add {
                    *v *= RC_AT_FULL_STICK / RATE_AT_FULL_STICK;
                }
            }
            match out.iter_mut().find(|entry| entry.0 == carrier.id()) {
                Some(entry) => entry.3.iter_mut().zip(add).for_each(|(v, a)| *v += a),
                None => out.push((carrier.id(), carrier.field(), Unit::Raw, add)),
            }
        }
        out
    }

    /// The flight as a session, ready for the log writer: time, the three
    /// axes, the throttle and four motors that follow it, and the parts
    /// only some flights have.
    pub fn build(&self) -> FlightSession {
        let n = self.len();
        let dt_us = 1.0e6 / self.fs;
        let stamp_us = self.quirks.clock.map_or(dt_us, |c| dt_us * c);
        let time_us: Vec<i64> = (0..n)
            .map(|i| (i as f64 * stamp_us).round() as i64)
            .collect();
        let kept = self.kept();
        let mut gyro = Vec::new();
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
            if self.quirks.prefilt_in_debug {
                let id = ChannelId::Debug { idx: i as u8 };
                add(id, format!("debug[{i}]"), Unit::Raw, samples.unfiltered);
            } else {
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
            }
            let filtered = ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            };
            if self.quirks.pauses.is_some() {
                // a sample without a gyro value is not logged: the writer
                // pauses the log there, as the firmware does
                let data = samples.filtered.iter().zip(&kept);
                gyro.push(Channel {
                    id: filtered.clone(),
                    name: format!("gyroADC[{i}]"),
                    unit: Unit::DegreesPerSecond,
                    data: Series::from_options(data.map(|(v, k)| k.then_some(*v))),
                    origin: Origin::Derived {
                        from: Vec::new(),
                        by: "openpid-probe".into(),
                    },
                });
            }
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
                self.motor_samples_of(usize::from(idx)),
            );
        }
        add(
            ChannelId::Throttle,
            "throttle".into(),
            Unit::Percent,
            throttle,
        );
        // a craft at rest on its sensors: a full battery with no load, a
        // strong radio link, one g straight down and no rotation. A log
        // has these columns on every real craft, and every log the
        // reference tool has opened from this crate had them with these
        // values; a tool that reads them must go on finding them.
        let still = |value: f64| vec![value; n];
        add(
            ChannelId::Battery(BatteryField::Voltage),
            "vbatLatest".into(),
            Unit::Volts,
            still(RESTING_VOLTS),
        );
        add(
            ChannelId::Battery(BatteryField::Current),
            "amperageLatest".into(),
            Unit::Amperes,
            still(0.0),
        );
        add(
            ChannelId::Rssi,
            "rssi".into(),
            Unit::Raw,
            still(RESTING_RSSI),
        );
        for axis in Axis::ALL {
            let i = axis.index();
            let g = if axis == Axis::Yaw { 1.0 } else { 0.0 };
            add(
                ChannelId::Acc { axis },
                format!("accSmooth[{i}]"),
                Unit::Gs,
                still(g),
            );
            add(
                ChannelId::Custom(format!("imuQuaternion[{i}]")),
                format!("imuQuaternion[{i}]"),
                Unit::Unitless,
                still(0.0),
            );
        }
        for (id, name, unit, values) in self.extra_channels() {
            add(id, name, unit, values);
        }
        // in place of the channels of the same name, where they were
        for channel in gyro {
            channels.insert(channel);
        }
        let mut settings = Settings {
            loop_rates: openpid_fmt_blackbox::write::typical_rates(self.fs),
            ..Settings::default()
        };
        settings.motor.count = Some(4);
        if let Some(output) = &self.motor_output {
            // the session names the range its motors are scaled to, and a
            // writer states it in the header (`motorOutput`)
            settings.motor.output_low = Some(f64::from(output.low));
            settings.motor.output_high = Some(f64::from(output.high));
        }
        settings.debug_mode = self.debug.as_ref().map(|debug| DebugMode {
            code: debug.mode,
            name: None,
        });
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
            flight_modes: self.mode_changes(),
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
    fn a_graded_plant_takes_its_gain_from_the_size_of_a_move() {
        let graded = Plant::Graded {
            natural_hz: 10.0,
            damping: 0.7,
            delay_ms: 3.0,
            gain: 1.0,
            large_from: 500.0,
            large_gain: 0.5,
        };
        let small = Plant::SecondOrder {
            natural_hz: 10.0,
            damping: 0.7,
            delay_ms: 3.0,
            gain: 1.0,
        };
        assert_eq!(graded.small(), small);
        assert_eq!(graded.for_move(-499.0), small);
        assert_eq!(graded.for_move(500.0).gain(), 0.5);
        assert_eq!(graded.for_move(-800.0).gain(), 0.5);
        assert_eq!(small.for_move(800.0), small);
        for t in [0.0, 0.002, 0.01, 0.1] {
            assert_eq!(graded.step_response(t), small.step_response(t));
        }
        assert_eq!(graded.gain(), 1.0);
        assert_eq!(graded.transfer(20.0), small.transfer(20.0));
    }

    #[test]
    fn a_log_that_pauses_leaves_out_whole_stretches_of_an_i_interval() {
        let mut spec = FlightSpec::quiet(2000.0, 10.0);
        assert!(spec.quirks.is_none());
        assert!(spec.kept().iter().all(|k| *k));
        spec.quirks.pauses = Some(Pauses {
            share: 0.1,
            seed: 3,
        });
        assert!(!spec.quirks.is_none());
        let kept = spec.kept();
        assert_eq!(kept.len(), 20_000);
        // 313 stretches of 64 samples, 31 of them left out
        assert_eq!(spec.logged_len(), 20_000 - 31 * 64);
        for block in kept.chunks(64) {
            assert!(block.iter().all(|k| *k == block[0]));
        }
        assert!(
            kept[..64]
                .iter()
                .chain(&kept[kept.len() - 64..])
                .all(|k| *k)
        );
        // the same choice every time
        assert_eq!(spec.kept(), kept);
        // the filtered gyro of a sample left out has no value
        let session = spec.build();
        let gyro = ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Filtered,
        };
        let data = &session.channel(&gyro).unwrap().data;
        assert!((0..kept.len()).all(|i| data.is_valid(i) == kept[i]));
        // and a clock that runs off the rate
        spec.quirks.clock = Some(1.02);
        let t = spec.build().timebase.time_us;
        assert_eq!((t[1], t[1000]), (510, 510_000));
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
        // the parts only some flights have are not written down for a
        // flight without them, and the session has none of their channels
        for part in [
            "motors",
            "motor_levels",
            "pid_terms",
            "motor_rpm",
            "debug",
            "modes",
            "stick_throttle",
            "motor_output",
        ] {
            assert!(!text.contains(part), "{part}");
        }
        assert!(session.flight_modes.is_empty() && session.settings.debug_mode.is_none());
        assert_eq!(session.settings.motor.output_low, None);
        let extra = [
            ChannelId::MotorRpm { idx: 0 },
            ChannelId::Debug { idx: 0 },
            ChannelId::RcCommand { ch: 3 },
            ChannelId::PidTerm {
                axis: Axis::Roll,
                term: PidTerm::P,
            },
        ];
        assert!(extra.iter().all(|id| session.channel(id).is_none()));
    }

    #[test]
    fn a_spike_is_one_sample() {
        let spike = |at_s: f64| {
            let wave = Wave::Spike { at_s, value: 7.0 };
            flight_of(2000.0, 1.0, AxisSpec::of(vec![wave]))
                .samples(Axis::Roll)
                .unfiltered
        };
        let x = spike(0.25);
        assert_eq!(x[500], 7.0);
        assert_eq!(x.iter().filter(|v| **v != 0.0).count(), 1);
        assert_eq!(spike(0.0)[0], 7.0);
        // a time outside the flight leaves nothing
        assert!(spike(1.0).iter().all(|v| *v == 0.0));
        assert!(spike(-0.5).iter().all(|v| *v == 0.0));
        // on top of what else the channel carries
        let waves = vec![
            Wave::Offset { value: 100.0 },
            Wave::Spike {
                at_s: 0.5,
                value: 900.0,
            },
        ];
        let x = flight_of(2000.0, 1.0, AxisSpec::of(waves))
            .samples(Axis::Roll)
            .unfiltered;
        assert_eq!((x[999], x[1000], x[1001]), (100.0, 1000.0, 100.0));
    }

    #[test]
    fn a_triangle_rises_and_falls_evenly() {
        let triangle = |fs: f64, duration_s: f64, period_s: f64| {
            let wave = Wave::Triangle {
                from: -300.0,
                to: 300.0,
                period_s,
            };
            flight_of(fs, duration_s, AxisSpec::of(vec![wave]))
                .samples(Axis::Roll)
                .unfiltered
        };
        // 1.2 s at 2 kHz: 1200 samples up and 1200 down, half a deg/s each
        let x = triangle(2000.0, 3.0, 1.2);
        assert_eq!((x[0], x[1], x[2]), (-300.0, -299.5, -299.0));
        assert_eq!(
            (x[600], x[1199], x[1200], x[1201]),
            (0.0, 299.5, 300.0, 299.5)
        );
        assert_eq!((x[2399], x[2400], x[2401]), (-299.5, -300.0, -299.5));
        assert!(x.windows(2).all(|w| (w[1] - w[0]).abs() == 0.5));
        assert_eq!(x[..2400], x[2400..4800]);
        // the slope is the span over half a period, at any rate
        let slow = triangle(1000.0, 4.0, 2.0);
        assert!((slow[500] - slow[0] - 300.0).abs() < 1e-9);
        assert!(slow.iter().all(|v| (-300.0..=300.0).contains(v)));
        assert_eq!((slow[1000], slow[2000]), (300.0, -300.0));
        // a period that is no whole number of samples still turns in time
        let odd = triangle(1000.0, 1.0, 0.0125);
        assert!((odd[25] + 300.0).abs() < 1e-9 && odd.iter().all(|v| v.abs() <= 300.0));
        // no period: the value it starts at
        assert_eq!(triangle(1000.0, 0.01, 0.0), [-300.0; 10]);
    }

    #[test]
    fn a_pulse_is_its_samples_in_a_row() {
        let pulse = |at_s: f64, samples: u32| {
            let wave = Wave::Pulse {
                at_s,
                samples,
                value: 500.0,
            };
            flight_of(1000.0, 2.0, AxisSpec::of(vec![wave]))
                .samples(Axis::Roll)
                .unfiltered
        };
        let held = |x: &[f64]| -> Vec<usize> { (0..x.len()).filter(|i| x[*i] != 0.0).collect() };
        assert_eq!(held(&pulse(0.5, 1)), [500]);
        assert_eq!(held(&pulse(0.5, 5)), [500, 501, 502, 503, 504]);
        assert_eq!(held(&pulse(1.0, 50)), (1000..1050).collect::<Vec<_>>());
        assert!(pulse(1.0, 50).iter().all(|v| *v == 0.0 || *v == 500.0));
        // one that reaches past the end is a step; one outside leaves nothing
        assert_eq!(held(&pulse(1.5, 9000)), (1500..2000).collect::<Vec<_>>());
        assert_eq!(held(&pulse(-0.002, 5)), [0, 1, 2]);
        assert!(held(&pulse(2.0, 5)).is_empty() && held(&pulse(-1.0, 5)).is_empty());
        assert!(held(&pulse(0.5, 0)).is_empty());
    }

    #[test]
    fn a_flight_can_part_the_stick_from_the_mixer_and_name_its_motor_outputs() {
        let output = MotorOutput {
            low: 1070,
            high: 2000,
            protocol: 1,
        };
        assert_eq!(output.percent(1070.0), 0.0);
        assert_eq!(output.percent(1535.0), 50.0);
        assert_eq!(output.percent(2000.0), 100.0);
        assert!((output.percent(1300.0) - 24.731_182_795_698_924).abs() < 1e-12);
        let spec = FlightSpec {
            throttle: Throttle::Constant { percent: 30.0 },
            stick_throttle: Some(50.0),
            motor_output: Some(output),
            ..FlightSpec::quiet(2000.0, 1.0)
        };
        let session = spec.build();
        let held = |id: ChannelId| {
            let values = session.channel(&id).unwrap().data.values();
            assert!(values.len() == 2000 && values.iter().all(|v| *v == values[0]));
            values[0]
        };
        // the stick in the firmware's units, the mixer's throttle in percent
        assert_eq!(held(ChannelId::RcCommand { ch: 3 }), 1500.0);
        assert_eq!(held(ChannelId::Throttle), 30.0);
        assert!(session.channel(&ChannelId::RcCommand { ch: 0 }).is_none());
        let motor = &session.settings.motor;
        assert_eq!(
            (motor.output_low, motor.output_high),
            (Some(1070.0), Some(2000.0))
        );
        let text = serde_json::to_string(&spec).unwrap();
        assert!(text.contains("\"stick_throttle\":50.0"), "{text}");
        assert!(text.contains("\"motor_output\":{\"low\":1070,\"high\":2000,\"protocol\":1}"));
        assert_eq!(serde_json::from_str::<FlightSpec>(&text).unwrap(), spec);
        // the new waves are written down under names of their own
        let waves = vec![
            Wave::Triangle {
                from: -1.0,
                to: 1.0,
                period_s: 2.0,
            },
            Wave::Pulse {
                at_s: 0.5,
                samples: 3,
                value: 9.0,
            },
        ];
        let text = serde_json::to_string(&waves).unwrap();
        assert!(text.contains("\"kind\":\"triangle\"") && text.contains("\"kind\":\"pulse\""));
        assert_eq!(serde_json::from_str::<Vec<Wave>>(&text).unwrap(), waves);
    }

    #[test]
    fn flight_modes_change_at_their_times() {
        let modes = Modes {
            armed_from_s: 1.0,
            angle_s: Some((2.0, 3.0)),
        };
        let changes = |modes: Modes, duration_s: f64| -> Vec<(usize, Vec<FlightMode>)> {
            let spec = FlightSpec {
                modes: Some(modes),
                ..FlightSpec::quiet(1000.0, duration_s)
            };
            spec.mode_changes()
                .into_iter()
                .map(|c| (c.sample_index, c.modes))
                .collect()
        };
        assert_eq!(
            changes(modes, 4.0),
            [
                (0, vec![]),
                (1000, vec![FlightMode::Arm]),
                (2000, vec![FlightMode::Arm, FlightMode::Angle]),
                (3000, vec![FlightMode::Arm]),
            ]
        );
        // a flight that ends in angle mode, and one armed from its start
        assert_eq!(changes(modes, 2.5).len(), 3);
        let armed = Modes {
            armed_from_s: 0.0,
            angle_s: None,
        };
        assert_eq!(changes(armed, 4.0), [(0, vec![FlightMode::Arm])]);
        // without modes the session names none: armed throughout, for a writer
        assert!(FlightSpec::quiet(1000.0, 4.0).mode_changes().is_empty());
    }

    #[test]
    fn a_flight_can_hold_terms_speeds_debug_values_and_motors_of_their_own() {
        let spec = FlightSpec {
            motor_levels: Some([10.0, 20.0, 30.0, 40.0]),
            pid_terms: Some(PidTerms {
                p: [1.0, 2.0, 3.0],
                i: [4.0, 5.0, 6.0],
                d: [7.0, 8.0, 9.0],
                f: [-1.0, -2.0, -3.0],
            }),
            motor_rpm: Some([6000.0, 7000.0, 8000.0, 9000.0]),
            debug: Some(DebugValues {
                mode: 3,
                values: vec![11.0, -12.0],
            }),
            modes: Some(Modes {
                armed_from_s: 0.25,
                angle_s: None,
            }),
            ..FlightSpec::quiet(2000.0, 1.0)
        };
        let session = spec.build();
        let held = |id: ChannelId| {
            let channel = session.channel(&id).unwrap();
            let values = channel.data.values();
            assert!(values.len() == 2000 && values.iter().all(|v| *v == values[0]));
            (channel.name.clone(), channel.unit.clone(), values[0])
        };
        assert_eq!(
            held(ChannelId::Motor { idx: 2 }),
            ("motor[2]".into(), Unit::Percent, 30.0)
        );
        assert_eq!(spec.motor_samples_of(3), [40.0; 2000]);
        let term = |axis: Axis, term: PidTerm| held(ChannelId::PidTerm { axis, term });
        assert_eq!(
            term(Axis::Roll, PidTerm::P),
            ("axisP[0]".into(), Unit::Raw, 1.0)
        );
        assert_eq!(
            term(Axis::Pitch, PidTerm::I),
            ("axisI[1]".into(), Unit::Raw, 5.0)
        );
        assert_eq!(
            term(Axis::Yaw, PidTerm::D),
            ("axisD[2]".into(), Unit::Raw, 9.0)
        );
        assert_eq!(
            term(Axis::Yaw, PidTerm::F),
            ("axisF[2]".into(), Unit::Raw, -3.0)
        );
        // the speed is the mechanical one: a log writer makes the
        // electrical speed of it
        assert_eq!(
            held(ChannelId::MotorRpm { idx: 1 }),
            ("motorRPM[1]".into(), Unit::Rpm, 7000.0)
        );
        assert!(session.channel(&ChannelId::Erpm { idx: 1 }).is_none());
        assert_eq!(
            held(ChannelId::Debug { idx: 1 }),
            ("debug[1]".into(), Unit::Raw, -12.0)
        );
        assert!(session.channel(&ChannelId::Debug { idx: 2 }).is_none());
        assert_eq!(
            session.settings.debug_mode.as_ref().map(|d| d.code),
            Some(3)
        );
        let modes = &session.flight_modes;
        assert_eq!((modes.len(), modes[1].sample_index), (2, 500));
        // the motors do not follow the throttle here, which stays as it was
        assert_eq!(
            session.channel(&ChannelId::Throttle).unwrap().data.values(),
            [50.0; 2000]
        );
        let text = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<FlightSpec>(&text).unwrap(), spec);
    }

    #[test]
    fn a_throttle_path_runs_straight_between_its_points() {
        let path = Throttle::Path {
            points: vec![(1.0, 20.0), (3.0, 60.0), (4.0, 40.0)],
        };
        let x = path.render(10.0, 60, 6.0);
        // held before the first point and after the last
        assert!(x[..10].iter().all(|v| *v == 20.0));
        assert!(x[40..].iter().all(|v| *v == 40.0));
        assert!((x[20] - 40.0).abs() < 1e-9 && (x[35] - 50.0).abs() < 1e-9);
        assert_eq!(along(&[], 1.0), 0.0);
        assert_eq!(along(&[(2.0, 7.0), (2.0, 9.0)], 2.0), 9.0);
    }

    /// A flight of 1 s at 1 kHz whose roll stick moves to 100 at 0.2 s and
    /// to -50 at 0.5 s.
    fn moving(carried: Vec<Carried>) -> FlightSpec {
        FlightSpec {
            roll: AxisSpec::following(
                Stick::Steps {
                    start_s: 0.2,
                    every_s: 0.3,
                    levels: vec![100.0, -50.0],
                },
                Plant::Delay { delay_ms: 5.0 },
            ),
            carried,
            ..FlightSpec::quiet(1000.0, 0.8)
        }
    }

    #[test]
    fn an_echo_holds_the_stick_later_and_kicks_follow_its_moves() {
        let echo = Carried {
            channel: Carrier::RcCommand { axis: Axis::Roll },
            waves: vec![Wave::Echo {
                delay_ms: 40.0,
                scale: 0.5,
            }],
        };
        let kicks = Carried {
            channel: Carrier::Term {
                axis: Axis::Roll,
                term: PidTerm::F,
            },
            waves: vec![Wave::Kicks {
                ms: 10.0,
                gain: 2.0,
            }],
        };
        let spec = moving(vec![echo, kicks]);
        let rc = spec
            .carried_samples(Carrier::RcCommand { axis: Axis::Roll })
            .unwrap();
        assert!(rc[..240].iter().all(|v| *v == 0.0));
        assert!(rc[240..540].iter().all(|v| *v == 50.0));
        assert!(rc[540..].iter().all(|v| *v == -25.0));
        let feedforward = Carrier::Term {
            axis: Axis::Roll,
            term: PidTerm::F,
        };
        let f = spec.carried_samples(feedforward).unwrap();
        assert!(f[200..210].iter().all(|v| *v == 200.0));
        assert!(f[500..510].iter().all(|v| *v == -300.0));
        assert_eq!(f.iter().filter(|v| **v != 0.0).count(), 20);
        // neither moves the setpoint, nor the gyro, nor a channel of no axis
        let samples = spec.samples(Axis::Roll);
        assert_eq!(
            (samples.setpoint[300], samples.response[300]),
            (100.0, 100.0)
        );
        assert!(spec.carried_samples(Carrier::Debug { idx: 0 }).is_none());
        let lone = moving(vec![Carried {
            channel: Carrier::Debug { idx: 0 },
            waves: vec![Wave::Echo {
                delay_ms: 0.0,
                scale: 1.0,
            }],
        }]);
        let debug = lone.carried_samples(Carrier::Debug { idx: 0 }).unwrap();
        assert!(debug.iter().all(|v| *v == 0.0));
        // in the session the RC command is in the units of a log, ±500 at
        // full stick, and the feedforward is a channel of its own
        let session = spec.build();
        let channel = |id: ChannelId| session.channel(&id).unwrap().data.values().to_vec();
        assert_eq!(channel(ChannelId::RcCommand { ch: 0 })[300], 25.0);
        assert_eq!(channel(feedforward.id())[205], 200.0);
        assert!(session.channel(&ChannelId::RcCommand { ch: 1 }).is_none());
    }

    #[test]
    fn carried_channels_add_to_the_setpoint_the_terms_the_motors_and_the_debug_values() {
        let tone = |hz: f64, size: f64| vec![Wave::tone(hz, size)];
        let carried = vec![
            Carried {
                channel: Carrier::Setpoint { axis: Axis::Pitch },
                waves: tone(30.0, 6.0),
            },
            Carried {
                channel: Carrier::Term {
                    axis: Axis::Roll,
                    term: PidTerm::P,
                },
                waves: tone(120.0, 4.0),
            },
            Carried {
                channel: Carrier::Motor { idx: 2 },
                waves: tone(85.0, 1.5),
            },
            Carried {
                channel: Carrier::Debug { idx: 1 },
                waves: tone(330.0, 8.0),
            },
        ];
        let spec = FlightSpec {
            pid_terms: Some(PidTerms {
                p: [10.0; 3],
                i: [0.0; 3],
                d: [0.0; 3],
                f: [0.0; 3],
            }),
            debug: Some(DebugValues {
                mode: 1,
                values: vec![0.0, 3.0],
            }),
            carried,
            ..FlightSpec::quiet(1000.0, 2.0)
        };
        let fs = spec.fs;
        let setpoint = spec.samples(Axis::Pitch).setpoint;
        assert!((amplitude_at(&setpoint, 30.0, fs) - 6.0).abs() < 1e-9);
        assert!(spec.samples(Axis::Pitch).response.iter().all(|v| *v == 0.0));
        let motor = spec.motor_samples_of(2);
        assert!((amplitude_at(&motor, 85.0, fs) - 1.5).abs() < 1e-9);
        assert!((motor.iter().sum::<f64>() / motor.len() as f64 - 50.0).abs() < 1e-9);
        assert_eq!(spec.motor_samples_of(1), [50.0; 2000]);
        let session = spec.build();
        let values = |id: ChannelId| session.channel(&id).unwrap().data.values().to_vec();
        let p = values(ChannelId::PidTerm {
            axis: Axis::Roll,
            term: PidTerm::P,
        });
        assert!((amplitude_at(&p, 120.0, fs) - 4.0).abs() < 1e-9);
        assert!((p.iter().sum::<f64>() / p.len() as f64 - 10.0).abs() < 1e-9);
        let debug = values(ChannelId::Debug { idx: 1 });
        assert!((amplitude_at(&debug, 330.0, fs) - 8.0).abs() < 1e-9);
        assert_eq!(values(ChannelId::Debug { idx: 0 }), [0.0; 2000]);
        // the names and the axes of the carriers
        assert_eq!(Carrier::Setpoint { axis: Axis::Yaw }.field(), "setpoint[2]");
        assert_eq!(Carrier::Motor { idx: 3 }.field(), "motor[3]");
        assert_eq!(Carrier::Debug { idx: 3 }.axis(), None);
        assert_eq!(
            Carrier::Term {
                axis: Axis::Yaw,
                term: PidTerm::D
            }
            .field(),
            "axisD[2]"
        );
        // written down and read back; a flight without them says nothing
        let text = serde_json::to_string(&spec).unwrap();
        assert!(text.contains("\"carried\""));
        assert_eq!(serde_json::from_str::<FlightSpec>(&text).unwrap(), spec);
        let quiet = serde_json::to_string(&FlightSpec::quiet(1000.0, 1.0)).unwrap();
        assert!(!quiet.contains("carried"));
    }
}
