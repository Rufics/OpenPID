//! A firmware's filter chain built from [`Settings`]: the static stages plus
//! the parts that move while flying (throttle-driven low-pass cutoff, RPM
//! notches following the motors, dynamic notches following spectral peaks).
//!
//! The model is an approximation (documented per stage below): it runs at
//! the log's sample rate, not the gyro's, and the dynamic-notch tracker is a
//! plain STFT peak picker rather than the firmware's SDFT. Stage order is the
//! Betaflight gyro order: RPM notches → static notches → low-pass 1 (dynamic)
//! → low-pass 2 → dynamic notches; linear stages commute, so the order only
//! matters for the weighted and retuned ones.

use crate::filter::{Biquad, Chain, Filter, Pt1, Ptn, Stage};
use openpid_core::FirmwareVersion;
use openpid_core::settings::{FilterChain, Lowpass, LowpassKind, Notch, Settings};
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

/// Throttle steps the firmware quantises to before retuning the dynamic low-pass.
const DYN_LPF_THROTTLE_STEPS: f64 = 100.0;
/// How often the dynamic low-pass cutoff is recomputed, seconds.
const DYN_LPF_UPDATE_S: f64 = 0.005;
/// Notch centres above this fraction of the sample rate are not applied.
const MAX_NOTCH_FRACTION: f64 = 0.48;
/// Smoothing of the tracked dynamic-notch centres, Hz (Betaflight `DYN_NOTCH_SMOOTH_HZ`).
const DYN_NOTCH_SMOOTH_HZ: f64 = 4.0;

