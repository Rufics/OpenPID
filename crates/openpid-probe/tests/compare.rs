//! `openpid-probe compare` end to end, on folders of copies written by the
//! small metafile writer of `support`: a copy drawn from `OpenPID`'s own
//! result passes, the same shifted fails, and a file the plan did not make
//! is listed as not compared.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]
#![allow(
    clippy::unreadable_literal,
    reason = "colours are written 0xRRGGBB, as a copy stores them"
)]

mod support;

use openpid_core::{Axis, ChannelId, GyroStage};
use openpid_probe::compare::{Report, Verdict, compare};
use openpid_probe::{capture, catalogue, cli, pack};
use std::path::{Path, PathBuf};
use support::{Plot, Writer, hot};

/// A new empty folder for one test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpid-probe-compare-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(dir: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(dir.join(format!("{name}.emf")), bytes).unwrap();
}

fn verdict(report: &Report, copy: &str) -> Verdict {
    report
        .rows
        .iter()
        .find(|r| r.copy == copy)
        .unwrap_or_else(|| panic!("no row {copy}: {report:?}"))
        .verdict
}

/// Whether the plan of `round` copies `name`.
fn planned(round: u32, name: &str) -> bool {
    let probes = catalogue::round(round).unwrap();
    capture::plan(round, &probes)
        .copies()
        .iter()
        .any(|(c, _)| c == name)
}

/// A plot of the main window: 0 … 20 s against ±500 deg/s.
fn viewer_plot() -> Plot {
    Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 20.0),
        y: (-500.0, 500.0),
    }
}

/// A copy of the roll plot of the main window drawing the log's gyro and
/// setpoint, the gyro moved up by `shift`.
fn viewer_copy(shift: f64) -> Vec<u8> {
    let probe = &catalogue::round(15).unwrap()[0];
    let session = pack::session_of(probe).unwrap();
    let line = |id: ChannelId, by: f64| -> Vec<(f64, f64)> {
        let c = session.channel(&id).unwrap();
        (0..session.len())
            .step_by(7)
            .map(|i| (session.time_s(i).unwrap(), c.data.get(i).unwrap() + by))
            .map(|(t, v)| (t, v.clamp(-500.0, 500.0)))
            .collect()
    };
    let mut w = Writer::new();
    let p = viewer_plot();
    let xs: Vec<f64> = (0..10).map(|i| f64::from(2 * i)).collect();
    p.axes(&mut w, &xs, &[-250.0, 0.0, 250.0]);
    let gyro = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Filtered,
    };
    p.curve(&mut w, 0x000000, 0, &line(gyro, shift));
    p.curve(
        &mut w,
        0xff0033,
        0,
        &line(ChannelId::Setpoint { axis: Axis::Roll }, 0.0),
    );
    w.finish()
}

