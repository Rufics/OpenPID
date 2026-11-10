//! Advice (docs/design/advice-tab, A2): per visible log, a card per proposal
//! of the recommender (the setting in words, from → to, how sure, why, the
//! firmware's command to copy, and the tab with the evidence), then every
//! other check of the rules with its mark, then the commands of all the
//! proposals together. The thresholds of the rules are in the panel.

use crate::app::{App, Loaded, Tab};
use crate::axis_layout::control_panel;
use crate::colormap::{ACCENT, MUTED};
use crate::i18n::tr;
use crate::panel;
use crate::views::slot_color;
use crate::views::{SlotResult, apply_label, level_color, paint_mark, panel_width, show_error};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, RichText, Ui};
use openpid_analysis::{Finding, FindingStatus, FitnessLevel, RecommendOutput};
use openpid_core::{Exporter, Proposal};

/// Width of the drawn arrow between a proposal's values, with its gaps.
const ARROW: f32 = 26.0;
/// Width of the column with a check's rule and axis.
const RULE_W: f32 = 124.0;

/// Display options of the tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdviceDisplay {
    /// The checks that proposed nothing, under the proposals.
    pub checks: bool,
}

impl Default for AdviceDisplay {
    fn default() -> Self {
        AdviceDisplay { checks: true }
    }
}

/// The tab.
pub fn advice(app: &mut App, ui: &mut Ui) {
    control_panel(app, ui, "advice-panel", controls);
    ui.separator();
    ui.label(
        RichText::new(tr(
            "Rules are hints with their evidence, not an autotune: apply one change, fly, compare the new log.",
        ))
        .color(MUTED),
    );
    let params = app.tools.recommend.clone();
    let slots: Vec<Slot> = app.visible_slots();
    let results: Vec<SlotResult> = slots
        .iter()
        .map(|slot| (*slot, app.result(*slot, Kind::Recommend(params.clone()))))
        .collect();
    let w = panel_width(ui, slots.len());
    let mut cli_anchor: Option<egui::Rect> = None;
    let mut go: Option<Tab> = None;
    egui::ScrollArea::both()
        .id_salt("advice-panels")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (slot, result) in results {
                    ui.vertical(|ui| {
                        ui.set_width(w - 12.0);
                        match result {
                            Some(Ok(o)) => {
                                if let Output::Recommend(r) = o.as_ref() {
                                    let drawn = column(app, ui, slot, r);
                                    cli_anchor = cli_anchor.or(drawn.commands);
                                    go = go.or(drawn.go);
                                }
                            }
                            Some(Err(e)) => show_error(ui, slot, &e),
                            None => {
                                ui.spinner();
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        });
    if let Some(rect) = cli_anchor {
        app.anchors.insert("advice-cli", rect);
    }
    if let Some(tab) = go {
        app.tab = tab;
    }
}

/// The panel: the thresholds of the rules, and what the columns list.
fn controls(app: &mut App, ui: &mut Ui) {
    let r = &mut app.tools.recommend;
    panel::section(ui, tr("Rules"));
    panel::drag(
        ui,
        tr("Overshoot limit"),
        tr(
            "an axis that overshoots more gets more D; the same target as on the Step tab (Settings, Targets)",
        ),
        &mut r.targets.overshoot_warn_pct,
        1.0,
        1.0..=100.0,
        "%",
    );
    panel::drag(
        ui,
        tr("D step"),
        tr("how much D is raised in one proposal; small steps keep the motors cool"),
        &mut r.d_step_pct,
        1.0,
        1.0..=50.0,
        "%",
    );
    panel::drag(
        ui,
        tr("Peak prominence"),
        tr("how far a noise peak must stand above the spectrum around it to count"),
        &mut r.peak_min_prominence_db,
        0.5,
        1.0..=40.0,
        "dB",
    );
    panel::drag(
        ui,
        tr("LPF headroom"),
        tr(
            "how much quieter the band above the gyro low-pass must be than the low band before its cut-off is raised",
        ),
        &mut r.lpf_headroom_db,
        0.5,
        1.0..=40.0,
        "dB",
    );
    panel::section(ui, tr("Show"));
    panel::check(
        ui,
        &mut app.tools.advice_display.checks,
        tr("Checks without a proposal"),
        tr("under the proposals, what every other rule saw: satisfied, or not judged and why"),
    );
}

/// What a column drew that the tab needs.
#[derive(Default)]
struct Drawn {
    /// Where the box of commands is (the tour points at it).
    commands: Option<egui::Rect>,
    /// The tab a "Show on …" button asked for.
    go: Option<Tab>,
}

/// The tab that shows what a proposal was read from.
fn evidence_tab(p: &Proposal) -> Tab {
    if p.path.starts_with("pid.") {
        Tab::Step
    } else {
        Tab::Spectrum
    }
}

/// One log: its title, the proposals, the other checks, the commands.
fn column(app: &App, ui: &mut Ui, slot: Slot, r: &RecommendOutput) -> Drawn {
    let mut drawn = Drawn::default();
    let Some(loaded) = app.loaded(slot) else {
        return drawn;
    };
    let Some(session) = loaded.current() else {
        return drawn;
    };
    title(ui, slot, loaded, r.proposals.len());
    let exporter = crate::worker::formats().exporter(session);
    if r.proposals.is_empty() {
        crate::theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(tr(
                    "nothing to change: every rule that could be judged is satisfied",
                ))
                .color(MUTED),
            );
        });
    }
    for p in &r.proposals {
        ui.add_space(2.0);
        if let Some(tab) = card(ui, slot, p, exporter.as_deref()) {
            drawn.go = Some(tab);
        }
    }
    if app.tools.advice_display.checks {
        let rest: Vec<&Finding> = r.findings.iter().filter(|f| !f.proposed()).collect();
        if !rest.is_empty() {
            panel::section(ui, tr("Checked"));
            for f in rest {
                check_row(ui, slot, f);
            }
        }
    }
    if !r.proposals.is_empty() {
        ui.add_space(8.0);
        // the firmware profile of the log writes the commands; a firmware
        // without one gets the proposals as they are
        let Some(exporter) = exporter else {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    session.firmware.revision,
                    tr("no ready commands for this firmware; set the values by hand")
                ))
                .color(MUTED),
            );
            return drawn;
        };
        let l = ui.label(apply_label(exporter.label()));
        let mut cli = match exporter.export(&r.proposals) {
            Ok(text) => text,
            Err(e) => format!("# {e}"),
        };
        let rows = cli.lines().count().clamp(3, 12);
        let edit = ui.add(
            egui::TextEdit::multiline(&mut cli)
                .code_editor()
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .interactive(true),
        );
        drawn.commands = Some(edit.rect);
        edit.labelled_by(l.id);
    }
    drawn
}

