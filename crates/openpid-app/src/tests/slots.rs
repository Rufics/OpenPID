//! Logs in slots: loading, dropping, hiding, sessions, projects and the
//! log check.

use super::{LOG_4_2, LOG_4_5, LOG_4_5_1, LOG_2025, LOG_TWO, epoch, temp_dir};
use crate::app::{SlotState, Tab};
use crate::testkit::Ui;
use crate::worker::Slot;
use openpid_analysis::{Epoch, FitnessLevel};
use std::path::PathBuf;

#[test]
fn loads_logs_into_both_slots() {
    let mut ui = Ui::setup().files([LOG_4_2, LOG_TWO]).open();
    let a = ui.app().loaded(Slot::A).expect("A ready");
    assert_eq!(a.sessions.len(), 1);
    let b = ui.app().loaded(Slot::B).expect("B ready");
    assert_eq!(b.sessions.len(), 2);
    let longest = (0..2)
        .max_by_key(|i| b.sessions[*i].len())
        .expect("two sessions");
    assert_eq!(b.session, longest, "the longest session is selected");
    assert!(ui.app().status.contains("session(s)"));
    assert!(ui.sees("betaflight-4.2.0.BFL"), "chip A");
    // the dock shows the last log opened, B, with its session picker
    assert_eq!(ui.app().dock_slot(), Some(Slot::B));
    assert!(ui.sees("Betaflight 4.2.11"));
    assert!(ui.sees("Hz"));
    assert!(ui.sees("session "), "slot B offers a session picker");
    // a click on chip A brings A into the dock
    ui.click_containing("Slot A:");
    assert_eq!(ui.app().dock_slot(), Some(Slot::A));
    assert!(ui.sees("Betaflight 4.2.0"));
}

