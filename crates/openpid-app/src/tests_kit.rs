//! UI tests written with the driver in `testkit.rs` (the proof of concept
//! for docs/design/ui-tests): each says what a user does, in control names
//! and data units, and runs on every screen of `testkit::SCREENS`.
//!
//! The two tests here redo `log_tab_stacks_the_axes_with_the_motors_and_reads_the_cursor`
//! and `epoch_bounds_can_be_dragged_on_the_plot` of `tests.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed lookup is the failure"
)]

use crate::app::Tab;
use crate::log_tab::row_summary;
use crate::testkit::{Ui, at_every_size};
use crate::worker::Slot;
use eframe::egui::Key;
use openpid_analysis::Epoch;
use openpid_core::{Axis, FlightMode};

const LOG_4_2: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-4.2.0.BFL"
);
const LOG_2025: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-2025.12.2-btfl_027.bbl"
);

fn rows(ui: &Ui) -> Vec<&'static str> {
    row_summary(ui.app()).iter().map(|r| r.0).collect()
}

#[test]
fn log_viewer_rows_quantities_cursor_and_sticks() {
    at_every_size(|screen| {
        let mut ui = Ui::open(&[LOG_2025], Tab::LogViewer, screen);
        // the default: gyro and setpoint on every axis, throttle and motors under them
        assert_eq!(rows(&ui), ["roll", "pitch", "yaw", "motors"]);
        assert_eq!(ui.plot("log/roll").y_range(), (-500.0, 500.0));

        // a quantity adds a line on every axis; one the log lacks is greyed out
        ui.check("P-term", true);
        ui.check("Motor RPM", true);
        assert_eq!(row_summary(ui.app())[1].1, ["setpoint", "gyro", "P"]);
        assert_eq!(rows(&ui).last(), Some(&"rpm"));
        assert!(!ui.is_enabled("Debug"), "no debug in this log");

        // one scale for the three axes
        ui.set("Y scale ±", 250.0);
        assert_eq!(ui.plot("log/pitch").y_range(), (-250.0, 250.0));

        // a click places the cursor where it was made
        ui.plot("log/roll").click(14.2);
        let cursor = ui.app().viewer.cursor_s.expect("cursor placed");
        assert!(
            (cursor - 14.2).abs() <= ui.plot("log/roll").x_tolerance(),
            "{cursor}"
        );
        assert!(ui.has("Position"));

        // the sticks and the modes follow the pointer, then stay at the cursor
        ui.plot("log/yaw").hover(20.0);
        let modes = ui
            .app()
            .viewer
            .modes
            .clone()
            .expect("modes under the pointer");
        assert_eq!(modes, [FlightMode::Arm, FlightMode::PosHold]);
        ui.plot("log/yaw").leave();
        let at_cursor = ui
            .app()
            .loaded(Slot::A)
            .unwrap()
            .current()
            .unwrap()
            .sticks_at(cursor);
        assert_eq!(ui.app().viewer.sticks, at_cursor);

        // the last controls of a long panel work without scrolling to them
        ui.check("Sticks", false);
        ui.check("Flight modes", false);
        assert!(ui.app().viewer.sticks_rect.is_none());
        ui.check("Mode changes", true);
        assert!(ui.app().viewer.mode_marks >= 1);

        // one axis takes the height of the three; the same key brings them back
        let three = ui.plot("log/pitch").height();
        ui.key(Key::P);
        assert_eq!(ui.app().tools.axis_focus, Some(Axis::Pitch));
        // (the motors and rpm rows stay, so it is not quite three times)
        assert!(ui.plot("log/pitch").height() > 1.5 * three);
        ui.click("all");
        assert_eq!(ui.app().tools.axis_focus, None);
    });
}

#[test]
fn epoch_bounds_drag_on_the_rows_and_a_drag_elsewhere_pans() {
    at_every_size(|screen| {
        let mut ui = Ui::open(&[LOG_4_2], Tab::LogViewer, screen);
        let duration = ui
            .app()
            .loaded(Slot::A)
            .unwrap()
            .current()
            .unwrap()
            .timebase
            .duration_s();
        let (start, end) = (duration * 0.2, duration * 0.6);
        ui.app_mut().loaded_mut(Slot::A).unwrap().epoch = Some(Epoch {
            start_s: start,
            end_s: end,
        });
        ui.settle();
        let epoch = |ui: &Ui| ui.app().loaded(Slot::A).unwrap().epoch.unwrap();
        let tolerance = ui.plot("log/roll").x_tolerance();
        let view = ui.plot("log/roll").x_range();

        // the start bound follows the drag; the end and the view stay
        let target = duration * 0.4;
        ui.plot("log/roll").drag(start, target);
        assert!(
            (epoch(&ui).start_s - target).abs() <= tolerance,
            "{:?}",
            epoch(&ui)
        );
        assert_eq!(epoch(&ui).end_s, end);
        assert_eq!(ui.plot("log/roll").x_range(), view, "the plot did not pan");

        // dragged past the end it stops there, never inverts
        let from = epoch(&ui).start_s;
        ui.plot("log/roll").drag(from, duration * 0.9);
        assert!(
            (epoch(&ui).start_s - end).abs() <= tolerance,
            "{:?}",
            epoch(&ui)
        );

        // a drag that starts away from a bound pans the plot instead
        let before = epoch(&ui);
        ui.plot("log/roll").drag(duration * 0.75, duration * 0.70);
        assert_eq!(epoch(&ui), before);
        assert_ne!(ui.plot("log/roll").x_range(), view, "the plot panned");
    });
}
