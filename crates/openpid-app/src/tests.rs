//! Headless UI tests (ADR-0008). `egui_kittest` runs `App::frame` without a
//! window and finds widgets through the AccessKit tree; the worker thread is
//! real, so these cover file → worker → cache → tab end to end.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed lookup is the failure"
)]

use crate::app::{App, Cached, SlotState, Tab};
use crate::worker::{Done, Job, JobKey, Kind, Slot, Worker};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use openpid_analysis::{Epoch, StepParams};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const LOG_4_2: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-4.2.0.BFL"
);
const LOG_TWO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-4.2.11-two-logs.bbl"
);
const LOG_4_5: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-4.5.0-gps-rescue.bbl"
);
/// Two sessions; the longer one logs eRPM of four motors and debug `GYRO_SCALED`.
const LOG_4_5_1: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-4.5.1-two-sessions.bbl"
);
/// Logs `gyroUnfilt`, motor rpm and throttle: the Filter Sim input.
const LOG_2025: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/betaflight-2025.12.2-btfl_027.bbl"
);

/// The app is built lazily inside the ui closure so its worker wakes the
/// harness's own context.
type H = Harness<'static, Option<App>>;

fn harness(files: Vec<PathBuf>, tab: Option<Tab>) -> H {
    harness_with_prefs(files, tab, None)
}

/// A harness whose app persists its preferences to `prefs_path` (`None` = in memory).
fn harness_with_prefs(files: Vec<PathBuf>, tab: Option<Tab>, prefs_path: Option<PathBuf>) -> H {
    let mut init = Some((files, tab, prefs_path));
    Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .with_accessibility_check(true)
        .allow_missing_glyphs()
        .with_max_steps(500)
        .build_ui_state(
            move |ui, state: &mut Option<App>| {
                let app = state.get_or_insert_with(|| {
                    let (files, tab, prefs_path) = init.take().expect("built once");
                    App::with_prefs_path(ui.ctx(), files, tab, prefs_path)
                });
                app.frame(ui);
            },
            None,
        )
}

fn app(h: &H) -> &App {
    h.state().as_ref().expect("app built on first frame")
}

fn app_mut(h: &mut H) -> &mut App {
    h.state_mut().as_mut().expect("app built on first frame")
}

