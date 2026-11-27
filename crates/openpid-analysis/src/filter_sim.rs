//! Filter Sim: the unfiltered gyro of one axis through the model of the
//! controller's own filter chain (`openpid-sim`), validated against the
//! filtered gyro the log carries, and through a what-if chain with the
//! user's overrides.
//!
//! Two rates are at work (decision D-007). The response curves of the chain
//! (magnitude, phase, delay and the delays printed with them) are those of
//! the craft's filters, each computed at the rate of the loop it runs in on
//! the craft ([`curve_rate`]). The simulated gyro, its spectra and the fit
//! run the same filters on the samples of the log, so at the rate of the
//! log.
//!
//! The printed delays follow the reference tool's web edition 1.0
//! (decision D-015): each filter's phase delay and phase lag read at a
//! chosen frequency ([`FilterSimParams::delay_at_hz`], 25 Hz by default;
//! 1 Hz gives the figures of its version 0.94), summed over the low-passes,
//! over the notches and over both.

use crate::prepare::{Prepared, prepare};
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_core::settings::{ControlLoop, FilterStage, LowpassKind, Notch};
use openpid_core::{Axis, ChannelId, GyroStage, Settings, Unit};
use openpid_dsp::spectrum::{WelchParams, welch};
use openpid_dsp::xcorr::lag_of_max;
use openpid_dsp::{Fft, Window};
use openpid_sim::{
    ChainKind, ChainModel, ChainPart, Complex, Drive, DynLowpass, DynNotchModel, Filter,
    LowpassModel, OperatingPoint, PartGroup, RpmNotchModel, track_peaks,
};
use serde::{Deserialize, Serialize};

/// What-if changes on top of the chain the log flew with. `None` keeps the
/// log's value; a zero cutoff, harmonic count or notch count switches that
/// stage off.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FilterOverrides {
    /// Low-pass 1 static cutoff, Hz (0 = off). Clears the dynamic range unless `lpf1_dyn_hz` is set.
    pub lpf1_hz: Option<f64>,
    /// Low-pass 1 topology.
    pub lpf1_type: Option<LowpassKind>,
    /// Low-pass 1 throttle-driven range (min, max), Hz; (0, 0) = static.
    pub lpf1_dyn_hz: Option<(f64, f64)>,
    /// Low-pass 2 cutoff, Hz (0 = off).
    pub lpf2_hz: Option<f64>,
    /// Low-pass 2 topology.
    pub lpf2_type: Option<LowpassKind>,
    /// RPM notch harmonics per motor (0 = off).
    pub rpm_harmonics: Option<u32>,
    /// RPM notch quality.
    pub rpm_q: Option<f64>,
    /// RPM notch minimum frequency, Hz.
    pub rpm_min_hz: Option<f64>,
    /// Dynamic notches per axis (0 = off).
    pub dyn_count: Option<u32>,
    /// Dynamic notch quality.
    pub dyn_q: Option<f64>,
    /// Dynamic notch lowest frequency, Hz.
    pub dyn_min_hz: Option<f64>,
    /// Dynamic notch highest frequency, Hz.
    pub dyn_max_hz: Option<f64>,
    /// One static notch centre, Hz, replacing the log's (0 = none).
    pub notch_hz: Option<f64>,
    /// Cutoff of that notch, Hz (default 0.7 × centre).
    pub notch_cutoff_hz: Option<f64>,
}

/// Low-pass topology from its Betaflight code (0 PT1, 1 biquad, 2 PT2, 3 PT3).
fn lowpass_kind(code: f64) -> Result<LowpassKind, String> {
    Ok(match code as i64 {
        0 => LowpassKind::Pt1,
        1 => LowpassKind::Biquad,
        2 => LowpassKind::Pt2,
        3 => LowpassKind::Pt3,
        other => {
            return Err(format!(
                "unknown low-pass type {other} (0 PT1, 1 biquad, 2 PT2, 3 PT3)"
            ));
        }
    })
}

impl FilterOverrides {
    /// Keys accepted by [`FilterOverrides::set`] (the CLI `--set` names).
    pub const KEYS: [&'static str; 15] = [
        "lpf1_hz",
        "lpf1_type",
        "lpf1_dyn_min_hz",
        "lpf1_dyn_max_hz",
        "lpf2_hz",
        "lpf2_type",
        "rpm_harmonics",
        "rpm_q",
        "rpm_min_hz",
        "dyn_count",
        "dyn_q",
        "dyn_min_hz",
        "dyn_max_hz",
        "notch_hz",
        "notch_cutoff_hz",
    ];

    /// Whether nothing is overridden.
    pub fn is_empty(&self) -> bool {
        *self == FilterOverrides::default()
    }

    /// Set one override by key; values are numbers (types by their code).
    ///
    /// # Errors
    /// An unknown key, an unknown type code or a negative value.
    pub fn set(&mut self, key: &str, value: f64) -> Result<(), String> {
        if !value.is_finite() || value < 0.0 {
            return Err(format!("{key}: value must be a non-negative number"));
        }
        match key {
            "lpf1_hz" => self.lpf1_hz = Some(value),
            "lpf1_type" => self.lpf1_type = Some(lowpass_kind(value)?),
            "lpf1_dyn_min_hz" => {
                self.lpf1_dyn_hz = Some((value, self.lpf1_dyn_hz.map_or(0.0, |d| d.1)))
            }
            "lpf1_dyn_max_hz" => {
                self.lpf1_dyn_hz = Some((self.lpf1_dyn_hz.map_or(0.0, |d| d.0), value))
            }
            "lpf2_hz" => self.lpf2_hz = Some(value),
            "lpf2_type" => self.lpf2_type = Some(lowpass_kind(value)?),
            "rpm_harmonics" => self.rpm_harmonics = Some(value as u32),
            "rpm_q" => self.rpm_q = Some(value),
            "rpm_min_hz" => self.rpm_min_hz = Some(value),
            "dyn_count" => self.dyn_count = Some(value as u32),
            "dyn_q" => self.dyn_q = Some(value),
            "dyn_min_hz" => self.dyn_min_hz = Some(value),
            "dyn_max_hz" => self.dyn_max_hz = Some(value),
            "notch_hz" => self.notch_hz = Some(value),
            "notch_cutoff_hz" => self.notch_cutoff_hz = Some(value),
            other => {
                return Err(format!(
                    "unknown filter key {other}; known: {}",
                    Self::KEYS.join(", ")
                ));
            }
        }
        Ok(())
    }

    /// Apply the overrides to `model` (the chain as flown).
    pub fn apply(&self, model: &mut ChainModel) {
        let fs = model.fs;
        let lowpass = |slot: &mut Option<LowpassModel>,
                       hz: Option<f64>,
                       kind: Option<&LowpassKind>,
                       dynamic: Option<(f64, f64)>| {
            if hz == Some(0.0) {
                *slot = None;
                return;
            }
            if hz.is_none() && kind.is_none() && dynamic.is_none() {
                return;
            }
            let l = slot.get_or_insert_with(|| LowpassModel {
                kind: LowpassKind::Pt1,
                cutoff_hz: 0.0,
                dynamic: None,
            });
            if let Some(hz) = hz {
                l.cutoff_hz = hz;
                if dynamic.is_none() {
                    l.dynamic = None;
                }
            }
            if let Some(k) = kind {
                l.kind = k.clone();
            }
            if let Some((min, max)) = dynamic {
                l.dynamic = if min > 0.0 && max > min {
                    Some(DynLowpass {
                        min_hz: min,
                        max_hz: max,
                        expo: l.dynamic.as_ref().map_or(0.5, |d| d.expo),
                    })
                } else {
                    None
                };
                if l.cutoff_hz <= 0.0 && l.dynamic.is_none() {
                    l.cutoff_hz = max.max(min);
                }
            }
            if l.cutoff_hz <= 0.0 && l.dynamic.is_none() {
                *slot = None;
            }
        };
        lowpass(
            &mut model.lowpass1,
            self.lpf1_hz,
            self.lpf1_type.as_ref(),
            self.lpf1_dyn_hz,
        );
        lowpass(
            &mut model.lowpass2,
            self.lpf2_hz,
            self.lpf2_type.as_ref(),
            None,
        );
        if self.rpm_harmonics == Some(0) {
            model.rpm_notch = None;
        } else if self.rpm_harmonics.is_some() || self.rpm_q.is_some() || self.rpm_min_hz.is_some()
        {
            let r = model.rpm_notch.get_or_insert_with(|| RpmNotchModel {
                harmonics: 3,
                q: 5.0,
                min_hz: 100.0,
                fade_range_hz: 50.0,
                weights: vec![1.0; 3],
                rpm_lpf_hz: 150.0,
            });
            if let Some(n) = self.rpm_harmonics {
                r.harmonics = n;
                r.weights.resize(n as usize, 1.0);
            }
            if let Some(q) = self.rpm_q {
                r.q = q.max(0.1);
            }
            if let Some(m) = self.rpm_min_hz {
                r.min_hz = m;
            }
        }
        if self.dyn_count == Some(0) {
            model.dyn_notch = None;
        } else if self.dyn_count.is_some()
            || self.dyn_q.is_some()
            || self.dyn_min_hz.is_some()
            || self.dyn_max_hz.is_some()
        {
            let d = model.dyn_notch.get_or_insert(DynNotchModel {
                count: 1,
                q: 3.0,
                min_hz: 150.0,
                max_hz: 600.0,
            });
            if let Some(n) = self.dyn_count {
                d.count = n;
            }
            if let Some(q) = self.dyn_q {
                d.q = q.max(0.1);
            }
            if let Some(m) = self.dyn_min_hz {
                d.min_hz = m;
            }
            if let Some(m) = self.dyn_max_hz {
                d.max_hz = m;
            }
            if d.max_hz <= d.min_hz || d.min_hz <= 0.0 {
                model.dyn_notch = None;
            }
        }
        if let Some(hz) = self.notch_hz {
            model.notches.clear();
            if hz > 0.0 && hz < 0.48 * fs {
                let cutoff = self.notch_cutoff_hz.unwrap_or(hz * 0.7);
                if cutoff > 0.0 && cutoff < hz {
                    model.notches.push(Notch {
                        center_hz: hz,
                        cutoff_hz: cutoff,
                    });
                }
            }
        } else if let (Some(cutoff), Some(n)) = (self.notch_cutoff_hz, model.notches.first_mut())
            && cutoff > 0.0
            && cutoff < n.center_hz
        {
            n.cutoff_hz = cutoff;
        }
    }