/// "A · file    1 change proposed".
fn title(ui: &mut Ui, slot: Slot, loaded: &Loaded, proposals: usize) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(r.center(), 4.0, slot_color(slot));
        let file = loaded
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        ui.add(
            egui::Label::new(RichText::new(format!("{} · {file}", slot.name())).strong())
                .truncate(),
        );
        let summary = match proposals {
            0 => tr("nothing to change").to_string(),
            1 => tr("1 change proposed").to_string(),
            n => format!("{n} {}", tr("changes proposed")),
        };
        ui.label(RichText::new(summary).color(MUTED));
    });
}

/// "30 -> 33" with a drawn arrow (the font has none), read out as written.
fn change(ui: &mut Ui, from: Option<&str>, to: &str, unit: &str) {
    let font = egui::FontId::proportional(16.0);
    let colour = ui.visuals().strong_text_color();
    let to_text = if unit.is_empty() {
        to.to_string()
    } else {
        format!("{to} {unit}")
    };
    let p = ui.painter();
    let from_g = from.map(|f| p.layout_no_wrap(f.to_string(), font.clone(), colour));
    let to_g = p.layout_no_wrap(to_text.clone(), font, colour);
    let from_w = from_g.as_ref().map_or(0.0, |g| g.size().x + ARROW);
    let size = egui::vec2(from_w + to_g.size().x, to_g.size().y);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let label = match from {
        Some(f) => format!("{f} -> {to_text}"),
        None => to_text,
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    let p = ui.painter();
    if let Some(g) = from_g {
        let x = rect.left() + g.size().x;
        p.galley(rect.min, g, colour);
        let y = rect.center().y + 1.0;
        let (a, b) = (egui::pos2(x + 6.0, y), egui::pos2(x + ARROW - 6.0, y));
        let stroke = egui::Stroke::new(1.5, MUTED);
        p.line_segment([a, b], stroke);
        p.line_segment([b, b + egui::vec2(-4.5, -3.5)], stroke);
        p.line_segment([b, b + egui::vec2(-4.5, 3.5)], stroke);
    }
    p.galley(egui::pos2(rect.left() + from_w, rect.top()), to_g, colour);
}

/// How sure the rule is: a short bar and the percent.
fn confidence(ui: &mut Ui, value: f64) {
    ui.label(RichText::new(format!("{:.0} %", value * 100.0)).color(MUTED));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(56.0, 6.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, ui.visuals().widgets.inactive.bg_fill);
    let mut fill = rect;
    fill.set_width(rect.width() * value.clamp(0.0, 1.0) as f32);
    p.rect_filled(fill, 3.0, ACCENT);
    resp.on_hover_text(tr(
        "how sure the rule is, from how far the measurement is past its threshold",
    ));
    ui.label(RichText::new(tr("confidence")).color(MUTED));
}

/// A proposal: what to change, why, and how. Returns the tab its "Show on"
/// button asked for.
fn card(ui: &mut Ui, slot: Slot, p: &Proposal, exporter: Option<&dyn Exporter>) -> Option<Tab> {
    let mut go = None;
    let frame = crate::theme::card(ui).stroke(egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.7)));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(p.title()).size(16.0).strong());
            change(ui, p.from.as_deref(), &p.to, p.unit());
            ui.label(RichText::new(&p.path).color(MUTED).monospace());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                confidence(ui, p.confidence);
            });
        });
        ui.label(&p.rationale);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            match exporter.map(|e| e.command(p)) {
                Some(Ok(command)) => {
                    let copy = ui
                        .button(tr("Copy"))
                        .on_hover_text(format!("{}: {command}", tr("copy")));
                    let name = format!("{} {} {}", tr("Copy"), slot.name(), p.path);
                    copy.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                    if copy.clicked() {
                        ui.ctx().copy_text(command.clone());
                    }
                    ui.label(RichText::new(command).monospace());
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e.to_string()).color(MUTED));
                }
                None => {}
            }
            let tab = evidence_tab(p);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let show = ui
                    .button(format!("{} {}", tr("Show on"), tab.title()))
                    .on_hover_text(tr("the tab with what this proposal was read from"));
                let name = format!(
                    "{} {} {} {}",
                    tr("Show on"),
                    tab.title(),
                    slot.name(),
                    p.path
                );
                show.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if show.clicked() {
                    go = Some(tab);
                }
            });
        });
    });
    go
}

