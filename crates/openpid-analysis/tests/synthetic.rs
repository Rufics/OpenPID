//! Generator-based ground truth (`openpid_sim::SignalSpec`): every phase-1
//! analysis recovers what the synthetic session was built from.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{
    Analysis, AnalysisCtx, FilterSimAnalysis, FilterSimOutput, FilterSimParams, MotorNoiseAnalysis,
    MotorNoiseParams, PrintedDelay, PsdAnalysis, PsdParams, RateSource, RpmEstimateParams,
    RpmEstimator, StepAnalysis, StepParams, ThrottleHeatmap, ThrottleHeatmapParams,
};
use openpid_core::settings::{GyroFilterLayout, Lowpass, LowpassKind};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage};
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

/// Filter Sim of `s` with its loop rates set to `gyro_hz` and `pid_hz`.
fn filter_sim_at(
    s: &FlightSession,
    gyro_hz: Option<f64>,
    pid_hz: Option<f64>,
    layout: GyroFilterLayout,
) -> FilterSimOutput {
    let mut s = s.clone();
    s.settings.loop_rates.gyro_hz = gyro_hz;
    s.settings.loop_rates.pid_hz = pid_hz;
    s.settings.loop_rates.gyro_filter_layout = layout;
    let ctx = AnalysisCtx {
        session: &s,
        epoch: None,
    };
    FilterSimAnalysis
        .run(&ctx, &FilterSimParams::default())
        .unwrap()
}

