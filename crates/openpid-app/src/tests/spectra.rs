//! The spectral tabs: Spectrum, Freq × throttle and Freq × time with its
//! player; their colour scales and overlays.

use super::{LOG_4_2, LOG_4_5_1, LOG_2025, jobs, result, set_epoch, shown, temp_dir};
use crate::app::Tab;
use crate::spectrum_tab::Quantity;
use crate::testkit::{Ui, at_every_size};
use crate::time_tab::ColourScale;
use crate::worker::{Output, Slot};
use eframe::egui::Key;
use openpid_analysis::{FreqSmoothing, TimeSmoothing};
use openpid_core::{Axis, ChannelId, GyroStage};
use std::path::Path;

#[test]
fn spectrum_controls_change_what_is_drawn() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Spectrum);
    // docs/design/spectrum-tab (V1): roll, pitch and yaw at once, each with the
    // full range and below 100 Hz, gyro solid and its prefilt dotted
    assert_eq!(jobs(&ui, "psd"), 12, "3 axes × gyro/prefilt × full/low");
    assert!(ui.sees("A Gyro"));
    assert!(ui.sees("A prefilt"));
    assert_eq!(ui.app().tools.spectrum_display.smoothing, 3, "medium");
    assert!(ui.app().tools.psd_db);
    ui.check("dB", false);
    assert!(!ui.app().tools.psd_db);
    ui.check("dB", true);
    // prefilt off: half the jobs are drawn
    ui.check("Prefilt (dotted)", false);
    assert!(!ui.app().tools.spectrum_display.prefilt);
    assert!(!ui.sees("A prefilt"));
    // another quantity: D-term, no prefilt counterpart
    ui.app_mut().tools.spectrum_channel = Quantity::DTerm.channel(Axis::Roll);
    ui.app_mut().tools.spectrum_display.prefilt = true;
    ui.settle();
    assert!(ui.sees("A D-term"));
    assert!(!ui.sees("A prefilt"));
    ui.app_mut().tools.spectrum_channel = Quantity::Gyro.channel(Axis::Roll);
    // log frequency and fixed Y ends
    ui.app_mut().tools.spectrum_display.log_freq = true;
    ui.app_mut().tools.spectrum_display.y_min = Some(-50.0);
    ui.settle();
    let (x0, x1) = ui.plot("spectrum/roll").x_range();
    assert!(x0 >= 1.0 && x1 > 400.0, "log axis in Hz: {x0}..{x1}");
    ui.click("auto");
    assert_eq!(ui.app().tools.spectrum_display.y_min, None);
    ui.app_mut().tools.spectrum_display.log_freq = false;
    // automatic peaks: with a 3 dB prominence floor the gyro spectrum of a real log has some
    ui.app_mut().spectrum.min_prominence_db = 3.0;
    ui.settle();
    let peaks = ui.app().spectrum.peaks.clone();
    assert!(
        peaks.iter().any(|(s, _, p)| *s == Slot::A && !p.is_empty()),
        "{peaks:?}"
    );
    assert!(ui.sees("A top peak"));
    ui.app_mut().spectrum.auto_peaks = 0;
    ui.settle();
    assert!(ui.app().spectrum.peaks.iter().all(|(_, _, p)| p.is_empty()));
}

#[test]
fn spectrum_markers_are_placed_by_clicks() {
    at_every_size(|screen| {
        let mut ui = Ui::open_on(&[LOG_4_2], Tab::Spectrum, screen);
        // click the plot twice, read the delta, clear
        for f in [100.0, 150.0] {
            ui.plot("spectrum/roll").click(f);
        }
        let m = ui.app().spectrum.markers.clone();
        // a marker snaps to the spectrum's nearest bin
        let bin = 1.0 / ui.app().tools.psd.segment_s;
        let tolerance = ui.plot("spectrum/roll").x_tolerance() + bin;
        assert_eq!(m.len(), 2, "{m:?}");
        assert!(
            (m[0] - 100.0).abs() <= tolerance && (m[1] - 150.0).abs() <= tolerance,
            "{m:?} within {tolerance}"
        );
        assert!(ui.sees("Δ "));
        // the same marker on a log axis is at the same frequency
        ui.app_mut().tools.spectrum_display.log_freq = true;
        ui.settle();
        ui.plot("spectrum/pitch").click(300.0);
        let third = ui.app().spectrum.markers[2];
        assert!((third / 300.0 - 1.0).abs() < 0.03, "{third}");
        ui.click("Clear markers");
        assert!(ui.app().spectrum.markers.is_empty());
    });
}

