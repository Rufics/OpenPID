//! Synthetic flight sessions with known ground truth: a stick profile, a
//! first-order plant with transport delay, motor-harmonic and broadband noise
//! tied to a throttle profile, a PT1 "firmware" filter, optional sample-rate
//! jitter and dropouts. Used by the generator-based tests of the analyses and
//! by `openpid gen`.

use crate::filter::{Filter, Pt1};
use openpid_core::session::{Diagnostics, Origin};
use openpid_core::settings::{Lowpass, LowpassKind};
use openpid_core::{
    Axis, Channel, ChannelId, ChannelSet, FirmwareInfo, FlightSession, GyroStage, Series, Settings,
    Timebase, Unit,
};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// Throttle over the session, percent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThrottleProfile {
    /// Fixed throttle.
    Constant {
        /// Percent.
        percent: f64,
    },
    /// Linear ramp over the whole session.
    Ramp {
        /// Percent at the start.
        from: f64,
        /// Percent at the end.
        to: f64,
    },
    /// Sine around a mean.
    Sine {
        /// Percent.
        mean: f64,
        /// Percent.
        amplitude: f64,
        /// Hz.
        hz: f64,
    },
}

/// Roll setpoint over the session, deg/s (pitch and yaw stay at zero).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetpointProfile {
    /// No stick input.
    Silence,
    /// Piecewise-constant random levels held for a random time.
    Steps {
        /// Shortest hold, seconds.
        hold_min_s: f64,
        /// Longest hold, seconds.
        hold_max_s: f64,
        /// Levels are uniform in ±amplitude.
        amplitude: f64,
    },
    /// Linear frequency sweep.
    Chirp {
        /// Hz at the start.
        from_hz: f64,
        /// Hz at the end.
        to_hz: f64,
        /// deg/s.
        amplitude: f64,
    },
    /// Pure tone.
    Sine {
        /// Hz.
        hz: f64,
        /// deg/s.
        amplitude: f64,
    },
}

/// First-order plant with transport delay: `gyro = gain · setpoint(t − delay)`
/// through `1 / (1 + s τ)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plant {
    /// Steady-state gain.
    pub gain: f64,
    /// Time constant, ms.
    pub tau_ms: f64,
    /// Transport delay, ms.
    pub delay_ms: f64,
    /// Damping ratio of a second-order plant (`ω_n = 1 / τ`); `None` keeps
    /// the first-order lag. Below 1 the step response overshoots
    /// (`exp(-π ζ / sqrt(1 - ζ²))`: 25 % at ζ = 0.4), which exercises the
    /// tuning rules.
    pub damping: Option<f64>,
}

impl Default for Plant {
    fn default() -> Self {
        Plant {
            gain: 1.0,
            tau_ms: 15.0,
            delay_ms: 5.0,
            damping: None,
        }
    }
}

/// A fixed tone on the gyro.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tone {
    /// Hz.
    pub hz: f64,
    /// deg/s.
    pub amplitude: f64,
}

/// Motor noise: harmonics of every motor's speed, which follows the throttle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotorNoise {
    /// Motor speed at zero throttle, rpm.
    pub rpm_idle: f64,
    /// Motor speed at full throttle, rpm.
    pub rpm_full: f64,
    /// Amplitude of harmonic 1, 2, … on the gyro, deg/s.
    pub harmonics: Vec<f64>,
    /// Motors differ in speed by up to this fraction, spread evenly.
    pub spread: f64,
    /// Amplitude multiplier per motor (a "bent prop" is louder); missing
    /// motors get 1.
    pub gains: Vec<f64>,
}

impl Default for MotorNoise {
    fn default() -> Self {
        MotorNoise {
            rpm_idle: 3000.0,
            rpm_full: 25000.0,
            harmonics: vec![2.0, 1.0, 0.5],
            spread: 0.05,
            gains: Vec::new(),
        }
    }
}