#[test]
fn the_lines_of_the_main_window_against_the_decoded_log() {
    assert!(planned(15, "r15-log-open-roll"));
    let good = folder("viewer-good");
    save(&good, "r15-log-open-roll", &viewer_copy(0.0));
    save(&good, "r15-not-a-copy", &viewer_copy(0.0));
    // a smoothing level OpenPID does not have is not compared either
    let report = compare(15, &good, None).unwrap();
    assert_eq!(
        verdict(&report, "r15-log-open-roll"),
        Verdict::Pass,
        "{report:?}"
    );
    assert_eq!(verdict(&report, "r15-not-a-copy"), Verdict::NotCompared);
    assert!(report.passed());
    assert!(report.missing.contains(&"r15-log-open-pitch".to_string()));
    let text = report.markdown();
    assert!(text.contains("| `r15-not-a-copy` | unknown | not a copy of the plan of round 15"));
    assert!(text.contains("Result: **PASS**"));
    assert!(text.contains("| line | 0.005 |"));

    let bad = folder("viewer-bad");
    save(&bad, "r15-log-open-roll", &viewer_copy(50.0));
    save(&bad, "r15-log-open-pitch", b"not a metafile at all");
    let report = compare(15, &bad, None).unwrap();
    assert_eq!(verdict(&report, "r15-log-open-roll"), Verdict::Fail);
    assert_eq!(verdict(&report, "r15-log-open-pitch"), Verdict::Error);
    assert!(!report.passed());
    assert!(report.markdown().contains("Result: **FAIL**"));

    // the command: 0 for the good folder, 1 (an error) for the bad one
    let mut out = Vec::new();
    let args = |dir: &Path| {
        vec![
            "compare".to_string(),
            "15".into(),
            dir.display().to_string(),
        ]
    };
    cli::run(&args(&good), &mut out).unwrap();
    assert!(String::from_utf8_lossy(&out).contains("# Round 15"));
    let written = good.join("report.md");
    let mut with_out = args(&good);
    with_out.extend(["--out".to_string(), written.display().to_string()]);
    cli::run(&with_out, &mut out).unwrap();
    assert!(std::fs::read_to_string(&written).unwrap().contains("PASS"));
    let err = cli::run(&args(&bad), &mut Vec::new()).unwrap_err();
    assert!(err.contains("not every copy passed"), "{err}");
    for dir in [good, bad] {
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn the_command_line_refuses_what_it_cannot_use() {
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        cli::run(&args, &mut Vec::new()).unwrap_err()
    };
    assert!(run(&["compare"]).contains("needs a round and a folder"));
    assert!(run(&["compare", "x", "dir"]).contains("must be a number"));
    assert!(run(&["compare", "15", "dir", "--bogus"]).contains("unknown option"));
    assert!(run(&["compare", "15", "dir", "--ours"]).contains("--ours needs a file"));
    assert!(run(&["compare", "15", "dir", "--out"]).contains("--out needs a file"));
    assert!(run(&["compare", "99", "dir"]).contains("there is no round 99"));
    let dir = folder("cli");
    let d = dir.display().to_string();
    assert!(run(&["compare", "15", "/nonexistent/openpid"]).contains("cannot read"));
    assert!(
        run(&["compare", "15", &d, "--ours", "/nonexistent/ours.json"]).contains("cannot read")
    );
    let junk = dir.join("junk.json");
    std::fs::write(&junk, "[1, 2").unwrap();
    assert!(
        run(&["compare", "15", &d, "--ours", &junk.display().to_string()])
            .contains("not a results file")
    );
    let mut out = Vec::new();
    cli::run(&["--help".to_string()], &mut out).unwrap();
    assert!(String::from_utf8_lossy(&out).contains("openpid-probe compare <round>"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Writes `json` as a results file in `dir`.
fn results(dir: &Path, json: &serde_json::Value) -> PathBuf {
    let path = dir.join("ours.json");
    std::fs::write(&path, json.to_string()).unwrap();
    path
}

/// A curve of `f` from `from` to `to`.
fn sampled(f: impl Fn(f64) -> f64, from: f64, to: f64, step: f64) -> (Vec<f64>, Vec<f64>) {
    let n = ((to - from) / step).round() as usize;
    (0..=n)
        .map(|i| from + step * i as f64)
        .map(|x| (x, f(x)))
        .unzip()
}

fn spectrum_copy(f: &dyn Fn(f64) -> f64) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 100.0),
        y: (-50.0, 20.0),
    };
    p.axes(
        &mut w,
        &[0.0, 20.0, 40.0, 60.0, 80.0, 100.0],
        &[-40.0, -20.0, 0.0, 20.0],
    );
    let (x, y) = sampled(f, 0.0, 100.0, 0.05);
    let points: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
    p.curve(&mut w, 0xe60000, 0, &points);
    // the dotted curve of the unfiltered gyro is not compared, whether its
    // pen is dotted or the program dashes it with the solid one's pen
    p.curve(
        &mut w,
        0xe60000,
        2,
        &[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)],
    );
    let dotted: Vec<(f64, f64)> = points.iter().map(|(x, y)| (*x, y + 5.0)).collect();
    p.dashes(&mut w, 0xe60000, &dotted);
    w.finish()
}

#[test]
fn a_spectrum_against_the_results_file_and_against_the_log() {
    let name = "r2-c4-short-2k-spectrum-medium-roll-sub";
    let full = "r2-c4-short-2k-spectrum-medium-roll-full";
    assert!(planned(2, name) && planned(2, full));
    let curve = |f: f64| -40.0 + 10.0 * (f / 7.0).sin();
    let (freq, power_db) = sampled(curve, 0.0, 100.0, 0.25);
    let json = serde_json::json!({"probes": [{"id": "c4-short-2k", "axes": [{"axis": "roll",
        "spectra": [{"channel": "gyro", "segment_s": 1.0, "freq": freq, "power_db": power_db}]}]}]});
    let dir = folder("spectrum");
    let ours = results(&dir, &json);
    save(&dir, name, &spectrum_copy(&curve));
    // the full range is not in the file: OpenPID's is computed from the log
    save(&dir, full, &spectrum_copy(&curve));
    let report = compare(2, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Pass, "{report:?}");
    assert!(matches!(
        verdict(&report, full),
        Verdict::Pass | Verdict::Fail | Verdict::Decided("D-012")
    ));
    assert!(report.source.contains("ours.json"));
    let row = report.rows.iter().find(|r| r.copy == name).unwrap();
    // a unit of the copy is an eighth of a device unit: 70 dB over 500
    assert!(
        row.against.contains("the copy reads to 0.0175 dB"),
        "{row:?}"
    );
    // a level other than the one the window comes up with is not compared
    let low = "r2-c4-short-2k-spectrum-low-roll-sub";
    assert!(planned(2, low));
    save(&dir, low, &spectrum_copy(&curve));
    // a curve 0.2 dB off is 11 units of the copy off: it fails on either
    // stretch
    save(&dir, name, &spectrum_copy(&|f| curve(f) + 0.2));
    let report = compare(2, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Fail);
    assert_eq!(verdict(&report, low), Verdict::NotCompared);
    let row = report.rows.iter().find(|r| r.copy == name).unwrap();
    assert!(
        row.numbers.contains("on the reference tool's stretch"),
        "{row:?}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// A line of a copy: its colour, its pen style (0 solid, 2 dotted) and its
/// levels against frequency.
type Line<'a> = (u32, u32, &'a dyn Fn(f64) -> f64);

/// A copy of a spectrum plot below 100 Hz with one line per curve.
fn lines_copy(curves: &[Line]) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 100.0),
        y: (-90.0, 20.0),
    };
    p.axes(
        &mut w,
        &[0.0, 20.0, 40.0, 60.0, 80.0, 100.0],
        &[-80.0, -40.0, 0.0],
    );
    for (colour, style, f) in curves {
        let (x, y) = sampled(f, 0.0, 100.0, 0.05);
        let points: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
        p.curve(&mut w, *colour, *style, &points);
    }
    w.finish()
}

