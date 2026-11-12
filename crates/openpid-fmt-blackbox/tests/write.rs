//! The writer of Betaflight logs against the decoder: what is written is
//! read back, by our decoder and by the independent one, as exactly the
//! quantised values of the session.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use blackbox_log::frame::Frame as _;
use blackbox_log::prelude::*;
use openpid_core::session::EventKind;
use openpid_core::settings::{
    DynNotch, FilterChain, Lowpass, LowpassKind, Notch, PerAxis, PidGains, RpmNotch,
};
use openpid_core::{
    Axis, ChannelId, FlightSession, GyroStage, LogFormat, OpenOptions, Series, StickAxis,
};
use openpid_fmt_blackbox::write::{WriteError, WriteOptions, betaflight, typical_rates};
use openpid_fmt_blackbox::{BlackboxFormat, decode};
use openpid_sim::{Dropout, Noise, SetpointProfile, SignalSpec, ThrottleProfile, Tone};

/// A synthetic flight at `fs` with the loop rates a craft would run.
fn flight(spec: &SignalSpec) -> FlightSession {
    let mut s = spec.build();
    s.settings.loop_rates = typical_rates(spec.fs);
    s
}

fn spec(fs: f64, motors_spin: bool, seed: u64) -> SignalSpec {
    let mut noise = Noise {
        tones: vec![Tone {
            hz: 120.0,
            amplitude: 25.0,
        }],
        ..Noise::default()
    };
    if !motors_spin {
        noise.motor = None;
    }
    SignalSpec {
        fs,
        duration_s: 1.5,
        seed,
        noise,
        ..SignalSpec::default()
    }
}

fn read(bytes: &[u8]) -> FlightSession {
    let mut log = BlackboxFormat::new()
        .open(bytes, &OpenOptions::default())
        .unwrap();
    assert_eq!(log.sessions.len(), 1, "one log in the file");
    log.sessions.remove(0)
}

fn values(s: &FlightSession, id: &ChannelId) -> Vec<f64> {
    let c = s.channel(id).unwrap_or_else(|| panic!("no channel {id:?}"));
    assert!(c.data.all_valid(), "{id:?} has gaps");
    c.data.values().to_vec()
}

/// `written` holds, sample for sample, `expected(original)`.
fn assert_channel(
    original: &FlightSession,
    written: &FlightSession,
    id: &ChannelId,
    expected: impl Fn(f64) -> f64,
) {
    let a = values(original, id);
    let b = values(written, id);
    assert_eq!(a.len(), b.len(), "{id:?}: sample count");
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        let want = expected(*x);
        assert!(
            (want - y).abs() < 1e-9,
            "{id:?} sample {i}: wrote {x}, expected {want}, read {y}"
        );
    }
}

fn whole(v: f64) -> f64 {
    v.round_ties_even()
}

fn gyro(axis: Axis, stage: GyroStage) -> ChannelId {
    ChannelId::Gyro { axis, stage }
}