#[test]
fn missing_and_garbage_files_fail_visibly() {
    let dir = temp_dir("garbage");
    let garbage = dir.join("garbage.bbl");
    std::fs::write(&garbage, b"this is not a blackbox log at all").unwrap();
    let ui = Ui::setup()
        .files([PathBuf::from("/definitely/not/here.bbl"), garbage])
        .open();
    let a = ui.app().slot_state(Slot::A).unwrap();
    assert!(
        matches!(a, SlotState::Failed(_, e) if e.contains("cannot read")),
        "{a:?}"
    );
    assert!(matches!(
        ui.app().slot_state(Slot::B),
        Some(SlotState::Failed(..))
    ));
    assert!(ui.sees("Slot A: /definitely/not/here.bbl"));
    assert!(ui.sees("Slot B:"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dropped_files_fill_free_slots_then_add_new_ones() {
    let mut ui = Ui::setup().open();
    ui.drop_files(&[LOG_4_2]);
    assert!(ui.app().loaded(Slot::A).is_some());
    assert!(ui.app().loaded(Slot::B).is_none());
    ui.drop_files(&[LOG_TWO]);
    let path_of = |ui: &Ui, slot: Slot| ui.app().loaded(slot).unwrap().path.clone();
    assert!(
        path_of(&ui, Slot::B).ends_with("betaflight-4.2.11-two-logs.bbl"),
        "second drop goes to the free slot B"
    );
    // both full: two more files open two more slots, nothing is replaced
    ui.drop_files(&[LOG_4_5, LOG_4_5_1]);
    assert_eq!(
        ui.app().slot_ids(),
        vec![Slot(0), Slot(1), Slot(2), Slot(3)]
    );
    assert!(path_of(&ui, Slot::A).ends_with("betaflight-4.2.0.BFL"));
    assert!(path_of(&ui, Slot(2)).ends_with("betaflight-4.5.0-gps-rescue.bbl"));
    assert!(path_of(&ui, Slot(3)).ends_with("betaflight-4.5.1-two-sessions.bbl"));
    assert_eq!(Slot(2).name(), "C");
    assert_eq!(Slot(7).name(), "H");
}

#[test]
fn many_slots_have_colours_visibility_solo_and_stable_letters() {
    // three logs with enough stick input for a step response each
    let mut ui = Ui::open(&[LOG_4_2, LOG_2025, LOG_4_5_1], Tab::Step);
    assert_eq!(ui.app().loaded_slots(), vec![Slot(0), Slot(1), Slot(2)]);
    assert_eq!(ui.app().visible_slots().len(), 3);
    // three distinct colours, three step curves in the legend
    let colours: std::collections::BTreeSet<[u8; 4]> = ui
        .app()
        .loaded_slots()
        .iter()
        .map(|s| crate::views::slot_color(*s).to_array())
        .collect();
    assert_eq!(colours.len(), 3);
    for name in ["A mean", "B mean", "C mean"] {
        assert!(ui.sees(name), "{name}");
    }
    assert!(ui.has("Slot C"));
    // hide B: it stays loaded but leaves the plot
    ui.click_containing("Slot B:");
    ui.click("hide B");
    assert_eq!(ui.app().visible_slots(), vec![Slot(0), Slot(2)]);
    assert!(ui.app().loaded(Slot::B).is_some());
    assert!(!ui.sees("B mean"));
    // solo C, then solo C again brings everyone back
    ui.app_mut().solo(Slot(2));
    ui.settle();
    assert_eq!(ui.app().visible_slots(), vec![Slot(2)]);
    assert!(ui.sees("C mean") && !ui.sees("A mean"));
    ui.app_mut().solo(Slot(2));
    ui.settle();
    assert_eq!(ui.app().visible_slots().len(), 3);
    // the panel tabs lay the slots out side by side
    ui.tab(Tab::ThrottleHeatmap);
    assert!(ui.sees("C Gyro"));
    // closing B keeps C's letter; the next log reuses the free letter B
    ui.click("close B");
    assert_eq!(ui.app().slot_ids(), vec![Slot(0), Slot(2)]);
    assert!(ui.app().cache.keys().all(|k| k.slot != Slot::B));
    assert_eq!(ui.app_mut().free_slot(), Some(Slot::B));
    assert_eq!(ui.app().slot_ids(), vec![Slot(0), Slot(2), Slot(1)]);
    // the limit
    while ui.app_mut().add_slot().is_some() {}
    assert_eq!(ui.app().slots.len(), Slot::MAX);
    assert!(ui.app_mut().free_slot().is_some(), "empty slots are free");
    for s in ui.app().slot_ids() {
        if ui.app().loaded(s).is_none() {
            ui.app_mut().close_slot(s);
        }
    }
    // closing everything leaves an empty slot A behind
    for s in ui.app().loaded_slots() {
        ui.app_mut().close_slot(s);
    }
    ui.settle();
    assert_eq!(ui.app().slot_ids(), vec![Slot::A]);
    assert!(ui.app().start_screen_wanted(), "back to the start screen");
    assert!(ui.has("Open sample log"));
}

#[test]
fn session_picker_switches_session_and_clears_epoch() {
    let mut ui = Ui::open(&[LOG_TWO], Tab::Setup);
    let other = 1 - ui.app().loaded(Slot::A).unwrap().session;
    ui.app_mut().loaded_mut(Slot::A).unwrap().epoch = Some(Epoch {
        start_s: 0.0,
        end_s: 0.5,
    });
    ui.settle();
    ui.choose("session", &format!("session {other} ("));
    assert_eq!(ui.app().loaded(Slot::A).unwrap().session, other);
    assert!(epoch(&ui).is_none(), "switching session resets the epoch");
}

#[test]
fn log_check_badges_and_card_follow_the_session() {
    // 4.2.0: raw gyro logged, no eRPM, 17 s (a short-log warning), and only
    // gentle stick moves: no step window above the default 80 deg/s (the
    // overall level follows rate, duration and frames only)
    let mut ui = Ui::open(&[LOG_4_2], Tab::Setup);
    let f = ui
        .app()
        .loaded(Slot::A)
        .and_then(|l| l.fitness())
        .expect("fitness is computed on load")
        .clone();
    assert_eq!(f.overall, FitnessLevel::Warn);
    assert_eq!(f.level("sticks"), FitnessLevel::Fail);
    assert_eq!(f.level("duration"), FitnessLevel::Warn);
    assert_eq!(f.level("raw_gyro"), FitnessLevel::Ok);
    assert_eq!(f.level("erpm"), FitnessLevel::Warn);
    assert!(ui.sees("raw [ok]"), "badge in the slot header");
    assert!(ui.sees("rpm [!]"));
    assert!(ui.sees("sticks [x]"));
    assert!(ui.sees("A verdict [!]"), "the check's rows on Setup info");
    assert!(
        ui.sees("dshot_telemetry"),
        "the fix for the missing eRPM is shown"
    );
    assert!(ui.has("Copy A rpm: fix"));
    // the badges are in the header, so they show on every tab with slots
    ui.tab(Tab::Step);
    assert!(ui.sees("rpm [!]"));
    assert!(!ui.sees("A verdict"), "the rows are on Setup info only");

    // 4.5.1: the longest session is fine, session 0 is a short stub
    let mut ui = Ui::open(&[LOG_4_5_1], Tab::Setup);
    assert!(ui.sees("rpm [ok]"));
    let l = ui.app().loaded(Slot::A).unwrap();
    assert_eq!(l.fitness.len(), l.sessions.len(), "one check per session");
    assert_eq!(l.fitness[0].overall, FitnessLevel::Fail);
    assert_eq!(l.fitness[0].level("duration"), FitnessLevel::Fail);
    ui.choose("session", "session 0 (");
    assert_eq!(ui.app().loaded(Slot::A).unwrap().session, 0);
    assert!(ui.sees("A verdict [x]"));
}

#[test]
fn projects_keep_every_slot_and_read_version_1_files() {
    use openpid_analysis::project::{PROJECT_VERSION, Project};
    let dir = temp_dir("slots");
    let file = dir.join("three.openpid.json");
    let mut ui = Ui::open(&[LOG_4_2, LOG_TWO, LOG_4_5], Tab::Spectrum);
    ui.app_mut().set_visible(Slot::B, false);
    ui.app_mut().save_project(&file);
    let saved = Project::load(&file).unwrap();
    assert_eq!(saved.version, PROJECT_VERSION);
    assert_eq!(saved.slots.len(), 3);
    assert_eq!(saved.slots[1].id, Some(1));
    assert!(!saved.slots[1].visible && saved.slots[2].visible);
    // reopen: letters, visibility and order come back
    let ui = Ui::setup().files([file]).open();
    assert_eq!(ui.app().loaded_slots(), vec![Slot(0), Slot(1), Slot(2)]);
    assert_eq!(ui.app().visible_slots(), vec![Slot(0), Slot(2)]);
    // a version-1 file without letters: slots by position
    let v1 = dir.join("old.openpid.json");
    std::fs::write(
        &v1,
        format!(
            r#"{{"version":1,"slots":[{{"path":"{}","session":0}},{{"path":"{}","session":0}}],"tab":"step"}}"#,
            LOG_4_2.replace('\\', "/"),
            LOG_4_5.replace('\\', "/")
        ),
    )
    .unwrap();
    let ui = Ui::setup().files([v1]).open();
    assert_eq!(ui.app().loaded_slots(), vec![Slot::A, Slot::B]);
    assert_eq!(ui.app().tab, Tab::Step);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_file_saves_and_restores_the_view() {
    use openpid_analysis::Project;
    let dir = temp_dir("project");
    // the log lives beside the project file so its path is stored relative
    let log = dir.join("two.bbl");
    std::fs::copy(LOG_TWO, &log).unwrap();
    let mut ui = Ui::setup().files([log]).tab(Tab::Spectrum).open();
    assert_eq!(ui.app().loaded(Slot::A).unwrap().sessions.len(), 2);
    {
        let a = ui.app_mut();
        a.loaded_mut(Slot::A).unwrap().session = 0;
        // session 0 is the short one (~0.2 s): an epoch inside it
        a.loaded_mut(Slot::A).unwrap().epoch = Some(Epoch {
            start_s: 0.05,
            end_s: 0.15,
        });
        a.tools.step.cutoff_hz = 31.0;
        a.tools.psd.segment_s = 0.4;
        a.tools.throttle.smoothing = openpid_analysis::ThrottleSmoothing::MedHigh;
        a.tools.min_segments = 7;
        a.tools.filter_sim.overrides.set("lpf1_hz", 80.0).unwrap();
        a.touch_mode = true;
    }
    ui.settle();
    let file = dir.join("flight.openpid.json");
    ui.app_mut().save_project(&file);
    assert!(ui.app().status.contains("saved project"));
    let saved = Project::load(&file).unwrap();
    assert_eq!(saved.slots.len(), 1);
    assert_eq!(
        saved.slots[0].path,
        PathBuf::from("two.bbl"),
        "relative to the project"
    );
    assert_eq!(saved.slots[0].session, 0);
    assert_eq!(saved.tab, "spectrum");
    assert!(saved.touch_mode);
    assert_eq!(saved.step.cutoff_hz, 31.0);
    drop(ui);
    // a project on the command line restores everything once the log is in
    let ui = Ui::setup().files([file.clone()]).open();
    let a = ui.app();
    let l = a.loaded(Slot::A).expect("log reopened from the project");
    assert_eq!(l.path, dir.join("two.bbl"));
    assert_eq!(l.session, 0, "the project's session, not the longest");
    assert_eq!(
        l.epoch,
        Some(Epoch {
            start_s: 0.05,
            end_s: 0.15
        })
    );
    assert_eq!(a.tab, Tab::Spectrum);
    assert!(a.touch_mode);
    assert_eq!(a.tools.step.cutoff_hz, 31.0);
    assert_eq!(a.tools.psd.segment_s, 0.4);
    assert_eq!(
        a.tools.throttle.smoothing,
        openpid_analysis::ThrottleSmoothing::MedHigh
    );
    assert_eq!(a.tools.min_segments, 7);
    assert_eq!(a.tools.filter_sim.overrides.lpf1_hz, Some(80.0));
    assert!(a.loaded(Slot::B).is_none());
    assert_eq!(a.project(&file), saved, "round trip");
    // dropping a project file onto the window applies it too
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    ui.drop_files(&[file.to_str().unwrap()]);
    assert_eq!(ui.app().loaded(Slot::A).unwrap().path, dir.join("two.bbl"));
    assert_eq!(ui.app().tab, Tab::Spectrum);
    // a bad project file reports in the status bar and keeps the slots
    let bad = dir.join("bad.openpid.json");
    std::fs::write(&bad, "{nope").unwrap();
    ui.app_mut().open_project(bad);
    assert!(ui.app().status.contains("not a project file"));
    assert!(ui.app().loaded(Slot::A).is_some());
    ui.open_menu();
    assert!(ui.has("Project"), "in the top bar's menu");
    // a session index past the file falls back to the longest session
    let mut p = saved.clone();
    p.slots[0].session = 9;
    let file2 = dir.join("nine.openpid.json");
    p.save(&file2).unwrap();
    ui.app_mut().open_project(file2);
    ui.settle();
    assert_eq!(
        ui.app().loaded(Slot::A).unwrap().session,
        1,
        "longest of two"
    );
    let _ = std::fs::remove_dir_all(dir);
}
