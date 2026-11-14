//! The frequency × throttle map on signals made here, so that every expected
//! number follows from the rule (`docs/DESIGN.md` §6.4) and the signal alone:
//! which columns a segment fills, what a tone and white noise read before and
//! after the smoothing, how many segments a stretch has, where an even kernel
//! puts a cell, the decibel matrix and the two printed numbers.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::throttle::{
    COLUMNS, ColumnMeans, DB_FLOOR, Smoothing, column_means, first_counted_row, hop_seconds,
    in_column, kernel_taps, mean_peak, smooth,
};
use openpid_analysis::{
    Analysis, AnalysisCtx, AnalysisError, Epoch, ThrottleHeatmap, ThrottleHeatmapOutput,
    ThrottleHeatmapParams,
};
use openpid_core::session::Diagnostics;
use openpid_core::{
    Axis, Channel, ChannelId, ChannelSet, FirmwareInfo, FlightSession, GyroStage, Origin, Series,
    Settings, Timebase, Unit,
};

/// The channel every test analyses.
const GYRO: ChannelId = ChannelId::Gyro {
    axis: Axis::Roll,
    stage: GyroStage::Raw,
};

/// A session at `fs` with the gyro, the throttle (percent) and, when given,
/// the speed of one motor.
fn session(fs: f64, gyro: Series, throttle: Vec<f64>, rpm: Option<Vec<f64>>) -> FlightSession {
    let n = gyro.len();
    let mut channels = ChannelSet::new();
    let mut add = |id: ChannelId, name: &str, unit: Unit, data: Series| {
        channels.insert(Channel {
            id,
            name: name.into(),
            unit,
            data,
            origin: Origin::Logged { field: name.into() },
        });
    };
    add(GYRO, "gyroUnfilt[0]", Unit::DegreesPerSecond, gyro);
    add(
        ChannelId::Throttle,
        "throttle",
        Unit::Percent,
        Series::from_vec(throttle),
    );
    if let Some(rpm) = rpm {
        add(
            ChannelId::MotorRpm { idx: 0 },
            "eRPM[0]",
            Unit::Rpm,
            Series::from_vec(rpm),
        );
    }
    FlightSession {
        index: 0,
        offset_bytes: 0,
        firmware: FirmwareInfo::parse_revision("Synthetic 1.0"),
        profile_id: "synthetic".into(),
        settings: Settings::default(),
        raw_headers: Vec::new(),
        timebase: Timebase::from_times(
            (0..n).map(|i| (i as f64 * 1.0e6 / fs) as i64).collect(),
            Some(1.0e6 / fs),
        ),
        channels,
        events: Vec::new(),
        aux: Vec::new(),
        flight_modes: Vec::new(),
        diagnostics: Diagnostics::default(),
    }
}

fn whole(s: &FlightSession) -> AnalysisCtx<'_> {
    AnalysisCtx {
        session: s,
        epoch: None,
    }
}

fn run(s: &FlightSession, smoothing: Smoothing) -> ThrottleHeatmapOutput {
    ThrottleHeatmap
        .run(
            &whole(s),
            &ThrottleHeatmapParams {
                channel: GYRO,
                smoothing,
            },
        )
        .unwrap()
}

fn columns(s: &FlightSession) -> ColumnMeans {
    column_means(&whole(s), &GYRO).unwrap()
}

/// A tone of `amplitude` on bin `bin` of a segment of `nperseg` samples.
fn tone(n: usize, nperseg: usize, bin: usize, amplitude: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            amplitude * (std::f64::consts::TAU * (bin * i) as f64 / nperseg as f64 + 0.3).sin()
        })
        .collect()
}

/// White noise of standard deviation 1: uniform on ±√3, from a fixed seed.
fn white(n: usize) -> Vec<f64> {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * 3f64.sqrt()
        })
        .collect()
}

