//! The pack of a round: reproducible, readable, and true to its manifest.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use openpid_core::{Axis, BatteryField, ChannelId, GyroStage, LogFormat, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;
use openpid_probe::pack::{self, ChannelTruth, PackError, Spectrum};
use openpid_probe::{Pack, catalogue};
use std::sync::OnceLock;

/// Round 1, built once for every test.
fn round_1() -> &'static Pack {
    static PACK: OnceLock<Pack> = OnceLock::new();
    PACK.get_or_init(|| pack::build(1, "test").unwrap())
}

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("openpid-probe-{}-{name}", std::process::id()))
}

#[test]
fn the_same_round_is_the_same_bytes() {
    let again = pack::build(1, "test").unwrap();
    let (a, b) = (round_1().files(), again.files());
    assert_eq!(a.len(), b.len());
    for ((name_a, bytes_a), (name_b, bytes_b)) in a.iter().zip(&b) {
        assert_eq!(name_a, name_b);
        assert!(bytes_a == bytes_b, "{name_a} differs between two builds");
    }
}

#[test]
fn every_log_reads_back_as_the_rounded_flight() {
    let pack = round_1();
    let probes = catalogue::round(1).unwrap();
    assert_eq!(pack.logs.len(), probes.len());
    for (probe, (name, bytes)) in probes.iter().zip(&pack.logs) {
        assert_eq!(name, &format!("{}.bbl", probe.id));
        let log = BlackboxFormat::new()
            .open(bytes, &OpenOptions::default())
            .unwrap();
        let session = &log.sessions[0];
        assert_eq!(session.len(), probe.flight.len(), "{name}");
        assert_eq!(session.diagnostics.corrupt_frames, 0, "{name}");
        assert_eq!(session.settings.craft_name.as_deref(), Some(probe.id));
        assert_eq!(session.settings.loop_rates.log_hz, Some(probe.flight.fs));
        // armed half a minute after power-up
        assert_eq!(session.timebase.time_us[0], 30_000_000, "{name}");
        for axis in Axis::ALL {
            let exact = probe.flight.samples(axis);
            for (stage, want) in [
                (GyroStage::Raw, &exact.unfiltered),
                (GyroStage::Filtered, &exact.filtered),
            ] {
                let id = ChannelId::Gyro { axis, stage };
                let got = session.channel(&id).unwrap().data.values();
                let equal = got.iter().zip(want).all(|(g, w)| *g == w.round_ties_even());
                assert!(equal, "{name} {id:?}: not the rounded samples");
            }
            let setpoint = session.channel(&ChannelId::Setpoint { axis }).unwrap();
            assert!(setpoint.data.values().iter().all(|v| *v == 0.0), "{name}");
        }
        let throttle = session.channel(&ChannelId::Throttle).unwrap();
        assert!(throttle.data.values().iter().all(|v| *v == 50.0), "{name}");
    }
}

fn channels(pack: &Pack) -> impl Iterator<Item = (String, f64, &ChannelTruth)> {
    pack.manifest.probes.iter().flat_map(|p| {
        p.axes.iter().flat_map(move |a| {
            [("unfiltered", &a.unfiltered), ("filtered", &a.filtered)]
                .map(|(name, c)| (format!("{} {} {name}", p.id, a.axis), p.fs, c))
        })
    })
}

