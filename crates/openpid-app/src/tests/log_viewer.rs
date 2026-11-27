//! The Log viewer: rows, quantities, the cursor, the sticks and the flight
//! modes, the channel list, touch gestures, and one axis at full height.

use super::{LOG_4_2, LOG_2025, jobs};
use crate::app::Tab;
use crate::log_tab::{LineSmoothing, LogQuantity, line_summary, period_text, row_summary};
use crate::testkit::{Ui, at_every_size};
use crate::worker::Slot;
use eframe::egui::Key;
use openpid_core::{Axis, ChannelId, FlightMode, GyroStage};
use openpid_dsp::smooth::CutMovingAverage;

fn rows(ui: &Ui) -> Vec<&'static str> {
    row_summary(ui.app()).iter().map(|r| r.0).collect()
}

#[test]
fn log_viewer_rows_quantities_cursor_and_sticks() {
    at_every_size(|screen| {
        let mut ui = Ui::open_on(&[LOG_2025], Tab::LogViewer, screen);
        // the reference tool's default: gyro and setpoint on every axis, the
        // throttle and the motors under them; the setpoint drawn first
        assert_eq!(rows(&ui), ["roll", "pitch", "yaw", "motors"]);
        let summary = row_summary(ui.app());
        assert_eq!(summary[0].1, ["setpoint", "gyro"]);
        assert_eq!(summary[3].1, ["throttle", "M1", "M2", "M3", "M4"]);
        // one scale for the three axes: ± 500 deg/s
        assert_eq!(ui.plot("log/roll").y_range(), (-500.0, 500.0));

        // a quantity adds a line on every axis; one the log lacks is greyed out
        ui.check("P-term", true);
        ui.check("Motor RPM", true);
        assert!(ui.app().tools.log_display.shows(LogQuantity::PTerm));
        assert_eq!(row_summary(ui.app())[1].1, ["setpoint", "gyro", "P"]);
        assert_eq!(rows(&ui).last(), Some(&"rpm"));
        assert!(!ui.is_enabled("Debug"), "no debug in this log");
        ui.check("Motors", false);
        ui.check("Throttle", false);
        assert_eq!(rows(&ui), ["roll", "pitch", "yaw", "rpm"], "no motor row");

        // other channels get a row of their own, unless a quantity draws them
        ui.app_mut().tools.viewer_channels = vec![
            ChannelId::Gyro {
                axis: Axis::Roll,
                stage: GyroStage::Filtered,
            },
            ChannelId::RcCommand { ch: 3 },
        ];
        ui.settle();
        assert_eq!(
            row_summary(ui.app()).last().unwrap(),
            &("other", vec!["rc.3".to_string()])
        );

        // the y scale
        ui.set("Y scale ±", 250.0);
        assert_eq!(ui.plot("log/pitch").y_range(), (-250.0, 250.0));

        // the sticks: none without a cursor or a pointer over the rows
        assert!(ui.app().viewer.sticks.is_none());
        // the pointer over a row shows the sticks and the flight modes of
        // that moment, on a card at the right edge under the first row
        ui.plot("log/yaw").hover(20.0);
        let sticks = ui.app().viewer.sticks.expect("sticks under the pointer");
        assert!((0.0..=1.0).contains(&sticks.throttle.unwrap()));
        let modes = ui
            .app()
            .viewer
            .modes
            .clone()
            .expect("modes under the pointer");
        assert_eq!(modes, [FlightMode::Arm, FlightMode::PosHold]);
        let card = ui.app().viewer.sticks_rect.unwrap();
        let first_row = ui.plot("log/roll").probe().frame;
        assert!(card.right() <= first_row.right() + 1.0 && card.bottom() > first_row.bottom());

        // a click places the cursor where it was made; its time is in the panel
        ui.plot("log/roll").click(14.2);
        let cursor = ui.app().viewer.cursor_s.expect("cursor placed");
        assert!(
            (cursor - 14.2).abs() <= ui.plot("log/roll").x_tolerance(),
            "{cursor}"
        );
        assert!(ui.has("Position"));

        // the pointer leaves the rows: the sticks stay at the cursor
        ui.plot("log/yaw").leave();
        let at_cursor = ui
            .app()
            .loaded(Slot::A)
            .unwrap()
            .current()
            .unwrap()
            .sticks_at(cursor);
        assert_eq!(ui.app().viewer.sticks, at_cursor);

        // they can be hidden; these are the last controls of a long panel,
        // out of view on a short window
        assert!(ui.app().tools.log_display.sticks, "shown by default");
        ui.check("Sticks", false);
        assert!(ui.app().viewer.sticks.is_none());
        assert!(
            ui.app().viewer.modes.is_some(),
            "the modes stay on the card"
        );
        ui.check("Flight modes", false);
        assert!(ui.app().viewer.modes.is_none() && ui.app().viewer.sticks_rect.is_none());
        // the marks where the modes change: off by default
        assert_eq!(ui.app().viewer.mode_marks, 0);
        ui.check("Mode changes", true);
        assert!(ui.app().viewer.mode_marks >= 1, "btfl_027 changes modes");
        // the lines run through every row, from the top of the first to
        // the bottom of the last (#141)
        let (top, bottom) = ui.app().viewer.mode_lines.expect("the lines are drawn");
        let first = ui.plot("log/roll").probe().frame;
        let last = ui.plot("log/other").probe().frame;
        assert!((top - first.top()).abs() < 1.0, "{top} vs {first:?}");
        assert!((bottom - last.bottom()).abs() < 1.0, "{bottom} vs {last:?}");
        assert!(bottom - top > 3.0 * first.height(), "more than one row");
        // the pointer on a line reads the time and the modes from then on
        let session = ui.app().loaded(Slot::A).unwrap().current().unwrap().clone();
        let changes = session.mode_changes();
        let (at, modes) = changes
            .iter()
            .find(|(t, _)| (12.0..14.0).contains(t))
            .expect("ANGLE to POSHOLD near 12.9 s");
        assert!(modes.contains(&FlightMode::PosHold));
        assert!(
            changes[0].0 < 0.5,
            "the modes at the start: {}",
            changes[0].0
        );
        assert!(
            ui.app().viewer.mode_hover.is_none(),
            "the pointer is elsewhere"
        );
        ui.plot("log/pitch").hover(*at);
        let label = ui
            .app()
            .viewer
            .mode_hover
            .clone()
            .expect("a label at the pointer");
        assert_eq!(label, format!("{at:.1} s · ARM · POSHOLD"));
        ui.plot("log/pitch").hover(at + 4.0);
        assert!(ui.app().viewer.mode_hover.is_none(), "away from the line");
    });
}

