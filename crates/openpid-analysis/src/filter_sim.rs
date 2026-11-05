//! Filter Sim: the unfiltered gyro of one axis through the model of the
//! controller's own filter chain (`openpid-sim`), validated against the
//! filtered gyro the log carries, and through a what-if chain with the
//! user's overrides.

use crate::prepare::{Prepared, prepare};
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_core::settings::{LowpassKind, Notch};
use openpid_core::{Axis, ChannelId, GyroStage, Unit};
use openpid_dsp::spectrum::{WelchParams, welch};
use openpid_dsp::xcorr::lag_of_max;
use openpid_dsp::{Fft, Window};
use openpid_sim::{
    ChainKind, ChainModel, Drive, DynLowpass, DynNotchModel, Filter, LowpassModel, RpmNotchModel,
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
pub struct FilterSimParams {
    /// Axis to simulate.
    pub axis: Axis,
    /// What-if changes on top of the log's chain.
    pub overrides: FilterOverrides,
    /// Welch segment length for the spectra, seconds.
    pub segment_s: f64,
    /// Upper frequency of the spectra, Hz (`None` = Nyquist).
    pub max_hz: Option<f64>,
}

impl Default for FilterSimParams {
    fn default() -> Self {
        FilterSimParams {
            axis: Axis::Roll,
            overrides: FilterOverrides::default(),
            segment_s: 0.25,
            max_hz: None,
        }
    }
}

impl std::hash::Hash for FilterSimParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.axis.hash(state);
        self.overrides.hash_into(state);
        self.segment_s.to_bits().hash(state);
        self.max_hz.map(f64::to_bits).hash(state);
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Result of [`FilterSimAnalysis`].
pub struct FilterSimOutput {
    /// Simulated axis.
    pub axis: Axis,
    /// Sample rate the model ran at (the log's).
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
    /// Magnitude of the what-if chain's static part at `freq`, dB.
    pub response_db: Vec<f64>,
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
        let model = ChainModel::from_settings(
            &ctx.session.settings,
            ctx.session.firmware.version,
            ChainKind::Gyro,
            fs,
        );
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
        let mut s = openpid_core::Settings::default();
        s.gyro_filters.lowpass1 = Some(openpid_core::settings::Lowpass {
            kind: Some(LowpassKind::Pt1),
            cutoff_hz: 250.0,
            dyn_min_hz: Some(250.0),
            dyn_max_hz: Some(500.0),
        });
        s.gyro_filters.notches.push(Notch {
            center_hz: 200.0,
            cutoff_hz: 150.0,
        });
        ChainModel::from_settings(&s, None, ChainKind::Gyro, 1000.0)
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
