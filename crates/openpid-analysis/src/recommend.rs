//! Rule-based tuning recommendations (phase 3, DESIGN §9, #40): every rule
//! names its evidence, and every [`Proposal`] carries the rationale and a
//! confidence. The rules are deliberately few and conservative:
//!
//! 1. **Overshoot → more D.** A step-response overshoot above the limit on an
//!    axis whose D-term is not already noisy proposes `pid.<axis>.d` up by a
//!    step; a noisy D-term gets a finding instead (filter first).
//! 2. **A peak no notch covers → a notch.** The strongest peak of the raw
//!    gyro spectrum above `peak_min_hz` that no static notch, the dynamic
//!    notch range or the RPM filter covers proposes widening the dynamic notch
//!    range or a static notch at the peak.
//! 3. **Headroom above the gyro LPF → a higher cutoff.** When the raw gyro
//!    noise between the LPF1 cutoff and twice the cutoff sits well below the
//!    low band on every axis, the static cutoff can go up (less phase delay).

use crate::psd::{PsdAnalysis, PsdOutput, PsdParams};
use crate::step::{StepAnalysis, StepParams};
use openpid_core::analysis::{Analysis, AnalysisCtx, AnalysisError, Requirement};
use openpid_core::{Axis, ChannelId, GyroStage, PidTerm, Proposal};
use serde::{Deserialize, Serialize};

/// Thresholds of the rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecommendParams {
    /// Overshoot (percent of the final value) above which rule 1 fires.
    pub overshoot_max_pct: f64,
    /// How much rule 1 raises D, percent.
    pub d_step_pct: f64,
    /// D-term noise ceiling for rule 1: PSD level at 150–400 Hz relative to
    /// 5–30 Hz, dB. Above it the D-term is noisy and D is not raised.
    pub dterm_noise_max_db: f64,
    /// Minimum prominence (dB) of a spectrum peak for rule 2.
    pub peak_min_prominence_db: f64,
    /// Peaks below this frequency (Hz) are flight dynamics, not noise (rule 2).
    pub peak_min_hz: f64,
    /// Rule 3: the band above the cutoff must be this many dB below the low band.
    pub lpf_headroom_db: f64,
    /// Rule 3: the cutoff multiplier proposed.
    pub lpf_step: f64,
    /// Step-response parameters (the axis is set per rule).
    pub step: StepParams,
    /// Spectrum parameters (the channel is set per rule).
    pub psd: PsdParams,
}

impl Default for RecommendParams {
    fn default() -> Self {
        RecommendParams {
            overshoot_max_pct: 15.0,
            d_step_pct: 10.0,
            dterm_noise_max_db: -15.0,
            peak_min_prominence_db: 8.0,
            peak_min_hz: 80.0,
            lpf_headroom_db: 10.0,
            lpf_step: 1.25,
            step: StepParams::default(),
            psd: PsdParams::default(),
        }
    }
}

impl std::hash::Hash for RecommendParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in [
            self.overshoot_max_pct,
            self.d_step_pct,
            self.dterm_noise_max_db,
            self.peak_min_prominence_db,
            self.peak_min_hz,
            self.lpf_headroom_db,
            self.lpf_step,
        ] {
            v.to_bits().hash(state);
        }
        self.step.hash(state);
        self.psd.hash(state);
    }
}

/// What a rule saw, whether or not it proposed anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Rule name.
    pub rule: String,
    /// Axis the finding is about, if any.
    pub axis: Option<Axis>,
    /// The evidence in words.
    pub text: String,
    /// Whether a proposal came out of it.
    pub proposed: bool,
}

/// Result of [`Recommend`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecommendOutput {
    /// Changes to apply, deduplicated by path (highest confidence kept).
    pub proposals: Vec<Proposal>,
    /// The evidence of every rule, fired or not.
    pub findings: Vec<Finding>,
}