#[test]
fn throttle_heatmap_reuses_its_textures_and_names_thin_bins() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::ThrottleHeatmap);
    assert!(ui.sees("Throttle window"));
    // every grid (3 axes × gyro and prefilt) is rasterised once and reused
    // frame after frame
    // (scales settle while the six jobs come in, so earlier textures remain)
    let settled = ui.app().heatmap_textures.len();
    assert!(settled >= 6, "{settled}");
    ui.settle();
    assert_eq!(ui.app().heatmap_textures.len(), settled, "no re-upload");
    ui.app_mut().tools.heatmap_max = 5.0;
    ui.settle();
    assert_eq!(
        ui.app().heatmap_textures.len(),
        settled + 6,
        "a new colour scale is a new texture per grid"
    );
    // segments per bin: the hint names the bins below the minimum
    let counts = result(&ui, |o| match o {
        Output::Throttle(t) => Some(t.counts.clone()),
        _ => None,
    })
    .expect("throttle heatmap computed");
    assert!(counts.iter().sum::<usize>() > 0);
    let min = ui.app().tools.min_segments;
    let below = counts.iter().filter(|&&c| c < min).count();
    assert!(ui.sees("Min segments"));
    if below > 0 {
        assert!(ui.sees(&format!("{below} of {} throttle bins", counts.len())));
    } else {
        assert!(ui.sees("every throttle bin"));
    }
    // a minimum above every count flags every bin; the analysis is not rerun
    let cached = ui.app().cache.len();
    ui.app_mut().tools.min_segments = counts.iter().max().copied().unwrap_or(0) + 1;
    ui.settle();
    assert!(ui.sees(&format!(
        "{} of {} throttle bins",
        counts.len(),
        counts.len()
    )));
    assert_eq!(ui.app().cache.len(), cached);
    ui.tab(Tab::TimeHeatmap);
    assert!(ui.sees("Smooth frequency"));
}

#[test]
fn throttle_tab_stacks_the_axes_with_prefilt_and_zooms_to_the_flown_range() {
    // docs/design/throttle-tab (T1): R/P/Y × (prefilt | gyro) per log, one
    // colour scale per log, the throttle axis on the range the log flew
    let mut ui = Ui::open(&[LOG_2025], Tab::ThrottleHeatmap);
    assert_eq!(jobs(&ui, "thr"), 6, "3 axes × gyro and prefilt");
    assert!(ui.sees("A prefilt"));
    assert!(ui.sees("A Gyro"));
    assert!(ui.has_plot("throttle/A/yaw/prefilt"));
    assert_eq!(
        ui.app().colour_scales.len(),
        1,
        "prefilt and gyro share a scale"
    );
    // 0–100 % throttle as the reference tool; btfl_027 flew 10–34 %, and
    // "Zoom to the flown range" ends the axis at the edges of those bins
    assert_eq!(ui.app().throttle_view.x_range, Some((0.0, 100.0)));
    ui.check("Zoom to the flown range", true);
    let (lo, hi) = ui.app().throttle_view.x_range.expect("range applied");
    assert!(
        (9.0..=10.0).contains(&lo) && (34.0..=35.0).contains(&hi),
        "{lo}..{hi}"
    );
    // and that is what the plots show
    let (x0, x1) = ui.plot("throttle/A/roll").x_range();
    assert!((x0 - lo).abs() < 0.5 && (x1 - hi).abs() < 0.5, "{x0}..{x1}");
    // prefilt off: the gyro alone
    ui.check("Prefilt beside it", false);
    assert!(!ui.sees("A prefilt"));
    assert!(ui.sees("A Gyro"));
    assert!(!ui.has_plot("throttle/A/yaw/prefilt"));
}