/// Steps frames until `cond` holds (the worker is asynchronous), then one more
/// so the widgets reflect the new state.
fn run_until(h: &mut H, mut cond: impl FnMut(&App) -> bool) {
    let t0 = Instant::now();
    while !cond(app(h)) {
        assert!(
            t0.elapsed() < Duration::from_secs(120),
            "timed out waiting for the worker"
        );
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.step();
}

fn no_slot_loading(a: &App) -> bool {
    !a.slots.values().any(|s| matches!(s, SlotState::Loading(_)))
}

fn idle(a: &App) -> bool {
    no_slot_loading(a) && !a.cache.values().any(|c| matches!(c, Cached::Pending))
}

fn select_tab(h: &mut H, tab: Tab) {
    h.get_by_label(tab.title()).click();
    h.step();
    assert_eq!(app(h).tab, tab);
}

fn has_label_containing(h: &H, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn drop_files(h: &mut H, paths: &[&str]) {
    for p in paths {
        h.input_mut()
            .dropped_files
            .push(Arc::new(Dropped(PathBuf::from(p))));
    }
    h.step();
}

// ------------------------------------------------------------------ tabs

#[test]
fn tab_names_and_titles() {
    for (name, tab) in [
        ("log", Tab::LogViewer),
        ("step", Tab::Step),
        ("spectrum", Tab::Spectrum),
        ("throttle", Tab::ThrottleHeatmap),
        ("time", Tab::TimeHeatmap),
        ("filter", Tab::FilterSim),
        ("sliders", Tab::Sliders),
        ("diagnostics", Tab::Diagnostics),
        ("settings", Tab::Settings),
    ] {
        assert_eq!(Tab::from_name(name), Some(tab));
    }
    assert_eq!(Tab::from_name("nope"), None);
    let titles: std::collections::BTreeSet<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
    assert_eq!(titles.len(), Tab::ALL.len(), "titles are unique");
}

#[test]
fn empty_slots_show_hints_on_every_tab() {
    let mut h = harness(Vec::new(), None);
    assert!(has_label_containing(&h, "Open a blackbox log"));
    for tab in Tab::ALL {
        select_tab(&mut h, tab);
        if tab == Tab::Settings {
            // the only tab without slots
            assert!(has_label_containing(&h, "Theme"), "{tab:?}");
            continue;
        }
        assert!(h.query_by_label("Slot A: empty").is_some(), "{tab:?}");
        assert!(h.query_by_label("Slot B: empty").is_some(), "{tab:?}");
        assert!(app(&h).cache.is_empty(), "nothing to analyse without a log");
        if tab == Tab::Sliders {
            assert!(has_label_containing(&h, "Load a Betaflight 4.3+ log"));
        }
    }
}

#[test]
fn command_line_gives_files_and_tab() {
    let args = |v: &[&str]| {
        v.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    };
    let (files, tab) = crate::parse_args(args(&["a.bbl", "--tab", "step", "b.bbl"]));
    assert_eq!(files, vec![PathBuf::from("a.bbl"), PathBuf::from("b.bbl")]);
    assert_eq!(tab, Some(Tab::Step));
    let (files, tab) = crate::parse_args(args(&["--tab", "bogus"]));
    assert!(files.is_empty());
    assert_eq!(tab, None);
    let (files, tab) = crate::parse_args(args(&["--tab"]));
    assert!(files.is_empty() && tab.is_none());
}

#[test]
fn starts_on_the_requested_tab() {
    let h = harness(Vec::new(), Some(Tab::Diagnostics));
    assert_eq!(app(&h).tab, Tab::Diagnostics);
}

// ------------------------------------------------------------------ loading

#[test]
fn loads_logs_into_both_slots() {
    let mut h = harness(vec![LOG_4_2.into(), LOG_TWO.into()], None);
    run_until(&mut h, no_slot_loading);
    let a = app(&h).loaded(Slot::A).expect("A ready");
    assert_eq!(a.sessions.len(), 1);
    let b = app(&h).loaded(Slot::B).expect("B ready");
    assert_eq!(b.sessions.len(), 2);
    let longest = (0..2)
        .max_by_key(|i| b.sessions[*i].len())
        .expect("two sessions");
    assert_eq!(b.session, longest, "the longest session is selected");
    assert!(app(&h).status.contains("session(s)"));
    assert!(has_label_containing(&h, "betaflight-4.2.0.BFL"));
    assert!(has_label_containing(&h, "Betaflight 4.2.0"));
    assert!(has_label_containing(&h, "Hz"));
    assert!(
        has_label_containing(&h, "session "),
        "slot B offers a session picker"
    );
}

#[test]
fn missing_and_garbage_files_fail_visibly() {
    let garbage =
        std::env::temp_dir().join(format!("openpid-gui-garbage-{}.bbl", std::process::id()));
    std::fs::write(&garbage, b"this is not a blackbox log at all").unwrap();
    let mut h = harness(
        vec![PathBuf::from("/definitely/not/here.bbl"), garbage.clone()],
        None,
    );
    run_until(&mut h, no_slot_loading);
    let a = &app(&h).slots[&Slot::A];
    assert!(
        matches!(a, SlotState::Failed(_, e) if e.contains("cannot read")),
        "{a:?}"
    );
    assert!(matches!(app(&h).slots[&Slot::B], SlotState::Failed(..)));
    assert!(has_label_containing(&h, "Slot A: /definitely/not/here.bbl"));
    assert!(has_label_containing(&h, "Slot B:"));
    let _ = std::fs::remove_file(garbage);
}

#[test]
fn dropped_files_fill_free_slots_then_replace() {
    let mut h = harness(Vec::new(), None);
    drop_files(&mut h, &[LOG_4_2]);
    run_until(&mut h, no_slot_loading);
    assert!(app(&h).loaded(Slot::A).is_some());
    assert!(app(&h).loaded(Slot::B).is_none());
    drop_files(&mut h, &[LOG_TWO]);
    run_until(&mut h, no_slot_loading);
    assert!(
        app(&h)
            .loaded(Slot::B)
            .unwrap()
            .path
            .ends_with("betaflight-4.2.11-two-logs.bbl"),
        "second drop goes to the free slot B"
    );
    drop_files(&mut h, &[LOG_TWO, LOG_4_2]);
    run_until(&mut h, no_slot_loading);
    assert!(
        app(&h)
            .loaded(Slot::A)
            .unwrap()
            .path
            .ends_with("betaflight-4.2.11-two-logs.bbl")
    );
    assert!(
        app(&h)
            .loaded(Slot::B)
            .unwrap()
            .path
            .ends_with("betaflight-4.2.0.BFL")
    );
}

// ------------------------------------------------------------------ analyses

#[test]
fn every_tab_runs_its_analysis_on_a_real_log() {
    let mut h = harness(vec![LOG_4_2.into()], None);
    run_until(&mut h, no_slot_loading);
    for tab in Tab::ALL {
        select_tab(&mut h, tab);
        run_until(&mut h, idle);
        let failed: Vec<&str> = app(&h)
            .cache
            .values()
            .filter_map(|c| match c {
                Cached::Failed(e) => Some(e.as_str()),
                _ => None,
            })
            .collect();
        assert!(failed.is_empty(), "{tab:?}: {failed:?}");
        assert!(!has_label_containing(&h, "A: error"), "{tab:?}");
    }
    assert!(has_label_containing(&h, "windows ·") || app(&h).cache.len() >= 4);
    let kinds: std::collections::BTreeSet<&str> = app(&h).cache.keys().map(|k| k.kind).collect();
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["fsim", "psd", "step", "stft", "thr"],
        "debug_mode GYRO_SCALED gives the 4.2.0 log a raw gyro, so Filter Sim runs too"
    );
    select_tab(&mut h, Tab::Diagnostics);
    assert!(has_label_containing(&h, "profile"));
    assert!(has_label_containing(&h, "samples"));
    assert!(has_label_containing(&h, "frames"));
}

#[test]
fn step_results_are_cached_per_parameters_and_epoch() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::Step));
    run_until(&mut h, idle);
    assert_eq!(app(&h).cache.len(), 1);
    assert!(has_label_containing(&h, "windows ·"));
    // the same parameters hit the cache: no new job
    h.run_steps(3);
    assert_eq!(app(&h).cache.len(), 1);
    // a different cutoff is a different job
    app_mut(&mut h).tools.step.cutoff_hz += 5.0;
    h.step();
    run_until(&mut h, idle);
    assert_eq!(app(&h).cache.len(), 2);
    // an epoch is part of the key
    app_mut(&mut h).loaded_mut(Slot::A).unwrap().epoch = Some(Epoch {
        start_s: 0.0,
        end_s: 1.0,
    });
    h.step();
    run_until(&mut h, idle);
    assert_eq!(app(&h).cache.len(), 3);
    // reloading a slot drops its cache
    app_mut(&mut h).open(Slot::A, LOG_4_2.into());
    assert!(app(&h).cache.is_empty());
    run_until(&mut h, idle);
}