    fn hash_into<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::Hash;
        let bits = |v: Option<f64>| v.map(f64::to_bits);
        bits(self.lpf1_hz).hash(state);
        format!("{:?}", self.lpf1_type).hash(state);
        self.lpf1_dyn_hz
            .map(|(a, b)| (a.to_bits(), b.to_bits()))
            .hash(state);
        bits(self.lpf2_hz).hash(state);
        format!("{:?}", self.lpf2_type).hash(state);
        self.rpm_harmonics.hash(state);
        bits(self.rpm_q).hash(state);
        bits(self.rpm_min_hz).hash(state);
        self.dyn_count.hash(state);
        bits(self.dyn_q).hash(state);
        bits(self.dyn_min_hz).hash(state);
        bits(self.dyn_max_hz).hash(state);
        bits(self.notch_hz).hash(state);
        bits(self.notch_cutoff_hz).hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Parameters of [`FilterSimAnalysis`].
#[serde(default)]
pub struct FilterSimParams {
    /// Axis to simulate.
    pub axis: Axis,
    /// What-if changes on top of the log's chain.
    pub overrides: FilterOverrides,
    /// Welch segment length for the spectra, seconds.
    pub segment_s: f64,
    /// Upper frequency of the spectra, Hz (`None` = Nyquist).
    pub max_hz: Option<f64>,
    /// The frequency the printed delays and phase lags are read at, Hz
    /// ([`DEFAULT_DELAY_AT_HZ`] as the reference tool prints them; 1 Hz for
    /// the low end of the curve).
    pub delay_at_hz: f64,
}

/// The frequency the printed delays are read at unless asked otherwise, Hz:
/// the reference tool's web edition 1.0 prints them at 25 Hz (D-015).
pub const DEFAULT_DELAY_AT_HZ: f64 = 25.0;

impl Default for FilterSimParams {
    fn default() -> Self {
        FilterSimParams {
            axis: Axis::Roll,
            overrides: FilterOverrides::default(),
            segment_s: 0.25,
            max_hz: None,
            delay_at_hz: DEFAULT_DELAY_AT_HZ,
        }
    }
}

impl std::hash::Hash for FilterSimParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.axis.hash(state);
        self.overrides.hash_into(state);
        self.segment_s.to_bits().hash(state);
        self.max_hz.map(f64::to_bits).hash(state);
        self.delay_at_hz.to_bits().hash(state);
    }
}

/// How well the chain as flown reproduces the logged filtered gyro.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilterFit {
    /// RMS of `sim_as_flown − logged` after the first second, deg/s.
    pub residual_rms: f64,
    /// `residual_rms` relative to the RMS of the logged filtered gyro.
    pub residual_rel: f64,
    /// RMS of `raw − logged` relative to the logged RMS (what the filters remove).
    pub unfiltered_rel: f64,
    /// Lag of the simulated gyro behind the logged one at the cross-correlation
    /// peak, ms (negative = the simulation leads).
    pub delay_ms: f64,
}

/// Where the rate a part's curves were computed at comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateSource {
    /// The craft's gyro rate, from the log's headers.
    GyroRate,
    /// The craft's PID loop rate, from the log's headers.
    PidLoopRate,
    /// The rate of the log, because the headers do not give the rate of the
    /// loop the part runs in.
    LogRateNoLoopRate,
    /// The rate of the log, because the headers give the loop a rate below
    /// that of the log, which a loop that writes the log cannot have.
    LogRateLoopBelowLog,
}

impl RateSource {
    /// Whether the rate is the craft's own (not the fallback to the log's).
    pub fn is_craft_rate(self) -> bool {
        matches!(self, RateSource::GyroRate | RateSource::PidLoopRate)
    }
}

/// The rate the curves of one part of the chain were computed at, and why
/// that one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PartRate {
    /// The rate, Hz.
    pub hz: f64,
    /// Where it comes from.
    pub source: RateSource,
}

/// How far below the rate of the log a loop rate of the headers may read and
/// still be used: the headers give a nominal rate and the log's is measured
/// from its time stamps, so a log written at the loop rate can read a little
/// above it.
const LOOP_RATE_TOLERANCE: f64 = 0.05;

/// The rate the curves of `stage` are computed at for a log sampled at
/// `log_hz`: the rate of the loop the stage runs in on the craft
/// ([`Settings::gyro_filter_rate`]), or the rate of the log when the headers
/// give that loop no usable rate (none, zero, or one below the log's).
pub fn curve_rate(settings: &Settings, stage: FilterStage, log_hz: f64) -> PartRate {
    let (control_loop, hz) = settings.gyro_filter_rate(stage);
    let Some(hz) = hz.filter(|hz| hz.is_finite() && *hz > 0.0) else {
        return PartRate {
            hz: log_hz,
            source: RateSource::LogRateNoLoopRate,
        };
    };
    if hz < log_hz * (1.0 - LOOP_RATE_TOLERANCE) {
        return PartRate {
            hz: log_hz,
            source: RateSource::LogRateLoopBelowLog,
        };
    }
    PartRate {
        hz,
        source: match control_loop {
            ControlLoop::Gyro => RateSource::GyroRate,
            ControlLoop::Pid => RateSource::PidLoopRate,
        },
    }
}

/// A delay as the reference tool prints it: the phase delay of a filter at
/// one frequency and the phase lag it stands for there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PrintedDelay {
    /// Phase delay `−phase / (2π f)`, ms.
    pub delay_ms: f64,
    /// Phase lag `−phase`, degrees: `delay_ms · f · 0.36`.
    pub lag_deg: f64,
}

impl PrintedDelay {
    /// The printed delay of `h` at `f_hz`, from its phase unwrapped from the
    /// low end up, so that a lag past half a turn still reads as one.
    fn of(h: impl Fn(f64) -> Complex<f64>, f_hz: f64) -> PrintedDelay {
        const STEPS: u32 = 64;
        let mut phase = 0.0;
        let mut prev: Option<f64> = None;
        for i in 1..=STEPS {
            let phi = h(f_hz * f64::from(i) / f64::from(STEPS)).arg();
            phase = match prev {
                Some(p) => phase + wrap(phi - p),
                None => phi,
            };
            prev = Some(phi);
        }
        PrintedDelay::from_phase(phase, f_hz)
    }

    /// The printed delay of a phase of `phase` radians at `f_hz`.
    fn from_phase(phase: f64, f_hz: f64) -> PrintedDelay {
        PrintedDelay {
            delay_ms: -phase / (2.0 * std::f64::consts::PI * f_hz) * 1000.0,
            lag_deg: -phase.to_degrees(),
        }
    }

    /// The sum of `delays`: delays and lags add.
    fn sum<'a>(delays: impl IntoIterator<Item = &'a PrintedDelay>) -> PrintedDelay {
        delays
            .into_iter()
            .fold(PrintedDelay::default(), |a, d| PrintedDelay {
                delay_ms: a.delay_ms + d.delay_ms,
                lag_deg: a.lag_deg + d.lag_deg,
            })
    }
}

/// `a` brought into `(−π, π]`.
fn wrap(a: f64) -> f64 {
    let turn = 2.0 * std::f64::consts::PI;
    a - turn * (a / turn).round()
}

/// One harmonic of the RPM notches as the reference tool prints it: the
/// notch of one motor, its weight and its delay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarmonicDelay {
    /// The harmonic, from 1 (the motor frequency itself).
    pub harmonic: u32,
    /// Its centre, Hz: the mean over the motors that have it.
    pub hz: f64,
    /// Its weight in 0..=1 (the fade with the motor frequency times the
    /// harmonic's own weight): the mean over those motors.
    pub weight: f64,
    /// How many motors have it.
    pub motors: usize,
    /// Its delay per motor: the mean over those motors.
    pub printed: PrintedDelay,
}

/// Frequency response of one part of a chain (or of the whole chain).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartResponse {
    /// What the part is ("LPF1 PT1 330 Hz", "total").
    pub label: String,
    /// Low-pass or notch; `None` for the whole chain.
    pub group: Option<PartGroup>,
    /// The rate the part's curves were computed at; `None` for the whole
    /// chain, whose parts each have their own.
    pub rate: Option<PartRate>,
    /// Magnitude at [`ChainResponse::freq`], dB.
    pub magnitude_db: Vec<f64>,
    /// Phase (unwrapped), degrees.
    pub phase_deg: Vec<f64>,
    /// Phase delay `−phase / (2π f)`, ms: how late a sine of that frequency
    /// comes out.
    pub delay_ms: Vec<f64>,
    /// The delay printed for the part, at [`ChainResponse::delay_at_hz`]. A
    /// part that holds several filters (every motor's RPM notches) adds the
    /// delays of all of them.
    pub printed: PrintedDelay,
    /// For the RPM part, each harmonic of one motor; empty for every other
    /// part.
    pub harmonics: Vec<HarmonicDelay>,
}

