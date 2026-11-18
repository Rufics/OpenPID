//! The copies of the filter simulator's plots.
//!
//! The simulator reads no log: what it draws is the set of filters typed
//! into it, which the manifest of the round states
//! ([`crate::capture::filter_set`]). Each of those filters is built with
//! `OpenPID`'s filter simulator (`openpid-sim`) at the loop rate of the set,
//! and its curve, magnitude, phase delay, phase or step response, is held
//! against the copy's nearest line. A point of a copy is only known to a
//! unit of its metafile each way, so a point agrees with a curve when the
//! curve passes within half a unit of it. The delays the simulator prints
//! (each filter's phase delay at 1 Hz, the sum of each column, an RPM notch
//! counted once a motor) are held against `OpenPID`'s to their last digit.
//! A results file may give the curves of a copy instead.

use super::{
    Copy, Held, Outcome, Source, differences, limit, not_compared, number, resolution, tokens,
    unit_of,
};
use crate::capture::FilterSet;
use crate::emf::{Axes, Metafile, Scale};
use openpid_sim::{Biquad, Filter, Pt1, Ptn, Stage};
use std::fmt::Write as _;

/// The two columns of the simulator's plots, as the copies name them.
const COLUMNS: [&str; 2] = ["lowpass", "notch"];

/// The check of each plot of a column, by its name in the copies.
const PLOTS: [(&str, &str); 4] = [
    ("magnitude", "filter magnitude"),
    ("delay", "filter delay"),
    ("phase", "filter phase"),
    ("step", "filter step"),
];

/// The column and the plot a copy's name ends with.
fn column_and_plot(rest: &str) -> Option<(&str, &str)> {
    let words = tokens(rest);
    let at = words.iter().position(|w| COLUMNS.contains(w))?;
    let plot = words.get(at + 1..)?.join("-");
    let (name, _) = PLOTS.iter().find(|(p, _)| *p == plot)?;
    Some((words[at], name))
}

/// What `compare` reads off the name of a copy of the simulator.
#[cfg(test)]
pub(super) fn parsed(rest: &str) -> Option<String> {
    column_and_plot(rest).map(|(c, p)| format!("{c} {p}"))
}

/// One filter of a set, as `OpenPID`'s simulator builds it.
struct Part {
    /// What it is, for the report: its type and frequency.
    label: String,
    stage: Stage,
    /// How often it counts in the delay its column prints: an RPM notch
    /// once a motor.
    counted: f64,
}

/// A low-pass of the set's list of types at `hz` and the loop rate `fs`.
fn lowpass(kind: &str, hz: f64, fs: f64) -> Result<Stage, String> {
    if !(hz > 0.0 && hz < fs / 2.0) {
        return Err(format!("a low-pass at {hz} Hz at a loop rate of {fs} Hz"));
    }
    match kind {
        "pt1" => Ok(Stage::Pt1(Pt1::new(hz, fs))),
        "pt2" => Ok(Stage::Ptn(Ptn::new(2, hz, fs))),
        "pt3" => Ok(Stage::Ptn(Ptn::new(3, hz, fs))),
        "biquad-butter" => Ok(Stage::Biquad(Biquad::lowpass(hz, fs))),
        other => Err(format!(
            "OpenPID's filter simulator has no {other} low-pass"
        )),
    }
}

/// A notch at `hz` of quality `q` at the loop rate `fs`.
fn notch(hz: f64, q: f64, fs: f64) -> Result<Stage, String> {
    if !(hz > 0.0 && hz < fs / 2.0 && q > 0.0) {
        return Err(format!("a notch at {hz} Hz at a loop rate of {fs} Hz"));
    }
    Ok(Stage::Biquad(Biquad::notch_q(hz, q, fs)))
}

/// The filters of one column of the simulator for the set `set`.
fn parts(set: &FilterSet, column: &str) -> Result<Vec<Part>, String> {
    let fs = f64::from(set.loop_rate_hz);
    let mut out = Vec::new();
    if column == "lowpass" {
        for lp in [set.lowpass_1, set.lowpass_2] {
            out.push(Part {
                label: format!("{} {} Hz", lp.kind, lp.hz),
                stage: lowpass(lp.kind, f64::from(lp.hz), fs)?,
                counted: 1.0,
            });
        }
        return Ok(out);
    }
    for k in 1..=set.rpm.harmonics {
        let hz = f64::from(k * set.rpm.hz);
        out.push(Part {
            label: format!("RPM notch {hz} Hz"),
            stage: notch(hz, set.rpm.q, fs)?,
            counted: f64::from(set.motors),
        });
    }
    out.push(Part {
        label: format!("notch {} Hz", set.notch.hz),
        stage: notch(f64::from(set.notch.hz), set.notch.q, fs)?,
        counted: 1.0,
    });
    Ok(out)
}

