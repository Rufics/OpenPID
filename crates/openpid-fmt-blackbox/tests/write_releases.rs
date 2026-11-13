//! The writer against the decoder for every release it writes: a flight
//! with every field, frame and event a flight controller can log goes out
//! and comes back, at several rates, in normal and in high resolution.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

mod common;

use openpid_core::modes::{FlightMode, ModeChange};
use openpid_core::session::{AdjustmentValue, AuxRow, AuxTable, Event, EventKind};
use openpid_core::settings::{
    DebugMode, FilterChain, LoopRates, Lowpass, LowpassKind, PerAxis, PidGains, RpmNotch,
};
use openpid_core::{
    Axis, BatteryField, Channel, ChannelId, FlightSession, GyroStage, LogFormat, OpenOptions,
    Origin, PidTerm, Series, Unit,
};
use openpid_fmt_blackbox::write::{Target, WriteError, WriteOptions, betaflight};
use openpid_fmt_blackbox::{BlackboxFormat, DecodedLog, decode};
use openpid_sim::{Dropout, SignalSpec};

fn read(bytes: &[u8]) -> FlightSession {
    let mut log = BlackboxFormat::new()
        .open(bytes, &OpenOptions::default())
        .unwrap();
    assert_eq!(log.sessions.len(), 1, "one log in the file");
    log.sessions.remove(0)
}

fn whole(v: f64) -> f64 {
    v.round_ties_even()
}

fn custom(name: &str) -> ChannelId {
    ChannelId::Custom(name.to_string())
}

fn rates(gyro_hz: f64, pid_hz: f64, log_hz: f64) -> LoopRates {
    LoopRates {
        gyro_hz: Some(gyro_hz),
        pid_hz: Some(pid_hz),
        log_hz: Some(log_hz),
    }
}

fn gains(d: f64) -> PidGains {
    PidGains {
        p: 45.0,
        i: 80.0,
        d,
        f: Some(120.0),
        d_min: None,
        d_max: None,
    }
}

/// Puts a channel with `values(sample)` into the session.
fn put(s: &mut FlightSession, id: ChannelId, values: impl Fn(usize) -> f64) {
    let n = s.len();
    s.channels.insert(Channel {
        name: id.key(),
        id,
        unit: Unit::Raw,
        data: Series::from_vec((0..n).map(values).collect()),
        origin: Origin::Derived {
            from: Vec::new(),
            by: "test".into(),
        },
    });
}

/// A smooth signal that is different for every `seed` and has fractions.
fn wave(seed: usize, amplitude: f64) -> impl Fn(usize) -> f64 {
    move |i| amplitude * ((i as f64 * 0.013 + seed as f64).sin() + 0.37 * (seed as f64).cos())
}

/// The columns of the GPS frames of the newest release, and of its home
/// frames: a session carries what the reader of such a log would give it.
const FIX: [&str; 11] = [
    "time",
    "GPS_numSat",
    "GPS_coord[0]",
    "GPS_coord[1]",
    "GPS_altitude",
    "GPS_speed",
    "GPS_ground_course",
    "GPS_velned[0]",
    "GPS_velned[1]",
    "GPS_velned[2]",
    "GPS_time",
];
const HOME: [&str; 4] = [
    "GPS_home[0]",
    "GPS_home[1]",
    "GPS_home[2]",
    "GPS_home_epoch",
];