/// The smoothing written out cell by cell from the rule: a kernel of `5n` by
/// `n` taps, `K[i][j] ∝ exp(−((i − (5n−1)/2)² + (j − (n−1)/2)²) / (2·4²))`
/// summing to 1, `out[p] = Σ K[t] · M[p + o − t]` with
/// `o = (size − 1) / 2 (+ 1 for an even size)`, `outside` beyond the matrix.
fn smooth_by_the_rule(m: &[f64], nx: usize, ny: usize, n: usize, outside: f64) -> Vec<f64> {
    let (st, sf) = (5 * n, n);
    let k = |i: usize, j: usize| {
        let a = i as f64 - (st as f64 - 1.0) / 2.0;
        let b = j as f64 - (sf as f64 - 1.0) / 2.0;
        (-(a * a + b * b) / 32.0).exp()
    };
    let sum: f64 = (0..st)
        .flat_map(|i| (0..sf).map(move |j| (i, j)))
        .map(|(i, j)| k(i, j))
        .sum();
    let offset = |size: usize| ((size - 1) / 2 + usize::from(size.is_multiple_of(2))) as isize;
    let mut out = vec![0.0; m.len()];
    for p in 0..nx as isize {
        for r in 0..ny as isize {
            let mut acc = 0.0;
            for i in 0..st {
                for j in 0..sf {
                    let q = p + offset(st) - i as isize;
                    let s = r + offset(sf) - j as isize;
                    if (0..nx as isize).contains(&q) && (0..ny as isize).contains(&s) {
                        acc += k(i, j) / sum * (m[s as usize * nx + q as usize] - outside);
                    }
                }
            }
            out[r as usize * nx + p as usize] = acc + outside;
        }
    }
    out
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn a_tone_at_a_held_throttle_reads_a_quarter_in_two_columns() {
    // 10 s at 2 kHz: L = round(0.3·2000) = 600 samples, rows = 300, a row is
    // 2000/600 Hz; a stretch under 20 s has its segments 0.06 s = 120 samples
    // apart, and floor((20000 − 600) / 120) = 161 of them
    let (fs, n, nperseg) = (2000.0, 20_000, 600);
    let amplitude = 8.0;
    // a second, stronger tone on row 20: below the rows the printed numbers count
    let x: Vec<f64> = tone(n, nperseg, 60, amplitude)
        .iter()
        .zip(tone(n, nperseg, 20, 3.0 * amplitude))
        .map(|(a, b)| a + b)
        .collect();
    // 30.5 %: the columns c with c − 1 < 30.5 ≤ c + 1 are 30 and 31
    let s = session(fs, Series::from_vec(x), vec![30.5; n], None);
    let raw = columns(&s);
    assert_eq!(
        (raw.nperseg, raw.hop, raw.positions, raw.segments, raw.rows),
        (600, 120, 161, 161, 300)
    );
    assert!((raw.fs - fs).abs() < 1e-9);
    for (ix, count) in raw.counts.iter().enumerate() {
        let expected = if ix == 29 || ix == 30 { 161 } else { 0 };
        assert_eq!(*count, expected, "column {}", ix + 1);
    }
    // no compensation for the window: a tone of A on a bin reads A/4 there
    // and A/8 on either neighbour (the sum of a Hann window is L/2, and the
    // spectrum is two-sided); to (L − 1)/L, the window being symmetric
    let at = |row: usize, ix: usize| raw.linear[row * COLUMNS + ix];
    for ix in [29, 30] {
        assert!(close(at(60, ix), amplitude / 4.0, 0.005 * amplitude));
        assert!(close(at(59, ix), amplitude / 8.0, 0.005 * amplitude));
        assert!(close(at(61, ix), amplitude / 8.0, 0.005 * amplitude));
        assert!(close(at(20, ix), 3.0 * amplitude / 4.0, 0.015 * amplitude));
        assert!(at(100, ix) < 1e-3 * amplitude, "nothing between the tones");
    }
    assert!((0..300).all(|row| at(row, 28) == 0.0 && at(row, 31) == 0.0));

    // "low": 5 taps along the throttle, 1 along the frequency. Two adjacent
    // columns of the same value v: the cell of either holds v·(K[2] + K[1])
    let low = run(&s, Smoothing::Low);
    let k = kernel_taps(5);
    let expected = at(60, 29) * (k[2] + k[1]);
    assert!(close(low.grid.get(29, 60), expected, 1e-12));
    assert!(close(low.grid.get(30, 60), expected, 1e-12));
    // one column further only one of the two reaches with K[0], K[1]
    assert!(close(
        low.grid.get(28, 60),
        at(60, 29) * (k[1] + k[0]),
        1e-12
    ));
    assert_eq!(low.grid.get(26, 60), 0.0, "out of the kernel's reach");
    // the peak is the tone on row 60: row 20 is below rows/10 − 1 = 29
    assert!(close(low.peak, expected, 1e-12));
    assert!(low.grid.get(29, 20) > 2.0 * low.peak);

    // "medium": 15 × 3 taps. Over the two columns and the three rows of the
    // tone: (Kt[7] + Kt[6]) · (Kf[1]·v60 + Kf[0]·v61 + Kf[2]·v59)
    let medium = run(&s, Smoothing::Medium);
    let (kt, kf) = (kernel_taps(15), kernel_taps(3));
    let expected = (kt[7] + kt[6]) * (kf[1] * at(60, 29) + kf[0] * at(61, 29) + kf[2] * at(59, 29));
    assert!(close(medium.grid.get(29, 60), expected, 1e-12));
    assert!(close(medium.peak, expected, 1e-12));
    // which is why a cell is not a tone's amplitude: about 0.035·A
    assert!(
        close(medium.peak / amplitude, 0.035, 0.0005),
        "{}",
        medium.peak
    );
    assert_eq!(medium.smoothing, Smoothing::Medium);
    assert_eq!(medium.counts, raw.counts);
    assert_eq!(
        (medium.nperseg, medium.hop, medium.segments),
        (600, 120, 161)
    );
    // the grid: 100 columns drawn from c − 1 to c, rows to half the rate
    assert_eq!(medium.grid.x.len(), 100);
    assert_eq!((medium.grid.x[0], medium.grid.x[99]), (0.5, 99.5));
    assert_eq!(medium.grid.y.len(), 300);
    assert!(close(medium.grid.y[299], 1000.0 - fs / 600.0, 1e-9));
    assert_eq!(medium.grid_db.x, medium.grid.x);
    assert_eq!(medium.grid_db.y, medium.grid.y);
    assert_eq!(medium.view(false), &medium.grid);
    assert_eq!(medium.view(true), &medium.grid_db);
    assert_eq!(medium.mean_peak(false), (medium.mean, medium.peak));
    assert_eq!(medium.mean_peak(true), (medium.mean_db, medium.peak_db));
}

#[test]
fn every_level_is_the_kernel_of_the_rule() {
    // noise and a tone, a throttle that sweeps the lower edge of the map (so
    // the zeros outside the matrix count)
    let (fs, n) = (1000.0, 12_000);
    let x: Vec<f64> = white(n)
        .iter()
        .zip(tone(n, 300, 40, 5.0))
        .map(|(a, b)| a + b)
        .collect();
    let throttle: Vec<f64> = (0..n).map(|i| 0.5 + 9.0 * i as f64 / n as f64).collect();
    let s = session(fs, Series::from_vec(x), throttle, None);
    let raw = columns(&s);
    assert_eq!(raw.rows, 150);
    for level in Smoothing::ALL {
        let out = run(&s, level);
        let linear = smooth_by_the_rule(&raw.linear, COLUMNS, raw.rows, level.level(), 0.0);
        let db = smooth_by_the_rule(&raw.db, COLUMNS, raw.rows, level.level(), DB_FLOOR);
        for (i, (a, b)) in out.grid.z.iter().zip(&linear).enumerate() {
            assert!(close(*a, *b, 1e-12), "{level:?} linear cell {i}: {a} {b}");
        }
        for (i, (a, b)) in out.grid_db.z.iter().zip(&db).enumerate() {
            assert!(close(*a, *b, 1e-9), "{level:?} decibel cell {i}: {a} {b}");
        }
        // the printed numbers: every column, the rows from rows/10 − 1 = 14 up
        let cells = &linear[14 * COLUMNS..];
        assert!(close(
            out.mean,
            cells.iter().sum::<f64>() / cells.len() as f64,
            1e-12
        ));
        assert!(close(
            out.peak,
            cells.iter().copied().fold(0.0, f64::max),
            1e-12
        ));
        let cells = &db[14 * COLUMNS..];
        assert!(close(
            out.mean_db,
            cells.iter().sum::<f64>() / cells.len() as f64,
            1e-9
        ));
        assert!(close(
            out.peak_db,
            cells.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            1e-9
        ));
    }
}

#[test]
fn white_noise_reads_its_mean_level() {
    // white noise of standard deviation σ = 1 at a held throttle. A bin of the
    // windowed segment is a complex Gaussian of mean square σ²·Σw², and
    // Σw² = 3/8·(L − 1) for the symmetric Hann window; its modulus is
    // Rayleigh with the mean √(π/4 · σ²·Σw²). So a cell of |X|/L averages
    //   √(π · 3/8 · 599) / (2 · 600) = 0.022137
    let (fs, n, nperseg) = (2000.0, 38_000, 600.0);
    let s = session(fs, Series::from_vec(white(n)), vec![70.5; n], None);
    let raw = columns(&s);
    assert_eq!(raw.hop, 120, "19 s: 0.06 s apart");
    let sum_w2 = 3.0 / 8.0 * (nperseg - 1.0);
    let level = (std::f64::consts::PI * sum_w2).sqrt() / (2.0 * nperseg);
    assert!(close(level, 0.022_137, 1e-6));
    let cells: Vec<f64> = (5..raw.rows)
        .map(|row| raw.linear[row * COLUMNS + 69])
        .collect();
    let mean = cells.iter().sum::<f64>() / cells.len() as f64;
    assert!(close(mean, level, 0.01 * level), "{mean} against {level}");
    // in decibels the mean of 10·log10 of an exponential variable lies
    // 10·γ/ln 10 = 2.507 dB under the decibels of its mean:
    //   10·log10(2·σ²·Σw² / (L·fs)) − 2.507 = −34.267 − 2.507 = −36.774 dB
    let euler = 0.577_215_664_901_532_9;
    let db_level = 10.0 * (2.0 * sum_w2 / (nperseg * fs)).log10() - 10.0 * euler / 10f64.ln();
    assert!(close(db_level, -36.774, 1e-3));
    let cells: Vec<f64> = (5..raw.rows)
        .map(|row| raw.db[row * COLUMNS + 69])
        .collect();
    let mean_db = cells.iter().sum::<f64>() / cells.len() as f64;
    assert!(
        close(mean_db, db_level, 0.1),
        "{mean_db} against {db_level}"
    );

    // the printed mean is over all 100 columns, and two of them hold the
    // noise; the smoothing moves it between cells without changing the sum
    // (away from the edges), so mean = 2/100 of the level, and in decibels
    // −40 + 2/100 · (level + 40)
    let out = run(&s, Smoothing::Medium);
    assert!(
        close(out.mean, 0.02 * level, 0.02 * 0.02 * level),
        "{}",
        out.mean
    );
    assert!(
        close(out.mean_db, DB_FLOOR + 0.02 * (db_level - DB_FLOOR), 0.005),
        "{}",
        out.mean_db
    );
    // far from the two columns there is nothing: 0 and the decibel floor
    assert_eq!(out.grid.get(20, 100), 0.0);
    assert_eq!(out.grid_db.get(20, 100), DB_FLOOR);
    assert!(out.peak > out.mean && out.peak_db > out.mean_db);
}

#[test]
fn stairs_fill_the_columns_of_their_levels_and_of_the_straddling_segments() {
    // 20 % for 5 s, then 60 % for 5 s, at 2 kHz: segments of 600 samples
    // start every 120. Those that start at 0 … 9360 (k = 0 … 78) lie on the
    // first stair, those from 10080 (k = 84 … 160) on the second. A stair at
    // an integer T fills the columns T − 1 and T (c − 1 < T ≤ c + 1).
    let (fs, n) = (2000.0, 20_000);
    let throttle: Vec<f64> = (0..n)
        .map(|i| if i < 10_000 { 20.0 } else { 60.0 })
        .collect();
    let rpm: Vec<f64> = throttle.iter().map(|t| 100.0 * t).collect();
    let s = session(
        fs,
        Series::from_vec(white(n)),
        throttle.clone(),
        Some(rpm.clone()),
    );
    let raw = columns(&s);
    assert_eq!((raw.positions, raw.segments), (161, 161));
    let mut expected = vec![0usize; COLUMNS];
    expected[18] = 79;
    expected[19] = 79;
    expected[58] = 77;
    expected[59] = 77;
    // the five segments across the step (k = 79 … 83) have 520, 400, 280, 160
    // and 40 of their 600 samples at 20 %: means 25.33, 33.33, 41.33, 49.33
    // and 57.33 %, each in the two columns around it
    for column in [25, 26, 33, 34, 41, 42, 49, 50, 57, 58] {
        expected[column - 1] = 1;
    }
    assert_eq!(raw.counts, expected);
    assert_eq!(raw.counts.iter().sum::<usize>(), 2 * raw.segments);
    // a segment is in two columns, three never
    for throttle in [1.5, 20.0, 25.33, 99.9, 100.0] {
        let held = (1..=COLUMNS).filter(|c| in_column(throttle, *c)).count();
        assert_eq!(held, 2, "{throttle}");
    }
    assert!(in_column(20.0, 19) && in_column(20.0, 20) && !in_column(20.0, 21));
    // at the ends of the range one column, or none
    assert_eq!((1..=COLUMNS).filter(|c| in_column(0.5, *c)).count(), 1);
    assert_eq!((1..=COLUMNS).filter(|c| in_column(100.5, *c)).count(), 1);
    assert_eq!((1..=COLUMNS).filter(|c| in_column(0.0, *c)).count(), 0);
    assert_eq!((1..=COLUMNS).filter(|c| in_column(102.0, *c)).count(), 0);

    // the motor speed of a column is the median of its segments' mean speeds
    let out = run(&s, Smoothing::Medium);
    assert_eq!(out.counts, expected);
    let overlay = out.overlay.as_ref().unwrap();
    assert_eq!(overlay.motors.len(), 1);
    assert_eq!(overlay.motors[0].len(), COLUMNS);
    assert_eq!(overlay.harmonics, 3, "no RPM filter configured");
    for (ix, speed) in overlay.motors[0].iter().enumerate() {
        match (ix + 1, expected[ix]) {
            (_, 0) => assert_eq!(*speed, None, "column {}", ix + 1),
            (19 | 20, _) => assert!(close(speed.unwrap(), 2000.0, 1e-9)),
            (59 | 60, _) => assert!(close(speed.unwrap(), 6000.0, 1e-9)),
            (25 | 26, _) => assert!(close(
                speed.unwrap(),
                100.0 * (60.0 - 40.0 * 520.0 / 600.0),
                1e-9
            )),
            _ => assert!(speed.is_some()),
        }
    }
    assert!(close(
        overlay.harmonic_hz(0, 2, 18).unwrap(),
        2000.0 / 60.0 * 2.0,
        1e-9
    ));
    assert_eq!(overlay.harmonic_hz(0, 1, 0), None);
    assert_eq!(overlay.harmonic_hz(3, 1, 18), None);
    // without motor speeds there is no overlay
    let bare = session(fs, Series::from_vec(white(n)), throttle, None);
    assert!(run(&bare, Smoothing::Medium).overlay.is_none());
}

#[test]
fn the_distance_between_segments_follows_the_length_of_the_stretch() {
    // what the reference tool was seen with
    for (seconds, hop) in [
        (10.0, 0.06),
        (14.5, 0.06),
        (19.0, 0.06),
        (21.25, 0.1),
        (23.5, 0.1),
        (25.75, 0.1),
        (28.0, 0.1),
        (32.0, 0.1),
        (37.0, 0.1),
        (55.0, 0.1),
        (57.25, 0.1),
        (64.0, 0.3),
        (73.0, 0.3),
        (82.0, 0.3),
    ] {
        assert_eq!(hop_seconds(seconds), hop, "{seconds} s");
    }
    // and the two thresholds, 20 and 60 s; at 1 kHz L = 300 and
    // there are floor((n − L) / hop) segments: the last that would still fit
    // is not taken (n = 60000, hop 300: 200 fit, 199 are used)
    for (n, hop, positions) in [
        (19_999, 60, 328),
        (20_000, 100, 197),
        (59_999, 100, 596),
        (60_000, 300, 199),
        // the shortest stretch with a segment: L + hop samples
        (360, 60, 1),
    ] {
        let s = session(1000.0, Series::from_vec(vec![1.0; n]), vec![50.5; n], None);
        let raw = columns(&s);
        assert_eq!(
            (raw.nperseg, raw.hop, raw.positions),
            (300, hop, positions),
            "{n} samples"
        );
        assert_eq!(raw.positions, (n - 300) / hop);
        assert_eq!(raw.counts[49], positions);
        assert_eq!(raw.counts[50], positions);
    }
    // the epoch counts, not the log: 12 s of a 70 s log are 0.06 s apart
    let n = 70_000;
    let s = session(1000.0, Series::from_vec(vec![1.0; n]), vec![50.5; n], None);
    let ctx = AnalysisCtx {
        session: &s,
        epoch: Some(Epoch {
            start_s: 5.0,
            end_s: 16.9995,
        }),
    };
    let raw = column_means(&ctx, &GYRO).unwrap();
    assert_eq!((raw.hop, raw.positions), (60, (12_000 - 300) / 60));
    // other rates: L = round(0.3·fs), the hop round(seconds·fs)
    for (fs, nperseg, hop) in [(8000.0, 2400, 480), (500.0, 150, 30)] {
        let n = (4.0 * fs) as usize;
        let s = session(fs, Series::from_vec(vec![1.0; n]), vec![50.5; n], None);
        let raw = columns(&s);
        assert_eq!(
            (raw.nperseg, raw.hop, raw.rows),
            (nperseg, hop, nperseg / 2)
        );
    }
}

#[test]
fn an_even_kernel_reaches_half_a_cell_towards_the_lower_index() {
    let (nx, ny) = (COLUMNS, 40);
    let (q, r) = (50usize, 20usize);
    let mut m = vec![0.0; nx * ny];
    m[r * nx + q] = 1.0;
    let cell = |z: &[f64], p: usize, s: usize| z[s * nx + p];
    // "low", 5 × 1 taps, odd: centred on the cell, columns q − 2 … q + 2
    let low = smooth(&m, nx, ny, Smoothing::Low, 0.0);
    let k = kernel_taps(5);
    for (t, tap) in k.iter().enumerate() {
        assert!(close(cell(&low, q + t - 2, r), *tap, 1e-15));
    }
    assert_eq!(low.iter().filter(|v| **v != 0.0).count(), 5);
    // "low-med", 10 × 2 taps, even: out[p] = Σ K[t]·M[p + 5 − t], so the
    // cell lands on the columns q − 5 … q + 4 and on the rows r − 1 and r
    let low_med = smooth(&m, nx, ny, Smoothing::LowMed, 0.0);
    let (kt, kf) = (kernel_taps(10), kernel_taps(2));
    assert_eq!(kf, vec![0.5, 0.5]);
    for (t, tap) in kt.iter().enumerate() {
        assert!(close(cell(&low_med, q + t - 5, r), tap * 0.5, 1e-15));
        assert!(close(cell(&low_med, q + t - 5, r - 1), tap * 0.5, 1e-15));
    }
    assert_eq!(cell(&low_med, q + 5, r), 0.0);
    assert_eq!(cell(&low_med, q - 6, r), 0.0);
    assert_eq!(cell(&low_med, q, r + 1), 0.0);
    assert_eq!(low_med.iter().filter(|v| **v != 0.0).count(), 20);
    // "medium", 15 × 3: centred again, rows r − 1 … r + 1
    let medium = smooth(&m, nx, ny, Smoothing::Medium, 0.0);
    assert!(cell(&medium, q - 7, r - 1) > 0.0 && cell(&medium, q + 7, r + 1) > 0.0);
    assert_eq!(medium.iter().filter(|v| **v != 0.0).count(), 45);
    // "med-high", 20 × 4: columns q − 10 … q + 9, rows r − 2 … r + 1
    let med_high = smooth(&m, nx, ny, Smoothing::MedHigh, 0.0);
    assert!(cell(&med_high, q - 10, r - 2) > 0.0 && cell(&med_high, q + 9, r + 1) > 0.0);
    assert_eq!(cell(&med_high, q + 10, r), 0.0);
    assert_eq!(cell(&med_high, q, r + 2), 0.0);
    // every kernel sums to 1, and each is the kernel of the rule
    for level in Smoothing::ALL {
        let out = smooth(&m, nx, ny, level, 0.0);
        assert!(close(out.iter().sum::<f64>(), 1.0, 1e-12), "{level:?}");
        let by_rule = smooth_by_the_rule(&m, nx, ny, level.level(), 0.0);
        assert!(out.iter().zip(&by_rule).all(|(a, b)| close(*a, *b, 1e-15)));
        let (along_throttle, along_frequency) = level.kernel_size();
        assert_eq!(
            (along_throttle, along_frequency),
            (5 * level.level(), level.level())
        );
    }
    // zeros outside the matrix: a cell in the first column loses what the
    // kernel would put below column 1
    let mut edge = vec![0.0; nx * ny];
    edge[r * nx] = 1.0;
    let out = smooth(&edge, nx, ny, Smoothing::Low, 0.0);
    assert!(close(out.iter().sum::<f64>(), k[2] + k[3] + k[4], 1e-15));
    // and with a value outside, a matrix at that value stays there
    let floor = vec![DB_FLOOR; nx * ny];
    let out = smooth(&floor, nx, ny, Smoothing::High, DB_FLOOR);
    assert!(out.iter().all(|v| *v == DB_FLOOR));
    // while zeros outside pull its edges towards 0
    let out = smooth(&floor, nx, ny, Smoothing::High, 0.0);
    assert!(out[0] > DB_FLOOR + 10.0 && close(out[r * nx + q], DB_FLOOR, 1e-9));
    // the names and numbers of the levels
    let names: Vec<&str> = Smoothing::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["low", "low-med", "medium", "med-high", "high"]);
    assert_eq!(Smoothing::default(), Smoothing::Medium);
    for level in Smoothing::ALL {
        assert_eq!(Smoothing::parse(level.name()), Some(level));
        assert_eq!(Smoothing::parse(&level.level().to_string()), Some(level));
        assert_eq!(Smoothing::from_level(level.level()), Some(level));
    }
    assert_eq!(Smoothing::parse("0"), None);
    assert_eq!(Smoothing::parse("6"), None);
    assert_eq!(Smoothing::parse("strong"), None);
    assert_eq!(
        serde_json::to_string(&Smoothing::MedHigh).unwrap(),
        "\"med-high\""
    );
}