#[test]
fn rpm_overlay_on_the_throttle_heatmap_has_per_motor_toggles() {
    let mut ui = Ui::open(&[LOG_4_5_1], Tab::ThrottleHeatmap);
    let overlay = result(&ui, |o| match o {
        Output::Throttle(t) => t.overlay.clone(),
        _ => None,
    })
    .expect("the 4.5.1 log has motor rpm");
    assert_eq!(overlay.motors.len(), 4);
    assert_eq!(overlay.harmonics, 3, "rpm_filter_harmonics 3");
    assert!(overlay.motors[0].iter().flatten().any(|r| *r > 1000.0));
    assert!(overlay.dyn_notch_hz.is_some());
    assert!(!ui.app().overlay.show, "the maps alone by default");
    ui.check("RPM overlay", true);
    assert!(ui.app().overlay.show);
    assert!(!ui.app().overlay.per_motor, "one line per harmonic");
    assert!(!ui.app().overlay.notches, "notches off by default");
    assert!(!ui.has("M2"), "motor toggles only per motor");
    // the default line follows the mean of the motors
    let mean = crate::views::mean_motor_rpm(&overlay);
    let i = mean
        .iter()
        .position(Option::is_some)
        .expect("a bin with speeds");
    let speeds: Vec<f64> = overlay.motors.iter().filter_map(|m| m[i]).collect();
    let avg = speeds.iter().sum::<f64>() / speeds.len() as f64;
    assert!((mean[i].unwrap() - avg).abs() < 1e-9);
    ui.check("Per motor", true);
    assert!(ui.app().overlay.per_motor);
    assert_eq!(ui.app().overlay.motors, vec![true; 4]);
    ui.check("M2", false);
    assert_eq!(ui.app().overlay.motors, vec![true, false, true, true]);
    ui.check("RPM overlay", false);
    assert!(!ui.app().overlay.show);
    ui.check("Notches", true);
    assert!(ui.app().overlay.notches);
    assert_eq!(
        ui.app().cache.len(),
        6,
        "3 axes × gyro and prefilt; the overlay is drawn from the cached results"
    );
    // a log without rpm has no overlay controls
    let ui = Ui::open(&[LOG_4_2], Tab::ThrottleHeatmap);
    assert!(!ui.has("RPM overlay"));
}

#[test]
fn heatmaps_show_a_colour_scale_with_linked_maxima_and_a_hover_value() {
    let mut ui = Ui::open(&[LOG_4_2, LOG_2025], Tab::ThrottleHeatmap);
    // linked: both panels share the larger automatic maximum
    let linked = ui.app().colour_scales.clone();
    assert_eq!(linked.len(), 2, "{linked:?}");
    assert!(
        linked[0].1 > 0.0 && (linked[0].1 - linked[1].1).abs() < 1e-12,
        "{linked:?}"
    );
    assert!(ui.sees("colour scale 0 to"));
    // unlinked: each its own, the larger one equals the shared value
    ui.app_mut().tools.heatmap_link = false;
    ui.settle();
    let own = ui.app().colour_scales.clone();
    assert!((own[0].1 - own[1].1).abs() > 1e-9, "{own:?}");
    assert!((own.iter().map(|s| s.1).fold(0.0, f64::max) - linked[0].1).abs() < 1e-12);
    // an explicit colour max wins everywhere
    ui.app_mut().tools.heatmap_max = 5.0;
    ui.settle();
    assert!(ui.app().colour_scales.iter().all(|s| s.1 == 5.0));
    assert!(ui.sees("colour scale 0 to 5.00"));
    assert!(!ui.sees("deg/s"), "no unit on the heatmaps");
    // the cell under the pointer is read out and matches the grid
    ui.plot("throttle/A/roll").hover_middle();
    let (slot, x, y, z) = ui.app().heatmap_hover.expect("a cell under the pointer");
    assert_eq!(slot, Slot::A);
    assert!((0.0..=100.0).contains(&x) && y >= 0.0, "{x} {y}");
    let result = shown(&ui);
    let Output::Throttle(t) = result.as_ref() else {
        panic!("throttle output")
    };
    assert_eq!(openpid_analysis::render::value_at(&t.grid, x, y), Some(z));
    assert!(ui.sees("Hz:"));
    ui.plot("throttle/A/roll").leave();
    assert!(ui.app().heatmap_hover.is_none());
    // freq × time shows the docked log, here A, on its own scale in
    // decibels (the colour max is the throttle maps'), with the same read-out
    ui.app_mut().dock = Some(Slot::A);
    ui.tab(Tab::TimeHeatmap);
    assert_eq!(ui.app().colour_scales, [(Slot::A, 10.0)], "the docked log");
    ui.plot("time/roll").hover_middle();
    let (slot, x, y, z) = ui.app().heatmap_hover.expect("a cell under the pointer");
    assert_eq!(slot, Slot::A);
    let result = shown(&ui);
    let Output::Stft(t) = result.as_ref() else {
        panic!("the map of frequency against time")
    };
    assert_eq!(openpid_analysis::render::value_at(&t.grid, x, y), Some(z));
    assert!(ui.sees(" s, "));
    // the project keeps the link setting
    assert!(!ui.app().project(Path::new("p.openpid.json")).heatmap_link);
}