#[test]
fn channel_filter_and_toggles_edit_the_channel_list() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::LogViewer);
    assert!(ui.app().tools.viewer_channels.is_empty(), "none by default");
    // the other channels are folded away under the quantities
    assert!(!ui.has("rc.0"));
    ui.click("Other channels");
    // the list scrolls; the filter brings a channel into view
    ui.type_into("filter", "rc.");
    ui.click("rc.0");
    assert_eq!(
        ui.app().tools.viewer_channels,
        vec![ChannelId::RcCommand { ch: 0 }]
    );
    assert!(ui.has("Other channels (1)"));
    ui.click("rc.0");
    assert!(ui.app().tools.viewer_channels.is_empty());
    ui.app_mut().viewer.channel_filter.clear();
    ui.settle();
    ui.type_into("filter", "thr");
    assert_eq!(ui.app().viewer.channel_filter, "thr");
    assert!(!ui.has("rc.0"), "filtered out");
    assert!(ui.has("throttle"));
}

#[test]
fn horizontal_pinch_zooms_time_only_and_a_long_press_places_the_cursor() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::LogViewer);
    let span = |b: (f64, f64)| b.1 - b.0;
    let x_before = ui.plot("log/roll").x_range();
    let y_before = ui.plot("log/roll").y_range();
    // two fingers 100 points apart, then 300 points apart
    ui.plot("log/roll").pinch_x(100.0, 300.0);
    let x_after = ui.plot("log/roll").x_range();
    let y_after = ui.plot("log/roll").y_range();
    assert!(
        span(x_after) < 0.5 * span(x_before),
        "time axis zoomed in: {x_before:?} → {x_after:?}"
    );
    assert!(
        (span(y_after) - span(y_before)).abs() < 1e-6 * span(y_before).max(1.0),
        "amplitude axis untouched: {y_before:?} → {y_after:?}"
    );
    // a finger held still on the plot places the cursor there (the harness
    // has no touch-to-pointer translation, so the resting finger is a
    // pointer press that does not move)
    assert_eq!(ui.app().viewer.cursor_s, None);
    let at = ui.plot("log/roll").x_mid() + 4.0 * ui.plot("log/roll").x_tolerance();
    ui.plot("log/roll").hold(at, 12);
    let cursor = ui
        .app()
        .viewer
        .cursor_s
        .expect("long press placed the cursor");
    assert!(
        (cursor - at).abs() <= ui.plot("log/roll").x_tolerance(),
        "cursor {cursor} at {at}"
    );
}