#[test]
fn the_decibel_matrix_of_one_tone() {
    // a tone of A = 8 on bin 60 at 2 kHz (L = 600), throttle held at 30.5 %:
    // |X| = A·L/4 there, so
    //   d = 10·log10(2·(A·L/4)² / (L·fs)) = 10·log10(A²·L / (8·fs))
    //     = 10·log10(64·600 / 16000) = 10·log10(2.4) = 3.802 dB
    let (fs, n, amplitude) = (2000.0, 20_000, 8.0);
    let s = session(
        fs,
        Series::from_vec(tone(n, 600, 60, amplitude)),
        vec![30.5; n],
        None,
    );
    let raw = columns(&s);
    let by_hand = 10.0 * 2.4f64.log10();
    assert!(close(by_hand, 3.802, 1e-3));
    assert!(close(raw.db[60 * COLUMNS + 29], by_hand, 0.03));
    assert!(close(raw.db[60 * COLUMNS + 30], by_hand, 0.03));
    // half the amplitude on either neighbour: 6.02 dB less
    assert!(close(raw.db[59 * COLUMNS + 29], by_hand - 6.02, 0.05));
    // a column without segments is at the floor, −40 dB, on every row
    assert_eq!(DB_FLOOR, -40.0);
    assert!((0..raw.rows).all(|row| raw.db[row * COLUMNS + 28] == DB_FLOOR));
    assert!((0..raw.rows).all(|row| raw.db[row * COLUMNS + 31] == DB_FLOOR));
    // "low" (5 × 1 taps, K = exp(−d²/32) / 4.70346 for d = −2 … 2): the
    // matrix + 40 is smoothed with zeros outside and 40 taken off again. On
    // the two columns of the tone:
    //   −40 + (K[2] + K[1]) · (3.802 + 40) = −40 + 0.41868 · 43.802 = −21.66 dB
    let out = run(&s, Smoothing::Low);
    let k = kernel_taps(5);
    assert!(close(k[2] + k[1], 0.418_68, 1e-5));
    assert!(close(out.grid_db.get(29, 60), -21.66, 0.02));
    assert!(close(out.grid_db.get(30, 60), -21.66, 0.02));
    assert!(close(out.peak_db, -21.66, 0.02), "{}", out.peak_db);
    // where nothing is within the kernel's reach the floor stays
    assert_eq!(out.grid_db.get(26, 60), DB_FLOOR);
    assert_eq!(out.grid_db.get(80, 200), DB_FLOOR);
    // the linear matrix of the same result
    assert!(close(
        out.peak,
        amplitude / 4.0 * (k[2] + k[1]),
        0.005 * amplitude
    ));
    // a signal of exact zeros has no logarithm: the guard keeps it finite
    let silent = session(fs, Series::from_vec(vec![0.0; n]), vec![30.5; n], None);
    let out = run(&silent, Smoothing::Medium);
    assert!(out.grid_db.z.iter().all(|v| v.is_finite()));
    assert!(out.grid.z.iter().all(|v| *v == 0.0));
    assert_eq!((out.mean, out.peak), (0.0, 0.0));
    assert!(out.peak_db == DB_FLOOR && out.mean_db < DB_FLOOR);
}

