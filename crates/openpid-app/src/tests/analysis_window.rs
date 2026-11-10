//! The analysis window (epoch): the automatic trim, the timeline strip, its
//! fields, the cursor buttons and the bounds on the Log viewer.

use super::{LOG_4_2, LOG_4_5_1, duration, epoch, set_epoch, shown};
use crate::app::Tab;
use crate::testkit::{Ui, at_every_size};
use crate::views::StripDrag;
use crate::worker::{Output, Slot};

fn step_windows(ui: &Ui) -> usize {
    match shown(ui).as_ref() {
        Output::Step(o) => o.windows_used,
        other => panic!("expected a step result, got {other:?}"),
    }
}

#[test]
fn a_log_opens_on_its_automatic_trim_and_all_restores_the_whole_log() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    let l = ui.app().loaded(Slot::A).unwrap();
    let auto = l.epoch.expect("the 4.2.0 log flies for 17 s");
    assert!(l.epoch_auto);
    assert!(auto.start_s > 1.0 && auto.end_s < 17.0, "{auto:?}");
    assert!(ui.sees("· auto"), "the badge says it was automatic");
    assert!(ui.has("A timeline"), "the strip is on the Step tab");
    let trimmed = step_windows(&ui);
    ui.click("All");
    let l = ui.app().loaded(Slot::A).unwrap();
    assert!(l.epoch.is_none() && !l.epoch_auto);
    assert!(!ui.sees("· auto"));
    assert!(
        step_windows(&ui) >= trimmed,
        "the whole log has at least as many windows"
    );
    // the Auto menu brings the trim back, "Most active 20 s" is not auto
    ui.click("Auto");
    ui.click("Trim take-off and landing");
    let l = ui.app().loaded(Slot::A).unwrap();
    assert_eq!(l.epoch, Some(auto));
    assert!(l.epoch_auto);
    ui.click("Auto");
    ui.click("Most active 20 s");
    assert!(
        ui.sees("log shorter than 20 s"),
        "17 s: {}",
        ui.app().status
    );
    // the strip is on every tab with slots, not on Settings
    for tab in Tab::ALL {
        ui.tab(tab);
        assert_eq!(ui.has("A timeline"), tab != Tab::Settings, "{tab:?}");
    }
}

#[test]
fn a_short_session_is_too_short_to_trim() {
    let mut ui = Ui::open(&[LOG_4_5_1], Tab::Diagnostics);
    // the longest session opened on its trim; session 0 is a 3 s stub
    assert!(epoch(&ui).is_some());
    ui.choose("session", "session 0 (");
    assert_eq!(ui.app().loaded(Slot::A).unwrap().session, 0);
    assert!(epoch(&ui).is_none(), "too short: the whole log");
    ui.click("Auto");
    ui.click("Trim take-off and landing");
    assert!(
        ui.app().status.contains("log too short to trim"),
        "{}",
        ui.app().status
    );
    assert!(epoch(&ui).is_none());
}

#[test]
fn cursor_buttons_set_the_epoch() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::LogViewer);
    assert!(ui.sees("Click or tap the plot"));
    assert!(!ui.has("start = cursor"));
    // the log opened on its automatic trim; start from the whole log
    ui.click("All");
    assert!(epoch(&ui).is_none());
    let duration = duration(&ui);
    let place_cursor = |ui: &mut Ui, t: f64| {
        ui.plot("log/roll").click(t);
        let at = ui.app().viewer.cursor_s.expect("cursor placed");
        assert!((at - t).abs() <= ui.plot("log/roll").x_tolerance(), "{at}");
        at
    };
    let first = place_cursor(&mut ui, 1.0);
    assert!(ui.has("Position"), "the cursor's time");
    ui.click("start = cursor");
    let e = epoch(&ui).unwrap();
    assert_eq!(e.start_s, first);
    assert!((e.end_s - duration).abs() < 1e-9);
    let second = place_cursor(&mut ui, 2.0);
    ui.click("end = cursor");
    let e = epoch(&ui).unwrap();
    assert_eq!((e.start_s, e.end_s), (first, second));
    // end before start swaps the bounds rather than producing an empty epoch
    let third = place_cursor(&mut ui, 0.5);
    ui.click("end = cursor");
    let e = epoch(&ui).unwrap();
    assert_eq!((e.start_s, e.end_s), (third, first));
    ui.click("All");
    assert!(epoch(&ui).is_none());
}