#[test]
fn a_written_log_reads_back_as_the_quantised_session() {
    for fs in [1000.0, 2000.0, 4000.0, 8000.0] {
        for motors_spin in [true, false] {
            let original = flight(&spec(fs, motors_spin, 7));
            let bytes = betaflight(&original, &WriteOptions::default()).unwrap();
            let back = read(&bytes);
            let name = format!("{fs} Hz, motors {motors_spin}");

            assert_eq!(back.len(), original.len(), "{name}: samples");
            let d = &back.diagnostics;
            assert_eq!(
                (d.corrupt_frames, d.desync_frames, d.skipped_bytes),
                (0, 0, 0),
                "{name}: the decoder met something it did not expect"
            );
            assert_channel(&original, &back, &ChannelId::Time, |t| t);
            for axis in Axis::ALL {
                // whole degrees per second, as the firmware logs them
                assert_channel(&original, &back, &gyro(axis, GyroStage::Filtered), whole);
                assert_channel(&original, &back, &gyro(axis, GyroStage::Raw), whole);
                assert_channel(&original, &back, &ChannelId::Setpoint { axis }, whole);
            }
            // tenths of a percent
            assert_channel(&original, &back, &ChannelId::Throttle, |t| {
                whole(t * 10.0) / 10.0
            });
            for idx in 0..4 {
                // DShot units between the outputs the header declares
                assert_channel(&original, &back, &ChannelId::Motor { idx }, |p| {
                    let raw = whole(158.0 + p / 100.0 * (2047.0 - 158.0));
                    (raw - 158.0) * 100.0 / (2047.0 - 158.0)
                });
                let rpm = ChannelId::MotorRpm { idx };
                if motors_spin {
                    // eRPM / 100 with 14 poles
                    assert_channel(&original, &back, &rpm, |r| {
                        whole(r * 14.0 / 200.0) * 200.0 / 14.0
                    });
                } else {
                    assert!(back.channel(&rpm).is_none(), "{name}: no eRPM when at rest");
                }
            }
            let rates = &back.settings.loop_rates;
            assert_eq!(rates.log_hz, Some(fs), "{name}: log rate");
            assert_eq!(
                rates.pid_hz,
                Some(if fs == 8000.0 { 8000.0 } else { 4000.0 }),
                "{name}: PID rate"
            );
            assert_eq!(rates.gyro_hz, Some(8000.0), "{name}: gyro rate");
        }
    }
}

#[test]
fn the_log_is_what_a_betaflight_craft_writes() {
    let original = flight(&spec(1000.0, true, 3));
    let bytes = betaflight(&original, &WriteOptions::default()).unwrap();
    let back = read(&bytes);

    assert_eq!(back.profile_id, "betaflight");
    assert_eq!(
        back.firmware.revision,
        "Betaflight 2025.12.2 (79065c96b) STM32F7X2"
    );
    assert!(back.firmware.is_at_least(2025, 12));
    assert_eq!(back.header("I interval"), Some("128"));
    assert_eq!(back.header("P interval"), Some("4"));
    assert_eq!(back.header("looptime"), Some("125"));
    assert_eq!(back.header("pid_process_denom"), Some("2"));
    assert_eq!(back.header("dshot_bidir"), Some("1"));

    // one I frame per 32 frames, the rest P; the beep, a slow frame, the end
    let frames = &back.diagnostics.frames;
    assert_eq!(frames["I"].valid, 47);
    assert_eq!(frames["P"].valid, 1500 - 47);
    assert_eq!(frames["S"].valid, 1);
    assert_eq!(frames["E"].valid, 3);
    let kinds: Vec<&EventKind> = back.events.iter().map(|e| &e.kind).collect();
    assert!(matches!(kinds[0], EventKind::SyncBeep { time_us: 0 }));
    assert!(matches!(kinds[1], EventKind::Disarm { reason: 4 }));
    assert!(matches!(kinds[2], EventKind::LogEnd));
    assert_eq!(back.timebase.intentionally_absent_iterations, 3 * 1499);
    assert!(bytes.ends_with(b"E\xffEnd of log\0"));

    // the sticks follow the setpoint through linear rates of 1000 deg/s
    assert_channel(
        &original,
        &back,
        &ChannelId::Setpoint { axis: Axis::Roll },
        whole,
    );
    let setpoint = values(&original, &ChannelId::Setpoint { axis: Axis::Roll });
    let stick = values(
        &back,
        &ChannelId::Stick {
            axis: StickAxis::Roll,
        },
    );
    for (sp, st) in setpoint.iter().zip(&stick) {
        assert!((whole(sp / 2.0) / 5.0 - st).abs() < 1e-9, "{sp} -> {st}");
    }
    // armed, in acro
    assert_eq!(back.aux[0].rows[0].values, [1, 0, 0, 1, 1]);
}

