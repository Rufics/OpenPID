//! Step-response targets and verdicts (docs/design/step-verdicts): one set of
//! thresholds behind the Step tab badges, the Advice overshoot rule and the
//! CLI, with presets for the flying style.

use crate::step::StepMetrics;
use serde::{Deserialize, Serialize};

/// Thresholds a step response is judged against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StepTargets {
    /// Overshoot (percent of the final value) above which it is a warning.
    pub overshoot_warn_pct: f64,
    /// Overshoot above which it is a failure.
    pub overshoot_fail_pct: f64,
    /// Rise time (10–90 %) above this is slow, ms.
    pub rise_ms_max: f64,
    /// Settling (±5 %) later than this is slow, ms; the target band on the
    /// plot starts here.
    pub settling_ms_max: f64,
    /// `final_value` must be within 1 ± this.
    pub final_tol: f64,
    /// Step windows for a reliable verdict.
    pub min_windows_ok: usize,
    /// Step windows for a weak verdict; fewer is unreliable.
    pub min_windows_weak: usize,
}

impl Default for StepTargets {
    fn default() -> Self {
        TargetPreset::Freestyle.targets()
    }
}

impl std::hash::Hash for StepTargets {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in [
            self.overshoot_warn_pct,
            self.overshoot_fail_pct,
            self.rise_ms_max,
            self.settling_ms_max,
            self.final_tol,
        ] {
            v.to_bits().hash(state);
        }
        self.min_windows_ok.hash(state);
        self.min_windows_weak.hash(state);
    }
}

/// Ready-made targets for a flying style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetPreset {
    /// Everyday acro: the defaults.
    Freestyle,
    /// Faster rise, a little more overshoot tolerated.
    Racing,
    /// Smooth: little overshoot, quick settling.
    Cinematic,
}

impl TargetPreset {
    /// Every preset, in menu order.
    pub const ALL: [TargetPreset; 3] = [
        TargetPreset::Freestyle,
        TargetPreset::Racing,
        TargetPreset::Cinematic,
    ];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            TargetPreset::Freestyle => "Freestyle",
            TargetPreset::Racing => "Racing",
            TargetPreset::Cinematic => "Cinematic",
        }
    }

    /// The thresholds of the preset.
    pub fn targets(self) -> StepTargets {
        let freestyle = StepTargets {
            overshoot_warn_pct: 15.0,
            overshoot_fail_pct: 30.0,
            rise_ms_max: 60.0,
            settling_ms_max: 120.0,
            final_tol: 0.10,
            min_windows_ok: 10,
            min_windows_weak: 5,
        };
        match self {
            TargetPreset::Freestyle => freestyle,
            TargetPreset::Racing => StepTargets {
                rise_ms_max: 40.0,
                overshoot_warn_pct: 20.0,
                ..freestyle
            },
            TargetPreset::Cinematic => StepTargets {
                overshoot_warn_pct: 8.0,
                settling_ms_max: 100.0,
                ..freestyle
            },
        }
    }

    /// The preset these targets equal, if any.
    pub fn matching(t: &StepTargets) -> Option<TargetPreset> {
        Self::ALL.into_iter().find(|p| p.targets() == *t)
    }
}

/// How a metric compares with its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VerdictLevel {
    /// Within the target.
    Ok,
    /// Outside the target, worth a look.
    Warn,
    /// Well outside the target.
    Fail,
    /// Not measured, or too few windows to judge.
    Unknown,
}

impl VerdictLevel {
    /// ASCII marker next to the colour: `ok`, `!`, `x`, `?`.
    pub fn marker(self) -> &'static str {
        match self {
            VerdictLevel::Ok => "ok",
            VerdictLevel::Warn => "!",
            VerdictLevel::Fail => "x",
            VerdictLevel::Unknown => "?",
        }
    }
}

/// One metric judged against the targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// `windows`, `rise`, `delay`, `latency`, `overshoot`, `settling`, `final`.
    pub metric: String,
    /// Value as shown, with its unit (`35 %`, `29 ms`, `–`).
    pub value: String,
    /// How it compares with the target.
    pub level: VerdictLevel,
    /// The threshold and what the level means.
    pub note: String,
}

fn verdict(metric: &str, value: String, level: VerdictLevel, note: String) -> Verdict {
    Verdict {
        metric: metric.to_string(),
        value,
        level,
        note,
    }
}

fn ms(v: Option<f64>) -> String {
    v.map_or("–".into(), |v| format!("{v:.0} ms"))
}