#[test]
fn one_axis_takes_the_full_height_on_every_axis_tab() {
    // docs/design/axis-focus (A1): "Show: all | roll | pitch | yaw" in the
    // panel and the keys R, P, Y; three axes by default
    let mut ui = Ui::open(&[LOG_2025], Tab::LogViewer);
    assert_eq!(ui.app().tools.axis_focus, None, "three axes by default");
    let three = ui.plot("log/pitch").height();
    // the key P: pitch alone, taller; the motors row stays
    ui.key(Key::P);
    assert_eq!(ui.app().tools.axis_focus, Some(Axis::Pitch));
    assert_eq!(rows(&ui), ["pitch", "motors"]);
    let alone = ui.plot("log/pitch").height();
    assert!(alone > 1.5 * three, "{alone} vs {three}");
    // P again: all three back
    ui.key(Key::P);
    assert_eq!(ui.app().tools.axis_focus, None);
    // the panel's buttons, and the choice carries over to the other tabs
    ui.click("yaw");
    assert_eq!(ui.app().tools.axis_focus, Some(Axis::Yaw));
    assert_eq!(ui.app().tools.axis, Axis::Yaw, "the selected axis follows");
    ui.tab(Tab::TimeHeatmap);
    assert_eq!(jobs(&ui, "stft"), 1, "one spectrogram for one axis");
    ui.tab(Tab::Spectrum);
    let one = ui.plot("spectrum/yaw").height();
    ui.click("all");
    assert_eq!(ui.app().tools.axis_focus, None);
    let all = ui.plot("spectrum/yaw").height();
    assert!(one > 2.0 * all, "{one} vs {all}");
    // the step tab's verdict table picks the axis too
    ui.tab(Tab::Step);
    ui.key(Key::R);
    assert_eq!(ui.app().tools.axis_focus, Some(Axis::Roll));
    assert_eq!(ui.app().tools.step.axis, Axis::Roll);
}

/// A log without flight-mode data has no marks, and asking for them is no error (#141).
#[test]
fn mode_changes_of_a_log_without_modes() {
    let mut ui = Ui::setup().open();
    ui.click("Open sample log");
    let session = ui.app().loaded(Slot::A).unwrap().current().unwrap().clone();
    assert!(
        session.mode_changes().is_empty(),
        "the sample logs no modes"
    );
    ui.check("Mode changes", true);
    assert_eq!(ui.app().viewer.mode_marks, 0);
    assert!(ui.app().viewer.mode_lines.is_none());
    ui.plot("log/roll").hover_middle();
    assert!(ui.app().viewer.mode_hover.is_none());
    assert!(
        ui.app().viewer.modes.is_none(),
        "no modes on the card either"
    );
}

/// Picks `level` in the list of the line smoothing; the harness leaves the
/// list open after a pick.
fn pick(ui: &mut Ui, level: LineSmoothing) {
    if ui.has(level.name()) {
        ui.click(level.name());
    } else {
        ui.choose_exact("Line smoothing", level.name());
    }
}

/// The line smoothing (D-011, D-015): the web edition's list Off, 2, 5, 10
/// and 20, off by default; a level smooths what the rows draw and the values
/// at the cursor, never the channels.
#[test]
fn line_smoothing_smooths_what_the_rows_draw() {
    at_every_size(|screen| {
        let mut ui = Ui::open_on(&[LOG_2025], Tab::LogViewer, screen);
        assert_eq!(ui.app().tools.log_display.smoothing, LineSmoothing::Off);
        assert!(ui.has("Line smoothing"));
        let gyro = |ui: &mut Ui| line_summary(ui.app_mut(), "roll", "gyro", 14.2).unwrap();
        let (off, off_at) = gyro(&mut ui);
        let session = ui.app().loaded(Slot::A).unwrap().current().unwrap().clone();
        let id = ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Filtered,
        };
        let logged = session.channel(&id).unwrap().data.clone();
        assert_eq!(off.as_slice(), logged.values(), "off: the lines as logged");
        // the list holds the five levels, each of which can be picked
        ui.choose_exact("Line smoothing", LineSmoothing::Two.name());
        for level in LineSmoothing::ALL {
            assert!(ui.has(level.name()), "{level:?} in the list");
        }
        for level in [2, 3, 0, 4].map(|k| LineSmoothing::ALL[k]) {
            pick(&mut ui, level);
            assert_eq!(ui.app().tools.log_display.smoothing, level);
        }
        assert_eq!(ui.app().tools.log_display.smoothing, LineSmoothing::Twenty);
        assert_eq!(
            LineSmoothing::ALL.map(|l| (l.name(), l.smoother().width())),
            [("Off", 1), ("2", 3), ("5", 5), ("10", 11), ("20", 21)]
        );
        // the rows drew their lines smoothed: gyro and setpoint on three
        // axes, the throttle and four motors
        assert_eq!(ui.app().viewer.smoothed.len(), 11);
        let (smooth, smooth_at) = gyro(&mut ui);
        let want = CutMovingAverage::new(21).smooth_range(
            logged.values(),
            |i| logged.is_valid(i),
            0,
            logged.len(),
        );
        assert_eq!(smooth.len(), want.len());
        assert!(smooth.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-9));
        assert!(
            smooth.iter().zip(&off).any(|(a, b)| (a - b).abs() > 1.0),
            "smoothed"
        );
        // the value at the cursor is the smoothed one
        let (off_at, smooth_at) = (off_at.unwrap(), smooth_at.unwrap());
        assert!(want.iter().any(|v| (v - smooth_at).abs() < 1e-9));
        assert!(logged.values().contains(&off_at));
        // the row was drawn at that level, and the log is as it was
        ui.plot("log/roll").click(14.2);
        assert!(ui.app().viewer.cursor_s.is_some());
        let now = ui.app().loaded(Slot::A).unwrap().current().unwrap().clone();
        assert_eq!(
            now.channel(&id).unwrap().data,
            logged,
            "the channel untouched"
        );
        // the motors row is smoothed too
        let motors = line_summary(ui.app_mut(), "motors", "M1", 14.2).unwrap().0;
        pick(&mut ui, LineSmoothing::Off);
        assert_eq!(ui.app().viewer.smoothed.len(), 0, "off frees the lines");
        let raw = line_summary(ui.app_mut(), "motors", "M1", 14.2).unwrap().0;
        assert_ne!(motors, raw);
    });
}

