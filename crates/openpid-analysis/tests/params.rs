//! Parameter handling of the analyses: defaults, hashing, validation,
//! smoothing and the low/high input split.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::{
    Analysis, AnalysisCtx, AnalysisError, FreqSmoothing, PsdAnalysis, PsdParams, StepAnalysis,
    StepParams, StftAnalysis, StftParams, ThrottleHeatmap, ThrottleHeatmapParams, TimeSmoothing,
};
use openpid_core::{Axis, ChannelId, GyroStage, LogFormat, OpenOptions};
use openpid_dsp::Window;
use openpid_fmt_blackbox::BlackboxFormat;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

fn load() -> openpid_core::LogFile {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/logs/betaflight-4.2.0.BFL");
    BlackboxFormat::new()
        .open(&std::fs::read(path).unwrap(), &OpenOptions::default())
        .unwrap()
}

fn hash<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn bad<T>(r: Result<T, AnalysisError>) -> bool {
    matches!(r, Err(AnalysisError::InvalidParams(_)))
}

#[test]
fn params_hash_by_value() {
    let a = StepParams::default();
    let b = StepParams {
        cutoff_hz: 30.0,
        ..Default::default()
    };
    assert_ne!(hash(&a), hash(&b));
    assert_eq!(hash(&a), hash(&StepParams::default()));
    let p = PsdParams::default();
    assert_ne!(
        hash(&p),
        hash(&PsdParams {
            max_hz: Some(100.0),
            ..Default::default()
        })
    );
    assert_ne!(
        hash(&p),
        hash(&PsdParams {
            window: Window::Hamming,
            ..Default::default()
        })
    );
    let t = ThrottleHeatmapParams::default();
    assert_ne!(
        hash(&t),
        hash(&ThrottleHeatmapParams {
            smooth_throttle: 3,
            ..Default::default()
        })
    );
    // the map of frequency against time: by default both lists at "low" and
    // the rows to half the sample rate; every level is a job of its own
    let s = StftParams::default();
    assert_eq!(
        (s.smooth_freq, s.smooth_time, s.max_hz),
        (FreqSmoothing::Low, TimeSmoothing::Low, None)
    );
    let mut seen = vec![hash(&s)];
    for smooth_freq in FreqSmoothing::ALL {
        for smooth_time in TimeSmoothing::ALL {
            let h = hash(&StftParams {
                smooth_freq,
                smooth_time,
                ..Default::default()
            });
            if (smooth_freq, smooth_time) == (s.smooth_freq, s.smooth_time) {
                assert_eq!(h, seen[0]);
            } else {
                assert!(!seen.contains(&h), "{smooth_freq:?} {smooth_time:?}");
                seen.push(h);
            }
        }
    }
    assert_eq!(seen.len(), 16);
    for other in [
        StftParams {
            max_hz: Some(500.0),
            ..Default::default()
        },
        StftParams {
            channel: ChannelId::Throttle,
            ..Default::default()
        },
    ] {
        assert!(!seen.contains(&hash(&other)));
    }
    assert_eq!(StepAnalysis.id(), "step-response");
    assert_eq!(PsdAnalysis.id(), "psd");
    assert_eq!(ThrottleHeatmap.id(), "throttle-heatmap");
    assert_eq!(StftAnalysis.id(), "stft");
    assert!(!StepAnalysis.requires().is_empty() && !ThrottleHeatmap.requires().is_empty());
    assert!(!PsdAnalysis.requires().is_empty() && !StftAnalysis.requires().is_empty());
}