/// Judge a step response: one verdict per metric, in the order of the Step
/// tab columns. With fewer than `min_windows_weak` windows every other
/// verdict is `Unknown`.
pub fn verdicts(m: &StepMetrics, windows_used: usize, t: &StepTargets) -> Vec<Verdict> {
    let (reliability, unreliable) = if windows_used >= t.min_windows_ok {
        (
            verdict(
                "windows",
                windows_used.to_string(),
                VerdictLevel::Ok,
                format!(
                    "{windows_used} step windows: at least {} needed",
                    t.min_windows_ok
                ),
            ),
            false,
        )
    } else if windows_used >= t.min_windows_weak {
        (
            verdict(
                "windows",
                windows_used.to_string(),
                VerdictLevel::Warn,
                format!(
                    "weak: {windows_used} step windows, {} wanted (fly sharper stick inputs)",
                    t.min_windows_ok
                ),
            ),
            false,
        )
    } else {
        (
            verdict(
                "windows",
                windows_used.to_string(),
                VerdictLevel::Fail,
                format!(
                    "unreliable: {windows_used} step windows, at least {} needed (the metrics are not judged)",
                    t.min_windows_weak
                ),
            ),
            true,
        )
    };
    let mut out = vec![reliability];
    let judge = |level: VerdictLevel| {
        if unreliable {
            VerdictLevel::Unknown
        } else {
            level
        }
    };
    let too_few = "too few windows to judge";

    // rise
    let (level, note) = match m.rise_time_ms {
        None => (VerdictLevel::Unknown, "rise time not measured".to_string()),
        Some(v) if v <= t.rise_ms_max => (
            VerdictLevel::Ok,
            format!("10–90 % in {v:.0} ms, target up to {:.0} ms", t.rise_ms_max),
        ),
        Some(v) => (
            VerdictLevel::Warn,
            format!(
                "slow: 10–90 % in {v:.0} ms, target up to {:.0} ms (more P or feedforward)",
                t.rise_ms_max
            ),
        ),
    };
    out.push(verdict(
        "rise",
        ms(m.rise_time_ms),
        judge(level),
        if unreliable { too_few.into() } else { note },
    ));

    // delay and latency: informational
    out.push(verdict(
        "delay",
        ms(m.delay_ms),
        judge(if m.delay_ms.is_some() {
            VerdictLevel::Ok
        } else {
            VerdictLevel::Unknown
        }),
        if unreliable {
            too_few.into()
        } else {
            "time to 50 % of the response; no target, read it next to rise".into()
        },
    ));
    out.push(verdict(
        "latency",
        m.latency_ms.map_or("–".into(), |v| format!("{v:.1} ms")),
        judge(if m.latency_ms.is_some() {
            VerdictLevel::Ok
        } else {
            VerdictLevel::Unknown
        }),
        if unreliable {
            too_few.into()
        } else {
            "setpoint-to-gyro cross-correlation lag; no target".into()
        },
    ));

    // overshoot
    let warn = t.overshoot_warn_pct.min(t.overshoot_fail_pct);
    let fail = t.overshoot_fail_pct.max(t.overshoot_warn_pct);
    let (level, note) = match m.overshoot_pct {
        None => (VerdictLevel::Unknown, "overshoot not measured".to_string()),
        Some(v) if v <= warn => (
            VerdictLevel::Ok,
            format!("{v:.0} %, within the {warn:.0} % target"),
        ),
        Some(v) if v <= fail => (
            VerdictLevel::Warn,
            format!("{v:.0} %, above the {warn:.0} % target (a little more D)"),
        ),
        Some(v) => (
            VerdictLevel::Fail,
            format!("too high: {v:.0} %, above {fail:.0} % (more D, or less P)"),
        ),
    };
    out.push(verdict(
        "overshoot",
        m.overshoot_pct.map_or("–".into(), |v| format!("{v:.0} %")),
        judge(level),
        if unreliable { too_few.into() } else { note },
    ));

    // settling
    let (level, note) = match m.settling_ms {
        None => (
            VerdictLevel::Warn,
            "never stays within ±5 % of the final value in the response window".to_string(),
        ),
        Some(v) if v <= t.settling_ms_max => (
            VerdictLevel::Ok,
            format!(
                "within ±5 % after {v:.0} ms, target up to {:.0} ms",
                t.settling_ms_max
            ),
        ),
        Some(v) => (
            VerdictLevel::Warn,
            format!(
                "slow: within ±5 % only after {v:.0} ms, target up to {:.0} ms",
                t.settling_ms_max
            ),
        ),
    };
    out.push(verdict(
        "settling",
        ms(m.settling_ms),
        judge(level),
        if unreliable { too_few.into() } else { note },
    ));

    // final value
    let off = (m.final_value - 1.0).abs();
    let (level, note) = if off <= t.final_tol {
        (
            VerdictLevel::Ok,
            format!(
                "settles at {:.2}, within 1 ± {:.2}",
                m.final_value, t.final_tol
            ),
        )
    } else {
        (
            VerdictLevel::Warn,
            format!(
                "gain mismatch: settles at {:.2}, not within 1 ± {:.2}; enable Y-offset or check the setpoint scaling",
                m.final_value, t.final_tol
            ),
        )
    };
    out.push(verdict(
        "final",
        format!("{:.2}", m.final_value),
        judge(level),
        if unreliable { too_few.into() } else { note },
    ));
    out
}