/// A flight with everything: every channel some release logs, GPS fixes, a
/// home point that moves once, mode changes and adjustments in flight.
fn everything(log_hz: f64, duration_s: f64) -> FlightSession {
    let spec = SignalSpec {
        fs: log_hz,
        duration_s,
        seed: 21,
        ..SignalSpec::default()
    };
    let mut s = spec.build();
    let n = s.len();
    let mut seed = 0;
    let mut next = || {
        seed += 1;
        seed
    };
    for axis in Axis::ALL {
        for term in [PidTerm::P, PidTerm::I, PidTerm::D, PidTerm::F, PidTerm::S] {
            put(
                &mut s,
                ChannelId::PidTerm { axis, term },
                wave(next(), 180.0),
            );
        }
        put(&mut s, ChannelId::Acc { axis }, wave(next(), 1.5));
        let quaternion = format!("imuQuaternion[{}]", axis.index());
        put(&mut s, custom(&quaternion), wave(next(), 0.6));
        put(
            &mut s,
            custom(&format!("magADC[{}]", axis.index())),
            wave(next(), 400.0),
        );
    }
    for ch in 0..3u8 {
        put(&mut s, ChannelId::RcCommand { ch }, wave(next(), 300.0));
    }
    put(&mut s, ChannelId::RcCommand { ch: 3 }, |i| {
        1500.0 + 400.0 * (i as f64 * 0.002).sin()
    });
    put(&mut s, ChannelId::Battery(BatteryField::Voltage), |i| {
        16.8 - i as f64 * 0.0004
    });
    put(
        &mut s,
        ChannelId::Battery(BatteryField::Current),
        wave(next(), 9.0),
    );
    put(&mut s, ChannelId::Rssi, |i| 900.0 + (i % 50) as f64);
    for idx in 0..8u8 {
        put(&mut s, ChannelId::Debug { idx }, wave(next(), 700.0));
        put(&mut s, custom(&format!("servo[{idx}]")), |i| {
            1500.0 + f64::from(idx) * 10.0 + (i % 90) as f64
        });
    }
    for idx in 0..4u8 {
        put(&mut s, ChannelId::Erpm { idx }, |i| {
            120.0 + f64::from(idx) + (i % 200) as f64
        });
    }
    put(&mut s, custom("baroAlt"), wave(next(), 5000.0));
    put(&mut s, custom("surfaceRaw"), |i| (i % 300) as f64);
    put(&mut s, custom("pitot[0]"), |i| 1200.0 + (i % 40) as f64);
    put(&mut s, custom("pitot[1]"), wave(next(), 90.0));

    let time = s.channel(&ChannelId::Time).unwrap().data.values().to_vec();
    let fix = |at: usize, k: i64| AuxRow {
        sample_index: at,
        values: vec![
            time[at - 1] as i64 + 30,
            12 + k % 3,
            473_977_000 + k * 37,
            85_455_000 - k * 53,
            4500 + k * 7,
            900 + k * 11,
            1800 + k,
            120 - k * 9,
            -40 + k * 5,
            7 - k,
            1_780_000_000 + k,
        ],
    };
    let fixes: Vec<usize> = (0..12).map(|k| 1 + k * (n / 13)).collect();
    s.aux.push(AuxTable {
        kind: "gps".into(),
        columns: FIX.map(str::to_string).to_vec(),
        rows: fixes
            .iter()
            .enumerate()
            .map(|(k, at)| fix(*at, k as i64))
            .collect(),
    });
    // the home point is set at arming and once more in flight
    let home = |at: usize, shift: i64| AuxRow {
        sample_index: at,
        values: vec![473_977_100 + shift, 85_455_100 - shift, 4400, 1_780_000_000],
    };
    s.aux.push(AuxTable {
        kind: "gps_home".into(),
        columns: HOME.map(str::to_string).to_vec(),
        rows: vec![home(fixes[0], 0), home(fixes[6], 2500)],
    });

    let modes = |at: usize, modes: &[FlightMode]| ModeChange {
        sample_index: at,
        modes: modes.to_vec(),
    };
    // the changes on odd samples, which are P frames at every rate here: the
    // firmware logs a change with the next P frame
    let angle = [FlightMode::Arm, FlightMode::Angle, FlightMode::AirMode];
    s.flight_modes = vec![
        modes(0, &[FlightMode::Arm, FlightMode::AirMode]),
        modes((n / 3) | 1, &angle),
        modes((2 * n / 3) | 1, &[FlightMode::Arm, FlightMode::AirMode]),
    ];
    s.events = vec![
        Event {
            sample_index: n / 4,
            kind: EventKind::InflightAdjustment {
                function: 9,
                value: AdjustmentValue::Int(-17),
            },
        },
        Event {
            sample_index: n / 2,
            kind: EventKind::InflightAdjustment {
                function: 3,
                value: AdjustmentValue::Float(0.625),
            },
        },
    ];
    s.settings.pid = Some(PerAxis {
        roll: gains(30.0),
        pitch: gains(34.0),
        yaw: gains(5.0),
    });
    s.settings.debug_mode = Some(DebugMode {
        code: 3,
        name: Some("RC_INTERPOLATION".into()),
    });
    s
}

