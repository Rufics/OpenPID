//! Betaflight's tuning settings in words, for the rows the typed model has
//! no field for: rates, throttle curve, TPA, I-term relax, anti-gravity,
//! feedforward, the simplified-tuning sliders, idle and battery.
//!
//! The header names and their units are Betaflight's (4.0 and later; the
//! names changed at 4.3 and both are read). Other firmwares of the family
//! reuse some of the names with other meanings (INAV's cell voltages are in
//! other units, Rotorflight's rate types are its own), so only the
//! Betaflight profile calls this.

use crate::releases::Release;
use openpid_core::settings::Detail;
use openpid_core::{FirmwareInfo, RawHeaders};
use std::fmt::Write as _;

fn detail(label: &str, value: String) -> Detail {
    Detail {
        label: label.to_string(),
        value,
    }
}

/// "a / b / c" of a three-value header through `show`.
fn triple(h: &RawHeaders, name: &str, show: impl Fn(f64) -> String) -> Option<String> {
    let v = h.get_list(name)?;
    (v.len() >= 3).then(|| {
        v[..3]
            .iter()
            .map(|x| show(*x))
            .collect::<Vec<_>>()
            .join(" / ")
    })
}

fn whole(x: f64) -> String {
    format!("{x:.0}")
}

fn hundredths(x: f64) -> String {
    format!("{:.2}", x / 100.0)
}

/// A header that is a number.
fn number(h: &RawHeaders, names: &[&str]) -> Option<f64> {
    h.first_of(names)?.trim().parse().ok()
}

fn rates(h: &RawHeaders) -> Option<Detail> {
    // roll / pitch / yaw of the three rate headers; what they mean depends
    // on the rates type (Betaflight's `ratesType_e`)
    let kind = number(h, &["rates_type"]).unwrap_or(0.0) as i64;
    let value = match kind {
        0 => format!(
            "Betaflight: RC rate {}, super rate {}, expo {}",
            triple(h, "rc_rates", hundredths)?,
            triple(h, "rates", hundredths)?,
            triple(h, "rc_expo", hundredths)?,
        ),
        3 => format!(
            "Actual: centre {} deg/s, max {} deg/s, expo {}",
            triple(h, "rc_rates", |x| whole(x * 10.0))?,
            triple(h, "rates", |x| whole(x * 10.0))?,
            triple(h, "rc_expo", hundredths)?,
        ),
        other => {
            let name = match other {
                1 => "Raceflight".to_string(),
                2 => "KISS".to_string(),
                4 => "Quick".to_string(),
                n => format!("type {n}"),
            };
            // as logged: the scaling of these types is not decoded here
            format!(
                "{name}: rc_rates {}, rates {}, rc_expo {} (as logged)",
                triple(h, "rc_rates", whole)?,
                triple(h, "rates", whole)?,
                triple(h, "rc_expo", whole)?,
            )
        }
    };
    Some(detail("Rates", value))
}

fn tpa(h: &RawHeaders) -> Option<Detail> {
    let rate = number(h, &["tpa_rate"])?;
    let breakpoint = number(h, &["tpa_breakpoint"])?;
    if rate <= 0.0 {
        return Some(detail("TPA", "off".to_string()));
    }
    let mut value = format!("{rate:.0} % above {breakpoint:.0}");
    // `tpaMode_e`: 0 = P and D, 1 = D only (4.3 and later log it)
    match number(h, &["tpa_mode"]).map(|m| m as i64) {
        Some(0) => value.push_str(", on P and D"),
        Some(1) => value.push_str(", on D"),
        _ => {}
    }
    if let (Some(low), Some(below)) = (
        number(h, &["tpa_low_rate"]).filter(|r| *r > 0.0),
        number(h, &["tpa_low_breakpoint"]),
    ) {
        let _ = write!(value, "; {low:.0} % below {below:.0}");
    }
    Some(detail("TPA", value))
}

fn iterm_relax(h: &RawHeaders) -> Option<Detail> {
    // `itermRelax_e`
    let axes = match number(h, &["iterm_relax"])? as i64 {
        0 => return Some(detail("I-term relax", "off".to_string())),
        1 => "roll and pitch",
        2 => "roll, pitch and yaw",
        3 => "roll and pitch, increasing only",
        4 => "roll, pitch and yaw, increasing only",
        _ => return None,
    };
    let mut value = axes.to_string();
    match number(h, &["iterm_relax_type"]).map(|t| t as i64) {
        Some(0) => value.push_str(", from the gyro"),
        Some(1) => value.push_str(", from the setpoint"),
        _ => {}
    }
    if let Some(hz) = number(h, &["iterm_relax_cutoff"]) {
        let _ = write!(value, ", cut-off {hz:.0} Hz");
    }
    Some(detail("I-term relax", value))
}