#[test]
fn the_manifest_is_true_of_the_logs() {
    let pack = round_1();
    let m = &pack.manifest;
    assert_eq!((m.format, m.round, m.built_from.as_str()), (1, 1, "test"));
    let mut tones = 0;
    for (name, fs, truth) in channels(pack) {
        // a log keeps whole deg/s
        assert!(truth.rounding <= 0.5, "{name}: {}", truth.rounding);
        assert!(truth.rounding > 0.4, "{name}: nothing was rounded?");
        for tone in &truth.tones {
            tones += 1;
            // measured in the written log, a second way: the projection
            // on the tone's frequency
            // (the noise of the channel limits how well: its RMS · sqrt(2 / N))
            assert!(
                (tone.logged_amplitude - tone.amplitude).abs() < 0.06 + 0.03 * truth.noise_rms,
                "{name}: {} Hz asked {} logged {}",
                tone.hz,
                tone.amplitude,
                tone.logged_amplitude
            );
            assert!((tone.amplitude_db - 20.0 * tone.amplitude.log10()).abs() < 1e-6);
            assert!((tone.power_db - (tone.amplitude_db - 3.010_300)).abs() < 1e-5);
        }
        // the density of white noise: its power over 0 … fs/2
        assert!(
            (truth.noise_density - truth.noise_rms.powi(2) / (fs / 2.0)).abs() < 1e-6,
            "{name}"
        );
        // noise alone: the logged RMS is the noise plus the rounding (1/12)
        for band in &truth.bands {
            // measured in the written log, a second way: the power of its
            // spectrum between the edges
            assert!(
                (band.logged_rms / band.rms - 1.0).abs() < 0.01,
                "{name}: band {} … {} Hz asked {} logged {}",
                band.from_hz,
                band.to_hz,
                band.rms,
                band.logged_rms
            );
            let density = band.rms.powi(2) / (band.to_hz - band.from_hz);
            assert!((band.density - density).abs() < 1e-5, "{name}");
            assert!(
                (band.density_db - 10.0 * density.log10()).abs() < 1e-5,
                "{name}"
            );
        }
        if truth.tones.is_empty() && truth.bands.is_empty() && truth.logged_mean.abs() < 1.0 {
            let want = (truth.noise_rms.powi(2) + 1.0 / 12.0).sqrt();
            assert!(
                (truth.logged_rms / want - 1.0).abs() < 0.01,
                "{name}: logged {} for {want}",
                truth.logged_rms
            );
        }
    }
    assert!(tones >= 40, "only {tones} tones checked");

    let probe = |id: &str| m.probes.iter().find(|p| p.id == id).unwrap();
    // the offset and the drift are in the logged samples
    let low = probe("s7-low-2k");
    assert!((low.axes[0].unfiltered.logged_mean - 30.0).abs() < 0.01);
    // -50 … 50 deg/s of drift under a tone of 10 deg/s
    assert_eq!(low.axes[1].unfiltered.logged_range, (-61.0, 61.0));
    // the two channels of the last probe carry different tones
    let channels = probe("s8-channels-2k");
    assert_eq!(channels.axes[0].unfiltered.tones[0].hz, 100.0);
    assert_eq!(channels.axes[0].filtered.tones[0].hz, 248.0);
    assert_eq!(channels.axes[2].filtered.tones[0].amplitude, 10.0);
    // the bands of the last probe: one density, three widths
    let bands = &probe("s9-bands-2k").axes[1].unfiltered.bands;
    assert_eq!(bands.len(), 3);
    assert!(
        bands
            .iter()
            .all(|b| b.density == 1.0 && b.density_db == 0.0)
    );
    assert_eq!(
        bands
            .iter()
            .map(|b| b.to_hz - b.from_hz)
            .collect::<Vec<_>>(),
        [4.0, 20.0, 80.0]
    );
    // a burst is described by its span
    let burst = &probe("s6-averaging-2k").axes[2].unfiltered.tones[0];
    assert_eq!(burst.span_s, (10.0, 11.0));
    // the file is identified
    let first = &m.probes[0];
    assert_eq!(first.bytes, pack.logs[0].1.len());
    assert_eq!(first.fnv1a64.len(), 16);
    assert_eq!(
        (first.samples, first.fs, first.duration_s),
        (60_000, 2000.0, 30.0)
    );
    assert_eq!(first.flight, catalogue::round(1).unwrap()[0].flight);
}

fn spectrum<'a>(
    pack: &'a Pack,
    id: &str,
    axis: usize,
    channel: &str,
    segment_s: f64,
) -> &'a Spectrum {
    let probe = pack.ours.probes.iter().find(|p| p.id == id).unwrap();
    probe.axes[axis]
        .spectra
        .iter()
        .find(|s| s.channel == channel && s.segment_s == segment_s)
        .unwrap()
}

fn level_at(s: &Spectrum, hz: f64) -> f64 {
    let i = s.freq.iter().position(|f| (*f - hz).abs() < 1e-9).unwrap();
    s.power_db[i]
}