/// A chain's frequency response, part by part, at the flight's typical
/// operating point: the mean throttle, the mean speed of each motor and the
/// median centre of each dynamic notch over the epoch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainResponse {
    /// Frequency axis, Hz.
    pub freq: Vec<f64>,
    /// The parts in signal order.
    pub parts: Vec<PartResponse>,
    /// The whole chain.
    pub total: PartResponse,
    /// Mean throttle, 0..=1, when logged.
    pub throttle: Option<f64>,
    /// Mean frequency of each motor, Hz.
    pub motor_hz: Vec<f64>,
    /// Median centre of each dynamic notch, Hz.
    pub dyn_hz: Vec<f64>,
    /// Phase delay of the whole chain at 100 Hz, ms.
    pub delay_100hz_ms: f64,
    /// The frequency the printed delays are read at, Hz.
    pub delay_at_hz: f64,
    /// Sum of the printed delays ([`PartResponse::printed`]) of the low-pass
    /// parts.
    pub printed_lowpass: PrintedDelay,
    /// Sum of the printed delays of the notch parts (RPM, static and
    /// dynamic): an RPM notch counted once per motor.
    pub printed_notch: PrintedDelay,
    /// Sum of the two: the printed delay of the whole chain.
    pub printed_total: PrintedDelay,
}

/// Points of the response plots.
const RESPONSE_POINTS: usize = 400;

/// Phase delay of `h` at `f_hz`, ms.
fn phase_delay_ms(h: impl Fn(f64) -> Complex<f64>, f_hz: f64) -> f64 {
    -h(f_hz).arg() / (2.0 * std::f64::consts::PI * f_hz) * 1000.0
}

/// Magnitude, unwrapped phase and phase delay of `h` at `freq`, and its
/// printed delay at `delay_at_hz`.
fn part_response(
    label: String,
    group: Option<PartGroup>,
    rate: Option<PartRate>,
    freq: &[f64],
    delay_at_hz: f64,
    h: impl Fn(f64) -> Complex<f64>,
) -> PartResponse {
    let mut magnitude_db = Vec::with_capacity(freq.len());
    let mut phase_deg = Vec::with_capacity(freq.len());
    let mut delay_ms = Vec::with_capacity(freq.len());
    let mut prev: Option<f64> = None;
    for f in freq {
        let v = h(*f);
        magnitude_db.push(20.0 * v.norm().max(1e-9).log10());
        let mut phi = v.arg();
        if let Some(p) = prev {
            phi +=
                (2.0 * std::f64::consts::PI) * ((p - phi) / (2.0 * std::f64::consts::PI)).round();
        }
        prev = Some(phi);
        phase_deg.push(phi.to_degrees());
        delay_ms.push(-phi / (2.0 * std::f64::consts::PI * f) * 1000.0);
    }
    PartResponse {
        label,
        group,
        rate,
        magnitude_db,
        phase_deg,
        delay_ms,
        printed: PrintedDelay::of(&h, delay_at_hz),
        harmonics: Vec::new(),
    }
}

/// The harmonics of the RPM part `p` as the reference tool prints them:
/// per harmonic, the notch of one motor at `f_hz` (the mean over the motors
/// that have it).
fn harmonic_delays(p: &ChainPart, f_hz: f64) -> Vec<HarmonicDelay> {
    let mut out: Vec<HarmonicDelay> = Vec::new();
    for ((stage, weight), which) in p.stages.iter().zip(&p.harmonics) {
        let notch = |f: f64| stage.response(f) * *weight + Complex::new(1.0 - *weight, 0.0);
        let printed = PrintedDelay::of(notch, f_hz);
        let Some(h) = out.iter_mut().find(|h| h.harmonic == which.harmonic) else {
            out.push(HarmonicDelay {
                harmonic: which.harmonic,
                hz: which.hz,
                weight: *weight,
                motors: 1,
                printed,
            });
            continue;
        };
        h.hz += which.hz;
        h.weight += *weight;
        h.printed = PrintedDelay::sum([&h.printed, &printed]);
        h.motors += 1;
    }
    for h in &mut out {
        let n = h.motors as f64;
        h.hz /= n;
        h.weight /= n;
        h.printed.delay_ms /= n;
        h.printed.lag_deg /= n;
    }
    out.sort_by_key(|h| h.harmonic);
    out
}

/// The response of `model` at `op` on `freq`, part by part, every part at
/// the rate `rate` gives its stage, with the delays printed at
/// `delay_at_hz`: the curves and the figures of the Filter Sim tab.
pub fn chain_response(
    model: &ChainModel,
    op: &OperatingPoint,
    freq: &[f64],
    rate: &dyn Fn(FilterStage) -> PartRate,
    delay_at_hz: f64,
) -> ChainResponse {
    let parts_at = model.parts_at_rates(op, &|stage| rate(stage).hz);
    let parts: Vec<PartResponse> = parts_at
        .iter()
        .map(|p| {
            let mut r = part_response(
                p.label.clone(),
                Some(p.group),
                Some(rate(p.stage)),
                freq,
                delay_at_hz,
                |f| p.response(f),
            );
            r.harmonics = harmonic_delays(p, delay_at_hz);
            r
        })
        .collect();
    let all = |f: f64| {
        parts_at
            .iter()
            .fold(Complex::new(1.0, 0.0), |h, p| h * p.response(f))
    };
    let total = part_response("total".into(), None, None, freq, delay_at_hz, all);
    let printed = |group: PartGroup| {
        PrintedDelay::sum(
            parts
                .iter()
                .filter(|p| p.group == Some(group))
                .map(|p| &p.printed),
        )
    };
    let (printed_lowpass, printed_notch) = (printed(PartGroup::Lowpass), printed(PartGroup::Notch));
    ChainResponse {
        freq: freq.to_vec(),
        parts,
        total,
        throttle: op.throttle,
        motor_hz: op.motor_hz.clone(),
        dyn_hz: op.dyn_hz.clone(),
        delay_100hz_ms: phase_delay_ms(all, 100.0_f64.min(model.fs * 0.45)),
        delay_at_hz,
        printed_lowpass,
        printed_notch,
        printed_total: PrintedDelay::sum([&printed_lowpass, &printed_notch]),
    }
}

