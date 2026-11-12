//! The Step response tab: the cache, errors, the verdicts and their
//! targets, and the axis rows with the control panel.

use super::{LOG_4_2, LOG_2025, jobs, set_epoch};
use crate::app::{Cached, Tab};
use crate::testkit::Ui;
use crate::worker::Slot;
use openpid_analysis::{TargetPreset, VerdictLevel};
use openpid_core::Axis;

#[test]
fn step_results_are_cached_per_parameters_and_epoch() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    // the verdict table judges all three axes, plus the Advice line's recommender
    assert_eq!(ui.app().cache.len(), 4);
    ui.click("Details");
    assert!(ui.has("windows"));
    // the same parameters hit the cache: no new job
    ui.settle();
    assert_eq!(ui.app().cache.len(), 4);
    // a different cutoff is a different job (per axis)
    let cutoff = ui.app().tools.step.cutoff_hz;
    ui.set("Cut-off", cutoff + 5.0);
    assert_eq!(ui.app().tools.step.cutoff_hz, cutoff + 5.0);
    assert_eq!(ui.app().cache.len(), 7);
    // an epoch is part of the key
    set_epoch(&mut ui, Some((0.0, 1.0)));
    assert_eq!(ui.app().cache.len(), 11);
    // reloading a slot drops its cache
    ui.app_mut().open(Slot::A, LOG_4_2.into());
    assert!(ui.app().cache.is_empty());
    ui.settle();
}

#[test]
fn analysis_errors_are_shown_not_swallowed() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    // no control lets a user type this: the field clamps it
    ui.app_mut().tools.step.window_s = 0.0;
    ui.settle();
    assert!(
        ui.app()
            .cache
            .values()
            .any(|c| matches!(c, Cached::Failed(_))),
        "invalid parameters fail the job"
    );
    assert!(ui.sees("A: "), "the error is drawn in the tab");
}

#[test]
fn step_tab_judges_three_axes_and_targets_change_badges_without_new_jobs() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    // the acceptance numbers are for the whole log
    ui.click("All");
    let rows = ui.app().step_view.rows.clone();
    assert_eq!(rows.len(), 3, "roll, pitch and yaw of slot A");
    let level = |axis: Axis, metric: &str| {
        rows.iter()
            .find(|(_, a, _)| *a == axis)
            .and_then(|(_, _, v)| v.iter().find(|x| x.metric == metric))
            .map(|x| x.level)
            .unwrap()
    };
    assert_eq!(level(Axis::Roll, "overshoot"), VerdictLevel::Fail, "35 %");
    assert_eq!(level(Axis::Pitch, "overshoot"), VerdictLevel::Fail, "37 %");
    assert_eq!(level(Axis::Yaw, "overshoot"), VerdictLevel::Warn, "25 %");
    assert_eq!(level(Axis::Roll, "windows"), VerdictLevel::Ok, "48 windows");
    // the table sits closed under the plots: a pill per log, then the table
    assert!(!ui.sees("35 % x"), "closed by default");
    assert!(ui.sees("A 3/3 axes"));
    ui.click("Details");
    assert!(ui.app().tools.step_display.details);
    assert!(ui.sees("35 % x"), "value and ASCII marker");
    assert!(ui.sees("25 % !"));
    assert!(ui.has("overshoot"), "column header");
    // the target band and the Advice line
    assert_eq!(ui.app().step_view.band, Some((120.0, 0.9, 1.1)));
    let advice = ui
        .app()
        .step_view
        .advice
        .clone()
        .expect("roll D is proposed");
    assert!(
        advice.contains("A roll D") && advice.contains("->"),
        "{advice}"
    );
    // a click on a row switches the plotted axis
    ui.click("A pitch");
    assert_eq!(ui.app().tools.axis, Axis::Pitch);
    assert_eq!(ui.app().tools.step.axis, Axis::Pitch);
    // other targets: the badges change, the step jobs do not
    let before = jobs(&ui, "step");
    ui.app_mut().tools.recommend.targets.overshoot_fail_pct = 20.0;
    ui.settle();
    assert!(ui.sees("25 % x"), "yaw fails a 20 % overshoot limit");
    assert_eq!(
        jobs(&ui, "step"),
        before,
        "verdicts are a pure function of the metrics"
    );
    // new targets are a new recommender job: the Advice line is back once it ran
    ui.click("Go to Advice");
    assert_eq!(ui.app().tab, Tab::Advice);
    // Settings: presets, custom edits, persisted in prefs
    ui.tab(Tab::Settings);
    assert!(ui.has("Rise max"), "the targets are in view, not folded");
    ui.click("Racing");
    assert_eq!(ui.app().prefs.targets, TargetPreset::Racing.targets());
    assert_eq!(
        ui.app().tools.recommend.targets,
        TargetPreset::Racing.targets()
    );
    assert!(!ui.has("Custom"));
    ui.set("Rise max", 33.0);
    assert_eq!(ui.app().prefs.targets.rise_ms_max, 33.0);
    assert!(ui.has("Custom"), "no preset matches any more");
}

