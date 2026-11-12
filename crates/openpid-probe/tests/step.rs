//! The model of the reference tool's step response against what the tool
//! drew for the probes of round 4 (`tests/calibration/step/`).
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{Analysis, AnalysisCtx, Epoch, StepAnalysis, StepMethod, StepParams};
use openpid_core::{Axis, LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;
use openpid_fmt_blackbox::write::{WriteOptions, betaflight};
use openpid_probe::catalogue;
use openpid_probe::step_model::{self, Step};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The copies hold single-precision coordinates.
const COPY_PRECISION: f64 = 2.0e-6;

fn file(name: &str) -> String {
    let path = format!(
        "{}/../../tests/calibration/step/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The points of a curve read from the reference tool: sample, value.
fn reading(name: &str) -> Vec<(usize, f64)> {
    file(name)
        .lines()
        .skip(1)
        .map(|line| {
            let (sample, value) = line.split_once(',').unwrap();
            (sample.parse().unwrap(), value.parse().unwrap())
        })
        .collect()
}

/// The largest difference between a curve and the points read from the
/// tool.
fn worst(curve: &[f64], drawn: &[(usize, f64)]) -> f64 {
    drawn
        .iter()
        .map(|(sample, value)| (curve[*sample] - value).abs())
        .fold(0.0, f64::max)
}

fn axis(name: &str) -> Axis {
    match name {
        "roll" => Axis::Roll,
        "pitch" => Axis::Pitch,
        _ => Axis::Yaw,
    }
}

/// The setpoint and the filtered gyro a log of probe `id` holds for an
/// axis (the rounded flight, as the tests of the pack show), and its rate.
fn logged(id: &str, axis: Axis) -> (Vec<f64>, Vec<f64>, f64) {
    static ROUND: OnceLock<Vec<openpid_probe::Probe>> = OnceLock::new();
    let probes = ROUND.get_or_init(|| catalogue::round(4).unwrap());
    let flight = &probes.iter().find(|p| p.id == id).unwrap().flight;
    let samples = flight.samples(axis);
    let rounded = |x: &[f64]| x.iter().map(|v| v.round_ties_even()).collect();
    (
        rounded(&samples.setpoint),
        rounded(&samples.filtered),
        flight.fs,
    )
}

/// Samples by which the first window of a probe's log starts after the
/// first second: three for the 12 s log, two for the others.
fn late(id: &str) -> usize {
    if id == "t9-short-2k" {
        3
    } else {
        step_model::LATE
    }
}

/// What the model gives for an axis of a probe, computed once for all
/// the tests.
fn step(id: &str, axis: Axis, smoothing: usize) -> Arc<Step> {
    type Key = (String, usize, usize);
    static DONE: OnceLock<Mutex<HashMap<Key, Arc<Step>>>> = OnceLock::new();
    let key = (id.to_string(), axis.index(), smoothing);
    let done = DONE.get_or_init(Mutex::default);
    if let Some(step) = done.lock().unwrap().get(&key) {
        return Arc::clone(step);
    }
    let (setpoint, gyro, fs) = logged(id, axis);
    let step = Arc::new(step_model::step_from(&setpoint, &gyro, fs, smoothing, late(id)).unwrap());
    done.lock().unwrap().insert(key, Arc::clone(&step));
    step
}

#[test]
fn the_model_gives_the_mean_curves_of_round_4() {
    // (probe, axis, windows in the tool's legend, points of the copy)
    for (id, name, windows, points) in [
        ("t1-plants-2k", "roll", 258, 995),
        ("t1-plants-2k", "pitch", 256, 994),
        ("t1-plants-2k", "yaw", 259, 995),
        ("t2-plants-1k", "yaw", 259, 499),
        ("t3-plants-4k", "yaw", 259, 1985),
        // stick moves within 25 deg/s: two windows reach a mean of 20
        ("t4-amplitude-2k", "roll", 2, 998),
        // gyro noise of 20 deg/s RMS: 58 windows leave 0.5 … 1.5
        ("t5-noise-2k", "yaw", 201, 986),
        ("t6-holds-2k", "yaw", 216, 1001),
        // a craft that reaches 0.8 of the rate asked for
        ("t7-gain-2k", "roll", 256, 996),
        ("t8-sparse-2k", "roll", 55, 993),
        ("t8-sparse-2k", "pitch", 31, 995),
        ("t9-short-2k", "yaw", 79, 990),
    ] {
        let step = step(id, axis(name), 0);
        assert_eq!(step.windows.len(), windows, "{id} {name}");
        let drawn = reading(&format!("r4-{id}-{name}-mean.csv"));
        assert_eq!(drawn.len(), points, "{id} {name}");
        let off = worst(&step.mean, &drawn);
        assert!(off < COPY_PRECISION, "{id} {name}: {off} off");
    }
}

#[test]
fn the_model_gives_single_windows() {
    // two of the 259 curves the tool draws with "Raw" on: the first, and
    // one where the division is far from plain (without the zeros after
    // the window and the constant in the denominator it is 0.03 off)
    let step = step("t1-plants-2k", Axis::Yaw, 0);
    for index in [0, 116] {
        let window = &step.windows[index];
        assert_eq!(window.index, index);
        let drawn = reading(&format!("r4-t1-plants-2k-yaw-window-{index}.csv"));
        assert_eq!(drawn.len(), 1001);
        let off = worst(&window.response, &drawn);
        assert!(off < COPY_PRECISION, "window {index}: {off} off");
    }
    // roll leaves out one window of the 259, pitch three
    let left_out = |axis| -> Vec<usize> {
        let step = self::step("t1-plants-2k", axis, 0);
        let taken: Vec<usize> = step.windows.iter().map(|w| w.index).collect();
        (0..step.grid.count)
            .filter(|i| !taken.contains(i))
            .collect()
    };
    assert_eq!(left_out(Axis::Roll), [135]);
    assert_eq!(left_out(Axis::Pitch), [14, 158, 182]);
}

#[test]
fn the_model_gives_the_smoothed_curves() {
    for (id, name, counts) in [
        // a pure delay: the same windows, a blurred step
        ("t1-plants-2k", "roll", [258, 258, 258]),
        // the noisy axis: every level brings windows back (201 without)
        ("t5-noise-2k", "yaw", [231, 249, 256]),
    ] {
        for (level, windows) in counts.into_iter().enumerate() {
            let (label, _) = step_model::SMOOTHING[level + 1];
            let step = step(id, axis(name), level + 1);
            assert_eq!(step.windows.len(), windows, "{id} {label}");
            let drawn = reading(&format!("r4-{id}-{name}-mean-{label}.csv"));
            assert!(drawn.len() > 980, "{id} {label}");
            let off = worst(&step.mean, &drawn);
            assert!(off < COPY_PRECISION, "{id} {label}: {off} off");
        }
    }
}

#[test]
fn the_model_gives_the_peak_and_latency_plots() {
    let text = file("r4-points.csv");
    let mut rows = 0;
    let mut delays: HashMap<bool, usize> = HashMap::new();
    for line in text.lines().skip(1) {
        let cells: Vec<&str> = line.split(',').collect();
        let (id, name) = (cells[0], cells[1]);
        let number = |i: usize| cells[i].parse::<f64>().ok();
        let step = step(id, axis(name), 0);
        assert_eq!(
            step.windows.len(),
            cells[2].parse::<usize>().unwrap(),
            "{line}"
        );
        // the latency and both bars on every plot
        let latency = number(5).unwrap();
        assert!((step.latency_ms - latency).abs() < 2.0e-5, "{line}");
        let bar = number(6).unwrap();
        assert!((step.latency_spread_ms - bar).abs() < 2.0e-5, "{line}");
        // the gain of 1.25 lifts the peak out of the plot
        let Some(peak) = number(3) else {
            assert_eq!((id, name), ("t7-gain-2k", "yaw"));
            assert!(step.peak > 1.5);
            continue;
        };
        assert!(
            (step.peak_spread - number(4).unwrap()).abs() < 2.0e-6,
            "{line}"
        );
        // two plants that never overshoot, in the 12 s log: the tool stops
        // looking for the peak somewhere between 137 and 324 ms
        let flat = id == "t9-short-2k" && name != "yaw";
        let off = (step.peak - peak).abs();
        assert_eq!(off > 2.0e-6, flat, "{line}: {}", step.peak);
        if flat {
            let early = step.mean[..=648].iter().copied().fold(f64::MIN, f64::max);
            assert!((early - peak).abs() < 2.0e-6 && off < 5.0e-4, "{line}");
        }
        // the delay between setpoint and gyro: all but two plots
        let (setpoint, gyro, fs) = logged(id, axis(name));
        let delay = step_model::sp_gyro_delay_ms(&setpoint, &gyro, fs).unwrap();
        let same = (delay - number(7).unwrap()).abs() < 1.0e-3;
        *delays.entry(same).or_default() += 1;
        if !same {
            let known = [("t4-amplitude-2k", "roll"), ("t5-noise-2k", "yaw")];
            assert!(known.contains(&(id, name)), "{line}: {delay}");
        }
        rows += 1;
    }
    assert_eq!(rows, 26);
    assert_eq!((delays[&true], delays[&false]), (24, 2));
}

#[test]
fn the_step_analysis_of_a_log_is_the_curve_the_reference_tool_drew() {
    // the whole way a user goes: the log as a file, read back, an epoch,
    // the analysis of the Step tab with its defaults. The epoch that is the
    // tool's own stretch of the log: from two samples after the first
    // second to the end of the 29th.
    let probes = catalogue::round(4).unwrap();
    let flight = &probes
        .iter()
        .find(|p| p.id == "t1-plants-2k")
        .unwrap()
        .flight;
    let bytes = betaflight(&flight.build(), &WriteOptions::default()).unwrap();
    let log = BlackboxFormat::new()
        .open(&bytes, &OpenOptions::default())
        .unwrap();
    let ctx = AnalysisCtx {
        session: &log.sessions[0],
        epoch: Some(Epoch {
            start_s: 1.001,
            end_s: 29.0,
        }),
    };
    assert_eq!(StepParams::default().method, StepMethod::Division);
    for (name, windows) in [("roll", 258), ("pitch", 256), ("yaw", 259)] {
        let params = StepParams {
            axis: axis(name),
            ..StepParams::default()
        };
        let out = StepAnalysis.run(&ctx, &params).unwrap();
        assert_eq!(out.method, StepMethod::Division);
        assert_eq!(
            (out.windows_total, out.windows_used),
            (259, windows),
            "{name}"
        );
        assert_eq!(out.windows_rejected, 259 - windows, "{name}");
        assert_eq!((out.mean.len(), out.time_ms[1000]), (1001, 500.0));
        let drawn = reading(&format!("r4-t1-plants-2k-{name}-mean.csv"));
        let off = worst(&out.mean, &drawn);
        assert!(off < COPY_PRECISION, "{name}: {off} off");
        // the Peak and SR Latency plots of the tool, value and bar
        let row = file("r4-points.csv")
            .lines()
            .find(|l| l.starts_with(&format!("t1-plants-2k,{name},")))
            .unwrap()
            .to_string();
        let cells: Vec<f64> = row.split(',').skip(2).map(|c| c.parse().unwrap()).collect();
        let peak = out.mean.iter().copied().fold(f64::MIN, f64::max);
        assert!((peak - cells[1]).abs() < 2.0e-6, "{row}");
        assert!((out.spread.peak_sd - cells[2]).abs() < 2.0e-6, "{row}");
        assert!((out.spread.half_ms_mean - cells[3]).abs() < 2.0e-5, "{row}");
        assert!((out.spread.half_ms_sd - cells[4]).abs() < 2.0e-5, "{row}");
        assert!(
            (out.metrics.latency_ms.unwrap() - cells[5]).abs() < 1.0e-3,
            "{row}"
        );
        assert!(out.low.is_none() && out.high.is_none());
    }
    // smoothing is of the gyro, before the windows: the tool's "high"
    let params = StepParams {
        axis: Axis::Roll,
        smoothing: 3,
        ..StepParams::default()
    };
    let out = StepAnalysis.run(&ctx, &params).unwrap();
    let drawn = reading("r4-t1-plants-2k-roll-mean-high.csv");
    assert!(worst(&out.mean, &drawn) < COPY_PRECISION);
}
