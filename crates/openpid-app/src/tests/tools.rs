//! The tools on top of the analyses: Filter Sim, Advice, the PID sliders,
//! the export, and every tab on one real log.

use super::temp_dir;
use super::{
    LOG_4_2, LOG_4_5, LOG_4_5_1, LOG_2025, LOG_ARDUCOPTER, LOG_TWO, jobs, result, set_epoch,
};
use crate::app::{Cached, Tab};
use crate::testkit::Ui;
use crate::worker::{Output, Slot};
use eframe::egui::{self, accesskit::Role};
use std::path::Path;

#[test]
fn every_tab_runs_its_analysis_on_a_real_log() {
    let mut ui = Ui::setup().files([LOG_4_2]).open();
    for tab in Tab::ALL {
        ui.tab(tab);
        let failed: Vec<&str> = ui
            .app()
            .cache
            .values()
            .filter_map(|c| match c {
                Cached::Failed(e) => Some(e.as_str()),
                _ => None,
            })
            .collect();
        assert!(failed.is_empty(), "{tab:?}: {failed:?}");
        assert!(!ui.sees("A: error"), "{tab:?}");
    }
    let kinds: std::collections::BTreeSet<&str> = ui.app().cache.keys().map(|k| k.kind).collect();
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["advice", "fsim", "psd", "step", "stft", "thr"],
        "debug_mode GYRO_SCALED gives the 4.2.0 log a raw gyro, so Filter Sim runs too"
    );
    ui.tab(Tab::Setup);
    // the table shows the rows that fit the window, so the search brings
    // one into view whatever the screen
    ui.type_into("Search", "frames");
    assert!(ui.sees("Corrupt frames"));
    assert!(ui.sees("I:525"), "the frames by kind");
}

#[test]
fn filter_sim_tab_validates_the_model_and_applies_what_if_changes() {
    let mut ui = Ui::open(&[LOG_2025], Tab::FilterSim);
    assert_eq!(ui.app().cache.len(), 1);
    assert!(ui.sees("fit: residual"));
    assert!(ui.sees("RPM notch: 3 harmonics"));
    assert!(ui.sees("drives LPF1"));
    assert!(!ui.has("Reset to log"), "nothing to reset yet");
    let sim = result(&ui, |o| match o {
        Output::FilterSim(f) => Some(f.clone()),
        _ => None,
    })
    .expect("filter sim ran");
    let fit = sim
        .fit
        .expect("the log has a filtered gyro to compare with");
    assert!(fit.residual_rel < 0.25 && fit.residual_rel < fit.unfiltered_rel / 2.0);
    // the chain at the flight's typical moment: four motors, both low-passes,
    // the dynamic notch, and a delay at 100 Hz
    let response = &sim.response;
    assert_eq!(response.motor_hz.len(), 4);
    let labels: Vec<&str> = response.parts.iter().map(|p| p.label.as_str()).collect();
    assert!(labels[0].starts_with("RPM 3 × 4 motors"), "{labels:?}");
    assert!(labels.iter().any(|l| l.starts_with("LPF1 PT1")));
    assert!(labels.iter().any(|l| l.starts_with("Dyn notch")));
    assert!(response.delay_100hz_ms > 0.0);
    assert!(ui.sees("LPF2 PT1 500 Hz"), "the legend");
    assert!(ui.sees("delay at 100 Hz"));
    // a what-if cutoff is a new job and changes the description and the spectrum
    ui.app_mut()
        .tools
        .filter_sim
        .overrides
        .set("lpf1_hz", 90.0)
        .unwrap();
    ui.settle();
    assert_eq!(ui.app().cache.len(), 2);
    assert!(ui.sees("lowpass 1: PT1 90 Hz"));
    // the chain's legend follows: the what-if low-pass, the chain as flown dashed
    assert!(ui.sees("LPF1 PT1 90 Hz"));
    assert!(ui.sees("as flown (dashed)"));
    let changed = result(&ui, |o| match o {
        Output::FilterSim(f) if f.whatif != f.model => Some(f.clone()),
        _ => None,
    })
    .expect("what-if result");
    let hi = changed.freq.partition_point(|f| *f < 300.0);
    assert!(
        changed.psd_sim_db[hi..].iter().sum::<f64>()
            < changed.psd_as_flown_db[hi..].iter().sum::<f64>(),
        "a 90 Hz low-pass removes more above 300 Hz than the chain as flown"
    );
    ui.click("Reset to log");
    assert!(ui.app().tools.filter_sim.overrides.is_empty());
    assert_eq!(
        ui.app().cache.len(),
        2,
        "the as-flown result is still cached"
    );
    // the editor shows the log's values and turns a field into an override on edit
    assert!(ui.sees("Harmonics"));
    // a log without unfiltered gyro gets a hint and no job
    let ui = Ui::open(&[LOG_TWO], Tab::FilterSim);
    assert!(ui.sees("no unfiltered gyro"));
    assert!(ui.app().cache.is_empty());
}

