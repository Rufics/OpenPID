//! The frequency × time map on signals made here, so that every expected
//! number follows from the rule (`docs/DESIGN.md` §6.5) and the signal alone:
//! the lengths of the segment and of its transform, where the segments lie
//! and how many there are (the first columns included), what a tone, an
//! offset, a click and white noise read, and what the moving average along
//! the frequency does inside the axis and at its two ends.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::stft::{
    DB_FLOOR, DB_SCALE, SEGMENT_S, column_count, moving_average, segment_len, segment_start,
    transform_len,
};
use openpid_analysis::{
    Analysis, AnalysisCtx, AnalysisError, Epoch, FreqSmoothing, StftAnalysis, StftOutput,
    StftParams, TimeSmoothing,
};
use openpid_core::session::Diagnostics;
use openpid_core::{
    Axis, Channel, ChannelId, ChannelSet, FirmwareInfo, FlightSession, GyroStage, Origin, Series,
    Settings, Timebase, Unit,
};
use std::f64::consts::TAU;

/// The channel every test analyses.
const GYRO: ChannelId = ChannelId::Gyro {
    axis: Axis::Roll,
    stage: GyroStage::Raw,
};

/// A session at `fs` whose unfiltered roll gyro is `gyro`.
fn session(fs: f64, gyro: Vec<f64>) -> FlightSession {
    let n = gyro.len();
    let mut channels = ChannelSet::new();
    channels.insert(Channel {
        id: GYRO,
        name: "gyroUnfilt[0]".into(),
        unit: Unit::DegreesPerSecond,
        data: Series::from_vec(gyro),
        origin: Origin::Logged {
            field: "gyroUnfilt[0]".into(),
        },
    });
    FlightSession {
        index: 0,
        offset_bytes: 0,
        firmware: FirmwareInfo::parse_revision("Synthetic 1.0"),
        profile_id: "synthetic".into(),
        settings: Settings::default(),
        raw_headers: Vec::new(),
        timebase: Timebase::from_times(
            (0..n)
                .map(|i| (i as f64 * 1.0e6 / fs).round() as i64)
                .collect(),
            Some(1.0e6 / fs),
        ),
        channels,
        events: Vec::new(),
        aux: Vec::new(),
        flight_modes: Vec::new(),
        diagnostics: Diagnostics::default(),
    }
}

fn params(smooth_freq: FreqSmoothing, smooth_time: TimeSmoothing) -> StftParams {
    StftParams {
        channel: GYRO,
        smooth_freq,
        smooth_time,
        max_hz: None,
    }
}

/// The default levels, on [`GYRO`].
fn defaults() -> StftParams {
    StftParams {
        channel: GYRO,
        ..StftParams::default()
    }
}

fn run_in(s: &FlightSession, epoch: Option<Epoch>, p: &StftParams) -> StftOutput {
    StftAnalysis
        .run(&AnalysisCtx { session: s, epoch }, p)
        .unwrap()
}

/// The map of the whole of `x` at `fs`.
fn map(fs: f64, x: Vec<f64>, smooth_freq: FreqSmoothing, smooth_time: TimeSmoothing) -> StftOutput {
    run_in(&session(fs, x), None, &params(smooth_freq, smooth_time))
}

/// The cell of row `row` and column `column`.
fn cell(out: &StftOutput, row: usize, column: usize) -> f64 {
    out.grid.z[row * out.grid.x.len() + column]
}

/// A tone of `amplitude` on bin `bin` of a transform of `nfft` points.
fn tone(n: usize, nfft: usize, bin: usize, amplitude: f64) -> Vec<f64> {
    (0..n)
        .map(|i| amplitude * (TAU * (bin * i) as f64 / nfft as f64 + 0.3).sin())
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

/// The symmetric Hann window of `n` samples, written out.
fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (TAU * i as f64 / (n - 1) as f64).cos())
        .collect()
}

/// `|Σ w[i]·e^(−2πj·d·i/nfft)|`: what the window makes of a tone `d` bins
/// of the transform away, by the direct sum (no FFT).
fn window_response(w: &[f64], nfft: usize, d: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, v) in w.iter().enumerate() {
        let phase = TAU * d * i as f64 / nfft as f64;
        re += v * phase.cos();
        im -= v * phase.sin();
    }
    re.hypot(im)
}