#[test]
fn time_tab_stacks_the_axes_in_db_with_the_throttle_under_them() {
    // docs/design/time-tab (F2): roll, pitch and yaw spectrograms of the
    // docked log (the unfiltered gyro by default), the spectrum at the
    // position beside each row, the throttle under them. The map is the one
    // of DESIGN §6.5: a power density in decibels on −30 … 10 dB
    let mut ui = Ui::open(&[LOG_2025], Tab::TimeHeatmap);
    assert_eq!(jobs(&ui, "stft"), 3, "one spectrogram per axis");
    for axis in ["roll", "pitch", "yaw"] {
        assert!(ui.has_plot(&format!("time/{axis}")), "{axis}");
    }
    let display = ui.app().tools.time_display.clone();
    assert!(display.prefilt);
    assert_eq!(display.scale, ColourScale::Fixed);
    assert_eq!(
        ui.app().tools.stft.channel,
        ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Raw
        },
        "prefilt by default, as the reference tool"
    );
    assert!(ui.sees("A prefilt"));
    assert!(ui.sees("power density (dB)"));
    assert!(
        !ui.has("dB"),
        "no switch to a linear colour: decibels always"
    );
    assert_eq!(ui.app().colour_scales, [(Slot::A, 10.0)]);
    assert!(ui.sees("colour scale -30 to 10"));
    // both lists at "low", the rows to half the log rate
    let (stft, t) = (ui.app().tools.stft.clone(), time_map(&ui));
    assert_eq!(
        (stft.smooth_freq, stft.smooth_time, stft.max_hz),
        (FreqSmoothing::Low, TimeSmoothing::Low, None)
    );
    assert_eq!(t.hop * 2, t.nperseg);
    assert!((t.grid.y.last().unwrap() - t.fs / 2.0).abs() < 1e-6);
    let (_, top) = ui.plot("time/roll").y_range();
    assert!((top - t.fs / 2.0).abs() < 0.01 * t.fs, "{top}");
    // filtered gyro: three new jobs
    ui.check("Prefilt", false);
    assert_eq!(jobs(&ui, "stft"), 6);
    assert!(ui.sees("A Gyro"));
}

/// The map the Freq × time tab shows for roll.
fn time_map(ui: &Ui) -> openpid_analysis::StftOutput {
    match shown(ui).as_ref() {
        Output::Stft(t) => t.clone(),
        _ => panic!("the map of frequency against time"),
    }
}

