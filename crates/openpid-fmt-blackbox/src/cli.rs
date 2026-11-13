//! Betaflight CLI exporter (DESIGN §9): proposals and header diffs as
//! `set name = value` lines the configurator's CLI tab accepts, ending in
//! `save`. The same `set …` format as the slider model's commands.
//!
//! Canonical paths (`pid.roll.d`, `gyro_filters.lpf1.cutoff_hz`) map to the
//! setting names of the firmware that wrote the log: Betaflight 4.3 renamed
//! the filter settings (`gyro_lowpass_hz` → `gyro_lpf1_static_hz`), which the
//! headers reveal. A path that is already a setting name passes through.

use crate::releases::Release;
use openpid_core::{ExportError, Exporter, FirmwareInfo, HeaderDiff, Proposal, RawHeaders};
use std::fmt::Write as _;

/// Identifier of the exporter.
pub const TARGET: &str = "betaflight-cli";

/// `set …` lines for the Betaflight that wrote one log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BetaflightCli {
    /// The firmware revision, for the heading of the text.
    revision: String,
    /// Whether the firmware uses the 4.3+ setting names.
    new_names: bool,
}

/// Canonical path → (setting name since 4.3, setting name before 4.3).
const PATHS: &[(&str, &str, &str)] = &[
    (
        "gyro_filters.lpf1.cutoff_hz",
        "gyro_lpf1_static_hz",
        "gyro_lowpass_hz",
    ),
    (
        "gyro_filters.lpf1.kind",
        "gyro_lpf1_type",
        "gyro_lowpass_type",
    ),
    (
        "gyro_filters.lpf1.dyn_min_hz",
        "gyro_lpf1_dyn_min_hz",
        "dyn_lpf_gyro_min_hz",
    ),
    (
        "gyro_filters.lpf1.dyn_max_hz",
        "gyro_lpf1_dyn_max_hz",
        "dyn_lpf_gyro_max_hz",
    ),
    (
        "gyro_filters.lpf2.cutoff_hz",
        "gyro_lpf2_static_hz",
        "gyro_lowpass2_hz",
    ),
    (
        "gyro_filters.lpf2.kind",
        "gyro_lpf2_type",
        "gyro_lowpass2_type",
    ),
    (
        "gyro_filters.notch1.center_hz",
        "gyro_notch1_hz",
        "gyro_notch1_hz",
    ),
    (
        "gyro_filters.notch1.cutoff_hz",
        "gyro_notch1_cutoff",
        "gyro_notch1_cutoff",
    ),
    (
        "gyro_filters.notch2.center_hz",
        "gyro_notch2_hz",
        "gyro_notch2_hz",
    ),
    (
        "gyro_filters.notch2.cutoff_hz",
        "gyro_notch2_cutoff",
        "gyro_notch2_cutoff",
    ),
    (
        "gyro_filters.dyn_notch.min_hz",
        "dyn_notch_min_hz",
        "dyn_notch_min_hz",
    ),
    (
        "gyro_filters.dyn_notch.max_hz",
        "dyn_notch_max_hz",
        "dyn_notch_max_hz",
    ),
    ("gyro_filters.dyn_notch.q", "dyn_notch_q", "dyn_notch_q"),
    (
        "gyro_filters.dyn_notch.count",
        "dyn_notch_count",
        "dyn_notch_width_percent",
    ),
    (
        "gyro_filters.rpm_notch.harmonics",
        "rpm_filter_harmonics",
        "rpm_notch_harmonics",
    ),
    ("gyro_filters.rpm_notch.q", "rpm_filter_q", "rpm_notch_q"),
    (
        "gyro_filters.rpm_notch.min_hz",
        "rpm_filter_min_hz",
        "rpm_notch_min_hz",
    ),
    (
        "dterm_filters.lpf1.cutoff_hz",
        "dterm_lpf1_static_hz",
        "dterm_lowpass_hz",
    ),
    (
        "dterm_filters.lpf1.kind",
        "dterm_lpf1_type",
        "dterm_lowpass_type",
    ),
    (
        "dterm_filters.lpf1.dyn_min_hz",
        "dterm_lpf1_dyn_min_hz",
        "dyn_lpf_dterm_min_hz",
    ),
    (
        "dterm_filters.lpf1.dyn_max_hz",
        "dterm_lpf1_dyn_max_hz",
        "dyn_lpf_dterm_max_hz",
    ),
    (
        "dterm_filters.lpf2.cutoff_hz",
        "dterm_lpf2_static_hz",
        "dterm_lowpass2_hz",
    ),
    (
        "dterm_filters.lpf2.kind",
        "dterm_lpf2_type",
        "dterm_lowpass2_type",
    ),
    (
        "dterm_filters.notch.center_hz",
        "dterm_notch_hz",
        "dterm_notch_hz",
    ),
    (
        "dterm_filters.notch.cutoff_hz",
        "dterm_notch_cutoff",
        "dterm_notch_cutoff",
    ),
];