#[test]
fn a_channel_of_the_spectrum_window_against_its_counterpart() {
    let dterm = "r17-i1-chan-2k-spectrum-dterm-roll-sub";
    let motors = "r17-i1-chan-2k-spectrum-motors-all";
    let pterm = "r17-i1-chan-2k-spectrum-pterm-roll-sub";
    assert!([dterm, motors, pterm].iter().all(|n| planned(17, n)));
    // the motors draw one plot of their own
    assert!(!planned(17, "r17-i1-chan-2k-spectrum-motors-roll-sub"));
    // the entry OpenPID has no counterpart of is only photographed
    assert!(!planned(
        17,
        "r17-i1-chan-2k-spectrum-dterm-prefilt-roll-sub"
    ));
    let level = |k: f64| move |f: f64| -40.0 + k + 10.0 * (f / 7.0).sin();
    let spectrum = |channel: &str, k: f64, segment_s: f64| {
        let (freq, power_db) = sampled(level(k), 0.0, 100.0, 0.25);
        serde_json::json!({"channel": channel, "segment_s": segment_s, "freq": freq,
            "power_db": power_db})
    };
    let mut spectra = vec![spectrum("dterm", 0.0, 1.0)];
    spectra.extend((1..=4).map(|m| spectrum(&format!("motor{m}"), 5.0 * f64::from(m), 0.25)));
    let json = serde_json::json!({"probes": [{"id": "i1-chan-2k", "axes": [{"axis": "roll",
        "spectra": spectra}]}]});
    let dir = folder("channels");
    let ours = results(&dir, &json);
    // a channel alone may be drawn dotted: every line is held
    save(&dir, dterm, &lines_copy(&[(0xe60000, 2, &level(0.0))]));
    // the motors in the reference tool's units: the logged number over
    // 20, which for outputs of 158 … 2047 is about 1889 / 2000 of OpenPID's
    // percent, half a decibel below it (D-008)
    let shift = 20.0 * (1889.0f64 / 2000.0).log10();
    let motor = |k: f64| move |f: f64| level(k)(f) + shift;
    let (m1, m3) = (motor(5.0), motor(15.0));
    save(
        &dir,
        motors,
        &lines_copy(&[(0xe60000, 0, &m1), (0x0000e6, 0, &m3)]),
    );
    // the P term is not in the file: OpenPID's is computed from the log
    save(&dir, pterm, &lines_copy(&[(0xe60000, 0, &level(0.0))]));
    let report = compare(17, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, dterm), Verdict::Pass, "{report:?}");
    assert_eq!(
        verdict(&report, motors),
        Verdict::Decided("D-008"),
        "{report:?}"
    );
    let row = |name: &str| report.rows.iter().find(|r| r.copy == name).unwrap().clone();
    assert!(
        row(motors).numbers.contains("#0000e6 motor 3"),
        "{:?}",
        row(motors)
    );
    assert!(row(motors).numbers.contains("#e60000 motor 1"));
    assert!(
        row(motors).against.contains("units (-0.49"),
        "{:?}",
        row(motors)
    );
    assert!(row(dterm).against.contains("the D term on roll"));
    assert!(matches!(
        verdict(&report, pterm),
        Verdict::Fail | Verdict::Decided("D-012")
    ));
    assert!(row(pterm).against.contains("the P term"));
    assert!(report.markdown().contains("**D-008** (1 copies)"));
    save(&dir, dterm, &lines_copy(&[(0xe60000, 2, &level(0.2))]));
    // the motors in OpenPID's percent are not the reference tool's
    let (p1, p3) = (level(5.0), level(15.0));
    save(
        &dir,
        motors,
        &lines_copy(&[(0xe60000, 0, &p1), (0x0000e6, 0, &p3)]),
    );
    let report = compare(17, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, dterm), Verdict::Fail);
    assert_eq!(verdict(&report, motors), Verdict::Fail);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A plot of the step response window holding a run of each of `runs`,
/// in the colours the window gives them, the newest last.
fn response_runs(runs: &[&dyn Fn(f64) -> f64]) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 500.0),
        y: (0.0, 1.5),
    };
    p.axes(
        &mut w,
        &[0.0, 100.0, 200.0, 300.0, 400.0, 500.0],
        &[0.0, 0.5, 1.0, 1.5],
    );
    for (f, colour) in runs.iter().zip([0xe60000, 0xe67300, 0xe6e600]) {
        // an earlier run's curve has more points than the newest's
        let (x, y) = sampled(f, 0.0, 500.0, 0.25);
        let points: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
        p.curve(&mut w, colour, 0, &points);
    }
    w.finish()
}

