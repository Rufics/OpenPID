//! The tools on top of the analyses: Filter Sim, Advice, the PID sliders,
//! the export, and every tab on one real log.

use super::temp_dir;
use super::{LOG_4_2, LOG_4_5, LOG_2025, LOG_ARDUCOPTER, LOG_TWO, result, set_epoch};
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
    ui.tab(Tab::Diagnostics);
    assert!(ui.sees("profile"));
    assert!(ui.sees("samples"));
    assert!(ui.sees("frames"));
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
    let mut ui = Ui::open(&[LOG_4_2], Tab::Advice);
    // the advice below is about the whole log, not the automatic trim
    set_epoch(&mut ui, None);
    assert!(ui.has("Proposals"));
    assert!(ui.has("Evidence"));
    // the 4.2.0 log overshoots on roll and pitch: D proposals with rationale and CLI text
    assert!(ui.sees("pid.roll.d"), "roll D proposed");
    assert!(ui.sees("overshoots"));
    assert!(ui.sees("Betaflight CLI: commands to apply"));
    assert!(ui.text_box().contains("set d_roll = "));
    let cached = ui.app().cache.len();
    // a looser overshoot limit: recomputed, nothing left to propose on the PIDs
    ui.app_mut().tools.recommend.targets.overshoot_warn_pct = 90.0;
    ui.settle();
    assert_eq!(ui.app().cache.len(), cached + 1, "new parameters, new job");
    assert!(!ui.sees("pid.roll.d"));
    assert!(ui.sees("within the 90 % limit"));
    // the project round-trip keeps the thresholds
    let project = ui.app().project(Path::new("advice.openpid.json"));
    assert_eq!(project.recommend.targets.overshoot_warn_pct, 90.0);
}

/// The Advice tab speaks the log's firmware: gains at its own step, and no
/// Betaflight commands for a firmware whose profile has no exporter.
#[test]
fn advice_is_in_the_firmwares_own_terms() {
    let mut ui = Ui::open(&[LOG_ARDUCOPTER], Tab::Advice);
    set_epoch(&mut ui, None);
    assert!(ui.sees("pid.pitch.d"), "pitch D proposed");
    assert!(ui.sees("0.008 -> 0.0088"), "a gain of 0.0001 steps");
    assert!(ui.sees("no ready commands for this firmware"));
    assert!(!ui.sees("Betaflight"));
    assert_eq!(ui.count(Role::MultilineTextInput), 0, "no box of commands");
}

#[test]
fn sliders_tab_edits_resets_and_prints_cli() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::Sliders);
    assert!(
        ui.sees("no simplified-tuning headers"),
        "4.2 has no sliders"
    );
    // a 4.5.0 log: master 75, D 105, pitch 105/105, D-term filter slider 125, gyro slider off
    ui.app_mut().open(Slot::A, LOG_4_5.into());
    ui.settle();
    let value = |ui: &Ui, key: &str| ui.app().slider_state.values[key];
    assert!(ui.sees("betaflight-simplified-tuning"));
    assert!(ui.sees("Master multiplier"));
    assert!(ui.sees("from sliders"));
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
    assert!(!ui.sees("⟵ changed"));
    assert_eq!(ui.count(Role::Slider), 12);
    // every slider is named by its label
    ui.slide("Master multiplier", 150.0);
    assert_eq!(value(&ui, "simplified_master_multiplier"), 150.0);
    assert!(ui.sees("⟵ changed"));
    ui.click("Reset to log");
    assert_eq!(value(&ui, "simplified_master_multiplier"), 75.0);
    assert_eq!(value(&ui, "simplified_d_gain"), 105.0);
    ui.click("Firmware defaults");
    assert_eq!(value(&ui, "simplified_d_gain"), 100.0);
    assert_eq!(value(&ui, "simplified_master_multiplier"), 100.0);
    // the commands are the slider model's, under its own caption
    assert!(ui.sees("Betaflight CLI: commands to apply"));
    let cli = ui.text_box();
    assert!(cli.contains("set simplified_pids_mode = 2"));
    assert!(cli.contains("simplified_tuning apply"));
    assert!(cli.ends_with("save"));
    // switching slot A to another file re-seeds the slider state
    ui.app_mut().open(Slot::A, LOG_TWO.into());
    ui.settle();
    assert!(ui.sees("no simplified-tuning headers"));
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