#[test]
fn time_tab_lists_set_the_two_levels_of_the_map() {
    let mut ui = Ui::open(&[LOG_2025], Tab::TimeHeatmap);
    let low = time_map(&ui);
    let columns = |t: &openpid_analysis::StftOutput| t.grid.x.len();
    assert!(ui.sees(&format!("column 1 of {}", columns(&low))));
    // along the frequency: the same grid, other cells; a job per axis
    for (name, level) in [
        ("high", FreqSmoothing::High),
        ("medium", FreqSmoothing::Medium),
        ("off", FreqSmoothing::Off),
    ] {
        let before = jobs(&ui, "stft");
        ui.pick("Smooth frequency", name);
        assert_eq!(ui.app().tools.stft.smooth_freq, level);
        assert_eq!(jobs(&ui, "stft"), before + 3, "{name}");
        let t = time_map(&ui);
        assert_eq!(t.smooth_freq, level);
        assert_eq!(
            (t.grid.x.len(), t.grid.y.len()),
            (columns(&low), low.grid.y.len())
        );
        assert_ne!(t.grid.z, low.grid.z, "{name}");
    }
    ui.pick("Smooth frequency", "low");
    assert_eq!(time_map(&ui).grid.z, low.grid.z, "the first result again");
    // along the time: the distance between the columns, so their number
    for (name, level, per_segment) in [
        ("off", TimeSmoothing::Off, 1),
        ("medium", TimeSmoothing::Medium, 5),
        ("high", TimeSmoothing::High, 10),
    ] {
        ui.pick("Smooth time", name);
        assert_eq!(ui.app().tools.stft.smooth_time, level);
        let t = time_map(&ui);
        assert_eq!(t.smooth_time, level);
        assert_eq!(t.hop * per_segment, t.nperseg, "{name}");
        assert!(
            ui.sees(&format!("of {}", columns(&t))),
            "the player counts them"
        );
        assert_eq!(ui.app().time_view.columns, columns(&t));
        for axis in ["roll", "pitch", "yaw"] {
            assert!(ui.has_plot(&format!("time/{axis}")), "{name} {axis}");
        }
    }
    // ten columns a segment: about five times those of "low", all drawn
    let high = time_map(&ui);
    assert!(columns(&high) > 4 * columns(&low));
    let widest = ui
        .app()
        .heatmap_textures
        .values()
        .map(|t| t.size()[0])
        .max()
        .unwrap();
    let limit = ui.ctx().input(|i| i.max_texture_side);
    assert!(widest <= limit, "{widest} pixels, the limit is {limit}");
    // the project keeps both levels
    let saved = ui.app().project(Path::new("p.openpid.json"));
    assert_eq!(
        (saved.stft.smooth_freq, saved.stft.smooth_time),
        (FreqSmoothing::Low, TimeSmoothing::High)
    );
    // the lists in the other language of the interface
    ui.pick("Smooth time", "low");
    ui.app_mut().prefs.lang = crate::i18n::Lang::Ua;
    ui.settle();
    assert!(ui.has("Згладж. частоти") && ui.has("Згладж. часу"));
    assert!(ui.has("Шкала кольору") && ui.sees("густина потужності (дБ)"));
    ui.pick("Згладж. частоти", "висока");
    assert_eq!(ui.app().tools.stft.smooth_freq, FreqSmoothing::High);
}

#[test]
fn time_tab_colour_scale_is_fixed_automatic_or_typed() {
    let mut ui = Ui::open(&[LOG_2025], Tab::TimeHeatmap);
    let scale = |ui: &Ui| ui.app().time_view.range.expect("a scale on screen");
    let textures = |ui: &Ui| ui.app().heatmap_textures.len();
    // the reference tool's scale by default, with no ends to type
    assert_eq!(scale(&ui), (-30.0, 10.0));
    assert!(!ui.has("Z min") && !ui.has("Z max"));
    let fixed = textures(&ui);
    // automatic: from the maps of the log, one scale for the three rows
    ui.pick("Colour scale", "automatic");
    assert_eq!(ui.app().tools.time_display.scale, ColourScale::Automatic);
    let (lo, hi) = scale(&ui);
    assert!(lo.is_finite() && hi >= lo + 1.0, "{lo} … {hi}");
    assert_ne!((lo, hi), (-30.0, 10.0));
    assert_eq!((lo.fract(), hi.fract()), (0.0, 0.0), "whole decibels");
    let ranges: Vec<(f64, f64)> = [Axis::Roll, Axis::Pitch, Axis::Yaw]
        .into_iter()
        .map(|axis| {
            let channel = ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            };
            let grid = result(&ui, |o| match o {
                Output::Stft(t) if t.channel == channel => Some(t.grid.clone()),
                _ => None,
            })
            .unwrap();
            crate::time_tab::auto_db_range(&grid)
        })
        .collect();
    assert_eq!(lo, ranges.iter().map(|r| r.0).fold(f64::INFINITY, f64::min));
    assert_eq!(
        hi,
        ranges.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max)
    );
    assert_eq!(ui.app().colour_scales, [(Slot::A, hi)]);
    assert!(ui.sees(&format!("colour scale {lo:.0} to {hi:.0}")));
    assert_eq!(textures(&ui), fixed + 3, "the maps are drawn anew");
    // typed in: the two ends start from the scale on screen
    ui.pick("Colour scale", "typed in");
    let d = ui.app().tools.time_display.clone();
    assert_eq!((d.scale, d.z_min, d.z_max), (ColourScale::Typed, lo, hi));
    assert_eq!(scale(&ui), (lo, hi));
    ui.set("Z min", -70.0);
    ui.set("Z max", -5.0);
    assert_eq!(scale(&ui), (-70.0, -5.0));
    assert!(ui.sees("colour scale -70 to -5"));
    assert_eq!(ui.app().colour_scales, [(Slot::A, -5.0)]);
    // the top stays a decibel above the floor
    ui.set("Z max", -90.0);
    assert_eq!(ui.app().tools.time_display.z_max, -90.0);
    assert_eq!(scale(&ui), (-70.0, -69.0));
    // back to the fixed scale; the typed ends are kept for the next time
    ui.pick("Colour scale", "−30 … 10 dB");
    assert_eq!(scale(&ui), (-30.0, 10.0));
    assert!(!ui.has("Z min"));
    ui.pick("Colour scale", "typed in");
    assert_eq!(scale(&ui), (-30.0, 10.0), "from the scale on screen again");
    // the spectrum beside a row is drawn on the scale of the maps
    assert!(ui.sees("A spectrum at"));
}