fn response_copy(f: &dyn Fn(f64) -> f64) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 500.0),
        y: (0.0, 1.5),
    };
    p.axes(
        &mut w,
        &[0.0, 100.0, 200.0, 300.0, 400.0, 500.0],
        &[0.0, 0.5, 1.0, 1.5],
    );
    let (x, y) = sampled(f, 0.0, 500.0, 0.5);
    let points: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
    p.curve(&mut w, 0xe60000, 0, &points);
    w.finish()
}

fn peak_copy(peak: f64) -> Vec<u8> {
    peaks_copy(&[peak])
}

/// A Peak plot with a bar for each run of `peaks`, the newest last.
fn peaks_copy(peaks: &[f64]) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 600.0, 520.0),
        x: (0.0, 1.0),
        y: (0.8, 1.5),
    };
    p.axes(&mut w, &[], &[0.8, 1.0, 1.2, 1.4]);
    w.text("Test", (300, 600), 15);
    w.modify_transform([0.125, 0.0, 0.0, 0.125, 0.0, 0.0], 2);
    let at = |v: f64| (p.page((0.5, v)).1 * 8.0).round() as i16;
    for (k, (peak, colour)) in peaks.iter().zip([0x0000e6, 0x0073e6, 0x00e6e6]).enumerate() {
        let x = 1800 + 1000 * k as i16;
        w.pen(colour, 16, 0);
        w.polyline(&[(x, at(*peak)), (x, at(peak - 0.05))], false);
        w.polyline(&[(x, at(*peak)), (x, at(peak + 0.05))], false);
        w.polyline(
            &[(x - 50, at(peak + 0.05)), (x + 50, at(peak + 0.05))],
            false,
        );
    }
    w.finish()
}

#[test]
fn a_step_response_and_its_peak_against_the_results_file() {
    let names = [
        "r7-y1-delays-2k-step-roll-response",
        "r7-y1-delays-2k-step-roll-peak",
        "r7-y1-delays-2k-step-params-roll-risetime",
        "r7-y4-damping-2k-step-smoothing-high-roll-response",
    ];
    assert!(names.iter().all(|n| planned(7, n)));
    let response = |t: f64| 1.0 - (-t / 20.0).exp();
    let (time_ms, mean) = sampled(response, 0.0, 500.0, 0.5);
    let json = serde_json::json!({"probes": [{"id": "y1-delays-2k", "step": [{"axis": "roll",
        "window_s": 2.0, "time_ms": time_ms, "mean": mean, "peak": 1.05}]}]});
    let dir = folder("step");
    let ours = results(&dir, &json);
    save(&dir, names[0], &response_copy(&response));
    save(&dir, names[1], &peak_copy(1.05));
    save(&dir, names[2], &response_copy(&response));
    save(&dir, names[3], &response_copy(&response));
    // the pitch axis is not in the file: OpenPID's is computed from the log
    let computed = "r7-y1-delays-2k-step-pitch-response";
    save(&dir, computed, &response_copy(&response));
    let report = compare(7, &dir, Some(&ours)).unwrap();
    assert!(matches!(
        verdict(&report, computed),
        Verdict::Pass | Verdict::Fail
    ));
    assert_eq!(verdict(&report, names[0]), Verdict::Pass, "{report:?}");
    assert_eq!(verdict(&report, names[1]), Verdict::Pass, "{report:?}");
    assert_eq!(verdict(&report, names[2]), Verdict::NotCompared);
    assert_eq!(verdict(&report, names[3]), Verdict::NotCompared);
    save(&dir, names[0], &response_copy(&|t| response(t) * 1.03));
    save(&dir, names[1], &peak_copy(1.1));
    let report = compare(7, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, names[0]), Verdict::Fail);
    assert_eq!(verdict(&report, names[1]), Verdict::Fail);
    // the window keeps the runs before the log's own: the newest is held
    let other = |t: f64| response(t) * 1.03;
    save(&dir, names[0], &response_runs(&[&other, &response]));
    save(&dir, names[1], &peaks_copy(&[1.2, 1.05]));
    let report = compare(7, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, names[0]), Verdict::Pass, "{report:?}");
    assert_eq!(verdict(&report, names[1]), Verdict::Pass, "{report:?}");
    let row = report.rows.iter().find(|r| r.copy == names[0]).unwrap();
    assert!(
        row.against
            .contains("the newest of the copy's 2 runs (#e67300)"),
        "{row:?}"
    );
    save(&dir, names[0], &response_runs(&[&response, &other]));
    save(&dir, names[1], &peaks_copy(&[1.05, 1.2]));
    let report = compare(7, &dir, Some(&ours)).unwrap();
    assert_eq!(verdict(&report, names[0]), Verdict::Fail);
    assert_eq!(verdict(&report, names[1]), Verdict::Fail);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A map of 20 columns and 10 rows whose cell `(i, j)` is colour step