fn db(power: f64) -> f64 {
    10.0 * power.log10()
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn the_levels_their_factors_and_their_names() {
    assert!((SEGMENT_S - 0.3).abs() < 1e-15);
    assert_eq!(DB_SCALE, (-30.0, 10.0));
    assert_eq!(DB_FLOOR, -120.0);
    // the defaults of the web edition: time "Max", frequency "Medium"
    assert_eq!(TimeSmoothing::default(), TimeSmoothing::Max);
    assert_eq!(FreqSmoothing::default(), FreqSmoothing::Medium);
    let freq: Vec<(usize, usize)> = FreqSmoothing::ALL
        .iter()
        .map(|l| (l.factor(), l.rows()))
        .collect();
    assert_eq!(freq, [(2, 3), (5, 5), (10, 11), (20, 21)]);
    let time: Vec<usize> = TimeSmoothing::ALL.iter().map(|l| l.factor()).collect();
    assert_eq!(time, [1, 2, 5, 10]);
    let names = ["low", "medium", "high", "max"];
    for (i, name) in names.iter().enumerate() {
        assert_eq!(FreqSmoothing::ALL[i].name(), *name);
        assert_eq!(TimeSmoothing::ALL[i].name(), *name);
        assert_eq!(FreqSmoothing::parse(name), Some(FreqSmoothing::ALL[i]));
        assert_eq!(TimeSmoothing::parse(name), Some(TimeSmoothing::ALL[i]));
        // a project file and the JSON of a result write the same name
        assert_eq!(
            serde_json::to_string(&FreqSmoothing::ALL[i]).unwrap(),
            format!("\"{name}\"")
        );
        assert_eq!(
            serde_json::from_str::<TimeSmoothing>(&format!("\"{name}\"")).unwrap(),
            TimeSmoothing::ALL[i]
        );
    }
    for bad in ["", "off", "Low", "med", "medium-high", "2"] {
        assert_eq!(FreqSmoothing::parse(bad), None, "{bad}");
        assert_eq!(TimeSmoothing::parse(bad), None, "{bad}");
    }
}

#[test]
fn the_moving_average_inside_the_axis_and_at_its_ends() {
    let values: Vec<f64> = (0..12).map(|i| f64::from(i * i % 7) - 2.5).collect();
    for rows in [3, 5, 11, 21] {
        let out = moving_average(&values, rows);
        assert_eq!(out.len(), values.len());
        let h = rows / 2;
        for (i, v) in out.iter().enumerate() {
            // the rows i − h … i + h that exist
            let read: Vec<f64> = (0..values.len())
                .filter(|j| j.abs_diff(i) <= h)
                .map(|j| values[j])
                .collect();
            let mean = read.iter().sum::<f64>() / read.len() as f64;
            assert!(close(*v, mean, 1e-12), "rows {rows} row {i}");
        }
    }
    // the numbers at the ends, written out: three rows, then five
    let v = [6.0, 0.0, 3.0, 9.0, 12.0, -6.0];
    assert_eq!(moving_average(&v, 3), [3.0, 3.0, 4.0, 8.0, 5.0, 3.0]);
    assert_eq!(
        moving_average(&v, 5),
        [3.0, 4.5, 6.0, 3.6, 4.5, 5.0],
        "(6+0+3)/3, (6+0+3+9)/4, …, (9+12−6)/3"
    );
    // a level stays level, one row is the identity, nothing stays nothing
    assert!(moving_average(&[-12.5; 30], 21).iter().all(|x| *x == -12.5));
    assert_eq!(moving_average(&v, 1), v);
    assert!(moving_average(&[], 5).is_empty());
    // fewer values than rows: every row is the mean of all of them
    assert_eq!(moving_average(&[1.0, 2.0, 6.0], 21), [3.0, 3.0, 3.0]);
}

#[test]
fn the_transform_is_the_segment_padded_to_a_power_of_two() {
    // L = round(0.3·fs), then zeros to the next power of two: 1 024 points
    // at 2 kHz, the rows 0 … 512, fs/1 024 = 1.953125 Hz apart
    for (fs, nperseg, nfft) in [
        (1000.0, 300, 512),
        (2000.0, 600, 1024),
        (4000.0, 1200, 2048),
        (8000.0, 2400, 4096),
        (1010.0, 303, 512),
        (3200.0, 960, 1024),
    ] {
        assert_eq!(segment_len(fs), nperseg);
        assert_eq!(transform_len(nperseg), nfft);
        let out = map(
            fs,
            white(nperseg * 3),
            FreqSmoothing::Medium,
            TimeSmoothing::Low,
        );
        assert_eq!((out.nperseg, out.nfft), (nperseg, nfft), "{fs} Hz");
        assert_eq!(out.grid.y.len(), nfft / 2 + 1);
        for (k, f) in out.grid.y.iter().enumerate() {
            assert!(close(*f, k as f64 * fs / nfft as f64, 1e-9));
        }
        assert!(close(*out.grid.y.last().unwrap(), fs / 2.0, 1e-9));
    }
    assert_eq!(transform_len(512), 512, "a power of two stays");
    assert_eq!(segment_len(1.0), 2, "never shorter than two samples");
}

#[test]
fn a_tone_on_a_bin_reads_its_power_density() {
    // 2 kHz: L = 600, nfft = 1 024. A tone of amplitude A on bin 100 of the
    // transform (195.3 Hz): |X| = A·Σw/2 on its bin, Σw = (L − 1)/2 for the
    // symmetric window, so 2·|X|²/(L·fs) = A²·(L − 1)²/(8·L·fs):
    // 20·log10(A) − 14.27 dB. A bin d away holds A·|W(d)|/2, W the window's
    // own transform, summed here directly.
    let (fs, nperseg, nfft, bin) = (2000.0, 600, 1024, 100);
    let w = hann(nperseg);
    let sum: f64 = w.iter().sum();
    assert!(close(sum, (nperseg - 1) as f64 / 2.0, 1e-9));
    let scale = nperseg as f64 * fs;
    for amplitude in [1.0, 8.0, 40.0] {
        let level = 20.0 * f64::log10(amplitude);
        let at = |d: i32| {
            let x = amplitude * window_response(&w, nfft, f64::from(d)) / 2.0;
            db(2.0 * x * x / scale)
        };
        assert!(close(at(0), level - 14.27, 0.005));
        // the padded bins lie 600/1 024 of a segment's bin apart: the bins
        // beside the tone keep most of it (−1.96 dB), two away −8.51 dB
        assert!(close(at(1) - at(0), -1.96, 0.005), "{}", at(1) - at(0));
        assert!(close(at(2) - at(0), -8.51, 0.005), "{}", at(2) - at(0));
        let x = tone(6000, nfft, bin, amplitude);
        let run = |level| map(fs, x.clone(), level, TimeSmoothing::Low);
        let low = run(FreqSmoothing::Low);
        let medium = run(FreqSmoothing::Medium);
        let high = run(FreqSmoothing::High);
        assert!(close(low.grid.y[bin], 195.3125, 1e-9));
        for column in 0..low.grid.x.len() {
            // "low", three rows: the mean of the decibels of bins 99 … 101
            let three = (at(-1) + at(0) + at(1)) / 3.0;
            assert!(close(cell(&low, bin, column), three, 0.002), "{column}");
            // "medium", five rows: bins 98 … 102
            let five = (at(-2) + at(-1) + at(0) + at(1) + at(2)) / 5.0;
            assert!(close(cell(&medium, bin, column), five, 0.002));
            let next = (at(-1) + at(0) + at(1) + at(2) + at(3)) / 5.0;
            assert!(close(cell(&medium, bin + 1, column), next, 0.002));
            // the wider means take more of the rows without the tone in
            assert!(cell(&high, bin, column) < cell(&medium, bin, column) - 5.0);
        }
        // the levels of "low" and of the default, to two decimals
        assert!(close(cell(&low, bin, 0) - level, -15.58, 0.005));
        assert!(close(cell(&medium, bin, 0) - level, -18.46, 0.005));
    }
}

#[test]
fn the_bins_of_zero_hertz_and_of_half_the_rate_count_once() {
    // An offset c: |X₀| = c·Σw, and the bin of 0 Hz has no factor 2:
    // c²·(L − 1)²/(4·L·fs), 20·log10(c) − 11.26 dB; bin k holds c·|W(k)|
    // with the 2. c·(−1)ⁱ is the same moved to the last bin, nfft/2, which
    // has no factor 2 either. The mean is not removed.
    let (fs, nperseg, nfft, c) = (2000.0, 600, 1024, 4.0_f64);
    let level = 20.0 * c.log10();
    let w = hann(nperseg);
    let scale = nperseg as f64 * fs;
    let bin = |k: usize| {
        let sides = if k == 0 { 1.0 } else { 2.0 };
        level + db(sides * window_response(&w, nfft, k as f64).powi(2) / scale)
    };
    assert!(close(bin(0) - level, -11.26, 0.005));
    let run = |x: Vec<f64>| map(fs, x, FreqSmoothing::Low, TimeSmoothing::Low);
    let offset = run(vec![c; 3000]);
    let alternating: Vec<f64> = (0..3000).map(|i| if i % 2 == 0 { c } else { -c }).collect();
    let top = run(alternating);
    let last = top.grid.y.len() - 1;
    assert_eq!(last, nfft / 2);
    for column in 0..offset.grid.x.len() {
        // "low" at an end: the mean of the two rows that exist
        let end = f64::midpoint(bin(0), bin(1));
        assert!(close(cell(&offset, 0, column), end, 1e-6));
        assert!(close(
            cell(&offset, 1, column),
            (bin(0) + bin(1) + bin(2)) / 3.0,
            1e-6
        ));
        assert!(close(cell(&top, last, column), end, 1e-6));
    }
    // a click: every bin holds the same |X|, so the rows inside the axis
    // read 3.01 dB above the two end bins, which have no 2
    let mut click = vec![0.0; 3000];
    click[300] = 50.0;
    let base = db(2500.0 * w[300].powi(2) / scale);
    let doubled = base + 10.0 * 2f64.log10();
    // column 0 holds samples 0 … 599: the click near its middle
    let low = map(fs, click.clone(), FreqSmoothing::Low, TimeSmoothing::Low);
    assert!(close(cell(&low, 0, 0), f64::midpoint(base, doubled), 1e-9));
    assert!(close(cell(&low, 1, 0), (base + 2.0 * doubled) / 3.0, 1e-9));
    assert!(close(cell(&low, 200, 0), doubled, 1e-9));
    assert!(close(
        cell(&low, last, 0),
        f64::midpoint(base, doubled),
        1e-9
    ));
    assert!(close(
        cell(&low, last - 1, 0),
        (base + 2.0 * doubled) / 3.0,
        1e-9
    ));
    // "medium" at the ends: three rows of five, then four
    let medium = map(fs, click, FreqSmoothing::Medium, TimeSmoothing::Low);
    assert!(close(
        cell(&medium, 0, 0),
        (base + 2.0 * doubled) / 3.0,
        1e-9
    ));
    assert!(close(
        cell(&medium, 1, 0),
        (base + 3.0 * doubled) / 4.0,
        1e-9
    ));
    assert!(close(
        cell(&medium, 2, 0),
        (base + 4.0 * doubled) / 5.0,
        1e-9
    ));
    assert!(close(cell(&medium, 3, 0), doubled, 1e-9));
    assert!(close(
        cell(&medium, last, 0),
        (base + 2.0 * doubled) / 3.0,
        1e-9
    ));
}

#[test]
fn nothing_at_all_reads_the_floor() {
    // a channel that is all zero: every cell −120 dB, as the web edition
    // draws it (each bin reads −300 dB before the smoothing; the floor holds
    // the cells, not the bins)
    let silence = map(
        2000.0,
        vec![0.0; 3000],
        FreqSmoothing::Medium,
        TimeSmoothing::Max,
    );
    assert!(silence.grid.z.iter().all(|v| *v == DB_FLOOR));
    // noise of 0.01 deg/s RMS (a density of −70 dB) lies above it, noise
    // of 10⁻⁵ (−130 dB) below it
    let noise = |sigma: f64| {
        let x = white(3000).iter().map(|v| v * sigma).collect();
        map(2000.0, x, FreqSmoothing::Medium, TimeSmoothing::Max)
    };
    assert!(noise(1e-2).grid.z.iter().all(|v| *v > DB_FLOOR + 20.0));
    assert!(noise(1e-5).grid.z.iter().all(|v| *v == DB_FLOOR));
}

#[test]
fn white_noise_reads_its_density_less_the_window_and_the_mean_of_decibels() {
    // White noise of RMS σ at 2 kHz has the one-sided density σ²/(fs/2).
    // A cell holds 2·|X|²/(L·fs) with E|X|² = σ²·Σw² and Σw² = 3·L/8 for a
    // Hann window (the zeros of the padding add nothing), with no
    // compensation for the window: the mean power of a cell is the density
    // times 3/8, 4.26 dB below it. |X|² of a bin that is not 0 Hz or half the
    // rate is exponentially distributed, and the mean of the decibels of
    // such a value is 10·γ/ln(10) = 2.51 dB below the decibels of its mean
    // (γ = 0.5772, Euler's constant): 6.77 dB below the density in all.
    let (fs, sigma, n) = (2000.0, 5.0, 60_000);
    let x: Vec<f64> = white(n).iter().map(|v| v * sigma).collect();
    // "low" along the time: a segment apart, the columns independent
    let out = map(fs, x.clone(), FreqSmoothing::Low, TimeSmoothing::Low);
    let raw_rows = 513;
    assert_eq!((out.grid.y.len(), out.grid.x.len()), (raw_rows, 100));
    let density = 10.0 * (sigma * sigma / (fs / 2.0)).log10();
    assert!(close(density, -16.02, 0.005));
    let window = 10.0 * f64::log10(3.0 / 8.0);
    let log_mean = -10.0 * 0.577_215_664_9 / std::f64::consts::LN_10;
    assert!(close(window, -4.26, 0.005) && close(log_mean, -2.51, 0.005));
    // the mean of the rows 2 … 510 (the moving average keeps the mean of
    // the decibels away from the ends)
    let cells: Vec<f64> = (2..raw_rows - 2)
        .flat_map(|row| (0..100).map(move |column| (row, column)))
        .map(|(row, column)| cell(&out, row, column))
        .collect();
    let mean = cells.iter().sum::<f64>() / cells.len() as f64;
    // Tolerance: the decibels of one bin scatter by (10/ln 10)·π/√6 =
    // 5.57 dB; the 100 columns are independent, and the 509 padded rows are
    // worth about 150 independent bins (the padding puts 1.7 rows on a bin
    // of the segment, and the window ties a bin to its neighbours): the mean
    // scatters by about 5.57/√15 000 = 0.045 dB. 0.15 dB is three of that.
    assert!(
        close(mean, density + window + log_mean, 0.15),
        "mean {mean}, density {density}"
    );
    // the mean of three rows calms the scatter
    let var = cells.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / cells.len() as f64;
    assert!(var.sqrt() < 5.0, "{}", var.sqrt());
    let high = map(fs, x, FreqSmoothing::High, TimeSmoothing::Low);
    let wide: Vec<f64> = (10..raw_rows - 10)
        .flat_map(|row| (0..100).map(move |column| (row, column)))
        .map(|(row, column)| cell(&high, row, column))
        .collect();
    let mean_high = wide.iter().sum::<f64>() / wide.len() as f64;
    assert!(close(mean_high, mean, 0.05), "{mean_high} vs {mean}");
    let var_high = wide.iter().map(|v| (v - mean_high).powi(2)).sum::<f64>() / wide.len() as f64;
    assert!(var_high.sqrt() < 0.6 * var.sqrt());
}

#[test]
fn the_distance_between_segments_by_level_and_rate() {
    // L = round(0.3·fs); low: L, medium: L/2, high: L/5, max: L/10, that is
    // 0.3, 0.15, 0.06 and 0.03 s
    for (fs, nperseg, hops) in [
        (1000.0, 300, [300, 150, 60, 30]),
        (2000.0, 600, [600, 300, 120, 60]),
        (4000.0, 1200, [1200, 600, 240, 120]),
        (8000.0, 2400, [2400, 1200, 480, 240]),
    ] {
        assert_eq!(segment_len(fs), nperseg);
        let levels = TimeSmoothing::ALL.iter().zip(hops);
        for ((level, hop), seconds) in levels.zip([0.3, 0.15, 0.06, 0.03]) {
            assert_eq!(level.hop(nperseg), hop, "{fs} Hz {level:?}");
            assert!(close(hop as f64 / fs, seconds, 1e-12));
        }
    }
    // a part that is not whole is rounded, and never 0
    assert_eq!(TimeSmoothing::Medium.hop(303), 152);
    assert_eq!(TimeSmoothing::High.hop(303), 61);
    assert_eq!(TimeSmoothing::Max.hop(303), 30);
    assert_eq!(TimeSmoothing::Max.hop(4), 1);
    assert_eq!(TimeSmoothing::Max.hop(2), 1);
}

#[test]
fn how_many_columns_and_which_segment_each_holds() {
    // Column k stands at sample k·hop of the stretch; there are
    // floor((N − L)/hop) + 1 of them. The first F hold the segment that
    // starts there, the later ones the segment centred there.
    //
    // N = 90 001 (the samples 2 000 … 92 000 of a log of 47 s at 2 kHz):
    //   low,    hop 600: floor(89 401/600) + 1 = 150
    //   medium, hop 300: floor(89 401/300) + 1 = 299
    //   high,   hop 120: floor(89 401/120) + 1 = 746
    //   max,    hop  60: floor(89 401/60)  + 1 = 1 491
    for (hop, count) in [(600, 150), (300, 299), (120, 746), (60, 1491)] {
        assert_eq!(column_count(90_001, 600, hop), Some(count));
    }
    // one segment is enough for one column; less is none
    assert_eq!(column_count(600, 600, 60), Some(1));
    assert_eq!(column_count(659, 600, 60), Some(1));
    assert_eq!(column_count(660, 600, 60), Some(2));
    assert_eq!(column_count(599, 600, 60), None);
    assert_eq!(column_count(0, 600, 60), None);
    assert_eq!(column_count(10, 3, 0), Some(8), "a distance of 0 is 1");
    // the first-columns rule at 2 kHz
    let max = TimeSmoothing::Max;
    let starts: Vec<usize> = (0..14).map(|k| segment_start(k, 600, max)).collect();
    assert_eq!(
        starts,
        [
            0, 60, 120, 180, 240, 300, 360, 420, 480, 540, 300, 360, 420, 480
        ]
    );
    for (level, factor) in TimeSmoothing::ALL.iter().zip([1, 2, 5, 10]) {
        let hop = level.hop(600);
        for k in 0..factor {
            assert_eq!(segment_start(k, 600, *level), k * hop, "{level:?} {k}");
        }
        for k in factor..factor + 5 {
            assert_eq!(segment_start(k, 600, *level), k * hop - 300, "{level:?}");
        }
    }
    // at every rate and level the segments lie inside the stretch, the last
    // one too
    for fs in [1000.0, 2000.0, 4000.0, 8000.0, 1010.0] {
        let nperseg = segment_len(fs);
        for level in TimeSmoothing::ALL {
            for n in [nperseg, nperseg + 1, 3 * nperseg + 7, 20 * nperseg] {
                let count = column_count(n, nperseg, level.hop(nperseg)).unwrap();
                let end = segment_start(count - 1, nperseg, level) + nperseg;
                assert!(end <= n, "{fs} {level:?} {n}");
            }
        }
    }
}

#[test]
fn a_click_shows_which_segment_each_column_holds() {
    // One sample of 50 at sample 1 000 (0.5 s) of a stretch at 2 kHz, "max":
    // the column k holds the samples [s, s + 600) with s from the rule, and
    // its bins read the click through the window there, w[1000 − s]
    let (fs, nperseg, at) = (2000.0, 600, 1000);
    let mut x = vec![0.0; 6000];
    x[at] = 50.0;
    let out = map(fs, x, FreqSmoothing::Medium, TimeSmoothing::Max);
    let w = hann(nperseg);
    let read = |k: usize| cell(&out, 200, k);
    for k in 0..out.grid.x.len() {
        let s = segment_start(k, nperseg, TimeSmoothing::Max);
        let expected = if (s..s + nperseg).contains(&at) {
            db(2.0 * 2500.0 * w[at - s].powi(2) / (nperseg as f64 * fs))
        } else {
            DB_FLOOR
        };
        assert!(close(read(k), expected, 1e-9), "column {k}");
    }
    // the columns 7, 8 and 9 start at 420, 480 and 540 and hold the click;
    // column 10 is the segment centred at 600 (300 … 899), without it; the
    // columns 12 … 14 are the segments of the columns 7 … 9 again
    for k in 0..7 {
        assert_eq!(read(k), DB_FLOOR, "column {k}");
    }
    assert!(read(7) > DB_FLOOR && read(8) > read(7) && read(9) > read(8));
    assert_eq!((read(10), read(11)), (DB_FLOOR, DB_FLOOR));
    assert_eq!((read(12), read(13), read(14)), (read(7), read(8), read(9)));
    // the time of a column is k·hop/fs, whichever segment it holds
    for (k, t) in out.grid.x.iter().enumerate() {
        assert!(close(*t, k as f64 * 0.03, 1e-12));
    }
}

#[test]
fn the_columns_of_an_epoch_and_their_times() {
    // a log of 47 s at 2 kHz, the stretch 1 … 46 s: the samples
    // 2 000 … 92 000, both ends included, 90 001 samples
    let s = session(2000.0, tone(94_000, 1024, 100, 1.0));
    let epoch = Some(Epoch {
        start_s: 1.0,
        end_s: 46.0,
    });
    for (level, hop, count) in [
        (TimeSmoothing::Low, 600, 150),
        (TimeSmoothing::Medium, 300, 299),
        (TimeSmoothing::High, 120, 746),
        (TimeSmoothing::Max, 60, 1491),
    ] {
        let out = run_in(&s, epoch, &params(FreqSmoothing::Medium, level));
        assert_eq!(
            (out.nperseg, out.nfft, out.hop),
            (600, 1024, hop),
            "{level:?}"
        );
        assert_eq!(
            (out.smooth_freq, out.smooth_time),
            (FreqSmoothing::Medium, level)
        );
        assert_eq!(out.grid.x.len(), count, "{level:?}");
        assert_eq!(out.grid.z.len(), count * 513);
        // the time of a column from the first sample of the log: 1 s, then
        // a step of the distance
        let step = hop as f64 / 2000.0;
        for (k, t) in out.grid.x.iter().enumerate() {
            assert!(close(*t, 1.0 + k as f64 * step, 1e-9), "{level:?}");
        }
        assert!(close(*out.grid.x.last().unwrap(), 45.70, 1e-9), "{level:?}");
        // a steady tone reads the same in every column
        let first = cell(&out, 100, 0);
        assert!((0..count).all(|k| close(cell(&out, 100, k), first, 1e-6)));
    }
}

#[test]
fn a_short_stretch_still_has_a_map() {
    // a log of 10 s with the first and the last second left out: 8 s,
    // 16 001 samples, floor(15 401/60) + 1 = 257 columns (decision D-014:
    // OpenPID draws the map of any stretch that holds a segment)
    let s = session(2000.0, white(20_000));
    let epoch = Some(Epoch {
        start_s: 1.0,
        end_s: 9.0,
    });
    let out = run_in(&s, epoch, &defaults());
    assert_eq!(out.grid.x.len(), 257);
    assert!(close(*out.grid.x.last().unwrap(), 1.0 + 256.0 * 0.03, 1e-9));
}

#[test]
fn the_frequency_range_is_cut_after_the_smoothing() {
    let (fs, n) = (4000.0, 12_000);
    let x: Vec<f64> = white(n)
        .iter()
        .zip(tone(n, 2048, 256, 6.0))
        .map(|(a, b)| a + b)
        .collect();
    let s = session(fs, x);
    for level in FreqSmoothing::ALL {
        let whole = run_in(&s, None, &params(level, TimeSmoothing::Medium));
        // to half the sample rate by default: bins 0 … 1 024 of 2 048
        assert_eq!(whole.grid.y.len(), 1025);
        assert!(close(*whole.grid.y.last().unwrap(), 2000.0, 1e-9));
        let cut = run_in(
            &s,
            None,
            &StftParams {
                max_hz: Some(500.0),
                ..params(level, TimeSmoothing::Medium)
            },
        );
        // the rows up to 500 Hz, 500 Hz included: bins 0 … 256
        assert_eq!(cut.grid.y.len(), 257);
        assert_eq!(cut.grid.y[..], whole.grid.y[..257]);
        assert_eq!(cut.grid.x, whole.grid.x);
        // and they are the rows of the whole map: the row at the cut was
        // smoothed with the rows above it
        let nt = whole.grid.x.len();
        assert_eq!(cut.grid.z[..], whole.grid.z[..257 * nt], "{level:?}");
    }
    // a bound above half the rate changes nothing, one below the first row
    // keeps that row only
    let bound = |max_hz| {
        run_in(
            &s,
            None,
            &StftParams {
                max_hz: Some(max_hz),
                ..defaults()
            },
        )
    };
    assert_eq!(bound(1e6).grid.y.len(), 1025);
    assert_eq!(bound(1.0).grid.y, [0.0]);
}

#[test]
fn what_the_result_says_about_itself() {
    let out = run_in(
        &session(1000.0, tone(2000, 512, 50, 2.0)),
        None,
        &defaults(),
    );
    assert_eq!(out.channel, GYRO);
    assert_eq!(out.unit, "deg/s");
    assert!(close(out.fs, 1000.0, 1e-9));
    assert_eq!((out.nperseg, out.nfft, out.hop), (300, 512, 30));
    assert_eq!(out.grid.x_label, "time (s)");
    assert_eq!(out.grid.y_label, "frequency (Hz)");
    assert_eq!(out.grid.z_label, "power density (dB, (deg/s)²/Hz)");
    // the rows are fs/nfft apart
    assert!(close(out.grid.y[1] - out.grid.y[0], 1000.0 / 512.0, 1e-9));
    // the JSON of the CLI carries the levels by name
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["smooth_freq"], "medium");
    assert_eq!(json["smooth_time"], "max");
    assert_eq!(
        (json["hop"].as_u64(), json["nfft"].as_u64()),
        (Some(30), Some(512))
    );
}