#[test]
fn advice_tab_lists_proposals_evidence_and_cli() {
    // docs/design/advice-tab (A2)
    // the 4.2.0 log on its automatic trim: roll overshoots, so a card with
    // the setting in words, the change, the reason, the firmware's command
    let mut ui = Ui::open(&[LOG_4_2], Tab::Advice);
    assert!(ui.sees("1 change proposed"));
    assert!(ui.has("Roll D"));
    assert!(ui.sees("pid.roll.d"));
    assert!(ui.has("83 -> 91"), "from and to, read out as written");
    assert!(ui.sees("roll overshoots 40 % (limit 15 %)"));
    assert!(ui.has("set d_roll = 91"));
    assert!(ui.has("Copy A pid.roll.d"));
    assert!(ui.sees("90 %"), "how sure the rule is");
    // the other checks, each with what the rule concluded
    assert!(ui.sees("CHECKED"));
    assert!(ui.has("A Noise peak roll [ok]"));
    assert!(
        ui.has("A Overshoot pitch [!]"),
        "pitch overshoots too, but its D-term is noisy: no proposal, a warning"
    );
    assert!(ui.sees("lower the D-term filter before raising D"));
    assert!(ui.has("A Overshoot yaw [-]"), "not judged");
    assert!(ui.has("A Gyro low-pass [-]"));
    assert!(
        !ui.has("A Overshoot roll [!]"),
        "the check behind a proposal is the card"
    );
    // the whole log: the D-term is quieter over it, and pitch gets D too
    set_epoch(&mut ui, None);
    assert!(ui.sees("2 changes proposed"));
    assert!(ui.has("Pitch D"));
    assert!(ui.has("90 -> 99"));
    assert!(!ui.has("A Overshoot pitch [!]"));
    // the commands of every proposal together
    assert!(ui.sees("Betaflight CLI: commands to apply"));
    assert!(ui.text_box().contains("set d_roll = 91\n"));
    assert!(ui.text_box().ends_with("set d_pitch = 99\nsave\n"));
    ui.check("Checks without a proposal", false);
    assert!(!ui.sees("CHECKED"));
    assert!(!ui.has("A Noise peak roll [ok]"));
    assert!(ui.has("Roll D"), "the proposals stay");
    ui.check("Checks without a proposal", true);

    let cached = ui.app().cache.len();
    // a looser overshoot limit: recomputed, nothing left to propose on the PIDs
    ui.set("Overshoot limit", 90.0);
    assert_eq!(ui.app().tools.recommend.targets.overshoot_warn_pct, 90.0);
    assert_eq!(ui.app().cache.len(), cached + 1, "new parameters, new job");
    assert!(!ui.sees("pid.roll.d"));
    assert!(ui.sees("nothing to change"));
    assert!(ui.sees("within the 90 % limit"));
    assert!(ui.has("A Overshoot roll [ok]"));
    assert_eq!(ui.count(Role::MultilineTextInput), 0, "no commands");
    // the project round-trip keeps the thresholds
    let project = ui.app().project(Path::new("advice.openpid.json"));
    assert_eq!(project.recommend.targets.overshoot_warn_pct, 90.0);

    // the card leads to the tab its evidence is on
    ui.set("Overshoot limit", 15.0);
    ui.click("Show on Step response A pid.roll.d");
    assert_eq!(ui.app().tab, Tab::Step);
}