/// `(7i + 13j) mod 256`, the value `low + span · (step + ½) / 256`.
struct MapSpec {
    x: (f64, f64),
    y: (f64, f64),
    low: f64,
    high: f64,
}

impl MapSpec {
    fn step(i: usize, j: usize) -> usize {
        (7 * i + 13 * j) % 256
    }

    fn value(&self, i: usize, j: usize) -> f64 {
        self.low + (self.high - self.low) * (Self::step(i, j) as f64 + 0.5) / 256.0
    }

    /// The map as a results file holds it: amplitudes; the values are
    /// decibels when the lower end is below zero.
    fn ours(&self, shift: f64) -> serde_json::Value {
        let dx = (self.x.1 - self.x.0) / 20.0;
        let dy = (self.y.1 - self.y.0) / 10.0;
        let xs: Vec<f64> = (0..20)
            .map(|i| self.x.0 + dx * (f64::from(i) + 0.5))
            .collect();
        let ys: Vec<f64> = (0..10)
            .map(|j| self.y.0 + dy * (f64::from(j) + 0.5))
            .collect();
        let z: Vec<f64> = (0..10)
            .flat_map(|j| (0..20).map(move |i| (i, j)))
            .map(|(i, j)| {
                let v = self.value(i, j) + shift;
                if self.low < 0.0 {
                    10f64.powf(v / 20.0)
                } else {
                    v
                }
            })
            .collect();
        serde_json::json!({"x": xs, "y": ys, "z": z})
    }

    /// The copy: the map over a box, its colour bar beside it (upright) or
    /// above it, or none, and the numbers it prints.
    fn copy(&self, bar: Option<bool>, printed: &[String]) -> Vec<u8> {
        self.drawn(bar, printed, false)
    }

    /// The copy with the map and its colour bar (beside it) pictures of 16
    /// bits, as the reference tool copies some of its maps: each colour's
    /// channels cut to five bits.
    fn copy16(&self) -> Vec<u8> {
        self.drawn(Some(true), &[], true)
    }

    fn drawn(&self, bar: Option<bool>, printed: &[String], sixteen: bool) -> Vec<u8> {
        let mut w = Writer::new();
        let p = Plot {
            frame: (100.0, 120.0, 2100.0, 620.0),
            x: self.x,
            y: self.y,
        };
        let ticks = |(a, b): (f64, f64)| -> Vec<f64> {
            (0..=4).map(|k| a + (b - a) * f64::from(k) / 4.0).collect()
        };
        p.axes(&mut w, &ticks(self.x), &ticks(self.y));
        let palette: Vec<u32> = (0..256).map(hot).collect();
        // ten pixels a cell, the rows from the top: the highest row first
        let rows: Vec<Vec<u8>> = (0..100)
            .map(|r| {
                let j = 9 - r / 10;
                (0..200).map(|c| Self::step(c / 10, j) as u8).collect()
            })
            .collect();
        if sixteen {
            let colours: Vec<Vec<u32>> = rows
                .iter()
                .map(|r| r.iter().map(|i| palette[usize::from(*i)]).collect())
                .collect();
            w.picture16((100, 120), (2000, 500), &colours);
        } else {
            w.picture8((100, 120), (2000, 500), &palette, &rows, false);
        }
        let label = |v: f64| format!("{v}");
        match bar {
            Some(true) => {
                let rows: Vec<Vec<u8>> = (0..256).rev().map(|s| vec![s as u8; 4]).collect();
                if sixteen {
                    // the bar of such a copy is of 16 bits too
                    let colours: Vec<Vec<u32>> = rows
                        .iter()
                        .map(|r| r.iter().map(|i| palette[usize::from(*i)]).collect())
                        .collect();
                    w.picture16((2150, 120), (30, 500), &colours);
                } else {
                    w.picture8((2150, 120), (30, 500), &palette, &rows, true);
                }
                for (v, py) in [(self.low, 620.0), (self.high, 120.0)] {
                    w.text(&label(v), (2190, (py + 9.75) as i32), 15);
                }
            }
            Some(false) => {
                let rows = vec![(0..256).map(|s| s as u8).collect::<Vec<u8>>(); 4];
                w.picture8((100, 40), (2000, 30), &palette, &rows, false);
                for (v, px) in [(self.low, 100.0), (self.high, 2100.0)] {
                    let n = label(v).len() as i32;
                    w.text(&label(v), (px as i32 - 15 * n / 2, 30), 15);
                }
            }
            None => {}
        }
        for (k, text) in printed.iter().enumerate() {
            w.text(text, (1500, 700 + 30 * k as i32), 15);
        }
        w.finish()
    }

    fn mean_and_peak(&self) -> (f64, f64) {
        let values: Vec<f64> = (0..10)
            .flat_map(|j| (0..20).map(move |i| (i, j)))
            .map(|(i, j)| self.value(i, j))
            .collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        (mean, values.iter().copied().fold(f64::MIN, f64::max))
    }
}