#[test]
fn the_independent_decoder_reads_the_same_values() {
    let original = flight(&spec(2000.0, true, 11));
    // the reference crate (0.4.3) predates the year.month versions and
    // refuses them; the frames are the same under a 4.5 name
    let options = WriteOptions {
        headers: vec![(
            "Firmware revision".into(),
            "Betaflight 4.5.1 (77d01ba3b) STM32F7X2".into(),
        )],
        ..WriteOptions::default()
    };
    let bytes = betaflight(&original, &options).unwrap();
    let mine = decode(&bytes).unwrap();
    let def = mine.headers.main_def();
    let inter = mine.headers.def(b'P').unwrap();

    let reference = blackbox_log::File::new(&bytes);
    assert_eq!(reference.log_count(), 1);
    let headers = reference.parse(0).unwrap().unwrap();
    let names: Vec<String> = headers
        .main_frame_def()
        .iter()
        .map(|f| f.name.to_string())
        .collect();
    // the reference lists every field but the time, which it keeps apart
    let index: Vec<usize> = names.iter().map(|n| def.index_of(n).unwrap()).collect();
    assert_eq!(names.len(), def.len() - 1, "the same fields");
    let time = def.index_of("time").unwrap();

    let mut parser = headers.data_parser();
    let (mut row, mut compared) = (0usize, 0u64);
    while let Some(event) = parser.next() {
        let ParserEvent::Main(frame) = event else {
            continue;
        };
        let intra = mine.intra[row];
        assert_eq!(
            mine.columns[time][row],
            frame.time_raw() as i64,
            "row {row}"
        );
        for (j, value) in frame.iter_raw().enumerate() {
            // the reference keeps the wrong history after an I frame and
            // ignores the sub-sampling (see `conformance.rs`): fields
            // predicted from two frames are compared on I frames only
            let from_history = [2, 3, 6].contains(&inter.predictors[index[j]]);
            if names[j] == "loopIteration" || (!intra && from_history) {
                continue;
            }
            assert_eq!(
                mine.columns[index[j]][row] as u32, value,
                "row {row}, field {}",
                names[j]
            );
            compared += 1;
        }
        row += 1;
    }
    assert_eq!(row, original.len(), "every frame was read");
    assert!(compared > 50_000, "compared only {compared} values");
}

/// A small deterministic generator, so the property test needs no crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn pick<T: Clone>(&mut self, of: &[T]) -> T {
        of[(self.next() * of.len() as f64) as usize].clone()
    }
}

#[test]
fn any_flight_survives_the_round_trip() {
    let mut rng = Rng(2026);
    for case in 0..40 {
        let fs = rng.pick(&[500.0, 1000.0, 2000.0, 3200.0, 4000.0, 8000.0]);
        let spec = SignalSpec {
            fs,
            duration_s: 0.05 + rng.next() * 0.8,
            seed: case,
            throttle: rng.pick(&[
                ThrottleProfile::Constant { percent: 37.5 },
                ThrottleProfile::Ramp {
                    from: 0.0,
                    to: 100.0,
                },
                ThrottleProfile::Sine {
                    mean: 50.0,
                    amplitude: 60.0,
                    hz: 3.0,
                },
            ]),
            setpoint: SetpointProfile::Steps {
                hold_min_s: 0.01,
                hold_max_s: 0.1,
                amplitude: rng.next() * 1900.0,
            },
            noise: Noise {
                white: rng.next() * 400.0,
                ..Noise::default()
            },
            jitter_us: rng.next() * 40.0,
            ..SignalSpec::default()
        };
        let original = flight(&spec);
        let options = WriteOptions {
            time_offset_us: (rng.next() * 3.0e9) as i64,
            disarm: rng.next() < 0.5,
            ..WriteOptions::default()
        };
        let bytes = betaflight(&original, &options).unwrap();
        let back = read(&bytes);
        let name = format!("case {case}: {spec:?}");
        assert_eq!(back.len(), original.len(), "{name}");
        assert_eq!(back.diagnostics.corrupt_frames, 0, "{name}");
        assert_eq!(back.diagnostics.skipped_bytes, 0, "{name}");
        let offset = options.time_offset_us as f64;
        assert_channel(&original, &back, &ChannelId::Time, |t| t + offset);
        for axis in Axis::ALL {
            assert_channel(&original, &back, &gyro(axis, GyroStage::Filtered), whole);
            assert_channel(&original, &back, &gyro(axis, GyroStage::Raw), whole);
            assert_channel(&original, &back, &ChannelId::Setpoint { axis }, whole);
        }
        assert_channel(&original, &back, &ChannelId::Throttle, |t| {
            whole(t.clamp(0.0, 100.0) * 10.0) / 10.0
        });
        let disarmed = back
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Disarm { .. }));
        assert_eq!(disarmed, options.disarm, "{name}");
    }
}