#[derive(Debug, Default, Clone, Copy)]
/// The rule-based recommender.
pub struct Recommend;

/// Mean PSD level (dB) over `lo..=hi` Hz, `None` when the band is empty.
fn band_db(psd: &PsdOutput, lo: f64, hi: f64) -> Option<f64> {
    let v: Vec<f64> = psd
        .freq
        .iter()
        .zip(&psd.power_db)
        .filter(|(f, p)| **f >= lo && **f <= hi && p.is_finite())
        .map(|(_, p)| *p)
        .collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn confidence(excess: f64, scale: f64) -> f64 {
    (excess / scale).clamp(0.3, 0.9)
}

fn round_to(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

impl Recommend {
    fn push(out: &mut RecommendOutput, p: Proposal) {
        if let Some(existing) = out.proposals.iter_mut().find(|e| e.path == p.path) {
            if p.confidence > existing.confidence {
                *existing = p;
            }
        } else {
            out.proposals.push(p);
        }
    }

    fn finding(
        out: &mut RecommendOutput,
        rule: &str,
        axis: Option<Axis>,
        text: String,
        proposed: bool,
    ) {
        out.findings.push(Finding {
            rule: rule.to_string(),
            axis,
            text,
            proposed,
        });
    }

    /// Rule 1 on one axis.
    fn overshoot_rule(
        ctx: &AnalysisCtx<'_>,
        p: &RecommendParams,
        axis: Axis,
        out: &mut RecommendOutput,
    ) {
        const RULE: &str = "overshoot";
        let gyro = ChannelId::Gyro {
            axis,
            stage: GyroStage::Filtered,
        };
        if !ctx.session.channels.contains(&gyro)
            || !ctx.session.channels.contains(&ChannelId::Setpoint { axis })
        {
            return;
        }
        let step = match StepAnalysis.run(
            ctx,
            &StepParams {
                axis,
                ..p.step.clone()
            },
        ) {
            Ok(s) => s,
            Err(e) => {
                Self::finding(
                    out,
                    RULE,
                    Some(axis),
                    format!("no step response: {e}"),
                    false,
                );
                return;
            }
        };
        let Some(overshoot) = step.metrics.overshoot_pct else {
            Self::finding(out, RULE, Some(axis), "no overshoot measured".into(), false);
            return;
        };
        if step.windows_used < 5 {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!("only {} step windows: too few to judge", step.windows_used),
                false,
            );
            return;
        }
        if overshoot <= p.overshoot_max_pct {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "overshoot {overshoot:.0} % within the {:.0} % limit",
                    p.overshoot_max_pct
                ),
                false,
            );
            return;
        }
        if overshoot > 200.0 {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "overshoot {overshoot:.0} % is implausible: the deconvolution is unreliable on this epoch (little or jerky stick input)"
                ),
                false,
            );
            return;
        }
        // D-term noise: level at 150–400 Hz against 5–30 Hz
        let dterm = ChannelId::PidTerm {
            axis,
            term: PidTerm::D,
        };
        let noise_db = if ctx.session.channels.contains(&dterm) {
            PsdAnalysis
                .run(
                    ctx,
                    &PsdParams {
                        channel: dterm,
                        max_hz: None,
                        ..p.psd.clone()
                    },
                )
                .ok()
                .and_then(|psd| Some(band_db(&psd, 150.0, 400.0)? - band_db(&psd, 5.0, 30.0)?))
        } else {
            None
        };
        if let Some(n) = noise_db
            && n > p.dterm_noise_max_db
        {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "overshoot {overshoot:.0} % but the D-term is noisy ({n:+.0} dB at 150–400 Hz vs 5–30 Hz, limit {:+.0} dB): lower the D-term filter before raising D",
                    p.dterm_noise_max_db
                ),
                false,
            );
            return;
        }
        let Some(gains) = ctx.session.settings.pid.as_ref() else {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!("overshoot {overshoot:.0} % but the log has no PID gains to change"),
                false,
            );
            return;
        };
        let d = match axis {
            Axis::Roll => gains.roll.d,
            Axis::Pitch => gains.pitch.d,
            Axis::Yaw => gains.yaw.d,
        };
        if d <= 0.0 {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "overshoot {overshoot:.0} % but {} flies without D: damping comes from P/I and feedforward, not judged here",
                    axis.name()
                ),
                false,
            );
            return;
        }
        let to = (d * (1.0 + p.d_step_pct / 100.0)).round();
        let noise = noise_db.map_or(String::new(), |n| format!(", D-term noise {n:+.0} dB"));
        Self::finding(
            out,
            RULE,
            Some(axis),
            format!(
                "overshoot {overshoot:.0} % (limit {:.0} %){noise}",
                p.overshoot_max_pct
            ),
            true,
        );
        Self::push(
            out,
            Proposal {
                path: format!("pid.{}.d", axis.name()),
                from: Some(format!("{d:.0}")),
                to: format!("{to:.0}"),
                rationale: format!(
                    "{} overshoots {overshoot:.0} % (limit {:.0} %){noise}: more D damps it",
                    axis.name(),
                    p.overshoot_max_pct
                ),
                confidence: confidence(overshoot - p.overshoot_max_pct, p.overshoot_max_pct),
            },
        );
    }

    /// Rule 2 on one axis.
    fn peak_rule(
        ctx: &AnalysisCtx<'_>,
        p: &RecommendParams,
        axis: Axis,
        psd: &PsdOutput,
        out: &mut RecommendOutput,
    ) {
        const RULE: &str = "peak";
        let peaks = openpid_dsp::peaks::find_peaks(
            &psd.freq,
            &psd.power_db,
            5,
            p.peak_min_prominence_db,
            10.0,
        );
        let Some(peak) = peaks.iter().find(|pk| pk.x >= p.peak_min_hz) else {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "no peak of {:.0} dB prominence above {:.0} Hz",
                    p.peak_min_prominence_db, p.peak_min_hz
                ),
                false,
            );
            return;
        };
        let f = peak.x;
        let filters = &ctx.session.settings.gyro_filters;
        if let Some(n) = filters
            .notches
            .iter()
            .find(|n| (f - n.center_hz).abs() <= (n.center_hz - n.cutoff_hz).abs())
        {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "peak at {f:.0} Hz sits in the static notch at {:.0} Hz",
                    n.center_hz
                ),
                false,
            );
            return;
        }
        if let Some(d) = &filters.dyn_notch
            && f >= d.min_hz
            && f <= d.max_hz
        {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "peak at {f:.0} Hz is inside the dynamic notch range {:.0}–{:.0} Hz",
                    d.min_hz, d.max_hz
                ),
                false,
            );
            return;
        }
        if let Some(r) = &filters.rpm_notch
            && f >= r.min_hz
        {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "peak at {f:.0} Hz is above the RPM filter minimum ({:.0} Hz): likely a motor harmonic it tracks — see Motor noise",
                    r.min_hz
                ),
                false,
            );
            return;
        }
        let conf = confidence(peak.prominence, 20.0);
        let evidence = format!(
            "{} peak at {f:.0} Hz, {:.0} dB prominent, no notch covers it",
            axis.name(),
            peak.prominence
        );
        Self::finding(out, RULE, Some(axis), evidence.clone(), true);
        if let Some(d) = &filters.dyn_notch {
            if f < d.min_hz {
                Self::push(
                    out,
                    Proposal {
                        path: "gyro_filters.dyn_notch.min_hz".into(),
                        from: Some(format!("{:.0}", d.min_hz)),
                        to: format!("{:.0}", round_to(f * 0.8, 5.0)),
                        rationale: format!("{evidence}: extend the dynamic notch range down"),
                        confidence: conf,
                    },
                );
            } else {
                Self::push(
                    out,
                    Proposal {
                        path: "gyro_filters.dyn_notch.max_hz".into(),
                        from: Some(format!("{:.0}", d.max_hz)),
                        to: format!("{:.0}", round_to(f * 1.2, 5.0)),
                        rationale: format!("{evidence}: extend the dynamic notch range up"),
                        confidence: conf,
                    },
                );
            }
        } else {
            let slot = if filters.notches.is_empty() { 1 } else { 2 };
            Self::push(
                out,
                Proposal {
                    path: format!("gyro_filters.notch{slot}.center_hz"),
                    from: None,
                    to: format!("{:.0}", round_to(f, 5.0)),
                    rationale: format!("{evidence}: a static notch on it"),
                    confidence: conf,
                },
            );
            Self::push(
                out,
                Proposal {
                    path: format!("gyro_filters.notch{slot}.cutoff_hz"),
                    from: None,
                    to: format!("{:.0}", round_to(f * 0.8, 5.0)),
                    rationale: format!("{evidence}: notch cutoff 20 % below the centre"),
                    confidence: conf,
                },
            );
        }
    }

    /// Rule 3 across the axes: `(axis, headroom dB)` per spectrum.
    fn lpf_rule(
        ctx: &AnalysisCtx<'_>,
        p: &RecommendParams,
        spectra: &[(Axis, PsdOutput)],
        out: &mut RecommendOutput,
    ) {
        const RULE: &str = "gyro-lpf";
        let Some(lp) = &ctx.session.settings.gyro_filters.lowpass1 else {
            Self::finding(out, RULE, None, "no gyro LPF1 in the log".into(), false);
            return;
        };
        if lp.dyn_min_hz.is_some() {
            Self::finding(
                out,
                RULE,
                None,
                "the gyro LPF1 is dynamic (throttle-driven): not judged".into(),
                false,
            );
            return;
        }
        let c = lp.cutoff_hz;
        if c <= 0.0 {
            Self::finding(out, RULE, None, "the gyro LPF1 is off".into(), false);
            return;
        }
        // the loudest low band (signal) against the loudest band above the
        // cutoff (noise) across the axes: an axis without stick input has no
        // signal to compare, but its noise still counts
        let mut low_max: Option<f64> = None;
        let mut band_max: Option<(Axis, f64)> = None;
        for (axis, psd) in spectra {
            if 2.0 * c > psd.fs * 0.45 {
                Self::finding(
                    out,
                    RULE,
                    Some(*axis),
                    format!("cutoff {c:.0} Hz too close to Nyquist to judge"),
                    false,
                );
                return;
            }
            let (Some(low), Some(band)) = (band_db(psd, 10.0, c / 2.0), band_db(psd, c, 2.0 * c))
            else {
                return;
            };
            if low_max.is_none_or(|l| low > l) {
                low_max = Some(low);
            }
            if band_max.is_none_or(|(_, b)| band > b) {
                band_max = Some((*axis, band));
            }
        }
        let (Some(low), Some((axis, band))) = (low_max, band_max) else {
            return;
        };
        let headroom = low - band;
        let delay = |cut: f64| 1000.0 / (std::f64::consts::TAU * cut);
        if headroom < p.lpf_headroom_db {
            Self::finding(
                out,
                RULE,
                Some(axis),
                format!(
                    "{headroom:.0} dB between the low band (10–{:.0} Hz) and the band above the cutoff ({c:.0}–{:.0} Hz, loudest on {}; needs {:.0} dB): the LPF1 cutoff {c:.0} Hz stays",
                    c / 2.0,
                    2.0 * c,
                    axis.name(),
                    p.lpf_headroom_db
                ),
                false,
            );
            return;
        }
        let to = round_to(c * p.lpf_step, 10.0);
        let evidence = format!(
            "raw gyro noise at {c:.0}–{:.0} Hz is {headroom:.0} dB below 10–{:.0} Hz on every axis (worst {}); PT1 at {c:.0} Hz delays by {:.1} ms, at {to:.0} Hz by {:.1} ms",
            2.0 * c,
            c / 2.0,
            axis.name(),
            delay(c),
            delay(to)
        );
        Self::finding(out, RULE, Some(axis), evidence.clone(), true);
        Self::push(
            out,
            Proposal {
                path: "gyro_filters.lpf1.cutoff_hz".into(),
                from: Some(format!("{c:.0}")),
                to: format!("{to:.0}"),
                rationale: evidence,
                confidence: confidence(headroom - p.lpf_headroom_db, p.lpf_headroom_db),
            },
        );
    }
}