/// The phase delay of `stage` at `hz`, ms.
fn delay_ms(stage: &Stage, hz: f64) -> f64 {
    -stage.response(hz).arg() / (std::f64::consts::TAU * hz) * 1000.0
}

/// The response of `stage` to a unit step, sample by sample: `n` samples.
fn step_response(stage: &Stage, n: usize) -> Vec<f64> {
    let mut f = stage.clone();
    f.reset();
    (0..n).map(|_| f.update(1.0)).collect()
}

/// The curve of `stage` a plot draws, as a function of its abscissa: of
/// the frequency for magnitude, delay and phase, of the time in ms for the
/// step response (the samples joined by straight lines, the first at 0).
fn curve(stage: &Stage, plot: &str, until: f64) -> Box<dyn Fn(f64) -> Option<f64>> {
    let stage = stage.clone();
    match plot {
        "magnitude" => Box::new(move |f| (f > 0.0).then(|| stage.response(f).norm())),
        "delay" => Box::new(move |f| (f > 0.0).then(|| delay_ms(&stage, f))),
        "phase" => Box::new(move |f| (f > 0.0).then(|| stage.response(f).arg().to_degrees())),
        _ => {
            let fs = stage.sample_rate();
            let n = (until.max(0.0) / 1000.0 * fs).ceil() as usize + 2;
            let samples = step_response(&stage, n);
            let times: Vec<f64> = (0..n).map(|k| k as f64 / fs * 1000.0).collect();
            Box::new(move |t| super::interpolate(&times, &samples, t))
        }
    }
}

/// The largest distance of the points of a copy's line from `curve`, a
/// point known to `half_x` device units across and `half_y` (data units)
/// up: from the nearest value the curve takes within `half_x` of it; `None` when the curve reaches none of them. A point on the left or
/// the right edge of the box, `edges`, is where the box cut the line, not
/// a point of it, and is passed over.
fn distance(
    points: &[(f64, f64)],
    curve: &dyn Fn(f64) -> Option<f64>,
    (scale, half_x, half_y): (&Scale, f64, f64),
    edges: (f64, f64),
) -> Option<f64> {
    let mut worst: Option<f64> = None;
    for (x, y) in points {
        let p = scale.position(*x);
        if (p - edges.0).abs() <= 2.0 * half_x || (p - edges.1).abs() <= 2.0 * half_x {
            continue;
        }
        // the curve across the half units each side, closely enough to
        // catch the bottom of a notch between them
        let values: Vec<f64> = (-4..=4)
            .map(|k| scale.value(p + f64::from(k) * half_x / 4.0))
            .filter_map(curve)
            .collect();
        if values.is_empty() {
            continue;
        }
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let d = ((low - y).max(y - high) - half_y).max(0.0);
        worst = Some(worst.map_or(d, |w: f64| w.max(d)));
    }
    worst
}

/// A delay printed on a copy: the text it stands in, its value and the
/// unit of its last digit.
fn printed_delays(m: &Metafile) -> Vec<(String, f64, f64)> {
    m.texts
        .iter()
        .filter_map(|t| {
            let text = t.text.trim();
            let (_, after) = text.rsplit_once("elay")?;
            let digits: String = after
                .trim_start_matches([':', ' '])
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let value: f64 = digits.parse().ok()?;
            let decimals = digits.split_once('.').map_or(0, |(_, d)| d.len());
            let unit = 10f64.powi(-i32::try_from(decimals).ok()?);
            Some((text.to_string(), value, unit))
        })
        .collect()
}

