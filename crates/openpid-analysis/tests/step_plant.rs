//! Step response on a synthetic first-order plant with a known transport
//! delay: for a white input the cross-correlation latency equals the delay
//! within one sample, for stick-like input it stays between the delay and the
//! 50 % crossing, and the Y-offset correction brings the settled response to 1.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{Analysis, AnalysisCtx, StepAnalysis, StepMethod, StepParams};
use openpid_core::session::Diagnostics;
use openpid_core::{
    Axis, Channel, ChannelId, ChannelSet, FirmwareInfo, FlightSession, GyroStage, Origin, Series,
    Settings, Timebase, Unit,
};

const FS: f64 = 1000.0;
const DELAY: usize = 8; // samples = ms
const TAU_MS: f64 = 15.0;
const GAIN: f64 = 0.8; // the gyro only reaches 80 % of the setpoint

/// Deterministic uniform pseudo-random numbers in [0, 1).
fn uniform() -> impl FnMut() -> f64 {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    move || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (s >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// White input: a new level every sample.
fn white(n: usize) -> Vec<f64> {
    let mut next = uniform();
    (0..n).map(|_| (next() * 2.0 - 1.0) * 600.0).collect()
}

/// Piecewise-constant pseudo-random stick input: a new level every 50–250 ms.
fn stick(n: usize) -> Vec<f64> {
    let mut next = uniform();
    let mut x = Vec::with_capacity(n);
    while x.len() < n {
        let hold = 50 + (next() * 200.0) as usize;
        let level = (next() * 2.0 - 1.0) * 600.0;
        x.extend(std::iter::repeat_n(level, hold));
    }
    x.truncate(n);
    x
}

fn plant(x: &[f64]) -> Vec<f64> {
    let a = 1.0 - (-1.0 / TAU_MS).exp();
    let mut y = vec![0.0; x.len()];
    for i in 1..x.len() {
        let u = if i >= DELAY { GAIN * x[i - DELAY] } else { 0.0 };
        y[i] = y[i - 1] + a * (u - y[i - 1]);
    }
    y
}

fn session(seconds: f64, input: fn(usize) -> Vec<f64>) -> FlightSession {
    let n = (seconds * FS) as usize;
    let x = input(n);
    let y = plant(&x);
    let mut channels = ChannelSet::new();
    for (id, name, data) in [
        (ChannelId::Setpoint { axis: Axis::Roll }, "setpoint[0]", x),
        (
            ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Filtered,
            },
            "gyroADC[0]",
            y,
        ),
    ] {
        channels.insert(Channel {
            id,
            name: name.into(),
            unit: Unit::DegreesPerSecond,
            data: Series::from_vec(data),
            origin: Origin::Logged { field: name.into() },
        });
    }
    FlightSession {
        index: 0,
        offset_bytes: 0,
        firmware: FirmwareInfo::parse_revision("Synthetic 1.0"),
        profile_id: "synthetic".into(),
        settings: Settings::default(),
        raw_headers: Vec::new(),
        timebase: Timebase::from_times(
            (0..n).map(|i| (i as f64 * 1.0e6 / FS) as i64).collect(),
            Some(1.0e6 / FS),
        ),
        channels,
        events: Vec::new(),
        aux: Vec::new(),
        flight_modes: Vec::new(),
        diagnostics: Diagnostics::default(),
    }
}

#[test]
fn latency_equals_the_plant_delay_for_a_white_input() {
    let s = session(12.0, white);
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let out = StepAnalysis.run(&ctx, &StepParams::default()).unwrap();
    let m = out.metrics;
    let latency = m.latency_ms.unwrap();
    assert!(
        (latency - DELAY as f64).abs() <= 1.0,
        "latency {latency} ms vs delay {DELAY} ms"
    );
    // the 50 % crossing includes the first-order lag: delay + tau·ln 2
    let delay = m.delay_ms.unwrap();
    let expected = DELAY as f64 + TAU_MS * 2f64.ln();
    assert!(
        (delay - expected).abs() < 4.0,
        "delay {delay} vs {expected}"
    );
    assert!(
        (m.final_value - GAIN).abs() < 0.08,
        "without Y-offset the response settles at the plant gain: {}",
        m.final_value
    );
    assert_eq!(
        out.windows_used + out.windows_rejected,
        out.windows_total,
        "a white input always has the stick far enough from its centre"
    );
    // six of the 201 windows this flight has settle farther from the
    // others than three deviations; without that test every window counts
    assert_eq!(out.windows_total, 201);
    assert!(out.windows_rejected <= 8, "{}", out.windows_rejected);
    let all = StepParams {
        dist_to_median: false,
        ..StepParams::default()
    };
    let every = StepAnalysis.run(&ctx, &all).unwrap();
    assert!(every.windows_rejected <= 2, "{}", every.windows_rejected);
    // the Wiener estimate takes every window
    let wiener = StepParams {
        method: StepMethod::Wiener,
        ..StepParams::default()
    };
    let out = StepAnalysis.run(&ctx, &wiener).unwrap();
    assert_eq!(out.windows_used, out.windows_total);
    assert!((out.metrics.latency_ms.unwrap() - DELAY as f64).abs() <= 1.0);
}

#[test]
fn the_window_length_sets_the_windows() {
    // a window of 4 s is twice as long and twice as far from the next as
    // one of 2 s: on 12 s, (12 − 4) / 0.1 + 1 = 81 windows, not 201
    let s = session(12.0, white);
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let two = StepAnalysis.run(&ctx, &StepParams::default()).unwrap();
    let four = StepAnalysis
        .run(
            &ctx,
            &StepParams {
                window_s: 4.0,
                ..StepParams::default()
            },
        )
        .unwrap();
    assert_eq!(two.windows_total, 201);
    assert_eq!(four.windows_total, 81);
    assert!(
        (four.metrics.latency_ms.unwrap() - DELAY as f64).abs() <= 1.0,
        "the same plant through longer windows"
    );
}

#[test]
fn stick_input_latency_and_y_offset_correction() {
    let s = session(12.0, stick);
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    // the division: the latency lies as it does for the Wiener estimate
    let division = StepAnalysis.run(&ctx, &StepParams::default()).unwrap();
    let (latency, delay) = (
        division.metrics.latency_ms.unwrap(),
        division.metrics.delay_ms.unwrap(),
    );
    assert!(latency >= DELAY as f64 - 1.0 && latency <= delay + 1.0);
    assert!((division.metrics.final_value - GAIN).abs() < 0.08);
    // the Y-offset is a correction of the Wiener estimate
    let wiener = StepParams {
        method: StepMethod::Wiener,
        ..StepParams::default()
    };
    let plain = StepAnalysis.run(&ctx, &wiener).unwrap();
    let m = plain.metrics;
    let latency = m.latency_ms.unwrap();
    let delay = m.delay_ms.unwrap();
    assert!(
        latency >= DELAY as f64 - 1.0 && latency <= delay + 1.0,
        "smooth input: latency {latency} between the delay {DELAY} and the 50 % crossing {delay}"
    );
    let corrected = StepAnalysis
        .run(
            &ctx,
            &StepParams {
                y_offset: true,
                ..wiener.clone()
            },
        )
        .unwrap();
    let c = corrected.metrics;
    assert!(
        (m.final_value - GAIN).abs() < 0.08,
        "without Y-offset: {}",
        m.final_value
    );
    assert!(
        (c.final_value - 1.0).abs() < 0.03,
        "Y-offset brings the settled value to 1: {}",
        c.final_value
    );
    assert_eq!(
        c.latency_ms, m.latency_ms,
        "latency does not depend on Y-offset"
    );
    assert_eq!(corrected.windows_used, plain.windows_used);
    for (a, b) in corrected.mean.iter().zip(&plain.mean) {
        assert!((a - b / GAIN).abs() < 0.05, "{a} vs {b}");
    }
    let json = serde_json::to_value(c).unwrap();
    assert!(json["latency_ms"].is_number());
    assert!(json["delay_ms"].is_number());
}

/// The plant's own response to a unit step at sample 0.
fn plant_step(i: usize) -> f64 {
    if i < DELAY {
        return 0.0;
    }
    let a = 1.0 - (-1.0 / TAU_MS).exp();
    GAIN * (1.0 - (1.0 - a).powi((i - DELAY + 1) as i32))
}

#[test]
fn the_curve_is_the_plant_step_response() {
    // the plant's response to a unit step is known in closed form; the
    // curve of the division is it, with and without the distance to the
    // median, and the windows that test leaves out move it by little
    let s = session(30.0, white);
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let near = StepAnalysis.run(&ctx, &StepParams::default()).unwrap();
    let all = StepAnalysis
        .run(
            &ctx,
            &StepParams {
                dist_to_median: false,
                ..StepParams::default()
            },
        )
        .unwrap();
    assert_eq!(near.mean.len(), 501);
    assert!(near.windows_used < all.windows_used);
    for out in [&near, &all] {
        let worst = out
            .mean
            .iter()
            .enumerate()
            .map(|(i, v)| (v - plant_step(i)).abs())
            .fold(0.0, f64::max);
        // 0.003 and 0.002 off the plant at most
        assert!(worst < 0.004, "{worst}");
    }
    let moved = near
        .mean
        .iter()
        .zip(&all.mean)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(moved > 0.0 && moved < 0.003, "{moved}");
    // the half-way time of a window, (i + 1)/fs − 1 ms at its first sample
    // above 0.5: the plant's is sample 22, 22 ms
    let half = (0..500).find(|i| plant_step(*i) > 0.5).unwrap();
    assert_eq!(half, 22);
    let latency = near.spread.half_ms_mean;
    assert!((latency - 22.0).abs() < 0.25, "{latency}");
}