#[test]
fn a_segment_of_an_odd_number_of_samples() {
    // 1010 Hz: L = round(303) = 303, nfft 512, bins 0 … 256; max:
    // round(30.3) = 30 samples apart; L/2 rounded down is 151
    let fs = 1010.0;
    let out = map(fs, white(3030), FreqSmoothing::Medium, TimeSmoothing::Max);
    assert_eq!((out.nperseg, out.nfft, out.hop), (303, 512, 30));
    assert_eq!(out.grid.y.len(), 257);
    assert_eq!(column_count(3030, 303, 30), Some(91));
    assert_eq!(out.grid.x.len(), 91);
    assert_eq!(segment_start(10, 303, TimeSmoothing::Max), 300 - 151);
    assert!(close(out.grid.x[1], 30.0 / fs, 1e-6));
    assert!(out.grid.z.iter().all(|v| v.is_finite()));
}

#[test]
fn a_stretch_too_short_for_a_segment_is_refused() {
    // 2 kHz: a segment is 600 samples, and 600 samples give one column
    let fits = session(2000.0, white(600));
    let short = session(2000.0, white(599));
    let p = defaults();
    assert_eq!(run_in(&fits, None, &p).grid.x.len(), 1);
    let refused = StftAnalysis.run(
        &AnalysisCtx {
            session: &short,
            epoch: None,
        },
        &p,
    );
    match refused {
        Err(AnalysisError::NotEnoughData(why)) => {
            assert_eq!(why, "599 samples, a segment needs 600");
        }
        other => panic!("{other:?}"),
    }
    // an epoch of a long log that is too short, and a channel that is not there
    let long = session(2000.0, white(20_000));
    let ctx = AnalysisCtx {
        session: &long,
        epoch: Some(Epoch {
            start_s: 1.0,
            end_s: 1.2,
        }),
    };
    assert!(matches!(
        StftAnalysis.run(&ctx, &p),
        Err(AnalysisError::NotEnoughData(_))
    ));
    let missing = StftParams {
        channel: ChannelId::Throttle,
        ..p
    };
    assert!(matches!(
        StftAnalysis.run(&ctx, &missing),
        Err(AnalysisError::MissingChannel(_))
    ));
}