#[test]
fn a_gap_pauses_the_log_and_it_resumes_on_an_i_frame() {
    let spec = SignalSpec {
        dropouts: vec![Dropout {
            start_s: 0.5,
            len_s: 0.1,
        }],
        ..spec(1000.0, true, 5)
    };
    let original = flight(&spec);
    let bytes = betaflight(&original, &WriteOptions::default()).unwrap();
    let back = read(&bytes);

    // samples 500..600 have no gyro; the log resumes at the next I frame,
    // every 32 samples: sample 608
    let logged: Vec<usize> = (0..500).chain(608..1500).collect();
    assert_eq!(back.len(), logged.len());
    assert_eq!(back.diagnostics.corrupt_frames, 0);
    assert_eq!(back.diagnostics.skipped_bytes, 0);
    let resumes: Vec<&EventKind> = back
        .events
        .iter()
        .map(|e| &e.kind)
        .filter(|k| matches!(k, EventKind::LoggingResume { .. }))
        .collect();
    assert_eq!(resumes.len(), 1);
    assert!(matches!(
        resumes[0],
        EventKind::LoggingResume {
            iteration: 2432,
            time_us: 608_000
        }
    ));
    let id = gyro(Axis::Roll, GyroStage::Filtered);
    let wrote = &original.channel(&id).unwrap().data;
    let read = values(&back, &id);
    let time = values(&back, &ChannelId::Time);
    for (row, sample) in logged.into_iter().enumerate() {
        assert_eq!(time[row], sample as f64 * 1000.0, "row {row}");
        assert_eq!(read[row], whole(wrote.get(sample).unwrap()), "row {row}");
    }
}

#[test]
fn a_log_that_starts_in_a_gap_starts_at_its_first_sample() {
    let spec = SignalSpec {
        dropouts: vec![Dropout {
            start_s: 0.0,
            len_s: 0.01,
        }],
        ..spec(1000.0, false, 5)
    };
    let back = read(&betaflight(&flight(&spec), &WriteOptions::default()).unwrap());
    assert_eq!(back.len(), 1490);
    assert_eq!(values(&back, &ChannelId::Time)[0], 10_000.0);
    assert_eq!(values(&back, &ChannelId::LoopIteration)[0], 0.0);
    assert!(
        !back
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::LoggingResume { .. }))
    );
}

fn gains(p: f64, i: f64, d: f64, d_max: f64, f: f64) -> PidGains {
    PidGains {
        p,
        i,
        d,
        f: Some(f),
        d_min: None,
        d_max: Some(d_max),
    }
}

fn lowpass(kind: LowpassKind, cutoff_hz: f64) -> Lowpass {
    Lowpass {
        kind: Some(kind),
        cutoff_hz,
        dyn_min_hz: None,
        dyn_max_hz: None,
        dyn_expo: 0.5,
    }
}