#[test]
fn too_few_windows_grey_out_the_verdicts() {
    // the first 3 s of the 4.2.0 log give 7 step windows; with stricter
    // window targets that is unreliable
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    ui.app_mut().tools.recommend.targets.min_windows_weak = 10;
    ui.app_mut().tools.recommend.targets.min_windows_ok = 20;
    set_epoch(&mut ui, Some((0.0, 3.0)));
    let rows = ui.app().step_view.rows.clone();
    let roll = &rows.iter().find(|(_, a, _)| *a == Axis::Roll).unwrap().2;
    assert_eq!(roll[0].metric, "windows");
    assert_eq!(roll[0].level, VerdictLevel::Fail, "{:?}", roll[0]);
    assert!(
        roll[1..].iter().all(|v| v.level == VerdictLevel::Unknown),
        "{roll:?}"
    );
    ui.click("Details");
    assert!(ui.sees(" ?"), "grey question marks");
}

#[test]
fn step_tab_stacks_the_axes_and_the_panel_collapses() {
    // docs/design/axis-layout (R4): roll, pitch and yaw at once, the controls
    // in a right-hand panel that the button and Ctrl+. collapse
    let mut ui = Ui::open(&[LOG_2025], Tab::Step);
    assert!(ui.app().prefs.panel_open, "open by default");
    assert!(ui.has("Control panel"));
    // the three axes and the reference tool's label of each log
    for pid in ["45,80,30,40,120", "47,84,34,46,125", "45,80,0,0,120"] {
        assert!(ui.sees(pid), "P,I,D,Dm,F {pid}");
    }
    // Raw draws every window under the mean, in the slot's colour ramp
    assert!(!ui.app().tools.step_display.raw, "Raw off by default");
    ui.click("Raw responses");
    assert!(ui.app().tools.step_display.raw);
    ui.click("Raw responses");
    assert!(!ui.app().tools.step_display.raw);
    // collapse with the button: a strip with the reopen button stays inside the window
    ui.click("Hide the control panel");
    assert!(!ui.app().prefs.panel_open);
    let reopen = ui.rect_of("Show the control panel");
    let window = ui.ctx().content_rect();
    assert!(
        reopen.right() <= window.right(),
        "reopen button inside the window: {reopen:?} in {window:?}"
    );
    // and back with the shortcut
    ui.app_mut().perform(crate::shortcuts::Action::TogglePanel);
    ui.settle();
    assert!(ui.app().prefs.panel_open);
    assert!(ui.has("Hide the control panel"));
}

/// On an `ArduPilot` log the legend shows the gains as the firmware has
/// them (fractions), not rounded to Betaflight's integers.
#[test]
fn the_legend_shows_gains_at_the_firmwares_step() {
    let mut ui = Ui::open(&[super::LOG_ARDUCOPTER], Tab::Step);
    set_epoch(&mut ui, None);
    assert!(ui.sees("0.1288,0.1288,0.0072"), "roll P, I, D");
    assert!(!ui.sees("0,0,0,"), "not rounded to integers");
}

