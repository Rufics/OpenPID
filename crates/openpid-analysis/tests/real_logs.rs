//! Run every analysis on the committed real logs and check physical plausibility.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{
    Analysis, AnalysisCtx, AnalysisError, Epoch, PsdAnalysis, PsdParams, StepAnalysis, StepParams,
    StftAnalysis, StftParams, ThrottleHeatmap, ThrottleHeatmapParams,
};
use openpid_core::{Axis, ChannelId, GyroStage, LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;
use std::path::PathBuf;

fn load(name: &str) -> openpid_core::LogFile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/logs")
        .join(name);
    let data = std::fs::read(&path).unwrap();
    BlackboxFormat::new()
        .open(&data, &OpenOptions::default())
        .unwrap()
}

#[test]
fn step_response_of_a_real_flight() {
    let log = load("betaflight-4.2.0.BFL");
    let s = &log.sessions[0];
    for axis in [Axis::Roll, Axis::Pitch] {
        let ctx = AnalysisCtx {
            session: s,
            epoch: None,
        };
        let out = StepAnalysis
            .run(
                &ctx,
                &StepParams {
                    axis,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(out.time_ms.len(), 500, "1 kHz log, 0.5 s response");
        assert!(
            out.windows_used > 10,
            "{axis:?}: used {} of {}",
            out.windows_used,
            out.windows_total
        );
        let m = out.metrics;
        assert!(
            (m.final_value - 1.0).abs() < 0.3,
            "{axis:?}: final value {}",
            m.final_value
        );
        let rise = m.rise_time_ms.expect("rise time");
        assert!((5.0..150.0).contains(&rise), "{axis:?}: rise {rise} ms");
        assert!(
            m.overshoot_pct.unwrap() < 60.0,
            "{axis:?}: overshoot {:?}",
            m.overshoot_pct
        );
        assert!(
            out.mean[0].abs() < 0.2,
            "{axis:?}: mean[0] = {}",
            out.mean[0]
        );
        assert!(out.mean[20] > out.mean[0], "{axis:?}");
        // every time column carries total weight 1 minus whatever fell outside the range
        let weight: f64 = out.histogram.z.iter().sum::<f64>() / out.time_ms.len() as f64;
        assert!(
            weight > 0.5 && weight <= 1.0 + 1e-9,
            "histogram weight per column {weight}"
        );
    }
    let ctx = AnalysisCtx {
        session: s,
        epoch: Some(Epoch {
            start_s: 2.0,
            end_s: 8.0,
        }),
    };
    let out = StepAnalysis.run(&ctx, &StepParams::default()).unwrap();
    assert!(
        out.windows_total < 60,
        "epoch windows {}",
        out.windows_total
    );
    let ctx = AnalysisCtx {
        session: s,
        epoch: Some(Epoch {
            start_s: 100.0,
            end_s: 101.0,
        }),
    };
    assert!(StepAnalysis.run(&ctx, &StepParams::default()).is_err());
}

#[test]
fn spectra_of_a_real_flight() {
    let log = load("betaflight-4.2.0.BFL");
    let s = &log.sessions[0];
    let ctx = AnalysisCtx {
        session: s,
        epoch: None,
    };
    let gyro = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Filtered,
    };
    let psd = PsdAnalysis
        .run(
            &ctx,
            &PsdParams {
                channel: gyro.clone(),
                max_hz: Some(500.0),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(psd.unit, "deg/s");
    assert!(psd.freq.last().copied().unwrap() <= 500.0);
    assert!(psd.segments > 50);
    assert!(psd.power.iter().all(|v| v.is_finite() && *v >= 0.0));
    let lo: f64 = psd.power[1..20].iter().sum();
    let hi: f64 = psd.power[psd.power.len() - 20..].iter().sum();
    assert!(lo > hi, "lo {lo} hi {hi}");

    let hm = ThrottleHeatmap
        .run(
            &ctx,
            &ThrottleHeatmapParams {
                channel: gyro.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hm.grid.x.len(), 101);
    assert_eq!(hm.grid.z.len(), hm.grid.x.len() * hm.grid.y.len());
    assert!(hm.counts.iter().sum::<usize>() > 0);
    assert!(hm.grid.z.iter().all(|v| v.is_finite() && *v >= 0.0));

    let st = StftAnalysis
        .run(
            &ctx,
            &StftParams {
                channel: gyro,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(st.grid.x.len() > 50);
    assert!(st.grid.y.last().copied().unwrap() <= 1000.0);
    assert_eq!(st.grid.z.len(), st.grid.x.len() * st.grid.y.len());
}

#[test]
fn errors_are_explicit() {
    let log = load("betaflight-4.2.0.BFL");
    let s = &log.sessions[0];
    let ctx = AnalysisCtx {
        session: s,
        epoch: None,
    };
    // a channel this log does not have
    let missing = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Raw,
    };
    assert!(matches!(
        PsdAnalysis.run(
            &ctx,
            &PsdParams {
                channel: missing,
                ..Default::default()
            }
        ),
        Err(AnalysisError::MissingChannel(_))
    ));
    // heatmap over a raw-unit channel is allowed
    assert!(
        ThrottleHeatmap
            .run(
                &ctx,
                &ThrottleHeatmapParams {
                    channel: ChannelId::Motor { idx: 0 },
                    ..Default::default()
                }
            )
            .is_ok()
    );
    // bad parameters
    assert!(matches!(
        PsdAnalysis.run(
            &ctx,
            &PsdParams {
                overlap: 1.5,
                ..Default::default()
            }
        ),
        Err(AnalysisError::InvalidParams(_))
    ));
}