fn lowpass_model(l: &Lowpass, expo: f64) -> Option<LowpassModel> {
    let dynamic = match (l.dyn_min_hz, l.dyn_max_hz) {
        (Some(min), Some(max)) if min > 0.0 && max > min => Some(DynLowpass {
            min_hz: min,
            max_hz: max,
            expo,
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

fn extra_f64(settings: &Settings, key: &str) -> Option<f64> {
    settings.extra.get(key)?.trim().parse().ok()
}

fn extra_list(settings: &Settings, key: &str) -> Option<Vec<f64>> {
    settings
        .extra
        .get(key)?
        .split(',')
        .map(|v| v.trim().parse().ok())
        .collect()
}

impl ChainModel {
    /// The model of `kind` from the typed settings (and the raw headers in
    /// `Settings::extra` for what the typed chain does not carry: dynamic
    /// low-pass expo, RPM fade range, weights and smoothing). `version`
    /// fills defaults the firmware of that version uses when a header is
    /// missing.
    pub fn from_settings(
        settings: &Settings,
        version: Option<FirmwareVersion>,
        kind: ChainKind,
        fs: f64,
    ) -> Self {
        let (chain, prefix): (&FilterChain, &str) = match kind {
            ChainKind::Gyro => (&settings.gyro_filters, "gyro"),
            ChainKind::Dterm => (&settings.dterm_filters, "dterm"),
        };
        let at_least =
            |maj: u32, min: u32| version.is_some_and(|v| v >= FirmwareVersion::new(maj, min, 0));
        // the expo curve exists since 4.2 (default 5); before that the cutoff is linear
        let expo = extra_f64(settings, &format!("{prefix}_lpf1_dyn_expo"))
            .or_else(|| extra_f64(settings, &format!("{prefix}_lowpass_dyn_expo")))
            .unwrap_or(if at_least(4, 2) { 5.0 } else { 0.0 })
            / 10.0;
        let rpm_notch = chain.rpm_notch.map(|r| {
            let weights = extra_list(settings, "rpm_filter_weights")
                .unwrap_or_default()
                .into_iter()
                .map(|w| (w / 100.0).clamp(0.0, 1.0))
                .chain(std::iter::repeat(1.0))
                .take(r.harmonics as usize)
                .collect();
            RpmNotchModel {
                harmonics: r.harmonics,
                q: if r.q > 0.0 { r.q } else { 5.0 },
                min_hz: r.min_hz,
                // the fade-in arrived with 4.3; older firmware switches the notch hard at min_hz
                fade_range_hz: extra_f64(settings, "rpm_filter_fade_range_hz")
                    .unwrap_or(if at_least(4, 3) { 50.0 } else { 0.0 }),
                weights,
                rpm_lpf_hz: extra_f64(settings, "rpm_filter_lpf_hz")
                    .or_else(|| extra_f64(settings, "gyro_rpm_notch_lpf_hz"))
                    .unwrap_or(150.0),
            }
        });
        let dyn_notch = chain
            .dyn_notch
            .filter(|d| d.max_hz > d.min_hz)
            .map(|d| DynNotchModel {
                count: d.count.unwrap_or(if at_least(4, 3) { 3 } else { 1 }).max(1),
                q: if d.q > 0.0 { d.q } else { 3.0 },
                min_hz: d.min_hz,
                max_hz: d.max_hz,
            });
        ChainModel {
            fs,
            kind,
            lowpass1: chain.lowpass1.as_ref().and_then(|l| lowpass_model(l, expo)),
            lowpass2: chain.lowpass2.as_ref().and_then(|l| lowpass_model(l, expo)),
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
                    for (k, notch) in filters[m].iter_mut().enumerate() {
                        let f = f_motor * (k + 1) as f64;
                        let fade = if model.fade_range_hz > 0.0 {
                            ((f - model.min_hz) / model.fade_range_hz).clamp(0.0, 1.0)
                        } else if f >= model.min_hz {
                            1.0
                        } else {
                            0.0
                        };
                        let weight = fade * model.weights.get(k).copied().unwrap_or(1.0);
                        if f >= nyq || weight <= 0.0 || f <= 0.0 {
                            // the notch keeps running on its last centre but contributes nothing
                            notch.update(v);
                            continue;
                        }
                        notch.retune(&Biquad::notch_q(f.max(model.min_hz), model.q, fs));
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
    use std::collections::BTreeMap;

    const FS: f64 = 1000.0;

    fn lowpass(kind: LowpassKind, cutoff_hz: f64, dynamic: Option<(f64, f64)>) -> Lowpass {
        Lowpass {
            kind: Some(kind),
            cutoff_hz,
            dyn_min_hz: dynamic.map(|d| d.0),
            dyn_max_hz: dynamic.map(|d| d.1),
        }
    }

    fn settings(extra: &[(&str, &str)]) -> Settings {
        Settings {
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
                }),
            },
            dterm_filters: FilterChain {
                lowpass1: Some(lowpass(LowpassKind::Pt1, 75.0, Some((75.0, 150.0)))),
                lowpass2: Some(lowpass(LowpassKind::Pt1, 150.0, None)),
                notches: Vec::new(),
                dyn_notch: None,
                rpm_notch: None,
            },
            extra: extra
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..Default::default()
        }
    }

    #[test]
    fn builds_from_settings_with_version_defaults() {
        let v2025 = Some(FirmwareVersion::new(2025, 12, 2));
        let s = settings(&[
            ("gyro_lpf1_dyn_expo", "7"),
            ("rpm_filter_fade_range_hz", "30"),
            ("rpm_filter_weights", "100,50,0"),
            ("rpm_filter_lpf_hz", "120"),
        ]);
        let m = ChainModel::from_settings(&s, v2025, ChainKind::Gyro, FS);
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
        let s2 = settings(&[("dterm_lpf1_dyn_expo", "3")]);
        let dt = ChainModel::from_settings(&s2, v2025, ChainKind::Dterm, FS);
        assert_eq!(
            dt.lowpass1.as_ref().unwrap().dynamic.as_ref().unwrap().expo,
            0.3
        );
        assert!(dt.rpm_notch.is_none() && dt.dyn_notch.is_none());
        // 4.1 defaults: linear curve, no fade, one dynamic notch
        let old = ChainModel::from_settings(
            &settings(&[]),
            Some(FirmwareVersion::new(4, 1, 0)),
            ChainKind::Gyro,
            FS,
        );
        assert_eq!(
            old.lowpass1
                .as_ref()
                .unwrap()
                .dynamic
                .as_ref()
                .unwrap()
                .expo,
            0.0
        );
        assert_eq!(old.rpm_notch.as_ref().unwrap().fade_range_hz, 0.0);
        assert_eq!(old.rpm_notch.as_ref().unwrap().weights, vec![1.0; 3]);
        // 4.3 defaults without the headers: expo 5, fade 50
        let v43 = ChainModel::from_settings(
            &settings(&[]),
            Some(FirmwareVersion::new(4, 3, 0)),
            ChainKind::Gyro,
            FS,
        );
        assert_eq!(
            v43.lowpass1
                .as_ref()
                .unwrap()
                .dynamic
                .as_ref()
                .unwrap()
                .expo,
            0.5
        );
        assert_eq!(v43.rpm_notch.as_ref().unwrap().fade_range_hz, 50.0);
        // an empty chain
        let none = ChainModel::from_settings(&Settings::default(), None, ChainKind::Gyro, FS);
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
        let s = settings(&[]);
        let mut m = ChainModel::from_settings(&s, None, ChainKind::Gyro, FS);
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
        let s = settings(&[("gyro_lpf1_dyn_expo", "0")]);
        let mut m = ChainModel::from_settings(&s, None, ChainKind::Gyro, FS);
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
        let s = settings(&[
            ("rpm_filter_fade_range_hz", "50"),
            ("rpm_filter_weights", "100,100,100"),
        ]);
        let mut m = ChainModel::from_settings(&s, None, ChainKind::Gyro, FS);
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
        let s = settings(&[]);
        let mut m = ChainModel::from_settings(&s, None, ChainKind::Gyro, FS);
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