#[test]
fn the_step_tab_ships_with_the_division_and_draws_what_the_reference_tool_reads() {
    use crate::axis_layout::{LatencyShown, latency_values, peak_values};
    use crate::worker::{Kind, Output};
    use openpid_analysis::{StepMethod, StepParams};
    use std::sync::Arc;

    let mut ui = Ui::setup()
        .files([LOG_2025])
        .tab(Tab::Step)
        .shipped()
        .open();
    assert_eq!(ui.app().tools.step.method, StepMethod::Division);
    assert_eq!(ui.app().tools.recommend.step.method, StepMethod::Division);
    // the reference tool analyses a log without its first and last second,
    // from two samples in: the same stretch here
    set_epoch(&mut ui, Some((1.001, 38.6)));
    let result = |ui: &mut Ui, axis: Axis| -> (Slot, Arc<Output>) {
        let p = StepParams {
            axis,
            ..ui.app().tools.step.clone()
        };
        let out = ui.app_mut().result(Slot::A, Kind::Step(p));
        (Slot::A, out.expect("computed").expect("has a response"))
    };
    // (axis, windows / peak / SR latency the reference tool showed for this
    // log; DESIGN §6.2)
    for (axis, windows, peak, latency) in [
        (Axis::Roll, 78, 1.15, 22.0),
        (Axis::Pitch, 13, 1.12, 22.5),
        (Axis::Yaw, 30, 1.18, 12.3),
    ] {
        let shown = [result(&mut ui, axis)];
        let Output::Step(step) = shown[0].1.as_ref() else {
            panic!("a step result");
        };
        assert_eq!(step.method, StepMethod::Division);
        assert!(step.windows_used.abs_diff(windows) <= 1, "{axis:?}");
        // the Peak plot: the point and its bar
        let drawn = peak_values(&shown);
        assert_eq!(drawn.len(), 1);
        assert!(
            (drawn[0].1 - peak).abs() < 0.015,
            "{axis:?}: {}",
            drawn[0].1
        );
        assert_eq!(drawn[0].2, Some(step.spread.peak_sd));
        // the Latency plot shows the SR latency of the windows, not the
        // half-way time of the mean curve
        let drawn = latency_values(&shown, LatencyShown::HalfRise);
        assert!(
            (drawn[0].1 - latency).abs() < 0.5,
            "{axis:?}: {}",
            drawn[0].1
        );
        assert_eq!(drawn[0].1, step.spread.half_ms_mean);
        assert_eq!(drawn[0].2, Some(step.spread.half_ms_sd));
        let delay = latency_values(&shown, LatencyShown::Xcorr);
        assert_eq!(Some(delay[0].1), step.metrics.latency_ms);
        assert_eq!(delay[0].2, None);
    }
    // the panel of the division: its method and its smoothing, none of the
    // fields of the Wiener estimate
    assert!(ui.has("Method") && ui.has("Smoothing"));
    for field in [
        "Cut-off",
        "Min input",
        "High input",
        "Window",
        "Y correction",
    ] {
        assert!(!ui.has(field), "{field}");
    }
    let (_, plain) = result(&mut ui, Axis::Roll);
    ui.choose("Smoothing", "high");
    assert_eq!(ui.app().tools.step.smoothing, 3);
    let (_, smoothed) = result(&mut ui, Axis::Roll);
    let (Output::Step(a), Output::Step(b)) = (plain.as_ref(), smoothed.as_ref()) else {
        panic!("step results");
    };
    assert_ne!(a.mean, b.mean, "smoothing the gyro changes the response");
    // the other method brings its own fields, and another curve
    ui.choose("Method", "Wiener with cut-off");
    assert_eq!(ui.app().tools.step.method, StepMethod::Wiener);
    for field in [
        "Cut-off",
        "Min input",
        "High input",
        "Window",
        "Y correction",
    ] {
        assert!(ui.has(field), "{field}");
    }
    let (_, wiener) = result(&mut ui, Axis::Roll);
    let Output::Step(w) = wiener.as_ref() else {
        panic!("a step result");
    };
    assert_eq!(w.method, StepMethod::Wiener);
    assert_ne!(w.windows_used, a.windows_used);
    let drawn = latency_values(&[(Slot::A, Arc::clone(&wiener))], LatencyShown::HalfRise);
    assert_eq!(Some(drawn[0].1), w.metrics.delay_ms);
}
