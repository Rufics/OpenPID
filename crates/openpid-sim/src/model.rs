//! A firmware's filter chain built from [`Settings`]: the static stages plus
//! the parts that move while flying (throttle-driven low-pass cutoff, RPM
//! notches following the motors, dynamic notches following spectral peaks).
//!
//! The model is an approximation (documented per stage below): it runs a
//! signal at the log's sample rate, not the gyro's, and the dynamic-notch
//! tracker is a plain STFT peak picker rather than the firmware's SDFT. Stage
//! order is the Betaflight gyro order: RPM notches → static notches →
//! low-pass 1 (dynamic) → low-pass 2 → dynamic notches; linear stages
//! commute, so the order only matters for the weighted and retuned ones.
//!
//! The frequency response of the chain does not need the log's samples, so
//! it can be asked for at the rate each stage runs at on the craft
//! ([`ChainModel::parts_at_rates`]) as well as at the rate the model runs a
//! signal at ([`ChainModel::parts_at`]).

use crate::filter::{Biquad, Chain, Filter, Pt1, Ptn, Stage};
use openpid_core::settings::{FilterChain, FilterStage, Lowpass, LowpassKind, Notch, Settings};
use openpid_dsp::{Complex, Fft, Window, next_smooth, spectrum::amplitude_spectrum};
use serde::{Deserialize, Serialize};

/// Which chain of the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainKind {
    /// Gyro filtering (what `gyro.filtered` went through).
    Gyro,
    /// D-term filtering.
    Dterm,
}

/// Throttle-driven cutoff of a low-pass (Betaflight `*_lpf1_dyn_hz`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynLowpass {
    /// Cutoff at zero throttle, Hz.
    pub min_hz: f64,
    /// Cutoff at full throttle, Hz.
    pub max_hz: f64,
    /// Curve expo (`*_lpf1_dyn_expo / 10`), 0 = linear.
    pub expo: f64,
}

impl DynLowpass {
    /// Cutoff for `throttle` in 0..=1: `min + (max − min) · curve`, with
    /// `curve = t (1 − t) expo + t` (Betaflight's expo curve; linear when
    /// expo is 0).
    pub fn cutoff_hz(&self, throttle: f64) -> f64 {
        let t = throttle.clamp(0.0, 1.0);
        let curve = t * (1.0 - t) * self.expo + t;
        (self.max_hz - self.min_hz) * curve + self.min_hz
    }
}

/// One low-pass stage of the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LowpassModel {
    /// Topology (`Other` falls back to PT1).
    pub kind: LowpassKind,
    /// Static cutoff, Hz (used when `dynamic` is `None`).
    pub cutoff_hz: f64,
    /// Throttle-driven cutoff, when the firmware moves it.
    pub dynamic: Option<DynLowpass>,
}

/// RPM notches following the motors (Betaflight 4.1+; fade and weights 4.3+).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpmNotchModel {
    /// Harmonics per motor.
    pub harmonics: u32,
    /// Notch quality.
    pub q: f64,
    /// Lowest notch centre, Hz.
    pub min_hz: f64,
    /// Width of the fade-in above `min_hz`, Hz (0 = no fade).
    pub fade_range_hz: f64,
    /// Weight per harmonic in 0..=1.
    pub weights: Vec<f64>,
    /// PT1 cutoff smoothing the motor frequency, Hz (0 = none).
    pub rpm_lpf_hz: f64,
}

/// Dynamic notches following the strongest spectral peaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynNotchModel {
    /// Notches per axis.
    pub count: u32,
    /// Notch quality.
    pub q: f64,
    /// Lowest tracked frequency, Hz.
    pub min_hz: f64,
    /// Highest tracked frequency, Hz.
    pub max_hz: f64,
}

/// A whole chain as configured, at sample rate `fs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainModel {
    /// Sample rate the model runs at (the log's).
    pub fs: f64,
    /// Which chain this is.
    pub kind: ChainKind,
    /// First low-pass.
    pub lowpass1: Option<LowpassModel>,
    /// Second low-pass.
    pub lowpass2: Option<LowpassModel>,
    /// Static notches.
    pub notches: Vec<Notch>,
    /// RPM notches.
    pub rpm_notch: Option<RpmNotchModel>,
    /// Dynamic notches.
    pub dyn_notch: Option<DynNotchModel>,
}

/// The flight data that drives the moving parts of a [`ChainModel`].
#[derive(Debug, Clone, Default)]
pub struct Drive<'a> {
    /// Throttle in 0..=1 per sample (dynamic low-pass).
    pub throttle: Option<&'a [f64]>,
    /// Motor speed in rpm per motor, per sample (RPM notches).
    pub rpm: Vec<&'a [f64]>,
    /// Dynamic-notch centres per sample (one `Vec` per sample, one entry per
    /// notch); `None` lets the model track peaks of its own input.
    pub dyn_centers: Option<&'a [Vec<f64>]>,
}

/// Which kind of filtering a part of the chain does, for the response plots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartGroup {
    /// Low-pass filters.
    Lowpass,
    /// Notch filters (static, RPM and dynamic).
    Notch,
}

/// Where the moving parts of a chain sit, for its frequency response: a
/// typical moment of a flight rather than a whole one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperatingPoint {
    /// Throttle in 0..=1 (dynamic low-pass); `None` = its minimum cutoff.
    pub throttle: Option<f64>,
    /// Motor frequency per motor, Hz (RPM notches); empty = RPM notches off,
    /// as the model runs them without motor speeds.
    pub motor_hz: Vec<f64>,
    /// Centre per dynamic notch, Hz; empty = dynamic notches left out.
    pub dyn_hz: Vec<f64>,
}

/// Which notch of the RPM filter a stage of [`ChainPart::stages`] is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RpmHarmonic {
    /// The motor, from 0.
    pub motor: usize,
    /// The harmonic, from 1 (the motor frequency itself).
    pub harmonic: u32,
    /// The notch centre, Hz.
    pub hz: f64,
}

/// One part of a chain at an operating point: a low-pass, a static notch,
/// all the RPM notches, or all the dynamic notches. Each stage carries the
/// weight it is blended with (`weight · filtered + (1 − weight) · input`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChainPart {
    /// What the part is, with its frequency ("LPF1 PT1 330 Hz").
    pub label: String,
    /// Low-pass or notch.
    pub group: PartGroup,
    /// Which stage of the chain the part is.
    pub stage: FilterStage,
    /// The stages and their weights.
    pub stages: Vec<(Stage, f64)>,
    /// For the RPM part, which motor and harmonic each stage is, in the
    /// order of [`ChainPart::stages`]; empty for every other part.
    pub harmonics: Vec<RpmHarmonic>,
}

