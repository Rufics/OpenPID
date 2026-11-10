//! Settings (docs/design/settings-tab, G2): four cards in two columns, all
//! in view at once: Appearance, Units and First run on the left, the step
//! response Targets on the right. A row is a label on the left and its value
//! right-aligned, with an explanation on hover, as the control panels have;
//! a choice of two or three is a segmented control, so every option is in
//! view. Every change is saved at once.

use crate::app::App;
use crate::colormap::MUTED;
use crate::i18n::{Lang, tr};
use crate::panel;
use crate::prefs::{AccUnit, PlotColours, Prefs, RateUnit, Theme};
use eframe::egui::{self, Response, RichText, Ui};
use openpid_analysis::TargetPreset;

/// Under this width the cards are one column.
const TWO_COLUMNS_FROM: f32 = 820.0;
/// The widest the tab's content grows.
const MAX_W: f32 = 1100.0;

/// A choice among a few `options`, all in view: the chosen one is filled.
/// Each option is a button named by its text. Returns whether the value
/// changed; the response is that of the whole control.
fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str, &str)],
    changed: &mut bool,
) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // the row lays its value out from the right: keep the reading order
        let ordered: Vec<&(T, &str, &str)> = if ui.layout().prefer_right_to_left() {
            options.iter().rev().collect()
        } else {
            options.iter().collect()
        };
        for (option, text, hint) in ordered {
            let mut r = ui.selectable_label(*value == *option, *text);
            if !hint.is_empty() {
                r = r.on_hover_text(*hint);
            }
            if r.clicked() && *value != *option {
                *value = *option;
                *changed = true;
            }
        }
    })
    .response
}

/// A card with a small upper-case title.
fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    crate::theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(title.to_uppercase())
                .size(11.0)
                .strong()
                .color(MUTED),
        );
        ui.add_space(2.0);
        body(ui);
    });
    ui.add_space(10.0);
}

/// The tab.
pub fn settings(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .id_salt("settings")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            let mut changed = false;
            let w = ui.available_width().min(MAX_W);
            ui.allocate_ui(egui::vec2(w, 0.0), |ui| {
                ui.set_width(w);
                if w >= TWO_COLUMNS_FROM {
                    ui.columns(2, |cols| {
                        appearance(app, &mut cols[0], &mut changed);
                        units(app, &mut cols[0], &mut changed);
                        first_run(app, &mut cols[0], &mut changed);
                        targets(app, &mut cols[1], &mut changed);
                    });
                } else {
                    appearance(app, ui, &mut changed);
                    units(app, ui, &mut changed);
                    targets(app, ui, &mut changed);
                    first_run(app, ui, &mut changed);
                }
                footer(app, ui, &mut changed);
            });
            if changed {
                app.save_prefs();
            }
        });
}

fn appearance(app: &mut App, ui: &mut Ui, changed: &mut bool) {
    card(ui, tr("Appearance"), |ui| {
        panel::row(
            ui,
            tr("Theme"),
            tr("the colours of the interface: dark or light"),
            |ui| {
                segmented(
                    ui,
                    &mut app.prefs.theme,
                    &[
                        (Theme::Dark, tr("Dark"), ""),
                        (Theme::Light, tr("Light"), ""),
                    ],
                    changed,
                )
            },
        );
        panel::row(
            ui,
            tr("Plot colours"),
            tr("the colours of the logs' lines and of the heatmaps"),
            |ui| {
                segmented(
                    ui,
                    &mut app.prefs.plot_colours,
                    &[
                        (
                            PlotColours::Explorer,
                            tr("Blackbox Explorer"),
                            tr("Blackbox Explorer's line colours and its black-red-white heatmaps"),
                        ),
                        (
                            PlotColours::ColourBlindSafe,
                            tr("Colour-blind safe"),
                            tr(
                                "Okabe-Ito line colours and viridis heatmaps, readable with colour-vision deficiencies",
                            ),
                        ),
                    ],
                    changed,
                )
            },
        );
        panel::row(
            ui,
            tr("Language"),
            tr("the language of the interface, switched at once; units and numbers do not change"),
            |ui| {
                let options: Vec<(Lang, &str, &str)> = Lang::ALL
                    .into_iter()
                    .map(|l| (l, l.native_name(), ""))
                    .collect();
                segmented(ui, &mut app.prefs.lang, &options, changed)
            },
        );
        let touch = panel::row(
            ui,
            tr("Touch mode"),
            tr(
                "larger controls and touch gestures (pinch, two-finger pan, long press); it also switches itself on at the first touch",
            ),
            |ui| ui.checkbox(&mut app.prefs.touch_mode, ""),
        );
        if touch.changed() {
            app.touch_mode = app.prefs.touch_mode;
            *changed = true;
        }
    });
}

fn units(app: &mut App, ui: &mut Ui, changed: &mut bool) {
    card(ui, tr("Units"), |ui| {
        panel::row(
            ui,
            tr("Angular rate"),
            tr("the unit of the gyro and setpoint traces in the log viewer"),
            |ui| {
                segmented(
                    ui,
                    &mut app.prefs.rate_unit,
                    &[
                        (RateUnit::DegPerS, "deg/s", ""),
                        (RateUnit::RadPerS, "rad/s", ""),
                    ],
                    changed,
                )
            },
        );
        panel::row(
            ui,
            tr("Acceleration"),
            tr("the unit of the accelerometer traces in the log viewer"),
            |ui| {
                segmented(
                    ui,
                    &mut app.prefs.acc_unit,
                    &[(AccUnit::G, "g", ""), (AccUnit::MPerS2, "m/s²", "")],
                    changed,
                )
            },
        );
        ui.label(
            RichText::new(tr(
                "Units apply to the log viewer traces; analyses stay in deg/s.",
            ))
            .color(MUTED),
        );
    });
}