/// The Advice tab speaks the log's firmware: gains at its own step, and no
/// Betaflight commands for a firmware whose profile has no exporter.
#[test]
fn advice_is_in_the_firmwares_own_terms() {
    let mut ui = Ui::open(&[LOG_ARDUCOPTER], Tab::Advice);
    set_epoch(&mut ui, None);
    assert!(ui.sees("2 changes proposed"));
    assert!(ui.has("Pitch D"));
    assert!(ui.sees("pid.pitch.d"));
    assert!(ui.has("0.008 -> 0.0088"), "a gain of 0.0001 steps");
    assert!(ui.sees("no ready commands for this firmware"));
    assert!(!ui.sees("Betaflight"));
    assert!(!ui.has("Copy A pid.pitch.d"), "no command to copy");
    assert_eq!(ui.count(Role::MultilineTextInput), 0, "no box of commands");
}

/// Every visible log has its column of advice.
#[test]
fn advice_has_a_column_per_log() {
    let mut ui = Ui::open(&[LOG_4_2, LOG_4_5_1], Tab::Advice);
    assert!(ui.sees("A · betaflight-4.2.0"));
    assert!(ui.sees("B · betaflight-4.5.1-two-sessions"));
    assert!(ui.has("A Gyro low-pass [-]"));
    assert!(ui.has("B Gyro low-pass [-]"));
    assert_eq!(jobs(&ui, "advice"), 2);
    ui.click("hide B");
    assert!(!ui.sees("B · betaflight-4.5.1-two-sessions"));
}

#[test]
fn sliders_tab_tracks_every_log_and_gives_the_gains_and_commands() {
    // docs/design/sliders-tab (S3). A 4.5.0 log: master 75, D 105, pitch
    // 105/105, D-term filter slider 125, gyro slider off
    let mut ui = Ui::open(&[LOG_4_5], Tab::Sliders);
    let value = |ui: &Ui, key: &str| ui.app().slider_state.values[key];
    assert!(ui.sees("betaflight-simplified-tuning"));
    assert!(ui.has("Master multiplier"));
    assert!(ui.sees("from the sliders"));
    assert!(ui.sees("dterm lpf1 93–187 Hz"));
    assert!(
        !ui.sees("gyro lpf1"),
        "gyro filter sliders are off in this log"
    );
    assert_eq!(
        value(&ui, "simplified_d_gain"),
        105.0,
        "sliders start at the logged positions"
    );
    assert_eq!(value(&ui, "simplified_master_multiplier"), 75.0);
    assert!(ui.sees("nothing: the sliders are as flown"));
    // eight PID and two filter tracks; the mode and the switch are in the panel
    assert_eq!(ui.count(Role::Slider), 10);
    // a track is named by its label, and so is the number beside it
    ui.slide("Master multiplier", 150.0);
    assert_eq!(value(&ui, "simplified_master_multiplier"), 150.0);
    assert!(!ui.sees("nothing: the sliders are as flown"));
    ui.set("D gain", 90.0);
    assert_eq!(value(&ui, "simplified_d_gain"), 90.0);
    ui.choose_exact("PID mode", "RP");
    assert_eq!(value(&ui, "simplified_pids_mode"), 1.0);
    ui.check("D-term filter sliders", false);
    assert_eq!(value(&ui, "simplified_dterm_filter"), 0.0);
    assert!(
        !ui.sees("dterm lpf1"),
        "the D-term sliders no longer set it"
    );
    ui.click("Reset to log");
    assert_eq!(value(&ui, "simplified_master_multiplier"), 75.0);
    assert_eq!(value(&ui, "simplified_d_gain"), 105.0);
    assert_eq!(value(&ui, "simplified_pids_mode"), 2.0);
    ui.click("Firmware defaults");
    assert_eq!(value(&ui, "simplified_d_gain"), 100.0);
    assert_eq!(value(&ui, "simplified_master_multiplier"), 100.0);
    // the commands are the slider model's, under its own caption
    assert!(ui.sees("Betaflight CLI: commands to apply"));
    let cli = ui.text_box();
    assert!(cli.contains("set simplified_pids_mode = 2"));
    assert!(cli.contains("simplified_tuning apply"));
    assert!(cli.ends_with("save"));

    // a second log: the newest one is docked and edited, and every log's
    // as-flown position is marked on the same tracks, the docked log first
    ui.drop_files(&[LOG_2025]);
    assert_eq!(ui.app().dock_slot(), Some(Slot::B));
    assert_eq!(value(&ui, "simplified_master_multiplier"), 100.0);
    let marks = ui.app().slider_state.marks.clone();
    let master = &marks
        .iter()
        .find(|(key, _)| key == "simplified_master_multiplier")
        .expect("a track for the master multiplier")
        .1;
    assert_eq!(master, &vec![(Slot::B, 100.0), (Slot::A, 75.0)]);

    // a log whose firmware has no sliders of its own: ratio sliders over
    // the gains it flew with, and no commands
    let mut ui = Ui::open(&[LOG_4_2], Tab::Sliders);
    assert!(ui.sees("ratio-sliders"));
    assert_eq!(ui.count(Role::Slider), 5);
    assert!(!ui.has("Firmware defaults"), "its defaults are the log");
    assert!(ui.sees("type the result into its configurator"));
    assert_eq!(ui.count(Role::MultilineTextInput), 0);
    // roll P 77, I 119 at a master of 110 %, in Betaflight's whole numbers
    assert!(!ui.has("85") && !ui.has("131"));
    ui.slide("Master multiplier", 110.0);
    assert!(ui.has("85") && ui.has("131"));
}