/// What is added to the clean gyro.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Noise {
    /// White noise RMS, deg/s.
    pub white: f64,
    /// Pink (1/f) noise RMS, deg/s.
    pub pink: f64,
    /// Fixed tones.
    pub tones: Vec<Tone>,
    /// Motor harmonics (`None` = quiet motors).
    pub motor: Option<MotorNoise>,
}

impl Default for Noise {
    fn default() -> Self {
        Noise {
            white: 0.5,
            pink: 0.0,
            tones: Vec::new(),
            motor: Some(MotorNoise::default()),
        }
    }
}

/// A gap in the log.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dropout {
    /// Seconds from the start.
    pub start_s: f64,
    /// Seconds.
    pub len_s: f64,
}

/// Everything that defines a synthetic session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SignalSpec {
    /// Sample rate, Hz.
    pub fs: f64,
    /// Length, seconds.
    pub duration_s: f64,
    /// Seed of the deterministic generator.
    pub seed: u64,
    /// Throttle profile.
    pub throttle: ThrottleProfile,
    /// Roll setpoint profile.
    pub setpoint: SetpointProfile,
    /// Plant from setpoint to gyro.
    pub plant: Plant,
    /// Noise on the gyro.
    pub noise: Noise,
    /// PT1 cutoff of the "firmware" filter that makes `gyro.filtered` from
    /// `gyro.raw`, Hz (0 = none, the two are equal).
    pub filter_hz: f64,
    /// Number of motors.
    pub motors: usize,
    /// Uniform timestamp jitter, µs (clamped to a quarter of the period).
    pub jitter_us: f64,
    /// Gaps, during which every channel but time is invalid.
    pub dropouts: Vec<Dropout>,
}

impl Default for SignalSpec {
    fn default() -> Self {
        SignalSpec {
            fs: 1000.0,
            duration_s: 10.0,
            seed: 1,
            throttle: ThrottleProfile::Ramp {
                from: 20.0,
                to: 80.0,
            },
            setpoint: SetpointProfile::Steps {
                hold_min_s: 0.05,
                hold_max_s: 0.25,
                amplitude: 400.0,
            },
            plant: Plant::default(),
            noise: Noise::default(),
            filter_hz: 100.0,
            motors: 4,
            jitter_us: 0.0,
            dropouts: Vec::new(),
        }
    }
}

/// Deterministic uniform numbers in [0, 1).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal (Box–Muller).
    fn normal(&mut self) -> f64 {
        let u1 = self.next().max(1e-12);
        let u2 = self.next();
        (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
    }
}

/// Pink noise from white by a three-pole approximation (the usual
/// "economy" filter; normalised to unit RMS over long records).
struct Pink {
    b: [f64; 3],
}

impl Pink {
    fn update(&mut self, white: f64) -> f64 {
        self.b[0] = 0.997_65 * self.b[0] + white * 0.099_046;
        self.b[1] = 0.963_00 * self.b[1] + white * 0.296_516_4;
        self.b[2] = 0.570_00 * self.b[2] + white * 1.052_691_3;
        (self.b[0] + self.b[1] + self.b[2] + white * 0.184_8) / 1.46
    }
}

impl SignalSpec {
    /// Number of samples.
    pub fn len(&self) -> usize {
        (self.duration_s * self.fs).round().max(0.0) as usize
    }

    /// Whether the spec produces no samples.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Throttle in percent at sample `i`.
    pub fn throttle_at(&self, i: usize) -> f64 {
        let t = i as f64 / self.fs;
        let frac = if self.duration_s > 0.0 {
            t / self.duration_s
        } else {
            0.0
        };
        match self.throttle {
            ThrottleProfile::Constant { percent } => percent,
            ThrottleProfile::Ramp { from, to } => from + (to - from) * frac,
            ThrottleProfile::Sine {
                mean,
                amplitude,
                hz,
            } => mean + amplitude * (TAU * hz * t).sin(),
        }
        .clamp(0.0, 100.0)
    }