#[test]
fn mean_and_peak_leave_out_the_lowest_rows() {
    // rows/10 − 1 (integer division): 300 rows count from row 29, 150 from
    // 14, 1200 from 119; fewer than 20 rows count from the first
    for (rows, first) in [
        (300, 29),
        (150, 14),
        (1200, 119),
        (20, 1),
        (19, 0),
        (5, 0),
        (0, 0),
    ] {
        assert_eq!(first_counted_row(rows), first, "{rows} rows");
    }
    // 3 columns × 30 rows: the rows from 30/10 − 1 = 2 up count
    let (nx, ny) = (3, 30);
    let mut z = vec![1.0; nx * ny];
    z[0] = 100.0; // row 0: not counted
    z[nx + 2] = 50.0; // row 1: not counted
    z[2 * nx + 1] = 9.0; // row 2: counted
    z[29 * nx] = -5.0; // the last row: counted
    let (mean, peak) = mean_peak(&z, nx);
    let counted = 28.0 * 3.0;
    assert!(close(mean, (counted - 2.0 + 9.0 - 5.0) / counted, 1e-12));
    assert_eq!(peak, 9.0);
    // a matrix of negative values (decibels) has a negative peak
    assert_eq!(mean_peak(&vec![-40.0; nx * ny], nx), (-40.0, -40.0));
    assert_eq!(mean_peak(&[], nx), (0.0, 0.0));
    assert_eq!(mean_peak(&[], 0), (0.0, 0.0));
}