#[test]
fn the_settings_of_the_session_are_the_settings_of_the_log() {
    let mut original = flight(&spec(4000.0, true, 1));
    let pid = PerAxis {
        roll: gains(51.0, 92.0, 33.0, 44.0, 130.0),
        pitch: gains(55.0, 97.0, 37.0, 50.0, 135.0),
        yaw: gains(48.0, 88.0, 1.0, 2.0, 110.0),
    };
    original.settings.pid = Some(pid);
    original.settings.gyro_filters = FilterChain {
        lowpass1: Some(Lowpass {
            dyn_min_hz: Some(200.0),
            dyn_max_hz: Some(450.0),
            dyn_expo: 0.7,
            ..lowpass(LowpassKind::Pt2, 0.0)
        }),
        lowpass2: Some(Lowpass {
            dyn_expo: 0.7,
            ..lowpass(LowpassKind::Biquad, 310.0)
        }),
        notches: vec![Notch {
            center_hz: 180.0,
            cutoff_hz: 150.0,
        }],
        dyn_notch: Some(DynNotch {
            min_hz: 90.0,
            max_hz: 550.0,
            q: 3.5,
            count: Some(2),
        }),
        rpm_notch: Some(RpmNotch {
            harmonics: 2,
            q: 6.0,
            min_hz: 80.0,
            fade_range_hz: 40.0,
            weights: vec![1.0, 0.5],
            lpf_hz: 120.0,
        }),
    };
    original.settings.dterm_filters = FilterChain {
        lowpass1: Some(lowpass(LowpassKind::Pt3, 95.0)),
        lowpass2: None,
        notches: vec![Notch {
            center_hz: 260.0,
            cutoff_hz: 200.0,
        }],
        dyn_notch: None,
        rpm_notch: None,
    };
    original.settings.motor.poles = Some(12);
    original.settings.motor.output_low = Some(48.0);
    original.settings.motor.output_high = Some(2047.0);
    original.settings.craft_name = Some("Probe 7".into());
    original.settings.board = Some("OPID BENCH".into());
    original.settings.log_start = Some("2026-11-11T10:00:00.000+00:00".into());
    let options = WriteOptions {
        headers: vec![
            ("debug_mode".into(), "6".into()),
            ("probe_id".into(), "spectrum-01".into()),
        ],
        ..WriteOptions::default()
    };
    let back = read(&betaflight(&original, &options).unwrap());
    let s = &back.settings;

    assert_eq!(s.pid, Some(pid));
    assert_eq!(s.gyro_filters, original.settings.gyro_filters);
    // the firmware has one expo for both low-passes; the D term keeps the default
    let mut dterm = original.settings.dterm_filters.clone();
    dterm.lowpass1.as_mut().unwrap().dyn_expo = 0.5;
    assert_eq!(s.dterm_filters, dterm);
    assert_eq!(s.motor.poles, Some(12));
    assert_eq!(s.motor.output_low, Some(48.0));
    assert_eq!(s.craft_name.as_deref(), Some("Probe 7"));
    assert_eq!(s.board.as_deref(), Some("OPID BENCH"));
    assert_eq!(
        s.log_start.as_deref(),
        Some("2026-11-11T10:00:00.000+00:00")
    );
    assert_eq!(s.debug_mode.as_ref().map(|d| d.code), Some(6));
    assert_eq!(back.header("probe_id"), Some("spectrum-01"));
    // 12 poles and the lowest output of 48 are used for the values too
    let rpm = ChannelId::MotorRpm { idx: 0 };
    assert_channel(&original, &back, &rpm, |r| {
        whole(r * 12.0 / 200.0) * 200.0 / 12.0
    });
    assert_channel(&original, &back, &ChannelId::Motor { idx: 0 }, |p| {
        (whole(48.0 + p / 100.0 * 1999.0) - 48.0) * 100.0 / 1999.0
    });
}

