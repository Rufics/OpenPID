//! Timing report for the decoder and the analyses (DESIGN §11 targets:
//! decoding an 8 kHz × 3 min log under 1 s, every analysis under 2 s).
//! No harness dependency: `cargo bench -p openpid-analysis` prints a table
//! and CI keeps it in the job log without gating on it.
#![allow(
    clippy::unwrap_used,
    reason = "a benchmark may panic on unexpected data"
)]

use openpid_analysis::{
    Analysis, AnalysisCtx, FilterSimAnalysis, FilterSimParams, MotorNoiseAnalysis,
    MotorNoiseParams, PsdAnalysis, PsdParams, RpmEstimateParams, RpmEstimator, StepAnalysis,
    StepParams, StftAnalysis, StftParams, ThrottleHeatmap, ThrottleHeatmapParams,
};
use openpid_core::{Axis, ChannelId, GyroStage, LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn logs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/logs")
}

/// Median of `runs` timings of `f`.
fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut durations = Vec::with_capacity(runs);
    let mut last = None;
    for _ in 0..runs {
        let t0 = Instant::now();
        last = Some(f());
        durations.push(t0.elapsed());
    }
    durations.sort();
    (durations[durations.len() / 2], last.unwrap())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runs: usize = args
        .iter()
        .position(|a| a == "--runs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    println!("| what | input | median of {runs} | rate |");
    println!("|---|---|---|---|");
    let format = BlackboxFormat::new();
    let mut session = None;
    for name in [
        "betaflight-4.2.8-44-logs.bbl",
        "betaflight-2025.12.2-btfl_027.bbl",
    ] {
        let path = logs_dir().join(name);
        let Ok(data) = std::fs::read(&path) else {
            println!("| decode | {name} | missing (git lfs pull) | |");
            continue;
        };
        if data.starts_with(b"version https://git-lfs") {
            println!("| decode | {name} | LFS pointer (git lfs pull) | |");
            continue;
        }
        let (d, log) = time(runs, || {
            format.open(&data, &OpenOptions::default()).unwrap()
        });
        let samples: usize = log
            .sessions
            .iter()
            .map(openpid_core::FlightSession::len)
            .sum();
        println!(
            "| decode | {name} ({:.1} MB, {} sessions, {samples} samples) | {:.1} ms | {:.1} MB/s |",
            data.len() as f64 / 1e6,
            log.sessions.len(),
            d.as_secs_f64() * 1e3,
            data.len() as f64 / 1e6 / d.as_secs_f64()
        );
        if name.starts_with("betaflight-2025") {
            session = log
                .sessions
                .into_iter()
                .max_by_key(openpid_core::FlightSession::len);
        }
    }
    let Some(s) = session else {
        println!("| analyses | skipped: the 2025.12 fixture is not available | | |");
        return;
    };
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let input = format!(
        "2025.12 session, {:.1} s @ {:.0} Hz",
        s.timebase.duration_s(),
        s.timebase.sample_rate_hz()
    );
    let raw = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Raw,
    };
    let row = |what: &str, d: Duration| {
        println!("| {what} | {input} | {:.1} ms | |", d.as_secs_f64() * 1e3);
    };
    row(
        "step response",
        time(runs, || {
            StepAnalysis.run(&ctx, &StepParams::default()).unwrap()
        })
        .0,
    );
    row(
        "psd",
        time(runs, || {
            PsdAnalysis.run(&ctx, &PsdParams::default()).unwrap()
        })
        .0,
    );
    row(
        "freq × throttle",
        time(runs, || {
            ThrottleHeatmap
                .run(&ctx, &ThrottleHeatmapParams::default())
                .unwrap()
        })
        .0,
    );
    row(
        "freq × time",
        time(runs, || {
            StftAnalysis.run(&ctx, &StftParams::default()).unwrap()
        })
        .0,
    );
    row(
        "filter sim",
        time(runs, || {
            FilterSimAnalysis
                .run(&ctx, &FilterSimParams::default())
                .unwrap()
        })
        .0,
    );
    row(
        "rpm estimate",
        time(runs, || {
            RpmEstimator
                .run(
                    &ctx,
                    &RpmEstimateParams {
                        channel: raw.clone(),
                        ..Default::default()
                    },
                )
                .unwrap()
        })
        .0,
    );
    row(
        "motor noise",
        time(runs, || {
            MotorNoiseAnalysis
                .run(
                    &ctx,
                    &MotorNoiseParams {
                        channel: raw.clone(),
                        ..Default::default()
                    },
                )
                .unwrap()
        })
        .0,
    );
}