#[test]
fn analysis_errors_are_shown_not_swallowed() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::Step));
    run_until(&mut h, no_slot_loading);
    app_mut(&mut h).tools.step.window_s = 0.0; // invalid
    h.step();
    run_until(&mut h, idle);
    assert!(
        app(&h)
            .cache
            .values()
            .any(|c| matches!(c, Cached::Failed(_))),
        "invalid parameters fail the job"
    );
    assert!(
        has_label_containing(&h, "A: "),
        "the error is drawn in the tab"
    );
}

#[test]
fn spectrum_and_heatmap_controls_change_parameters() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::Spectrum));
    run_until(&mut h, idle);
    assert!(app(&h).tools.psd_db);
    h.get_by_role_and_label(Role::CheckBox, "dB").click();
    h.step();
    assert!(!app(&h).tools.psd_db);
    h.get_by_label("pitch").click();
    h.step();
    assert_eq!(app(&h).tools.axis, openpid_core::Axis::Pitch);
    run_until(&mut h, idle);
    // automatic peaks: with a 3 dB prominence floor the gyro spectrum of a real log has some
    app_mut(&mut h).spectrum.min_prominence_db = 3.0;
    h.run_steps(2);
    let peaks = app(&h).spectrum.peaks.clone();
    assert!(
        peaks.iter().any(|(s, p)| *s == Slot::A && !p.is_empty()),
        "{peaks:?}"
    );
    assert!(has_label_containing(&h, "A top peak"));
    app_mut(&mut h).spectrum.auto_peaks = 0;
    h.run_steps(2);
    assert!(app(&h).spectrum.peaks.iter().all(|(_, p)| p.is_empty()));
    // manual markers: click the plot twice, read the delta, clear
    let frame = app(&h).spectrum.plot_frame.expect("psd plot drawn");
    let (x0, x1) = app(&h).spectrum.x_bounds.unwrap();
    let at = |f: f64| frame.min.x + ((f - x0) / (x1 - x0)) as f32 * frame.width();
    let y = frame.center().y;
    for f in [100.0, 150.0] {
        pointer_move(&mut h, egui::pos2(at(f), y));
        pointer_button(&mut h, egui::pos2(at(f), y), true);
        pointer_button(&mut h, egui::pos2(at(f), y), false);
        h.step();
    }
    let m = app(&h).spectrum.markers.clone();
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(
        (m[0] - 100.0).abs() < 5.0 && (m[1] - 150.0).abs() < 5.0,
        "{m:?}"
    );
    assert!(has_label_containing(&h, "Δ "));
    h.get_by_label("Clear markers").click();
    h.step();
    assert!(app(&h).spectrum.markers.is_empty());
    select_tab(&mut h, Tab::ThrottleHeatmap);
    run_until(&mut h, idle);
    assert!(has_label_containing(&h, "throttle window"));
    // segments per bin: the hint names the bins below the minimum
    let counts: Vec<usize> = app(&h)
        .cache
        .values()
        .find_map(|c| match c {
            Cached::Ready(o) => match o.as_ref() {
                crate::worker::Output::Throttle(t) => Some(t.counts.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("throttle heatmap computed");
    assert!(counts.iter().sum::<usize>() > 0);
    let min = app(&h).tools.min_segments;
    let below = counts.iter().filter(|&&c| c < min).count();
    assert!(has_label_containing(&h, "min segments"));
    if below > 0 {
        assert!(has_label_containing(
            &h,
            &format!("{below} of {} throttle bins", counts.len())
        ));
    } else {
        assert!(has_label_containing(&h, "every throttle bin"));
    }
    // a minimum above every count flags every bin; the analysis is not rerun
    let jobs = app(&h).cache.len();
    app_mut(&mut h).tools.min_segments = counts.iter().max().copied().unwrap_or(0) + 1;
    h.step();
    assert!(has_label_containing(
        &h,
        &format!("{} of {} throttle bins", counts.len(), counts.len())
    ));
    assert_eq!(app(&h).cache.len(), jobs);
    select_tab(&mut h, Tab::TimeHeatmap);
    run_until(&mut h, idle);
    assert!(has_label_containing(&h, "segment"));
}

// ------------------------------------------------------------------ log viewer

#[test]
fn cursor_buttons_set_the_epoch() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::LogViewer));
    run_until(&mut h, no_slot_loading);
    assert!(has_label_containing(&h, "Click or tap the plot"));
    assert!(h.query_by_label("A start = cursor").is_none());
    let duration = app(&h)
        .loaded(Slot::A)
        .unwrap()
        .current()
        .unwrap()
        .timebase
        .duration_s();
    app_mut(&mut h).viewer.cursor_s = Some(1.0);
    h.step();
    assert!(has_label_containing(&h, "cursor: 1.000 s"));
    h.get_by_label("A start = cursor").click();
    h.step();
    let e = app(&h).loaded(Slot::A).unwrap().epoch.unwrap();
    assert_eq!(e.start_s, 1.0);
    assert!((e.end_s - duration).abs() < 1e-9);
    app_mut(&mut h).viewer.cursor_s = Some(2.0);
    h.step();
    h.get_by_label("A end = cursor").click();
    h.step();
    let e = app(&h).loaded(Slot::A).unwrap().epoch.unwrap();
    assert_eq!((e.start_s, e.end_s), (1.0, 2.0));
    // end before start swaps the bounds rather than producing an empty epoch
    app_mut(&mut h).viewer.cursor_s = Some(0.5);
    h.step();
    h.get_by_label("A end = cursor").click();
    h.step();
    let e = app(&h).loaded(Slot::A).unwrap().epoch.unwrap();
    assert_eq!((e.start_s, e.end_s), (0.5, 1.0));
    h.get_by_label("whole log").click();
    h.step();
    assert!(app(&h).loaded(Slot::A).unwrap().epoch.is_none());
}

