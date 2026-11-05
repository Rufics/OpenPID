//! Run every analysis on the committed real logs and check physical plausibility.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{
    Analysis, AnalysisCtx, AnalysisError, Epoch, PsdAnalysis, PsdParams, RpmEstimateParams,
    RpmEstimator, StepAnalysis, StepParams, StftAnalysis, StftParams, ThrottleHeatmap,
    ThrottleHeatmapParams,
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
    // a channel this log does not have (debug_mode GYRO_SCALED gives it a raw gyro)
    let missing = ChannelId::Debug { idx: 7 };
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

#[test]
fn rpm_overlay_follows_the_motor_ridge() {
    let log = load("betaflight-4.5.1-two-sessions.bbl");
    let s = log.sessions.iter().max_by_key(|s| s.len()).unwrap();
    let ctx = AnalysisCtx {
        session: s,
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
                ..Default::default()
            },
        )
        .unwrap();
    let ov = out.overlay.as_ref().expect("eRPM logged");
    assert_eq!(ov.motors.len(), 4);
    assert_eq!(ov.harmonics, 3);
    assert_eq!(ov.rpm_min_hz, Some(100.0));
    assert!(ov.dyn_notch_hz.is_some_and(|(lo, hi)| lo < hi));
    assert!(ov.harmonic_hz(0, 2, 30).is_none_or(|f| f > 0.0));
    assert_eq!(ov.harmonic_hz(9, 1, 0), None);
    // in every throttle bin with a motor speed, the fundamental of each
    // motor sits on a ridge of the unfiltered gyro spectrum: the amplitude
    // there beats the median of its ±120 Hz surroundings in most bins
    let g = &out.grid;
    let dy = g.y[1] - g.y[0];
    let mut hits = 0usize;
    let mut total = 0usize;
    for (m, bins) in ov.motors.iter().enumerate() {
        for (ix, rpm) in bins.iter().enumerate() {
            let Some(rpm) = rpm else { continue };
            if out.counts[ix] < 2 {
                continue;
            }
            let f1 = rpm / 60.0;
            let iy = (f1 / dy).round() as usize;
            if iy + 2 >= g.y.len() || iy < 2 {
                continue;
            }
            let at = (iy - 1..=iy + 1).map(|k| g.get(ix, k)).fold(0.0, f64::max);
            let mut around: Vec<f64> =
                g.y.iter()
                    .enumerate()
                    .filter(|(_, f)| (*f - f1).abs() <= 120.0 && (*f - f1).abs() > 15.0)
                    .map(|(k, _)| g.get(ix, k))
                    .collect();
            around.sort_by(f64::total_cmp);
            let median = around[around.len() / 2];
            total += 1;
            if at > 1.2 * median {
                hits += 1;
            }
            let _ = m;
        }
    }
    eprintln!("ridge hits {hits} of {total}");
    assert!(total > 50, "{total} bins checked");
    assert!(
        hits * 2 > total,
        "fundamental on the ridge in {hits} of {total} bins"
    );
}

#[test]
fn rpm_estimate_tracks_the_logged_motor_speed() {
    for (name, max_median) in [
        ("betaflight-4.5.1-two-sessions.bbl", 0.05),
        ("betaflight-2025.12.2-btfl_027.bbl", 0.05),
    ] {
        let log = load(name);
        let s = log.sessions.iter().max_by_key(|s| s.len()).unwrap();
        let ctx = AnalysisCtx {
            session: s,
            epoch: None,
        };
        let out = RpmEstimator
            .run(&ctx, &RpmEstimateParams::default())
            .unwrap();
        assert_eq!(out.rpm.len(), out.frames);
        assert_eq!(out.confidence.len(), out.frames);
        let err = out.median_error.expect("eRPM logged");
        eprintln!(
            "{name}: {} frames, median error {:.1} %",
            out.frames,
            err * 100.0
        );
        assert!(err < max_median, "{name}: median error {err}");
        assert!(out.confidence.iter().all(|c| (0.0..=1.0).contains(c)));
        let mean_conf = out.confidence.iter().sum::<f64>() / out.frames as f64;
        assert!(mean_conf > 0.3, "{name}: confidence {mean_conf}");
        let fundamentals: usize = out.harmonic.iter().map(|h| usize::from(*h == 1)).sum();
        assert!(fundamentals * 2 > out.frames, "mostly the fundamental");
        // the filtered gyro is refused, the parameters are checked
        let filtered = RpmEstimator.run(
            &ctx,
            &RpmEstimateParams {
                channel: ChannelId::Gyro {
                    axis: Axis::Roll,
                    stage: GyroStage::Filtered,
                },
                ..Default::default()
            },
        );
        assert!(matches!(filtered, Err(AnalysisError::InvalidParams(_))));
        assert!(matches!(
            RpmEstimator.run(
                &ctx,
                &RpmEstimateParams {
                    rpm_max: 100.0,
                    ..Default::default()
                }
            ),
            Err(AnalysisError::InvalidParams(_))
        ));
    }
}

#[test]
fn fitness_of_the_real_logs() {
    use openpid_analysis::{FitnessLevel, check_session};
    // a long flight with raw gyro, eRPM and plenty of stick input
    let log = load("betaflight-2025.12.2-btfl_027.bbl");
    let f = check_session(&log.sessions[0]);
    assert_eq!(f.overall, FitnessLevel::Ok, "{f:?}");
    for id in [
        "rate", "duration", "raw_gyro", "erpm", "frames", "gaps", "sticks",
    ] {
        assert_eq!(f.level(id), FitnessLevel::Ok, "{id}: {:?}", f.check(id));
    }
    // 4.2.0: raw gyro through debug_mode GYRO_SCALED, no eRPM
    let log = load("betaflight-4.2.0.BFL");
    let f = check_session(&log.sessions[0]);
    assert_eq!(f.level("raw_gyro"), FitnessLevel::Ok);
    let erpm = f.check("erpm").unwrap();
    assert_eq!(erpm.level, FitnessLevel::Warn);
    assert!(erpm.fix.as_deref().unwrap().contains("dshot_telemetry"));
    assert_eq!(
        f.level("sticks"),
        FitnessLevel::Ok,
        "{:?}",
        f.check("sticks")
    );
    // 4.5.1 session 0: three seconds of arming, nothing to analyse
    let log = load("betaflight-4.5.1-two-sessions.bbl");
    let f = check_session(&log.sessions[0]);
    assert_eq!(f.overall, FitnessLevel::Fail);
    assert_eq!(f.level("duration"), FitnessLevel::Fail);
    assert_eq!(f.level("sticks"), FitnessLevel::Fail);
    assert!(f.check("duration").unwrap().fix.is_some());
}