#[test]
fn an_empty_or_too_short_epoch_is_not_enough_data() {
    let (fs, n) = (1000.0, 5000);
    let s = session(fs, Series::from_vec(white(n)), vec![40.5; n], None);
    let at = |start_s: f64, end_s: f64| {
        let ctx = AnalysisCtx {
            session: &s,
            epoch: Some(Epoch { start_s, end_s }),
        };
        column_means(&ctx, &GYRO)
    };
    // 0.2 s: shorter than one segment of 0.3 s
    assert!(matches!(
        at(1.0, 1.1995),
        Err(AnalysisError::NotEnoughData(_))
    ));
    // 0.35 s = 350 samples: a segment fits, but floor((350 − 300) / 60) = 0
    assert!(matches!(
        at(1.0, 1.3495),
        Err(AnalysisError::NotEnoughData(_))
    ));
    // 0.36 s: one segment
    assert_eq!(at(1.0, 1.3595).unwrap().positions, 1);
    // an epoch outside the log, and one of a single sample
    assert!(matches!(
        at(50.0, 60.0),
        Err(AnalysisError::NotEnoughData(_))
    ));
    assert!(matches!(at(1.0, 1.0), Err(AnalysisError::NotEnoughData(_))));
    // through the analysis the same error
    let ctx = AnalysisCtx {
        session: &s,
        epoch: Some(Epoch {
            start_s: 1.0,
            end_s: 1.1,
        }),
    };
    assert!(matches!(
        ThrottleHeatmap.run(&ctx, &ThrottleHeatmapParams::default()),
        Err(AnalysisError::MissingChannel(_))
    ));
    assert!(matches!(
        ThrottleHeatmap.run(
            &ctx,
            &ThrottleHeatmapParams {
                channel: GYRO,
                ..Default::default()
            }
        ),
        Err(AnalysisError::NotEnoughData(_))
    ));
    // a throttle that is not in percent is refused, not converted
    let mut raw_throttle = s.clone();
    let mut throttle = s.channel(&ChannelId::Throttle).unwrap().clone();
    throttle.unit = Unit::Raw;
    raw_throttle.channels.insert(throttle);
    assert!(matches!(
        column_means(&whole(&raw_throttle), &GYRO),
        Err(AnalysisError::WrongUnit(..))
    ));
}