/// The headers of a wing with an S gain on roll and yaw, for the releases
/// that have wings.
fn wing_headers(target: Target) -> Vec<(String, String)> {
    if target < Target::V2025_12 {
        return Vec::new();
    }
    [("s_roll", "20"), ("s_pitch", "0"), ("s_yaw", "35")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

struct Case {
    target: Target,
    rates: LoopRates,
    high_resolution: bool,
}

impl Case {
    fn name(&self) -> String {
        format!(
            "{} at {:?} Hz, high resolution {}",
            self.target, self.rates.log_hz, self.high_resolution
        )
    }

    fn options(&self) -> WriteOptions {
        let mut headers = wing_headers(self.target);
        if self.high_resolution {
            headers.push(("blackbox_high_resolution".into(), "1".into()));
        }
        WriteOptions {
            target: self.target,
            headers,
            ..WriteOptions::default()
        }
    }
}

/// `back` holds, sample for sample, `expected(original)`.
fn assert_channel(
    name: &str,
    original: &FlightSession,
    back: &FlightSession,
    id: &ChannelId,
    expected: impl Fn(f64) -> f64,
) {
    let a = original.channel(id).unwrap().data.values();
    let b = back
        .channel(id)
        .unwrap_or_else(|| panic!("{name}: no channel {id:?}"))
        .data
        .values();
    assert_eq!(a.len(), b.len(), "{name}: {id:?}");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let want = expected(*x);
        assert!(
            (want - y).abs() < 1e-9,
            "{name}: {id:?} sample {i}: wrote {x}, expected {want}, read {y}"
        );
    }
}

fn assert_absent(name: &str, back: &FlightSession, id: &ChannelId) {
    assert!(back.channel(id).is_none(), "{name}: {id:?} is logged");
}

/// Every field the release logs comes back as the firmware's integer, and
/// no field the release does not have is there.
fn assert_fields(case: &Case, original: &FlightSession, back: &FlightSession) {
    let name = &case.name();
    let t = case.target;
    // the firmware logs ten times finer in high resolution
    let fine = if case.high_resolution { 10.0 } else { 1.0 };
    let steps = move |per_unit: f64| move |v: f64| whole(v * per_unit) / per_unit;
    for axis in Axis::ALL {
        for stage in [GyroStage::Filtered, GyroStage::Raw] {
            let id = ChannelId::Gyro { axis, stage };
            if stage == GyroStage::Raw && t < Target::V4_5 {
                assert_absent(name, back, &id);
            } else {
                assert_channel(name, original, back, &id, steps(fine));
            }
        }
        assert_channel(
            name,
            original,
            back,
            &ChannelId::Setpoint { axis },
            steps(fine),
        );
        for term in [PidTerm::P, PidTerm::I, PidTerm::D, PidTerm::F] {
            let id = ChannelId::PidTerm { axis, term };
            assert_channel(name, original, back, &id, whole);
        }
        // the S term of a wing: 2025.12 on, and only where the gain is set
        let s_term = ChannelId::PidTerm {
            axis,
            term: PidTerm::S,
        };
        if t >= Target::V2025_12 && axis != Axis::Pitch {
            assert_channel(name, original, back, &s_term, whole);
        } else {
            assert_absent(name, back, &s_term);
        }
        // 2048 counts per g
        let acc = ChannelId::Acc { axis };
        assert_channel(name, original, back, &acc, steps(2048.0));
        let quaternion = custom(&format!("imuQuaternion[{}]", axis.index()));
        if t >= Target::V2025_12 {
            assert_channel(name, original, back, &quaternion, steps(32767.0));
        } else {
            assert_absent(name, back, &quaternion);
        }
        let mag = custom(&format!("magADC[{}]", axis.index()));
        assert_channel(name, original, back, &mag, whole);
    }
    for ch in 0..4u8 {
        let id = ChannelId::RcCommand { ch };
        if t == Target::Master {
            // a fraction of full deflection, in thousandths
            let per_unit = 1000.0 * fine;
            assert_channel(name, original, back, &id, |v| {
                let fraction = if ch == 3 {
                    (v - 1000.0) / 1000.0
                } else {
                    v / 500.0
                };
                whole(fraction * per_unit) / per_unit
            });
        } else {
            assert_channel(name, original, back, &id, steps(fine));
        }
    }
    let cents = steps(100.0);
    let voltage = ChannelId::Battery(BatteryField::Voltage);
    assert_channel(name, original, back, &voltage, cents);
    let current = ChannelId::Battery(BatteryField::Current);
    assert_channel(name, original, back, &current, cents);
    assert_channel(name, original, back, &ChannelId::Rssi, whole);
    for idx in 0..8u8 {
        let debug = ChannelId::Debug { idx };
        if idx < 4 || t >= Target::V4_5 {
            assert_channel(name, original, back, &debug, whole);
        } else {
            assert_absent(name, back, &debug);
        }
        // every servo from 2025.12; before, the tail servo of a tricopter
        let servo = custom(&format!("servo[{idx}]"));
        if t >= Target::V2025_12 || idx == 5 {
            assert_channel(name, original, back, &servo, whole);
        } else {
            assert_absent(name, back, &servo);
        }
    }
    for idx in 0..4u8 {
        let erpm = ChannelId::Erpm { idx };
        if t >= Target::V4_5 {
            assert_channel(name, original, back, &erpm, whole);
        } else {
            assert_absent(name, back, &erpm);
        }
        assert!(back.channel(&ChannelId::Motor { idx }).is_some(), "{name}");
    }
    // the barometer changed its name with 4.4
    let (baro, other) = if t >= Target::V4_4 {
        ("baroAlt", "BaroAlt")
    } else {
        ("BaroAlt", "baroAlt")
    };
    let wrote = original.channel(&custom("baroAlt")).unwrap().data.values();
    let read = back.channel(&custom(baro)).unwrap().data.values();
    assert!(
        wrote.iter().zip(read).all(|(a, b)| whole(*a) == *b),
        "{name}"
    );
    assert_absent(name, back, &custom(other));
    assert_channel(name, original, back, &custom("surfaceRaw"), whole);
    for pitot in ["pitot[0]", "pitot[1]"] {
        if t == Target::Master {
            assert_channel(name, original, back, &custom(pitot), whole);
        } else {
            assert_absent(name, back, &custom(pitot));
        }
    }
    assert_eq!(
        back.diagnostics.unmapped_fields.len(),
        unmapped(t),
        "{name}"
    );
}

/// Fields the decoder's profile has no channel for: they come back under
/// their own names.
fn unmapped(target: Target) -> usize {
    // magADC × 3, surfaceRaw, the servos, the pitot
    let servos = if target >= Target::V2025_12 { 8 } else { 1 };
    let pitot = if target == Target::Master { 2 } else { 0 };
    3 + 1 + servos + pitot
}

/// The GPS frames: every column the release has, with the values written;
/// the home frames likewise, and the fixes are relative to the right one.
fn assert_gps(case: &Case, original: &FlightSession, raw: &DecodedLog, every_frame_intra: bool) {
    let name = &case.name();
    let t = case.target;
    let expected = |kind: &str, columns: &[&str]| -> Vec<AuxRow> {
        let table = original.aux.iter().find(|a| a.kind == kind).unwrap();
        let index: Vec<usize> = columns
            .iter()
            .map(|c| table.columns.iter().position(|x| x == c).unwrap())
            .collect();
        table
            .rows
            .iter()
            .map(|r| AuxRow {
                sample_index: r.sample_index,
                values: index.iter().map(|i| r.values[*i]).collect(),
            })
            .collect()
    };
    let mut fix: Vec<&str> = FIX[..7].to_vec();
    if t >= Target::V2025_12 {
        fix.extend(&FIX[7..10]);
    }
    if t >= Target::V2026_6 {
        fix.push(FIX[10]);
    }
    if every_frame_intra {
        // no time of their own: they follow a frame that has it
        fix.remove(0);
    }
    let gps = raw.gps.as_ref().unwrap_or_else(|| panic!("{name}: no G"));
    assert_eq!(gps.columns, fix, "{name}");
    assert_eq!(gps.rows, expected("gps", &fix), "{name}");
    let mut home: Vec<&str> = HOME[..2].to_vec();
    if t >= Target::V2025_12 {
        home.push(HOME[2]);
    }
    if t >= Target::V2026_6 {
        home.push(HOME[3]);
    }
    let table = raw.gps_home.as_ref().unwrap();
    assert_eq!(table.columns, home, "{name}");
    assert_eq!(table.rows, expected("gps_home", &home), "{name}");
    assert_eq!(raw.stats.frames[&b'G'].valid, 12, "{name}");
    assert_eq!(raw.stats.frames[&b'H'].valid, 2, "{name}");
}

/// The slow frames and the events a flight controller logs for the modes of
/// the flight, and the events the session carries. `slow_frames` is false
/// for the one schedule on which the firmware logs none.
fn assert_modes_and_events(
    case: &Case,
    original: &FlightSession,
    back: &FlightSession,
    slow_frames: bool,
) {
    let name = &case.name();
    let slow = &back.aux[0];
    assert_eq!(slow.kind, "slow");
    let n = original.len();
    let kinds: Vec<&EventKind> = back.events.iter().map(|e| &e.kind).collect();
    let at: Vec<usize> = back.events.iter().map(|e| e.sample_index).collect();
    assert!(matches!(kinds[0], EventKind::SyncBeep { .. }), "{name}");
    assert_eq!(
        kinds[1],
        &EventKind::InflightAdjustment {
            function: 9,
            value: AdjustmentValue::Int(-17)
        },
        "{name}"
    );
    assert_eq!(at[1], n / 4, "{name}");
    let EventKind::FlightMode { flags, last_flags } = *kinds[2] else {
        panic!("{name}: {:?}", kinds[2]);
    };
    assert_eq!((flags ^ last_flags).count_ones(), 1, "{name}: ANGLE");
    assert!(flags > last_flags && last_flags & 1 == 1, "{name}: ARM");
    assert_eq!(
        kinds[3],
        &EventKind::InflightAdjustment {
            function: 3,
            value: AdjustmentValue::Float(0.625)
        },
        "{name}"
    );
    assert_eq!(at[3], n / 2, "{name}");
    let back_again = EventKind::FlightMode {
        flags: last_flags,
        last_flags: flags,
    };
    assert_eq!(kinds[4], &back_again, "{name}");
    assert_eq!(kinds[5], &EventKind::Disarm { reason: 4 }, "{name}");
    assert_eq!(kinds[6], &EventKind::LogEnd, "{name}");
    assert_eq!(kinds.len(), 7, "{name}");
    // the changes are logged where they happened
    let changes: Vec<usize> = original
        .flight_modes
        .iter()
        .map(|c| c.sample_index)
        .collect();
    assert!(
        at[2].abs_diff(changes[1]) <= 1 && at[4].abs_diff(changes[2]) <= 1,
        "{name}: events at {at:?}, changes at {changes:?}"
    );
    if !slow_frames {
        assert!(
            slow.rows.is_empty() && back.flight_modes.is_empty(),
            "{name}"
        );
        return;
    }
    // a slow frame at the start and at both changes; a healthy link, no
    // failsafe; the modes come back as the session named them
    assert_eq!(slow.rows.len(), 3, "{name}");
    for (row, change) in slow.rows.iter().zip(&original.flight_modes) {
        assert_eq!(row.values[1..], [0, 0, 1, 1], "{name}");
        assert!(
            row.sample_index.abs_diff(change.sample_index) <= 1,
            "{name}"
        );
    }
    let stated: Vec<i64> = slow.rows.iter().map(|r| r.values[0]).collect();
    let logged = [
        i64::from(last_flags),
        i64::from(flags),
        i64::from(last_flags),
    ];
    assert_eq!(stated, logged, "{name}");
    let modes = |s: &FlightSession| -> Vec<Vec<FlightMode>> {
        s.flight_modes.iter().map(|c| c.modes.clone()).collect()
    };
    assert_eq!(modes(back), modes(original), "{name}");
}

#[test]
fn every_field_frame_and_event_of_every_release_at_every_rate() {
    // the rates: a quarter of a 4 kHz loop; a P interval that does not divide
    // the I interval (3.2 kHz gyros: I 102, P 4); every loop; I frames only
    // (the last: 16 is one of the five P intervals of 4.3 and later; 4.2,
    // which divides the I interval by any ratio, logs it as no P frames)
    let schedules = [
        (rates(8000.0, 4000.0, 1000.0), ("128", "4", "4"), 1000.0),
        (rates(3200.0, 3200.0, 800.0), ("102", "4", "4"), 800.0),
        (rates(8000.0, 8000.0, 8000.0), ("256", "1", "1"), 8000.0),
        (rates(8000.0, 2000.0, 31.25), ("64", "0", "0"), 31.25),
        (rates(500.0, 500.0, 31.25), ("16", "16", "0"), 31.25),
    ];
    let mut cases = 0;
    for (loop_rates, (i_interval, p_interval, p_of_4_2), log_hz) in schedules {
        // some 600 frames each, and less than the 8.2 s after which the
        // firmware repeats a slow frame
        let mut original = everything(log_hz, (600.0 / log_hz).min(6.0));
        original.settings.loop_rates = loop_rates;
        for target in Target::ALL {
            for high_resolution in [false, true] {
                if high_resolution && target < Target::V4_4 {
                    // 4.3 has no high resolution
                    continue;
                }
                let case = Case {
                    target,
                    rates: loop_rates,
                    high_resolution,
                };
                let p_interval = if target == Target::V4_2 {
                    p_of_4_2
                } else {
                    p_interval
                };
                let name = case.name();
                let bytes = betaflight(&original, &case.options()).unwrap();
                let back = read(&bytes);
                let raw = decode(&bytes).unwrap();
                let d = &back.diagnostics;
                assert_eq!(
                    (d.corrupt_frames, d.desync_frames, d.skipped_bytes),
                    (0, 0, 0),
                    "{name}"
                );
                assert_eq!(back.len(), original.len(), "{name}");
                assert_eq!(back.header("I interval"), Some(i_interval), "{name}");
                assert_eq!(back.header("P interval"), Some(p_interval), "{name}");
                let version = back.firmware.version.unwrap();
                assert_eq!(
                    (version.major, version.minor),
                    (target.version().major, target.version().minor)
                );
                assert_channel(&name, &original, &back, &ChannelId::Time, |t| t);
                assert_fields(&case, &original, &back);
                assert_gps(&case, &original, &raw, i_interval == p_interval);
                // a P interval as long as the I interval: no P frame ever,
                // and the firmware logs slow frames before P frames only
                let slow_frames = i_interval != p_interval;
                assert_modes_and_events(&case, &original, &back, slow_frames);
                // the I frames are where the schedule puts them
                let iteration = raw.column("loopIteration").unwrap();
                let every: i64 = i_interval.parse().unwrap();
                for (k, it) in iteration.iter().enumerate() {
                    assert_eq!(raw.intra[k], it % every == 0, "{name}: frame {k}");
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 5 * (7 + 5));
}

/// What the headers of the full flight say on each release: nothing is
/// switched off, the sensors have their lines, the GPS feature is on.
#[test]
fn the_headers_describe_the_craft_that_logs_all_this() {
    let mut original = everything(1000.0, 0.4);
    original.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    for target in Target::ALL {
        let options = WriteOptions {
            target,
            headers: wing_headers(target),
            ..WriteOptions::default()
        };
        let back = read(&betaflight(&original, &options).unwrap());
        let name = target.to_string();
        // 4.2 has no mask
        let mask = (target >= Target::V4_3).then_some("0");
        assert_eq!(back.header("fields_disabled_mask"), mask, "{name}");
        assert_eq!(back.header("debug_mode"), Some("3"), "{name}");
        assert_eq!(back.header("dshot_bidir"), Some("1"), "{name}");
        let features: i64 = back.header("features").unwrap().parse().unwrap();
        assert_eq!(features & 128, 128, "{name}: the GPS feature");
        // the voltage of the first frame
        assert_eq!(back.header("vbatref"), Some("1680"), "{name}");
        for line in ["vbat_scale", "currentSensor", "mag_hardware", "acc_1G"] {
            assert!(back.header(line).is_some(), "{name}: {line}");
        }
        assert_eq!(back.header("yawPID"), Some("45,80,5"), "{name}");
        let wing = target >= Target::V2025_12;
        assert_eq!(back.header("s_roll").is_some(), wing, "{name}");
        // the autopilot of a multirotor is not in a wing build
        assert_eq!(back.header("ap_hover_throttle"), None, "{name}");
        let scale = (target == Target::Master).then_some("1000");
        assert_eq!(back.header("rc_command_scale"), scale, "{name}");
        let mode_name = (target == Target::Master).then_some("RC_INTERPOLATION");
        assert_eq!(back.header("debug_mode_name"), mode_name, "{name}");
        let kind = if wing { "Betaflight" } else { "Cleanflight" };
        assert_eq!(back.header("Firmware type"), Some(kind), "{name}");
    }
}

/// A flight with nothing but time, gyro, setpoint, throttle and motors (what
/// the simulator makes) on every release: the fields it has no channel for
/// are not logged, and the headers say why.
#[test]
fn a_bare_flight_logs_what_it_has_and_the_headers_say_so() {
    let mut bare = SignalSpec {
        fs: 1000.0,
        duration_s: 0.3,
        ..SignalSpec::default()
    }
    .build();
    bare.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    for target in Target::ALL {
        let options = WriteOptions {
            target,
            ..WriteOptions::default()
        };
        let bytes = betaflight(&bare, &options).unwrap();
        let raw = decode(&bytes).unwrap();
        let back = read(&bytes);
        let name = target.to_string();
        assert_eq!(back.diagnostics.corrupt_frames, 0, "{name}");
        let mut fields = vec!["loopIteration", "time"];
        // the PID terms of an idle controller; yaw has no D gain
        fields.extend(["axisP[0]", "axisP[1]", "axisP[2]"]);
        fields.extend(["axisI[0]", "axisI[1]", "axisI[2]"]);
        fields.extend(["axisD[0]", "axisD[1]"]);
        fields.extend(["axisF[0]", "axisF[1]", "axisF[2]"]);
        fields.extend([
            "rcCommand[0]",
            "rcCommand[1]",
            "rcCommand[2]",
            "rcCommand[3]",
        ]);
        fields.extend(["setpoint[0]", "setpoint[1]", "setpoint[2]", "setpoint[3]"]);
        fields.extend(["gyroADC[0]", "gyroADC[1]", "gyroADC[2]"]);
        if target >= Target::V4_5 {
            fields.extend(["gyroUnfilt[0]", "gyroUnfilt[1]", "gyroUnfilt[2]"]);
        }
        fields.extend(["motor[0]", "motor[1]", "motor[2]", "motor[3]"]);
        if target >= Target::V4_5 {
            fields.extend(["eRPM[0]", "eRPM[1]", "eRPM[2]", "eRPM[3]"]);
        }
        assert_eq!(raw.field_names(), fields, "{name}");
        assert!(raw.gps.is_none() && raw.gps_home.is_none(), "{name}");
        // no sensor is switched off: the craft has none of them
        let mask = (target >= Target::V4_3).then_some("0");
        assert_eq!(back.header("fields_disabled_mask"), mask, "{name}");
        for line in ["vbat_scale", "vbatcellvoltage", "vbatref", "currentSensor"] {
            assert_eq!(back.header(line), None, "{name}: {line}");
        }
        let features: i64 = back.header("features").unwrap().parse().unwrap();
        assert_eq!(features & 128, 0, "{name}: no GPS feature");
        assert_eq!(back.header("debug_mode"), Some("0"), "{name}");
        // a multirotor build of the releases that are assembled from options
        let multirotor = target >= Target::V2025_12;
        assert_eq!(back.header("ap_hover_throttle").is_some(), multirotor);
        assert_eq!(back.header("s_roll"), None, "{name}");
        for term in ["axisP[1]", "axisD[0]", "axisF[2]"] {
            assert!(raw.column(term).unwrap().iter().all(|v| *v == 0), "{name}");
        }
    }
}

/// What the configuration switches off is not logged, whatever the session
/// carries: the mask of the caller, a debug mode of none, a D gain of zero,
/// `DShot` without telemetry.
#[test]
fn the_configuration_decides_what_is_logged() {
    let mut original = everything(1000.0, 0.3);
    original.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    original.settings.debug_mode = None;
    let pid = original.settings.pid.as_mut().unwrap();
    pid.roll.d = 0.0;
    // 2025.12: PID is bit 0, the battery 3, the accelerometer 9, the GPS 12
    let mask = 1 | 1 << 3 | 1 << 9 | 1 << 12;
    let options = WriteOptions {
        headers: vec![
            ("fields_disabled_mask".into(), mask.to_string()),
            ("dshot_bidir".into(), "0".into()),
        ],
        ..WriteOptions::default()
    };
    let bytes = betaflight(&original, &options).unwrap();
    let raw = decode(&bytes).unwrap();
    let names = raw.field_names();
    let has = |field: &str| names.iter().any(|n| n == field);
    assert!(!has("axisP[0]") && !has("axisD[1]") && !has("axisF[0]"));
    assert!(!has("vbatLatest") && !has("amperageLatest"));
    assert!(!has("accSmooth[0]") && has("imuQuaternion[0]"));
    assert!(!has("debug[0]") && !has("eRPM[0]"));
    assert!(has("rssi") && has("magADC[2]") && has("servo[7]"));
    assert!(raw.gps.is_none());
    let back = read(&bytes);
    assert_eq!(back.header("fields_disabled_mask"), Some("4617"));
    // the battery is switched off, so the firmware skips its lines, but the
    // current sensor is described as long as there is one
    assert_eq!(back.header("vbatref"), None);

    // without the mask the D term follows the gains: none on roll
    let options = WriteOptions::default();
    let raw = decode(&betaflight(&original, &options).unwrap()).unwrap();
    let names = raw.field_names();
    let has = |field: &str| names.iter().any(|n| n == field);
    assert!(!has("axisD[0]") && has("axisD[1]") && has("axisD[2]"));
    assert!(!has("debug[0]"), "no debug mode, no debug fields");

    // a debug mode without debug values: the mode stays, the fields are off
    let mut silent = original.clone();
    for idx in 0..8u8 {
        silent.channels.remove(&ChannelId::Debug { idx });
        silent.channels.remove(&ChannelId::Erpm { idx });
        silent.channels.remove(&ChannelId::MotorRpm { idx });
    }
    for axis in Axis::ALL {
        let stage = GyroStage::Raw;
        silent.channels.remove(&ChannelId::Gyro { axis, stage });
        silent.channels.remove(&ChannelId::Acc { axis });
    }
    let options = WriteOptions {
        headers: vec![
            ("debug_mode".into(), "6".into()),
            ("dshot_bidir".into(), "1".into()),
        ],
        ..WriteOptions::default()
    };
    let back = read(&betaflight(&silent, &options).unwrap());
    // 2025.12: ACC 9, DEBUG 10, RPM 13, GYROUNFILT 14
    let mask = 1 << 9 | 1 << 10 | 1 << 13 | 1 << 14;
    assert_eq!(
        back.header("fields_disabled_mask"),
        Some(mask.to_string().as_str())
    );
    assert!(back.channel(&ChannelId::Debug { idx: 0 }).is_none());
    assert!(back.channel(&custom("imuQuaternion[0]")).is_some());

    // 2025.12 and 2026.6 log the servos even when the mask switches them
    // off; master obeys the mask
    for (target, logged) in [(Target::V2026_6, true), (Target::Master, false)] {
        let options = WriteOptions {
            target,
            headers: vec![("fields_disabled_mask".into(), (1 << 15).to_string())],
            ..WriteOptions::default()
        };
        let back = read(&betaflight(&original, &options).unwrap());
        assert_eq!(back.channel(&custom("servo[0]")).is_some(), logged);
    }
}

/// With the gyro switched off in the mask a log needs no gyro at all.
#[test]
fn a_log_without_the_gyro() {
    let mut s = everything(1000.0, 0.2);
    s.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    for axis in Axis::ALL {
        for stage in [GyroStage::Filtered, GyroStage::Raw] {
            s.channels.remove(&ChannelId::Gyro { axis, stage });
        }
    }
    assert_eq!(
        betaflight(&s, &WriteOptions::default()),
        Err(WriteError::MissingChannel("filtered gyro"))
    );
    let options = WriteOptions {
        headers: vec![("fields_disabled_mask".into(), (1 << 7).to_string())],
        ..WriteOptions::default()
    };
    let back = read(&betaflight(&s, &options).unwrap());
    assert_eq!(back.len(), s.len());
    let stage = GyroStage::Filtered;
    let axis = Axis::Roll;
    assert!(back.channel(&ChannelId::Gyro { axis, stage }).is_none());
    // GYRO is bit 7, GYROUNFILT 14
    assert_eq!(back.header("fields_disabled_mask"), Some("16512"));
}

/// The firmware repeats the slow frame every 256 I intervals (8.2 s) and
/// logs one when a paused log resumes.
#[test]
fn slow_frames_are_repeated_and_follow_a_pause() {
    let spec = SignalSpec {
        fs: 500.0,
        duration_s: 17.0,
        dropouts: vec![Dropout {
            start_s: 2.0,
            len_s: 0.5,
        }],
        ..SignalSpec::default()
    };
    let mut s = spec.build();
    s.settings.loop_rates = rates(8000.0, 4000.0, 500.0);
    let back = read(&betaflight(&s, &WriteOptions::default()).unwrap());
    let slow = &back.aux[0];
    let iteration = back.channel(&ChannelId::LoopIteration).unwrap();
    let at: Vec<f64> = slow
        .rows
        .iter()
        .map(|r| iteration.data.values()[r.sample_index])
        .collect();
    // before the first P frame; after the pause; then every 128 × 256
    // iterations since the last one
    assert_eq!(at[0], 8.0);
    assert_eq!(at[1], 10_120.0, "the first P frame after the log resumed");
    assert_eq!(at[2], 10_120.0 + 32_768.0);
    assert_eq!(at.len(), 3);
    assert!(slow.rows.iter().all(|r| r.values == [1, 0, 0, 1, 1]));
    let resumed = back
        .events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::LoggingResume { .. }))
        .count();
    assert_eq!(resumed, 1);
}

/// The rules of the slow frames, read off a real log: one before the first
/// P frame, then only when the state changed or 256 I intervals after the
/// last one. They are the rules the writer applies to a flight.
#[test]
fn the_slow_frames_of_a_real_log_follow_the_rules_the_writer_applies() {
    let data = std::fs::read(common::logs_dir().join("betaflight-2025.12.2-btfl_027.bbl")).unwrap();
    let raw = decode(&data).unwrap();
    let iteration = raw.column("loopIteration").unwrap();
    let every = i64::from(raw.headers.i_interval) * 256;
    let rows = &raw.slow.rows;
    assert_eq!(rows[0].sample_index, 1, "after the I frame, before the P");
    let (mut changed, mut repeated) = (0, 0);
    for pair in rows.windows(2) {
        let since = iteration[pair[1].sample_index] - iteration[pair[0].sample_index];
        if pair[1].values == pair[0].values {
            assert_eq!(since, every, "an unchanged state is repeated");
            repeated += 1;
        } else {
            assert!(since < every);
            changed += 1;
        }
    }
    assert!(changed >= 2 && repeated >= 2, "{changed} / {repeated}");
}

/// The stick commands of a log of `master` (fractions of full deflection)
/// written for a release that counts in the old units, and back.
#[test]
fn stick_commands_change_their_units_with_the_release() {
    let mut original = everything(1000.0, 0.3);
    original.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    let options = |target: Target| WriteOptions {
        target,
        ..WriteOptions::default()
    };
    let on_master = read(&betaflight(&original, &options(Target::Master)).unwrap());
    let roll = ChannelId::RcCommand { ch: 0 };
    let throttle = ChannelId::RcCommand { ch: 3 };
    let peak = |s: &FlightSession, id: &ChannelId| {
        let values = s.channel(id).unwrap().data.values();
        values.iter().fold(0.0f64, |m, v| m.max(v.abs()))
    };
    assert!(peak(&on_master, &roll) < 1.0 && peak(&on_master, &throttle) <= 1.0);
    // and from that log to 2025.12: ±500 and 1000 … 2000 again
    let again = read(&betaflight(&on_master, &options(Target::V2025_12)).unwrap());
    assert_eq!(again.header("rc_command_scale"), None);
    for ch in 0..4u8 {
        let id = ChannelId::RcCommand { ch };
        let wrote = original.channel(&id).unwrap().data.values();
        let read = again.channel(&id).unwrap().data.values();
        for (a, b) in wrote.iter().zip(read) {
            // thousandths of the deflection are finer than the old units
            assert!((a - b).abs() <= 1.0, "channel {ch}: {a} became {b}");
        }
    }
    assert!(peak(&again, &throttle) > 1600.0);
    // master to master keeps every bit
    let same = read(&betaflight(&on_master, &options(Target::Master)).unwrap());
    assert_eq!(
        same.channel(&roll).unwrap().data,
        on_master.channel(&roll).unwrap().data
    );
    // a flight without stick channels: the sticks follow the setpoint
    for ch in 0..4u8 {
        original.channels.remove(&ChannelId::RcCommand { ch });
    }
    let derived = read(&betaflight(&original, &options(Target::Master)).unwrap());
    let setpoint = original
        .channel(&ChannelId::Setpoint { axis: Axis::Roll })
        .unwrap()
        .data
        .values();
    let stick = derived.channel(&roll).unwrap().data.values();
    for (sp, st) in setpoint.iter().zip(stick) {
        let want = whole((sp / 1000.0).clamp(-1.0, 1.0) * 1000.0) / 1000.0;
        assert!((want - st).abs() < 1e-9, "{sp} -> {st}");
    }
    let stick = derived.channel(&throttle).unwrap().data.values();
    assert!(stick.iter().all(|v| (0.0..=1.0).contains(v)));
}

/// The independent decoder (ADR-0005) on the full flight for the releases it
/// knows (it predates the year.month versions): the same value in every
/// field of every main frame, the GPS frames and the slow frames in between
/// notwithstanding.
#[test]
fn the_independent_decoder_reads_the_full_flight() {
    let mut original = everything(1000.0, 0.6);
    original.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    for target in [Target::V4_2, Target::V4_3, Target::V4_4, Target::V4_5] {
        let options = WriteOptions {
            target,
            ..WriteOptions::default()
        };
        let bytes = betaflight(&original, &options).unwrap();
        let read = common::assert_reference_decoder_agrees(&bytes, &target.to_string());
        assert_eq!(read.frames, original.len(), "{target}: every frame");
        assert_eq!(read.fixes, 12, "{target}: every fix was read");
        assert!(read.values > 15_000, "{target}: {} values", read.values);
    }
}

/// The gains and the filters of the session are in the headers of every
/// release, under the names that release has for them (4.2 calls the
/// low-passes `gyro_lowpass_hz` and `dterm_filter_type`, the feedforward
/// `feedforward_weight`, the RPM filter `gyro_rpm_notch_*`).
#[test]
fn the_settings_are_written_under_the_names_of_each_release() {
    let mut original = everything(1000.0, 0.2);
    original.settings.loop_rates = rates(8000.0, 4000.0, 1000.0);
    let lowpass = |kind: LowpassKind, cutoff_hz: f64| Lowpass {
        kind: Some(kind),
        cutoff_hz,
        dyn_min_hz: None,
        dyn_max_hz: None,
        dyn_expo: 0.5,
    };
    original.settings.gyro_filters = FilterChain {
        lowpass1: Some(lowpass(LowpassKind::Pt1, 180.0)),
        lowpass2: Some(lowpass(LowpassKind::Biquad, 260.0)),
        rpm_notch: Some(RpmNotch {
            harmonics: 2,
            q: 6.5,
            min_hz: 90.0,
            fade_range_hz: 50.0,
            weights: Vec::new(),
            lpf_hz: 150.0,
        }),
        ..FilterChain::default()
    };
    original.settings.dterm_filters = FilterChain {
        lowpass1: Some(lowpass(LowpassKind::Pt2, 110.0)),
        ..FilterChain::default()
    };
    for target in Target::ALL {
        let options = WriteOptions {
            target,
            ..WriteOptions::default()
        };
        let back = read(&betaflight(&original, &options).unwrap());
        let name = target.to_string();
        let (wrote, read) = (&original.settings, &back.settings);
        let gains = |s: &openpid_core::Settings| {
            let pid = s.pid.as_ref().unwrap();
            [&pid.roll, &pid.pitch, &pid.yaw].map(|g| (g.p, g.i, g.d, g.f))
        };
        assert_eq!(gains(read), gains(wrote), "{name}");
        let cut = |l: &Option<Lowpass>| l.as_ref().map(|l| (l.kind.clone(), l.cutoff_hz));
        let (g, h) = (&read.gyro_filters, &wrote.gyro_filters);
        assert_eq!(cut(&g.lowpass1), cut(&h.lowpass1), "{name}");
        assert_eq!(cut(&g.lowpass2), cut(&h.lowpass2), "{name}");
        assert_eq!(g.lowpass1.as_ref().unwrap().dyn_min_hz, None, "{name}");
        let (g, h) = (g.rpm_notch.as_ref().unwrap(), h.rpm_notch.as_ref().unwrap());
        assert_eq!(
            (g.harmonics, g.q, g.min_hz, g.lpf_hz),
            (h.harmonics, h.q, h.min_hz, h.lpf_hz),
            "{name}"
        );
        let (d, e) = (&read.dterm_filters, &wrote.dterm_filters);
        assert_eq!(cut(&d.lowpass1), cut(&e.lowpass1), "{name}");
        assert!(d.lowpass2.is_none(), "{name}");
        assert_eq!(read.loop_rates, wrote.loop_rates, "{name}");
    }
}