/// Screen x of time `t` on the last drawn log-viewer plot.
fn plot_x(h: &H, t: f64) -> f32 {
    let v = &app(h).viewer;
    let frame = v.plot_frame.expect("plot drawn");
    let (x0, x1) = v.x_bounds.expect("bounds known");
    frame.min.x + ((t - x0) / (x1 - x0)) as f32 * frame.width()
}

fn pointer_move(h: &mut H, pos: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(pos));
    h.step();
}

fn pointer_button(h: &mut H, pos: egui::Pos2, pressed: bool) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
}

#[test]
fn epoch_bounds_can_be_dragged_on_the_plot() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::LogViewer));
    run_until(&mut h, no_slot_loading);
    let duration = app(&h)
        .loaded(Slot::A)
        .unwrap()
        .current()
        .unwrap()
        .timebase
        .duration_s();
    let (start, end) = (duration * 0.2, duration * 0.6);
    app_mut(&mut h).loaded_mut(Slot::A).unwrap().epoch = Some(Epoch {
        start_s: start,
        end_s: end,
    });
    h.run_steps(2);
    let frame = app(&h).viewer.plot_frame.expect("plot drawn");
    let y = frame.center().y;
    let bounds_before = app(&h).viewer.x_bounds.unwrap();
    // grab the start bound and drag it to 40 % of the log
    let target = duration * 0.4;
    let x_start = plot_x(&h, start);
    let x_target = plot_x(&h, target);
    pointer_move(&mut h, egui::pos2(x_start, y));
    assert_eq!(
        app(&h).viewer.hover_handle,
        Some((Slot::A, true)),
        "hovering the start bound"
    );
    pointer_button(&mut h, egui::pos2(x_start, y), true);
    pointer_move(&mut h, egui::pos2(x_start + 12.0, y)); // past egui's drag threshold
    assert_eq!(app(&h).viewer.drag_handle, Some((Slot::A, true)));
    pointer_move(&mut h, egui::pos2(x_target, y));
    pointer_button(&mut h, egui::pos2(x_target, y), false);
    h.step();
    let e = app(&h).loaded(Slot::A).unwrap().epoch.unwrap();
    let px_s = (bounds_before.1 - bounds_before.0) / f64::from(frame.width());
    assert!(
        (e.start_s - target).abs() <= 2.0 * px_s,
        "start {} vs {target}",
        e.start_s
    );
    assert_eq!(e.end_s, end, "the other bound is untouched");
    assert_eq!(app(&h).viewer.drag_handle, None);
    assert_eq!(
        app(&h).viewer.x_bounds,
        Some(bounds_before),
        "the plot did not pan"
    );
    // dragging the start past the end collapses onto the end, never inverts
    let x_now = plot_x(&h, e.start_s);
    pointer_move(&mut h, egui::pos2(x_now, y));
    pointer_button(&mut h, egui::pos2(x_now, y), true);
    pointer_move(&mut h, egui::pos2(x_now + 12.0, y));
    let x_far = plot_x(&h, duration * 0.9);
    pointer_move(&mut h, egui::pos2(x_far, y));
    pointer_button(&mut h, egui::pos2(x_far, y), false);
    h.step();
    let e = app(&h).loaded(Slot::A).unwrap().epoch.unwrap();
    assert!(
        e.start_s <= e.end_s && (e.start_s - end).abs() <= 2.0 * px_s,
        "{e:?}"
    );
    // a drag that starts away from any bound pans the plot instead
    let x_mid = plot_x(&h, duration * 0.75);
    pointer_move(&mut h, egui::pos2(x_mid, y));
    assert_eq!(app(&h).viewer.hover_handle, None);
    pointer_button(&mut h, egui::pos2(x_mid, y), true);
    pointer_move(&mut h, egui::pos2(x_mid - 40.0, y));
    pointer_button(&mut h, egui::pos2(x_mid - 40.0, y), false);
    h.step();
    assert_eq!(
        app(&h).loaded(Slot::A).unwrap().epoch.unwrap(),
        e,
        "epoch unchanged by a pan"
    );
    assert_ne!(
        app(&h).viewer.x_bounds,
        Some(bounds_before),
        "the plot panned"
    );
}

