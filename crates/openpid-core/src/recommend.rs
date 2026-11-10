//! Proposals and exporters (DESIGN §9, phase 3): a recommender produces
//! [`Proposal`]s, an [`Exporter`] turns them into something a firmware
//! understands (Betaflight CLI `set …` lines), and [`diff_headers`] lists
//! what changed between two logs so the same exporter can write the diff.
//!
//! The exporter of a log comes from its firmware profile
//! ([`crate::BoundProfile::exporter`], reached through
//! [`crate::FormatRegistry::exporter`]); nothing else names a firmware.

use crate::session::FlightSession;
use serde::{Deserialize, Serialize};

/// A proposed change to one setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    /// Canonical path (`pid.roll.d`, `gyro_filters.lpf1.cutoff_hz`) or a raw
    /// firmware setting name (`gyro_lpf1_static_hz`).
    pub path: String,
    /// Current value as logged, when known.
    pub from: Option<String>,
    /// Proposed value.
    pub to: String,
    /// Why, in the user's terms.
    pub rationale: String,
    /// How sure the recommender is, `0..=1`.
    pub confidence: f64,
}

impl Proposal {
    /// The setting in words: "Roll D", "Gyro low-pass 1 cut-off"; the path
    /// itself for a setting without a name here.
    pub fn title(&self) -> String {
        let parts: Vec<&str> = self.path.split('.').collect();
        let axis = |a: &str| match a {
            "roll" => Some("Roll"),
            "pitch" => Some("Pitch"),
            "yaw" => Some("Yaw"),
            _ => None,
        };
        let chain = |c: &str| match c {
            "gyro_filters" => Some("Gyro"),
            "dterm_filters" => Some("D-term"),
            _ => None,
        };
        match parts.as_slice() {
            ["pid", a, term] => axis(a).map(|a| {
                let term = match *term {
                    "f" => "feedforward".to_string(),
                    "d_min" => "D min".to_string(),
                    "d_max" => "D max".to_string(),
                    t => t.to_uppercase(),
                };
                format!("{a} {term}")
            }),
            [c, "lpf1", "cutoff_hz"] => chain(c).map(|c| format!("{c} low-pass 1 cut-off")),
            [c, "lpf2", "cutoff_hz"] => chain(c).map(|c| format!("{c} low-pass 2 cut-off")),
            [c, "dyn_notch", "min_hz"] => chain(c).map(|c| format!("{c} dynamic notch minimum")),
            [c, "dyn_notch", "max_hz"] => chain(c).map(|c| format!("{c} dynamic notch maximum")),
            [c, notch, what @ ("center_hz" | "cutoff_hz")] => {
                chain(c).zip(notch.strip_prefix("notch")).map(|(c, n)| {
                    let what = if *what == "center_hz" {
                        "centre"
                    } else {
                        "cut-off"
                    };
                    format!("{c} notch {n} {what}")
                })
            }
            _ => None,
        }
        .unwrap_or_else(|| self.path.clone())
    }

    /// The unit of the value: "Hz" for a frequency, empty for a gain.
    pub fn unit(&self) -> &'static str {
        if self.path.ends_with("_hz") { "Hz" } else { "" }
    }
}

/// Why an exporter could not write a proposal.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExportError {
    /// The exporter has no setting for this path on this firmware.
    #[error("no {target} setting for `{path}`")]
    UnknownPath {
        /// Export target.
        target: &'static str,
        /// The proposal's path.
        path: String,
    },
}

/// Turns proposals and header diffs into text for the firmware of one log.
/// Created by the log's firmware profile, so it already knows the firmware
/// version and its setting names.
pub trait Exporter: Send + Sync + std::fmt::Debug {
    /// Identifier, e.g. `betaflight-cli`.
    fn id(&self) -> &'static str;

    /// What the text is, for the user: `Betaflight CLI`.
    fn label(&self) -> &'static str;

    /// The one command that applies `proposal`, without what frames a
    /// list of them (a header, a final "save").
    fn command(&self, proposal: &Proposal) -> Result<String, ExportError>;

    /// The text that applies `proposals` to the craft that wrote the log.
    fn export(&self, proposals: &[Proposal]) -> Result<String, ExportError>;

    /// The text that turns the settings of the first log of `diff` into
    /// those of the second; what cannot be set becomes a comment.
    fn diff(&self, diff: &[HeaderDiff]) -> String;
}

/// One header that differs between two sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderDiff {
    /// Header name.
    pub key: String,
    /// Value in the first session (`None` = absent).
    pub a: Option<String>,
    /// Value in the second session (`None` = absent).
    pub b: Option<String>,
}

/// Headers that differ from run to run without anything having been changed.
const VOLATILE: &[&str] = &["Log start datetime"];