#[test]
fn a_filter_the_session_does_not_have_is_logged_as_off() {
    // the simulator's flight has one PT1 on the gyro and nothing else
    let original = flight(&spec(1000.0, false, 1));
    let back = read(&betaflight(&original, &WriteOptions::default()).unwrap());
    let g = &back.settings.gyro_filters;
    assert_eq!(g.lowpass1.as_ref().map(|l| l.cutoff_hz), Some(100.0));
    assert_eq!(
        g.lowpass1.as_ref().and_then(|l| l.kind.clone()),
        Some(LowpassKind::Pt1)
    );
    assert_eq!(g.lowpass1.as_ref().and_then(|l| l.dyn_min_hz), None);
    assert!(g.lowpass2.is_none());
    assert!(g.notches.is_empty());
    assert!(g.dyn_notch.is_none(), "{:?}", g.dyn_notch);
    assert!(g.rpm_notch.is_none());
    assert_eq!(back.settings.dterm_filters, FilterChain::default());
    assert_eq!(back.header("dshot_bidir"), Some("0"));
    // gains the session does not name stay the stock ones
    assert_eq!(back.settings.pid.as_ref().map(|p| p.roll.p), Some(45.0));
}

#[test]
fn channels_of_the_session_are_used_where_it_has_them() {
    let mut original = flight(&spec(1000.0, false, 9));
    let n = original.len();
    let ramp =
        |from: f64, step: f64| Series::from_vec((0..n).map(|i| from + step * i as f64).collect());
    let mut put = |id: ChannelId, data: Series| {
        let mut c = original.channel(&ChannelId::Throttle).unwrap().clone();
        c.id = id;
        c.data = data;
        original.channels.insert(c);
    };
    for ch in 0..3u8 {
        put(
            ChannelId::RcCommand { ch },
            ramp(-400.0 + f64::from(ch), 0.25),
        );
    }
    put(ChannelId::RcCommand { ch: 3 }, ramp(1100.0, 0.5));
    put(
        ChannelId::PidTerm {
            axis: Axis::Pitch,
            term: openpid_core::PidTerm::I,
        },
        ramp(-30.0, 0.07),
    );
    put(
        ChannelId::Battery(openpid_core::BatteryField::Voltage),
        ramp(16.8, -0.0005),
    );
    put(
        ChannelId::Battery(openpid_core::BatteryField::Current),
        ramp(2.0, 0.01),
    );
    for idx in 0..4u8 {
        put(ChannelId::Erpm { idx }, ramp(50.0, 0.1));
    }
    let back = read(&betaflight(&original, &WriteOptions::default()).unwrap());
    for ch in 0..4u8 {
        assert_channel(&original, &back, &ChannelId::RcCommand { ch }, whole);
    }
    let i_term = ChannelId::PidTerm {
        axis: Axis::Pitch,
        term: openpid_core::PidTerm::I,
    };
    assert_channel(&original, &back, &i_term, whole);
    let cents = |v: f64| whole(v * 100.0) / 100.0;
    let voltage = ChannelId::Battery(openpid_core::BatteryField::Voltage);
    assert_channel(&original, &back, &voltage, cents);
    let current = ChannelId::Battery(openpid_core::BatteryField::Current);
    assert_channel(&original, &back, &current, cents);
    assert_channel(&original, &back, &ChannelId::Erpm { idx: 2 }, whole);
}

#[test]
fn what_cannot_be_logged_is_refused_with_the_reason() {
    let good = flight(&spec(1000.0, false, 1));
    let write = |s: &FlightSession| betaflight(s, &WriteOptions::default());

    let mut s = good.clone();
    s.channels = openpid_core::session::ChannelSet::new();
    assert_eq!(write(&s), Err(WriteError::MissingChannel("time")));

    let mut s = good.clone();
    let mut only_time = openpid_core::session::ChannelSet::new();
    only_time.insert(s.channel(&ChannelId::Time).unwrap().clone());
    s.channels = only_time;
    assert_eq!(write(&s), Err(WriteError::MissingChannel("filtered gyro")));

    let all_gone = SignalSpec {
        dropouts: vec![Dropout {
            start_s: 0.0,
            len_s: 10.0,
        }],
        ..spec(1000.0, false, 1)
    };
    assert_eq!(write(&flight(&all_gone)), Err(WriteError::Empty));

    // a PID loop that is not a whole multiple of the log rate
    let mut s = good.clone();
    s.settings.loop_rates.pid_hz = Some(1500.0);
    assert!(matches!(write(&s), Err(WriteError::Rate { .. })));
    // a log faster than its loop
    let mut s = good.clone();
    s.settings.loop_rates.pid_hz = Some(500.0);
    let error = write(&s).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("1000 Hz log of a 500 Hz PID loop")
    );
    // a gyro that is not a whole multiple of the PID loop
    let mut s = good.clone();
    s.settings.loop_rates.gyro_hz = Some(6000.0);
    assert!(matches!(write(&s), Err(WriteError::Rate { .. })));

    let early = WriteOptions {
        time_offset_us: -1,
        ..WriteOptions::default()
    };
    assert_eq!(betaflight(&good, &early), Err(WriteError::Time(-1)));
}