/// `OpenPID`'s delay for a printed text: the sum of a column, a named RPM
/// notch or the notch, or, for a delay printed without a name, the nearest
/// of the column's filters.
fn ours_for(text: &str, value: f64, parts: &[Part]) -> Option<f64> {
    let at1 = |p: &Part| delay_ms(&p.stage, 1.0);
    if text.contains("FILTERS") {
        return Some(parts.iter().map(|p| p.counted * at1(p)).sum());
    }
    let named = parts.iter().find(|p| {
        let hz = p.label.split(' ').rev().nth(1).unwrap_or_default();
        (text.contains("RPM notch")
            && p.label.starts_with("RPM")
            && text.contains(&format!("{hz}Hz")))
            || (text.contains("notch center") && p.label.starts_with("notch"))
    });
    if let Some(p) = named {
        return Some(at1(p));
    }
    parts
        .iter()
        .map(at1)
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
}

/// The copy's printed delays against `OpenPID`'s: the report and whether
/// every one agrees to its last digit.
fn hold_delays(m: &Metafile, parts: &[Part]) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut pass = true;
    for (text, value, unit) in printed_delays(m) {
        let Some(ours) = ours_for(&text, value, parts) else {
            continue;
        };
        let agree = (ours - value).abs() <= limit("printed delay") * unit * (1.0 + 1e-9);
        pass &= agree;
        out.push(format!("\"{text}\" against {ours:.6} ms"));
    }
    (out, pass)
}

