//! A session in plain words: what the decoder read and how the craft was set
//! up, as rows of a label and a value. The Setup info tab lists them for
//! every log side by side; a row is compared across logs by its label.
//!
//! The rows come from the typed model only ([`Settings`], [`Diagnostics`]),
//! so they read the same for every firmware a profile exists for.
//!
//! [`Diagnostics`]: crate::session::Diagnostics

use crate::session::FlightSession;
use crate::settings::{FilterChain, Lowpass, LowpassKind, PidGains, Settings};

/// One row: a fixed English label and the value in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// What the row is; fixed text, so an interface can translate it.
    pub label: &'static str,
    /// The value, with its unit.
    pub value: String,
}

fn fact(label: &'static str, value: impl Into<String>) -> Fact {
    Fact {
        label,
        value: value.into(),
    }
}

/// A frequency without a trailing ".0".
fn hz(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn kind_name(kind: &LowpassKind) -> String {
    match kind {
        LowpassKind::Pt1 => "PT1".to_string(),
        LowpassKind::Biquad => "biquad".to_string(),
        LowpassKind::Pt2 => "PT2".to_string(),
        LowpassKind::Pt3 => "PT3".to_string(),
        LowpassKind::Other(code) => format!("type {code}"),
    }
}

/// "PT1 500 Hz", "biquad 75–150 Hz by throttle" or "off".
fn lowpass_text(l: &Lowpass) -> String {
    let kind = l.kind.as_ref().map(kind_name);
    let range = match (l.dyn_min_hz, l.dyn_max_hz) {
        (Some(lo), Some(hi)) if hi > 0.0 => Some(format!("{}–{} Hz by throttle", hz(lo), hz(hi))),
        _ if l.cutoff_hz > 0.0 => Some(format!("{} Hz", hz(l.cutoff_hz))),
        _ => None,
    };
    match (kind, range) {
        (Some(k), Some(r)) => format!("{k} {r}"),
        (None, Some(r)) => r,
        (_, None) => "off".to_string(),
    }
}

/// "P 45 · I 80 · D 30 · D min 23 · FF 120" at the firmware's gain step.
fn pid_text(settings: &Settings, g: &PidGains) -> String {
    let mut parts = vec![
        format!("P {}", settings.gain_text(g.p)),
        format!("I {}", settings.gain_text(g.i)),
        format!("D {}", settings.gain_text(g.d)),
    ];
    if let Some(v) = g.d_min {
        parts.push(format!("D min {}", settings.gain_text(v)));
    }
    if let Some(v) = g.d_max {
        parts.push(format!("D max {}", settings.gain_text(v)));
    }
    if let Some(v) = g.f {
        parts.push(format!("FF {}", settings.gain_text(v)));
    }
    parts.join(" · ")
}

/// The labels of one filter chain's rows.
struct ChainLabels {
    lowpass1: &'static str,
    lowpass2: &'static str,
    notch: [&'static str; 2],
    dyn_notch: &'static str,
    rpm_notch: &'static str,
}

const GYRO: ChainLabels = ChainLabels {
    lowpass1: "Gyro low-pass 1",
    lowpass2: "Gyro low-pass 2",
    notch: ["Gyro notch 1", "Gyro notch 2"],
    dyn_notch: "Dynamic notch",
    rpm_notch: "RPM filter",
};

const DTERM: ChainLabels = ChainLabels {
    lowpass1: "D-term low-pass 1",
    lowpass2: "D-term low-pass 2",
    notch: ["D-term notch 1", "D-term notch 2"],
    dyn_notch: "D-term dynamic notch",
    rpm_notch: "D-term RPM filter",
};

fn chain(out: &mut Vec<Fact>, c: &FilterChain, labels: &ChainLabels) {
    for (label, l) in [
        (labels.lowpass1, &c.lowpass1),
        (labels.lowpass2, &c.lowpass2),
    ] {
        if let Some(l) = l {
            out.push(fact(label, lowpass_text(l)));
        }
    }
    for (label, n) in labels.notch.iter().zip(&c.notches) {
        out.push(fact(
            label,
            format!("{} Hz, cut-off {} Hz", hz(n.center_hz), hz(n.cutoff_hz)),
        ));
    }
    if let Some(d) = &c.dyn_notch {
        let count = d.count.map_or(String::new(), |n| format!("{n} × "));
        out.push(fact(
            labels.dyn_notch,
            format!("{count}Q {:.1}, {}–{} Hz", d.q, hz(d.min_hz), hz(d.max_hz)),
        ));
    }
    if let Some(r) = &c.rpm_notch {
        out.push(fact(
            labels.rpm_notch,
            format!(
                "{} harmonics, Q {:.1}, from {} Hz",
                r.harmonics,
                r.q,
                hz(r.min_hz)
            ),
        ));
    }
}

/// How the craft was set up, from the typed settings: firmware, rates, PID
/// gains, filters, motors. A row is left out when the log does not say.
pub fn setup(s: &FlightSession) -> Vec<Fact> {
    let set = &s.settings;
    let mut out = vec![fact("Firmware", s.firmware.revision.clone())];
    if let Some(v) = &s.firmware.target {
        out.push(fact("Board", v.clone()));
    }
    if let Some(v) = &s.firmware.build_date {
        out.push(fact("Firmware date", v.clone()));
    }
    if let Some(v) = &set.craft_name {
        out.push(fact("Craft name", v.clone()));
    }
    for (label, rate) in [
        ("Gyro rate", set.loop_rates.gyro_hz),
        ("PID loop rate", set.loop_rates.pid_hz),
        ("Logging rate", set.loop_rates.log_hz),
    ] {
        if let Some(v) = rate {
            out.push(fact(label, format!("{} Hz", hz(v))));
        }
    }
    if let Some(pid) = &set.pid {
        out.push(fact("PID roll", pid_text(set, &pid.roll)));
        out.push(fact("PID pitch", pid_text(set, &pid.pitch)));
        out.push(fact("PID yaw", pid_text(set, &pid.yaw)));
    }
    chain(&mut out, &set.gyro_filters, &GYRO);
    chain(&mut out, &set.dterm_filters, &DTERM);
    let m = &set.motor;
    if let Some(v) = &m.protocol {
        out.push(fact("ESC protocol", v.clone()));
    }
    if let Some(v) = m.count {
        out.push(fact("Motors", v.to_string()));
    }
    if let Some(v) = m.poles {
        out.push(fact("Motor poles", v.to_string()));
    }
    if let Some(v) = m.kv {
        out.push(fact("Motor KV", hz(v)));
    }
    if let (Some(lo), Some(hi)) = (m.output_low, m.output_high) {
        out.push(fact("Motor output", format!("{}–{}", hz(lo), hz(hi))));
    }
    if let Some(d) = &set.debug_mode {
        out.push(fact(
            "Debug mode",
            d.name.clone().unwrap_or_else(|| d.code.to_string()),
        ));
    }
    out
}

/// What the decoder read: the profile, the samples and their timing, the
/// frames by kind, what it could not read, and its notes (one row each).
pub fn decoder(s: &FlightSession) -> Vec<Fact> {
    let d = &s.diagnostics;
    let mut out = vec![
        fact("Profile", s.profile_id.clone()),
        fact(
            "Samples",
            format!("{} over {:.1} s", s.len(), s.timebase.duration_s()),
        ),
        fact(
            "Sample step",
            format!(
                "{} µs median, {} gaps",
                hz(s.timebase.median_dt_us),
                s.timebase.gaps.len()
            ),
        ),
    ];
    if !d.frames.is_empty() {
        out.push(fact(
            "Frames",
            d.frames
                .iter()
                .map(|(k, v)| format!("{k}:{}", v.valid))
                .collect::<Vec<_>>()
                .join("  "),
        ));
    }
    out.push(fact("Corrupt frames", d.corrupt_frames.to_string()));
    out.push(fact("Desynchronised frames", d.desync_frames.to_string()));
    out.push(fact("Skipped bytes", d.skipped_bytes.to_string()));
    if !d.unmapped_fields.is_empty() {
        out.push(fact("Unmapped fields", d.unmapped_fields.join(", ")));
    }
    out.extend(d.notes.iter().map(|n| fact("Note", n.clone())));
    out
}

/// One row of a comparison of several logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compared<L> {
    /// What the row is.
    pub label: L,
    /// The value of each log, in the order the logs were given; `None` for
    /// a log without this row.
    pub values: Vec<Option<String>>,
    /// The logs do not all say the same (a missing row counts).
    pub differs: bool,
}

/// The rows of several logs side by side. Rows are matched by label (a
/// repeated label by its turn), and keep the order of the logs: a row only
/// a later log has comes after the row that precedes it there.
pub fn compare<L: Clone + PartialEq>(logs: &[Vec<(L, String)>]) -> Vec<Compared<L>> {
    // (label, which occurrence of it) identifies a row
    let mut keys: Vec<(L, usize)> = Vec::new();
    let mut out: Vec<Compared<L>> = Vec::new();
    for (n, rows) in logs.iter().enumerate() {
        let mut after = 0;
        let mut seen: Vec<(L, usize)> = Vec::new();
        for (label, value) in rows {
            let turn = seen.iter().filter(|(l, _)| l == label).count();
            let key = (label.clone(), turn);
            seen.push(key.clone());
            let at = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                keys.insert(after, key);
                out.insert(
                    after,
                    Compared {
                        label: label.clone(),
                        values: vec![None; logs.len()],
                        differs: false,
                    },
                );
                after
            });
            out[at].values[n] = Some(value.clone());
            after = at + 1;
        }
    }
    for row in &mut out {
        row.differs = row.values.windows(2).any(|w| w[0] != w[1]);
    }
    out
}

