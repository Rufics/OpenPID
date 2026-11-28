//! Log fitness: what a log can and cannot show, checked once when it is
//! opened (docs/design/log-fitness, R5). Every check names its evidence and,
//! when something is missing, what to change before the next flight.

use crate::step::{StepParams, count_windows};
use openpid_core::analysis::AnalysisCtx;
use openpid_core::{Axis, ChannelId, FirmwareVendor, FlightSession, GyroStage};
use serde::{Deserialize, Serialize};

/// Outcome of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    /// Fine.
    Ok,
    /// Usable with a caveat.
    Warn,
    /// Something the log cannot show, or a log not worth analysing.
    Fail,
}

impl Level {
    /// Short ASCII marker (the UI font has no symbol glyphs).
    pub fn marker(self) -> &'static str {
        match self {
            Level::Ok => "[ok]",
            Level::Warn => "[!]",
            Level::Fail => "[x]",
        }
    }
}

/// One check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// Stable identifier (`rate`, `duration`, `raw_gyro`, `erpm`, `frames`,
    /// `gaps`, `sticks`, `throttle`, `debug_mode`).
    pub id: String,
    /// Outcome.
    pub level: Level,
    /// Short name for the badge.
    pub title: String,
    /// The evidence in words.
    pub detail: String,
    /// What to change before the next flight (a Betaflight CLI line where
    /// there is one), when the check is not `Ok`.
    pub fix: Option<String>,
}

/// The checks of one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fitness {
    /// In display order.
    pub checks: Vec<Check>,
    /// The worst of `rate`, `duration` and `frames`: whether the log is worth
    /// analysing at all. The other checks only limit what it can show.
    pub overall: Level,
}

impl Fitness {
    /// A check by id.
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.id == id)
    }

    /// Level of a check by id (`Ok` when there is no such check).
    pub fn level(&self, id: &str) -> Level {
        self.check(id).map_or(Level::Ok, |c| c.level)
    }
}

// Thresholds (docs/design/log-fitness).
/// 1 kHz nominal with a few percent of jitter still counts as 1 kHz.
const RATE_OK_HZ: f64 = 950.0;
const RATE_WARN_HZ: f64 = 500.0;
const DURATION_OK_S: f64 = 20.0;
const DURATION_WARN_S: f64 = 5.0;
const CORRUPT_WARN: f64 = 0.005;
const CORRUPT_FAIL: f64 = 0.05;
const GAPS_WARN: usize = 3;
const STICKS_OK: usize = 10;
const STICKS_WARN: usize = 5;
/// Real flights sit in a band of the throttle range; the map needs a fair
/// part of it, not all of it (a 40 s freestyle log covers ~30 %).
const THROTTLE_OK: f64 = 0.3;
const THROTTLE_WARN: f64 = 0.1;
/// A segment counts for the bins within this many percent. The check is a
/// measure of its own of how much of the throttle range a log visited: it
/// does not count the columns of the freq × throttle map, whose segments
/// overlap and whose columns are two percent wide (`throttle.rs`).
const THROTTLE_HALF_WINDOW: usize = 3;
/// Segment length for the throttle coverage count.
const THROTTLE_SEGMENT_S: f64 = 0.2;
const THROTTLE_MIN_SEGMENTS: usize = 3;

fn check(id: &str, level: Level, title: &str, detail: String, fix: Option<&str>) -> Check {
    Check {
        id: id.to_string(),
        level,
        title: title.to_string(),
        detail,
        fix: (level != Level::Ok)
            .then(|| fix.map(str::to_string))
            .flatten(),
    }
}

/// Whether the firmware is Betaflight 4.3 or newer (the `blackbox_disable_*`
/// settings and `gyroUnfilt` field exist there).
fn betaflight_4_3_plus(s: &FlightSession) -> bool {
    matches!(s.firmware.vendor, FirmwareVendor::Betaflight)
        && s.firmware
            .version
            .is_some_and(|v| v.major > 4 || (v.major == 4 && v.minor >= 3) || v.major >= 2025)
}

/// Fraction of 1 % throttle bins (0..=100) with at least `THROTTLE_MIN_SEGMENTS`
/// segments of `THROTTLE_SEGMENT_S`; `None` without a throttle channel.
fn throttle_coverage(s: &FlightSession) -> Option<f64> {
    let thr = s.channel(&ChannelId::Throttle)?;
    let fs = s.timebase.sample_rate_hz();
    let seg = ((THROTTLE_SEGMENT_S * fs).round() as usize).max(1);
    let values = thr.data.values();
    let mut counts = [0usize; 101];
    let mut start = 0;
    while start + seg <= values.len() {
        let window = &values[start..start + seg];
        let valid: Vec<f64> = window
            .iter()
            .enumerate()
            .filter(|(i, _)| thr.data.is_valid(start + i))
            .map(|(_, v)| *v)
            .collect();
        if valid.len() * 10 >= seg * 9 {
            let mean = valid.iter().sum::<f64>() / valid.len() as f64;
            let bin = mean.round().clamp(0.0, 100.0) as usize;
            counts[bin] += 1;
        }
        start += seg;
    }
    let covered = (0..=100usize)
        .filter(|bin| {
            let lo = bin.saturating_sub(THROTTLE_HALF_WINDOW);
            let hi = (bin + THROTTLE_HALF_WINDOW).min(100);
            counts[lo..=hi].iter().sum::<usize>() >= THROTTLE_MIN_SEGMENTS
        })
        .count();
    Some(covered as f64 / 101.0)
}