/// The worst level among the verdicts (`Unknown` counts as worst).
pub fn overall(v: &[Verdict]) -> VerdictLevel {
    v.iter()
        .map(|x| x.level)
        .max()
        .unwrap_or(VerdictLevel::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(
        overshoot: Option<f64>,
        rise: Option<f64>,
        settling: Option<f64>,
        final_value: f64,
    ) -> StepMetrics {
        StepMetrics {
            rise_time_ms: rise,
            delay_ms: Some(20.0),
            latency_ms: Some(12.0),
            overshoot_pct: overshoot,
            settling_ms: settling,
            final_value,
        }
    }

    fn level(v: &[Verdict], metric: &str) -> VerdictLevel {
        v.iter().find(|x| x.metric == metric).unwrap().level
    }

    #[test]
    fn presets_and_defaults() {
        assert_eq!(StepTargets::default(), TargetPreset::Freestyle.targets());
        assert_eq!(TargetPreset::Racing.targets().rise_ms_max, 40.0);
        assert_eq!(TargetPreset::Cinematic.targets().overshoot_warn_pct, 8.0);
        assert_eq!(
            TargetPreset::matching(&TargetPreset::Racing.targets()),
            Some(TargetPreset::Racing)
        );
        let custom = StepTargets {
            overshoot_warn_pct: 12.0,
            ..StepTargets::default()
        };
        assert_eq!(TargetPreset::matching(&custom), None);
        for p in TargetPreset::ALL {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn verdicts_follow_the_thresholds() {
        let t = StepTargets::default();
        let v = verdicts(&metrics(Some(10.0), Some(30.0), Some(90.0), 1.02), 48, &t);
        assert_eq!(v.len(), 7);
        assert_eq!(
            v.iter().map(|x| x.metric.as_str()).collect::<Vec<_>>(),
            [
                "windows",
                "rise",
                "delay",
                "latency",
                "overshoot",
                "settling",
                "final"
            ]
        );
        assert!(v.iter().all(|x| x.level == VerdictLevel::Ok), "{v:?}");
        assert_eq!(overall(&v), VerdictLevel::Ok);
        let v = verdicts(&metrics(Some(21.0), Some(70.0), Some(200.0), 1.3), 48, &t);
        assert_eq!(level(&v, "overshoot"), VerdictLevel::Warn);
        assert_eq!(level(&v, "rise"), VerdictLevel::Warn);
        assert_eq!(level(&v, "settling"), VerdictLevel::Warn);
        assert_eq!(level(&v, "final"), VerdictLevel::Warn);
        assert!(
            v.iter()
                .find(|x| x.metric == "final")
                .unwrap()
                .note
                .contains("Y-offset")
        );
        let v = verdicts(&metrics(Some(35.0), None, None, 1.0), 48, &t);
        assert_eq!(level(&v, "overshoot"), VerdictLevel::Fail);
        assert_eq!(level(&v, "rise"), VerdictLevel::Unknown);
        assert_eq!(level(&v, "settling"), VerdictLevel::Warn, "never settled");
        assert_eq!(v[1].value, "–");
        assert_eq!(overall(&v), VerdictLevel::Unknown);
        // Racing warns later on overshoot
        let v = verdicts(
            &metrics(Some(18.0), Some(30.0), Some(90.0), 1.0),
            48,
            &TargetPreset::Racing.targets(),
        );
        assert_eq!(level(&v, "overshoot"), VerdictLevel::Ok);
    }

    #[test]
    fn too_few_windows_make_everything_unknown() {
        let t = StepTargets::default();
        let v = verdicts(&metrics(Some(35.0), Some(30.0), Some(90.0), 1.0), 7, &t);
        assert_eq!(level(&v, "windows"), VerdictLevel::Warn);
        assert!(v[0].note.contains("weak"));
        assert_eq!(
            level(&v, "overshoot"),
            VerdictLevel::Fail,
            "weak still judges"
        );
        let v = verdicts(&metrics(Some(35.0), Some(30.0), Some(90.0), 1.0), 3, &t);
        assert_eq!(level(&v, "windows"), VerdictLevel::Fail);
        assert!(v[0].note.contains("unreliable"));
        for x in &v[1..] {
            assert_eq!(x.level, VerdictLevel::Unknown, "{x:?}");
            assert_eq!(x.note, "too few windows to judge");
        }
        assert_eq!(x_marker(VerdictLevel::Fail), "x");
    }

    fn x_marker(l: VerdictLevel) -> &'static str {
        l.marker()
    }
}