    /// Speed of motor `m` at throttle `percent`, rpm.
    pub fn rpm_at(&self, m: usize, percent: f64) -> f64 {
        let Some(mn) = &self.noise.motor else {
            return 0.0;
        };
        let base = mn.rpm_idle + (mn.rpm_full - mn.rpm_idle) * (percent / 100.0);
        let offset = if self.motors > 1 {
            (m as f64 / (self.motors - 1) as f64 - 0.5) * mn.spread
        } else {
            0.0
        };
        base * (1.0 + offset)
    }

    /// The setpoint samples (roll), deg/s.
    fn setpoint(&self, rng: &mut Rng) -> Vec<f64> {
        let n = self.len();
        match self.setpoint {
            SetpointProfile::Silence => vec![0.0; n],
            SetpointProfile::Steps {
                hold_min_s,
                hold_max_s,
                amplitude,
            } => {
                let mut x = Vec::with_capacity(n);
                while x.len() < n {
                    let hold = hold_min_s + rng.next() * (hold_max_s - hold_min_s).max(0.0);
                    let len = ((hold * self.fs).round() as usize).max(1);
                    let level = (rng.next() * 2.0 - 1.0) * amplitude;
                    x.extend(std::iter::repeat_n(level, len));
                }
                x.truncate(n);
                x
            }
            SetpointProfile::Chirp {
                from_hz,
                to_hz,
                amplitude,
            } => {
                let mut phase = 0.0;
                (0..n)
                    .map(|i| {
                        let f = from_hz + (to_hz - from_hz) * i as f64 / n.max(1) as f64;
                        phase += TAU * f / self.fs;
                        amplitude * phase.sin()
                    })
                    .collect()
            }
            SetpointProfile::Sine { hz, amplitude } => (0..n)
                .map(|i| amplitude * (TAU * hz * i as f64 / self.fs).sin())
                .collect(),
        }
    }