/// Run every check on `session`.
pub fn check_session(s: &FlightSession) -> Fitness {
    let mut checks = Vec::with_capacity(9);
    // sample rate
    let rate = s.timebase.sample_rate_hz();
    let level = if rate >= RATE_OK_HZ {
        Level::Ok
    } else if rate >= RATE_WARN_HZ {
        Level::Warn
    } else {
        Level::Fail
    };
    checks.push(check(
        "rate",
        level,
        "rate",
        format!("{rate:.0} Hz logged"),
        Some("log every loop: set blackbox_sample_rate = 1/1 (Betaflight 4.3+) or blackbox_p_ratio = 1"),
    ));
    // duration
    let duration = s.timebase.duration_s();
    let level = if duration >= DURATION_OK_S {
        Level::Ok
    } else if duration >= DURATION_WARN_S {
        Level::Warn
    } else {
        Level::Fail
    };
    checks.push(check(
        "duration",
        level,
        "duration",
        format!("{duration:.1} s"),
        Some("fly and log for at least 20 s"),
    ));
    // unfiltered gyro
    let has_raw = [Axis::Roll, Axis::Pitch, Axis::Yaw].iter().any(|axis| {
        s.channel(&ChannelId::Gyro {
            axis: *axis,
            stage: GyroStage::Raw,
        })
        .is_some()
    });
    let raw_fix = if betaflight_4_3_plus(s) {
        "set blackbox_disable_gyrounfilt = OFF (logs gyroUnfilt)"
    } else if matches!(s.firmware.vendor, FirmwareVendor::Ardupilot) {
        "enable the IMU batch sampler (INS_LOG_BAT_MASK = 1, INS_LOG_BAT_OPT = 0)"
    } else {
        "set debug_mode = GYRO_SCALED (unfiltered gyro in debug[0..2])"
    };
    checks.push(check(
        "raw_gyro",
        if has_raw { Level::Ok } else { Level::Fail },
        "raw",
        if has_raw {
            "unfiltered gyro logged: Filter Sim and the noise rules work".into()
        } else {
            "no unfiltered gyro: Filter Sim and the peak rule are unavailable".into()
        },
        Some(raw_fix),
    ));
    // motor rpm
    let has_rpm = s
        .channels
        .iter()
        .any(|c| matches!(c.id, ChannelId::MotorRpm { .. }));
    checks.push(check(
        "erpm",
        if has_rpm { Level::Ok } else { Level::Warn },
        "rpm",
        if has_rpm {
            "motor rpm logged: the RPM overlay uses it".into()
        } else {
            "no motor rpm: the overlay falls back to the rpm estimate".into()
        },
        Some("set dshot_telemetry = ON and log eRPM (Betaflight 4.3+: set blackbox_disable_rpm = OFF)"),
    ));
    // corrupt frames
    let valid: u64 = s.diagnostics.frames.values().map(|f| f.valid).sum();
    let corrupt = s.diagnostics.corrupt_frames;
    let ratio = if valid + corrupt == 0 {
        0.0
    } else {
        corrupt as f64 / (valid + corrupt) as f64
    };
    let level = if ratio < CORRUPT_WARN {
        Level::Ok
    } else if ratio <= CORRUPT_FAIL {
        Level::Warn
    } else {
        Level::Fail
    };
    checks.push(check(
        "frames",
        level,
        "frames",
        format!(
            "{corrupt} corrupt of {} frames ({:.2} %)",
            valid + corrupt,
            ratio * 100.0
        ),
        Some("check the SD card or flash write speed; a slower logging rate may help"),
    ));
    // gaps
    let gaps = s.timebase.gaps.len();
    let level = if gaps == 0 {
        Level::Ok
    } else if gaps <= GAPS_WARN {
        Level::Warn
    } else {
        Level::Fail
    };
    checks.push(check(
        "gaps",
        level,
        "gaps",
        format!("{gaps} gap(s) in the timebase"),
        Some("gaps come from dropped frames: see the frames check"),
    ));
    // stick input
    let ctx = AnalysisCtx {
        session: s,
        epoch: None,
    };
    let params = StepParams::default();
    let windows: Vec<usize> = [Axis::Roll, Axis::Pitch]
        .iter()
        .map(|axis| count_windows(&ctx, *axis, &params).map_or(0, |(used, _)| used))
        .collect();
    let fewest = windows.iter().copied().min().unwrap_or(0);
    let level = if fewest >= STICKS_OK {
        Level::Ok
    } else if fewest >= STICKS_WARN {
        Level::Warn
    } else {
        Level::Fail
    };
    checks.push(check(
        "sticks",
        level,
        "sticks",
        format!(
            "{} roll / {} pitch step windows with stick input above {:.0} deg/s",
            windows[0], windows[1], params.min_input
        ),
        Some("fly sharper stick inputs (snaps, flips, rolls) so the step response has windows to average"),
    ));
    // throttle coverage
    match throttle_coverage(s) {
        Some(cov) => {
            let level = if cov >= THROTTLE_OK {
                Level::Ok
            } else if cov >= THROTTLE_WARN {
                Level::Warn
            } else {
                Level::Fail
            };
            checks.push(check(
                "throttle",
                level,
                "throttle",
                format!("{:.0} % of the throttle range has {THROTTLE_MIN_SEGMENTS}+ segments", cov * 100.0),
                Some("vary the throttle through its range (hover, punch-outs, descents) for the freq x throttle map"),
            ));
        }
        None => checks.push(check(
            "throttle",
            Level::Warn,
            "throttle",
            "no throttle channel: the freq x throttle map is unavailable".into(),
            Some("log the rc command / motor outputs so the throttle can be derived"),
        )),
    }
    // debug mode (informational)
    let debug = s
        .settings
        .debug_mode
        .as_ref()
        .map_or("unknown".to_string(), |d| {
            d.name.as_deref().map_or_else(
                || format!("code {}", d.code),
                |n| format!("{n} (code {})", d.code),
            )
        });
    checks.push(check(
        "debug_mode",
        Level::Ok,
        "debug",
        format!("debug_mode {debug}"),
        None,
    ));
    let overall = ["rate", "duration", "frames"]
        .iter()
        .filter_map(|id| checks.iter().find(|c| c.id == *id))
        .map(|c| c.level)
        .max()
        .unwrap_or(Level::Ok);
    Fitness { checks, overall }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_sim::signal::{Noise, SetpointProfile, SignalSpec};

    fn session(duration_s: f64, steps: bool) -> FlightSession {
        SignalSpec {
            duration_s,
            setpoint: if steps {
                SetpointProfile::Steps {
                    hold_min_s: 0.3,
                    hold_max_s: 1.0,
                    amplitude: 300.0,
                }
            } else {
                SetpointProfile::Silence
            },
            noise: Noise {
                white: 0.5,
                pink: 0.0,
                tones: Vec::new(),
                motor: None,
            },
            ..SignalSpec::default()
        }
        .build()
    }

    #[test]
    fn a_good_synthetic_log_passes_and_a_short_silent_one_fails() {
        let f = check_session(&session(30.0, true));
        assert_eq!(f.overall, Level::Ok, "{f:?}");
        for id in ["rate", "duration", "frames", "gaps", "raw_gyro"] {
            assert_eq!(f.level(id), Level::Ok, "{id}: {:?}", f.check(id));
        }
        // the generator drives roll only: pitch has no windows, so sticks fail
        let sticks = f.check("sticks").unwrap();
        assert_eq!(sticks.level, Level::Fail);
        assert!(sticks.detail.contains("/ 0 pitch"), "{}", sticks.detail);
        assert!(f.check("debug_mode").is_some());
        assert_eq!(f.checks.len(), 9);
        assert!(f.check("rate").unwrap().fix.is_none(), "no fix when ok");
        let g = check_session(&session(3.0, false));
        assert_eq!(g.overall, Level::Fail);
        assert_eq!(g.level("duration"), Level::Fail);
        assert_eq!(g.level("sticks"), Level::Fail);
        assert!(
            g.check("sticks")
                .unwrap()
                .fix
                .as_deref()
                .unwrap()
                .contains("stick")
        );
        assert_eq!(Level::Fail.marker(), "[x]");
        assert!(Level::Ok < Level::Warn && Level::Warn < Level::Fail);
    }

    #[test]
    fn missing_channels_are_reported_with_a_fix() {
        let mut s = session(30.0, true);
        s.channels = openpid_core::ChannelSet::new();
        let f = check_session(&s);
        assert_eq!(f.level("raw_gyro"), Level::Fail);
        assert!(
            f.check("raw_gyro")
                .unwrap()
                .fix
                .as_deref()
                .unwrap()
                .contains("GYRO_SCALED")
        );
        assert_eq!(f.level("erpm"), Level::Warn);
        assert_eq!(f.level("throttle"), Level::Warn);
        assert_eq!(
            f.overall,
            Level::Ok,
            "missing channels limit, they do not condemn the log"
        );
    }
}
