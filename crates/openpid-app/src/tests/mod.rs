//! Headless UI tests (ADR-0008). `egui_kittest` runs `App::frame` without a
//! window and finds widgets through the AccessKit tree; the worker thread is
//! real, so these cover file → worker → cache → tab end to end.
//!
//! They are written with the driver in `testkit.rs` (docs/design/ui-tests):
//! a test says what a user does, in control names and data units, and what
//! must hold; it does not say where a widget is, how large the window is or
//! how many frames an action takes. `OPENPID_UI_SCREEN=1024x640 cargo test`
//! runs them on another screen of `testkit::SCREENS`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed lookup is the failure"
)]

mod analysis_window;
mod layout;
mod log_viewer;
mod setup;
mod shell;
mod slots;
mod spectra;
mod step;
mod tools;
mod worker;

use crate::testkit::Ui;
use crate::worker::{Output, Slot};
use openpid_analysis::Epoch;
use std::path::PathBuf;
use std::sync::Arc;

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
/// `ArduCopter` 3.3: gains are fractions, and there is no CLI to export to.
const LOG_ARDUCOPTER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/logs/ardupilot-copter-3.3-dronekit.bin"
);

/// A fresh, empty directory for one test's files.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("openpid-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Length of slot A's selected session, seconds.
fn duration(ui: &Ui) -> f64 {
    ui.app()
        .loaded(Slot::A)
        .unwrap()
        .current()
        .unwrap()
        .timebase
        .duration_s()
}

/// Slot A's analysis window.
fn epoch(ui: &Ui) -> Option<Epoch> {
    ui.app().loaded(Slot::A).unwrap().epoch
}

/// Sets slot A's analysis window as the timeline does (not automatic).
fn set_epoch(ui: &mut Ui, window: Option<(f64, f64)>) {
    let epoch = window.map(|(start_s, end_s)| Epoch { start_s, end_s });
    ui.app_mut()
        .loaded_mut(Slot::A)
        .unwrap()
        .set_epoch(epoch, false);
    ui.settle();
}

/// How many cached jobs are of `kind`.
fn jobs(ui: &Ui, kind: &str) -> usize {
    ui.app().cache.keys().filter(|k| k.kind == kind).count()
}

/// The first finished result that `pick` accepts.
fn result<T>(ui: &Ui, pick: impl Fn(&Output) -> Option<T>) -> Option<T> {
    ui.app().cache.values().find_map(|c| match c {
        crate::app::Cached::Ready(o) => pick(o),
        _ => None,
    })
}

/// The result the current tab shows for slot A.
fn shown(ui: &Ui) -> Arc<Output> {
    ui.app()
        .current_result(Slot::A)
        .expect("the tab has a result for slot A")
}