#[test]
fn channel_filter_and_toggles_edit_the_channel_list() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::LogViewer));
    run_until(&mut h, no_slot_loading);
    let first = app(&h).tools.viewer_channels[0].key();
    assert!(h.query_by_label(&first).is_some(), "toggle for {first}");
    h.get_by_label(&first).click();
    h.step();
    assert!(
        !app(&h)
            .tools
            .viewer_channels
            .iter()
            .any(|c| c.key() == first)
    );
    h.get_by_label(&first).click();
    h.step();
    assert!(
        app(&h)
            .tools
            .viewer_channels
            .iter()
            .any(|c| c.key() == first)
    );
    let filter = h.get_by_role_and_label(Role::TextInput, "filter");
    filter.focus();
    filter.type_text("thr");
    h.step();
    assert_eq!(app(&h).viewer.channel_filter, "thr");
    assert!(h.query_by_label(&first).is_none(), "filtered out: {first}");
    assert!(h.query_by_label("throttle").is_some());
}

#[test]
fn session_picker_switches_session_and_clears_epoch() {
    let mut h = harness(vec![LOG_TWO.into()], Some(Tab::Diagnostics));
    run_until(&mut h, no_slot_loading);
    let current = app(&h).loaded(Slot::A).unwrap().session;
    let other = 1 - current;
    app_mut(&mut h).loaded_mut(Slot::A).unwrap().epoch = Some(Epoch {
        start_s: 0.0,
        end_s: 0.5,
    });
    h.step();
    // the picker is named by its label; the status bar also says "session N"
    h.get_by_role_and_label(Role::ComboBox, "session").click();
    h.run_steps(2);
    h.get_by_label_contains(&format!("session {other} ("))
        .click();
    h.run_steps(2);
    let l = app(&h).loaded(Slot::A).unwrap();
    assert_eq!(l.session, other);
    assert!(l.epoch.is_none(), "switching session resets the epoch");
}

// ------------------------------------------------------------------ sliders