/// Median of `v` (0 when empty).
fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// The flight's typical operating point for the response of `model`.
fn operating_point(
    model: &ChainModel,
    x: &[f64],
    throttle: Option<&[f64]>,
    rpm: &[Vec<f64>],
) -> OperatingPoint {
    let mean = |v: &[f64]| (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64);
    let motor_hz = rpm
        .iter()
        .filter_map(|r| {
            let spinning: Vec<f64> = r.iter().filter(|v| **v > 0.0).map(|v| v / 60.0).collect();
            mean(&spinning)
        })
        .collect();
    let dyn_hz = match &model.dyn_notch {
        Some(d) if d.count > 0 => {
            let tracked = track_peaks(x, model.fs, d.min_hz, d.max_hz, d.count as usize);
            (0..d.count as usize)
                .map(|k| median(tracked.iter().filter_map(|c| c.get(k).copied()).collect()))
                .filter(|f| *f > 0.0)
                .collect()
        }
        _ => Vec::new(),
    };
    OperatingPoint {
        throttle: throttle.and_then(mean),
        motor_hz,
        dyn_hz,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`FilterSimAnalysis`].
pub struct FilterSimOutput {
    /// Simulated axis.
    pub axis: Axis,
    /// Sample rate of the log: the rate the simulated gyro, its spectra and
    /// the fit were computed at. The curves of [`FilterSimOutput::response`]
    /// have their own ([`PartResponse::rate`]).
    pub fs: f64,
    /// Seconds from the start of the session, per sample.
    pub time_s: Vec<f64>,
    /// Unfiltered gyro as logged, deg/s.
    pub raw: Vec<f64>,
    /// Filtered gyro as logged, deg/s (when the log has it).
    pub logged: Option<Vec<f64>>,
    /// The what-if chain's output, deg/s (equals `sim_as_flown` without overrides).
    pub sim: Vec<f64>,
    /// The chain as flown applied to `raw`, deg/s.
    pub sim_as_flown: Vec<f64>,
    /// Frequency axis of the spectra, Hz.
    pub freq: Vec<f64>,
    /// Welch PSD of `raw` in dB.
    pub psd_raw_db: Vec<f64>,
    /// Welch PSD of `logged` in dB.
    pub psd_logged_db: Option<Vec<f64>>,
    /// Welch PSD of `sim` in dB.
    pub psd_sim_db: Vec<f64>,
    /// Welch PSD of `sim_as_flown` in dB.
    pub psd_as_flown_db: Vec<f64>,
    /// Magnitude of the what-if chain's static part at `freq`, dB, at the
    /// rate of the log: what the simulation applies to the spectra beside it.
    pub response_db: Vec<f64>,
    /// The what-if chain's response, part by part, at the flight's typical
    /// operating point, every part at the rate of the loop it runs in on the
    /// craft.
    pub response: ChainResponse,
    /// The chain as flown, whole, when there are overrides.
    pub response_as_flown: Option<PartResponse>,
    /// The chain as flown.
    pub model: ChainModel,
    /// The chain with the overrides.
    pub whatif: ChainModel,
    /// One line per stage of `whatif`.
    pub description: Vec<String>,
    /// Validation against the logged filtered gyro, when the log has it.
    pub fit: Option<FilterFit>,
    /// Whether the throttle drove the dynamic low-pass.
    pub throttle_driven: bool,
    /// Motors whose rpm drove the RPM notches.
    pub motors: usize,
    /// Welch segments averaged in the spectra.
    pub segments: usize,
}

#[derive(Debug, Default, Clone, Copy)]
/// The unfiltered gyro through the modelled filter chain.
pub struct FilterSimAnalysis;

fn rms(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    (v.iter().map(|y| y * y).sum::<f64>() / v.len() as f64).sqrt()
}

/// Signed lag of `b` behind `a` at the cross-correlation peak, samples.
fn signed_lag(a: &[f64], b: &[f64], max_lag: usize) -> f64 {
    let forward = lag_of_max(a, b, max_lag).unwrap_or(0.0);
    if forward > 0.0 {
        return forward;
    }
    -lag_of_max(b, a, max_lag).unwrap_or(0.0)
}

fn optional(ctx: &AnalysisCtx<'_>, id: &ChannelId, unit: Option<&Unit>) -> Option<Prepared> {
    prepare(ctx, id, unit).ok()
}

impl Analysis for FilterSimAnalysis {
    type Params = FilterSimParams;
    type Output = FilterSimOutput;

    fn id(&self) -> &'static str {
        "filter-sim"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::Channel(ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Raw,
        })]
    }

    fn run(
        &self,
        ctx: &AnalysisCtx<'_>,
        p: &FilterSimParams,
    ) -> Result<FilterSimOutput, AnalysisError> {
        if p.segment_s <= 0.0 {
            return Err(AnalysisError::InvalidParams("segment_s > 0".into()));
        }
        if !p.delay_at_hz.is_finite() || p.delay_at_hz <= 0.0 {
            return Err(AnalysisError::InvalidParams("delay_at_hz > 0".into()));
        }
        let raw = prepare(
            ctx,
            &ChannelId::Gyro {
                axis: p.axis,
                stage: GyroStage::Raw,
            },
            Some(&Unit::DegreesPerSecond),
        )?;
        let fs = raw.fs;
        let x = raw.filled();
        let nperseg = openpid_dsp::next_smooth((p.segment_s * fs).round().max(16.0) as usize);
        if x.len() < nperseg.max(2) {
            return Err(AnalysisError::NotEnoughData(format!(
                "{} samples, segment needs {nperseg}",
                x.len()
            )));
        }
        let logged = optional(
            ctx,
            &ChannelId::Gyro {
                axis: p.axis,
                stage: GyroStage::Filtered,
            },
            Some(&Unit::DegreesPerSecond),
        )
        .map(|l| l.filled());
        let throttle: Option<Vec<f64>> = optional(ctx, &ChannelId::Throttle, Some(&Unit::Percent))
            .map(|t| t.filled().into_iter().map(|v| v / 100.0).collect());
        let rpm: Vec<Vec<f64>> = (0..16u8)
            .map_while(|idx| optional(ctx, &ChannelId::MotorRpm { idx }, None).map(|r| r.filled()))
            .collect();
        let model = ChainModel::from_settings(&ctx.session.settings, ChainKind::Gyro, fs);
        let mut whatif = model.clone();
        p.overrides.apply(&mut whatif);
        let drive = Drive {
            throttle: throttle.as_deref(),
            rpm: rpm.iter().map(Vec::as_slice).collect(),
            dyn_centers: None,
        };
        let sim_as_flown = model.run(&x, &drive);
        let sim = if p.overrides.is_empty() {
            sim_as_flown.clone()
        } else {
            whatif.run(&x, &drive)
        };

        let fft = Fft::new();
        let wp = WelchParams {
            nperseg,
            noverlap: nperseg / 2,
            window: Window::Hann,
            detrend: true,
        };
        let psd_db = |v: &[f64]| -> Result<(Vec<f64>, Vec<f64>, usize), AnalysisError> {
            let psd =
                welch(&fft, v, fs, &wp).map_err(|e| AnalysisError::InvalidParams(e.to_string()))?;
            let keep = p
                .max_hz
                .map_or(psd.freq.len(), |m| psd.freq.partition_point(|f| *f <= m));
            Ok((
                psd.freq[..keep].to_vec(),
                psd.power[..keep]
                    .iter()
                    .map(|v| 10.0 * v.max(1e-30).log10())
                    .collect(),
                psd.segments,
            ))
        };
        let (freq, psd_raw_db, segments) = psd_db(&x)?;
        let psd_logged_db = match &logged {
            Some(l) => Some(psd_db(l)?.1),
            None => None,
        };
        let psd_sim_db = psd_db(&sim)?.1;
        let psd_as_flown_db = if p.overrides.is_empty() {
            psd_sim_db.clone()
        } else {
            psd_db(&sim_as_flown)?.1
        };
        let static_chain = whatif.static_chain();
        let response_db: Vec<f64> = freq.iter().map(|f| static_chain.gain_db(*f)).collect();
        let top = p.max_hz.unwrap_or(fs / 2.0).min(fs / 2.0);
        let rfreq: Vec<f64> = (1..=RESPONSE_POINTS)
            .map(|i| top * i as f64 / RESPONSE_POINTS as f64)
            .collect();
        // the curves: the craft's filters, each at the rate of its loop
        let rate = |stage| curve_rate(&ctx.session.settings, stage, fs);
        let op = operating_point(&whatif, &x, throttle.as_deref(), &rpm);
        let response = chain_response(&whatif, &op, &rfreq, &rate, p.delay_at_hz);
        let response_as_flown = (!p.overrides.is_empty()).then(|| {
            let op = operating_point(&model, &x, throttle.as_deref(), &rpm);
            let mut r = chain_response(&model, &op, &rfreq, &rate, p.delay_at_hz).total;
            r.label = "total as flown".into();
            r
        });

        let fit = logged.as_ref().map(|l| {
            let from = (fs as usize).min(x.len() / 4);
            let res: Vec<f64> = sim_as_flown[from..]
                .iter()
                .zip(&l[from..])
                .map(|(a, b)| a - b)
                .collect();
            let unf: Vec<f64> = x[from..]
                .iter()
                .zip(&l[from..])
                .map(|(a, b)| a - b)
                .collect();
            let level = rms(&l[from..]).max(1e-9);
            let max_lag = ((0.02 * fs).round() as usize).max(1);
            FilterFit {
                residual_rms: rms(&res),
                residual_rel: rms(&res) / level,
                unfiltered_rel: rms(&unf) / level,
                delay_ms: signed_lag(&l[from..], &sim_as_flown[from..], max_lag) * 1000.0 / fs,
            }
        });
        let time_s: Vec<f64> = (0..x.len()).map(|i| raw.t0_s + i as f64 / fs).collect();
        Ok(FilterSimOutput {
            axis: p.axis,
            fs,
            time_s,
            raw: x,
            logged,
            sim,
            sim_as_flown,
            freq,
            psd_raw_db,
            psd_logged_db,
            psd_sim_db,
            psd_as_flown_db,
            response_db,
            response,
            response_as_flown,
            description: whatif.describe(),
            model,
            whatif,
            fit,
            throttle_driven: throttle.is_some(),
            motors: rpm.len(),
            segments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> ChainModel {
        let mut s = Settings::default();
        s.gyro_filters.lowpass1 = Some(openpid_core::settings::Lowpass {
            kind: Some(LowpassKind::Pt1),
            cutoff_hz: 250.0,
            dyn_min_hz: Some(250.0),
            dyn_max_hz: Some(500.0),
            dyn_expo: 0.0,
        });
        s.gyro_filters.notches.push(Notch {
            center_hz: 200.0,
            cutoff_hz: 150.0,
        });
        ChainModel::from_settings(&s, ChainKind::Gyro, 1000.0)
    }

    /// Every stage at the rate of the log, as when the headers give none.
    fn at_log_rate(fs: f64) -> PartRate {
        PartRate {
            hz: fs,
            source: RateSource::LogRateNoLoopRate,
        }
    }

    #[test]
    fn overrides_edit_every_stage() {
        let mut o = FilterOverrides::default();
        assert!(o.is_empty());
        for k in FilterOverrides::KEYS {
            let v = if k.ends_with("_type") { 2.0 } else { 100.0 };
            o.set(k, v).unwrap();
        }
        assert!(!o.is_empty());
        assert!(
            o.set("nope", 1.0)
                .unwrap_err()
                .contains("unknown filter key")
        );
        assert!(o.set("lpf1_type", 9.0).is_err());
        assert!(o.set("lpf1_hz", -1.0).is_err());
        assert!(o.set("lpf1_hz", f64::NAN).is_err());

        // switch stages off
        let mut off = FilterOverrides::default();
        off.set("lpf1_hz", 0.0).unwrap();
        off.set("notch_hz", 0.0).unwrap();
        off.set("rpm_harmonics", 0.0).unwrap();
        off.set("dyn_count", 0.0).unwrap();
        let mut m = model();
        off.apply(&mut m);
        assert!(m.lowpass1.is_none() && m.notches.is_empty());
        assert!(m.rpm_notch.is_none() && m.dyn_notch.is_none());

        // a static cutoff clears the dynamic range, a type changes the topology
        let mut st = FilterOverrides::default();
        st.set("lpf1_hz", 120.0).unwrap();
        st.set("lpf1_type", 1.0).unwrap();
        let mut m = model();
        st.apply(&mut m);
        let l = m.lowpass1.unwrap();
        assert_eq!(
            (l.cutoff_hz, l.kind, l.dynamic),
            (120.0, LowpassKind::Biquad, None)
        );

        // a dynamic range keeps the expo, lpf2 appears from nothing
        let mut dynr = FilterOverrides::default();
        dynr.set("lpf1_dyn_min_hz", 100.0).unwrap();
        dynr.set("lpf1_dyn_max_hz", 400.0).unwrap();
        dynr.set("lpf2_hz", 300.0).unwrap();
        dynr.set("lpf2_type", 3.0).unwrap();
        let mut m = model();
        m.lowpass1.as_mut().unwrap().dynamic.as_mut().unwrap().expo = 0.7;
        dynr.apply(&mut m);
        let d = m.lowpass1.unwrap().dynamic.unwrap();
        assert_eq!((d.min_hz, d.max_hz, d.expo), (100.0, 400.0, 0.7));
        let l2 = m.lowpass2.unwrap();
        assert_eq!((l2.cutoff_hz, l2.kind), (300.0, LowpassKind::Pt3));

        // rpm and dynamic notches from nothing, with defaults filled in
        let mut n = FilterOverrides::default();
        n.set("rpm_harmonics", 2.0).unwrap();
        n.set("rpm_q", 8.0).unwrap();
        n.set("dyn_count", 3.0).unwrap();
        n.set("dyn_max_hz", 700.0).unwrap();
        n.set("notch_hz", 300.0).unwrap();
        let mut m = model();
        n.apply(&mut m);
        let r = m.rpm_notch.unwrap();
        assert_eq!((r.harmonics, r.q, r.weights.len()), (2, 8.0, 2));
        let dn = m.dyn_notch.unwrap();
        assert_eq!((dn.count, dn.max_hz, dn.min_hz), (3, 700.0, 150.0));
        assert_eq!(m.notches.len(), 1);
        assert!(
            (m.notches[0].cutoff_hz - 210.0).abs() < 1e-9,
            "0.7 × centre"
        );
        // a notch cutoff alone edits the log's notch
        let mut c = FilterOverrides::default();
        c.set("notch_cutoff_hz", 120.0).unwrap();
        let mut m = model();
        c.apply(&mut m);
        assert_eq!(m.notches[0].cutoff_hz, 120.0);
        // an inverted dynamic notch band switches it off
        let mut bad = FilterOverrides::default();
        bad.set("dyn_count", 1.0).unwrap();
        bad.set("dyn_min_hz", 800.0).unwrap();
        let mut m = model();
        bad.apply(&mut m);
        assert!(m.dyn_notch.is_none());
    }

    #[test]
    fn params_hash_follows_the_overrides() {
        use std::hash::{Hash, Hasher};
        let hash = |p: &FilterSimParams| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            p.hash(&mut h);
            h.finish()
        };
        let a = FilterSimParams::default();
        let mut b = a.clone();
        b.overrides.set("lpf1_hz", 90.0).unwrap();
        assert_ne!(hash(&a), hash(&b));
        assert_eq!(hash(&a), hash(&FilterSimParams::default()));
        let c = FilterSimParams {
            axis: Axis::Yaw,
            ..Default::default()
        };
        assert_ne!(hash(&a), hash(&c));
        assert_eq!(FilterSimAnalysis.id(), "filter-sim");
        assert_eq!(FilterSimAnalysis.requires().len(), 1);
    }

    #[test]
    fn chain_response_has_delay_and_notch_depth() {
        let m = model();
        let op = OperatingPoint {
            throttle: Some(0.0),
            ..OperatingPoint::default()
        };
        let freq: Vec<f64> = (1..=500).map(f64::from).collect();
        let r = chain_response(&m, &op, &freq, &|_| at_log_rate(m.fs), LOW_END_HZ);
        let labels: Vec<&str> = r.parts.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["Notch 200 Hz", "LPF1 PT1 250 Hz"]);
        assert_eq!(r.total.group, None);
        // the notch removes its centre; the low-pass passes DC
        assert!(r.parts[0].magnitude_db[199] < -40.0);
        assert!(r.parts[1].magnitude_db[0].abs() < 0.01);
        // a PT1 delays every frequency, less and less as it rises
        let lp = &r.parts[1].delay_ms;
        assert!(lp[0] > 0.5 && lp[0] < 0.7, "{}", lp[0]);
        assert!(lp[400] < lp[0]);
        // a notch delays below its centre and leads above it
        let n = &r.parts[0].delay_ms;
        assert!(n[180] > 0.0 && n[220] < 0.0, "{} {}", n[180], n[220]);
        // the whole chain: the sum of the parts' phases
        let sum = r.parts[0].phase_deg[99] + r.parts[1].phase_deg[99];
        assert!((r.total.phase_deg[99] - sum).abs() < 1e-6);
        assert!((r.delay_100hz_ms - r.total.delay_ms[99]).abs() < 1e-9);
        assert_eq!(median(vec![]), 0.0);
        assert_eq!(median(vec![3.0, 1.0, 2.0]), 2.0);
    }

    // ---- the curves at the craft's rates and the printed delays (D-007)

    /// Every stage at `hz`, as a craft whose two loops run at one rate.
    fn at_gyro_rate(hz: f64) -> PartRate {
        PartRate {
            hz,
            source: RateSource::GyroRate,
        }
    }

    /// Whether `value` prints as `text`, at the decimals `text` is given to.
    fn prints(value: f64, text: &str) -> bool {
        let decimals = text.split('.').nth(1).map_or(0, str::len);
        format!("{value:.decimals$}") == text
    }

    /// The low end of the curve, where version 0.94 of the reference tool
    /// read its printed delays.
    const LOW_END_HZ: f64 = 1.0;

    /// `e^(−j·2π·f / fs)`.
    fn z(fs: f64, f: f64) -> Complex<f64> {
        Complex::from_polar(1.0, -2.0 * std::f64::consts::PI * f / fs)
    }

    /// The printed delay of a first-order low-pass at `f`, from its
    /// definition and nothing of the model: `H = k / (1 − (1 − k)·z⁻¹)` with
    /// `k = dt / (1 / (2π·fc) + dt)`. Towards 0 Hz this is the time constant
    /// `1 / (2π·fc)` (0.63662 ms at 250 Hz); higher up it is less.
    fn pt1_at(fc: f64, fs: f64, f: f64) -> PrintedDelay {
        let dt = 1.0 / fs;
        let k = dt / (1.0 / (2.0 * std::f64::consts::PI * fc) + dt);
        let h = Complex::new(k, 0.0) / (Complex::new(1.0, 0.0) - z(fs, f) * (1.0 - k));
        PrintedDelay::from_phase(h.arg(), f)
    }

    /// [`pt1_at`] at the low end of the curve, ms.
    fn pt1_ms(fc: f64, fs: f64) -> f64 {
        pt1_at(fc, fs, LOW_END_HZ).delay_ms
    }

    /// The printed delay of a notch at `f`, from its definition: `b = [1,
    /// −2·cos w, 1]`, `a = [1 + α, −2·cos w, 1 − α]`, `w = 2π·f0 / fs`,
    /// `α = sin w / (2·Q)`, blended with the input by `weight`:
    /// `(1 − weight) + weight·H`. Towards 0 Hz the plain notch delays by
    /// `dt · cot(w / 2) / (2·Q)` (0.31815 ms at 100 Hz, Q 5, 8 kHz); at 1 Hz
    /// it is a hair more.
    fn notch_at(f0: f64, q: f64, fs: f64, f: f64, weight: f64) -> PrintedDelay {
        PrintedDelay::from_phase(notch_h(f0, q, fs, f, weight).arg(), f)
    }

    /// The response at `f` of the weighted notch of [`notch_at`].
    fn notch_h(f0: f64, q: f64, fs: f64, f: f64, weight: f64) -> Complex<f64> {
        let w = 2.0 * std::f64::consts::PI * f0 / fs;
        let (alpha, c) = (w.sin() / (2.0 * q), w.cos());
        let (z, one) = (z(fs, f), Complex::new(1.0, 0.0));
        let h = (one - z * (2.0 * c) + z * z)
            / (one * (1.0 + alpha) - z * (2.0 * c) + z * z * (1.0 - alpha));
        one * (1.0 - weight) + h * weight
    }

    /// [`notch_at`] unweighted at the low end of the curve, ms.
    fn notch_ms(f0: f64, q: f64, fs: f64) -> f64 {
        notch_at(f0, q, fs, LOW_END_HZ, 1.0).delay_ms
    }

    /// The cut-off that gives a notch at `centre` the quality `q`: the
    /// positive root of `Q = fc·fcut / (fc² − fcut²)`.
    fn cutoff_for_q(centre: f64, q: f64) -> f64 {
        centre * ((1.0 + 4.0 * q * q).sqrt() - 1.0) / (2.0 * q)
    }

    fn lowpass_of(kind: LowpassKind, cutoff_hz: f64) -> LowpassModel {
        LowpassModel {
            kind,
            cutoff_hz,
            dynamic: None,
        }
    }

    /// A chain of two first-order low-passes, RPM notches switched hard at
    /// 50 Hz and one static notch given by its quality, in a log at `log_hz`.
    fn typed_chain(
        log_hz: f64,
        lowpass_hz: (f64, f64),
        rpm: (u32, f64),
        notch: (f64, f64),
    ) -> ChainModel {
        ChainModel {
            fs: log_hz,
            kind: ChainKind::Gyro,
            lowpass1: Some(lowpass_of(LowpassKind::Pt1, lowpass_hz.0)),
            lowpass2: Some(lowpass_of(LowpassKind::Pt1, lowpass_hz.1)),
            notches: vec![Notch {
                center_hz: notch.0,
                cutoff_hz: cutoff_for_q(notch.0, notch.1),
            }],
            rpm_notch: Some(RpmNotchModel {
                harmonics: rpm.0,
                q: rpm.1,
                min_hz: 50.0,
                fade_range_hz: 0.0,
                weights: vec![1.0; rpm.0 as usize],
                rpm_lpf_hz: 150.0,
            }),
            dyn_notch: None,
        }
    }

    /// The printed delays of `model` with `motors` motors at `motor_hz`,
    /// every stage at `rate_hz`, at the low end of the curve.
    fn printed(model: &ChainModel, motors: usize, motor_hz: f64, rate_hz: f64) -> ChainResponse {
        printed_at(model, motors, motor_hz, rate_hz, LOW_END_HZ)
    }

    /// [`printed`] read at `delay_at_hz`.
    fn printed_at(
        model: &ChainModel,
        motors: usize,
        motor_hz: f64,
        rate_hz: f64,
        delay_at_hz: f64,
    ) -> ChainResponse {
        let op = OperatingPoint {
            throttle: None,
            motor_hz: vec![motor_hz; motors],
            dyn_hz: Vec::new(),
        };
        chain_response(
            model,
            &op,
            &[100.0],
            &|_| at_gyro_rate(rate_hz),
            delay_at_hz,
        )
    }

    #[test]
    fn the_formulas_give_the_figures_the_reference_tool_prints() {
        // the definitions alone, before any of the model: loop rate 8 kHz
        assert!(prints(pt1_ms(250.0, 8000.0), "0.63662"));
        assert!(prints(pt1_ms(500.0, 8000.0), "0.31831"));
        assert!(prints(pt1_ms(100.0, 8000.0), "1.5915"));
        assert!(prints(notch_ms(100.0, 5.0, 8000.0), "0.31818"));
        assert!(prints(notch_ms(200.0, 5.0, 8000.0), "0.15883"));
        assert!(prints(notch_ms(300.0, 5.0, 8000.0), "0.10561"));
        assert!(prints(notch_ms(500.0, 5.0, 8000.0), "0.062842"));
        // the same notch at a loop rate of 2 kHz
        assert!(prints(notch_ms(500.0, 5.0, 2000.0), "0.0500"));
        // loop rate 4 kHz
        assert!(prints(pt1_ms(180.0, 4000.0), "0.88418"));
        assert!(prints(pt1_ms(320.0, 4000.0), "0.49736"));
        assert!(prints(notch_ms(130.0, 3.0, 4000.0), "0.40669"));
        assert!(prints(notch_ms(260.0, 3.0, 4000.0), "0.2012"));
        assert!(prints(notch_ms(240.0, 3.5, 4000.0), "0.18722"));
        // the helper of these tests: 500 Hz with a Q of 5 has its cut-off at
        // 452.49 Hz, and 452 Hz is a Q of 500·452 / (500² − 452²) = 4.946
        assert!((cutoff_for_q(500.0, 5.0) - 452.494).abs() < 1e-3);
    }

    #[test]
    fn printed_delays_at_8_khz_are_the_reference_figures() {
        // the reference tool's window as it comes up: low-passes at 250 and
        // 500 Hz, three RPM harmonics from 100 Hz with a Q of 5 on four
        // motors, a notch at 500 Hz with a Q of 5; the log itself at 2 kHz
        let m = typed_chain(2000.0, (250.0, 500.0), (3, 5.0), (500.0, 5.0));
        let r = printed(&m, 4, 100.0, 8000.0);
        let labels: Vec<&str> = r.parts.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "RPM 3 × 4 motors, 100 Hz",
                "Notch 500 Hz",
                "LPF1 PT1 250 Hz",
                "LPF2 PT1 500 Hz"
            ]
        );
        let (rpm, notch, lpf1, lpf2) = (&r.parts[0], &r.parts[1], &r.parts[2], &r.parts[3]);
        // each part is the definition's figure, to the last digit
        assert!((lpf1.printed.delay_ms - pt1_ms(250.0, 8000.0)).abs() < 1e-12);
        assert!(
            prints(lpf1.printed.delay_ms, "0.63662"),
            "{}",
            lpf1.printed.delay_ms
        );
        assert!(
            prints(lpf2.printed.delay_ms, "0.31831"),
            "{}",
            lpf2.printed.delay_ms
        );
        assert!((notch.printed.delay_ms - notch_ms(500.0, 5.0, 8000.0)).abs() < 1e-12);
        assert!(
            prints(notch.printed.delay_ms, "0.062842"),
            "{}",
            notch.printed.delay_ms
        );
        // the RPM part is the product of its twelve notches, so its delay is
        // the sum of theirs: four motors × (0.31818 + 0.15883 + 0.10561)
        let per_motor: f64 = [100.0, 200.0, 300.0]
            .iter()
            .map(|f| notch_ms(*f, 5.0, 8000.0))
            .sum();
        assert!((rpm.printed.delay_ms - 4.0 * per_motor).abs() < 1e-9);
        assert!(
            prints(rpm.printed.delay_ms, "2.3305"),
            "{}",
            rpm.printed.delay_ms
        );
        // the sums: 0.63662 + 0.31831; 2.3305 + 0.062842; both
        assert!(prints(r.printed_lowpass.delay_ms, "0.95492"));
        assert!(prints(r.printed_notch.delay_ms, "2.3933"));
        assert!(prints(r.printed_total.delay_ms, "3.3483"));
        // the sum of the parts is the delay of the whole chain: phases add
        assert!((r.total.printed.delay_ms - r.printed_total.delay_ms).abs() < 1e-9);
        assert_eq!(r.total.rate, None);
        assert!(r.parts.iter().all(|p| p.rate == Some(at_gyro_rate(8000.0))));
        // the delay at 100 Hz stays beside them, a figure of its own
        assert!((r.delay_100hz_ms - r.total.delay_ms[0]).abs() < 1e-12);
        assert!((r.delay_100hz_ms - r.printed_total.delay_ms).abs() > 0.1);

        // one motor: 0.58262 + 0.062842
        assert!(prints(
            printed(&m, 1, 100.0, 8000.0).printed_notch.delay_ms,
            "0.64546"
        ));
        // a harmonic alone, at 100, 200 and 300 Hz
        let one = typed_chain(2000.0, (250.0, 500.0), (1, 5.0), (500.0, 5.0));
        for (hz, text) in [(100.0, "0.31818"), (200.0, "0.15883"), (300.0, "0.10561")] {
            let rpm = printed(&one, 1, hz, 8000.0).parts[0].printed.delay_ms;
            assert!(prints(rpm, text), "{hz} Hz: {rpm}");
        }
        // the same chain computed at the 2 kHz of the log, as before this
        // decision: the notch at 500 Hz reads 0.0500 ms, the sums 0.95492
        // and 2.3210 ms (the reference tool set to 2 kHz prints 3.276 ms)
        let at_log = printed(&m, 4, 100.0, 2000.0);
        assert!(prints(at_log.parts[1].printed.delay_ms, "0.0500"));
        assert!(prints(at_log.printed_lowpass.delay_ms, "0.95492"));
        assert!(prints(at_log.printed_notch.delay_ms, "2.3210"));
        assert!(prints(at_log.printed_total.delay_ms, "3.276"));
    }

    #[test]
    fn printed_delays_of_the_other_types_and_of_a_4_khz_loop() {
        // a first-order low-pass at 100 Hz; the static notch as the headers
        // give it, by its cut-off: 500 and 452 Hz are a Q of 4.946
        let mut m = typed_chain(2000.0, (100.0, 500.0), (3, 5.0), (500.0, 5.0));
        m.notches[0].cutoff_hz = 452.0;
        let r = printed(&m, 4, 100.0, 8000.0);
        assert!(prints(r.parts[2].printed.delay_ms, "1.5915"));
        assert!((r.parts[1].printed.delay_ms - notch_ms(500.0, 4.945_728, 8000.0)).abs() < 1e-8);
        assert!(prints(r.parts[1].printed.delay_ms, "0.063532"));
        // two and three first-order stages at fc / sqrt(2^(1/n) − 1) (388.4
        // and 490.4 Hz for 250 Hz), and the second-order low-pass
        let pt2 = 2.0 * pt1_ms(250.0 / (2f64.sqrt() - 1.0).sqrt(), 8000.0);
        let pt3 = 3.0 * pt1_ms(250.0 / (2f64.cbrt() - 1.0).sqrt(), 8000.0);
        for (kind, text, reference) in [
            (LowpassKind::Pt2, "0.81945", Some(pt2)),
            (LowpassKind::Pt3, "0.97369", Some(pt3)),
            (LowpassKind::Biquad, "0.89743", None),
        ] {
            m.lowpass1 = Some(lowpass_of(kind.clone(), 250.0));
            let lpf1 = printed(&m, 4, 100.0, 8000.0).parts[2].printed.delay_ms;
            assert!(prints(lpf1, text), "{kind:?}: {lpf1}");
            if let Some(reference) = reference {
                assert!((lpf1 - reference).abs() < 1e-12, "{kind:?}");
            }
        }
        // the set made for no question: a 4 kHz loop, six motors with two
        // harmonics from 130 Hz and a Q of 3, low-passes at 180 and 320 Hz,
        // a notch at 240 Hz with a Q of 3.5
        let m = typed_chain(1000.0, (180.0, 320.0), (2, 3.0), (240.0, 3.5));
        let r = printed(&m, 6, 130.0, 4000.0);
        assert!(prints(r.parts[2].printed.delay_ms, "0.88418"));
        assert!(prints(r.parts[3].printed.delay_ms, "0.49736"));
        assert!(prints(r.printed_lowpass.delay_ms, "1.3815"));
        assert!(prints(r.parts[1].printed.delay_ms, "0.18722"));
        // six motors × (0.40669 + 0.2012) + 0.18722
        let rpm = 6.0 * (notch_ms(130.0, 3.0, 4000.0) + notch_ms(260.0, 3.0, 4000.0));
        assert!((r.parts[0].printed.delay_ms - rpm).abs() < 1e-9);
        assert!(prints(r.printed_notch.delay_ms, "3.8346"));
    }

    // ---- the printed delays at a chosen frequency and the RPM fade (D-015)

    /// `v` to four significant digits, trailing zeros dropped: how the
    /// reference tool prints a delay or a lag ("0.6339", "0.063", "1.62").
    fn g4(v: f64) -> String {
        if v == 0.0 || !v.is_finite() {
            return format!("{v}");
        }
        let decimals = (3 - v.abs().log10().floor() as i32).max(0) as usize;
        let text = format!("{v:.decimals$}");
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        }
    }

    /// The reference tool's window as it comes up: low-passes at 250 and
    /// 500 Hz, `harmonics` RPM harmonics with a Q of 5, a minimum of 100 Hz
    /// and a fade of 50 Hz, a notch at 500 Hz with a Q of 5.
    fn window_chain(harmonics: u32) -> ChainModel {
        let mut m = typed_chain(2000.0, (250.0, 500.0), (harmonics, 5.0), (500.0, 5.0));
        let r = m.rpm_notch.as_mut().unwrap();
        (r.min_hz, r.fade_range_hz) = (100.0, 50.0);
        m
    }

    /// Delay and lag of `d`, as printed.
    fn texts(d: &PrintedDelay) -> (String, String) {
        (g4(d.delay_ms), g4(d.lag_deg))
    }

    #[test]
    fn a_first_order_low_pass_delays_by_its_phase_at_the_chosen_frequency() {
        // the closed form of the phase of k / (1 − (1 − k)·e^(−jω)):
        // −atan2((1 − k)·sin ω, 1 − (1 − k)·cos ω), ω = 2π·f / fs
        let closed = |fc: f64, fs: f64, f: f64| {
            let k = (1.0 / fs) / (1.0 / (2.0 * std::f64::consts::PI * fc) + 1.0 / fs);
            let w = 2.0 * std::f64::consts::PI * f / fs;
            let lag = ((1.0 - k) * w.sin()).atan2(1.0 - (1.0 - k) * w.cos());
            (
                lag / (2.0 * std::f64::consts::PI * f) * 1000.0,
                lag.to_degrees(),
            )
        };
        let m = window_chain(3);
        for (fs, f) in [(8000.0, 1.0), (8000.0, 25.0), (4000.0, 25.0)] {
            let r = printed_at(&m, 4, 150.0, fs, f);
            for (part, fc) in [(&r.parts[2], 250.0), (&r.parts[3], 500.0)] {
                let (ms, deg) = closed(fc, fs, f);
                assert!(
                    (part.printed.delay_ms - ms).abs() < 1e-12,
                    "{fc} Hz at {f} Hz"
                );
                assert!(
                    (part.printed.lag_deg - deg).abs() < 1e-10,
                    "{fc} Hz at {f} Hz"
                );
                assert!((part.printed.delay_ms - pt1_at(fc, fs, f).delay_ms).abs() < 1e-12);
            }
            assert_eq!(r.delay_at_hz, f);
        }
        // 1 / (2π·250) = 0.63662 ms towards 0 Hz; 0.6366 at 1 Hz, 0.6339 at
        // 25 Hz, which is a lag of 0.6339 ms · 25 Hz · 0.36 = 5.705°
        assert_eq!(texts(&pt1_at(250.0, 8000.0, 1.0)).0, "0.6366");
        assert_eq!(
            texts(&pt1_at(250.0, 8000.0, 25.0)),
            ("0.6339".into(), "5.705".into())
        );
        assert_eq!(
            texts(&pt1_at(500.0, 8000.0, 25.0)),
            ("0.3179".into(), "2.861".into())
        );
        assert_eq!(texts(&pt1_at(250.0, 4000.0, 25.0)).0, "0.6331");
        // a lag of more than half a turn still reads as one: three PT1s at
        // 20 Hz read at 400 Hz lag by about 3 × 87°
        let lag = PrintedDelay::of(
            |f| {
                let h = Complex::new(1.0, 0.0) / Complex::new(1.0, f / 20.0);
                h * h * h
            },
            400.0,
        );
        assert!((lag.lag_deg - 3.0 * 20f64.atan().to_degrees()).abs() < 1e-9);
        assert!(lag.lag_deg > 180.0);
    }

    #[test]
    fn printed_delays_at_25_and_1_hz_as_the_reference_tool_prints_them() {
        // its window as it comes up: four motors at 150 Hz, 8 kHz, at 25 Hz
        let m = window_chain(3);
        let r = printed_at(&m, 4, 150.0, 8000.0, 25.0);
        let (rpm, notch) = (&r.parts[0], &r.parts[1]);
        assert_eq!(
            texts(&r.parts[2].printed),
            ("0.6339".into(), "5.705".into())
        );
        assert_eq!(
            texts(&r.parts[3].printed),
            ("0.3179".into(), "2.861".into())
        );
        assert_eq!(texts(&notch.printed), ("0.063".into(), "0.567".into()));
        // the RPM notches, per harmonic of one motor
        let per_motor: Vec<(u32, String, String, String, usize)> = rpm
            .harmonics
            .iter()
            .map(|h| {
                let (ms, deg) = texts(&h.printed);
                (
                    h.harmonic,
                    format!("{:.0} Hz · {:.0} %", h.hz, h.weight * 100.0),
                    ms,
                    deg,
                    h.motors,
                )
            })
            .collect();
        let row = |n, at: &str, ms: &str, deg: &str| {
            (n, at.to_string(), ms.to_string(), deg.to_string(), 4)
        };
        assert_eq!(
            per_motor,
            [
                row(1, "150 Hz · 100 %", "0.2179", "1.961"),
                row(2, "300 Hz · 100 %", "0.1063", "0.957"),
                row(3, "450 Hz · 100 %", "0.07021", "0.6319"),
            ]
        );
        for h in &rpm.harmonics {
            let d = notch_at(h.hz, 5.0, 8000.0, 25.0, 1.0);
            assert!((h.printed.delay_ms - d.delay_ms).abs() < 1e-12);
        }
        // the columns: the low-passes; four motors × the harmonics + the
        // static notch; both, each with its lag at 25 Hz
        assert_eq!(texts(&r.printed_lowpass), ("0.9517".into(), "8.565".into()));
        assert_eq!(texts(&r.printed_notch), ("1.641".into(), "14.77".into()));
        assert_eq!(texts(&r.printed_total), ("2.593".into(), "23.33".into()));
        let harmonics: f64 = rpm.harmonics.iter().map(|h| h.printed.delay_ms).sum();
        assert!((rpm.printed.delay_ms - 4.0 * harmonics).abs() < 1e-12);
        assert!((r.total.printed.delay_ms - r.printed_total.delay_ms).abs() < 1e-12);
        for d in [&r.printed_lowpass, &r.printed_notch, &r.printed_total] {
            assert!((d.lag_deg - d.delay_ms * 25.0 * 0.36).abs() < 1e-12);
        }
        // at 1 Hz: the figures of the low end of the curve
        let low = printed_at(&m, 4, 150.0, 8000.0, 1.0);
        let columns = |r: &ChainResponse| {
            [&r.printed_lowpass, &r.printed_notch, &r.printed_total].map(|d| g4(d.delay_ms))
        };
        assert_eq!(columns(&low), ["0.9549", "1.613", "2.568"]);
        assert_eq!(g4(low.printed_total.lag_deg), "0.9245");
        assert_eq!(texts(&low.parts[2].printed).0, "0.6366");
        assert_eq!(texts(&low.parts[1].printed).0, "0.06284");
        // a 4 kHz loop
        let slow = printed_at(&m, 4, 150.0, 4000.0, 25.0);
        let h: Vec<String> = slow.parts[0]
            .harmonics
            .iter()
            .map(|h| g4(h.printed.delay_ms))
            .collect();
        assert_eq!(h, ["0.2171", "0.1048", "0.06796"]);
        assert_eq!(g4(slow.parts[1].printed.delay_ms), "0.0605");
        assert_eq!(columns(&slow), ["0.9508", "1.62", "2.571"]);
        assert_eq!(columns(&printed_at(&m, 4, 150.0, 4000.0, 1.0))[2], "2.548");
        // one harmonic
        let one = window_chain(1);
        assert_eq!(
            columns(&printed_at(&one, 4, 150.0, 8000.0, 1.0))[1..],
            ["0.9107", "1.866"]
        );
        assert_eq!(
            columns(&printed_at(&one, 4, 150.0, 8000.0, 25.0))[1..],
            ["0.9347", "1.886"]
        );
    }

    #[test]
    fn the_rpm_notches_fade_with_the_motor_frequency() {
        use openpid_sim::rpm_curve_weight;
        let m = window_chain(3);
        let r = m.rpm_notch.as_ref().unwrap();
        // from 0 at the minimum of 100 Hz to full weight at 100 + 50 Hz; a
        // motor below the minimum has no RPM notches at all
        let weights = [80.0, 100.0, 110.0, 125.0, 150.0, 300.0].map(|f| rpm_curve_weight(r, f, 1));
        assert_eq!(
            weights,
            [None, Some(0.0), Some(0.2), Some(0.5), Some(1.0), Some(1.0)]
        );
        // every harmonic fades with the motor frequency, the second at
        // 220 Hz as much as the first at 110 Hz
        assert_eq!(rpm_curve_weight(r, 110.0, 2), Some(0.2));
        assert_eq!(rpm_curve_weight(r, 110.0, 3), Some(0.2));
        // times the harmonic's own weight
        let mut weighted = r.clone();
        weighted.weights = vec![1.0, 0.5, 0.0];
        assert_eq!(rpm_curve_weight(&weighted, 125.0, 2), Some(0.25));
        assert_eq!(rpm_curve_weight(&weighted, 125.0, 3), Some(0.0));
        // without a fade range they switch on at the minimum
        let mut hard = r.clone();
        hard.fade_range_hz = 0.0;
        assert_eq!(rpm_curve_weight(&hard, 100.0, 1), Some(1.0));
        assert_eq!(rpm_curve_weight(&hard, 99.0, 1), None);

        // the notch acts as (1 − w) + w·H in every curve and printed figure
        let at = |motor_hz: f64, f: f64| printed_at(&m, 4, motor_hz, 8000.0, f);
        let fade = at(110.0, 25.0);
        let rpm = &fade.parts[0];
        assert!(rpm.harmonics.iter().all(|h| (h.weight - 0.2).abs() < 1e-12));
        for h in &rpm.harmonics {
            let d = notch_at(h.hz, 5.0, 8000.0, 25.0, 0.2);
            assert!(
                (h.printed.delay_ms - d.delay_ms).abs() < 1e-12,
                "{}",
                h.harmonic
            );
        }
        let per_motor = |r: &ChainResponse| -> Vec<String> {
            r.parts[0]
                .harmonics
                .iter()
                .map(|h| g4(h.printed.delay_ms))
                .collect()
        };
        let columns =
            |r: &ChainResponse| [g4(r.printed_notch.delay_ms), g4(r.printed_total.delay_ms)];
        assert_eq!(per_motor(&fade), ["0.06087", "0.02923", "0.01929"]);
        assert_eq!(columns(&fade), ["0.5006", "1.452"]);
        let half = at(125.0, 25.0);
        assert_eq!(per_motor(&half), ["0.1324", "0.06408", "0.04232"]);
        assert_eq!(columns(&half), ["1.018", "1.97"]);
        assert_eq!(columns(&at(125.0, 1.0)), ["0.9941", "1.949"]);
        // half way the notch takes out half of the motor's tone, on each of
        // the four motors (and its other harmonics take a little more)
        let curve = chain_response(
            &m,
            &OperatingPoint {
                throttle: None,
                motor_hz: vec![125.0; 4],
                dyn_hz: Vec::new(),
            },
            &[125.0],
            &|_| at_gyro_rate(8000.0),
            25.0,
        );
        let ratio = 10f64.powf(curve.parts[0].magnitude_db[0] / 20.0);
        let expected: f64 = [125.0, 250.0, 375.0]
            .iter()
            .map(|f0| notch_h(*f0, 5.0, 8000.0, 125.0, 0.5).norm().powi(4))
            .product();
        assert!((notch_h(125.0, 5.0, 8000.0, 125.0, 0.5).norm() - 0.5).abs() < 1e-9);
        assert!((ratio - expected).abs() < 1e-9, "{ratio} {expected}");
        // below the minimum: the static notch alone
        let none = at(80.0, 1.0);
        assert!(none.parts.iter().all(|p| p.harmonics.is_empty()));
        assert_eq!(none.parts[0].label, "Notch 500 Hz");
        assert_eq!(columns(&none), ["0.06284", "1.018"]);
        assert_eq!(columns(&at(80.0, 25.0)), ["0.063", "1.015"]);
    }

    #[test]
    fn curves_at_the_loop_rate_part_from_those_at_the_log_rate_towards_its_half() {
        // a log at 2 kHz of a craft that filters at 8 kHz
        let m = typed_chain(2000.0, (250.0, 500.0), (3, 5.0), (500.0, 5.0));
        let op = OperatingPoint {
            throttle: None,
            motor_hz: vec![100.0; 4],
            dyn_hz: Vec::new(),
        };
        let freq: Vec<f64> = (1..=1000).map(f64::from).collect();
        let craft = chain_response(&m, &op, &freq, &|_| at_gyro_rate(8000.0), LOW_END_HZ);
        let log = chain_response(&m, &op, &freq, &|_| at_log_rate(2000.0), LOW_END_HZ);
        let (lpf_craft, lpf_log) = (&craft.parts[2], &log.parts[2]);
        // the low-pass at 250 Hz, at 1 000 Hz: 0.28 of magnitude at the rate
        // of the log (half of it: z⁻¹ = −1, k / (2 − k) with k = 0.4399),
        // 0.23 at the loop rate
        let ratio = |db: f64| 10f64.powf(db / 20.0);
        assert!((ratio(lpf_log.magnitude_db[999]) - 0.2820).abs() < 1e-4);
        assert!((ratio(lpf_craft.magnitude_db[999]) - 0.2284).abs() < 1e-4);
        // and at its cut-off it delays by 0.47 ms on the craft, 0.37 ms at
        // the rate of the log
        assert!(prints(lpf_craft.delay_ms[249], "0.47"));
        assert!(prints(lpf_log.delay_ms[249], "0.37"));
        // at the low end the two are one curve
        assert!((lpf_craft.magnitude_db[0] - lpf_log.magnitude_db[0]).abs() < 1e-3);
        assert!((lpf_craft.delay_ms[0] - lpf_log.delay_ms[0]).abs() < 1e-4);
        // the notch at 500 Hz delays by 0.063 ms on the craft, 0.050 ms at
        // the rate of the log
        assert!(prints(craft.parts[1].printed.delay_ms, "0.063"));
        assert!(prints(log.parts[1].printed.delay_ms, "0.050"));
        // where the two rates are one, so are the curves: only the label of
        // where the rate comes from differs
        let same = chain_response(&m, &op, &freq, &|_| at_gyro_rate(2000.0), LOW_END_HZ);
        for (a, b) in same.parts.iter().zip(&log.parts) {
            assert_eq!(a.magnitude_db, b.magnitude_db);
            assert_eq!(a.phase_deg, b.phase_deg);
            assert_eq!(a.delay_ms, b.delay_ms);
            assert_eq!(a.printed.delay_ms, b.printed.delay_ms);
        }
        assert_eq!(same.total.delay_ms, log.total.delay_ms);
        assert_eq!(same.delay_100hz_ms, log.delay_100hz_ms);
        // each part at the rate of its own loop: the second low-pass at
        // 8 kHz and the rest at 4 kHz, as Betaflight 4.2 and later
        let mixed = chain_response(
            &m,
            &op,
            &freq,
            &|stage| match stage {
                FilterStage::Lowpass2 => at_gyro_rate(8000.0),
                _ => PartRate {
                    hz: 4000.0,
                    source: RateSource::PidLoopRate,
                },
            },
            LOW_END_HZ,
        );
        let rates: Vec<f64> = mixed.parts.iter().map(|p| p.rate.unwrap().hz).collect();
        assert_eq!(rates, [4000.0, 4000.0, 4000.0, 8000.0]);
        assert_eq!(mixed.parts[3].magnitude_db, craft.parts[3].magnitude_db);
        assert_ne!(mixed.parts[2].magnitude_db, craft.parts[2].magnitude_db);
        assert_eq!(mixed.parts[3].rate.unwrap().source, RateSource::GyroRate);
    }

    #[test]
    fn the_curve_rate_is_the_loop_rate_or_falls_back_to_the_log_rate() {
        use openpid_core::settings::GyroFilterLayout;
        let settings = |gyro_hz, pid_hz, layout| {
            let mut s = Settings::default();
            s.loop_rates.gyro_hz = gyro_hz;
            s.loop_rates.pid_hz = pid_hz;
            s.loop_rates.gyro_filter_layout = layout;
            s
        };
        let rate = |hz, source| PartRate { hz, source };
        let split = GyroFilterLayout::PidLoopAfterLowpass2;
        // Betaflight 4.2 and later: an 8 kHz gyro, the PID loop at 4 kHz
        let s = settings(Some(8000.0), Some(4000.0), split);
        for stage in FilterStage::ALL {
            let expected = if stage == FilterStage::Lowpass2 {
                rate(8000.0, RateSource::GyroRate)
            } else {
                rate(4000.0, RateSource::PidLoopRate)
            };
            assert_eq!(curve_rate(&s, stage, 1000.0), expected, "{stage:?}");
        }
        // any other firmware: every stage at the gyro rate
        let s = settings(Some(2000.0), Some(1000.0), GyroFilterLayout::AllAtGyroRate);
        for stage in FilterStage::ALL {
            assert_eq!(
                curve_rate(&s, stage, 1000.0),
                rate(2000.0, RateSource::GyroRate)
            );
        }
        // no rate in the headers (a CSV), a zero or a number that is none:
        // the rate of the log, and why
        for missing in [None, Some(0.0), Some(f64::NAN), Some(-8000.0)] {
            let s = settings(missing, missing, split);
            for stage in FilterStage::ALL {
                assert_eq!(
                    curve_rate(&s, stage, 1000.0),
                    rate(1000.0, RateSource::LogRateNoLoopRate),
                    "{missing:?} {stage:?}"
                );
            }
        }
        // one loop without a rate: only its stages fall back
        let s = settings(Some(8000.0), None, split);
        assert_eq!(
            curve_rate(&s, FilterStage::Lowpass1, 1000.0),
            rate(1000.0, RateSource::LogRateNoLoopRate)
        );
        assert_eq!(
            curve_rate(&s, FilterStage::Lowpass2, 1000.0),
            rate(8000.0, RateSource::GyroRate)
        );
        // a loop cannot be slower than the log it writes
        let s = settings(Some(8000.0), Some(500.0), split);
        assert_eq!(
            curve_rate(&s, FilterStage::RpmNotch, 1000.0),
            rate(1000.0, RateSource::LogRateLoopBelowLog)
        );
        // a log written at the loop rate measures a little off the nominal
        // rate of the headers: still the loop rate
        let s = settings(Some(8000.0), Some(1000.0), split);
        assert_eq!(
            curve_rate(&s, FilterStage::RpmNotch, 1013.2),
            rate(1000.0, RateSource::PidLoopRate)
        );
        assert_eq!(
            curve_rate(&s, FilterStage::RpmNotch, 1000.0),
            rate(1000.0, RateSource::PidLoopRate)
        );
        assert!(RateSource::GyroRate.is_craft_rate() && RateSource::PidLoopRate.is_craft_rate());
        assert!(!RateSource::LogRateNoLoopRate.is_craft_rate());
        assert!(!RateSource::LogRateLoopBelowLog.is_craft_rate());
    }

    #[test]
    fn signed_lag_has_a_direction() {
        let x: Vec<f64> = (0..400)
            .map(|i| f64::from((i * 7919) % 101) - 50.0)
            .collect();
        let mut later = vec![0.0; 5];
        later.extend_from_slice(&x[..395]);
        assert!((signed_lag(&x, &later, 20) - 5.0).abs() < 0.1);
        assert!((signed_lag(&later, &x, 20) + 5.0).abs() < 0.1);
        assert_eq!(signed_lag(&x, &x, 20), 0.0);
        assert_eq!(rms(&[]), 0.0);
    }
}
