//! Generator-based ground truth (`openpid_sim::SignalSpec`): every phase-1
//! analysis recovers what the synthetic session was built from.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{
    Analysis, AnalysisCtx, FilterSimAnalysis, FilterSimParams, PsdAnalysis, PsdParams,
    StepAnalysis, StepParams, ThrottleHeatmap, ThrottleHeatmapParams,
};
use openpid_core::{Axis, ChannelId, GyroStage};
use openpid_sim::{MotorNoise, Noise, Plant, SetpointProfile, SignalSpec, ThrottleProfile, Tone};

#[test]
fn step_response_recovers_the_plant() {
    let spec = SignalSpec {
        duration_s: 12.0,
        plant: Plant {
            gain: 0.9,
            tau_ms: 15.0,
            delay_ms: 8.0,
        },
        noise: Noise {
            white: 0.2,
            motor: None,
            ..Noise::default()
        },
        filter_hz: 0.0,
        ..SignalSpec::default()
    };
    let s = spec.build();
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let out = StepAnalysis.run(&ctx, &StepParams::default()).unwrap();
    let m = out.metrics;
    let delay = m.delay_ms.unwrap();
    let expected = 8.0 + 15.0 * 2f64.ln();
    assert!(
        (delay - expected).abs() < 4.0,
        "50 % crossing {delay} vs {expected}"
    );
    assert!((m.final_value - 0.9).abs() < 0.08, "gain {}", m.final_value);
    let latency = m.latency_ms.unwrap();
    assert!(
        latency >= 7.0 && latency <= delay + 1.0,
        "latency {latency}"
    );
    assert!(out.windows_used > 20);
}

#[test]
fn psd_finds_the_injected_tone() {
    let spec = SignalSpec {
        duration_s: 8.0,
        setpoint: SetpointProfile::Silence,
        noise: Noise {
            white: 0.3,
            pink: 0.0,
            tones: vec![Tone {
                hz: 137.0,
                amplitude: 5.0,
            }],
            motor: None,
        },
        filter_hz: 0.0,
        ..SignalSpec::default()
    };
    let s = spec.build();
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let out = PsdAnalysis
        .run(
            &ctx,
            &PsdParams {
                channel: ChannelId::Gyro {
                    axis: Axis::Roll,
                    stage: GyroStage::Raw,
                },
                segment_s: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
    let peaks = out.peaks(1, 10.0);
    assert_eq!(peaks.len(), 1);
    assert!((peaks[0].x - 137.0).abs() <= 1.0, "peak at {}", peaks[0].x);
}

#[test]
fn throttle_heatmap_ridge_follows_the_motor_speed() {
    let spec = SignalSpec {
        duration_s: 20.0,
        throttle: ThrottleProfile::Ramp {
            from: 10.0,
            to: 90.0,
        },
        setpoint: SetpointProfile::Silence,
        noise: Noise {
            white: 0.5,
            motor: Some(MotorNoise {
                harmonics: vec![4.0, 1.0],
                spread: 0.0,
                ..MotorNoise::default()
            }),
            ..Noise::default()
        },
        filter_hz: 0.0,
        ..SignalSpec::default()
    };
    let s = spec.build();
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let out = ThrottleHeatmap
        .run(
            &ctx,
            &ThrottleHeatmapParams {
                channel: ChannelId::Gyro {
                    axis: Axis::Roll,
                    stage: GyroStage::Raw,
                },
                max_hz: 500.0,
                ..Default::default()
            },
        )
        .unwrap();
    let ov = out.overlay.as_ref().unwrap();
    assert_eq!(ov.motors.len(), 4);
    let g = &out.grid;
    let dy = g.y[1] - g.y[0];
    let (mut hits, mut total) = (0, 0);
    for (ix, rpm) in ov.motors[0].iter().enumerate() {
        let Some(rpm) = rpm else { continue };
        let f1 = rpm / 60.0;
        if f1 >= 480.0 || out.counts[ix] < 2 {
            continue;
        }
        // the column's maximum sits at the fundamental
        let (best, _) =
            g.y.iter()
                .enumerate()
                .map(|(k, _)| (k, g.get(ix, k)))
                .fold((0, 0.0), |m, v| if v.1 > m.1 { v } else { m });
        total += 1;
        if (g.y[best] - f1).abs() <= 2.0 * dy {
            hits += 1;
        }
    }
    assert!(total > 30, "{total} bins");
    assert!(hits * 10 >= total * 9, "ridge in {hits} of {total} bins");
}

#[test]
fn filter_sim_reproduces_the_synthetic_filter_exactly() {
    let spec = SignalSpec {
        duration_s: 6.0,
        ..SignalSpec::default()
    };
    let s = spec.build();
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    let out = FilterSimAnalysis
        .run(&ctx, &FilterSimParams::default())
        .unwrap();
    let fit = out.fit.unwrap();
    assert!(fit.residual_rel < 0.01, "residual {}", fit.residual_rel);
    assert!(
        fit.unfiltered_rel > 2.0 * fit.residual_rel.max(1e-3),
        "the filter does something: {}",
        fit.unfiltered_rel
    );
    assert!(fit.delay_ms.abs() < 1.5, "delay {}", fit.delay_ms);
    assert_eq!(out.description, vec!["lowpass 1: PT1 100 Hz".to_string()]);
    assert_eq!(out.motors, 4);
}
