//! The frequency × time map on signals made here, so that every expected
//! number follows from the rule (`docs/DESIGN.md` §6.5) and the signal alone:
//! where the segments lie and how many there are, what a tone, an offset and
//! white noise read before the smoothing, and what the line fitted along the
//! frequency does inside the axis and at its two ends.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_analysis::stft::{
    DB_SCALE, SEGMENT_S, SPAN_HIGH, SPAN_LOW, SPAN_MEDIUM, columns, line_weights, segment_len,
    smooth_line,
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

fn params(smooth_freq: FreqSmoothing, smooth_time: TimeSmoothing) -> StftParams {
    StftParams {
        channel: GYRO,
        smooth_freq,
        smooth_time,
        max_hz: None,
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

/// Values that are neither level nor a line.
fn wavy(rows: usize) -> Vec<f64> {
    (0..rows)
        .map(|i| -20.0 + 7.0 * (i as f64 * 0.9).sin() + 0.3 * i as f64 + ((i * i) % 7) as f64)
        .collect()
}

/// The smoothed value of row `i` written out from the rule, with the rows
/// counted from 0 and not from `i`: the `span` nearest rows (shifted to stay
/// inside), the weights `(1 − (d/d_max)³)³`, the line `a + b·j` of weighted
/// least squares from its two normal equations, read at `i`.
fn fit_by_the_rule(values: &[f64], i: usize, span: usize) -> f64 {
    let rows = values.len() as isize;
    let half = (span / 2) as isize;
    let count = (span as isize).min(rows);
    let first = (i as isize - half).clamp(0, rows - count);
    let read: Vec<isize> = (first..first + count).collect();
    let d_max = read
        .iter()
        .map(|j| (j - i as isize).abs())
        .max()
        .unwrap_or(0);
    if d_max == 0 {
        return values[i];
    }
    let (mut sw, mut sx, mut sxx, mut sy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for j in read {
        let d = (j - i as isize).abs() as f64 / d_max as f64;
        let w = (1.0 - d.powi(3)).powi(3);
        let (x, y) = (j as f64, values[j as usize]);
        sw += w;
        sx += w * x;
        sxx += w * x * x;
        sy += w * y;
        sxy += w * x * y;
    }
    let det = sw * sxx - sx * sx;
    if det.abs() < 1e-9 {
        // one row with a weight: no line
        return sy / sw;
    }
    let b = (sw * sxy - sx * sy) / det;
    let a = (sy - b * sx) / sw;
    a + b * i as f64
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

const ALL_SPANS: [usize; 3] = [SPAN_LOW, SPAN_MEDIUM, SPAN_HIGH];

#[test]
fn the_levels_and_their_names() {
    assert!((SEGMENT_S - 0.3).abs() < 1e-15);
    assert_eq!((SPAN_LOW, SPAN_MEDIUM, SPAN_HIGH), (5, 11, 21));
    assert_eq!(DB_SCALE, (-30.0, 10.0));
    assert_eq!(FreqSmoothing::default(), FreqSmoothing::Low);
    assert_eq!(TimeSmoothing::default(), TimeSmoothing::Low);
    let spans: Vec<usize> = FreqSmoothing::ALL.iter().map(|l| l.span()).collect();
    assert_eq!(spans, [1, 5, 11, 21]);
    let parts: Vec<usize> = TimeSmoothing::ALL
        .iter()
        .map(|l| l.columns_per_segment())
        .collect();
    assert_eq!(parts, [1, 2, 5, 10]);
    let names = ["off", "low", "medium", "high"];
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
    // "med", as the list of the reference tool writes it
    assert_eq!(FreqSmoothing::parse("med"), Some(FreqSmoothing::Medium));
    assert_eq!(TimeSmoothing::parse("med"), Some(TimeSmoothing::Medium));
    for bad in ["", "Low", "medium-high", "2"] {
        assert_eq!(FreqSmoothing::parse(bad), None, "{bad}");
        assert_eq!(TimeSmoothing::parse(bad), None, "{bad}");
    }
}

#[test]
fn a_straight_line_passes_the_smoother_unchanged() {
    // a line fitted to a line is that line, wherever the rows read lie:
    // inside the axis and at both ends, at every span
    let line: Vec<f64> = (0..60).map(|i| -35.0 + 0.7 * f64::from(i)).collect();
    for span in ALL_SPANS {
        let out = smooth_line(&line, span);
        for (i, (a, b)) in out.iter().zip(&line).enumerate() {
            assert!(close(*a, *b, 1e-9), "span {span} row {i}: {a} vs {b}");
        }
    }
    // and a level stays level
    for span in ALL_SPANS {
        assert!(
            smooth_line(&[-12.5; 30], span)
                .iter()
                .all(|v| close(*v, -12.5, 1e-9))
        );
    }
}

#[test]
fn off_is_the_identity() {
    let values = wavy(40);
    assert_eq!(smooth_line(&values, FreqSmoothing::Off.span()), values);
    assert_eq!(line_weights(40, 7, 1), (7, vec![1.0]));
    assert_eq!(line_weights(40, 7, 0), (7, vec![1.0]), "no span: the row");
    assert!(smooth_line(&[], SPAN_LOW).is_empty());
}

#[test]
fn inside_the_axis_a_row_is_a_weighted_mean_of_its_neighbours() {
    // the rows read lie evenly around the row, so the line is level at it:
    // the weights are (1 − (d/h)³)³ over their sum, h = span/2 being the
    // farthest row read, which therefore weighs nothing
    for span in ALL_SPANS {
        let h = span / 2;
        let raw: Vec<f64> = (0..span)
            .map(|k| (1.0 - (k.abs_diff(h) as f64 / h as f64).powi(3)).powi(3))
            .collect();
        let sum: f64 = raw.iter().sum();
        let (first, weights) = line_weights(100, 50, span);
        assert_eq!((first, weights.len()), (50 - h, span));
        for (k, w) in weights.iter().enumerate() {
            assert!(close(*w, raw[k] / sum, 1e-12), "span {span} tap {k}: {w}");
        }
        assert_eq!(weights[0], 0.0);
        assert_eq!(weights[span - 1], 0.0);
        // the same read off the answer to one row standing out of nothing
        let mut impulse = vec![0.0; 100];
        impulse[50] = 1.0;
        let out = smooth_line(&impulse, span);
        for (k, w) in weights.iter().enumerate() {
            assert!(close(out[50 - h + k], *w, 1e-12));
        }
        assert!(out[..50 - h].iter().all(|v| *v == 0.0));
        assert!(out[50 + h + 1..].iter().all(|v| *v == 0.0));
    }
    // the numbers themselves, to four decimals: span 5 is a mean of three rows
    let (_, low) = line_weights(100, 50, SPAN_LOW);
    for (w, expected) in low.iter().zip([0.0, 0.2863, 0.4274, 0.2863, 0.0]) {
        assert!(close(*w, expected, 1e-4), "{low:?}");
    }
    // span 11, from the farthest row to the middle one
    let (_, medium) = line_weights(100, 50, SPAN_MEDIUM);
    for (w, expected) in medium
        .iter()
        .zip([0.0, 0.0201, 0.0832, 0.1417, 0.1686, 0.1728])
    {
        assert!(close(*w, expected, 1e-4), "{medium:?}");
    }
    // span 21: the middle row and the one beside it
    let (_, high) = line_weights(100, 50, SPAN_HIGH);
    assert!(close(high[10], 0.0864, 5e-5), "{high:?}");
    assert!(close(high[9], 0.0862, 5e-5), "{high:?}");
    assert!(close(high[1], 0.0017, 5e-5), "{high:?}");
}

#[test]
fn at_the_ends_the_rows_lie_to_one_side_and_the_line_is_not_level() {
    // row 0 at span 5 reads rows 0 … 4; the farthest is 4 rows away, so the
    // weights of the fit are 1, (1 − 1/64)³, (1 − 1/8)³, (1 − 27/64)³ and 0
    let (first, weights) = line_weights(50, 0, SPAN_LOW);
    assert_eq!((first, weights.len()), (0, 5));
    assert!(close(weights.iter().sum::<f64>(), 1.0, 1e-12));
    // the value of a line at its own row: the offsets weigh out
    let lever: f64 = weights.iter().enumerate().map(|(j, w)| w * j as f64).sum();
    assert!(close(lever, 0.0, 1e-12));
    // not a mean: the row itself weighs most, and a row on the far side
    // weighs less than nothing
    assert!(weights[0] > 0.6 && weights[3] < 0.0, "{weights:?}");
    assert_eq!(weights[4], 0.0);
    // row 1 reads the same five rows, 3 rows being the farthest
    let (first, weights) = line_weights(50, 1, SPAN_LOW);
    assert_eq!((first, weights.len()), (0, 5));
    assert_eq!(weights[4], 0.0);
    // the last rows mirror the first
    let (first, mirrored) = line_weights(50, 48, SPAN_LOW);
    assert_eq!(first, 45);
    for (a, b) in mirrored.iter().rev().zip(&weights) {
        assert!(close(*a, *b, 1e-12));
    }
    // every row of every span against the rule written out another way
    let values = wavy(64);
    for span in ALL_SPANS {
        let out = smooth_line(&values, span);
        for (i, v) in out.iter().enumerate() {
            let expected = fit_by_the_rule(&values, i, span);
            assert!(close(*v, expected, 1e-9), "span {span} row {i}");
        }
        // inside the axis nothing reaches beyond span/2 rows
        let (first, weights) = line_weights(64, 30, span);
        assert_eq!((first, weights.len()), (30 - span / 2, span));
    }
}

#[test]
fn with_fewer_rows_than_the_span_all_rows_are_read() {
    for rows in 1..=SPAN_HIGH {
        let values = wavy(rows);
        let line: Vec<f64> = (0..rows).map(|i| 4.0 - 1.5 * i as f64).collect();
        for span in ALL_SPANS {
            let out = smooth_line(&values, span);
            assert_eq!(out.len(), rows);
            for (i, v) in out.iter().enumerate() {
                let (first, weights) = line_weights(rows, i, span);
                assert!(first + weights.len() <= rows);
                assert_eq!(weights.len(), span.min(rows), "rows {rows} span {span}");
                let expected = fit_by_the_rule(&values, i, span);
                assert!(close(*v, expected, 1e-9), "rows {rows} span {span} row {i}");
            }
            for (a, b) in smooth_line(&line, span).iter().zip(&line) {
                assert!(close(*a, *b, 1e-9), "rows {rows} span {span}");
            }
        }
    }
    // one row and two rows have no line to fit: they stay
    assert_eq!(smooth_line(&[3.0], SPAN_HIGH), [3.0]);
    assert_eq!(smooth_line(&[3.0, -8.0], SPAN_LOW), [3.0, -8.0]);
}

#[test]
#[should_panic(expected = "row 5 of 5")]
fn the_weights_of_a_row_that_is_not_there() {
    let _ = line_weights(5, 5, SPAN_LOW);
}

#[test]
fn a_tone_on_a_bin_reads_its_power_density() {
    // 2 kHz: L = round(0.3·2000) = 600 samples, a row is 2000/600 Hz. A tone
    // of amplitude A on bin 60 (200 Hz): the sum of the window is L/2, so
    // |X| = A·L/4 and 2·|X|²/(L·fs) = A²·L/(8·fs) = A²·0.0375:
    // 20·log10(A) − 14.26 dB, and with half of |X| on either neighbour
    // 6.02 dB less there. The window is symmetric, not periodic, so its sum
    // is (L − 1)/2 and the bin reads 20·log10((L − 1)/L) = −0.014 dB lower:
    // inside the 0.02 dB this is held to (−14.274 and −20.273 are read).
    let (fs, nperseg, bin) = (2000.0, 600, 60);
    assert_eq!(segment_len(fs), nperseg);
    let on_bin = 10.0 * (nperseg as f64 / (8.0 * fs)).log10();
    assert!(close(on_bin, -14.26, 0.005));
    for amplitude in [1.0, 8.0, 40.0] {
        let level = 20.0 * f64::log10(amplitude);
        let x = tone(6000, nperseg, bin, amplitude);
        let raw = map(fs, x.clone(), FreqSmoothing::Off, TimeSmoothing::Low);
        assert_eq!(raw.grid.y.len(), nperseg / 2 + 1, "bins 0 … L/2");
        assert!(close(raw.grid.y[bin], 200.0, 1e-9));
        assert!(close(*raw.grid.y.last().unwrap(), fs / 2.0, 1e-9));
        for column in 0..raw.grid.x.len() {
            assert!(close(cell(&raw, bin, column), level - 14.26, 0.02));
            assert!(close(cell(&raw, bin - 1, column), level - 20.28, 0.03));
            assert!(close(cell(&raw, bin + 1, column), level - 20.28, 0.03));
            // two rows away the window leaves next to nothing
            assert!(cell(&raw, bin + 2, column) < level - 60.0);
            assert!(cell(&raw, bin - 2, column) < level - 60.0);
        }
        // "low": the mean of three rows with the weights 0.2863, 0.4274 and
        // 0.2863, of the decibels: 0.5726 · 6.02 = 3.45 dB lower, −17.71
        // (the reference tool's −17.7)
        let low = map(fs, x.clone(), FreqSmoothing::Low, TimeSmoothing::Low);
        for column in 0..low.grid.x.len() {
            assert!(close(cell(&low, bin, column), level - 17.70, 0.01));
            let by_hand = 0.4274 * cell(&raw, bin, column)
                + 0.2863 * (cell(&raw, bin - 1, column) + cell(&raw, bin + 1, column));
            assert!(close(cell(&low, bin, column), by_hand, 0.01));
        }
        // a wider line takes more of the rows without the tone in
        let medium = map(fs, x.clone(), FreqSmoothing::Medium, TimeSmoothing::Low);
        let high = map(fs, x, FreqSmoothing::High, TimeSmoothing::Low);
        assert!(cell(&medium, bin, 0) < cell(&low, bin, 0) - 20.0);
        assert!(cell(&high, bin, 0) < cell(&medium, bin, 0) - 20.0);
    }
}

#[test]
fn the_bin_of_zero_hertz_counts_once_and_the_last_bin_twice() {
    // an offset c: |X₀| = c·L/2, and the bin of 0 Hz has no factor 2:
    // c²·L/(4·fs) = c²·0.075, 20·log10(c) − 11.25 dB (with the 2 it would
    // read 3.01 dB more). Bin 1 holds half of |X₀| and has the 2: 3.01 dB
    // below bin 0, not 6.02. The mean is not removed.
    let (fs, nperseg, c) = (2000.0, 600, 4.0_f64);
    let level = 20.0 * c.log10();
    let offset = map(fs, vec![c; 3000], FreqSmoothing::Off, TimeSmoothing::Off);
    let at_zero = 10.0 * (nperseg as f64 / (4.0 * fs)).log10();
    assert!(close(at_zero, -11.25, 0.005));
    for column in 0..offset.grid.x.len() {
        assert!(close(cell(&offset, 0, column), level - 11.25, 0.02));
        assert!(close(cell(&offset, 1, column), level - 14.26, 0.03));
    }
    // c·(−1)ⁱ is a tone on the last bin, L/2 (half the sample rate):
    // |X| = c·L/2 again, and this bin does have the 2: 20·log10(c) − 8.24 dB
    let alternating: Vec<f64> = (0..3000).map(|i| if i % 2 == 0 { c } else { -c }).collect();
    let top = map(fs, alternating, FreqSmoothing::Off, TimeSmoothing::Off);
    for column in 0..top.grid.x.len() {
        assert!(close(cell(&top, nperseg / 2, column), level - 8.24, 0.02));
    }
    // nothing at all reads the floor of the logarithm, not −∞
    let silence = map(fs, vec![0.0; 3000], FreqSmoothing::Low, TimeSmoothing::Low);
    assert!(silence.grid.z.iter().all(|v| close(*v, -300.0, 1e-9)));
}

#[test]
fn white_noise_reads_its_density_less_the_window_and_the_mean_of_decibels() {
    // White noise of RMS σ at 2 kHz has the one-sided density σ²/(fs/2).
    // A cell holds 2·|X|²/(L·fs) with E|X|² = σ²·Σw² and Σw² = 3·L/8 for a
    // Hann window, with no compensation for the window: the mean power of a
    // cell is the density times 3/8, 4.26 dB below it. |X|² of a bin that is
    // not 0 Hz or half the rate is exponentially distributed, and the mean
    // of the decibels of such a value is 10·γ/ln(10) = 2.51 dB below the
    // decibels of its mean (γ = 0.5772, Euler's constant): 6.77 dB below
    // the density in all.
    let (fs, sigma, n) = (2000.0, 5.0, 60_000);
    let x: Vec<f64> = white(n).iter().map(|v| v * sigma).collect();
    let out = map(fs, x, FreqSmoothing::Off, TimeSmoothing::Off);
    let (rows, nt) = (out.grid.y.len(), out.grid.x.len());
    assert_eq!((rows, nt), (301, 98));
    let density = 10.0 * (sigma * sigma / (fs / 2.0)).log10();
    assert!(close(density, -16.02, 0.005));
    let window = 10.0 * f64::log10(3.0 / 8.0);
    let log_mean = -10.0 * 0.577_215_664_9 / std::f64::consts::LN_10;
    assert!(close(window, -4.26, 0.005) && close(log_mean, -2.51, 0.005));
    // the rows 1 … L/2 − 1 of every column
    let cells: Vec<f64> = (1..rows - 1)
        .flat_map(|row| (0..nt).map(move |column| (row, column)))
        .map(|(row, column)| cell(&out, row, column))
        .collect();
    let mean = cells.iter().sum::<f64>() / cells.len() as f64;
    // Tolerance: the decibels of an exponential value scatter by
    // (10/ln 10)·π/√6 = 5.57 dB. The segments do not overlap, so the 98
    // columns are independent; the window ties a bin to its neighbours
    // (their powers correlate by 4/9 and by 1/36 two bins away), which makes
    // 299 rows worth about 299 / (1 + 2·(4/9 + 1/36)) = 154. The mean of
    // 98 · 154 values scatters by 5.57/√15092 = 0.045 dB; 0.15 dB is more
    // than three of that.
    assert!(
        close(mean, density + window + log_mean, 0.15),
        "mean {mean}, density {density}"
    );
    // the scatter itself, as a check of the distribution: 5.57 dB
    let var = cells.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / cells.len() as f64;
    assert!(close(var.sqrt(), 5.57, 0.15), "{}", var.sqrt());
    // the smoothing is of the decibels, so it keeps their mean and calms them
    let x: Vec<f64> = white(n).iter().map(|v| v * sigma).collect();
    let low = map(fs, x, FreqSmoothing::Low, TimeSmoothing::Off);
    let smoothed: Vec<f64> = (3..rows - 3)
        .flat_map(|row| (0..nt).map(move |column| (row, column)))
        .map(|(row, column)| cell(&low, row, column))
        .collect();
    let mean_low = smoothed.iter().sum::<f64>() / smoothed.len() as f64;
    assert!(close(mean_low, mean, 0.05), "{mean_low} vs {mean}");
    let var_low =
        smoothed.iter().map(|v| (v - mean_low).powi(2)).sum::<f64>() / smoothed.len() as f64;
    assert!(var_low.sqrt() < 0.85 * var.sqrt());
}

#[test]
fn the_distance_between_segments_by_level_and_rate() {
    // L = round(0.3·fs); off: L, low: L/2, medium: L/5, high: L/10, that is
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
    assert_eq!(TimeSmoothing::Low.hop(303), 152);
    assert_eq!(TimeSmoothing::Medium.hop(303), 61);
    assert_eq!(TimeSmoothing::High.hop(303), 30);
    assert_eq!(TimeSmoothing::High.hop(4), 1);
    assert_eq!(TimeSmoothing::High.hop(2), 1);
    assert_eq!(segment_len(3200.0), 960);
    assert_eq!(segment_len(1.0), 2, "never shorter than two samples");
}

#[test]
fn where_the_columns_lie_and_how_many_there_are() {
    // Column m is centred on sample hop·m and takes [hop·m − L/2, … + L).
    // The first: the lowest m with hop·m ≥ L/2. The last:
    // floor((N − 1 − L) / hop). At 2 kHz, L = 600, L/2 = 300.
    //
    // N = 56 000 (28 s):
    //   off,    hop 600: m = 1 … floor(55399/600) = 92,   92 columns
    //   low,    hop 300: m = 1 … floor(55399/300) = 184, 184 columns
    //   medium, hop 120: m = 3 … floor(55399/120) = 461, 459 columns
    //   high,   hop  60: m = 5 … floor(55399/60)  = 923, 919 columns
    assert_eq!(columns(56_000, 600, 600), Some((1, 92)));
    assert_eq!(columns(56_000, 600, 300), Some((1, 184)));
    assert_eq!(columns(56_000, 600, 120), Some((3, 461)));
    assert_eq!(columns(56_000, 600, 60), Some((5, 923)));
    // N = 29 000 (14.5 s): floor(28399 / hop)
    assert_eq!(columns(29_000, 600, 600), Some((1, 47)));
    assert_eq!(columns(29_000, 600, 300), Some((1, 94)));
    assert_eq!(columns(29_000, 600, 120), Some((3, 236)));
    assert_eq!(columns(29_000, 600, 60), Some((5, 473)));
    // the same stretches at 4 kHz, L = 1200: the same columns
    assert_eq!(columns(112_000, 1200, 600), Some((1, 184)));
    assert_eq!(columns(58_000, 1200, 600), Some((1, 94)));
    // the shortest stretch with a column: hop·first ≤ N − 1 − L
    assert_eq!(columns(901, 600, 300), Some((1, 1)));
    assert_eq!(columns(900, 600, 300), None);
    assert_eq!(columns(1201, 600, 600), Some((1, 1)));
    assert_eq!(columns(1200, 600, 600), None);
    assert_eq!(columns(961, 600, 120), Some((3, 3)));
    assert_eq!(columns(960, 600, 120), None);
    // shorter than a segment, and nothing at all
    assert_eq!(columns(600, 600, 60), None);
    assert_eq!(columns(0, 600, 60), None);
    // an odd segment: L/2 is rounded down, as the first sample of a segment
    assert_eq!(columns(2000, 303, 152), Some((1, 11)));
    assert_eq!(columns(10, 3, 0), Some((1, 6)), "a distance of 0 is 1");

    // the analysis on the 28 s of an epoch: samples 4000 … 59 999 of 30 s
    let s = session(2000.0, tone(60_000, 600, 60, 1.0));
    let epoch = Some(Epoch {
        start_s: 2.0,
        end_s: 30.0,
    });
    for (level, hop, first, count) in [
        (TimeSmoothing::Off, 600, 1, 92),
        (TimeSmoothing::Low, 300, 1, 184),
        (TimeSmoothing::Medium, 120, 3, 459),
        (TimeSmoothing::High, 60, 5, 919),
    ] {
        let out = run_in(&s, epoch, &params(FreqSmoothing::Low, level));
        assert_eq!((out.nperseg, out.hop), (600, hop), "{level:?}");
        assert_eq!(
            (out.smooth_freq, out.smooth_time),
            (FreqSmoothing::Low, level)
        );
        assert_eq!(out.grid.x.len(), count, "{level:?}");
        assert_eq!(out.grid.z.len(), count * 301);
        // the time of a column is the middle of its segment, from the start
        // of the session: the epoch starts at 2 s
        let step = hop as f64 / 2000.0;
        for (i, t) in out.grid.x.iter().enumerate() {
            assert!(
                close(*t, 2.0 + (first + i) as f64 * step, 1e-9),
                "{level:?}"
            );
        }
        // the last segment ends half a segment (0.15 s) or more before the
        // stretch does, and one more column would not
        let end_of_last = *out.grid.x.last().unwrap() + 0.15;
        assert!(end_of_last <= 30.0 - 0.15 + 1e-9, "{level:?}");
        assert!(end_of_last + step > 30.0 - 0.15 - 1.0 / 2000.0, "{level:?}");
    }
    assert!((s.timebase.len() as f64 / 2000.0 - 30.0).abs() < 1e-9);
}

#[test]
fn a_tone_that_starts_shows_in_full_in_the_first_segment_inside_it() {
    // 2 kHz, low: column m is the samples 300·m − 300 … 300·m + 299 and sits
    // at 0.15·m s. Nothing until sample 3000 (1.5 s), then a tone on bin 60.
    //   m = 9  (1.35 s): samples 2400 … 2999, all before the tone
    //   m = 10 (1.50 s): samples 2700 … 3299, half inside: the half of the
    //                    window that holds the tone sums to L/4, 6.02 dB less
    //   m = 11 (1.65 s): samples 3000 … 3599, the first wholly inside
    let (fs, nperseg, bin, start) = (2000.0, 600, 60, 3000);
    let mut x = tone(9000, nperseg, bin, 10.0);
    x[..start].fill(0.0);
    let out = map(fs, x, FreqSmoothing::Off, TimeSmoothing::Low);
    let full = 20.0 - 14.26;
    // the grid starts at m = 1
    let column = |m: usize| m - 1;
    assert!(close(out.grid.x[column(9)], 1.35, 1e-9));
    assert!(close(out.grid.x[column(10)], 1.50, 1e-9));
    assert!(close(out.grid.x[column(11)], 1.65, 1e-9));
    for m in 1..=9 {
        assert!(cell(&out, bin, column(m)) < -250.0, "column {m}");
    }
    assert!(close(cell(&out, bin, column(10)), full - 6.02, 0.05));
    for m in 11..=out.grid.x.len() {
        assert!(close(cell(&out, bin, column(m)), full, 0.02), "column {m}");
    }
    // high puts ten columns on a segment: the first in full is the one
    // centred 0.15 s after the start, m = 55 at 0.03·55 = 1.65 s again
    let mut x = tone(9000, nperseg, bin, 10.0);
    x[..start].fill(0.0);
    let high = map(fs, x, FreqSmoothing::Off, TimeSmoothing::High);
    let column = |m: usize| m - 5;
    assert!(close(high.grid.x[column(55)], 1.65, 1e-9));
    assert!(close(cell(&high, bin, column(55)), full, 0.02));
    // the column before it still has 60 samples of nothing at its start,
    // where the window is small: 0.055 dB less
    assert!(cell(&high, bin, column(54)) < cell(&high, bin, column(55)) - 0.04);
    assert!(
        cell(&high, bin, column(45)) < -250.0,
        "0.3 s before: nothing"
    );
    assert!(cell(&high, bin, column(46)) > -100.0, "the first sample in");
}

#[test]
fn the_frequency_range_is_cut_after_the_smoothing() {
    let (fs, n) = (4000.0, 12_000);
    let x: Vec<f64> = white(n)
        .iter()
        .zip(tone(n, 1200, 150, 6.0))
        .map(|(a, b)| a + b)
        .collect();
    let s = session(fs, x);
    for level in FreqSmoothing::ALL {
        let whole = run_in(&s, None, &params(level, TimeSmoothing::Low));
        // to half the sample rate by default: bins 0 … 600 of 1200
        assert_eq!(whole.grid.y.len(), 601);
        assert!(close(*whole.grid.y.last().unwrap(), 2000.0, 1e-9));
        let cut = run_in(
            &s,
            None,
            &StftParams {
                max_hz: Some(500.0),
                ..params(level, TimeSmoothing::Low)
            },
        );
        // the rows up to 500 Hz, 500 Hz included: bins 0 … 150
        assert_eq!(cut.grid.y.len(), 151);
        assert_eq!(cut.grid.y[..], whole.grid.y[..151]);
        assert_eq!(cut.grid.x, whole.grid.x);
        // and they are the rows of the whole map: the row at the cut was
        // smoothed with the rows above it
        let nt = whole.grid.x.len();
        assert_eq!(cut.grid.z[..], whole.grid.z[..151 * nt], "{level:?}");
    }
    // a bound above half the rate changes nothing, one below the first row
    // keeps that row only
    let above = run_in(
        &s,
        None,
        &StftParams {
            max_hz: Some(1e6),
            ..params(FreqSmoothing::Low, TimeSmoothing::Low)
        },
    );
    assert_eq!(above.grid.y.len(), 601);
    let lowest = run_in(
        &s,
        None,
        &StftParams {
            max_hz: Some(1.0),
            ..params(FreqSmoothing::Low, TimeSmoothing::Low)
        },
    );
    assert_eq!(lowest.grid.y, [0.0]);
}

#[test]
fn what_the_result_says_about_itself() {
    let out = map(
        1000.0,
        tone(2000, 300, 30, 2.0),
        FreqSmoothing::Low,
        TimeSmoothing::Low,
    );
    assert_eq!(out.channel, GYRO);
    assert_eq!(out.unit, "deg/s");
    assert!(close(out.fs, 1000.0, 1e-9));
    assert_eq!((out.nperseg, out.hop), (300, 150));
    assert_eq!(out.grid.x_label, "time (s)");
    assert_eq!(out.grid.y_label, "frequency (Hz)");
    assert_eq!(out.grid.z_label, "power density (dB, (deg/s)²/Hz)");
    // the rows are fs/L apart
    assert!(close(out.grid.y[1] - out.grid.y[0], 1000.0 / 300.0, 1e-9));
    // the JSON of the CLI carries the levels by name
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["smooth_freq"], "low");
    assert_eq!(json["smooth_time"], "low");
    assert_eq!(json["hop"], 150);
}

#[test]
fn a_segment_of_an_odd_number_of_samples() {
    // 1010 Hz: L = round(303) = 303, bins 0 … 151, low: round(151.5) = 152
    // samples apart; column m starts at 152·m − 151
    let fs = 1010.0;
    let out = map(fs, white(3030), FreqSmoothing::Low, TimeSmoothing::Low);
    assert_eq!((out.nperseg, out.hop), (303, 152));
    assert_eq!(out.grid.y.len(), 152);
    let (first, last) = columns(3030, 303, 152).unwrap();
    assert_eq!((first, last), (1, 17));
    assert_eq!(out.grid.x.len(), 17);
    assert!(close(out.grid.x[0], 152.0 / fs, 1e-6));
    assert!(out.grid.z.iter().all(|v| v.is_finite()));
}

#[test]
fn a_stretch_too_short_for_a_segment_is_refused() {
    // 2 kHz, low: the shortest stretch with a column is 901 samples
    let fits = session(2000.0, white(901));
    let short = session(2000.0, white(900));
    let p = params(FreqSmoothing::Low, TimeSmoothing::Low);
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
            assert_eq!(why, "900 samples, the first segment of 600 needs 901");
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