fn anti_gravity(h: &RawHeaders) -> Option<Detail> {
    let gain = number(h, &["anti_gravity_gain"])?;
    let value = if let Some(hz) = number(h, &["anti_gravity_cutoff_hz"]) {
        // 4.4 and later
        let p = number(h, &["anti_gravity_p_gain"])
            .map_or(String::new(), |p| format!(", P gain {p:.0}"));
        format!("gain {gain:.0}, cut-off {hz:.0} Hz{p}")
    } else {
        // up to 4.3: the gain in thousandths, a mode and a threshold
        let mode = match number(h, &["anti_gravity_mode"]).map(|m| m as i64) {
            Some(0) => ", smooth",
            Some(1) => ", step",
            _ => "",
        };
        let threshold = number(h, &["anti_gravity_threshold"])
            .map_or(String::new(), |t| format!(", threshold {t:.0}"));
        format!("gain {:.1}{mode}{threshold}", gain / 1000.0)
    };
    Some(detail("Anti-gravity", value))
}

fn feedforward(h: &RawHeaders) -> Option<Detail> {
    let mut parts = Vec::new();
    if let Some(v) = number(h, &["feedforward_transition"]) {
        parts.push(format!("transition {v:.0}"));
    }
    // 4.3 and later: `feedforwardAveraging_e`; before: `ff_interpolate_sp`
    if let Some(v) = number(h, &["feedforward_averaging"]) {
        parts.push(match v as i64 {
            0 => "averaging off".to_string(),
            n @ 1..=3 => format!("averaging {} point", n + 1),
            n => format!("averaging {n}"),
        });
    } else if let Some(v) = number(h, &["ff_interpolate_sp"]) {
        parts.push(match v as i64 {
            0 => "interpolation off".to_string(),
            1 => "interpolation on".to_string(),
            n @ 2..=4 => format!("interpolation averaged {n}"),
            n => format!("interpolation {n}"),
        });
    }
    if let Some(v) = number(h, &["feedforward_smooth_factor"]) {
        parts.push(format!("smoothing {v:.0}"));
    }
    if let Some(v) = number(h, &["feedforward_jitter_factor"]) {
        parts.push(format!("jitter reduction {v:.0}"));
    }
    if let Some(v) = number(h, &["ff_spike_limit"]) {
        parts.push(format!("spike limit {v:.0}"));
    }
    if let Some(v) = number(h, &["feedforward_boost", "ff_boost"]) {
        parts.push(format!("boost {v:.0}"));
    }
    if let Some(v) = number(h, &["feedforward_max_rate_limit", "ff_max_rate_limit"]) {
        parts.push(format!("max rate limit {v:.0} %"));
    }
    (!parts.is_empty()).then(|| detail("Feedforward", parts.join(", ")))
}

fn sliders(h: &RawHeaders) -> Option<Detail> {
    let mode = match number(h, &["simplified_pids_mode"])? as i64 {
        0 => return Some(detail("Simplified tuning", "off".to_string())),
        1 => "roll and pitch",
        2 => "roll, pitch and yaw",
        _ => return None,
    };
    let mut parts = vec![mode.to_string()];
    for (names, what) in [
        (&["simplified_master_multiplier"][..], "master"),
        (&["simplified_pi_gain"], "PI"),
        (&["simplified_i_gain"], "I"),
        (&["simplified_d_gain"], "D"),
        (&["simplified_d_max_gain", "simplified_dmax_gain"], "D max"),
        (&["simplified_feedforward_gain"], "FF"),
        (&["simplified_pitch_d_gain"], "pitch D"),
        (&["simplified_pitch_pi_gain"], "pitch PI"),
    ] {
        if let Some(v) = number(h, names) {
            parts.push(format!("{what} {v:.0}"));
        }
    }
    Some(detail("Simplified tuning", parts.join(", ")))
}

fn idle(h: &RawHeaders) -> Option<Detail> {
    // both in hundredths of a percent of the throttle range
    let percent = number(h, &["motor_idle", "dshot_idle_value"])? / 100.0;
    let mut value = format!("{percent:.1} %");
    match number(h, &["dyn_idle_min_rpm"]) {
        Some(rpm) if rpm > 0.0 => {
            let _ = write!(value, ", dynamic idle from {:.0} rpm", rpm * 100.0);
        }
        Some(_) => value.push_str(", dynamic idle off"),
        None => {}
    }
    Some(detail("Motor idle", value))
}