#[test]
fn maps_against_the_results_file_cell_by_cell() {
    let time = "r11-g1-levels-2k-time-prefilt-roll";
    let smoothed = "r11-g1-levels-2k-time-prefilt-both-high-roll";
    assert!(planned(11, time) && planned(11, smoothed));
    let t = MapSpec {
        x: (1.0, 21.0),
        y: (0.0, 1000.0),
        low: -30.0,
        high: 10.0,
    };
    let dir = folder("time-map");
    let with = |map: serde_json::Value, probe: &str, channel: &str, window: &str, axis: &str| {
        let mut m = map;
        m["probe"] = probe.into();
        m["channel"] = channel.into();
        m["window"] = window.into();
        m["axis"] = axis.into();
        m
    };
    let ours = |shift: f64| serde_json::json!({"maps": [with(t.ours(shift), "g1-levels-2k", "gyro_unfiltered", "time", "roll")]});
    save(&dir, time, &t.copy(Some(true), &[]));
    save(&dir, smoothed, &t.copy(Some(true), &[]));
    // the pitch axis is not in the file: OpenPID's map is computed from the log
    let computed = "r11-g1-levels-2k-time-prefilt-pitch";
    save(&dir, computed, &t.copy(Some(true), &[]));
    let path = results(&dir, &ours(0.0));
    let report = compare(11, &dir, Some(&path)).unwrap();
    assert!(
        matches!(
            verdict(&report, computed),
            Verdict::Pass | Verdict::Fail | Verdict::Decided("D-012")
        ),
        "{:?}",
        report.rows.iter().find(|r| r.copy == computed)
    );
    assert_eq!(verdict(&report, time), Verdict::Pass, "{report:?}");
    assert_eq!(verdict(&report, smoothed), Verdict::NotCompared);
    let path = results(&dir, &ours(3.0));
    let report = compare(11, &dir, Some(&path)).unwrap();
    assert_eq!(verdict(&report, time), Verdict::Fail);
    std::fs::remove_dir_all(dir).unwrap();

    // the throttle window draws one bar, above its roll plot
    let roll = "r10-e1-len23-2k-throttle-roll-gyro";
    let pitch = "r10-e1-len23-2k-throttle-pitch-gyro";
    assert!(planned(10, roll) && planned(10, pitch));
    let th = MapSpec {
        x: (0.0, 100.0),
        y: (0.0, 1000.0),
        low: 0.0,
        high: 0.5,
    };
    let (mean, peak) = th.mean_and_peak();
    let dir = folder("throttle-map");
    let printed = [format!("mean={mean:.4}"), format!("peak={peak:.4}")];
    save(&dir, roll, &th.copy(Some(false), &printed));
    save(&dir, pitch, &th.copy(None, &[]));
    let maps = |shift: f64| {
        serde_json::json!({"maps": [
            with(th.ours(shift), "e1-len23-2k", "gyro", "throttle", "roll"),
            with(th.ours(shift), "e1-len23-2k", "gyro", "throttle", "pitch"),
        ]})
    };
    let yaw = "r10-e1-len23-2k-throttle-yaw-gyro";
    save(&dir, yaw, &th.copy(None, &[]));
    let path = results(&dir, &maps(0.0));
    let report = compare(10, &dir, Some(&path)).unwrap();
    assert!(matches!(
        verdict(&report, yaw),
        Verdict::Pass | Verdict::Fail | Verdict::Decided("D-012")
    ));
    let row = report.rows.iter().find(|r| r.copy == roll).unwrap();
    assert!(
        row.numbers
            .contains("pixels within 1 colour step (whole columns, whole rows)"),
        "{row:?}"
    );
    // the printed numbers agree to their last digit
    assert!(
        row.numbers.contains(&format!("mean copy {mean:.4}")),
        "{row:?}"
    );
    std::fs::remove_file(dir.join(format!("{yaw}.emf"))).unwrap();
    assert_eq!(verdict(&report, roll), Verdict::Pass, "{report:?}");
    assert_eq!(verdict(&report, pitch), Verdict::Pass, "{report:?}");
    let path = results(&dir, &maps(0.05));
    let report = compare(10, &dir, Some(&path)).unwrap();
    assert_eq!(verdict(&report, roll), Verdict::Fail);
    assert_eq!(verdict(&report, pitch), Verdict::Fail);
    // a picture of 16 bits is read cell by cell, to the three colour steps
    // a level of its five-bit channels spans
    save(&dir, pitch, &th.copy16());
    let report = compare(10, &dir, Some(&results(&dir, &maps(0.0)))).unwrap();
    assert_eq!(verdict(&report, pitch), Verdict::Pass, "{report:?}");
    let row = report.rows.iter().find(|r| r.copy == pitch).unwrap();
    assert!(
        row.numbers.contains("of 200 cells within 3 colour steps"),
        "{row:?}"
    );
    assert!(row.against.contains("a picture of 5 bits a colour channel"));
    let report = compare(10, &dir, Some(&results(&dir, &maps(0.02)))).unwrap();
    assert_eq!(verdict(&report, pitch), Verdict::Fail);
    // printed numbers off by more than their last digit fail
    let off = [
        format!("mean={:.4}", mean + 0.0003),
        format!("peak={peak:.4}"),
    ];
    save(&dir, roll, &th.copy(Some(false), &off));
    let report = compare(10, &dir, Some(&results(&dir, &maps(0.0)))).unwrap();
    assert_eq!(verdict(&report, roll), Verdict::Fail);
    // without the roll plot's copy the pitch plot has no colour scale
    save(&dir, pitch, &th.copy(None, &[]));
    std::fs::remove_file(dir.join(format!("{roll}.emf"))).unwrap();
    let report = compare(10, &dir, Some(&path)).unwrap();
    assert_eq!(verdict(&report, pitch), Verdict::Error);
    std::fs::remove_dir_all(dir).unwrap();
}

