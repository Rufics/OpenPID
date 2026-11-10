//! The shell around the tabs: the start screen, the keyboard, the menu,
//! touch mode, the tour, the language and the saved preferences.

use super::{LOG_4_2, LOG_4_5, LOG_4_5_1, LOG_2025, set_epoch, temp_dir};
use crate::app::{Dialog, Tab};
use crate::prefs::{Prefs, RecentFile};
use crate::shortcuts::{Action, keys_of, text_input_focused};
use crate::testkit::{DESK, Ui};
use crate::worker::Slot;
use eframe::egui::{self, Key, Modifiers};

#[test]
fn the_start_screen_replaces_every_tab_until_a_log_is_open() {
    let mut ui = Ui::setup().open();
    assert!(ui.sees("Open a blackbox log"));
    for tab in Tab::ALL {
        ui.tab(tab);
        if tab == Tab::Settings {
            // the only tab without slots
            assert!(ui.sees("Theme"), "{tab:?}");
            assert!(!ui.has("Open sample log"));
            continue;
        }
        assert!(ui.app().start_screen_wanted(), "{tab:?}");
        assert!(ui.has("Open sample log"), "{tab:?}");
        assert!(ui.has("Start the tour"), "{tab:?}");
        assert!(!ui.has("Slot A: empty"), "{tab:?}");
        assert!(
            ui.app().cache.is_empty(),
            "nothing to analyse without a log"
        );
    }
    assert!(!ui.has("Recent files"), "nothing opened yet");
}

#[test]
fn starts_on_the_requested_tab() {
    let ui = Ui::open(&[], Tab::Setup);
    assert_eq!(ui.app().tab, Tab::Setup);
}

#[test]
fn number_keys_ctrl_tab_and_the_help_window() {
    let mut ui = Ui::setup().open();
    ui.key(Key::Num2);
    assert_eq!(ui.app().tab, Tab::Step);
    ui.key(Key::Num0);
    assert_eq!(ui.app().tab, Tab::Settings, "0 is the 10th tab");
    ui.key_with(Modifiers::COMMAND, Key::Tab);
    assert_eq!(
        ui.app().tab,
        Tab::LogViewer,
        "wraps from the last tab to the first"
    );
    ui.key_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::Tab);
    assert_eq!(ui.app().tab, Tab::Settings);
    ui.key(Key::Num9);
    assert_eq!(ui.app().tab, Tab::Setup, "9 is the 9th tab");
    // the help window: ?, Esc, F1, the Help menu
    assert!(!ui.sees("Keyboard shortcuts"));
    ui.key(Key::Questionmark);
    assert!(ui.app().show_help);
    let prev = keys_of(ui.ctx(), Action::PrevTab);
    assert!(ui.sees(&prev), "generated from the table: {prev}");
    assert!(ui.sees("Tab 2: Step response"));
    ui.key(Key::Escape);
    assert!(!ui.app().show_help);
    ui.key(Key::F1);
    assert!(ui.app().show_help);
    ui.key(Key::Questionmark);
    assert!(!ui.app().show_help, "? toggles");
    ui.menu("Help");
    ui.click_containing("Keyboard shortcuts");
    assert!(ui.app().show_help);
    // Ctrl+T toggles touch mode
    let before = ui.app().touch_mode;
    ui.key_with(Modifiers::COMMAND, Key::T);
    assert_eq!(ui.app().touch_mode, !before);
}