#[test]
fn time_tab_max_frequency_lowers_the_top_of_the_maps() {
    let mut ui = Ui::open(&[LOG_2025], Tab::TimeHeatmap);
    let whole = time_map(&ui);
    let half_rate = whole.fs / 2.0;
    assert_eq!(ui.app().tools.stft.max_hz, None);
    // "auto" is half the log rate; a click fixes the top at 1000 Hz
    ui.click_button("Max frequency");
    assert_eq!(ui.app().tools.stft.max_hz, Some(1000.0));
    ui.set("Max frequency", 300.0);
    assert_eq!(ui.app().tools.stft.max_hz, Some(300.0));
    let cut = time_map(&ui);
    let rows = cut.grid.y.len();
    assert!(rows < whole.grid.y.len());
    assert!(*cut.grid.y.last().unwrap() <= 300.0);
    assert!(whole.grid.y[rows] > 300.0, "the next row is above the top");
    // the rows kept are those of the whole map: cut after the smoothing
    let nt = whole.grid.x.len();
    assert_eq!(cut.grid.z[..], whole.grid.z[..rows * nt]);
    let (_, top) = ui.plot("time/roll").y_range();
    assert!(top <= 300.0 + whole.fs / whole.nperseg as f64, "{top}");
    // out of range: held to 20 Hz … 16 kHz
    ui.set("Max frequency", 1.0);
    assert_eq!(ui.app().tools.stft.max_hz, Some(20.0));
    ui.set("Max frequency", 1e6);
    assert_eq!(ui.app().tools.stft.max_hz, Some(16_000.0));
    assert_eq!(time_map(&ui).grid.y.len(), whole.grid.y.len());
    // "auto" beside the number goes back
    ui.click("auto");
    assert_eq!(ui.app().tools.stft.max_hz, None);
    let (_, top) = ui.plot("time/roll").y_range();
    assert!((top - half_rate).abs() < 0.01 * whole.fs, "{top}");
}

#[test]
fn time_tab_says_when_the_window_is_too_short_for_a_segment() {
    // a 1 kHz log: a segment is 300 samples, and at "low" the first column
    // needs 150 + 300 + 1 of them; a window of 0.3 s has too few
    let mut ui = Ui::open(&[LOG_4_2], Tab::TimeHeatmap);
    assert!(ui.has_plot("time/roll"));
    set_epoch(&mut ui, Some((2.0, 2.3)));
    assert!(ui.sees("A: not enough data"));
    assert!(ui.sees("the first segment of 300 needs 451"));
    assert!(!ui.has_plot("time/roll"));
    // a window with room for the columns brings the maps back
    set_epoch(&mut ui, Some((2.0, 6.0)));
    assert!(ui.has_plot("time/roll") && !ui.sees("not enough data"));
    assert!(time_map(&ui).grid.x.len() > 20);
}