/// The period (D-011): with the toggle on a click places a second mark, the
/// time from the cursor to it is read out with its frequency, and the
/// toggle off removes the mark.
#[test]
fn period_places_a_second_mark_and_reads_out_the_time() {
    at_every_size(|screen| {
        let mut ui = Ui::open_on(&[LOG_2025], Tab::LogViewer, screen);
        assert!(!ui.app().viewer.period, "off by default");
        ui.check("Period", true);
        assert!(ui.sees("then the second mark"));
        // no cursor yet: the first click places it, the next one the mark
        let tol = ui.plot("log/roll").x_tolerance();
        ui.plot("log/roll").click(5.0);
        let cursor = ui.app().viewer.cursor_s.expect("the cursor");
        assert!((cursor - 5.0).abs() <= tol);
        assert_eq!(ui.app().viewer.mark_s, None);
        ui.plot("log/pitch").click(12.0);
        let mark = ui.app().viewer.mark_s.expect("the second mark");
        assert!((mark - 12.0).abs() <= tol);
        assert_eq!(ui.app().viewer.cursor_s, Some(cursor), "the cursor stays");
        // two known times read out
        ui.app_mut().viewer.cursor_s = Some(1.0);
        ui.app_mut().viewer.mark_s = Some(7.681);
        ui.settle();
        assert!(ui.sees("Δt = 6681 ms · 0.1497 Hz"));
        // off: the mark goes, and a click places the cursor again
        ui.check("Period", false);
        assert_eq!(ui.app().viewer.mark_s, None);
        assert!(!ui.sees("Δt"));
        ui.plot("log/roll").click(9.0);
        assert!((ui.app().viewer.cursor_s.unwrap() - 9.0).abs() <= tol);
        assert_eq!(ui.app().viewer.mark_s, None);
    });
}

#[test]
fn a_long_press_places_the_second_mark() {
    // a finger held still on a touch screen (held past the time of a click)
    let mut ui = Ui::open(&[LOG_4_2], Tab::LogViewer);
    ui.app_mut().touch_mode = true;
    ui.check("Period", true);
    ui.app_mut().viewer.cursor_s = Some(0.5);
    let at = ui.plot("log/roll").x_mid() + 4.0 * ui.plot("log/roll").x_tolerance();
    ui.plot("log/roll").hold(at, 60);
    let mark = ui.app().viewer.mark_s.expect("long press placed the mark");
    assert!((mark - at).abs() <= ui.plot("log/roll").x_tolerance());
    assert_eq!(ui.app().viewer.cursor_s, Some(0.5));
}

#[test]
fn the_period_read_out() {
    assert_eq!(period_text(6.681), "Δt = 6681 ms · 0.1497 Hz");
    assert_eq!(period_text(-6.681), "Δt = 6681 ms · 0.1497 Hz");
    assert_eq!(period_text(0.1), "Δt = 100 ms · 10.00 Hz");
    assert_eq!(period_text(0.0425), "Δt = 42.5 ms · 23.53 Hz");
    assert_eq!(period_text(0.000_05), "Δt = 0.1 ms · 20000 Hz");
    assert_eq!(period_text(0.000_123_45), "Δt = 0.1 ms · 8100 Hz");
    assert_eq!(period_text(1.0 / 9.999_96), "Δt = 100 ms · 10.00 Hz");
    assert_eq!(period_text(0.0), "Δt = 0.0 ms");
}