/// A session's own rows in the form [`compare`] takes.
pub fn rows(facts: Vec<Fact>) -> Vec<(&'static str, String)> {
    facts.into_iter().map(|f| (f.label, f.value)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ChannelSet, Diagnostics, FirmwareInfo, FrameStats, Timebase};
    use crate::settings::{DebugMode, DynNotch, Notch, PerAxis, RpmNotch};

    /// 11 samples at 100 ms, nothing else.
    fn session() -> FlightSession {
        FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware: FirmwareInfo::unknown(),
            profile_id: "test".into(),
            settings: Settings::default(),
            raw_headers: Vec::new(),
            timebase: Timebase::from_times(
                (0..11).map(|i| 1_000_000 + i * 100_000).collect(),
                None,
            ),
            channels: ChannelSet::new(),
            events: Vec::new(),
            aux: Vec::new(),
            flight_modes: Vec::new(),
            diagnostics: Diagnostics::default(),
        }
    }

    fn value<'a>(rows: &'a [Fact], label: &str) -> Option<&'a str> {
        rows.iter()
            .find(|f| f.label == label)
            .map(|f| f.value.as_str())
    }

    #[test]
    fn setup_reads_the_typed_settings_in_plain_words() {
        let mut s = session();
        s.firmware.revision = "Betaflight 4.5.1 (77d01ba3b) STM32F7X2".to_string();
        s.firmware.target = Some("STM32F7X2".to_string());
        let gains = PidGains {
            p: 45.0,
            i: 80.0,
            d: 30.0,
            f: Some(120.0),
            d_min: Some(23.0),
            d_max: None,
        };
        s.settings.pid = Some(PerAxis {
            roll: gains,
            pitch: gains,
            yaw: PidGains {
                d: 0.0,
                d_min: None,
                ..gains
            },
        });
        s.settings.loop_rates.gyro_hz = Some(8000.0);
        s.settings.loop_rates.log_hz = Some(1562.5);
        s.settings.gyro_filters = FilterChain {
            lowpass1: Some(Lowpass {
                kind: Some(LowpassKind::Pt1),
                cutoff_hz: 0.0,
                dyn_min_hz: None,
                dyn_max_hz: None,
                dyn_expo: 0.0,
            }),
            lowpass2: Some(Lowpass {
                kind: Some(LowpassKind::Pt1),
                cutoff_hz: 500.0,
                dyn_min_hz: None,
                dyn_max_hz: None,
                dyn_expo: 0.0,
            }),
            notches: vec![Notch {
                center_hz: 240.0,
                cutoff_hz: 200.0,
            }],
            dyn_notch: Some(DynNotch {
                min_hz: 100.0,
                max_hz: 600.0,
                q: 3.0,
                count: Some(3),
            }),
            rpm_notch: Some(RpmNotch {
                harmonics: 3,
                q: 5.0,
                min_hz: 100.0,
                fade_range_hz: 50.0,
                weights: Vec::new(),
                lpf_hz: 150.0,
            }),
        };
        s.settings.dterm_filters.lowpass1 = Some(Lowpass {
            kind: Some(LowpassKind::Biquad),
            cutoff_hz: 75.0,
            dyn_min_hz: Some(75.0),
            dyn_max_hz: Some(150.0),
            dyn_expo: 0.5,
        });
        s.settings.motor.protocol = Some("DSHOT600".to_string());
        s.settings.motor.poles = Some(14);
        s.settings.motor.output_low = Some(158.0);
        s.settings.motor.output_high = Some(2047.0);
        s.settings.debug_mode = Some(DebugMode {
            code: 6,
            name: Some("GYRO_SCALED".to_string()),
        });
        let rows = setup(&s);
        let expected = [
            ("Firmware", "Betaflight 4.5.1 (77d01ba3b) STM32F7X2"),
            ("Board", "STM32F7X2"),
            ("Gyro rate", "8000 Hz"),
            ("Logging rate", "1562.5 Hz"),
            ("PID roll", "P 45 · I 80 · D 30 · D min 23 · FF 120"),
            ("PID yaw", "P 45 · I 80 · D 0 · FF 120"),
            ("Gyro low-pass 1", "off"),
            ("Gyro low-pass 2", "PT1 500 Hz"),
            ("Gyro notch 1", "240 Hz, cut-off 200 Hz"),
            ("Dynamic notch", "3 × Q 3.0, 100–600 Hz"),
            ("RPM filter", "3 harmonics, Q 5.0, from 100 Hz"),
            ("D-term low-pass 1", "biquad 75–150 Hz by throttle"),
            ("ESC protocol", "DSHOT600"),
            ("Motor poles", "14"),
            ("Motor output", "158–2047"),
            ("Debug mode", "GYRO_SCALED"),
        ];
        for (label, text) in expected {
            assert_eq!(value(&rows, label), Some(text), "{label}");
        }
        // what the log does not say has no row
        for label in ["Craft name", "PID loop rate", "Motor KV", "D-term notch 1"] {
            assert_eq!(value(&rows, label), None, "{label}");
        }
        // the gains keep the firmware's step
        s.settings.pid_step = Some(0.0001);
        s.settings.pid.as_mut().unwrap().roll.p = 0.135;
        let rows = setup(&s);
        assert!(value(&rows, "PID roll").unwrap().starts_with("P 0.135 · "));
    }

    #[test]
    fn decoder_rows_count_what_was_read_and_what_was_not() {
        let mut s = session();
        s.profile_id = "betaflight".to_string();
        s.diagnostics.frames.insert(
            "I".to_string(),
            FrameStats {
                valid: 3,
                ..FrameStats::default()
            },
        );
        s.diagnostics.frames.insert(
            "P".to_string(),
            FrameStats {
                valid: 8,
                ..FrameStats::default()
            },
        );
        s.diagnostics.corrupt_frames = 2;
        s.diagnostics.unmapped_fields = vec!["foo".to_string(), "bar".to_string()];
        s.diagnostics.notes = vec!["first".to_string(), "second".to_string()];
        let rows = decoder(&s);
        assert_eq!(value(&rows, "Profile"), Some("betaflight"));
        assert_eq!(value(&rows, "Samples"), Some("11 over 1.0 s"));
        assert_eq!(
            value(&rows, "Sample step"),
            Some("100000 µs median, 0 gaps")
        );
        assert_eq!(value(&rows, "Frames"), Some("I:3  P:8"));
        assert_eq!(value(&rows, "Corrupt frames"), Some("2"));
        assert_eq!(value(&rows, "Skipped bytes"), Some("0"));
        assert_eq!(value(&rows, "Unmapped fields"), Some("foo, bar"));
        let notes: Vec<&str> = rows
            .iter()
            .filter(|f| f.label == "Note")
            .map(|f| f.value.as_str())
            .collect();
        assert_eq!(notes, ["first", "second"]);
    }

    #[test]
    fn compare_matches_rows_by_label_and_marks_the_differences() {
        let row = |l: &'static str, v: &str| (l, v.to_string());
        let a = vec![
            row("Firmware", "4.5.0"),
            row("Gyro rate", "8000 Hz"),
            row("Note", "x"),
        ];
        let b = vec![
            row("Firmware", "4.5.1"),
            row("Craft name", "Cinelog"),
            row("Gyro rate", "8000 Hz"),
            row("Note", "x"),
            row("Note", "y"),
        ];
        let table = compare(&[a.clone(), b]);
        let labels: Vec<&str> = table.iter().map(|r| r.label).collect();
        assert_eq!(
            labels,
            ["Firmware", "Craft name", "Gyro rate", "Note", "Note"],
            "a row of the second log only sits where that log has it"
        );
        let differs: Vec<bool> = table.iter().map(|r| r.differs).collect();
        assert_eq!(differs, [true, true, false, false, true]);
        assert_eq!(table[1].values, [None, Some("Cinelog".to_string())]);
        assert_eq!(table[4].values, [None, Some("y".to_string())]);
        // one log has nothing to differ from
        assert!(compare(&[a]).iter().all(|r| !r.differs));
        assert!(compare::<&str>(&[]).is_empty());
    }
}