#[test]
fn a_map_wider_than_a_texture_is_drawn_by_the_largest_cell_of_a_group() {
    use openpid_analysis::render::Colormap;
    use openpid_core::analysis::Grid2D;
    // 5 columns × 3 rows, the cell of column c and row r holding 10·r + c
    let grid = Grid2D {
        x: (0..5).map(f64::from).collect(),
        y: (0..3).map(f64::from).collect(),
        z: (0..3)
            .flat_map(|r| (0..5).map(move |c| f64::from(10 * r + c)))
            .collect(),
        x_label: String::new(),
        y_label: String::new(),
        z_label: String::new(),
    };
    let cmap = Colormap::Explorer;
    let colour = |v: f64| cmap.rgba((v / 24.0).max(1e-6));
    let scale = (0.0, 24.0);
    // it fits: a pixel per cell, the highest row first
    let (w, h, rgba) = crate::time_tab::pixels(&grid, scale, cmap, 8);
    assert_eq!((w, h), (5, 3));
    assert_eq!(rgba.len(), 4 * 15);
    assert_eq!(rgba[..4], colour(20.0));
    assert_eq!(rgba[4 * 14..], colour(4.0));
    // at most 2 pixels a side: 3 columns and 2 rows in a pixel, each pixel
    // the largest of its cells; the last pixel of a row holds what is left
    let (w, h, rgba) = crate::time_tab::pixels(&grid, scale, cmap, 2);
    assert_eq!((w, h), (2, 2));
    let pixel = |i: usize| -> [u8; 4] { rgba[4 * i..4 * i + 4].try_into().unwrap() };
    // top row of pixels: grid row 2 alone; bottom: rows 0 and 1
    assert_eq!(pixel(0), colour(22.0));
    assert_eq!(pixel(1), colour(24.0));
    assert_eq!(pixel(2), colour(12.0));
    assert_eq!(pixel(3), colour(14.0));
    // below the scale is the lowest colour, not transparent
    let (_, _, rgba) = crate::time_tab::pixels(&grid, (30.0, 40.0), cmap, 8);
    assert!(rgba.chunks(4).all(|p| p[3] == 255 && p == cmap.rgba(1e-6)));
    // the automatic scale of a grid: of its values above 20 Hz (here the
    // ten values 10 … 14 and 20 … 24 of the rows at 100 and 200 Hz) the 30th
    // percentile, the third of them, and the 99.5th, the ninth; the fixed
    // scale when no row is above 20 Hz
    let range = crate::time_tab::auto_db_range(&Grid2D {
        y: vec![0.0, 100.0, 200.0],
        ..grid.clone()
    });
    assert_eq!(range, (12.0, 23.0));
    assert_eq!(crate::time_tab::auto_db_range(&grid), (-30.0, 10.0));
}

#[test]
fn player_keys_work_on_the_time_tab_only() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Spectrum);
    ui.key(Key::Space);
    assert!(!ui.app().player.playing, "no player on Spectrum");
    ui.tab(Tab::TimeHeatmap);
    ui.key(Key::Space);
    assert!(ui.app().player.playing);
    ui.key(Key::Space);
    assert!(!ui.app().player.playing);
    let start = ui.app().player.pos_s.expect("position");
    ui.key(Key::Period);
    let after_step = ui.app().player.pos_s.expect("position");
    assert!(after_step > start, "one column forward");
    ui.key(Key::Comma);
    assert!(ui.app().player.pos_s.unwrap() < after_step);
    ui.key(Key::Period);
    ui.key(Key::Period);
    ui.key(Key::Home);
    let home = ui.app().player.pos_s.unwrap();
    assert!(
        home < start,
        "Home rewinds to the start of the data, before {start}, got {home}"
    );
}