fn battery(h: &RawHeaders) -> Option<Detail> {
    // hundredths of a volt
    let cells = h.get_list("vbatcellvoltage").filter(|v| v.len() >= 3)?;
    let volts = |x: f64| format!("{:.2}", x / 100.0);
    let mut value = format!(
        "cell min {} V, warning {} V, max {} V",
        volts(cells[0]),
        volts(cells[1]),
        volts(cells[2])
    );
    if let Some(v) = number(h, &["vbatref"]).filter(|v| *v > 0.0) {
        let _ = write!(value, "; {} V at arming", volts(v));
    }
    if let Some(v) = number(h, &["vbat_sag_compensation"]).filter(|v| *v > 0.0) {
        let _ = write!(value, "; sag compensation {v:.0} %");
    }
    Some(detail("Battery", value))
}

/// The settings of a Betaflight log in words, in the order to show them.
/// Nothing for Betaflight before 4.0, whose headers had other units.
pub fn betaflight(fw: &FirmwareInfo, h: &RawHeaders) -> Vec<Detail> {
    if Release::of_number(fw.version).is_before_4_0() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend(rates(h));
    if let Some(v) = triple(h, "rate_limits", whole) {
        out.push(detail("Rate limit", format!("{v} deg/s")));
    }
    if let (Some(mid), Some(expo)) = (number(h, &["thr_mid"]), number(h, &["thr_expo"])) {
        out.push(detail(
            "Throttle curve",
            format!("mid {mid:.0} %, expo {expo:.0} %"),
        ));
    }
    out.extend(tpa(h));
    out.extend(iterm_relax(h));
    if let Some(v) = number(h, &["iterm_windup"]) {
        out.push(detail("I-term windup", format!("{v:.0} %")));
    }
    out.extend(anti_gravity(h));
    if let Some(v) = number(h, &["throttle_boost"]) {
        let cutoff = number(h, &["throttle_boost_cutoff"])
            .map_or(String::new(), |c| format!(", cut-off {c:.0} Hz"));
        out.push(detail("Throttle boost", format!("{v:.0}{cutoff}")));
    }
    out.extend(feedforward(h));
    out.extend(sliders(h));
    out.extend(idle(h));
    out.extend(battery(h));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> RawHeaders {
        RawHeaders(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    fn value(details: &[Detail], label: &str) -> Option<String> {
        details
            .iter()
            .find(|d| d.label == label)
            .map(|d| d.value.clone())
    }

    #[test]
    fn betaflight_4_5_settings_in_words() {
        let fw = FirmwareInfo::parse_revision("Betaflight 4.5.1 (77d01ba3b) STM32F7X2");
        let h = headers(&[
            ("rates_type", "3"),
            ("rc_rates", "7,7,7"),
            ("rates", "67,67,67"),
            ("rc_expo", "0,0,0"),
            ("rate_limits", "1998,1998,1998"),
            ("thr_mid", "50"),
            ("thr_expo", "0"),
            ("tpa_mode", "1"),
            ("tpa_rate", "65"),
            ("tpa_breakpoint", "1350"),
            ("tpa_low_rate", "20"),
            ("tpa_low_breakpoint", "1050"),
            ("iterm_relax", "1"),
            ("iterm_relax_type", "1"),
            ("iterm_relax_cutoff", "15"),
            ("iterm_windup", "85"),
            ("anti_gravity_gain", "80"),
            ("anti_gravity_cutoff_hz", "5"),
            ("anti_gravity_p_gain", "100"),
            ("feedforward_transition", "0"),
            ("feedforward_averaging", "1"),
            ("feedforward_smooth_factor", "25"),
            ("feedforward_jitter_factor", "7"),
            ("feedforward_boost", "15"),
            ("feedforward_max_rate_limit", "90"),
            ("simplified_pids_mode", "2"),
            ("simplified_master_multiplier", "100"),
            ("simplified_d_gain", "120"),
            ("simplified_dmax_gain", "90"),
            ("dshot_idle_value", "550"),
            ("dyn_idle_min_rpm", "20"),
            ("vbatcellvoltage", "330,350,430"),
            ("vbatref", "1584"),
            ("vbat_sag_compensation", "0"),
        ]);
        let d = betaflight(&fw, &h);
        let expected = [
            (
                "Rates",
                "Actual: centre 70 / 70 / 70 deg/s, max 670 / 670 / 670 deg/s, expo 0.00 / 0.00 / 0.00",
            ),
            ("Rate limit", "1998 / 1998 / 1998 deg/s"),
            ("Throttle curve", "mid 50 %, expo 0 %"),
            ("TPA", "65 % above 1350, on D; 20 % below 1050"),
            (
                "I-term relax",
                "roll and pitch, from the setpoint, cut-off 15 Hz",
            ),
            ("I-term windup", "85 %"),
            ("Anti-gravity", "gain 80, cut-off 5 Hz, P gain 100"),
            (
                "Feedforward",
                "transition 0, averaging 2 point, smoothing 25, jitter reduction 7, boost 15, max rate limit 90 %",
            ),
            (
                "Simplified tuning",
                "roll, pitch and yaw, master 100, D 120, D max 90",
            ),
            ("Motor idle", "5.5 %, dynamic idle from 2000 rpm"),
            (
                "Battery",
                "cell min 3.30 V, warning 3.50 V, max 4.30 V; 15.84 V at arming",
            ),
        ];
        for (label, text) in expected {
            assert_eq!(value(&d, label).as_deref(), Some(text), "{label}");
        }
        let labels: Vec<&str> = d.iter().map(|x| x.label.as_str()).collect();
        let order: Vec<&str> = expected.iter().map(|e| e.0).collect();
        assert_eq!(labels, order, "in the order of the tuning tabs");
    }

    #[test]
    fn older_names_other_rate_types_and_what_a_log_lacks() {
        // 4.2: Betaflight rates, the old feedforward and anti-gravity names
        let fw = FirmwareInfo::parse_revision("Betaflight 4.2.0 (8f2d21460) STM32F745");
        let h = headers(&[
            ("rates_type", "0"),
            ("rc_rates", "100,100,100"),
            ("rates", "62,62,65"),
            ("rc_expo", "30,30,20"),
            ("tpa_rate", "65"),
            ("tpa_breakpoint", "1350"),
            ("iterm_relax", "0"),
            ("anti_gravity_mode", "0"),
            ("anti_gravity_threshold", "250"),
            ("anti_gravity_gain", "3500"),
            ("feedforward_transition", "40"),
            ("ff_interpolate_sp", "3"),
            ("ff_spike_limit", "55"),
            ("ff_max_rate_limit", "100"),
            ("ff_boost", "0"),
            ("dshot_idle_value", "550"),
        ]);
        let d = betaflight(&fw, &h);
        assert_eq!(
            value(&d, "Rates").as_deref(),
            Some(
                "Betaflight: RC rate 1.00 / 1.00 / 1.00, super rate 0.62 / 0.62 / 0.65, expo 0.30 / 0.30 / 0.20"
            )
        );
        assert_eq!(value(&d, "TPA").as_deref(), Some("65 % above 1350"));
        assert_eq!(value(&d, "I-term relax").as_deref(), Some("off"));
        assert_eq!(
            value(&d, "Anti-gravity").as_deref(),
            Some("gain 3.5, smooth, threshold 250")
        );
        assert_eq!(
            value(&d, "Feedforward").as_deref(),
            Some(
                "transition 40, interpolation averaged 3, spike limit 55, boost 0, max rate limit 100 %"
            )
        );
        assert_eq!(value(&d, "Motor idle").as_deref(), Some("5.5 %"));
        // what the log does not say has no row
        for label in [
            "Simplified tuning",
            "Battery",
            "Throttle curve",
            "Rate limit",
        ] {
            assert_eq!(value(&d, label), None, "{label}");
        }
        // a rate type whose scaling is not decoded is shown as logged
        let kiss = headers(&[
            ("rates_type", "2"),
            ("rc_rates", "100,100,100"),
            ("rates", "70,70,70"),
            ("rc_expo", "0,0,0"),
        ]);
        assert_eq!(
            value(&betaflight(&fw, &kiss), "Rates").as_deref(),
            Some(
                "KISS: rc_rates 100 / 100 / 100, rates 70 / 70 / 70, rc_expo 0 / 0 / 0 (as logged)"
            )
        );
        // switched off
        let off = headers(&[
            ("tpa_rate", "0"),
            ("tpa_breakpoint", "1350"),
            ("simplified_pids_mode", "0"),
        ]);
        let d = betaflight(&fw, &off);
        assert_eq!(value(&d, "TPA").as_deref(), Some("off"));
        assert_eq!(value(&d, "Simplified tuning").as_deref(), Some("off"));
        // before 4.0 the headers had other units: nothing is claimed
        let old = FirmwareInfo::parse_revision("Betaflight 3.5.7 (e9130527c) OMNIBUSF4SD");
        assert!(betaflight(&old, &h).is_empty());
    }
}