/// The firmware's clock is 32 bits of microseconds: 71.6 minutes after
/// power-up it starts again from zero, and so does the log.
#[test]
fn the_clock_of_a_long_flight_wraps_as_the_firmware_s_does() {
    let original = flight(&spec(1000.0, false, 4));
    // armed half a second before the clock wraps
    let offset = (1i64 << 32) - 500_000;
    let options = WriteOptions {
        time_offset_us: offset,
        ..WriteOptions::default()
    };
    let bytes = betaflight(&original, &options).unwrap();
    let raw = decode(&bytes).unwrap();
    assert_eq!(raw.len(), original.len());
    assert_eq!(raw.stats.corrupt_total() + raw.stats.desync_total(), 0);
    // the decoder follows the clock across the wrap
    let time = raw.column("time").unwrap();
    assert_eq!(time[0], offset);
    assert_eq!(time[499], offset + 499_000);
    assert_eq!(time[500], 1i64 << 32);
    assert_eq!(time[1499], offset + 1_499_000);
    // on the card the 501st frame starts again from zero: its time takes
    // one byte where the frame before needed five
    let back = read(&bytes);
    assert_channel(&original, &back, &ChannelId::Time, |t| t + offset as f64);
    // a whole flight after the wrap is logged with the times the clock shows
    let later = WriteOptions {
        time_offset_us: (1i64 << 32) + 7_000_000,
        ..WriteOptions::default()
    };
    let raw = decode(&betaflight(&original, &later).unwrap()).unwrap();
    assert_eq!(raw.column("time").unwrap()[0], 7_000_000);
}

#[test]
fn loop_rates_come_from_the_time_stamps_when_the_settings_have_none() {
    let mut s = spec(2000.0, false, 1).build();
    s.settings.loop_rates = openpid_core::settings::LoopRates::default();
    let back = read(&betaflight(&s, &WriteOptions::default()).unwrap());
    assert_eq!(back.settings.loop_rates.log_hz, Some(2000.0));
    assert_eq!(back.settings.loop_rates.pid_hz, Some(2000.0));
    assert_eq!(back.header("P interval"), Some("1"));
    assert_eq!(back.header("I interval"), Some("64"));
}

#[test]
fn typical_loop_rates() {
    let rates = |hz: f64| {
        let r = typical_rates(hz);
        (r.gyro_hz.unwrap(), r.pid_hz.unwrap(), r.log_hz.unwrap())
    };
    assert_eq!(rates(4000.0), (8000.0, 4000.0, 4000.0));
    assert_eq!(rates(1000.0), (8000.0, 4000.0, 1000.0));
    assert_eq!(rates(125.0), (8000.0, 4000.0, 125.0));
    assert_eq!(rates(8000.0), (8000.0, 8000.0, 8000.0));
    assert_eq!(rates(3200.0), (3200.0, 3200.0, 3200.0));
    // 4000 / 800 = 5 is not a power of two
    assert_eq!(rates(800.0), (800.0, 800.0, 800.0));
    assert_eq!(rates(62.5), (62.5, 62.5, 62.5));
}
