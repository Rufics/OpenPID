//! The chain model against real logs: on the 2025.12 fixture (which logs
//! `gyroUnfilt`) the simulated gyro must land close to the logged filtered
//! one; on 4.5.0 the model must build from the headers.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::{Axis, ChannelId, GyroStage, LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;
use openpid_sim::{ChainKind, ChainModel, Drive};
use std::path::PathBuf;

fn open(name: &str) -> openpid_core::FlightSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/logs")
        .join(name);
    let data = std::fs::read(&path).unwrap();
    assert!(
        !data.starts_with(b"version https://git-lfs"),
        "{}: Git LFS pointer, run `git lfs pull`",
        path.display()
    );
    let log = BlackboxFormat::new()
        .open(&data, &OpenOptions::default())
        .unwrap();
    log.sessions
        .into_iter()
        .max_by_key(openpid_core::FlightSession::len)
        .unwrap()
}

fn rms(v: &[f64]) -> f64 {
    (v.iter().map(|y| y * y).sum::<f64>() / v.len() as f64).sqrt()
}

#[test]
fn betaflight_2025_12_simulated_gyro_matches_the_logged_filtered_gyro() {
    let s = open("betaflight-2025.12.2-btfl_027.bbl");
    let fs = s.timebase.sample_rate_hz();
    let model = ChainModel::from_settings(&s.settings, s.firmware.version, ChainKind::Gyro, fs);
    let lines = model.describe();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("RPM notch: 3 harmonics")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("lowpass 1: PT1 250–500 Hz by throttle (expo 0.5)")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "lowpass 2: PT1 500 Hz"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "dynamic notch: 1 × Q 5.0 in 100–600 Hz"),
        "{lines:?}"
    );

    let throttle: Vec<f64> = s
        .channel(&ChannelId::Throttle)
        .unwrap()
        .data
        .values()
        .iter()
        .map(|t| t / 100.0)
        .collect();
    let rpm: Vec<Vec<f64>> = (0..4)
        .map(|idx| {
            s.channel(&ChannelId::MotorRpm { idx })
                .unwrap()
                .data
                .values()
                .to_vec()
        })
        .collect();
    let drive = Drive {
        throttle: Some(&throttle),
        rpm: rpm.iter().map(Vec::as_slice).collect(),
        dyn_centers: None,
    };
    for axis in [Axis::Roll, Axis::Pitch, Axis::Yaw] {
        let raw = s
            .channel(&ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            })
            .unwrap()
            .data
            .values();
        let filtered = s
            .channel(&ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            })
            .unwrap()
            .data
            .values();
        let sim = model.run(raw, &drive);
        assert_eq!(sim.len(), raw.len());
        // skip the first second: filters and trackers settle
        let from = fs as usize;
        let residual: Vec<f64> = sim[from..]
            .iter()
            .zip(&filtered[from..])
            .map(|(a, b)| a - b)
            .collect();
        let unfiltered: Vec<f64> = raw[from..]
            .iter()
            .zip(&filtered[from..])
            .map(|(a, b)| a - b)
            .collect();
        let level = rms(&filtered[from..]);
        let (r_sim, r_raw) = (rms(&residual) / level, rms(&unfiltered) / level);
        eprintln!(
            "{axis:?}: sim residual {r_sim:.4}, raw residual {r_raw:.4} (relative to filtered RMS {level:.2} deg/s)"
        );
        assert!(
            r_sim < 0.5 * r_raw,
            "{axis:?}: the model explains the filtering"
        );
        assert!(r_sim < 0.25, "{axis:?}: residual {r_sim}");
    }
}

#[test]
fn betaflight_4_5_0_model_builds_from_the_headers() {
    let s = open("betaflight-4.5.0-gps-rescue.bbl");
    let fs = s.timebase.sample_rate_hz();
    let model = ChainModel::from_settings(&s.settings, s.firmware.version, ChainKind::Gyro, fs);
    let lines = model.describe();
    // gyro_lpf1_static_hz 0 and dyn 0,500: no first low-pass
    assert!(model.lowpass1.is_none(), "{lines:?}");
    assert!(
        lines.iter().any(|l| l == "lowpass 2: PT2 137 Hz"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "dynamic notch: 2 × Q 5.0 in 150–500 Hz"),
        "{lines:?}"
    );
    let r = model.rpm_notch.as_ref().unwrap();
    assert_eq!(r.weights, vec![1.0, 0.25, 1.0]);
    assert_eq!(r.fade_range_hz, 50.0);
    let dterm = ChainModel::from_settings(&s.settings, s.firmware.version, ChainKind::Dterm, fs);
    let d = dterm.lowpass1.as_ref().unwrap().dynamic.as_ref().unwrap();
    assert_eq!((d.min_hz, d.max_hz), (93.0, 187.0));
    assert!((d.expo - 0.7).abs() < 1e-12);
    // the static part has a usable response curve
    let g = dterm.static_response(93.0).norm();
    assert!(g < 0.8 && g > 0.5, "{g}");
}