#[test]
fn a_segment_with_too_many_invalid_samples_is_left_out() {
    // 10 s at 2 kHz, a constant 5 with the samples 3000 … 3299 invalid: a
    // segment is used when at least 90 % of its 600 samples are valid, so
    // one with more than 60 invalid samples is not
    let (fs, n) = (2000.0, 20_000);
    let invalid = 3000..3300;
    let gyro = Series::from_options((0..n).map(|i| (!invalid.contains(&i)).then_some(5.0)));
    assert_eq!(gyro.invalid_count(), 300);
    let s = session(fs, gyro, vec![44.5; n], None);
    let raw = columns(&s);
    let skipped = (0..161)
        .filter(|k| {
            let (start, end) = (k * 120, k * 120 + 600);
            let lost = invalid
                .end
                .min(end)
                .saturating_sub(invalid.start.max(start));
            lost > 60
        })
        .count();
    assert_eq!(skipped, 6, "the segments that start at 2520 … 3120");
    assert_eq!(raw.positions, 161);
    assert_eq!(raw.segments, 161 - skipped);
    assert_eq!(raw.counts[43], 161 - skipped);
    assert_eq!(raw.counts[44], 161 - skipped);
    // inside a used segment the invalid samples are interpolated between
    // their neighbours: here back to the constant, so the result is that of
    // the same signal without a gap
    let clean = columns(&session(
        fs,
        Series::from_vec(vec![5.0; n]),
        vec![44.5; n],
        None,
    ));
    assert_eq!(clean.segments, 161);
    for (a, b) in raw.linear.iter().zip(&clean.linear) {
        assert!(close(*a, *b, 1e-12));
    }
    // (a constant c reads c/2 on row 0 and c/4 on row 1: the mean is not removed)
    assert!(close(raw.linear[44], 2.5, 0.01));
    assert!(close(raw.linear[COLUMNS + 44], 1.25, 0.01));
    // a log invalid throughout has no segment at all
    let gyro = Series::from_options((0..n).map(|_| None));
    let s = session(fs, gyro, vec![44.5; n], None);
    assert!(matches!(
        column_means(&whole(&s), &GYRO),
        Err(AnalysisError::NotEnoughData(_))
    ));
}