fn targets(app: &mut App, ui: &mut Ui, changed: &mut bool) {
    let mut edited = false;
    card(ui, tr("Targets"), |ui| {
        ui.label(
            RichText::new(tr(
                "Thresholds behind the step-response verdicts and the Advice overshoot rule.",
            ))
            .color(MUTED),
        );
        let current = TargetPreset::matching(&app.tools.recommend.targets);
        panel::row(
            ui,
            tr("Preset"),
            tr("ready thresholds for a flying style; editing a value below makes them custom"),
            |ui| {
                let mut chosen = current;
                let options: Vec<(Option<TargetPreset>, &str, &str)> = TargetPreset::ALL
                    .into_iter()
                    .map(|p| (Some(p), tr(p.name()), ""))
                    .collect();
                let r = segmented(ui, &mut chosen, &options, &mut edited);
                if chosen != current
                    && let Some(p) = chosen
                {
                    app.tools.recommend.targets = p.targets();
                }
                if current.is_none() {
                    ui.label(RichText::new(tr("Custom")).color(MUTED));
                }
                r
            },
        );
        let t = &mut app.tools.recommend.targets;
        let mut number = |ui: &mut Ui,
                          label: &'static str,
                          hint: &'static str,
                          v: &mut f64,
                          (speed, lo, hi): (f64, f64, f64),
                          unit: &str| {
            edited |= panel::drag(ui, tr(label), tr(hint), v, speed, lo..=hi, unit).changed();
        };
        number(
            ui,
            "Overshoot warn",
            "an overshoot above this is a warning, and the Advice rule proposes more D",
            &mut t.overshoot_warn_pct,
            (1.0, 1.0, 100.0),
            "%",
        );
        number(
            ui,
            "Overshoot fail",
            "an overshoot above this fails the axis",
            &mut t.overshoot_fail_pct,
            (1.0, 1.0, 200.0),
            "%",
        );
        number(
            ui,
            "Rise max",
            "the longest rise time (10 to 90 %) that is still fine",
            &mut t.rise_ms_max,
            (1.0, 5.0, 300.0),
            "ms",
        );
        number(
            ui,
            "Settling max",
            "the longest time to settle within the final tolerance that is still fine",
            &mut t.settling_ms_max,
            (1.0, 10.0, 500.0),
            "ms",
        );
        number(
            ui,
            "Final tolerance",
            "how far from 1 the settled response may be: 0.10 is within 10 %",
            &mut t.final_tol,
            (0.01, 0.01, 0.5),
            "",
        );
        let mut windows = |ui: &mut Ui, label: &'static str, hint: &'static str, v: &mut usize| {
            edited |= panel::drag_usize(ui, tr(label), tr(hint), v, 1..=100).changed();
        };
        windows(
            ui,
            "Min windows ok",
            "with at least this many step windows the verdicts are trusted",
            &mut t.min_windows_ok,
        );
        windows(
            ui,
            "Min windows weak",
            "with fewer step windows than this the verdicts are greyed out",
            &mut t.min_windows_weak,
        );
    });
    if edited {
        app.prefs.targets = app.tools.recommend.targets;
        *changed = true;
    }
}

fn first_run(app: &mut App, ui: &mut Ui, changed: &mut bool) {
    card(ui, tr("First run"), |ui| {
        *changed |= panel::row(
            ui,
            tr("Show the tour on the first opened log"),
            tr("a fresh install starts the seven-step tour when its first log opens"),
            |ui| ui.checkbox(&mut app.prefs.show_tour, ""),
        )
        .changed();
        let show = panel::action(
            ui,
            tr("Tour"),
            tr("walk through the tabs now, on the log that is open"),
            tr("Show tour"),
        );
        if show.clicked() {
            app.start_tour();
        }
        let reset = panel::action(
            ui,
            tr("First run"),
            tr("forget that the tour was seen, so the next first log starts it again"),
            tr("Reset first-run state"),
        );
        if reset.clicked() {
            app.prefs.tour_done = false;
            *changed = true;
        }
    });
}

/// Where the settings are saved, and the way back to the defaults.
fn footer(app: &mut App, ui: &mut Ui, changed: &mut bool) {
    ui.horizontal(|ui| {
        let saved = match &app.prefs_path {
            Some(p) => format!("{} {}", tr("Saved in"), p.display()),
            None => {
                tr("No configuration directory: settings last until the app closes.").to_string()
            }
        };
        ui.add(egui::Label::new(RichText::new(saved).color(MUTED)).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(tr("Reset to defaults"))
                .on_hover_text(tr(
                    "every setting on this tab back to its default; the recent files stay",
                ))
                .clicked()
            {
                let recent = std::mem::take(&mut app.prefs.recent);
                app.prefs = Prefs {
                    recent,
                    ..Prefs::default()
                };
                app.touch_mode = app.prefs.touch_mode;
                app.tools.recommend.targets = app.prefs.targets;
                *changed = true;
            }
        });
    });
}