fn median(s: &Spectrum) -> f64 {
    let mut v = s.power_db.clone();
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// What `OpenPID` computes for the probes against what is true of them:
/// the first calibration of our own spectrum.
#[test]
fn our_spectrum_is_a_power_density() {
    let pack = round_1();
    assert_eq!(pack.ours.probes[0].epoch_s, (1.0, 29.0));
    // a tone of amplitude A through a Hann window reads A² / 2 over the
    // noise bandwidth of the window, 1.5 bins
    let tone =
        |amplitude: f64, bin_hz: f64| 10.0 * (amplitude * amplitude / 2.0 / (1.5 * bin_hz)).log10();
    for (id, fs) in [
        ("s1-levels-2k", 2000.0f64),
        ("s2-levels-1k", 1000.0),
        ("s3-levels-4k", 4000.0),
    ] {
        let roll = spectrum(pack, id, 0, "gyro", 0.25);
        assert_eq!((roll.bin_hz, roll.segments), (4.0, 223), "{id}");
        assert_eq!(roll.freq.len(), (fs / 8.0) as usize + 1, "{id}");
        assert!(
            (level_at(roll, 100.0) - tone(3.0, 4.0)).abs() < 0.03,
            "{id}"
        );
        // ten times the amplitude is 20 dB
        let pitch = spectrum(pack, id, 1, "gyro", 0.25);
        assert!(
            (level_at(pitch, 100.0) - level_at(roll, 100.0) - 20.0).abs() < 0.03,
            "{id}"
        );
        // the level does not depend on the frequency or on the log rate
        let yaw = spectrum(pack, id, 2, "gyro", 0.25);
        for hz in [48.0, 200.0, 400.0] {
            assert!(
                (level_at(yaw, hz) - tone(10.0, 4.0)).abs() < 0.03,
                "{id} {hz}"
            );
        }
        // one-second segments: bins of 1 Hz up to 100 Hz, the tone 6 dB higher
        let low = spectrum(pack, id, 0, "gyro", 1.0);
        assert_eq!((low.bin_hz, low.freq.len()), (1.0, 101), "{id}");
        assert!((level_at(low, 100.0) - tone(3.0, 1.0)).abs() < 0.03, "{id}");
        // the floor is the dither plus the rounding of the log, as a density
        let floor = 10.0 * ((0.25 + 1.0 / 12.0) / (fs / 2.0)).log10();
        assert!(
            (median(roll) - floor).abs() < 0.3,
            "{id}: {} vs {floor}",
            median(roll)
        );
    }
    // white noise of 10 and of 3 deg/s RMS over 0 … 1000 Hz
    for (axis, rms) in [(0, 10.0f64), (2, 3.0)] {
        let s = spectrum(pack, "s5-noise-2k", axis, "gyro", 0.25);
        let density = 10.0 * ((rms * rms + 1.0 / 12.0) / 1000.0).log10();
        assert!(
            (median(s) - density).abs() < 0.15,
            "{rms}: {} vs {density}",
            median(s)
        );
    }
    // a band of density 1 is a plateau at 0 dB, whatever its width; one of
    // density 0.1 at -10 dB; between two bands only the floor is left
    let plateau = spectrum(pack, "s9-bands-2k", 0, "gyro", 0.25);
    for hz in [120.0, 148.0, 180.0] {
        let level = level_at(plateau, hz);
        assert!(level.abs() < 0.4, "{hz}: {level}");
    }
    assert!(level_at(plateau, 300.0) < -30.0);
    let widths = spectrum(pack, "s9-bands-2k", 1, "gyro", 0.25);
    for hz in [208.0, 212.0, 420.0, 440.0, 460.0] {
        let level = level_at(widths, hz);
        assert!(level.abs() < 0.4, "{hz}: {level}");
    }
    // 4 Hz of band are one bin wide: it reads 3 dB low
    let narrow = level_at(widths, 100.0);
    assert!((narrow + 3.0).abs() < 0.6, "{narrow}");
    let gap = spectrum(pack, "s9-bands-2k", 2, "gyro", 0.25);
    for hz in [60.0, 100.0, 300.0, 600.0] {
        let level = level_at(gap, hz);
        assert!((level + 10.0).abs() < 0.4, "{hz}: {level}");
    }
    assert!(level_at(gap, 176.0) < -30.0);
    // a tone in half of the analysed span is 3 dB down: powers are averaged
    let whole = level_at(spectrum(pack, "s6-averaging-2k", 1, "gyro", 0.25), 100.0);
    let half = level_at(spectrum(pack, "s6-averaging-2k", 0, "gyro", 0.25), 100.0);
    assert!((whole - half - 3.01).abs() < 0.1, "{}", whole - half);
    let second = level_at(spectrum(pack, "s6-averaging-2k", 2, "gyro", 0.25), 100.0);
    assert!((whole - second - 14.47).abs() < 0.3, "{}", whole - second);
    // the mean is removed: no line at 0 Hz from a constant 30 deg/s
    let offset = spectrum(pack, "s7-low-2k", 0, "gyro", 1.0);
    assert!(level_at(offset, 0.0) < level_at(offset, 20.0) - 30.0);
    // 60 and 61 Hz are two peaks with one-second segments
    let pair = spectrum(pack, "s7-low-2k", 2, "gyro", 1.0);
    assert!((level_at(pair, 60.0) - level_at(pair, 61.0)).abs() < 0.2);
    assert!((level_at(pair, 5.0) - tone(10.0, 1.0)).abs() < 0.05);
    // 100 and 104 Hz are neighbouring 4 Hz bins: no dip between them
    let close = spectrum(pack, "s4-resolution-2k", 0, "gyro", 0.25);
    assert!((level_at(close, 100.0) - level_at(close, 104.0)).abs() < 0.5);
    // off the grid a tone reads lower: 302.3 Hz between the bins 300 and 304
    let grid = spectrum(pack, "s4-resolution-2k", 2, "gyro", 0.25);
    let on = level_at(grid, 100.0);
    let off = level_at(grid, 304.0).max(level_at(grid, 300.0));
    assert!(on - off > 1.0 && on - off < 1.6, "{}", on - off);
    // the two channels: 100 Hz before the filters, 248 Hz after
    let before = spectrum(pack, "s8-channels-2k", 0, "gyro_unfiltered", 0.25);
    let after = spectrum(pack, "s8-channels-2k", 0, "gyro", 0.25);
    assert!(level_at(before, 100.0) > level_at(before, 248.0) + 40.0);
    assert!(level_at(after, 248.0) > level_at(after, 100.0) + 40.0);
    let (b, a) = (
        spectrum(pack, "s8-channels-2k", 2, "gyro_unfiltered", 0.25),
        spectrum(pack, "s8-channels-2k", 2, "gyro", 0.25),
    );
    assert!((level_at(b, 100.0) - level_at(a, 100.0) - 6.02).abs() < 0.02);
}

#[test]
fn a_pack_is_written_as_files() {
    let dir = temp("pack");
    round_1().write_to(&dir).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "README.txt",
            "capture.ps1",
            "manifest.json",
            "ours.json",
            "run.cmd",
            "s1-levels-2k.bbl",
            "s2-levels-1k.bbl",
            "s3-levels-4k.bbl",
            "s4-resolution-2k.bbl",
            "s5-noise-2k.bbl",
            "s6-averaging-2k.bbl",
            "s7-low-2k.bbl",
            "s8-channels-2k.bbl",
            "s9-bands-2k.bbl",
            "steps.json",
        ]
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["probes"][3]["id"], "s4-resolution-2k");
    assert_eq!(manifest["probes"][3]["axes"][1]["axis"], "pitch");
    assert_eq!(
        manifest["probes"][0]["flight"]["roll"]["waves"][0]["kind"],
        "tone"
    );
    let ours: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("ours.json")).unwrap()).unwrap();
    assert_eq!(
        ours["probes"][0]["axes"][0]["spectra"][0]["channel"],
        "gyro"
    );
    let steps: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("steps.json")).unwrap()).unwrap();
    assert_eq!(steps["steps"][1]["paths"][0], "s1-levels-2k.bbl");
    let note = std::fs::read_to_string(dir.join("README.txt")).unwrap();
    assert!(note.contains("saves 47 pictures"), "{note}");
    assert_eq!(
        std::fs::read(dir.join("s1-levels-2k.bbl")).unwrap(),
        round_1().logs[0].1
    );
    std::fs::remove_dir_all(&dir).unwrap();

    // a path that cannot be a directory
    let file = temp("file");
    std::fs::write(&file, b"x").unwrap();
    let error = round_1().write_to(&file.join("sub")).unwrap_err();
    assert!(matches!(error, PackError::Io { .. }), "{error}");
    assert!(error.to_string().starts_with("cannot write "));
    std::fs::remove_file(&file).unwrap();
}