    /// The session.
    pub fn build(&self) -> FlightSession {
        let n = self.len();
        let fs = self.fs.max(1.0);
        let mut rng = Rng(self.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        // timebase with optional jitter (never reordering samples)
        let dt_us = 1.0e6 / fs;
        let jitter = self.jitter_us.abs().min(dt_us / 4.0);
        let time_us: Vec<i64> = (0..n)
            .map(|i| (i as f64 * dt_us + (rng.next() * 2.0 - 1.0) * jitter).round() as i64)
            .collect();
        let throttle: Vec<f64> = (0..n).map(|i| self.throttle_at(i)).collect();
        let setpoint = self.setpoint(&mut rng);
        // plant: transport delay then first-order lag
        let delay = (self.plant.delay_ms / 1000.0 * fs).round().max(0.0) as usize;
        let a = if self.plant.tau_ms > 0.0 {
            1.0 - (-1.0 / (self.plant.tau_ms / 1000.0 * fs)).exp()
        } else {
            1.0
        };
        let mut clean = vec![0.0; n];
        let mut velocity = 0.0;
        let wn = if self.plant.tau_ms > 0.0 {
            1000.0 / self.plant.tau_ms
        } else {
            0.0
        };
        for i in 1..n {
            let u = if i >= delay {
                self.plant.gain * setpoint[i - delay]
            } else {
                0.0
            };
            match self.plant.damping {
                // second order, semi-implicit Euler: stable for ω_n / fs ≪ 1
                Some(zeta) if wn > 0.0 => {
                    let dt = 1.0 / fs;
                    velocity += dt * (wn * wn * (u - clean[i - 1]) - 2.0 * zeta * wn * velocity);
                    clean[i] = clean[i - 1] + dt * velocity;
                }
                _ => clean[i] = clean[i - 1] + a * (u - clean[i - 1]),
            }
        }
        // noise: white, pink, tones, motor harmonics
        let mut pink = Pink { b: [0.0; 3] };
        let mut phases: Vec<Vec<f64>> =
            vec![
                vec![0.0; self.noise.motor.as_ref().map_or(0, |m| m.harmonics.len())];
                self.motors
            ];
        let mut rpm: Vec<Vec<f64>> = vec![Vec::with_capacity(n); self.motors];
        let mut raw = vec![[0.0; 3]; n];
        for i in 0..n {
            let t = i as f64 / fs;
            let mut v = [0.0; 3];
            for (axis, slot) in v.iter_mut().enumerate() {
                let white = rng.normal();
                *slot += self.noise.white * white;
                if self.noise.pink > 0.0 {
                    // one pink source, shared across axes at different gains, keeps it cheap
                    let p = pink.update(white);
                    *slot += self.noise.pink * p * if axis == 0 { 1.0 } else { 0.7 };
                }
                for tone in &self.noise.tones {
                    *slot += tone.amplitude * (TAU * tone.hz * t + axis as f64).sin();
                }
            }
            if let Some(mn) = &self.noise.motor {
                for (m, (history, phase)) in rpm.iter_mut().zip(phases.iter_mut()).enumerate() {
                    let r = self.rpm_at(m, throttle[i]);
                    history.push(r);
                    let gain = mn.gains.get(m).copied().unwrap_or(1.0);
                    for (k, (amp, ph)) in mn.harmonics.iter().zip(phase.iter_mut()).enumerate() {
                        *ph += TAU * r / 60.0 * (k + 1) as f64 / fs;
                        let s = gain * amp * ph.sin();
                        v[0] += s;
                        v[1] += s * 0.8;
                        v[2] += s * 0.3;
                    }
                }
            } else {
                for r in &mut rpm {
                    r.push(0.0);
                }
            }
            raw[i] = [clean[i] + v[0], v[1], v[2]];
        }
        // the "firmware" filter
        let filtered: Vec<[f64; 3]> = if self.filter_hz > 0.0 {
            let mut f: Vec<Pt1> = (0..3).map(|_| Pt1::new(self.filter_hz, fs)).collect();
            raw.iter()
                .map(|s| [f[0].update(s[0]), f[1].update(s[1]), f[2].update(s[2])])
                .collect()
        } else {
            raw.clone()
        };
        let valid: Vec<bool> = (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                !self
                    .dropouts
                    .iter()
                    .any(|d| t >= d.start_s && t < d.start_s + d.len_s)
            })
            .collect();
        let series = |values: Vec<f64>| {
            Series::from_options(
                values
                    .into_iter()
                    .zip(&valid)
                    .map(|(v, ok)| ok.then_some(v)),
            )
        };
        let mut channels = ChannelSet::new();
        let mut add = |id: ChannelId, name: &str, unit: Unit, data: Series| {
            channels.insert(Channel {
                id,
                name: name.to_string(),
                unit,
                data,
                origin: Origin::Derived {
                    from: Vec::new(),
                    by: "openpid-sim signal generator".into(),
                },
            });
        };
        add(
            ChannelId::Time,
            "time",
            Unit::Microseconds,
            Series::from_vec(time_us.iter().map(|t| *t as f64).collect()),
        );
        add(
            ChannelId::LoopIteration,
            "loopIteration",
            Unit::Unitless,
            Series::from_vec((0..n).map(|i| i as f64).collect()),
        );
        for (ai, axis) in Axis::ALL.into_iter().enumerate() {
            add(
                ChannelId::Setpoint { axis },
                &format!("setpoint[{ai}]"),
                Unit::DegreesPerSecond,
                series(if ai == 0 {
                    setpoint.clone()
                } else {
                    vec![0.0; n]
                }),
            );
            add(
                ChannelId::Gyro {
                    axis,
                    stage: GyroStage::Raw,
                },
                &format!("gyroUnfilt[{ai}]"),
                Unit::DegreesPerSecond,
                series(raw.iter().map(|s| s[ai]).collect()),
            );
            add(
                ChannelId::Gyro {
                    axis,
                    stage: GyroStage::Filtered,
                },
                &format!("gyroADC[{ai}]"),
                Unit::DegreesPerSecond,
                series(filtered.iter().map(|s| s[ai]).collect()),
            );
        }
        add(
            ChannelId::Throttle,
            "throttle",
            Unit::Percent,
            series(throttle.clone()),
        );
        for (m, rpm_m) in rpm.iter().enumerate() {
            add(
                ChannelId::Motor { idx: m as u8 },
                &format!("motor[{m}]"),
                Unit::Percent,
                series(throttle.clone()),
            );
            add(
                ChannelId::MotorRpm { idx: m as u8 },
                &format!("motorRPM[{m}]"),
                Unit::Rpm,
                series(rpm_m.clone()),
            );
        }
        let mut settings = Settings::default();
        settings.loop_rates.log_hz = Some(fs);
        settings.loop_rates.gyro_hz = Some(fs);
        settings.loop_rates.pid_hz = Some(fs);
        settings.motor.poles = Some(14);
        settings.motor.count = Some(self.motors as u32);
        if self.filter_hz > 0.0 {
            settings.gyro_filters.lowpass1 = Some(Lowpass {
                kind: Some(LowpassKind::Pt1),
                cutoff_hz: self.filter_hz,
                dyn_min_hz: None,
                dyn_max_hz: None,
                dyn_expo: 0.0,
            });
        }
        let mut firmware = FirmwareInfo::parse_revision("Synthetic (openpid-sim)");
        firmware.revision = "openpid-sim signal generator".into();
        FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware,
            profile_id: "synthetic".into(),
            settings,
            raw_headers: vec![(
                "signal_spec".into(),
                serde_json::to_string(self).unwrap_or_default(),
            )],
            timebase: Timebase::from_times(time_us, Some(dt_us)),
            channels,
            events: Vec::new(),
            aux: Vec::new(),
            flight_modes: Vec::new(),
            diagnostics: Diagnostics {
                notes: vec![format!(
                    "synthetic: plant gain {} tau {} ms delay {} ms, filter PT1 {} Hz, {} motors",
                    self.plant.gain,
                    self.plant.tau_ms,
                    self.plant.delay_ms,
                    self.filter_hz,
                    self.motors
                )],
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_spec_builds_a_complete_session() {
        let spec = SignalSpec::default();
        assert_eq!(spec.len(), 10_000);
        assert!(!spec.is_empty());
        let s = spec.build();
        assert_eq!(s.len(), 10_000);
        assert!((s.timebase.sample_rate_hz() - 1000.0).abs() < 1e-9);
        for id in [
            ChannelId::Setpoint { axis: Axis::Roll },
            ChannelId::Gyro {
                axis: Axis::Yaw,
                stage: GyroStage::Raw,
            },
            ChannelId::Gyro {
                axis: Axis::Pitch,
                stage: GyroStage::Filtered,
            },
            ChannelId::Throttle,
            ChannelId::Motor { idx: 3 },
            ChannelId::MotorRpm { idx: 3 },
            ChannelId::Time,
        ] {
            assert!(s.channel(&id).is_some(), "{id:?}");
        }
        assert!(s.channel(&ChannelId::MotorRpm { idx: 4 }).is_none());
        let thr = s.channel(&ChannelId::Throttle).unwrap().data.values();
        assert!((thr[0] - 20.0).abs() < 0.01 && (thr[9_999] - 80.0).abs() < 0.01);
        let rpm0 = s
            .channel(&ChannelId::MotorRpm { idx: 0 })
            .unwrap()
            .data
            .values();
        let rpm3 = s
            .channel(&ChannelId::MotorRpm { idx: 3 })
            .unwrap()
            .data
            .values();
        assert!(rpm3[5000] > rpm0[5000], "motors are spread");
        assert!(rpm0[9_999] > rpm0[0], "speed follows throttle");
        assert_eq!(
            s.settings.gyro_filters.lowpass1.as_ref().unwrap().cutoff_hz,
            100.0
        );
        assert!(s.raw_headers[0].1.contains("\"fs\":1000.0"));
        // the same seed gives the same data, another seed does not
        let again = spec.build();
        assert_eq!(
            again
                .channel(&ChannelId::Setpoint { axis: Axis::Roll })
                .unwrap()
                .data
                .values(),
            s.channel(&ChannelId::Setpoint { axis: Axis::Roll })
                .unwrap()
                .data
                .values()
        );
        let other = SignalSpec {
            seed: 2,
            ..SignalSpec::default()
        }
        .build();
        assert_ne!(
            other
                .channel(&ChannelId::Setpoint { axis: Axis::Roll })
                .unwrap()
                .data
                .values(),
            s.channel(&ChannelId::Setpoint { axis: Axis::Roll })
                .unwrap()
                .data
                .values()
        );
    }

    #[test]
    fn profiles_jitter_dropouts_and_serde() {
        let spec = SignalSpec {
            duration_s: 2.0,
            throttle: ThrottleProfile::Sine {
                mean: 50.0,
                amplitude: 60.0,
                hz: 0.5,
            },
            setpoint: SetpointProfile::Chirp {
                from_hz: 1.0,
                to_hz: 20.0,
                amplitude: 100.0,
            },
            noise: Noise {
                white: 0.0,
                pink: 1.0,
                tones: vec![Tone {
                    hz: 137.0,
                    amplitude: 3.0,
                }],
                motor: None,
            },
            filter_hz: 0.0,
            jitter_us: 400.0,
            dropouts: vec![Dropout {
                start_s: 0.5,
                len_s: 0.1,
            }],
            ..SignalSpec::default()
        };
        let s = spec.build();
        let thr = s.channel(&ChannelId::Throttle).unwrap().data.values();
        assert!(thr.iter().all(|t| (0.0..=100.0).contains(t)), "clamped");
        assert!(thr.contains(&0.0) && thr.contains(&100.0));
        assert_eq!(spec.rpm_at(0, 50.0), 0.0, "quiet motors");
        // jitter is clamped to a quarter period and never reorders samples
        let t = &s.timebase.time_us;
        assert!(t.windows(2).all(|w| w[1] > w[0]));
        assert!(
            t.iter()
                .enumerate()
                .any(|(i, v)| (*v - i as i64 * 1000).abs() > 100)
        );
        // the dropout invalidates every channel but time
        let gy = s
            .channel(&ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Raw,
            })
            .unwrap();
        assert!(!gy.data.is_valid(550) && gy.data.is_valid(450) && gy.data.is_valid(650));
        assert_eq!(gy.data.valid_count(), 1900);
        assert!(s.channel(&ChannelId::Time).unwrap().data.is_valid(550));
        // no filter: raw and filtered are the same series
        let gf = s
            .channel(&ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Filtered,
            })
            .unwrap();
        assert_eq!(gf.data.values(), gy.data.values());
        assert!(s.settings.gyro_filters.lowpass1.is_none());
        // the chirp setpoint spans ±100
        let sp = s
            .channel(&ChannelId::Setpoint { axis: Axis::Roll })
            .unwrap()
            .data
            .values();
        assert!(sp.iter().fold(0.0f64, |m, v| m.max(v.abs())) > 99.0);
        // JSON round trip, unknown fields rejected? no: defaults fill the rest
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<SignalSpec>(&json).unwrap(), spec);
        let partial: SignalSpec =
            serde_json::from_str(r#"{"duration_s": 1, "setpoint": {"kind": "silence"}}"#).unwrap();
        assert_eq!(partial.len(), 1000);
        assert_eq!(partial.setpoint, SetpointProfile::Silence);
        assert_eq!(partial.motors, 4);
        let silent = partial.build();
        let sp = silent
            .channel(&ChannelId::Setpoint { axis: Axis::Roll })
            .unwrap()
            .data
            .values();
        assert!(sp.iter().all(|v| *v == 0.0));
        assert!(
            SignalSpec {
                duration_s: 0.0,
                ..SignalSpec::default()
            }
            .is_empty()
        );
        let one = SignalSpec {
            motors: 1,
            ..SignalSpec::default()
        };
        assert!(
            (one.rpm_at(0, 100.0) - 25000.0).abs() < 1e-9,
            "no spread with one motor"
        );
        let constant = SignalSpec {
            throttle: ThrottleProfile::Constant { percent: 33.0 },
            setpoint: SetpointProfile::Sine {
                hz: 5.0,
                amplitude: 50.0,
            },
            ..SignalSpec::default()
        };
        assert_eq!(constant.throttle_at(123), 33.0);
    }
}