fn filter_copy(shift: f64) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 1000.0),
        y: (0.0, 1.1),
    };
    p.axes(
        &mut w,
        &[0.0, 250.0, 500.0, 750.0, 1000.0],
        &[0.0, 0.5, 1.0],
    );
    for (colour, cut) in [(0x00cfcf, 250.0), (0x67cf67, 500.0)] {
        let (x, y) = sampled(
            |f| 1.0 / (1.0 + (f / cut).powi(2)).sqrt() + shift,
            1.0,
            1000.0,
            1.0,
        );
        let points: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
        p.curve(&mut w, colour, 0, &points);
    }
    w.finish()
}

#[test]
fn filter_curves_against_the_results_file() {
    let name = "r13-filters-open-lowpass-magnitude";
    let other = "r13-filters-open-lowpass-delay";
    assert!(planned(13, name) && planned(13, other));
    let curves: Vec<serde_json::Value> = [250.0, 500.0]
        .iter()
        .map(|cut: &f64| {
            let (x, y) = sampled(|f| 1.0 / (1.0 + (f / cut).powi(2)).sqrt(), 1.0, 1000.0, 4.0);
            serde_json::json!({"label": format!("pt1 {cut} Hz"), "x": x, "y": y})
        })
        .collect();
    let dir = folder("filters");
    let path = results(
        &dir,
        &serde_json::json!({"filters": [{"copy": name, "curves": curves}]}),
    );
    save(&dir, name, &filter_copy(0.0));
    save(&dir, other, &filter_copy(0.0));
    let report = compare(13, &dir, Some(&path)).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Pass, "{report:?}");
    assert_eq!(verdict(&report, other), Verdict::NotCompared);
    save(&dir, name, &filter_copy(0.01));
    let report = compare(13, &dir, Some(&path)).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Fail);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A copy of a plot of the filter simulator's notch column of round 17:
/// the magnitude of each notch of the manifest's set on 0 … 1000 Hz, as
/// `OpenPID`'s simulator computes it plus `shift`, and the delay it prints.
fn notch_copy(shift: f64, printed: &str) -> Vec<u8> {
    use openpid_sim::{Biquad, Filter};
    let set = capture::VERIFY_FILTERS;
    let fs = f64::from(set.loop_rate_hz);
    let mut notches: Vec<Biquad> = (1..=set.rpm.harmonics)
        .map(|k| Biquad::notch_q(f64::from(k * set.rpm.hz), set.rpm.q, fs))
        .collect();
    notches.push(Biquad::notch_q(f64::from(set.notch.hz), set.notch.q, fs));
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, 1000.0),
        y: (0.0, 1.1),
    };
    p.axes(
        &mut w,
        &[0.0, 250.0, 500.0, 750.0, 1000.0],
        &[0.0, 0.5, 1.0],
    );
    for (notch, colour) in notches.iter().zip([0xe60000, 0xe67300, 0xe6e600, 0x000000]) {
        let (x, y) = sampled(|f| notch.response(f).norm() + shift, 5.0, 995.0, 2.5);
        let points: Vec<(f64, f64)> = x.into_iter().zip(y).collect();
        p.curve(&mut w, colour, 0, &points);
    }
    w.text(printed, (1000, 10), 15);
    w.finish()
}

#[test]
fn the_filters_of_the_manifest_through_openpids_simulator() {
    let name = "r17-filters-verify-notch-magnitude";
    assert!(planned(17, name));
    let dir = folder("filters-computed");
    save(
        &dir,
        name,
        &notch_copy(0.0, "NOTCH FILTERS | Delay 2.5072ms"),
    );
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Pass, "{report:?}");
    let row = report.rows.iter().find(|r| r.copy == name).unwrap();
    assert!(
        row.against
            .contains("the manifest's notch filters at 4000 Hz")
    );
    assert!(
        row.numbers.contains("RPM notch 200 Hz ↔ #e67300"),
        "{row:?}"
    );
    assert!(
        row.numbers
            .contains("\"NOTCH FILTERS | Delay 2.5072ms\" against 2.507248 ms")
    );
    // a curve off, or a delay off in its last digit, fails
    save(
        &dir,
        name,
        &notch_copy(0.003, "NOTCH FILTERS | Delay 2.5072ms"),
    );
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Fail);
    save(
        &dir,
        name,
        &notch_copy(0.0, "NOTCH FILTERS | Delay 2.5074ms"),
    );
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(verdict(&report, name), Verdict::Fail);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A copy of a plot of the main window for a log of round 17: `lines` are
/// the channels drawn, each by its colour and its values.
fn lines_of(span: (f64, f64), seconds: f64, lines: &[(u32, Vec<(f64, f64)>)]) -> Vec<u8> {
    let mut w = Writer::new();
    let p = Plot {
        frame: (100.0, 20.0, 2100.0, 520.0),
        x: (0.0, seconds),
        y: span,
    };
    let xs: Vec<f64> = (0..=seconds as u32).step_by(2).map(f64::from).collect();
    let ys: Vec<f64> = (0..=4)
        .map(|k| span.0 + (span.1 - span.0) * f64::from(k) / 4.0)
        .collect();
    p.axes(&mut w, &xs, &ys);
    for (colour, points) in lines {
        let clipped: Vec<(f64, f64)> = points
            .iter()
            .map(|(t, v)| (*t, v.clamp(span.0, span.1)))
            .collect();
        p.curve(&mut w, *colour, 0, &clipped);
    }
    w.finish()
}