#[test]
fn an_unknown_round_is_refused() {
    let error = pack::build(7, "x").unwrap_err();
    assert!(matches!(error, PackError::UnknownRound(7)));
    assert_eq!(
        error.to_string(),
        format!(
            "there is no round 7; the rounds are {:?}",
            catalogue::ROUNDS
        )
    );
    assert_eq!(catalogue::ROUNDS, [1, 2, 3, 4, 5]);
}

#[test]
fn the_command_line() {
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let mut out = Vec::new();
        let result = openpid_probe::cli::run(&args, &mut out);
        (result, String::from_utf8(out).unwrap())
    };
    let (result, out) = run(&["list"]);
    assert!(result.is_ok());
    assert!(out.starts_with("round 1: 9 logs\n  s1-levels-2k.bbl  2000 Hz, 30 s  Tones of known"));
    assert!(out.contains("      pitch the same tone ten times larger"));

    let dir = temp("cli");
    let (result, out) = run(&[
        "pack",
        "1",
        "--out",
        dir.to_str().unwrap(),
        "--built-from",
        "abc",
    ]);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out.lines().count(), 15);
    assert!(out.contains("manifest.json"));
    let manifest = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"built_from\": \"abc\""));
    std::fs::remove_dir_all(&dir).unwrap();

    let (result, out) = run(&["--help"]);
    assert!(result.is_ok() && out.contains("USAGE:"));
    for (args, message) in [
        (&[][..], "USAGE:"),
        (&["fly"][..], "unknown command fly"),
        (&["pack"][..], "pack needs a round"),
        (&["pack", "1"][..], "pack needs --out <dir>"),
        (
            &["pack", "one", "--out", "x"][..],
            "the round must be a number, not one",
        ),
        (&["pack", "9", "--out", "x"][..], "there is no round 9"),
        (&["pack", "1", "--out"][..], "--out needs a directory"),
        (
            &["pack", "1", "--built-from"][..],
            "--built-from needs a commit",
        ),
        (&["pack", "1", "--fast"][..], "unknown option --fast"),
    ] {
        let (result, _) = run(args);
        let error = result.unwrap_err();
        assert!(error.contains(message), "{args:?}: {error}");
    }
}