#[test]
fn sliders_tab_edits_resets_and_prints_cli() {
    let mut h = harness(vec![LOG_4_2.into()], Some(Tab::Sliders));
    run_until(&mut h, no_slot_loading);
    assert!(
        has_label_containing(&h, "no simplified-tuning headers"),
        "4.2 has no sliders"
    );
    // a 4.5.0 log: master 75, D 105, pitch 105/105, D-term filter slider 125, gyro slider off
    app_mut(&mut h).open(Slot::A, LOG_4_5.into());
    run_until(&mut h, no_slot_loading);
    assert!(has_label_containing(&h, "betaflight-simplified-tuning"));
    assert!(has_label_containing(&h, "Master multiplier"));
    assert!(has_label_containing(&h, "from sliders"));
    assert!(has_label_containing(&h, "dterm lpf1 93–187 Hz"));
    assert!(
        !has_label_containing(&h, "gyro lpf1"),
        "gyro filter sliders are off in this log"
    );
    assert_eq!(
        app(&h).slider_state.values["simplified_d_gain"],
        105.0,
        "sliders start at the logged positions"
    );
    assert_eq!(
        app(&h).slider_state.values["simplified_master_multiplier"],
        75.0
    );
    assert!(!has_label_containing(&h, "⟵ changed"));
    assert_eq!(h.get_all_by_role(Role::Slider).count(), 12);
    // every slider is named by its label
    h.get_by_role_and_label(Role::Slider, "Master multiplier")
        .set_value_accesskit(150.0);
    h.step();
    assert_eq!(
        app(&h).slider_state.values["simplified_master_multiplier"],
        150.0
    );
    assert!(has_label_containing(&h, "⟵ changed"));
    h.get_by_label("Reset to log").click();
    h.step();
    assert_eq!(
        app(&h).slider_state.values["simplified_master_multiplier"],
        75.0
    );
    assert_eq!(app(&h).slider_state.values["simplified_d_gain"], 105.0);
    h.get_by_label("Firmware defaults").click();
    h.step();
    assert_eq!(app(&h).slider_state.values["simplified_d_gain"], 100.0);
    assert_eq!(
        app(&h).slider_state.values["simplified_master_multiplier"],
        100.0
    );
    let cli = h.get_by_role(Role::MultilineTextInput).value().unwrap();
    assert!(cli.contains("set simplified_pids_mode = 2"));
    assert!(cli.contains("simplified_tuning apply"));
    assert!(cli.ends_with("save"));
    // switching slot A to another file re-seeds the slider state
    app_mut(&mut h).open(Slot::A, LOG_TWO.into());
    run_until(&mut h, no_slot_loading);
    assert!(has_label_containing(&h, "no simplified-tuning headers"));
}

// ------------------------------------------------------------------ touch mode

#[test]
fn touch_mode_checkbox_and_touch_events_scale_the_ui() {
    let mut h = harness(Vec::new(), None);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    h.get_by_label("Touch mode").click();
    h.step();
    assert!(app(&h).touch_mode);
    h.run_steps(2); // applied at the start of the next frame, zoom one frame later
    assert_eq!(h.ctx.zoom_factor(), 1.25);
    h.get_by_label("Touch mode").click();
    h.run_steps(3);
    assert!(!app(&h).touch_mode);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    h.input_mut().events.push(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(0),
        phase: egui::TouchPhase::Start,
        pos: egui::pos2(10.0, 10.0),
        force: None,
    });
    h.step();
    assert!(app(&h).touch_mode, "a touch switches touch mode on");
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.25);
}

// ------------------------------------------------------------------ worker