impl BetaflightCli {
    /// The exporter for a log: Betaflight 4.3 renamed the filter settings,
    /// which the version tells, or the headers when the version is unknown.
    pub fn for_log(firmware: &FirmwareInfo, headers: &RawHeaders) -> Self {
        let new_names = if firmware.version.is_some() {
            Release::of_number(firmware.version).lpf1_names
        } else {
            headers
                .0
                .iter()
                .any(|(k, _)| k.starts_with("gyro_lpf1_") || k.starts_with("dterm_lpf1_"))
        };
        BetaflightCli {
            revision: firmware.revision.clone(),
            new_names,
        }
    }

    /// The setting name for a canonical path (or the path itself when it
    /// already is a setting name).
    pub fn setting_name(&self, path: &str) -> Option<String> {
        if let Some(rest) = path.strip_prefix("pid.") {
            let (axis, term) = rest.split_once('.')?;
            if !matches!(axis, "roll" | "pitch" | "yaw")
                || !matches!(term, "p" | "i" | "d" | "f" | "d_min" | "d_max")
            {
                return None;
            }
            return Some(format!("{term}_{axis}"));
        }
        if let Some((_, n, o)) = PATHS.iter().find(|(p, ..)| *p == path) {
            return Some((if self.new_names { *n } else { *o }).to_string());
        }
        openpid_core::is_setting_header(path).then(|| path.to_string())
    }
}

/// `set …` pairs for one header that changed: Betaflight logs some settings
/// as per-axis or min/max lists under one header, which the CLI takes one at
/// a time. `None` when the header is not a setting the CLI can set.
pub fn header_settings(key: &str, value: &str) -> Option<Vec<(String, String)>> {
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    let axes = |prefix: &str| -> Option<Vec<(String, String)>> {
        (parts.len() == 3).then(|| {
            ["roll", "pitch", "yaw"]
                .iter()
                .zip(&parts)
                .map(|(axis, v)| (format!("{prefix}_{axis}"), (*v).to_string()))
                .collect()
        })
    };
    let pair = |lo: &str, hi: &str| -> Option<Vec<(String, String)>> {
        (parts.len() == 2).then(|| {
            vec![
                (lo.to_string(), parts[0].to_string()),
                (hi.to_string(), parts[1].to_string()),
            ]
        })
    };
    let pid = |axis: &str| -> Option<Vec<(String, String)>> {
        (parts.len() == 3).then(|| {
            ["p", "i", "d"]
                .iter()
                .zip(&parts)
                .map(|(term, v)| (format!("{term}_{axis}"), (*v).to_string()))
                .collect()
        })
    };
    match key {
        "rollPID" => pid("roll"),
        "pitchPID" => pid("pitch"),
        "yawPID" => pid("yaw"),
        "d_min" | "d_max" => axes(key),
        "ff_weight" | "feedforward_weight" => axes("f"),
        "gyro_lpf1_dyn_hz" => pair("gyro_lpf1_dyn_min_hz", "gyro_lpf1_dyn_max_hz"),
        "gyro_lowpass_dyn_hz" => pair("dyn_lpf_gyro_min_hz", "dyn_lpf_gyro_max_hz"),
        "dterm_lpf1_dyn_hz" => pair("dterm_lpf1_dyn_min_hz", "dterm_lpf1_dyn_max_hz"),
        "dterm_lowpass_dyn_hz" => pair("dyn_lpf_dterm_min_hz", "dyn_lpf_dterm_max_hz"),
        "gyro_notch_hz" => pair("gyro_notch1_hz", "gyro_notch2_hz"),
        "gyro_notch_cutoff" => pair("gyro_notch1_cutoff", "gyro_notch2_cutoff"),
        _ if parts.len() == 1 && openpid_core::is_setting_header(key) => {
            Some(vec![(key.to_string(), value.to_string())])
        }
        _ => None,
    }
}