/// Round 2, built once for every test.
fn round_2() -> &'static Pack {
    static PACK: OnceLock<Pack> = OnceLock::new();
    PACK.get_or_init(|| pack::build(2, "test").unwrap())
}

#[test]
fn round_2_carries_the_model_s_predictions() {
    let pack = round_2();
    let names: Vec<String> = pack.files().into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        [
            "c1-mixed-2k.bbl",
            "c2-mixed-1k.bbl",
            "c3-mixed-4k.bbl",
            "c4-short-2k.bbl",
            "c5-long-2k.bbl",
            "c6-short-8k.bbl",
            "manifest.json",
            "ours.json",
            "predictions.json",
            "steps.json",
            "capture.ps1",
            "run.cmd",
            "README.txt",
        ]
    );
    // round 1 was run without predictions and stays as it was
    assert!(round_1().predictions.is_none());
    assert!(
        round_1()
            .files()
            .iter()
            .all(|(n, _)| n != "predictions.json")
    );

    let predictions = pack.predictions.as_ref().unwrap();
    assert_eq!(predictions.round, 2);
    assert_eq!(predictions.assumes.len(), 3);
    let spans: Vec<(&str, usize, usize)> = predictions
        .probes
        .iter()
        .map(|p| (p.id, p.first_sample, p.samples))
        .collect();
    assert_eq!(
        spans,
        [
            ("c1-mixed-2k", 2002, 55_999),
            ("c2-mixed-1k", 1002, 27_999),
            ("c3-mixed-4k", 4002, 111_999),
            ("c4-short-2k", 2002, 19_999),
            ("c5-long-2k", 2002, 89_999),
            ("c6-short-8k", 8002, 79_999),
        ]
    );
    let c1 = &predictions.probes[0];
    let windows: Vec<usize> = c1.levels.iter().map(|l| l.window).collect();
    assert_eq!(windows, [5, 35, 121, 285, 555]);
    let short: Vec<usize> = predictions.probes[3]
        .levels
        .iter()
        .map(|l| l.window)
        .collect();
    assert_eq!(short, [5, 33, 109, 257, 501]);
    let medium = &c1.levels[2];
    assert_eq!(medium.smoothing, "medium");
    let level = |axis: usize, hz: f64| {
        medium.axes[axis]
            .points
            .iter()
            .find(|p| (p.hz - hz).abs() < 0.02)
            .unwrap()
            .db
    };
    // yaw: white noise of 4 deg/s RMS has a density of 16 / 1000 (deg/s)²/Hz
    // (the rounding of the log adds 1/12 to the 16) and reads 6.8 dB lower
    let density = 10.0 * ((16.0 + 1.0 / 12.0) / 1000.0f64).log10();
    for hz in [200.0, 400.0] {
        let got = level(2, hz);
        assert!((got - (density - 6.77)).abs() < 0.6, "{hz} Hz: {got}");
    }
    // ... and the offset of 20 deg/s lifts the curve at 0 Hz
    let rise = level(2, 0.0) - level(2, 200.0);
    assert!(rise > 1.0 && rise < 12.0, "{rise}");
    // pitch: the band of density 0.3 stands some 30 dB over the floor
    assert!(level(1, 40.0) > level(1, 200.0) + 25.0);
    // roll: the tone of 32 deg/s is a bump of a few decibels at "medium"
    let bump = level(0, 110.0) - level(0, 200.0);
    assert!(bump > 3.0 && bump < 9.0, "{bump}");
    // every frequency of the list is below half of the lowest log rate
    for probe in &predictions.probes {
        for level in &probe.levels {
            for axis in &level.axes {
                assert!(axis.points.len() >= 17, "{} {}", probe.id, level.smoothing);
                assert!(axis.points.iter().all(|p| p.db.is_finite()));
            }
        }
    }
}