impl ChainPart {
    /// Response of the part at `f_hz`: the product of its weighted stages.
    pub fn response(&self, f_hz: f64) -> Complex<f64> {
        self.stages
            .iter()
            .fold(Complex::new(1.0, 0.0), |h, (stage, w)| {
                h * (stage.response(f_hz) * *w + Complex::new(1.0 - *w, 0.0))
            })
    }
}

/// Throttle steps the firmware quantises to before retuning the dynamic low-pass.
const DYN_LPF_THROTTLE_STEPS: f64 = 100.0;
/// How often the dynamic low-pass cutoff is recomputed, seconds.
const DYN_LPF_UPDATE_S: f64 = 0.005;
/// Notch centres above this fraction of the sample rate are not applied.
const MAX_NOTCH_FRACTION: f64 = 0.48;
/// Smoothing of the tracked dynamic-notch centres, Hz (Betaflight `DYN_NOTCH_SMOOTH_HZ`).
const DYN_NOTCH_SMOOTH_HZ: f64 = 4.0;

fn lowpass_model(l: &Lowpass) -> Option<LowpassModel> {
    let dynamic = match (l.dyn_min_hz, l.dyn_max_hz) {
        (Some(min), Some(max)) if min > 0.0 && max > min => Some(DynLowpass {
            min_hz: min,
            max_hz: max,
            expo: l.dyn_expo.clamp(0.0, 1.0),
        }),
        _ => None,
    };
    if l.cutoff_hz <= 0.0 && dynamic.is_none() {
        return None;
    }
    Some(LowpassModel {
        kind: l.kind.clone().unwrap_or(LowpassKind::Pt1),
        cutoff_hz: l.cutoff_hz,
        dynamic,
    })
}

/// Smoothing of the motor frequency when the settings give none, Hz.
const RPM_LPF_HZ: f64 = 150.0;

impl ChainModel {
    /// The model of `kind` from the typed settings; the firmware profile has
    /// already resolved what a firmware version implies (ADR-0015).
    pub fn from_settings(settings: &Settings, kind: ChainKind, fs: f64) -> Self {
        let chain: &FilterChain = match kind {
            ChainKind::Gyro => &settings.gyro_filters,
            ChainKind::Dterm => &settings.dterm_filters,
        };
        let rpm_notch = chain.rpm_notch.as_ref().map(|r| {
            let weights = r
                .weights
                .iter()
                .map(|w| w.clamp(0.0, 1.0))
                .chain(std::iter::repeat(1.0))
                .take(r.harmonics as usize)
                .collect();
            RpmNotchModel {
                harmonics: r.harmonics,
                q: if r.q > 0.0 { r.q } else { 5.0 },
                min_hz: r.min_hz,
                fade_range_hz: r.fade_range_hz.max(0.0),
                weights,
                rpm_lpf_hz: if r.lpf_hz > 0.0 { r.lpf_hz } else { RPM_LPF_HZ },
            }
        });
        let dyn_notch = chain
            .dyn_notch
            .filter(|d| d.max_hz > d.min_hz)
            .map(|d| DynNotchModel {
                count: d.count.unwrap_or(1).max(1),
                q: if d.q > 0.0 { d.q } else { 3.0 },
                min_hz: d.min_hz,
                max_hz: d.max_hz,
            });
        ChainModel {
            fs,
            kind,
            lowpass1: chain.lowpass1.as_ref().and_then(lowpass_model),
            lowpass2: chain.lowpass2.as_ref().and_then(lowpass_model),
            notches: chain
                .notches
                .iter()
                .copied()
                .filter(|n| n.center_hz > 0.0 && n.cutoff_hz > 0.0 && n.cutoff_hz < n.center_hz)
                .collect(),
            rpm_notch,
            dyn_notch,
        }
    }

    /// One line per stage, for the UI.
    pub fn describe(&self) -> Vec<String> {
        let lp = |name: &str, l: &LowpassModel| {
            let kind = match &l.kind {
                LowpassKind::Pt1 => "PT1".to_string(),
                LowpassKind::Pt2 => "PT2".to_string(),
                LowpassKind::Pt3 => "PT3".to_string(),
                LowpassKind::Biquad => "biquad".to_string(),
                LowpassKind::Other(o) => format!("type {o} (as PT1)"),
            };
            match &l.dynamic {
                Some(d) => format!(
                    "{name}: {kind} {:.0}–{:.0} Hz by throttle (expo {:.1})",
                    d.min_hz, d.max_hz, d.expo
                ),
                None => format!("{name}: {kind} {:.0} Hz", l.cutoff_hz),
            }
        };
        let mut out = Vec::new();
        if let Some(r) = &self.rpm_notch {
            out.push(format!(
                "RPM notch: {} harmonics, Q {:.1}, min {:.0} Hz, fade {:.0} Hz, weights {:?}",
                r.harmonics, r.q, r.min_hz, r.fade_range_hz, r.weights
            ));
        }
        for n in &self.notches {
            out.push(format!(
                "notch: {:.0} Hz, cutoff {:.0} Hz",
                n.center_hz, n.cutoff_hz
            ));
        }
        if let Some(l) = &self.lowpass1 {
            out.push(lp("lowpass 1", l));
        }
        if let Some(l) = &self.lowpass2 {
            out.push(lp("lowpass 2", l));
        }
        if let Some(d) = &self.dyn_notch {
            out.push(format!(
                "dynamic notch: {} × Q {:.1} in {:.0}–{:.0} Hz",
                d.count, d.q, d.min_hz, d.max_hz
            ));
        }
        if out.is_empty() {
            out.push("no filters".into());
        }
        out
    }

    /// The static part of the chain (low-passes at their static cutoff, or
    /// the dynamic minimum, and the static notches) as a [`Chain`], for the
    /// frequency-response overlay.
    pub fn static_chain(&self) -> Chain {
        let mut chain = Chain::new(self.fs);
        for n in &self.notches {
            if n.center_hz < MAX_NOTCH_FRACTION * self.fs {
                chain.push(Stage::Biquad(Biquad::notch(
                    n.center_hz,
                    n.cutoff_hz,
                    self.fs,
                )));
            }
        }
        for l in [&self.lowpass1, &self.lowpass2].into_iter().flatten() {
            let cutoff = l.dynamic.as_ref().map_or(l.cutoff_hz, |d| d.min_hz);
            if let Some(stage) = lowpass_stage(l, cutoff, self.fs) {
                chain.push(stage);
            }
        }
        chain
    }

