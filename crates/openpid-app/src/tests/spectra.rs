//! The spectral tabs: Spectrum, Freq × throttle and Freq × time with its
//! player; their colour scales and overlays.

use super::{LOG_4_2, LOG_4_5_1, LOG_2025, jobs, result, shown, temp_dir};
use crate::app::Tab;
use crate::spectrum_tab::Quantity;
use crate::testkit::{Ui, at_every_size};
use crate::worker::{Output, Slot};
use eframe::egui::Key;
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
fn spectrum_draws_the_curve_by_default_and_the_density_on_request() {
    use openpid_analysis::PsdMethod;
    let mut ui = Ui::open(&[LOG_4_2], Tab::Spectrum);
    // decision D-001: the reference tool's curve at its default level
    assert_eq!(ui.app().tools.psd.method, PsdMethod::Curve);
    assert_eq!(ui.app().tools.psd.smoothing, 2, "medium");
    // every finished full-range spectrum of the filtered roll gyro: method,
    // segments, frequency step and how much the curve moves from point to
    // point
    let spectra = |ui: &Ui| -> Vec<(PsdMethod, usize, f64, f64)> {
        let roll = Quantity::Gyro.channel(Axis::Roll);
        ui.app()
            .cache
            .values()
            .filter_map(|c| match c {
                crate::app::Cached::Ready(o) => match o.as_ref() {
                    Output::Psd(p)
                        if p.channel == roll && p.freq.last().is_some_and(|f| *f > 200.0) =>
                    {
                        let rough = p
                            .power_db
                            .windows(2)
                            .map(|w| (w[1] - w[0]).abs())
                            .sum::<f64>()
                            / p.power_db.len() as f64;
                        Some((p.method, p.segments, p.freq[1], rough))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };
    let first = spectra(&ui);
    assert_eq!(first.len(), 1, "{first:?}");
    let (method, segments, step, medium) = first[0];
    assert_eq!((method, segments), (PsdMethod::Curve, 1));
    // one transform of the whole analysis window: steps far below 1 Hz
    assert!(step < 0.5, "{step}");
    // a higher level is a flatter curve
    ui.app_mut().tools.psd.smoothing = 4;
    ui.settle();
    let both = spectra(&ui);
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both.iter().any(|s| s.3 < medium), "{both:?}");
    ui.app_mut().tools.psd.smoothing = 2;

    // the density: averaged segments, its own smoothing list and a segment
    assert!(!ui.sees("Segment"));
    ui.app_mut().tools.psd.method = PsdMethod::Density;
    ui.settle();
    let density: Vec<_> = spectra(&ui)
        .into_iter()
        .filter(|s| s.0 == PsdMethod::Density)
        .collect();
    assert_eq!(density.len(), 1, "{density:?}");
    assert!(density[0].1 > 1 && density[0].2 >= 1.0, "{density:?}");
    assert!(ui.sees("Segment"));
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
    assert!(ui.sees("Segment"));
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
    // the same scale and read-out on freq × time (linear; it shows the
    // docked log, here A)
    ui.app_mut().tools.time_display.db = false;
    ui.app_mut().dock = Some(Slot::A);
    ui.tab(Tab::TimeHeatmap);
    assert_eq!(ui.app().colour_scales.len(), 1, "the docked log");
    assert!(ui.app().colour_scales.iter().all(|s| s.1 == 5.0));
    ui.plot("time/roll").hover_middle();
    assert!(ui.app().heatmap_hover.is_some());
    assert!(ui.sees(" s, "));
    // the project keeps the link setting
    assert!(!ui.app().project(Path::new("p.openpid.json")).heatmap_link);
}

#[test]
fn time_tab_stacks_the_axes_in_db_with_the_throttle_under_them() {
    // docs/design/time-tab (F2): roll, pitch and yaw spectrograms of the
    // docked log (the unfiltered gyro by default), in dB, the spectrum at
    // the position beside each row, the throttle under them
    let mut ui = Ui::open(&[LOG_2025], Tab::TimeHeatmap);
    assert_eq!(jobs(&ui, "stft"), 3, "one spectrogram per axis");
    for axis in ["roll", "pitch", "yaw"] {
        assert!(ui.has_plot(&format!("time/{axis}")), "{axis}");
    }
    assert!(ui.app().tools.time_display.db && ui.app().tools.time_display.prefilt);
    assert_eq!(
        ui.app().tools.stft.channel,
        ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Raw
        },
        "prefilt by default, as the reference tool"
    );
    assert!(ui.sees("A prefilt"));
    // the dB range comes from the maps: negative floor, top above it
    let (_, top) = ui.app().colour_scales[0];
    assert!(ui.sees("colour scale -"), "a dB floor below 0");
    assert!(top.is_finite());
    // a fixed floor and top take over
    ui.app_mut().tools.time_display.z_min = Some(-30.0);
    ui.app_mut().tools.time_display.z_max = Some(10.0);
    ui.settle();
    assert_eq!(ui.app().colour_scales[0].1, 10.0);
    assert!(ui.sees("colour scale -30 to 10"));
    // filtered gyro: three new jobs
    ui.check("Prefilt", false);
    assert_eq!(jobs(&ui, "stft"), 6);
    assert!(ui.sees("A Gyro"));
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