#[test]
fn file_shortcuts_ask_for_dialogs_and_close_the_last_slot() {
    let mut ui = Ui::setup().files([LOG_4_2, LOG_4_5]).open();
    ui.app_mut().dialogs = false;
    assert_eq!(ui.app().loaded_slots(), vec![Slot::A, Slot::B]);
    ui.key_with(Modifiers::COMMAND, Key::O);
    assert_eq!(
        ui.app().dialog,
        Some(Dialog::OpenLog(Slot(2))),
        "first empty slot, here a new one"
    );
    ui.app_mut().dialog = None;
    ui.key_with(Modifiers::COMMAND, Key::S);
    assert_eq!(ui.app().dialog, Some(Dialog::SaveProject));
    ui.key_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    assert_eq!(ui.app().dialog, Some(Dialog::OpenProject));
    ui.key_with(Modifiers::COMMAND, Key::E);
    assert_eq!(ui.app().dialog, Some(Dialog::ExportPng));
    ui.app_mut().dialog = None;
    // the buttons go through the same dialog queue
    ui.menu("Open log");
    assert_eq!(ui.app().dialog, Some(Dialog::OpenLog(Slot(2))));
    ui.app_mut().dialog = None;
    // Ctrl+W closes the last opened slot; Ctrl+Shift+O opens after the last loaded one
    ui.key_with(Modifiers::COMMAND, Key::W);
    assert_eq!(ui.app().loaded_slots(), vec![Slot::A]);
    ui.key_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    assert_eq!(
        ui.app().dialog,
        Some(Dialog::OpenLog(Slot(2))),
        "the empty slot after A"
    );
    ui.app_mut().dialog = None;
    ui.app_mut().close_slot(Slot::A);
    ui.settle();
    ui.key_with(Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    assert_eq!(
        ui.app().dialog,
        Some(Dialog::OpenLog(Slot(2))),
        "nothing loaded: the first empty slot"
    );
    // Shift+A: every slot back to the whole log
    ui.app_mut().dialog = None;
    ui.app_mut().open(Slot(2), LOG_4_2.into());
    ui.settle();
    ui.app_mut().loaded_mut(Slot(2)).unwrap().epoch = Some(openpid_analysis::Epoch {
        start_s: 1.0,
        end_s: 2.0,
    });
    ui.settle();
    ui.key_with(Modifiers::SHIFT, Key::A);
    assert!(ui.app().loaded(Slot(2)).unwrap().epoch.is_none());
}

#[test]
fn single_keys_type_into_a_focused_field_instead_of_switching_tabs() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::TimeHeatmap);
    ui.edit("Speed");
    assert!(
        text_input_focused(ui.ctx()),
        "a clicked number field is a text field"
    );
    ui.key(Key::Num2);
    ui.send(egui::Event::Text("2".into()));
    assert_eq!(ui.app().tab, Tab::TimeHeatmap, "2 went into the field");
    ui.key(Key::Escape);
    assert!(!text_input_focused(ui.ctx()), "Esc leaves the field");
    ui.key(Key::Num2);
    assert_eq!(ui.app().tab, Tab::Step);
}

#[test]
fn touch_mode_checkbox_and_touch_events_scale_the_ui() {
    // the test is about switching touch mode on: it starts without it
    let mut ui = Ui::setup().screen(DESK).open();
    assert_eq!(ui.ctx().zoom_factor(), 1.0);
    ui.menu("Touch mode");
    assert!(ui.app().touch_mode);
    assert_eq!(ui.ctx().zoom_factor(), 1.25);
    ui.menu("Touch mode");
    assert!(!ui.app().touch_mode);
    assert_eq!(ui.ctx().zoom_factor(), 1.0);
    ui.send(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(0),
        phase: egui::TouchPhase::Start,
        pos: egui::pos2(10.0, 10.0),
        force: None,
    });
    assert!(ui.app().touch_mode, "a touch switches touch mode on");
    assert_eq!(ui.ctx().zoom_factor(), 1.25);
    assert_eq!(
        ui.ctx().global_style().spacing.interact_size.y,
        44.0,
        "44 pt touch targets"
    );
}

#[test]
fn the_shell_keeps_its_height_as_logs_are_added() {
    // docs/design/app-shell, R4: logs are chips in the one-line top bar and
    // the timeline sits in a fixed dock, so adding logs never moves the tab
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    let one = ui.app().central_rect;
    for path in [LOG_4_5_1, LOG_2025] {
        let slot = ui.app_mut().next_slot().expect("a free slot");
        ui.app_mut().open(slot, path.into());
    }
    ui.settle();
    assert_eq!(ui.app().loaded_slots().len(), 3);
    assert_eq!(ui.app().central_rect, one, "the tab area did not move");
    // three chips; the dock shows the last one opened, a click switches it
    for s in ["Slot A:", "Slot B:", "Slot C:"] {
        assert!(ui.sees(s), "{s}");
    }
    assert_eq!(ui.app().dock_slot(), Some(Slot(2)));
    ui.click_containing("Slot B:");
    assert_eq!(ui.app().dock_slot(), Some(Slot::B));
    assert!(ui.sees("Betaflight 4.5.1"));
    // the dock's eye hides B from the plots, the chip stays
    ui.click("hide B");
    assert!(!ui.app().visible_slots().contains(&Slot::B));
    ui.click("show B");
    assert!(ui.app().visible_slots().contains(&Slot::B));
    // closing the docked log hands the dock to the first log left
    ui.click("close B");
    assert_eq!(ui.app().dock_slot(), Some(Slot::A));
    assert_eq!(ui.app().central_rect, one);
}