#[test]
fn filter_curves_follow_the_loop_rate_and_the_simulation_the_log_rate() {
    // a log at 1 kHz with a PT1 at 100 Hz and a second one at 300 Hz
    let mut s = SignalSpec {
        duration_s: 6.0,
        ..SignalSpec::default()
    }
    .build();
    s.settings.gyro_filters.lowpass2 = Some(Lowpass {
        kind: Some(LowpassKind::Pt1),
        cutoff_hz: 300.0,
        dyn_min_hz: None,
        dyn_max_hz: None,
        dyn_expo: 0.0,
    });
    let split = GyroFilterLayout::PidLoopAfterLowpass2;
    // the craft's loops at the rate of the log, as the generator writes it;
    // at 8 and 4 kHz; and not in the headers at all
    let equal = filter_sim_at(&s, Some(1000.0), Some(1000.0), split);
    let craft = filter_sim_at(&s, Some(8000.0), Some(4000.0), split);
    let none = filter_sim_at(&s, None, None, split);

    // the simulation runs on the samples of the log: the same to the bit
    // whatever the loop rates of the headers
    for other in [&craft, &none] {
        assert_eq!(other.fs, 1000.0);
        assert_eq!(other.sim, equal.sim);
        assert_eq!(other.sim_as_flown, equal.sim_as_flown);
        assert_eq!(other.psd_sim_db, equal.psd_sim_db);
        assert_eq!(other.psd_as_flown_db, equal.psd_as_flown_db);
        assert_eq!(other.response_db, equal.response_db);
        assert_eq!(other.fit, equal.fit);
        assert_eq!(other.model, equal.model);
        // and the curves keep the axis of the log: to half its rate
        assert_eq!(other.response.freq, equal.response.freq);
    }
    assert_eq!(equal.response.freq.last(), Some(&500.0));

    // which rate each part was computed at, and why
    let rates = |o: &FilterSimOutput| -> Vec<(String, f64, RateSource)> {
        o.response
            .parts
            .iter()
            .map(|p| {
                let r = p.rate.unwrap();
                (p.label.clone(), r.hz, r.source)
            })
            .collect()
    };
    let part = |label: &str, hz, source| (label.to_string(), hz, source);
    assert_eq!(
        rates(&craft),
        [
            part("LPF1 PT1 100 Hz", 4000.0, RateSource::PidLoopRate),
            part("LPF2 PT1 300 Hz", 8000.0, RateSource::GyroRate),
        ]
    );
    assert_eq!(
        rates(&equal),
        [
            part("LPF1 PT1 100 Hz", 1000.0, RateSource::PidLoopRate),
            part("LPF2 PT1 300 Hz", 1000.0, RateSource::GyroRate),
        ]
    );
    assert_eq!(
        rates(&none),
        [
            part("LPF1 PT1 100 Hz", 1000.0, RateSource::LogRateNoLoopRate),
            part("LPF2 PT1 300 Hz", 1000.0, RateSource::LogRateNoLoopRate),
        ]
    );
    // every other firmware: both at the gyro rate
    let gyro = filter_sim_at(
        &s,
        Some(8000.0),
        Some(4000.0),
        GyroFilterLayout::AllAtGyroRate,
    );
    assert!(
        rates(&gyro)
            .iter()
            .all(|r| (r.1, r.2) == (8000.0, RateSource::GyroRate))
    );

    // equal rates give the curves of the fallback; the craft's own rates
    // give other curves, further apart the nearer to half the log rate
    for (a, b) in equal.response.parts.iter().zip(&none.response.parts) {
        assert_eq!(a.magnitude_db, b.magnitude_db);
        assert_eq!(a.delay_ms, b.delay_ms);
    }
    assert_eq!(
        equal.response.total.phase_deg,
        none.response.total.phase_deg
    );
    assert_eq!(equal.response.delay_100hz_ms, none.response.delay_100hz_ms);
    let gap = |i: usize| {
        (craft.response.total.magnitude_db[i] - none.response.total.magnitude_db[i]).abs()
    };
    let last = craft.response.freq.len() - 1;
    assert!(gap(0) < 1e-3, "{}", gap(0));
    assert!(gap(last) > 1.0, "{}", gap(last));
    assert!(craft.response.delay_100hz_ms > none.response.delay_100hz_ms + 0.05);

    // the printed delays, at 25 Hz unless asked otherwise: a PT1 at 100 Hz
    // on the 4 kHz loop delays a tone of 25 Hz by 1.5519 ms, a lag of
    // 13.97°; the one at 300 Hz on the 8 kHz gyro by 0.5288 ms, 4.759°
    let r = &craft.response;
    assert_eq!(r.delay_at_hz, 25.0);
    let printed = |p: &openpid_analysis::PartResponse| (p.printed.delay_ms, p.printed.lag_deg);
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-3;
    assert!(
        near(printed(&r.parts[0]), (1.5519, 13.967)),
        "{:?}",
        r.parts[0].printed
    );
    assert!(
        near(printed(&r.parts[1]), (0.5288, 4.7595)),
        "{:?}",
        r.parts[1].printed
    );
    assert_eq!(
        r.printed_lowpass.delay_ms,
        r.parts[0].printed.delay_ms + r.parts[1].printed.delay_ms
    );
    assert_eq!(r.printed_notch, PrintedDelay::default());
    assert_eq!(r.printed_total, r.printed_lowpass);
    assert!((r.total.printed.delay_ms - r.printed_total.delay_ms).abs() < 1e-9);
    assert!((r.printed_total.lag_deg - r.printed_total.delay_ms * 25.0 * 0.36).abs() < 1e-9);
    // at 1 Hz, the low end of the curve: the time constants 1 / (2π·fc),
    // 1.5915 and 0.5305 ms, whatever the rate
    let mut s1 = s.clone();
    s1.settings.loop_rates.gyro_hz = Some(8000.0);
    s1.settings.loop_rates.pid_hz = Some(4000.0);
    s1.settings.loop_rates.gyro_filter_layout = split;
    let low = FilterSimAnalysis
        .run(
            &AnalysisCtx {
                session: &s1,
                epoch: None,
            },
            &FilterSimParams {
                delay_at_hz: 1.0,
                ..FilterSimParams::default()
            },
        )
        .unwrap()
        .response;
    assert_eq!(low.delay_at_hz, 1.0);
    assert!((low.parts[0].printed.delay_ms - 1.5915).abs() < 1e-4);
    assert!((low.parts[1].printed.delay_ms - 0.5305).abs() < 1e-4);
    // the curves do not depend on where the delays are read
    assert_eq!(low.parts[0].delay_ms, r.parts[0].delay_ms);
    assert_eq!(low.total.magnitude_db, r.total.magnitude_db);
    // a frequency that is none is refused
    for bad in [0.0, -1.0, f64::NAN] {
        let refused = FilterSimAnalysis.run(
            &AnalysisCtx {
                session: &s1,
                epoch: None,
            },
            &FilterSimParams {
                delay_at_hz: bad,
                ..FilterSimParams::default()
            },
        );
        assert!(refused.is_err(), "{bad}");
    }

    // the output says so in its JSON, as the CLI prints it
    let v = serde_json::to_value(&craft).unwrap();
    let lpf1 = &v["response"]["parts"][0];
    assert_eq!(lpf1["rate"]["hz"], 4000.0);
    assert_eq!(lpf1["rate"]["source"], "pid_loop_rate");
    assert!(lpf1["printed"]["delay_ms"].is_number());
    assert!(lpf1["printed"]["lag_deg"].is_number());
    assert!(v["response"]["total"]["rate"].is_null());
    assert_eq!(v["response"]["delay_at_hz"], 25.0);
    assert!(v["response"]["printed_total"]["delay_ms"].is_number());
    let fallback = serde_json::to_value(&none).unwrap();
    assert_eq!(
        fallback["response"]["parts"][1]["rate"]["source"],
        "log_rate_no_loop_rate"
    );

    // settings written before the layout existed read as "every filter at
    // the gyro rate"
    let mut old = serde_json::to_value(&s.settings).unwrap();
    old["loop_rates"]
        .as_object_mut()
        .unwrap()
        .remove("gyro_filter_layout");
    let old: openpid_core::Settings = serde_json::from_value(old).unwrap();
    assert_eq!(
        old.loop_rates.gyro_filter_layout,
        GyroFilterLayout::AllAtGyroRate
    );
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