    /// The chain's parts at the operating point `op`, in the order the
    /// signal passes them: RPM notches (per motor and harmonic, faded and
    /// weighted by [`rpm_curve_weight`]), static notches, the low-passes
    /// (a dynamic one at the cutoff for `op.throttle`), dynamic notches.
    /// Every stage at the rate the model runs a signal at, [`ChainModel::fs`].
    pub fn parts_at(&self, op: &OperatingPoint) -> Vec<ChainPart> {
        self.parts_at_rates(op, &|_| self.fs)
    }

    /// [`ChainModel::parts_at`] with every stage at a rate of its own,
    /// `rate_hz(stage)`: the chain as it runs on the craft, whose loops are
    /// usually faster than its log. A notch at or above 0.48 of its stage's
    /// rate is left out and a low-pass cut-off is held below it, as the model
    /// does at its own rate.
    pub fn parts_at_rates(
        &self,
        op: &OperatingPoint,
        rate_hz: &dyn Fn(FilterStage) -> f64,
    ) -> Vec<ChainPart> {
        let mut parts = Vec::new();
        parts.extend(self.rpm_part(op, rate_hz(FilterStage::RpmNotch)));
        let fs = rate_hz(FilterStage::StaticNotch);
        let nyq = MAX_NOTCH_FRACTION * fs;
        for n in &self.notches {
            if n.center_hz > 0.0 && n.center_hz < nyq {
                parts.push(ChainPart {
                    label: format!("Notch {:.0} Hz", n.center_hz),
                    group: PartGroup::Notch,
                    stage: FilterStage::StaticNotch,
                    stages: vec![(
                        Stage::Biquad(Biquad::notch(n.center_hz, n.cutoff_hz, fs)),
                        1.0,
                    )],
                    harmonics: Vec::new(),
                });
            }
        }
        let lowpasses = [
            (&self.lowpass1, FilterStage::Lowpass1),
            (&self.lowpass2, FilterStage::Lowpass2),
        ];
        for (i, (l, stage)) in lowpasses.into_iter().enumerate() {
            let Some(l) = l else { continue };
            let fs = rate_hz(stage);
            let cutoff = match (&l.dynamic, op.throttle) {
                (Some(d), Some(t)) => d.cutoff_hz(t.clamp(0.0, 1.0)),
                (Some(d), None) => d.min_hz,
                (None, _) => l.cutoff_hz,
            };
            if let Some(filter) = lowpass_stage(l, cutoff, fs) {
                parts.push(ChainPart {
                    label: format!("LPF{} {} {cutoff:.0} Hz", i + 1, kind_label(&l.kind)),
                    group: PartGroup::Lowpass,
                    stage,
                    stages: vec![(filter, 1.0)],
                    harmonics: Vec::new(),
                });
            }
        }
        parts.extend(self.dyn_part(op, rate_hz(FilterStage::DynNotch)));
        parts
    }

    /// The RPM notches at the motor frequencies of `op`, one stage per motor
    /// and harmonic, at the rate `fs`, each weighted as the reference tool's
    /// web edition 1.0 draws them (decision D-015, [`rpm_curve_weight`]).
    fn rpm_part(&self, op: &OperatingPoint, fs: f64) -> Option<ChainPart> {
        let r = self.rpm_notch.as_ref()?;
        if op.motor_hz.is_empty() {
            return None;
        }
        let nyq = MAX_NOTCH_FRACTION * fs;
        let mut stages: Vec<(Stage, f64)> = Vec::new();
        let mut harmonics = Vec::new();
        for (motor, &f_motor) in op.motor_hz.iter().enumerate() {
            for harmonic in 1..=r.harmonics {
                let hz = f_motor * f64::from(harmonic);
                let Some(weight) = rpm_curve_weight(r, f_motor, harmonic) else {
                    continue;
                };
                if hz >= nyq || hz <= 0.0 || weight <= 0.0 {
                    continue;
                }
                stages.push((Stage::Biquad(Biquad::notch_q(hz, r.q, fs)), weight));
                harmonics.push(RpmHarmonic {
                    motor,
                    harmonic,
                    hz,
                });
            }
        }
        if stages.is_empty() {
            return None;
        }
        let mean = op.motor_hz.iter().sum::<f64>() / op.motor_hz.len() as f64;
        Some(ChainPart {
            label: format!(
                "RPM {} × {} motors, {mean:.0} Hz",
                r.harmonics,
                op.motor_hz.len()
            ),
            group: PartGroup::Notch,
            stage: FilterStage::RpmNotch,
            stages,
            harmonics,
        })
    }

    /// The dynamic notches at the centres of `op`, at the rate `fs`.
    fn dyn_part(&self, op: &OperatingPoint, fs: f64) -> Option<ChainPart> {
        let d = self.dyn_notch.as_ref()?;
        let nyq = MAX_NOTCH_FRACTION * fs;
        let stages: Vec<(Stage, f64)> = op
            .dyn_hz
            .iter()
            .filter(|f| **f > 0.0 && **f < nyq)
            .map(|f| (Stage::Biquad(Biquad::notch_q(*f, d.q, fs)), 1.0))
            .collect();
        if stages.is_empty() {
            return None;
        }
        let centres: Vec<String> = op.dyn_hz.iter().map(|f| format!("{f:.0}")).collect();
        Some(ChainPart {
            label: format!("Dyn notch {} Hz", centres.join(", ")),
            group: PartGroup::Notch,
            stage: FilterStage::DynNotch,
            stages,
            harmonics: Vec::new(),
        })
    }

    /// Response of [`ChainModel::static_chain`] at `f_hz`.
    pub fn static_response(&self, f_hz: f64) -> Complex<f64> {
        self.static_chain().response(f_hz)
    }