#[test]
fn the_tour_starts_once_on_the_first_log_and_walks_the_tuning_loop() {
    use crate::tour::STEPS;
    let dir = temp_dir("tour");
    let path = dir.join("prefs.json");
    let mut ui = Ui::setup().prefs(&path).with_tour().open();
    // the start screen's sample log opens without a file and starts the tour
    ui.click("Open sample log");
    let l = ui
        .app()
        .loaded(Slot::A)
        .expect("the sample is a loaded slot");
    assert_eq!(l.sessions.len(), 1);
    assert!(
        l.epoch.is_some(),
        "the sample opens on its automatic trim too"
    );
    assert!(ui.app().tour.active);
    for (i, step) in STEPS.iter().enumerate() {
        assert_eq!(ui.app().tour.step, i);
        assert_eq!(ui.app().tab, step.tab, "step {i} shows its tab");
        assert!(ui.sees(&format!("Step {} of 7", i + 1)), "step {i}");
        // the tab's analyses ran, so anchors like the Advice CLI box exist
        let anchors = &ui.app().anchors;
        let anchor = anchors
            .get(step.anchor)
            .or_else(|| anchors.get(step.fallback))
            .copied()
            .unwrap_or_else(|| panic!("step {i}: anchor {} missing", step.anchor));
        let cutout = ui.app().tour.cutout.expect("cut-out drawn");
        // a wide widget (the CLI box) is cut at the screen edge
        let anchor = anchor.intersect(ui.ctx().content_rect());
        assert!(
            cutout.contains_rect(anchor),
            "step {i}: {cutout:?} holds {anchor:?}"
        );
        if i + 1 < STEPS.len() {
            ui.key(Key::ArrowRight);
        }
    }
    // Done ends the tour and remembers it
    ui.click("Done");
    assert!(!ui.app().tour.active);
    assert!(ui.app().prefs.tour_done);
    assert!(Prefs::load(&path).tour_done, "saved");
    // the sample log is underdamped: Advice proposes more D on roll
    ui.tab(Tab::Advice);
    assert!(ui.sees("pid.roll.d"), "the sample overshoots");
    assert!(
        Prefs::load(&path).recent.is_empty(),
        "the sample is not a recent file"
    );
    drop(ui);
    // a second app with these prefs opens a real log without the tour
    let mut ui = Ui::setup().files([LOG_4_2]).prefs(&path).with_tour().open();
    assert!(!ui.app().tour.active, "seen once");
    let recent = Prefs::load(&path).recent;
    assert_eq!(recent.len(), 1);
    assert!(recent[0].path.ends_with("betaflight-4.2.0.BFL"));
    assert_eq!(recent[0].opened.len(), 10, "YYYY-MM-DD");
    // Help -> Show tour restarts it; Back, Skip and Esc
    ui.menu("Help");
    ui.click("Show tour");
    assert!(ui.app().tour.active);
    ui.key(Key::ArrowRight);
    assert_eq!(ui.app().tour.step, 1);
    ui.click("Back");
    assert_eq!(ui.app().tour.step, 0);
    // the overlay swallows a pointer click meant for the UI below
    ui.press("Spectrum");
    assert_eq!(ui.app().tab, Tab::LogViewer, "still on the step's tab");
    ui.click("Skip");
    assert!(!ui.app().tour.active);
    ui.menu("Help");
    ui.click("Show tour");
    ui.key(Key::Escape);
    assert!(!ui.app().tour.active, "Esc skips");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recent_files_open_from_the_start_screen() {
    let dir = temp_dir("recent");
    let path = dir.join("prefs.json");
    Prefs {
        recent: vec![RecentFile {
            path: LOG_4_2.into(),
            opened: "2026-11-06".into(),
        }],
        ..Prefs::default()
    }
    .save(&path)
    .unwrap();
    let mut ui = Ui::setup().tab(Tab::Step).prefs(&path).open();
    assert!(ui.has("Recent files"));
    ui.click_containing("betaflight-4.2.0.BFL");
    assert!(ui.app().loaded(Slot::A).is_some());
    assert!(!ui.app().start_screen_wanted());
    assert!(!ui.app().tour.active, "the tests keep the tour off");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn settings_tab_persists_preferences() {
    use crate::prefs::{AccUnit, RateUnit, Theme};
    use crate::views::display_unit;
    use openpid_core::Unit;

    let dir = temp_dir("settings");
    let path = dir.join("prefs.json");
    // previous run left touch mode on: it is applied at start
    Prefs {
        touch_mode: true,
        ..Prefs::default()
    }
    .save(&path)
    .unwrap();

    let mut ui = Ui::setup().tab(Tab::Settings).prefs(&path).open();
    assert!(ui.app().touch_mode, "touch mode restored from the file");
    assert!(ui.ctx().global_style().visuals.dark_mode, "dark by default");
    assert!(ui.sees("Saved in"));

    ui.click("Light");
    assert_eq!(ui.app().prefs.theme, Theme::Light);
    assert!(
        !ui.ctx().global_style().visuals.dark_mode,
        "theme applied immediately"
    );
    assert_eq!(Prefs::load(&path).theme, Theme::Light, "saved on change");

    ui.click("rad/s");
    ui.click("m/s²");
    let saved = Prefs::load(&path);
    assert_eq!(saved.rate_unit, RateUnit::RadPerS);
    assert_eq!(saved.acc_unit, AccUnit::MPerS2);
    let (k, sym) = display_unit(&Unit::DegreesPerSecond, &ui.app().prefs);
    assert!((k - std::f64::consts::PI / 180.0).abs() < 1e-12);
    assert_eq!(sym, "rad/s");
    let (k, sym) = display_unit(&Unit::Gs, &ui.app().prefs);
    assert!((k - 9.806_65).abs() < 1e-9);
    assert_eq!(sym, "m/s²");
    assert_eq!(
        display_unit(&Unit::Percent, &ui.app().prefs),
        (1.0, "%".to_string()),
        "other units pass through"
    );

    ui.click("Touch mode (larger controls, touch gestures)");
    assert!(
        !ui.app().touch_mode,
        "the settings checkbox drives the live mode"
    );
    assert!(!Prefs::load(&path).touch_mode);

    ui.click("Reset to defaults");
    assert_eq!(ui.app().prefs, Prefs::default());
    assert_eq!(Prefs::load(&path), Prefs::default());
    assert!(ui.ctx().global_style().visuals.dark_mode, "theme reverted");
    drop(ui);

    // a fresh app reads the file back
    let ui = Ui::setup().tab(Tab::Settings).prefs(&path).open();
    assert_eq!(ui.app().prefs, Prefs::default());
    drop(ui);

    // no config directory: nothing is written, the hint says so
    let ui = Ui::setup().tab(Tab::Settings).open();
    assert!(ui.sees("No configuration directory"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn language_switch_translates_labels_and_persists() {
    let dir = temp_dir("lang");
    let path = dir.join("prefs.json");
    let mut ui = Ui::setup()
        .files([LOG_4_2])
        .tab(Tab::Settings)
        .prefs(&path)
        .open();
    assert!(ui.has("Settings"));
    assert!(ui.has("Appearance"));
    assert!(!ui.has("Налаштування"));
    ui.click("Українська");
    assert_eq!(ui.app().prefs.lang, crate::i18n::Lang::Ua);
    // tab titles, headings, labels, buttons and tooltips all follow
    assert!(ui.has("Налаштування"));
    assert!(ui.has("Вигляд"));
    assert!(ui.has("Скинути до типових"));
    assert!(!ui.has("Settings"));
    ui.open_menu();
    assert!(ui.has("Сенсорний режим"));
    ui.click("Перегляд логу");
    assert_eq!(ui.app().tab, Tab::LogViewer);
    assert!(ui.sees("Слот A"));
    assert!(!ui.has("Слот B: порожній"), "an empty slot has no chip");
    assert!(ui.has("Додати лог"));
    assert!(ui.sees("Інші канали"));
    // dynamic read-outs keep their units and numbers
    assert!(ui.sees(" s"));
    // the choice is saved and restored
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("\"lang\": \"ua\""), "{saved}");
    drop(ui);
    let mut ui = Ui::setup().tab(Tab::Settings).prefs(&path).open();
    assert!(ui.has("Налаштування"));
    ui.click("English");
    assert!(ui.has("Settings"));
    assert_eq!(ui.app().prefs.lang, crate::i18n::Lang::En);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn log_viewer_uses_display_units() {
    let dir = temp_dir("units");
    let path = dir.join("prefs.json");
    Prefs {
        rate_unit: crate::prefs::RateUnit::RadPerS,
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let mut ui = Ui::setup()
        .files([LOG_4_2])
        .tab(Tab::LogViewer)
        .prefs(&path)
        .open();
    assert_eq!(ui.app().prefs.rate_unit, crate::prefs::RateUnit::RadPerS);
    // ± 500 deg/s is drawn as ± 8.73 rad/s
    set_epoch(&mut ui, None);
    let (lo, hi) = ui.plot("log/roll").y_range();
    let k = std::f64::consts::PI / 180.0;
    assert!(
        (lo + 500.0 * k).abs() < 0.01 && (hi - 500.0 * k).abs() < 0.01,
        "{lo} {hi}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