/// On an `ArduPilot` log the ratio sliders keep the firmware's fractions.
#[test]
fn ratio_sliders_keep_the_firmwares_gain_step() {
    let mut ui = Ui::open(&[LOG_ARDUCOPTER], Tab::Sliders);
    assert!(ui.sees("ratio-sliders"));
    assert!(ui.has("0.1288"), "roll P as flown");
    ui.slide("Master multiplier", 120.0);
    assert!(ui.has("0.1546"), "roll P × 1.2 at the 0.0001 step");
}

#[test]
fn json_export_equals_the_cli_serialisation_and_names_the_file() {
    use openpid_analysis::{Analysis, AnalysisCtx, StepAnalysis};
    let mut ui = Ui::open(&[LOG_4_2], Tab::Step);
    // compared with the CLI on the whole log, so drop the automatic trim
    set_epoch(&mut ui, None);
    assert_eq!(
        ui.app().export_file_name(Slot::A, "json"),
        "betaflight-4.2.0-step-all.json"
    );
    assert_eq!(
        ui.app().export_file_name(Slot::B, "png"),
        "openpid-step-all.png"
    );
    let dir = temp_dir("export");
    let path = dir.join("step.json");
    ui.app().export_json(Slot::A, &path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let session = ui.app().loaded(Slot::A).unwrap().current().unwrap().clone();
    let ctx = AnalysisCtx {
        session: &session,
        epoch: None,
    };
    let cli = serde_json::to_string_pretty(&StepAnalysis.run(&ctx, &ui.app().tools.step).unwrap())
        .unwrap();
    assert_eq!(written, cli, "byte-identical to `openpid run step`");
    assert!(
        ui.app()
            .export_json(Slot::B, &path)
            .unwrap_err()
            .contains("nothing to export"),
        "empty slot"
    );
    set_epoch(&mut ui, Some((1.0, 2.5)));
    assert_eq!(
        ui.app().export_file_name(Slot::A, "json"),
        "betaflight-4.2.0-step-1.0-2.5.json"
    );
    ui.tab(Tab::LogViewer);
    assert!(ui.app().current_kind().is_none());
    assert!(ui.app().export_json(Slot::A, &path).is_err());
    // the Export menu is in the top bar's menu
    ui.open_menu();
    assert!(ui.has("Export"));
    // a cropped PNG of a synthetic frame
    let image = egui::ColorImage::filled([4, 3], egui::Color32::from_rgb(10, 20, 30));
    let png = dir.join("plot.png");
    crate::export::write_png(
        &image,
        Some(egui::Rect::from_min_max(
            egui::pos2(1.0, 1.0),
            egui::pos2(3.0, 3.0),
        )),
        1.0,
        &png,
    )
    .unwrap();
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    assert_eq!(&bytes[16..24], &[0, 0, 0, 2, 0, 0, 0, 2], "2 × 2 pixels");
    assert!(
        crate::export::write_png(
            &image,
            Some(egui::Rect::from_min_max(
                egui::pos2(10.0, 10.0),
                egui::pos2(12.0, 12.0)
            )),
            1.0,
            &png
        )
        .unwrap_err()
        .contains("empty")
    );
    let _ = std::fs::remove_dir_all(dir);
}