    /// Run `x` (one axis) through the chain, driven by `drive`. Missing
    /// drive data leaves the matching stage at its static setting (dynamic
    /// low-pass at its minimum, RPM notches off); without `dyn_centers` the
    /// dynamic notches track peaks of `x` itself (see [`track_peaks`]).
    pub fn run(&self, x: &[f64], drive: &Drive<'_>) -> Vec<f64> {
        let fs = self.fs;
        let nyq = MAX_NOTCH_FRACTION * fs;
        // --- RPM notches: one biquad per (motor, harmonic), retuned every sample
        let mut rpm: Option<(RpmNotchModel, Vec<Vec<Biquad>>, Vec<Pt1>)> = match &self.rpm_notch {
            Some(r) if !drive.rpm.is_empty() => Some((
                r.clone(),
                drive
                    .rpm
                    .iter()
                    .map(|_| {
                        (0..r.harmonics)
                            .map(|_| Biquad::notch_q(r.min_hz.max(1.0).min(nyq), r.q, fs))
                            .collect()
                    })
                    .collect(),
                drive
                    .rpm
                    .iter()
                    .map(|_| Pt1::new(r.rpm_lpf_hz.max(1.0).min(nyq), fs))
                    .collect(),
            )),
            _ => None,
        };
        // --- static notches
        let mut statics: Vec<Biquad> = self
            .notches
            .iter()
            .filter(|n| n.center_hz < nyq)
            .map(|n| Biquad::notch(n.center_hz, n.cutoff_hz, fs))
            .collect();
        // --- low-passes
        let mut lp1 = self.lowpass1.as_ref().and_then(|l| {
            let start = l.dynamic.as_ref().map_or(l.cutoff_hz, |d| d.min_hz);
            lowpass_stage(l, start, fs).map(|s| (l.clone(), s))
        });
        let mut lp2 = self.lowpass2.as_ref().and_then(|l| {
            let start = l.dynamic.as_ref().map_or(l.cutoff_hz, |d| d.min_hz);
            lowpass_stage(l, start, fs).map(|s| (l.clone(), s))
        });
        // --- dynamic notches
        let tracked;
        let centers: Option<&[Vec<f64>]> = match (&self.dyn_notch, drive.dyn_centers) {
            (Some(_), Some(c)) => Some(c),
            (Some(d), None) => {
                tracked = track_peaks(x, fs, d.min_hz, d.max_hz, d.count as usize);
                Some(&tracked)
            }
            (None, _) => None,
        };
        let mut dyn_notches: Vec<(f64, Biquad)> =
            self.dyn_notch.as_ref().map_or_else(Vec::new, |d| {
                (0..d.count)
                    .map(|_| {
                        let c = d.max_hz.min(nyq - 1.0).max(1.0);
                        (c, Biquad::notch_q(c, d.q, fs))
                    })
                    .collect()
            });
        let dyn_q = self.dyn_notch.as_ref().map_or(0.0, |d| d.q);

        let update_every = ((DYN_LPF_UPDATE_S * fs).round() as usize).max(1);
        let mut last_step: Option<f64> = None;
        let mut out = Vec::with_capacity(x.len());
        for (i, &xi) in x.iter().enumerate() {
            let mut v = xi;
            // RPM notches
            if let Some((model, filters, smoothers)) = &mut rpm {
                for (m, motor) in drive.rpm.iter().enumerate() {
                    let rpm_now = motor.get(i).copied().unwrap_or(0.0).max(0.0);
                    let f_motor = smoothers[m].update(rpm_now / 60.0);
                    for (harmonic, notch) in (1..).zip(filters[m].iter_mut()) {
                        let f = f_motor * f64::from(harmonic);
                        let weight = rpm_curve_weight(model, f_motor, harmonic).unwrap_or(0.0);
                        if f >= nyq || weight <= 0.0 || f <= 0.0 {
                            // the notch keeps running on its last centre but contributes nothing
                            notch.update(v);
                            continue;
                        }
                        notch.retune(&Biquad::notch_q(f, model.q, fs));
                        v = notch.update_weighted(v, weight);
                    }
                }
            }
            for n in &mut statics {
                v = n.update(v);
            }
            // dynamic low-pass cutoff, quantised and rate-limited like the firmware
            if i % update_every == 0
                && let Some(th) = drive.throttle
            {
                let t = (th.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0)
                    * DYN_LPF_THROTTLE_STEPS)
                    .round()
                    / DYN_LPF_THROTTLE_STEPS;
                if last_step != Some(t) {
                    last_step = Some(t);
                    for (model, stage) in [&mut lp1, &mut lp2].into_iter().flatten() {
                        if let Some(d) = &model.dynamic {
                            set_cutoff(stage, d.cutoff_hz(t).min(nyq), fs);
                        }
                    }
                }
            }
            if let Some((_, s)) = &mut lp1 {
                v = s.update(v);
            }
            if let Some((_, s)) = &mut lp2 {
                v = s.update(v);
            }
            // dynamic notches
            if let Some(c) = centers {
                let row = c.get(i).or_else(|| c.last());
                for (slot, (current, notch)) in dyn_notches.iter_mut().enumerate() {
                    if let Some(&target) = row.and_then(|r| r.get(slot))
                        && target.is_finite()
                        && target > 0.0
                        && target < nyq
                        && (target - *current).abs() > 0.05
                    {
                        *current = target;
                        notch.retune(&Biquad::notch_q(target, dyn_q, fs));
                    }
                    v = notch.update(v);
                }
            }
            out.push(v);
        }
        out
    }
}

/// The weight of harmonic `harmonic` (from 1) of a motor at `f_motor`, as
/// the reference tool's web edition 1.0 weighs its RPM notches (decision
/// D-015): every harmonic fades with the motor frequency,
/// `min(1, (f_motor − min_hz) / fade_range_hz)` times the harmonic's own
/// weight, and a motor below `min_hz` has no RPM notches at all (`None`).
/// Without a fade range the notches switch on at `min_hz`.
///
/// The one rule for both the response curves ([`ChainModel::parts_at`]) and
/// the simulated gyro ([`ChainModel::run`]), the owner's choice of
/// 2026-11-28. The firmware fades each harmonic by its own frequency
/// instead; the two agree whenever the motor runs at
/// `min_hz + fade_range_hz` or faster.
pub fn rpm_curve_weight(r: &RpmNotchModel, f_motor: f64, harmonic: u32) -> Option<f64> {
    if f_motor.is_nan() || f_motor < r.min_hz {
        return None;
    }
    let fade = if r.fade_range_hz > 0.0 {
        ((f_motor - r.min_hz) / r.fade_range_hz).min(1.0)
    } else {
        1.0
    };
    let own = r
        .weights
        .get(harmonic.saturating_sub(1) as usize)
        .copied()
        .unwrap_or(1.0);
    Some(fade * own)
}

fn kind_label(k: &LowpassKind) -> &'static str {
    match k {
        LowpassKind::Pt2 => "PT2",
        LowpassKind::Pt3 => "PT3",
        LowpassKind::Biquad => "biquad",
        LowpassKind::Pt1 | LowpassKind::Other(_) => "PT1",
    }
}

/// A low-pass stage of `l`'s topology at `cutoff_hz` (none for a zero cutoff).
fn lowpass_stage(l: &LowpassModel, cutoff_hz: f64, fs: f64) -> Option<Stage> {
    if cutoff_hz <= 0.0 {
        return None;
    }
    let cutoff = cutoff_hz.min(MAX_NOTCH_FRACTION * fs);
    Some(match l.kind {
        LowpassKind::Pt2 => Stage::Ptn(Ptn::new(2, cutoff, fs)),
        LowpassKind::Pt3 => Stage::Ptn(Ptn::new(3, cutoff, fs)),
        LowpassKind::Biquad => Stage::Biquad(Biquad::lowpass(cutoff, fs)),
        LowpassKind::Pt1 | LowpassKind::Other(_) => Stage::Pt1(Pt1::new(cutoff, fs)),
    })
}