/// A rule's name in words.
fn rule_title(rule: &str) -> String {
    match rule {
        "overshoot" => tr("Overshoot").to_string(),
        "peak" => tr("Noise peak").to_string(),
        "gyro-lpf" => tr("Gyro low-pass").to_string(),
        other => other.to_string(),
    }
}

/// A check that proposed nothing: its mark, the rule and axis, what it saw.
fn check_row(ui: &mut Ui, slot: Slot, f: &Finding) {
    let what = match f.axis {
        Some(a) => format!("{} {}", rule_title(&f.rule), a.name()),
        None => rule_title(&f.rule),
    };
    ui.horizontal_top(|ui| {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
        let (marker, hint) = match f.status {
            FindingStatus::Fine => {
                let level = FitnessLevel::Ok;
                paint_mark(ui.painter(), r.center(), level, level_color(level));
                ("[ok]", tr("the rule is satisfied"))
            }
            FindingStatus::Attention | FindingStatus::Proposed => {
                let level = FitnessLevel::Warn;
                paint_mark(ui.painter(), r.center(), level, level_color(level));
                ("[!]", tr("something is off; the text says what to look at"))
            }
            FindingStatus::NotJudged => {
                let c = r.center();
                ui.painter().line_segment(
                    [c + egui::vec2(-3.5, 0.0), c + egui::vec2(3.5, 0.0)],
                    egui::Stroke::new(1.6, MUTED),
                );
                ("[-]", tr("not judged: the text says why"))
            }
        };
        let name = format!("{} {what} {marker}", slot.name());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
        resp.on_hover_text(hint);
        ui.allocate_ui_with_layout(
            egui::vec2(RULE_W, 18.0),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.set_min_width(RULE_W);
                ui.add(egui::Label::new(RichText::new(what).color(MUTED)).truncate());
            },
        );
        ui.add(egui::Label::new(&f.text).wrap());
    });
}