#[test]
fn round_5_carries_the_step_model_s_predictions() {
    let pack = pack::build(5, "test").unwrap();
    let names: Vec<String> = pack.files().into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        [
            "u1-plants-2k.bbl",
            "u2-rates-1k.bbl",
            "u3-rates-4k.bbl",
            "u4-slow-2k.bbl",
            "u5-stick-2k.bbl",
            "u6-short-2k.bbl",
            "u7-mid-2k.bbl",
            "manifest.json",
            "ours.json",
            "predictions.json",
            "steps.json",
            "capture.ps1",
            "run.cmd",
            "README.txt",
        ]
    );
    let predictions = pack.predictions.as_ref().unwrap();
    assert_eq!((predictions.round, predictions.assumes.len()), (5, 6));
    // a step-response round predicts that window and nothing of the spectrum
    assert!(predictions.probes.is_empty());
    let text = String::from_utf8(pack.files()[9].1.clone()).unwrap();
    assert!(text.contains("\"step\"") && !text.contains("\"probes\""));
    let grids: Vec<(&str, usize, usize)> = predictions
        .step
        .iter()
        .map(|p| (p.id, p.first_sample, p.windows_in_log))
        .collect();
    assert_eq!(
        grids,
        [
            ("u1-plants-2k", 2002, 219),
            ("u2-rates-1k", 1002, 259),
            ("u3-rates-4k", 4002, 259),
            ("u4-slow-2k", 2002, 259),
            ("u5-stick-2k", 2002, 259),
            ("u6-short-2k", 2002, 79),
            ("u7-mid-2k", 2002, 139),
        ]
    );
    let probe = |id: &str| predictions.step.iter().find(|p| p.id == id).unwrap();
    // the smoothing levels where the script captures them, "off" elsewhere
    for p in &predictions.step {
        let levels = if p.id.contains("rates") { 4 } else { 1 };
        assert_eq!(p.axes.len(), 3);
        assert!(p.axes.iter().all(|a| a.levels.len() == levels), "{}", p.id);
        assert!(p.axes.iter().all(|a| a.levels[0].smoothing == "off"));
    }

    // the three plants: the truth to a few thousandths
    let plants = probe("u1-plants-2k");
    let off = |axis: usize| &plants.axes[axis].levels[0];
    assert!((off(0).peak.unwrap() - 1.0).abs() < 0.005);
    assert_eq!(off(0).latency_ms, Some(9.5));
    assert_eq!(plants.axes[0].sp_gyro_ms, Some(10.0));
    assert!((off(2).peak.unwrap() - 1.2538).abs() < 0.005);
    assert_eq!(off(2).curve.len(), pack::PREDICTED_MS.len());
    assert!(off(2).curve[0].value.abs() < 0.01 && off(2).curve[0].ms == 0.0);

    // the stick always 19, 21 and 20 deg/s from its centre: none, all, all
    let stick = probe("u5-stick-2k");
    let windows: Vec<usize> = stick.axes.iter().map(|a| a.levels[0].windows).collect();
    assert_eq!(windows[0], 0);
    assert!(windows[1] > 250 && windows[2] > 250, "{windows:?}");
    let none = &stick.axes[0].levels[0];
    assert!(none.peak.is_none() && none.curve.is_empty() && none.peak_within.is_empty());

    // the creeping plant has no peak before its end: where the tool stops
    // looking shows in the value
    let slow = &probe("u4-slow-2k").axes[0].levels[0];
    let within: Vec<f64> = slow.peak_within.iter().map(|p| p.value).collect();
    assert!(within.windows(2).all(|w| w[0] + 0.01 < w[1]), "{within:?}");
    assert!(within[3] + 0.005 < slow.peak.unwrap());
    assert!((within[0] - 0.78).abs() < 0.02, "{within:?}");

    // a limit in samples would take other windows than one in milliseconds:
    // at 4 kHz the creeping plant is below 0.5 at sample 300 (75 ms) ...
    let creeping_4k = &probe("u3-rates-4k").axes[1].levels[0];
    assert!(creeping_4k.windows > 230 && creeping_4k.windows_if_from_sample_300 < 200);
    // ... and at 1 kHz the noisy axis has 150 ms more to leave the limits in
    let noisy_1k = &probe("u2-rates-1k").axes[2].levels[0];
    assert!(noisy_1k.windows + 10 < noisy_1k.windows_if_from_sample_300);
    // at 2 kHz both readings are the same
    for id in ["u1-plants-2k", "u4-slow-2k", "u6-short-2k"] {
        for a in &probe(id).axes {
            assert_eq!(a.levels[0].windows, a.levels[0].windows_if_from_sample_300);
        }
    }
    // smoothing brings windows of the noisy axis back
    let noisy: Vec<usize> = probe("u2-rates-1k").axes[2]
        .levels
        .iter()
        .map(|l| l.windows)
        .collect();
    assert!(
        noisy.windows(2).all(|w| w[0] <= w[1]) && noisy[0] + 30 < noisy[3],
        "{noisy:?}"
    );
}