impl Analysis for Recommend {
    type Params = RecommendParams;
    type Output = RecommendOutput;

    fn id(&self) -> &'static str {
        "recommend"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::AnyOf(
            [Axis::Roll, Axis::Pitch, Axis::Yaw]
                .into_iter()
                .flat_map(|axis| {
                    [GyroStage::Raw, GyroStage::Filtered]
                        .map(|stage| ChannelId::Gyro { axis, stage })
                })
                .collect(),
        )]
    }

    fn run(
        &self,
        ctx: &AnalysisCtx<'_>,
        p: &RecommendParams,
    ) -> Result<RecommendOutput, AnalysisError> {
        let mut out = RecommendOutput {
            proposals: Vec::new(),
            findings: Vec::new(),
        };
        let mut spectra = Vec::new();
        for axis in [Axis::Roll, Axis::Pitch, Axis::Yaw] {
            Self::overshoot_rule(ctx, p, axis, &mut out);
            let channel = [GyroStage::Raw, GyroStage::Filtered]
                .into_iter()
                .map(|stage| ChannelId::Gyro { axis, stage })
                .find(|c| ctx.session.channels.contains(c));
            let Some(channel) = channel else { continue };
            let psd = PsdAnalysis.run(
                ctx,
                &PsdParams {
                    channel,
                    max_hz: None,
                    ..p.psd.clone()
                },
            );
            match psd {
                Ok(psd) => {
                    Self::peak_rule(ctx, p, axis, &psd, &mut out);
                    if axis != Axis::Yaw {
                        spectra.push((axis, psd));
                    }
                }
                Err(e) => Self::finding(
                    &mut out,
                    "peak",
                    Some(axis),
                    format!("no spectrum: {e}"),
                    false,
                ),
            }
        }
        if spectra.is_empty() {
            return Err(AnalysisError::NotEnoughData(
                "no gyro spectrum on roll or pitch".into(),
            ));
        }
        Self::lpf_rule(ctx, p, &spectra, &mut out);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::settings::{DynNotch, Notch, PerAxis, PidGains};
    use openpid_sim::signal::{MotorNoise, Noise, Plant, SetpointProfile, SignalSpec, Tone};

    fn base() -> SignalSpec {
        SignalSpec {
            duration_s: 20.0,
            setpoint: SetpointProfile::Steps {
                hold_min_s: 0.3,
                hold_max_s: 1.0,
                amplitude: 300.0,
            },
            noise: Noise {
                white: 0.5,
                pink: 0.0,
                tones: Vec::new(),
                motor: None,
            },
            filter_hz: 100.0,
            ..SignalSpec::default()
        }
    }

    fn gains() -> PerAxis<PidGains> {
        let g = PidGains {
            p: 45.0,
            i: 80.0,
            d: 30.0,
            f: None,
            d_min: None,
            d_max: None,
        };
        PerAxis {
            roll: g,
            pitch: g,
            yaw: g,
        }
    }

    fn run(
        spec: &SignalSpec,
        tweak: impl FnOnce(&mut openpid_core::FlightSession),
    ) -> RecommendOutput {
        let mut s = spec.build();
        tweak(&mut s);
        let ctx = AnalysisCtx {
            session: &s,
            epoch: None,
        };
        Recommend.run(&ctx, &RecommendParams::default()).unwrap()
    }

    #[test]
    fn overshoot_raises_d_when_the_dterm_is_quiet() {
        let mut spec = base();
        spec.plant = Plant {
            damping: Some(0.4),
            ..Plant::default()
        };
        let out = run(&spec, |s| s.settings.pid = Some(gains()));
        let d: Vec<&Proposal> = out
            .proposals
            .iter()
            .filter(|p| p.path.starts_with("pid."))
            .collect();
        assert!(
            d.iter().any(|p| p.path == "pid.roll.d"),
            "{:?}",
            out.findings
        );
        let roll = d.iter().find(|p| p.path == "pid.roll.d").unwrap();
        assert_eq!(roll.from.as_deref(), Some("30"));
        assert_eq!(roll.to, "33");
        assert!(roll.rationale.contains("overshoots") && roll.confidence >= 0.3);
        assert!(
            out.findings
                .iter()
                .any(|f| f.rule == "overshoot" && f.proposed)
        );
        // a first-order plant: no overshoot, no proposal
        let calm = run(&base(), |s| s.settings.pid = Some(gains()));
        assert!(
            !calm.proposals.iter().any(|p| p.path.starts_with("pid.")),
            "{:?}",
            calm.proposals
        );
        assert!(
            calm.findings
                .iter()
                .any(|f| f.rule == "overshoot" && f.text.contains("within"))
        );
        // an axis without D (yaw on most quads; here roll, the only driven axis) gets no "0 → 0"
        let mut no_d = gains();
        no_d.roll.d = 0.0;
        let out = run(&spec, |s| s.settings.pid = Some(no_d));
        assert!(!out.proposals.iter().any(|p| p.path == "pid.roll.d"));
        assert!(
            out.findings.iter().any(|f| f.text.contains("without D")),
            "{:?}",
            out.findings
        );
        // overshoot but no gains in the log: a finding only
        let no_gains = run(&spec, |_| {});
        assert!(
            !no_gains
                .proposals
                .iter()
                .any(|p| p.path.starts_with("pid."))
        );
        assert!(
            no_gains
                .findings
                .iter()
                .any(|f| f.text.contains("no PID gains"))
        );
    }

    #[test]
    fn an_uncovered_peak_gets_a_notch() {
        let mut spec = base();
        spec.noise.tones = vec![Tone {
            hz: 180.0,
            amplitude: 40.0,
        }];
        // no dynamic notch in the log: a static notch at the peak
        let out = run(&spec, |_| {});
        let centre = out
            .proposals
            .iter()
            .find(|p| p.path == "gyro_filters.notch1.center_hz")
            .expect("notch proposed");
        assert_eq!(centre.to, "180");
        let cutoff = out
            .proposals
            .iter()
            .find(|p| p.path == "gyro_filters.notch1.cutoff_hz")
            .unwrap();
        assert_eq!(cutoff.to, "145");
        // one proposal per path even though roll and pitch both see the tone
        assert_eq!(
            out.proposals
                .iter()
                .filter(|p| p.path == "gyro_filters.notch1.center_hz")
                .count(),
            1
        );
        // a dynamic notch whose range misses the peak: extend it
        let out = run(&spec, |s| {
            s.settings.gyro_filters.dyn_notch = Some(DynNotch {
                min_hz: 200.0,
                max_hz: 500.0,
                q: 300.0,
                count: Some(1),
            });
        });
        let min = out
            .proposals
            .iter()
            .find(|p| p.path == "gyro_filters.dyn_notch.min_hz")
            .expect("range extended");
        assert_eq!(min.to, "145");
        // a dynamic notch covering it: nothing
        let out = run(&spec, |s| {
            s.settings.gyro_filters.dyn_notch = Some(DynNotch {
                min_hz: 100.0,
                max_hz: 500.0,
                q: 300.0,
                count: Some(1),
            });
        });
        assert!(
            !out.proposals.iter().any(|p| p.path.contains("notch")),
            "{:?}",
            out.proposals
        );
        assert!(
            out.findings
                .iter()
                .any(|f| f.text.contains("inside the dynamic notch range"))
        );
        // a static notch covering it: nothing
        let out = run(&spec, |s| {
            s.settings.gyro_filters.notches.push(Notch {
                center_hz: 185.0,
                cutoff_hz: 160.0,
            });
        });
        assert!(!out.proposals.iter().any(|p| p.path.contains("notch")));
        // no tone: no peak
        let out = run(&base(), |_| {});
        assert!(
            out.findings
                .iter()
                .any(|f| f.rule == "peak" && f.text.starts_with("no peak"))
        );
    }

    #[test]
    fn quiet_band_above_the_lpf_raises_the_cutoff() {
        // steps at 300 deg/s against 0.5 deg/s white noise: the band above 100 Hz is far below
        let out = run(&base(), |_| {});
        let lpf = out
            .proposals
            .iter()
            .find(|p| p.path == "gyro_filters.lpf1.cutoff_hz")
            .unwrap_or_else(|| panic!("lpf raised: {:?}", out.findings));
        assert_eq!(lpf.from.as_deref(), Some("100"));
        assert_eq!(lpf.to, "130");
        assert!(lpf.rationale.contains("ms"));
        // loud broadband noise: no headroom
        let mut loud = base();
        loud.noise.white = 100.0;
        let out = run(&loud, |_| {});
        assert!(
            !out.proposals
                .iter()
                .any(|p| p.path == "gyro_filters.lpf1.cutoff_hz")
        );
        assert!(
            out.findings
                .iter()
                .any(|f| f.rule == "gyro-lpf" && f.text.contains("stays"))
        );
        // a dynamic LPF is not judged; a missing one is reported
        let out = run(&base(), |s| {
            s.settings
                .gyro_filters
                .lowpass1
                .as_mut()
                .unwrap()
                .dyn_min_hz = Some(80.0);
        });
        assert!(out.findings.iter().any(|f| f.text.contains("dynamic")));
        let out = run(&base(), |s| s.settings.gyro_filters.lowpass1 = None);
        assert!(out.findings.iter().any(|f| f.text.contains("no gyro LPF1")));
        // motor noise through an RPM filter: peaks above its minimum are its business
        let mut motors = base();
        motors.noise.motor = Some(MotorNoise::default());
        let out = run(&motors, |s| {
            s.settings.gyro_filters.rpm_notch = Some(openpid_core::settings::RpmNotch {
                harmonics: 3,
                q: 500.0,
                min_hz: 100.0,
            });
        });
        assert!(
            !out.proposals.iter().any(|p| p.path.contains("notch")),
            "{:?}",
            out.proposals
        );
    }

    #[test]
    fn params_hash_and_requirements() {
        use std::hash::{Hash, Hasher};
        let h = |p: &RecommendParams| {
            let mut s = std::collections::hash_map::DefaultHasher::new();
            p.hash(&mut s);
            s.finish()
        };
        let a = RecommendParams::default();
        let mut b = a.clone();
        b.lpf_step = 1.5;
        assert_ne!(h(&a), h(&b));
        assert_eq!(h(&a), h(&RecommendParams::default()));
        assert_eq!(Recommend.id(), "recommend");
        assert_eq!(Recommend.requires().len(), 1);
        assert_eq!(
            band_db(
                &PsdOutput {
                    channel: ChannelId::Throttle,
                    unit: String::new(),
                    fs: 1.0,
                    nperseg: 1,
                    segments: 1,
                    freq: vec![1.0, 2.0],
                    power: vec![1.0, 1.0],
                    power_db: vec![0.0, 10.0],
                },
                0.0,
                1.5
            ),
            Some(0.0)
        );
    }
}