/// Headers whose values differ between `a` and `b`, in the order they appear
/// in `a` (then the ones only `b` has); a header logged more than once
/// counts by its first value. Volatile headers (the log start time) are skipped.
pub fn diff_headers(a: &FlightSession, b: &FlightSession) -> Vec<HeaderDiff> {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (key, va) in &a.raw_headers {
        if seen.contains(&key.as_str()) || VOLATILE.contains(&key.as_str()) {
            continue;
        }
        seen.push(key);
        let vb = b.header(key);
        if vb != Some(va.as_str()) {
            out.push(HeaderDiff {
                key: key.clone(),
                a: Some(va.clone()),
                b: vb.map(str::to_string),
            });
        }
    }
    for (key, vb) in &b.raw_headers {
        if seen.contains(&key.as_str()) || VOLATILE.contains(&key.as_str()) {
            continue;
        }
        seen.push(key);
        out.push(HeaderDiff {
            key: key.clone(),
            a: None,
            b: Some(vb.clone()),
        });
    }
    out
}

/// Whether a header name looks like a firmware setting (`snake_case`, as in
/// the Betaflight CLI) rather than a structural header (`Field I name`,
/// `Firmware revision`).
pub fn is_setting_header(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Diagnostics, FirmwareInfo, Timebase};
    use crate::{ChannelSet, Settings};

    fn session(headers: &[(&str, &str)]) -> FlightSession {
        FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware: FirmwareInfo::unknown(),
            profile_id: "test".into(),
            settings: Settings::default(),
            raw_headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            timebase: Timebase::default(),
            channels: ChannelSet::new(),
            events: Vec::new(),
            aux: Vec::new(),
            flight_modes: Vec::new(),
            diagnostics: Diagnostics::default(),
        }
    }

    #[test]
    fn proposals_name_their_setting_in_words() {
        let p = |path: &str| Proposal {
            path: path.to_string(),
            from: None,
            to: "1".to_string(),
            rationale: String::new(),
            confidence: 0.5,
        };
        for (path, title, unit) in [
            ("pid.roll.d", "Roll D", ""),
            ("pid.pitch.f", "Pitch feedforward", ""),
            ("pid.yaw.d_min", "Yaw D min", ""),
            (
                "gyro_filters.lpf1.cutoff_hz",
                "Gyro low-pass 1 cut-off",
                "Hz",
            ),
            (
                "dterm_filters.lpf2.cutoff_hz",
                "D-term low-pass 2 cut-off",
                "Hz",
            ),
            (
                "gyro_filters.dyn_notch.min_hz",
                "Gyro dynamic notch minimum",
                "Hz",
            ),
            ("gyro_filters.notch2.center_hz", "Gyro notch 2 centre", "Hz"),
            (
                "gyro_filters.notch1.cutoff_hz",
                "Gyro notch 1 cut-off",
                "Hz",
            ),
            // a setting without a name here keeps its path
            ("gyro_lpf1_static_hz", "gyro_lpf1_static_hz", "Hz"),
            ("pid.roll", "pid.roll", ""),
        ] {
            assert_eq!(p(path).title(), title, "{path}");
            assert_eq!(p(path).unit(), unit, "{path}");
        }
    }

    #[test]
    fn diff_lists_changed_missing_and_added_headers_once() {
        let a = session(&[
            ("Firmware revision", "Betaflight 4.5.1"),
            ("Log start datetime", "2026-01-01"),
            ("gyro_lpf1_static_hz", "250"),
            ("d_max", "40,46,0"),
            ("only_a", "1"),
            ("gyro_lpf1_static_hz", "999"), // a duplicate: the first value counts
        ]);
        let b = session(&[
            ("Firmware revision", "Betaflight 4.5.1"),
            ("Log start datetime", "2026-02-02"),
            ("gyro_lpf1_static_hz", "300"),
            ("d_max", "40,46,0"),
            ("only_b", "2"),
        ]);
        let d = diff_headers(&a, &b);
        assert_eq!(
            d,
            vec![
                HeaderDiff {
                    key: "gyro_lpf1_static_hz".into(),
                    a: Some("250".into()),
                    b: Some("300".into())
                },
                HeaderDiff {
                    key: "only_a".into(),
                    a: Some("1".into()),
                    b: None
                },
                HeaderDiff {
                    key: "only_b".into(),
                    a: None,
                    b: Some("2".into())
                },
            ]
        );
        assert!(diff_headers(&a, &a).is_empty());
    }

    #[test]
    fn setting_headers_are_snake_case() {
        assert!(is_setting_header("gyro_lpf1_static_hz"));
        assert!(is_setting_header("d_max"));
        assert!(!is_setting_header("Firmware revision"));
        assert!(!is_setting_header("rollPID"));
        assert!(!is_setting_header(""));
        let e = ExportError::UnknownPath {
            target: "x",
            path: "p".into(),
        };
        assert_eq!(e.to_string(), "no x setting for `p`");
    }
}