#[test]
fn a_probe_log_has_the_columns_the_reference_tool_has_read() {
    // every log the reference tool has opened from this crate (rounds 0 to
    // 5) was a Betaflight 2025.12 log with exactly these fields, the last
    // nine of them a craft at rest on its sensors. The writer logs what a
    // session has, so the probe flights carry those sensors; a round built
    // tomorrow must not surprise the tool with other columns.
    let (name, bytes) = &round_1().logs[0];
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(40_000)]).into_owned();
    let line = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .unwrap_or_else(|| panic!("{name}: no {key}"))
            .to_string()
    };
    assert_eq!(
        line("H Firmware revision:"),
        "Betaflight 2025.12.2 (79065c96b) STM32F7X2"
    );
    assert_eq!(
        line("H Field I name:"),
        "loopIteration,time,axisP[0],axisP[1],axisP[2],axisI[0],axisI[1],axisI[2],axisD[0],\
         axisD[1],axisF[0],axisF[1],axisF[2],rcCommand[0],rcCommand[1],rcCommand[2],\
         rcCommand[3],setpoint[0],setpoint[1],setpoint[2],setpoint[3],vbatLatest,\
         amperageLatest,rssi,gyroADC[0],gyroADC[1],gyroADC[2],gyroUnfilt[0],gyroUnfilt[1],\
         gyroUnfilt[2],accSmooth[0],accSmooth[1],accSmooth[2],imuQuaternion[0],\
         imuQuaternion[1],imuQuaternion[2],motor[0],motor[1],motor[2],motor[3]"
    );
    assert_eq!(
        line("H Field S name:"),
        "flightModeFlags,stateFlags,failsafePhase,rxSignalReceived,rxFlightChannelsValid"
    );
    // ... and the values of the resting sensors: 16 V, no current, a strong
    // link, one g straight down, no rotation
    let log = BlackboxFormat::new()
        .open(bytes, &OpenOptions::default())
        .unwrap();
    let session = &log.sessions[0];
    let first = |id: ChannelId| session.channel(&id).unwrap().data.values()[100];
    assert_eq!(first(ChannelId::Battery(BatteryField::Voltage)), 16.0);
    assert_eq!(first(ChannelId::Battery(BatteryField::Current)), 0.0);
    assert_eq!(first(ChannelId::Rssi), 1000.0);
    assert_eq!(first(ChannelId::Acc { axis: Axis::Yaw }), 1.0);
    assert_eq!(first(ChannelId::Acc { axis: Axis::Roll }), 0.0);
    assert_eq!(first(ChannelId::Custom("imuQuaternion[2]".into())), 0.0);
}