#[test]
fn epoch_bounds_drag_on_the_rows_and_a_drag_elsewhere_pans() {
    at_every_size(|screen| {
        let mut ui = Ui::open_on(&[LOG_4_2], Tab::LogViewer, screen);
        let duration = duration(&ui);
        let (start, end) = (duration * 0.2, duration * 0.6);
        set_epoch(&mut ui, Some((start, end)));
        let tolerance = ui.plot("log/roll").x_tolerance();
        let view = ui.plot("log/roll").x_range();

        // the pointer over a bound takes its handle
        ui.plot("log/roll").hover(start);
        assert_eq!(ui.app().viewer.hover_handle, Some((Slot::A, true)));

        // the start bound follows the drag; the end and the view stay
        let target = duration * 0.4;
        ui.plot("log/roll").grab(start, target);
        assert_eq!(ui.app().viewer.drag_handle, Some((Slot::A, true)));
        ui.plot("log/roll").release(target);
        assert_eq!(ui.app().viewer.drag_handle, None);
        let e = epoch(&ui).unwrap();
        assert!((e.start_s - target).abs() <= tolerance, "{e:?}");
        assert_eq!(e.end_s, end, "the other bound is untouched");
        assert_eq!(ui.plot("log/roll").x_range(), view, "the plot did not pan");

        // dragged past the end it stops there, never inverts
        ui.plot("log/roll").drag(e.start_s, duration * 0.9);
        let e = epoch(&ui).unwrap();
        assert!(
            e.start_s <= e.end_s && (e.start_s - end).abs() <= tolerance,
            "{e:?}"
        );

        // a drag that starts away from a bound pans the plot instead
        ui.plot("log/roll").hover(duration * 0.75);
        assert_eq!(ui.app().viewer.hover_handle, None);
        ui.plot("log/roll").drag(duration * 0.75, duration * 0.70);
        assert_eq!(epoch(&ui), Some(e), "epoch unchanged by a pan");
        assert_ne!(ui.plot("log/roll").x_range(), view, "the plot panned");
    });
}

#[test]
fn strip_handles_fields_and_the_log_viewer_share_the_epoch() {
    at_every_size(|screen| {
        let mut ui = Ui::open_on(&[LOG_4_2], Tab::Spectrum, screen);
        let duration = duration(&ui);
        ui.click("All");
        let tolerance = ui.plot("strip/A").x_tolerance();

        // drag the end handle from the end of the log to 60 %
        let target = duration * 0.6;
        ui.plot("strip/A").grab(duration, target);
        assert_eq!(ui.app().strip.drag, Some((Slot::A, StripDrag::End)));
        ui.plot("strip/A").release(target);
        let e = epoch(&ui).expect("set by the drag");
        assert!((e.end_s - target).abs() <= tolerance, "{e:?}");
        assert_eq!(e.start_s, 0.0);
        assert!(!ui.app().loaded(Slot::A).unwrap().epoch_auto);
        assert_eq!(ui.app().strip.drag, None);

        // a drag inside the window moves it, keeping its length
        let len = e.end_s - e.start_s;
        let by = duration * 0.2;
        ui.plot("strip/A").grab(len / 2.0, len / 2.0 + by);
        assert!(matches!(
            ui.app().strip.drag,
            Some((Slot::A, StripDrag::Move { .. }))
        ));
        ui.plot("strip/A").release(len / 2.0 + by);
        let moved = epoch(&ui).unwrap();
        assert!(
            (moved.end_s - moved.start_s - len).abs() < 1e-6,
            "{moved:?} keeps {len}"
        );
        assert!((moved.start_s - by).abs() <= tolerance, "{moved:?}");

        // the fields accept typed values, clamped to the session
        ui.set_first("start", 1.0);
        let e = epoch(&ui).unwrap();
        assert!((e.start_s - 1.0).abs() < 1e-9, "{e:?}");
        ui.set_first("end", duration + 100.0);
        let e = epoch(&ui).unwrap();
        assert!(
            (e.end_s - duration).abs() < 1e-9,
            "clamped to the log: {e:?}"
        );

        // the Log viewer shows the very same window: its start handle sits at 1 s
        ui.tab(Tab::LogViewer);
        ui.plot("log/roll").hover(1.0);
        assert_eq!(ui.app().viewer.hover_handle, Some((Slot::A, true)));

        // a double-click on the strip is the whole log
        ui.plot("strip/A").double_click(5.0);
        assert!(epoch(&ui).is_none());
    });
}