impl Exporter for BetaflightCli {
    fn id(&self) -> &'static str {
        TARGET
    }

    fn label(&self) -> &'static str {
        "Betaflight CLI"
    }

    fn command(&self, proposal: &Proposal) -> Result<String, ExportError> {
        let name = self
            .setting_name(&proposal.path)
            .ok_or_else(|| ExportError::UnknownPath {
                target: TARGET,
                path: proposal.path.clone(),
            })?;
        Ok(format!("set {name} = {}", proposal.to))
    }

    fn export(&self, proposals: &[Proposal]) -> Result<String, ExportError> {
        let mut out = format!("# OpenPID proposals for {}\n", self.revision);
        for p in proposals {
            let command = self.command(p)?;
            let from = p
                .from
                .as_deref()
                .map_or(String::new(), |f| format!("{f} → "));
            let _ = writeln!(
                out,
                "# {}: {from}{} — {} (confidence {:.0} %)",
                p.path,
                p.to,
                p.rationale,
                p.confidence * 100.0
            );
            let _ = writeln!(out, "{command}");
        }
        if !proposals.is_empty() {
            out.push_str("save\n");
        }
        Ok(out)
    }

    /// Headers the CLI cannot set become comments.
    fn diff(&self, diff: &[HeaderDiff]) -> String {
        let mut out = String::new();
        let mut any = false;
        for d in diff {
            let Some(b) = &d.b else {
                let _ = writeln!(
                    out,
                    "# {}: {} → (absent)",
                    d.key,
                    d.a.as_deref().unwrap_or("")
                );
                continue;
            };
            match header_settings(&d.key, b) {
                Some(pairs) => {
                    if let Some(a) = &d.a {
                        let _ = writeln!(out, "# {}: {a} → {b}", d.key);
                    }
                    for (k, v) in pairs {
                        let _ = writeln!(out, "set {k} = {v}");
                        any = true;
                    }
                }
                None => {
                    let _ = writeln!(
                        out,
                        "# {}: {} → {b} (not a CLI setting)",
                        d.key,
                        d.a.as_deref().unwrap_or("(absent)")
                    );
                }
            }
        }
        if any {
            out.push_str("save\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::{FirmwareVendor, FirmwareVersion};

    /// The exporter of a log with these headers and no firmware version.
    fn cli(headers: &[(&str, &str)]) -> BetaflightCli {
        let headers = RawHeaders(
            headers
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        );
        BetaflightCli::for_log(&FirmwareInfo::unknown(), &headers)
    }

    fn betaflight(major: u32, minor: u32) -> BetaflightCli {
        let fw = FirmwareInfo {
            vendor: FirmwareVendor::Betaflight,
            version: Some(FirmwareVersion::new(major, minor, 0)),
            ..FirmwareInfo::unknown()
        };
        BetaflightCli::for_log(&fw, &RawHeaders::default())
    }

    #[test]
    fn paths_map_to_the_firmware_generation() {
        let new = cli(&[("gyro_lpf1_static_hz", "250")]);
        let old = cli(&[("gyro_lowpass_hz", "200")]);
        assert_eq!(
            new.setting_name("gyro_filters.lpf1.cutoff_hz").unwrap(),
            "gyro_lpf1_static_hz"
        );
        assert_eq!(
            old.setting_name("gyro_filters.lpf1.cutoff_hz").unwrap(),
            "gyro_lowpass_hz"
        );
        assert_eq!(
            old.setting_name("dterm_filters.lpf1.dyn_min_hz").unwrap(),
            "dyn_lpf_dterm_min_hz"
        );
        assert_eq!(
            new.setting_name("gyro_filters.rpm_notch.q").unwrap(),
            "rpm_filter_q"
        );
        assert_eq!(new.setting_name("pid.pitch.d").unwrap(), "d_pitch");
        assert_eq!(new.setting_name("pid.yaw.d_max").unwrap(), "d_max_yaw");
        assert_eq!(new.setting_name("dyn_notch_q").unwrap(), "dyn_notch_q");
        assert!(new.setting_name("pid.roll.x").is_none());
        assert!(new.setting_name("pid.left.d").is_none());
        assert!(new.setting_name("Firmware revision").is_none());
        assert!(new.setting_name("motor.poles").is_none());
        // a known version decides, whatever the headers say
        assert_eq!(
            betaflight(4, 2)
                .setting_name("gyro_filters.lpf1.cutoff_hz")
                .unwrap(),
            "gyro_lowpass_hz"
        );
        assert_eq!(
            betaflight(2025, 12)
                .setting_name("gyro_filters.lpf1.cutoff_hz")
                .unwrap(),
            "gyro_lpf1_static_hz"
        );
    }

    #[test]
    fn header_lists_expand_to_single_settings() {
        assert_eq!(
            header_settings("rollPID", "45,80,30").unwrap(),
            vec![
                ("p_roll".to_string(), "45".to_string()),
                ("i_roll".to_string(), "80".to_string()),
                ("d_roll".to_string(), "30".to_string())
            ]
        );
        assert_eq!(
            header_settings("d_max", "40,46,0").unwrap()[2],
            ("d_max_yaw".to_string(), "0".to_string())
        );
        assert_eq!(
            header_settings("ff_weight", "120,125,120").unwrap()[0].0,
            "f_roll"
        );
        assert_eq!(
            header_settings("gyro_lpf1_dyn_hz", "250,500").unwrap(),
            vec![
                ("gyro_lpf1_dyn_min_hz".to_string(), "250".to_string()),
                ("gyro_lpf1_dyn_max_hz".to_string(), "500".to_string())
            ]
        );
        assert_eq!(
            header_settings("gyro_notch_cutoff", "0,0").unwrap()[1].0,
            "gyro_notch2_cutoff"
        );
        assert_eq!(
            header_settings("dyn_notch_q", "250").unwrap(),
            vec![("dyn_notch_q".to_string(), "250".to_string())]
        );
        assert!(header_settings("rollPID", "45,80").is_none());
        assert!(header_settings("Firmware revision", "x").is_none());
        assert!(header_settings("rc_rates", "7,7,7").is_none());
    }

    #[test]
    fn diff_commands_set_the_second_log_values() {
        let diff = vec![
            HeaderDiff {
                key: "Firmware revision".into(),
                a: Some("Betaflight 4.5.1".into()),
                b: Some("Betaflight 4.5.2".into()),
            },
            HeaderDiff {
                key: "gyro_lpf1_static_hz".into(),
                a: Some("250".into()),
                b: Some("300".into()),
            },
            HeaderDiff {
                key: "d_max".into(),
                a: Some("40,46,0".into()),
                b: Some("44,48,0".into()),
            },
            HeaderDiff {
                key: "only_a".into(),
                a: Some("1".into()),
                b: None,
            },
            HeaderDiff {
                key: "only_b".into(),
                a: None,
                b: Some("2".into()),
            },
        ];
        let text = cli(&[]).diff(&diff);
        assert_eq!(
            text,
            "# Firmware revision: Betaflight 4.5.1 → Betaflight 4.5.2 (not a CLI setting)\n\
             # gyro_lpf1_static_hz: 250 → 300\n\
             set gyro_lpf1_static_hz = 300\n\
             # d_max: 40,46,0 → 44,48,0\n\
             set d_max_roll = 44\n\
             set d_max_pitch = 48\n\
             set d_max_yaw = 0\n\
             # only_a: 1 → (absent)\n\
             set only_b = 2\n\
             save\n"
        );
        assert_eq!(cli(&[]).diff(&[]), "");
    }

    #[test]
    fn exporter_writes_set_lines_and_rejects_unknown_paths() {
        let s = cli(&[("gyro_lpf1_static_hz", "250")]);
        let proposals = vec![
            Proposal {
                path: "gyro_filters.lpf1.cutoff_hz".into(),
                from: Some("250".into()),
                to: "300".into(),
                rationale: "phase delay above the noise floor".into(),
                confidence: 0.75,
            },
            Proposal {
                path: "pid.roll.d".into(),
                from: None,
                to: "34".into(),
                rationale: "overshoot".into(),
                confidence: 0.5,
            },
        ];
        let text = s.export(&proposals).unwrap();
        assert!(text.starts_with("# OpenPID proposals for "));
        assert!(text.contains("# gyro_filters.lpf1.cutoff_hz: 250 → 300 — phase delay above the noise floor (confidence 75 %)\nset gyro_lpf1_static_hz = 300\n"));
        assert!(text.contains("# pid.roll.d: 34 — overshoot (confidence 50 %)\nset d_roll = 34\n"));
        assert!(text.ends_with("save\n"));
        assert_eq!(s.id(), "betaflight-cli");
        assert_eq!(s.label(), "Betaflight CLI");
        assert!(!s.export(&[]).unwrap().contains("save"));
        let bad = Proposal {
            path: "motor.poles".into(),
            from: None,
            to: "14".into(),
            rationale: String::new(),
            confidence: 1.0,
        };
        assert_eq!(
            s.export(&[bad]).unwrap_err(),
            ExportError::UnknownPath {
                target: "betaflight-cli",
                path: "motor.poles".into()
            }
        );
    }
}