/// The curves of a column of the manifest's set held against the copy.
fn computed(
    m: &Metafile,
    set: &FilterSet,
    (column, plot, check): (&str, &str, &str),
) -> Result<Outcome, String> {
    let parts = parts(set, column)?;
    let scales = Axes::of(m).map_err(|e| e.to_string())?;
    let xs: Vec<f64> = m
        .curves
        .iter()
        .filter(|c| c.points.len() > 2)
        .flat_map(|c| c.points.iter().map(|p| p.0))
        .collect();
    let half_x = 0.5 * unit_of(xs).unwrap_or(0.0);
    let half_y = 0.5 * resolution(m, &scales.y, true);
    let edges = (scales.frame.left, scales.frame.right);
    let drawn = super::groups(m, &scales, |_| true);
    let until = drawn
        .iter()
        .flat_map(|(_, p)| p.iter().map(|q| q.0))
        .fold(0.0, f64::max);
    let mut worst = 0.0f64;
    let mut found = Vec::new();
    for part in &parts {
        let f = curve(&part.stage, plot, until);
        let best = drawn
            .iter()
            .filter_map(|(colour, p)| {
                let d = distance(p, &f, (&scales.x, half_x, half_y), edges)?;
                Some((*colour, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or_else(|| format!("no line of the copy shares a range with {}", part.label))?;
        worst = worst.max(best.1);
        found.push(format!(
            "{} ↔ #{:06x}: {}",
            part.label,
            best.0,
            number(best.1)
        ));
    }
    let (delays, delays_agree) = hold_delays(m, &parts);
    let mut numbers = format!("largest {}; {}", number(worst), found.join("; "));
    if !delays.is_empty() {
        let _ = write!(numbers, "; printed delays at 1 Hz: {}", delays.join(", "));
    }
    Ok(Outcome::judged(
        format!(
            "OpenPID's filter simulator on the manifest's {column} filters at {} Hz",
            set.loop_rate_hz
        ),
        numbers,
        worst <= limit(check) && delays_agree,
    ))
}

/// The curves a results file gives for the copy, held against its lines.
fn given(m: &Metafile, src: &Source, copy: &Copy, check: &str) -> Option<Result<Outcome, String>> {
    let given = src.file.filters.iter().find(|f| f.copy == copy.name)?;
    Some((|| {
        let scales = Axes::of(m).map_err(|e| e.to_string())?;
        let drawn = super::groups(m, &scales, |_| true);
        let mut worst = 0.0f64;
        let mut parts = Vec::new();
        for curve in &given.curves {
            let best = drawn
                .iter()
                .filter_map(|(colour, p)| Some((*colour, differences(p, &curve.x, &curve.y)?.0)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .ok_or_else(|| {
                    format!("no curve of the copy shares a range with {}", curve.label)
                })?;
            worst = worst.max(best.1);
            parts.push(format!(
                "{} ↔ #{:06x}: {}",
                curve.label,
                best.0,
                number(best.1)
            ));
        }
        let held = Held {
            numbers: format!("largest {}; {}", number(worst), parts.join("; ")),
            pass: worst <= limit(check) && !given.curves.is_empty(),
        };
        Ok(Outcome::judged(
            format!("{} curves of the results file", given.curves.len()),
            held.numbers,
            held.pass,
        ))
    })())
}

pub(super) fn filters(copy: &Copy, m: &Metafile, src: &Source) -> Result<Outcome, String> {
    let Some((column, plot)) = column_and_plot(&copy.rest) else {
        let plot = tokens(&copy.rest).last().copied().unwrap_or_default();
        return Ok(not_compared(format!("the {plot} plot is not compared")));
    };
    let check = PLOTS
        .iter()
        .find(|(p, _)| *p == plot)
        .map_or("filter magnitude", |(_, c)| *c);
    if let Some(outcome) = given(m, src, copy, check) {
        return outcome;
    }
    let Some(set) = crate::capture::filter_set(src.round) else {
        return Ok(not_compared(
            "the round types no set of filters of its own, and the results file has no curves \
             for this copy (--ours; docs/calibration/PROTOCOL.md)",
        ));
    };
    computed(m, &set, (column, plot, check))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::VERIFY_FILTERS;

    #[test]
    fn the_set_of_round_17_through_openpids_simulator() {
        assert_eq!(
            column_and_plot("filters-verify-notch-phase"),
            Some(("notch", "phase"))
        );
        assert_eq!(column_and_plot("filters-verify-lowpass-chirp"), None);
        let lowpasses = parts(&VERIFY_FILTERS, "lowpass").unwrap();
        let notches = parts(&VERIFY_FILTERS, "notch").unwrap();
        assert_eq!(lowpasses.len(), 2);
        assert_eq!(notches.len(), 4);
        // the delays round 17 saw printed, to their last digit
        // (verification.md)
        for (part, printed) in lowpasses
            .iter()
            .chain(&notches)
            .zip([0.97553, 0.51632, 0.31769, 0.15785, 0.10413, 0.18858])
        {
            let ours = delay_ms(&part.stage, 1.0);
            assert!((ours - printed).abs() <= 0.5e-5, "{}: {ours}", part.label);
        }
        let column = |p: &[Part]| {
            p.iter()
                .map(|q| q.counted * delay_ms(&q.stage, 1.0))
                .sum::<f64>()
        };
        assert!((column(&lowpasses) - 1.4919).abs() <= 0.5e-4);
        assert!((column(&notches) - 2.5072).abs() <= 0.5e-4);
        // a step response starts at the first sample's output, at 0 ms
        let step = curve(&lowpasses[0].stage, "step", 10.0);
        assert!(step(0.0).is_some_and(|v| v > 0.1 && v < 0.2));
        assert!(step(50.0).is_none());
        assert!(lowpass("biquad-bessel", 100.0, 4000.0).is_err());
        assert!(lowpass("pt1", 3000.0, 4000.0).is_err());
        assert!(notch(330.0, 0.0, 4000.0).is_err());
    }

    #[test]
    fn printed_delays_are_read_and_named() {
        let mut m = Metafile::default();
        for text in [
            "LOWPASS FILTERS | Delay 1.4919ms",
            "RPM notch: 200Hz | delay: 0.15785ms",
            "notch center: 330Hz | delay: 0.18858ms",
            "pt2 cutoff: 210Hz",
        ] {
            m.texts.push(crate::emf::Text {
                text: text.into(),
                at: (0.0, 0.0),
                width: 0.0,
                align: 0,
            });
        }
        let read = printed_delays(&m);
        assert_eq!(read.len(), 3);
        assert!((read[0].1 - 1.4919).abs() < 1e-12 && (read[0].2 - 1e-4).abs() < 1e-15);
        let notches = parts(&VERIFY_FILTERS, "notch").unwrap();
        let rpm = ours_for(&read[1].0, read[1].1, &notches).unwrap();
        assert!((rpm - 0.15785).abs() < 0.5e-5, "{rpm}");
        let static_notch = ours_for(&read[2].0, read[2].1, &notches).unwrap();
        assert!((static_notch - 0.18858).abs() < 0.5e-5);
        // the notches agree with their two delays; the sum of the low-pass
        // column printed on them is the notches' sum, which it is not
        let (said, agree) = hold_delays(&m, &notches);
        assert_eq!(said.len(), 3);
        assert!(!agree);
        m.texts.remove(0);
        assert!(hold_delays(&m, &notches).1);
    }
}