fn set_cutoff(stage: &mut Stage, cutoff_hz: f64, fs: f64) {
    let cutoff = cutoff_hz.max(1.0);
    match stage {
        Stage::Pt1(f) => f.set_cutoff(cutoff),
        Stage::Ptn(f) => f.set_cutoff(cutoff),
        Stage::Biquad(f) => f.retune(&Biquad::lowpass(cutoff, fs)),
    }
}

/// Centres of the `count` strongest spectral peaks of `x` between `min_hz`
/// and `max_hz`, per sample: an STFT (Hann, ~100 ms segments, 16 ms hop)
/// peak picker whose centres are sorted by frequency, held between hops and
/// smoothed with a 4 Hz PT1, as a stand-in for the firmware's SDFT tracker.
/// A slot without a peak keeps its previous centre (initially `max_hz`).
pub fn track_peaks(x: &[f64], fs: f64, min_hz: f64, max_hz: f64, count: usize) -> Vec<Vec<f64>> {
    let count = count.max(1);
    let nperseg = next_smooth(((fs * 0.1).round() as usize).clamp(16, 4096)).min(x.len().max(16));
    let hop = ((fs * 0.016).round() as usize).clamp(1, nperseg);
    let hop_rate = fs / hop as f64;
    let mut smooth: Vec<Pt1> = (0..count)
        .map(|_| {
            let mut p = Pt1::new(DYN_NOTCH_SMOOTH_HZ.min(hop_rate / 4.0), hop_rate);
            // start settled at max_hz, like a freshly armed firmware
            for _ in 0..1000 {
                p.update(max_hz);
            }
            p
        })
        .collect();
    let mut current = vec![max_hz; count];
    let mut out: Vec<Vec<f64>> = Vec::with_capacity(x.len());
    if x.len() < nperseg {
        return vec![current; x.len()];
    }
    let fft = Fft::new();
    let df = fs / nperseg as f64;
    let mut start = 0;
    while start + nperseg <= x.len() {
        let amp = amplitude_spectrum(&fft, &x[start..start + nperseg], Window::Hann);
        let lo = ((min_hz / df).ceil() as usize).max(1);
        let hi = ((max_hz / df).floor() as usize).min(amp.len().saturating_sub(2));
        let mut peaks: Vec<(f64, f64)> = (lo.max(1)..=hi)
            .filter(|&k| k >= 1 && k + 1 < amp.len() && amp[k] >= amp[k - 1] && amp[k] > amp[k + 1])
            .map(|k| {
                // parabolic refinement of the bin
                let (a, b, c) = (amp[k - 1], amp[k], amp[k + 1]);
                let denom = a - 2.0 * b + c;
                let delta = if denom.abs() > f64::EPSILON {
                    0.5 * (a - c) / denom
                } else {
                    0.0
                };
                ((k as f64 + delta.clamp(-0.5, 0.5)) * df, b)
            })
            .collect();
        peaks.sort_by(|p, q| q.1.total_cmp(&p.1));
        peaks.truncate(count);
        peaks.sort_by(|p, q| p.0.total_cmp(&q.0));
        for (slot, (f, _)) in peaks.iter().enumerate() {
            current[slot] = *f;
        }
        let smoothed: Vec<f64> = current
            .iter()
            .zip(&mut smooth)
            .map(|(c, p)| p.update(*c).clamp(min_hz, max_hz))
            .collect();
        let until = if start + hop + nperseg <= x.len() {
            start + hop
        } else {
            x.len()
        };
        while out.len() < until {
            out.push(smoothed.clone());
        }
        start += hop;
    }
    while out.len() < x.len() {
        out.push(current.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::settings::{DynNotch, RpmNotch};

    const FS: f64 = 1000.0;

    fn lowpass(kind: LowpassKind, cutoff_hz: f64, dynamic: Option<(f64, f64)>) -> Lowpass {
        Lowpass {
            kind: Some(kind),
            cutoff_hz,
            dyn_min_hz: dynamic.map(|d| d.0),
            dyn_max_hz: dynamic.map(|d| d.1),
            dyn_expo: 0.0,
        }
    }

    /// A gyro and a D-term chain with every stage; `tweak` sets what a test is about.
    fn settings_with(tweak: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = Settings {
            gyro_filters: FilterChain {
                lowpass1: Some(lowpass(LowpassKind::Pt1, 250.0, Some((250.0, 500.0)))),
                lowpass2: Some(lowpass(LowpassKind::Pt1, 500.0, None)),
                notches: vec![Notch {
                    center_hz: 200.0,
                    cutoff_hz: 150.0,
                }],
                dyn_notch: Some(DynNotch {
                    min_hz: 100.0,
                    max_hz: 600.0,
                    q: 5.0,
                    count: Some(1),
                }),
                rpm_notch: Some(RpmNotch {
                    harmonics: 3,
                    q: 5.0,
                    min_hz: 100.0,
                    fade_range_hz: 0.0,
                    weights: Vec::new(),
                    lpf_hz: 0.0,
                }),
            },
            dterm_filters: FilterChain {
                lowpass1: Some(lowpass(LowpassKind::Pt1, 75.0, Some((75.0, 150.0)))),
                lowpass2: Some(lowpass(LowpassKind::Pt1, 150.0, None)),
                notches: Vec::new(),
                dyn_notch: None,
                rpm_notch: None,
            },
            ..Default::default()
        };
        tweak(&mut s);
        s
    }

    fn rpm(s: &mut Settings) -> &mut RpmNotch {
        s.gyro_filters.rpm_notch.as_mut().unwrap()
    }

    #[test]
    fn parts_at_an_operating_point() {
        let s = settings_with(|s| rpm(s).weights = vec![1.0, 0.5, 0.0]);
        let m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        // without motor speeds or dynamic-notch centres: notch and low-passes only
        let still = m.parts_at(&OperatingPoint::default());
        let labels: Vec<&str> = still.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Notch 200 Hz", "LPF1 PT1 250 Hz", "LPF2 PT1 500 Hz"]
        );
        assert_eq!(still[1].group, PartGroup::Lowpass);
        // half throttle moves the dynamic low-pass; the motors bring the RPM
        // notches (the third harmonic weighs 0 and is left out), and a centre
        // the dynamic notch
        let op = OperatingPoint {
            throttle: Some(0.5),
            motor_hz: vec![150.0, 160.0],
            dyn_hz: vec![320.0],
        };
        let parts = m.parts_at(&op);
        let rpm = &parts[0];
        assert_eq!(rpm.label, "RPM 3 × 2 motors, 155 Hz");
        assert_eq!(rpm.group, PartGroup::Notch);
        assert_eq!(rpm.stages.len(), 4, "2 motors × 2 weighted harmonics");
        assert!((rpm.stages[1].1 - 0.5).abs() < 1e-12);
        assert!(rpm.response(150.0).norm() < 0.05, "deep at the motor");
        // the second harmonic is weighted 0.5: it only halves the signal
        let h2 = rpm.response(300.0).norm();
        assert!(h2 > 0.2 && h2 < 0.7, "{h2}");
        let lpf1 = parts.iter().find(|p| p.label.starts_with("LPF1")).unwrap();
        let cutoff = m
            .lowpass1
            .as_ref()
            .unwrap()
            .dynamic
            .as_ref()
            .unwrap()
            .cutoff_hz(0.5);
        assert_eq!(lpf1.label, format!("LPF1 PT1 {cutoff:.0} Hz"));
        assert_eq!(parts.last().unwrap().label, "Dyn notch 320 Hz");
        assert!(parts.last().unwrap().response(320.0).norm() < 0.01);
        // a static notch outside the sample rate's reach is left out
        let mut far = m.clone();
        far.notches[0].center_hz = 900.0;
        assert!(
            far.parts_at(&OperatingPoint::default())
                .iter()
                .all(|p| !p.label.starts_with("Notch"))
        );
    }

    #[test]
    fn the_curves_fade_every_harmonic_with_the_motor() {
        // a minimum of 100 Hz and a fade of 50 Hz; one motor at 110 Hz
        let s = settings_with(|s| rpm(s).fade_range_hz = 50.0);
        let m = ChainModel::from_settings(&s, ChainKind::Gyro, 8000.0);
        let r = m.rpm_notch.as_ref().unwrap();
        let op = |motor_hz: f64| OperatingPoint {
            throttle: None,
            motor_hz: vec![motor_hz],
            dyn_hz: Vec::new(),
        };
        let rpm_of = |motor_hz: f64| {
            m.parts_at(&op(motor_hz))
                .into_iter()
                .find(|p| p.stage == FilterStage::RpmNotch)
        };
        // every harmonic is faded by the fundamental, the second and third
        // at 220 and 330 Hz as much as the first
        let part = rpm_of(110.0).unwrap();
        let weights: Vec<f64> = part.stages.iter().map(|(_, w)| *w).collect();
        assert!(
            weights.iter().all(|w| (w - 0.2).abs() < 1e-12),
            "{weights:?}"
        );
        let which: Vec<(usize, u32, f64)> = part
            .harmonics
            .iter()
            .map(|h| (h.motor, h.harmonic, h.hz))
            .collect();
        assert_eq!(which, [(0, 1, 110.0), (0, 2, 220.0), (0, 3, 330.0)]);
        // from the end of the fade up every harmonic is at full weight
        for f in [150.0, 200.0] {
            for k in 1..=3 {
                assert_eq!(rpm_curve_weight(r, f, k), Some(1.0));
            }
        }
        // below the minimum there are no RPM notches at all
        assert!(rpm_of(80.0).is_none());
        assert_eq!(rpm_curve_weight(r, 80.0, 2), None);
        assert_eq!(rpm_curve_weight(r, f64::NAN, 1), None);
        // the other parts carry no harmonics
        assert!(
            m.parts_at(&op(150.0))
                .iter()
                .filter(|p| p.stage != FilterStage::RpmNotch)
                .all(|p| p.harmonics.is_empty())
        );
    }

    #[test]
    fn the_simulated_gyro_fades_the_rpm_notches_as_the_curves_do() {
        // only the RPM notches: a minimum of 100 Hz and a fade of 50 Hz
        let s = settings_with(|s| rpm(s).fade_range_hz = 50.0);
        let mut m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        m.dyn_notch = None;
        m.notches.clear();
        m.lowpass1 = None;
        m.lowpass2 = None;
        let n = 6000;
        let gain = |motor_hz: f64, tone_hz: f64| {
            let x: Vec<f64> = (0..n)
                .map(|i| (2.0 * std::f64::consts::PI * tone_hz * i as f64 / FS).sin())
                .collect();
            let rpm = vec![motor_hz * 60.0; n];
            let y = m.run(
                &x,
                &Drive {
                    rpm: vec![&rpm],
                    ..Default::default()
                },
            );
            let tail = &y[3000..];
            (tail.iter().map(|v| v * v).sum::<f64>() / tail.len() as f64).sqrt()
                * std::f64::consts::SQRT_2
        };
        let curve = |motor_hz: f64, tone_hz: f64| {
            m.parts_at(&OperatingPoint {
                throttle: None,
                motor_hz: vec![motor_hz],
                dyn_hz: Vec::new(),
            })
            .into_iter()
            .find(|p| p.stage == FilterStage::RpmNotch)
            .map_or(1.0, |p| p.response(tone_hz).norm())
        };
        // a motor at 110 Hz: every harmonic at 20 %, so a tone on the second
        // keeps about 80 % (the firmware's own rule would notch it out)
        for tone in [110.0, 220.0, 330.0] {
            let (sim, drawn) = (gain(110.0, tone), curve(110.0, tone));
            assert!((sim - drawn).abs() < 0.01, "{tone} Hz: {sim} vs {drawn}");
            assert!(sim > 0.75 && sim < 0.85, "{tone} Hz: {sim}");
        }
        // a motor at 80 Hz, under the minimum: nothing is filtered, not even
        // the harmonics above it
        for tone in [160.0, 240.0] {
            assert_eq!(curve(80.0, tone), 1.0);
            assert!((gain(80.0, tone) - 1.0).abs() < 1e-3, "{tone} Hz");
        }
        // past the fade both notch every harmonic fully
        for tone in [200.0, 400.0] {
            let (sim, drawn) = (gain(200.0, tone), curve(200.0, tone));
            assert!(sim < 0.05 && drawn < 1e-6, "{tone} Hz: {sim} vs {drawn}");
        }
    }

    /// Phase delay of `part` at `f_hz`, ms.
    fn delay_ms(part: &ChainPart, f_hz: f64) -> f64 {
        -part.response(f_hz).arg() / (2.0 * std::f64::consts::PI * f_hz) * 1000.0
    }

    #[test]
    fn parts_at_the_rates_of_the_craft() {
        // a log at 2 kHz of a craft whose gyro runs at 8 kHz and whose PID
        // loop runs at 4 kHz
        let s = settings_with(|s| {
            s.gyro_filters.lowpass1.as_mut().unwrap().dyn_min_hz = None;
            s.gyro_filters.notches[0] = Notch {
                center_hz: 500.0,
                cutoff_hz: 452.0,
            };
        });
        let m = ChainModel::from_settings(&s, ChainKind::Gyro, 2000.0);
        let op = OperatingPoint {
            throttle: None,
            motor_hz: vec![150.0],
            dyn_hz: vec![320.0],
        };
        // every stage at the model's own rate is `parts_at`
        assert_eq!(m.parts_at_rates(&op, &|_| 2000.0), m.parts_at(&op));
        let craft = m.parts_at_rates(&op, &|stage| match stage {
            FilterStage::Lowpass2 => 8000.0,
            _ => 4000.0,
        });
        let log = m.parts_at(&op);
        let stages: Vec<FilterStage> = craft.iter().map(|p| p.stage).collect();
        assert_eq!(
            stages,
            [
                FilterStage::RpmNotch,
                FilterStage::StaticNotch,
                FilterStage::Lowpass1,
                FilterStage::Lowpass2,
                FilterStage::DynNotch
            ]
        );
        // each stage carries the rate it was asked at
        let rates: Vec<f64> = craft.iter().map(|p| p.stages[0].0.sample_rate()).collect();
        assert_eq!(rates, [4000.0, 4000.0, 4000.0, 8000.0, 4000.0]);
        // same labels: the rate changes the curve, not what the filter is
        for (a, b) in craft.iter().zip(&log) {
            assert_eq!(a.label, b.label);
        }
        // the first low-pass (PT1, 250 Hz) at 1 000 Hz, with
        // k = dt / (1 / (2π·250) + dt): at 2 kHz that is half the rate,
        // z⁻¹ = −1 and |H| = k / (2 − k) = 0.4399 / 1.5601 = 0.2820; at
        // 4 kHz it is a quarter of the rate, z⁻¹ = −j and
        // |H| = k / sqrt(1 + (1 − k)²) = 0.2820 / 1.2311 = 0.2290. The
        // curves part towards half the rate and agree at the low end
        let k = |fs: f64| (1.0 / fs) / (1.0 / (2.0 * std::f64::consts::PI * 250.0) + 1.0 / fs);
        let at_log = k(2000.0) / (2.0 - k(2000.0));
        let at_craft = k(4000.0) / (1.0 + (1.0 - k(4000.0)).powi(2)).sqrt();
        assert!((log[2].response(1000.0).norm() - at_log).abs() < 1e-12);
        assert!((at_log - 0.2820).abs() < 5e-5);
        assert!((craft[2].response(1000.0).norm() - at_craft).abs() < 1e-12);
        assert!((at_craft - 0.2290).abs() < 5e-5);
        assert!((delay_ms(&craft[2], 1.0) - delay_ms(&log[2], 1.0)).abs() < 1e-4);
        // the static notch at 500 Hz, with a Q of 500·452 / (500² − 452²) =
        // 4.946 from its cut-off: towards 0 Hz a notch delays by
        // dt · cot(w / 2) / (2·Q), w = 2π·500 / fs. At 2 kHz w is π/2 and
        // that is 0.5 ms / 9.891 = 0.0506 ms; at 4 kHz, where the craft runs
        // it, cot(π/8) = 2.414 and it is 0.25 ms · 2.414 / 9.891 = 0.0610 ms
        assert!((delay_ms(&log[1], 1.0) - 0.0506).abs() < 1e-4);
        assert!((delay_ms(&craft[1], 1.0) - 0.0610).abs() < 1e-4);
        // a notch the log's rate cannot hold is there at the craft's
        let mut far = m.clone();
        far.notches[0] = Notch {
            center_hz: 1200.0,
            cutoff_hz: 1000.0,
        };
        let has_notch = |parts: &[ChainPart]| parts.iter().any(|p| p.label == "Notch 1200 Hz");
        assert!(!has_notch(&far.parts_at(&op)));
        assert!(has_notch(&far.parts_at_rates(&op, &|_| 8000.0)));
    }

    #[test]
    fn builds_from_the_typed_settings() {
        let s = settings_with(|s| {
            s.gyro_filters.lowpass1.as_mut().unwrap().dyn_expo = 0.7;
            let r = rpm(s);
            r.fade_range_hz = 30.0;
            r.weights = vec![1.0, 0.5, 0.0];
            r.lpf_hz = 120.0;
        });
        let m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        let l1 = m.lowpass1.as_ref().unwrap();
        assert_eq!(l1.kind, LowpassKind::Pt1);
        let d = l1.dynamic.as_ref().unwrap();
        assert_eq!((d.min_hz, d.max_hz, d.expo), (250.0, 500.0, 0.7));
        assert!(m.lowpass2.as_ref().unwrap().dynamic.is_none());
        assert_eq!(m.notches.len(), 1);
        let r = m.rpm_notch.as_ref().unwrap();
        assert_eq!(r.weights, vec![1.0, 0.5, 0.0]);
        assert_eq!(
            (r.fade_range_hz, r.rpm_lpf_hz, r.harmonics),
            (30.0, 120.0, 3)
        );
        assert_eq!(m.dyn_notch.as_ref().unwrap().count, 1);
        assert_eq!(m.describe().len(), 5);
        // D-term chain: dynamic lpf1 with the dterm expo, no notches
        let s2 = settings_with(|s| s.dterm_filters.lowpass1.as_mut().unwrap().dyn_expo = 0.3);
        let dt = ChainModel::from_settings(&s2, ChainKind::Dterm, FS);
        assert_eq!(
            dt.lowpass1.as_ref().unwrap().dynamic.as_ref().unwrap().expo,
            0.3
        );
        assert!(dt.rpm_notch.is_none() && dt.dyn_notch.is_none());
        // what the settings do not give: a linear curve, no fade, every
        // harmonic at full weight, the model's smoothing of the motor speed
        let plain = ChainModel::from_settings(&settings_with(|_| {}), ChainKind::Gyro, FS);
        let l1 = plain.lowpass1.as_ref().unwrap();
        assert_eq!(l1.dynamic.as_ref().unwrap().expo, 0.0);
        let r = plain.rpm_notch.as_ref().unwrap();
        assert_eq!((r.fade_range_hz, r.rpm_lpf_hz), (0.0, RPM_LPF_HZ));
        assert_eq!(r.weights, vec![1.0; 3]);
        // an empty chain
        let none = ChainModel::from_settings(&Settings::default(), ChainKind::Gyro, FS);
        assert_eq!(none.describe(), vec!["no filters".to_string()]);
        assert!((none.static_response(100.0) - Complex::new(1.0, 0.0)).norm() < 1e-12);
    }

    #[test]
    fn dynamic_lowpass_curve_follows_betaflight() {
        let d = DynLowpass {
            min_hz: 100.0,
            max_hz: 500.0,
            expo: 0.5,
        };
        assert_eq!(d.cutoff_hz(0.0), 100.0);
        assert_eq!(d.cutoff_hz(1.0), 500.0);
        // t (1 − t) expo + t at t = 0.5: 0.125 + 0.5 = 0.625 → 350 Hz
        assert!((d.cutoff_hz(0.5) - 350.0).abs() < 1e-9);
        assert_eq!(d.cutoff_hz(2.0), 500.0, "clamped");
        let linear = DynLowpass { expo: 0.0, ..d };
        assert!((linear.cutoff_hz(0.5) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn static_run_equals_the_static_chain() {
        let s = settings_with(|_| {});
        let mut m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        m.rpm_notch = None;
        m.dyn_notch = None;
        m.lowpass1.as_mut().unwrap().dynamic = None;
        let x: Vec<f64> = (0..500).map(|i| f64::from((i * 7) % 11) - 5.0).collect();
        let by_run = m.run(&x, &Drive::default());
        let by_chain = m.static_chain().filter_signal(&x);
        for (a, b) in by_run.iter().zip(&by_chain) {
            assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        }
    }

    #[test]
    fn throttle_opens_the_dynamic_lowpass() {
        let s = settings_with(|_| {});
        let mut m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        m.rpm_notch = None;
        m.dyn_notch = None;
        m.notches.clear();
        m.lowpass2 = None;
        // a 400 Hz tone: at zero throttle the 250 Hz PT1 attenuates it, at full
        // throttle (500 Hz) much less
        let tone: Vec<f64> = (0..4000)
            .map(|i| (2.0 * std::f64::consts::PI * 400.0 * f64::from(i) / FS).sin())
            .collect();
        let rms = |v: &[f64]| (v.iter().map(|y| y * y).sum::<f64>() / v.len() as f64).sqrt();
        let low = vec![0.0; tone.len()];
        let high = vec![1.0; tone.len()];
        let y_low = m.run(
            &tone,
            &Drive {
                throttle: Some(&low),
                ..Default::default()
            },
        );
        let y_high = m.run(
            &tone,
            &Drive {
                throttle: Some(&high),
                ..Default::default()
            },
        );
        let (a, b) = (rms(&y_low[2000..]), rms(&y_high[2000..]));
        assert!(b > a * 1.3, "full throttle lets more through: {a} vs {b}");
        let expected_low =
            Pt1::new(250.0, FS).response(400.0).norm() * std::f64::consts::FRAC_1_SQRT_2;
        assert!((a - expected_low).abs() < 0.02, "{a} vs {expected_low}");
    }

    #[test]
    fn rpm_notches_remove_the_motor_tone_and_fade_near_the_minimum() {
        let s = settings_with(|s| rpm(s).fade_range_hz = 50.0);
        let mut m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        m.dyn_notch = None;
        m.notches.clear();
        m.lowpass1 = None;
        m.lowpass2 = None;
        let n = 6000;
        let rms = |v: &[f64]| (v.iter().map(|y| y * y).sum::<f64>() / v.len() as f64).sqrt();
        // motor at 12 000 rpm → 200 Hz fundamental
        let tone: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 200.0 * i as f64 / FS).sin())
            .collect();
        let rpm = vec![12_000.0; n];
        let y = m.run(
            &tone,
            &Drive {
                rpm: vec![&rpm],
                ..Default::default()
            },
        );
        assert!(
            rms(&y[3000..]) < 0.05,
            "fundamental notched: {}",
            rms(&y[3000..])
        );
        // the second harmonic (400 Hz) goes too; the third (600 Hz) is above Nyquist and skipped
        let tone2: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 400.0 * i as f64 / FS).sin())
            .collect();
        let y2 = m.run(
            &tone2,
            &Drive {
                rpm: vec![&rpm],
                ..Default::default()
            },
        );
        assert!(rms(&y2[3000..]) < 0.05);
        // a 120 Hz tone sits in the fade-in: partly attenuated
        let slow: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 120.0 * i as f64 / FS).sin())
            .collect();
        let rpm_slow = vec![7_200.0; n];
        let y3 = m.run(
            &slow,
            &Drive {
                rpm: vec![&rpm_slow],
                ..Default::default()
            },
        );
        let r3 = rms(&y3[3000..]);
        assert!(r3 > 0.3 && r3 < 0.65, "fade weight 0.4: {r3}");
        // below the minimum nothing is filtered
        let rpm_idle = vec![1_500.0; n]; // 25, 50, 75 Hz: every harmonic under the minimum
        let y4 = m.run(
            &slow,
            &Drive {
                rpm: vec![&rpm_idle],
                ..Default::default()
            },
        );
        let r4 = rms(&y4[3000..]);
        assert!(
            (r4 - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6,
            "nothing filtered below the minimum: {r4}"
        );
    }

    #[test]
    fn dynamic_notch_tracks_a_moving_tone() {
        let n = 8000;
        let mut phase = 0.0;
        let mut x = Vec::with_capacity(n);
        let mut freqs = Vec::with_capacity(n);
        for i in 0..n {
            let f = 200.0 + 150.0 * (i as f64 / n as f64); // 200 → 350 Hz sweep
            phase += 2.0 * std::f64::consts::PI * f / FS;
            x.push(phase.sin() + 0.05 * ((i * 37) % 17) as f64 / 17.0);
            freqs.push(f);
        }
        let centers = track_peaks(&x, FS, 100.0, 600.0, 1);
        assert_eq!(centers.len(), n);
        for i in (2000..n).step_by(500) {
            assert!(
                (centers[i][0] - freqs[i]).abs() < 12.0,
                "at {i}: {} vs {}",
                centers[i][0],
                freqs[i]
            );
        }
        let s = settings_with(|_| {});
        let mut m = ChainModel::from_settings(&s, ChainKind::Gyro, FS);
        m.rpm_notch = None;
        m.notches.clear();
        m.lowpass1 = None;
        m.lowpass2 = None;
        let rms = |v: &[f64]| (v.iter().map(|y| y * y).sum::<f64>() / v.len() as f64).sqrt();
        let y = m.run(&x, &Drive::default());
        assert!(
            rms(&y[2000..]) < 0.25 * rms(&x[2000..]),
            "tone suppressed: {}",
            rms(&y[2000..])
        );
        // explicit centres are used as given
        let fixed: Vec<Vec<f64>> = freqs.iter().map(|f| vec![*f]).collect();
        let y2 = m.run(
            &x,
            &Drive {
                dyn_centers: Some(&fixed),
                ..Default::default()
            },
        );
        assert!(rms(&y2[2000..]) < 0.15 * rms(&x[2000..]));
        // too short for one segment: centres stay at max
        assert_eq!(
            track_peaks(&x[..10], FS, 100.0, 600.0, 2),
            vec![vec![600.0, 600.0]; 10]
        );
    }
}