/// The values of channel `id` of a decoded log, every `step`-th sample,
/// passed through `f` (the whole series at once).
fn channel_line(
    session: &openpid_core::FlightSession,
    id: &ChannelId,
    f: impl Fn(&[f64]) -> Vec<f64>,
) -> Vec<(f64, f64)> {
    let c = session.channel(id).unwrap();
    let values = f(c.data.values());
    (0..session.len())
        .step_by(3)
        .map(|i| (session.time_s(i).unwrap(), values[i]))
        .collect()
}

#[test]
fn the_motors_and_the_line_smoothing_of_the_main_window() {
    let probes = catalogue::round(17).unwrap();
    let probe = |id: &str| probes.iter().find(|p| p.id == id).unwrap();
    // the motors as the reference tool draws them: the logged number over
    // 20, where OpenPID keeps its percent (D-008)
    let throttle = "r17-e6-view-2k-log-open-throttle";
    assert!(planned(17, throttle));
    let view = probe("e6-view-2k");
    let session = pack::session_of(view).unwrap();
    let (low, high) = (
        session.settings.motor.output_low.unwrap(),
        session.settings.motor.output_high.unwrap(),
    );
    let lines = |logged: bool| -> Vec<(u32, Vec<(f64, f64)>)> {
        let mut out = vec![(
            0x000000,
            channel_line(&session, &ChannelId::Throttle, <[f64]>::to_vec),
        )];
        for idx in 0..4 {
            let motor = channel_line(&session, &ChannelId::Motor { idx }, |v| {
                v.iter()
                    .map(|p| {
                        if logged {
                            (low + p * (high - low) / 100.0) / 20.0
                        } else {
                            *p
                        }
                    })
                    .collect()
            });
            out.push((
                [0xe60000, 0xff9900, 0x1a66cc, 0x1affcc][usize::from(idx)],
                motor,
            ));
        }
        out
    };
    let seconds = view.flight.duration_s;
    let dir = folder("viewer-motors");
    save(
        &dir,
        throttle,
        &lines_of((0.0, 100.0), seconds, &lines(true)),
    );
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(
        verdict(&report, throttle),
        Verdict::Decided("D-008"),
        "{report:?}"
    );
    save(
        &dir,
        throttle,
        &lines_of((0.0, 100.0), seconds, &lines(false)),
    );
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(verdict(&report, throttle), Verdict::Pass, "{report:?}");
    std::fs::remove_dir_all(dir).unwrap();

    // the line smoothing at "med": 21 samples of the log, as observed
    let smooth = "r17-x1-samp-2k-log-smooth-med-roll";
    assert!(planned(17, smooth));
    let samples = probe("x1-samp-2k");
    let session = pack::session_of(samples).unwrap();
    let gyro = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Filtered,
    };
    let drawn = |span: usize| {
        vec![(
            0x000000,
            channel_line(&session, &gyro, |v| {
                openpid_probe::smoothing::smoothed(v, span)
            }),
        )]
    };
    let seconds = samples.flight.duration_s;
    let dir = folder("viewer-smoothing");
    save(
        &dir,
        smooth,
        &lines_of((-500.0, 500.0), seconds, &drawn(21)),
    );
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(verdict(&report, smooth), Verdict::Pass, "{report:?}");
    let row = report.rows.iter().find(|r| r.copy == smooth).unwrap();
    assert!(row.against.contains("smoothed over 21 samples"), "{row:?}");
    // the samples as logged are not the smoothed line
    save(&dir, smooth, &lines_of((-500.0, 500.0), seconds, &drawn(1)));
    let report = compare(17, &dir, None).unwrap();
    assert_eq!(verdict(&report, smooth), Verdict::Fail, "{report:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_round_run_by_hand_has_nothing_to_compare() {
    assert!(!capture::planned(19) && capture::planned(18));
    let dir = folder("by-hand");
    let error = compare(19, &dir, None).unwrap_err();
    assert!(error.contains("run by hand"), "{error}");
    std::fs::remove_dir_all(&dir).unwrap();
}