#[test]
fn time_heatmap_player_plays_scrubs_and_syncs_the_cursor() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::TimeHeatmap);
    let pos = |ui: &Ui| ui.app().player.pos_s.expect("a position");
    assert!(!ui.app().player.playing);
    let first = pos(&ui);
    assert_eq!(
        ui.app().viewer.cursor_s,
        Some(first),
        "the cursor follows the player"
    );
    assert!(ui.sees("column 1 of"));
    assert!(ui.sees("A spectrum at"));
    // play: a frame of the tests is 0.25 s, so four frames are a second at 1×
    ui.click("Play");
    assert!(ui.app().player.playing);
    let a = pos(&ui);
    ui.advance(4);
    let b = pos(&ui);
    assert!((b - a - 1.0).abs() < 0.3, "{a} → {b} in a second");
    assert_eq!(ui.app().viewer.cursor_s, Some(b), "cursor in step");
    assert!(ui.app().player.column.unwrap() > 0);
    // faster
    ui.app_mut().player.speed = 4.0;
    ui.advance(4);
    let fast = pos(&ui);
    assert!(fast - b > 3.0, "{b} → {fast} at 4×");
    // pause holds the position
    ui.click("Pause");
    assert!(!ui.app().player.playing);
    let held = pos(&ui);
    assert!(held >= fast);
    ui.advance(3);
    assert_eq!(pos(&ui), held);
    // one column either way
    let col = ui.app().player.column.unwrap();
    ui.click("step >");
    assert_eq!(ui.app().player.column, Some(col + 1));
    ui.click("< step");
    assert_eq!(ui.app().player.column, Some(col));
    // the log-viewer cursor moved: the player follows it
    let (x0, x1) = ui.plot("time/roll").x_range();
    let mid = f64::midpoint(x0, x1);
    ui.app_mut().viewer.cursor_s = Some(mid);
    ui.settle();
    let followed = pos(&ui);
    assert!((followed - mid).abs() < 0.011, "{followed} vs {mid}");
    assert_eq!(ui.app().viewer.cursor_s, Some(followed));
    // clicking the heatmap scrubs
    let want = x0 + (x1 - x0) * 0.25;
    ui.plot("time/roll").click(want);
    let p = pos(&ui);
    assert!((p - want).abs() < (x1 - x0) * 0.02, "{p} vs {want}");
    assert_eq!(ui.app().viewer.cursor_s, Some(p));
    // to start, then play past the end loops
    ui.click("To start");
    assert_eq!(pos(&ui), first);
    ui.app_mut().player.pos_s = Some(x1 - 0.1);
    ui.app_mut().player.speed = 1.0;
    ui.click("Play");
    assert!(pos(&ui) < first + 1.5, "looped: {}", pos(&ui));
    ui.click("Pause");
    // touch: a drag over the heatmap scrubs instead of panning
    ui.app_mut().touch_mode = true;
    ui.settle();
    let (x0, x1) = ui.plot("time/roll").x_range();
    let at = |f: f64| x0 + (x1 - x0) * f;
    ui.plot("time/roll").grab(at(0.5), at(0.7));
    ui.plot("time/roll").drag_to(at(0.7));
    let dragged = pos(&ui);
    ui.plot("time/roll").release(at(0.7));
    assert!(
        (dragged - at(0.7)).abs() < (x1 - x0) * 0.05,
        "{dragged} vs {}",
        at(0.7)
    );
    assert_eq!(ui.plot("time/roll").x_range(), (x0, x1), "no pan");
    // without a log there is no player
    let ui = Ui::open(&[], Tab::TimeHeatmap);
    assert!(!ui.has("Play"));
}

#[test]
fn plot_colours_follow_the_setting() {
    use crate::prefs::{PlotColours, Prefs};
    // docs/design/plot-colours: Blackbox Explorer by default, Okabe–Ito and
    // viridis as the colour-blind safe choice
    let dir = temp_dir("colours");
    let prefs = dir.join("prefs.json");
    let mut ui = Ui::setup()
        .files([LOG_4_2])
        .tab(Tab::ThrottleHeatmap)
        .prefs(&prefs)
        .open();
    assert_eq!(ui.app().prefs.plot_colours, PlotColours::Explorer);
    assert_eq!(
        crate::views::slot_color(Slot::A),
        crate::colormap::EXPLORER_PALETTE[0],
        "A in Explorer's red"
    );
    assert_eq!(
        crate::colormap::heatmap(),
        openpid_analysis::render::Colormap::Explorer
    );
    let textures = ui.app().heatmap_textures.len();
    ui.tab(Tab::Settings);
    ui.click("Colour-blind safe");
    assert_eq!(ui.app().prefs.plot_colours, PlotColours::ColourBlindSafe);
    assert_eq!(
        crate::views::slot_color(Slot::A),
        crate::colormap::SLOT_PALETTE[0]
    );
    // the heatmaps are rasterised again in viridis
    ui.tab(Tab::ThrottleHeatmap);
    assert!(ui.app().heatmap_textures.len() > textures);
    assert_eq!(
        crate::colormap::heatmap(),
        openpid_analysis::render::Colormap::Viridis
    );
    // and the choice is saved
    assert_eq!(
        Prefs::load(&prefs).plot_colours,
        PlotColours::ColourBlindSafe
    );
    let _ = std::fs::remove_dir_all(&dir);
}