fn wait_for(w: &Worker) -> Done {
    let t0 = Instant::now();
    loop {
        if let Some(d) = w.poll().pop() {
            return d;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "worker timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn worker_loads_and_analyses_off_thread() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = {
        let woken = woken.clone();
        Worker::spawn(move || {
            woken.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
    };
    w.submit(Job::Load {
        slot: Slot::A,
        path: LOG_4_2.into(),
    });
    let Done::Loaded { slot, path, result } = wait_for(&w) else {
        panic!("expected a load result");
    };
    assert_eq!(slot, Slot::A);
    assert!(path.ends_with("betaflight-4.2.0.BFL"));
    let file = result.expect("decodes");
    assert_eq!(file.source.path.as_deref(), Some(Path::new(LOG_4_2)));
    let session = Arc::new(file.sessions[0].clone());
    let kind = Kind::Step(StepParams::default());
    let key = JobKey::new(Slot::A, 0, &kind, None);
    w.submit(Job::Analyze {
        key: key.clone(),
        session: session.clone(),
        kind: kind.clone(),
        epoch: None,
    });
    let Done::Analyzed { key: k, result } = wait_for(&w) else {
        panic!("expected an analysis result");
    };
    assert_eq!(k, key);
    assert!(matches!(
        *result.expect("step runs"),
        crate::worker::Output::Step(_)
    ));
    w.submit(Job::Analyze {
        key: key.clone(),
        session,
        kind: Kind::Step(StepParams {
            window_s: 0.0,
            ..StepParams::default()
        }),
        epoch: None,
    });
    let Done::Analyzed { result, .. } = wait_for(&w) else {
        panic!("expected an analysis result");
    };
    assert!(
        result.is_err(),
        "invalid params come back as an error string"
    );
    w.submit(Job::Load {
        slot: Slot::B,
        path: "/nowhere/missing.bbl".into(),
    });
    let Done::Loaded { result, .. } = wait_for(&w) else {
        panic!("expected a load result");
    };
    assert!(result.unwrap_err().contains("cannot read"));
    assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[test]
fn job_keys_identify_params_and_epoch() {
    let a = JobKey::new(Slot::A, 0, &Kind::Step(StepParams::default()), None);
    let b = JobKey::new(Slot::A, 0, &Kind::Step(StepParams::default()), None);
    assert_eq!(a, b);
    assert_eq!(a.kind, "step");
    let epoch = Some(Epoch {
        start_s: 0.0,
        end_s: 1.0,
    });
    assert_ne!(
        a,
        JobKey::new(Slot::A, 0, &Kind::Step(StepParams::default()), epoch)
    );
    assert_ne!(
        a,
        JobKey::new(Slot::B, 0, &Kind::Step(StepParams::default()), None)
    );
    assert_ne!(
        a,
        JobKey::new(Slot::A, 1, &Kind::Step(StepParams::default()), None)
    );
    assert_ne!(
        a,
        JobKey::new(
            Slot::A,
            0,
            &Kind::Psd(openpid_analysis::PsdParams::default()),
            None
        )
    );
    for (kind, id) in [
        (Kind::Psd(openpid_analysis::PsdParams::default()), "psd"),
        (
            Kind::Throttle(openpid_analysis::ThrottleHeatmapParams::default()),
            "thr",
        ),
        (Kind::Stft(openpid_analysis::StftParams::default()), "stft"),
    ] {
        assert_eq!(kind.id(), id);
    }
    assert_eq!(Slot::A.name(), "A");
    assert_eq!(Slot::B.name(), "B");
}

#[test]
fn settings_tab_persists_preferences() {
    use crate::prefs::{AccUnit, Prefs, RateUnit, Theme};
    use crate::views::display_unit;
    use openpid_core::Unit;

    let dir = std::env::temp_dir().join(format!("openpid-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("prefs.json");
    // previous run left touch mode on: it is applied at start
    Prefs {
        touch_mode: true,
        ..Prefs::default()
    }
    .save(&path)
    .unwrap();

    let mut h = harness_with_prefs(vec![], Some(Tab::Settings), Some(path.clone()));
    assert!(app(&h).touch_mode, "touch mode restored from the file");
    assert!(h.ctx.global_style().visuals.dark_mode, "dark by default");
    assert!(has_label_containing(&h, "Saved in"));

    h.get_by_label("Light").click();
    h.step();
    h.step();
    assert_eq!(app(&h).prefs.theme, Theme::Light);
    assert!(
        !h.ctx.global_style().visuals.dark_mode,
        "theme applied immediately"
    );
    assert_eq!(Prefs::load(&path).theme, Theme::Light, "saved on change");

    h.get_by_label("rad/s").click();
    h.step();
    h.get_by_label("m/s²").click();
    h.step();
    let saved = Prefs::load(&path);
    assert_eq!(saved.rate_unit, RateUnit::RadPerS);
    assert_eq!(saved.acc_unit, AccUnit::MPerS2);
    let (k, sym) = display_unit(&Unit::DegreesPerSecond, &app(&h).prefs);
    assert!((k - std::f64::consts::PI / 180.0).abs() < 1e-12);
    assert_eq!(sym, "rad/s");
    let (k, sym) = display_unit(&Unit::Gs, &app(&h).prefs);
    assert!((k - 9.806_65).abs() < 1e-9);
    assert_eq!(sym, "m/s²");
    assert_eq!(
        display_unit(&Unit::Percent, &app(&h).prefs),
        (1.0, "%".to_string()),
        "other units pass through"
    );

    h.get_by_label("Touch mode (larger controls, touch gestures)")
        .click();
    h.step();
    assert!(
        !app(&h).touch_mode,
        "the settings checkbox drives the live mode"
    );
    assert!(!Prefs::load(&path).touch_mode);

    h.get_by_label("Reset to defaults").click();
    h.step();
    h.step();
    assert_eq!(app(&h).prefs, Prefs::default());
    assert_eq!(Prefs::load(&path), Prefs::default());
    assert!(h.ctx.global_style().visuals.dark_mode, "theme reverted");

    // a fresh app reads the file back
    let h2 = harness_with_prefs(vec![], Some(Tab::Settings), Some(path.clone()));
    assert_eq!(app(&h2).prefs, Prefs::default());
    drop(h2);
    drop(h);

    // no config directory: nothing is written, the hint says so
    let h3 = harness_with_prefs(vec![], Some(Tab::Settings), None);
    assert!(has_label_containing(&h3, "No configuration directory"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn log_viewer_uses_display_units() {
    let dir = std::env::temp_dir().join(format!("openpid-units-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("prefs.json");
    crate::prefs::Prefs {
        rate_unit: crate::prefs::RateUnit::RadPerS,
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let mut h = harness_with_prefs(vec![LOG_4_2.into()], Some(Tab::LogViewer), Some(path));
    run_until(&mut h, no_slot_loading);
    h.step();
    assert!(app(&h).loaded(Slot::A).is_some());
    assert_eq!(app(&h).prefs.rate_unit, crate::prefs::RateUnit::RadPerS);
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------ filter sim

#[test]
fn filter_sim_tab_validates_the_model_and_applies_what_if_changes() {
    let mut h = harness(vec![LOG_2025.into()], Some(Tab::FilterSim));
    run_until(&mut h, idle);
    assert_eq!(app(&h).cache.len(), 1);
    assert!(has_label_containing(&h, "fit: residual"));
    assert!(has_label_containing(&h, "RPM notch: 3 harmonics"));
    assert!(has_label_containing(&h, "drives LPF1"));
    assert!(
        h.query_by_label("Reset to log").is_none(),
        "nothing to reset yet"
    );
    let fit = app(&h)
        .cache
        .values()
        .find_map(|c| match c {
            Cached::Ready(o) => match o.as_ref() {
                crate::worker::Output::FilterSim(f) => f.fit,
                _ => None,
            },
            _ => None,
        })
        .expect("filter sim ran");
    assert!(fit.residual_rel < 0.25 && fit.residual_rel < fit.unfiltered_rel / 2.0);
    // a what-if cutoff is a new job and changes the description and the spectrum
    app_mut(&mut h)
        .tools
        .filter_sim
        .overrides
        .set("lpf1_hz", 90.0)
        .unwrap();
    h.step();
    run_until(&mut h, idle);
    assert_eq!(app(&h).cache.len(), 2);
    assert!(has_label_containing(&h, "lowpass 1: PT1 90 Hz"));
    let changed = app(&h)
        .cache
        .values()
        .find_map(|c| match c {
            Cached::Ready(o) => match o.as_ref() {
                crate::worker::Output::FilterSim(f) if f.whatif != f.model => Some(f),
                _ => None,
            },
            _ => None,
        })
        .expect("what-if result");
    let hi = changed.freq.partition_point(|f| *f < 300.0);
    assert!(
        changed.psd_sim_db[hi..].iter().sum::<f64>()
            < changed.psd_as_flown_db[hi..].iter().sum::<f64>(),
        "a 90 Hz low-pass removes more above 300 Hz than the chain as flown"
    );
    h.get_by_label("Reset to log").click();
    h.step();
    assert!(app(&h).tools.filter_sim.overrides.is_empty());
    run_until(&mut h, idle);
    assert_eq!(
        app(&h).cache.len(),
        2,
        "the as-flown result is still cached"
    );
    // the editor shows the log's values and turns a field into an override on edit
    assert!(has_label_containing(&h, "harmonics"));
    // a log without unfiltered gyro gets a hint and no job
    let mut h2 = harness(vec![LOG_TWO.into()], Some(Tab::FilterSim));
    run_until(&mut h2, no_slot_loading);
    h2.step();
    assert!(has_label_containing(&h2, "no unfiltered gyro"));
    assert!(app(&h2).cache.is_empty());
}

// ------------------------------------------------------------------ rpm overlay

#[test]
fn rpm_overlay_on_the_throttle_heatmap_has_per_motor_toggles() {
    let mut h = harness(vec![LOG_4_5_1.into()], Some(Tab::ThrottleHeatmap));
    run_until(&mut h, idle);
    let overlay = app(&h)
        .cache
        .values()
        .find_map(|c| match c {
            Cached::Ready(o) => match o.as_ref() {
                crate::worker::Output::Throttle(t) => t.overlay.clone(),
                _ => None,
            },
            _ => None,
        })
        .expect("the 4.5.1 log has motor rpm");
    assert_eq!(overlay.motors.len(), 4);
    assert_eq!(overlay.harmonics, 3, "rpm_filter_harmonics 3");
    assert!(overlay.motors[0].iter().flatten().any(|r| *r > 1000.0));
    assert!(overlay.dyn_notch_hz.is_some());
    h.step();
    assert!(app(&h).overlay.show);
    assert_eq!(app(&h).overlay.motors, vec![true; 4]);
    h.get_by_role_and_label(Role::CheckBox, "M2").click();
    h.step();
    assert_eq!(app(&h).overlay.motors, vec![true, false, true, true]);
    h.get_by_role_and_label(Role::CheckBox, "RPM overlay")
        .click();
    h.step();
    assert!(!app(&h).overlay.show);
    h.get_by_role_and_label(Role::CheckBox, "notches").click();
    h.step();
    assert!(!app(&h).overlay.notches);
    assert_eq!(
        app(&h).cache.len(),
        1,
        "the overlay is drawn from the cached result"
    );
    // a log without rpm has no overlay controls
    let mut h2 = harness(vec![LOG_4_2.into()], Some(Tab::ThrottleHeatmap));
    run_until(&mut h2, idle);
    assert!(h2.query_by_label("RPM overlay").is_none());
}