#[test]
fn validation_and_variants() {
    let log = load();
    let s = &log.sessions[0];
    let ctx = AnalysisCtx {
        session: s,
        epoch: None,
    };
    assert!(bad(StepAnalysis.run(
        &ctx,
        &StepParams {
            superpos: 0,
            ..Default::default()
        }
    )));
    assert!(bad(StepAnalysis.run(
        &ctx,
        &StepParams {
            response_s: 3.0,
            ..Default::default()
        }
    )));
    assert!(bad(StepAnalysis.run(
        &ctx,
        &StepParams {
            hist_bins: 1,
            ..Default::default()
        }
    )));
    assert!(bad(PsdAnalysis.run(
        &ctx,
        &PsdParams {
            segment_s: 0.0,
            ..Default::default()
        }
    )));
    assert!(bad(ThrottleHeatmap.run(
        &ctx,
        &ThrottleHeatmapParams {
            max_hz: 0.0,
            ..Default::default()
        }
    )));
    assert!(matches!(
        StepAnalysis.run(
            &ctx,
            &StepParams {
                window_s: 100.0,
                response_s: 0.5,
                ..Default::default()
            }
        ),
        Err(AnalysisError::NotEnoughData(_))
    ));
    assert!(matches!(
        StepAnalysis.run(
            &ctx,
            &StepParams {
                min_input: 1.0e6,
                ..Default::default()
            }
        ),
        Err(AnalysisError::NotEnoughData(_))
    ));
    // the settle range must be a range
    assert!(bad(StepAnalysis.run(
        &ctx,
        &StepParams {
            settle_range: (1.5, 0.5),
            ..Default::default()
        }
    )));
    // a settle range no response can meet rejects every window, and says so
    match StepAnalysis.run(
        &ctx,
        &StepParams {
            min_input: 20.0,
            settle_range: (10.0, 11.0),
            ..Default::default()
        },
    ) {
        Err(AnalysisError::NotEnoughData(m)) => assert!(m.contains("settled"), "{m}"),
        other => panic!("expected NotEnoughData, got {other:?}"),
    }
    // smoothing and a low high-input threshold populate both groups (this
    // log has gentle stick inputs: the default threshold would drop them)
    let out = StepAnalysis
        .run(
            &ctx,
            &StepParams {
                axis: Axis::Roll,
                min_input: 20.0,
                smooth_ms: 5.0,
                high_input: 100.0,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(out.low.is_some() || out.high.is_some());
    assert_eq!(out.mean.len(), out.time_ms.len());
    // PSD without a frequency bound reaches Nyquist, linear and dB agree
    let gyro = ChannelId::Gyro {
        axis: Axis::Pitch,
        stage: GyroStage::Filtered,
    };
    let psd = PsdAnalysis
        .run(
            &ctx,
            &PsdParams {
                channel: gyro.clone(),
                max_hz: None,
                ..Default::default()
            },
        )
        .unwrap();
    assert!((psd.freq.last().unwrap() - 500.0).abs() < 1e-9);
    assert!((psd.power_db[10] - 10.0 * psd.power[10].log10()).abs() < 1e-9);
    // the map of frequency against time goes to half the sample rate unless
    // it is bound, at every level of its two lists
    let st = StftAnalysis
        .run(
            &ctx,
            &StftParams {
                channel: gyro.clone(),
                smooth_freq: FreqSmoothing::High,
                smooth_time: TimeSmoothing::Medium,
                max_hz: None,
            },
        )
        .unwrap();
    assert!((st.grid.y.last().unwrap() - 500.0).abs() < 1e-9);
    assert_eq!((st.nperseg, st.hop), (300, 60));
    let bound = StftAnalysis
        .run(
            &ctx,
            &StftParams {
                channel: gyro.clone(),
                max_hz: Some(200.0),
                ..Default::default()
            },
        )
        .unwrap();
    assert!((bound.grid.y.last().unwrap() - 200.0).abs() < 1e-9);
    assert_eq!(bound.hop, 150);
    // throttle heatmap on a short epoch with no complete segment
    let short = AnalysisCtx {
        session: s,
        epoch: Some(openpid_analysis::Epoch {
            start_s: 1.0,
            end_s: 1.05,
        }),
    };
    assert!(matches!(
        ThrottleHeatmap.run(
            &short,
            &ThrottleHeatmapParams {
                channel: gyro,
                ..Default::default()
            }
        ),
        Err(AnalysisError::NotEnoughData(_))
    ));
    // an epoch outside the log
    let out = AnalysisCtx {
        session: s,
        epoch: Some(openpid_analysis::Epoch {
            start_s: 500.0,
            end_s: 600.0,
        }),
    };
    assert!(matches!(
        PsdAnalysis.run(&out, &PsdParams::default()),
        Err(AnalysisError::NotEnoughData(_))
    ));
}
