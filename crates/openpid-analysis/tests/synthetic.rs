//! Generator-based ground truth (`openpid_sim::SignalSpec`): every phase-1
//! analysis recovers what the synthetic session was built from.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{
    Analysis, AnalysisCtx, FilterSimAnalysis, FilterSimParams, MotorNoiseAnalysis,
    MotorNoiseParams, PsdAnalysis, PsdParams, RpmEstimateParams, RpmEstimator, StepAnalysis,
    StepParams, ThrottleHeatmap, ThrottleHeatmapParams,
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
            damping: None,
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
fn the_curve_is_one_transform_of_the_epoch_smoothed_in_decibels() {
    use openpid_analysis::{PsdMethod, curve_db, curve_window};
    // white noise of 3 deg/s RMS: a density of 9 (deg/s)² over half the rate
    let spec = SignalSpec {
        duration_s: 12.0,
        setpoint: SetpointProfile::Silence,
        noise: Noise {
            white: 3.0,
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
    let channel = ChannelId::Gyro {
        axis: Axis::Roll,
        stage: GyroStage::Raw,
    };
    let curve = |smoothing: usize, max_hz: Option<f64>| {
        PsdAnalysis
            .run(
                &ctx,
                &PsdParams {
                    channel: channel.clone(),
                    method: PsdMethod::Curve,
                    smoothing,
                    max_hz,
                    ..Default::default()
                },
            )
            .unwrap()
    };
    let out = curve(2, None);
    let x = s.channel(&channel).unwrap().data.values();
    assert_eq!(out.method, PsdMethod::Curve);
    assert_eq!((out.nperseg, out.segments), (x.len(), 1));
    assert_eq!(out.freq.len(), x.len() / 2 + 1);
    assert!((out.freq[1] - out.fs / x.len() as f64).abs() < 1.0e-12);
    // it is the curve of the samples themselves
    let by_hand = curve_db(&openpid_dsp::Fft::new(), x, out.fs, 2);
    assert_eq!(out.power_db, by_hand);
    for (p, db) in out.power.iter().zip(&out.power_db) {
        assert!((10.0 * p.log10() - db).abs() < 1.0e-9);
    }
    // noise reads 6.8 dB below its density: 4.26 dB of the window that is
    // not put back, 2.51 dB from averaging decibels
    let density_db = 10.0 * (9.0 / (out.fs / 2.0)).log10();
    let floor: Vec<f64> = out
        .freq
        .iter()
        .zip(&out.power_db)
        .filter(|(f, _)| **f > 200.0 && **f < 450.0)
        .map(|(_, db)| *db)
        .collect();
    let mean = floor.iter().sum::<f64>() / floor.len() as f64;
    assert!((mean - (density_db - 6.77)).abs() < 0.3, "{mean}");
    // a steady tone of 5 deg/s is a small bump here ...
    let at = |o: &openpid_analysis::PsdOutput, hz: f64| {
        let k = (hz / o.freq[1]).round() as usize;
        o.power_db[k]
    };
    let bump = at(&out, 137.0) - mean;
    assert!(bump > 1.0 && bump < 12.0, "{bump}");
    // ... and a line of tens of decibels in the density
    let density = PsdAnalysis
        .run(
            &ctx,
            &PsdParams {
                channel: channel.clone(),
                segment_s: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(density.method, PsdMethod::Density);
    let line = at(&density, 137.0) - density_db;
    assert!(line > 25.0, "{line}");

    // a higher level is a wider window, and a flatter curve
    let points = out.freq.len();
    assert!(curve_window(4, points) > curve_window(2, points));
    let spread = |o: &openpid_analysis::PsdOutput| {
        let v: Vec<f64> = o
            .freq
            .iter()
            .zip(&o.power_db)
            .filter(|(f, _)| **f > 200.0 && **f < 450.0)
            .map(|(_, db)| *db)
            .collect();
        let m = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|d| (d - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    };
    assert!(spread(&curve(4, None)) < spread(&out));
    assert!(spread(&curve(0, None)) > spread(&out));
    // the upper bound cuts the curve, it does not change it
    let low = curve(2, Some(100.0));
    assert!(low.freq.last().is_some_and(|f| *f <= 100.0 && *f > 99.9));
    assert_eq!(low.power_db[..], out.power_db[..low.power_db.len()]);
}

#[test]
fn the_method_and_the_level_are_part_of_what_a_spectrum_is() {
    use openpid_analysis::PsdMethod;
    use std::hash::{Hash, Hasher};
    let hash = |p: &PsdParams| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    };
    let density = PsdParams::default();
    assert_eq!(density.method, PsdMethod::Density);
    assert_eq!(density.smoothing, 2);
    let curve = PsdParams {
        method: PsdMethod::Curve,
        ..PsdParams::default()
    };
    assert_ne!(hash(&density), hash(&curve));
    assert_ne!(
        hash(&curve),
        hash(&PsdParams {
            smoothing: 3,
            ..curve.clone()
        })
    );
    // parameters saved before the curve existed read as the density
    let old: PsdParams = serde_json::from_str(r#"{"segment_s":0.5}"#).unwrap();
    assert_eq!((old.method, old.segment_s), (PsdMethod::Density, 0.5));
    let text = serde_json::to_string(&curve).unwrap();
    assert!(text.contains(r#""method":"curve""#), "{text}");
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

#[test]
fn rpm_estimate_follows_the_synthetic_motors() {
    let spec = SignalSpec {
        duration_s: 15.0,
        throttle: ThrottleProfile::Sine {
            mean: 50.0,
            amplitude: 30.0,
            hz: 0.2,
        },
        setpoint: SetpointProfile::Silence,
        noise: Noise {
            white: 1.0,
            motor: Some(MotorNoise {
                harmonics: vec![3.0, 1.5, 0.5],
                spread: 0.04,
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
    let out = RpmEstimator
        .run(&ctx, &RpmEstimateParams::default())
        .unwrap();
    let err = out.median_error.unwrap();
    assert!(err < 0.03, "median error {err}");
    assert!(out.confidence.iter().sum::<f64>() / out.frames as f64 > 0.5);
    // the estimate moves with the throttle sine
    let (lo, hi) = out
        .rpm
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
    assert!(hi > lo * 1.5, "range {lo}..{hi}");
}

#[test]
fn motor_noise_table_shows_the_bent_prop() {
    let spec = SignalSpec {
        duration_s: 12.0,
        throttle: ThrottleProfile::Ramp {
            from: 30.0,
            to: 60.0,
        },
        setpoint: SetpointProfile::Silence,
        noise: Noise {
            white: 0.5,
            motor: Some(MotorNoise {
                harmonics: vec![3.0, 1.0, 0.4],
                spread: 0.12,
                gains: vec![1.0, 1.0, 2.5, 1.0],
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
    let out = MotorNoiseAnalysis
        .run(&ctx, &MotorNoiseParams::default())
        .unwrap();
    assert_eq!(out.rpm_source, "logged");
    assert_eq!(out.motors.len(), 4);
    assert_eq!(out.loudest, Some(2), "motor 3 carries the bent prop");
    let fundamental = |m: usize| out.motors[m].harmonics[0];
    // neighbouring motors leak into each other at this spread, so the
    // 2.5 × gain shows as a clearly higher ratio rather than exactly 1.8
    assert!(fundamental(2).ratio > 1.4, "{:?}", fundamental(2));
    for m in [0, 1, 3] {
        assert!(fundamental(m).ratio < 1.0, "{:?}", fundamental(m));
        assert!(
            (fundamental(m).mean_hz - out.motors[m].mean_rpm / 60.0).abs() < 2.0,
            "harmonic placed at the motor's speed"
        );
    }
    assert!(out.motors[2].total > 2.0 * out.motors[0].total);
    assert!(!out.freq.is_empty() && out.freq.len() == out.psd_db.len());
    // without eRPM the estimate stands in, one speed for all motors
    let mut quiet = s.clone();
    for idx in 0..4u8 {
        quiet.channels.remove(&ChannelId::MotorRpm { idx });
    }
    let ctx2 = AnalysisCtx {
        session: &quiet,
        epoch: None,
    };
    let est = MotorNoiseAnalysis
        .run(&ctx2, &MotorNoiseParams::default())
        .unwrap();
    assert_eq!(est.rpm_source, "estimated");
    assert_eq!(est.motors.len(), 1);
    assert!(est.motors[0].harmonics[0].amplitude > 0.0);
}
