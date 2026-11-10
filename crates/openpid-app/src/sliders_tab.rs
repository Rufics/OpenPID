//! PID sliders in the panel layout (docs/design/sliders-tab, S3): a track per
//! slider with the position every visible log flew with, as the reference
//! tool compares the sliders of its logs; the handle on each track is the
//! what-if of the docked log. Beside the tracks the gains the log flew with
//! and the gains the sliders give, under them the firmware's commands.
//!
//! The sliders are the log's own model: Betaflight's simplified tuning when
//! the log has it, else ratio sliders over the logged gains
//! (`openpid_core::RatioSliders`), so no log with PID gains is a dead end.

use crate::app::{App, Loaded};
use crate::axis_layout::control_panel;
use crate::colormap::{ACCENT, MUTED};
use crate::i18n::tr;
use crate::panel;
use crate::views::slot_color;
use crate::worker::Slot;
use eframe::egui::{self, RichText, Ui};
use openpid_core::settings::{PerAxis, PidGains};
use openpid_core::{FlightSession, Settings, SliderDef, SliderModel, SliderResult, SliderValues};
use std::sync::Arc;

/// Width of the label column of the tracks.
const LABEL_W: f32 = 150.0;
/// Width of the number beside a track.
const VALUE_W: f32 = 64.0;
/// Width of the column with the gain tables.
const RESULT_W: f32 = 330.0;

/// State of the PID sliders tab.
#[derive(Debug, Default)]
pub struct SliderState {
    /// What the positions were loaded from: slot, session and model.
    pub loaded_for: Option<(Slot, usize, &'static str)>,
    /// Current slider positions keyed by the model's parameter name.
    pub values: SliderValues,
    /// The as-flown marks drawn last frame: per slider key, the logs and
    /// their positions (for the tests).
    pub marks: Vec<(String, Vec<(Slot, f64)>)>,
}

/// The docked log with its slider model.
struct Docked {
    slot: Slot,
    session_idx: usize,
    session: Arc<FlightSession>,
    model: Box<dyn SliderModel>,
}

fn docked(app: &App) -> Option<Docked> {
    let slot = app.dock_slot()?;
    let l = app.loaded(slot)?;
    let session = l.current()?.clone();
    let model = crate::worker::formats().slider_model(&session)?;
    Some(Docked {
        slot,
        session_idx: l.session,
        session,
        model,
    })
}

/// A slider that is a choice or a switch, shown in the panel, not as a track.
fn is_choice(d: &SliderDef) -> bool {
    !d.options.is_empty()
}

fn reset(app: &mut App, defs: &[SliderDef], to_default: bool) {
    app.slider_state.values = defs
        .iter()
        .map(|d| (d.key.clone(), if to_default { d.default } else { d.value }))
        .collect();
}

/// The tab.
pub fn sliders(app: &mut App, ui: &mut Ui) {
    let docked = docked(app);
    let defs = docked
        .as_ref()
        .map(|d| d.model.sliders())
        .unwrap_or_default();
    if let Some(d) = &docked {
        let key = (d.slot, d.session_idx, d.model.id());
        if app.slider_state.loaded_for != Some(key) {
            app.slider_state.loaded_for = Some(key);
            reset(app, &defs, false);
        }
    }
    control_panel(app, ui, "sliders-panel", |app, ui| {
        controls(app, ui, docked.as_ref(), &defs);
    });
    ui.separator();
    app.slider_state.marks.clear();
    let Some(d) = docked else {
        if let Some(slot) = app.dock_slot()
            && let Some(session) = app.loaded(slot).and_then(Loaded::current)
        {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    session.firmware.revision,
                    tr("no PID gains in this log, so there is nothing for sliders to scale")
                ))
                .color(MUTED),
            );
        }
        return;
    };
    let flown = flown_positions(app, &d);
    let result = d.model.compute(&app.slider_state.values);
    let full_w = ui.available_width();
    let track_w = (full_w - LABEL_W - VALUE_W - RESULT_W - 60.0).clamp(180.0, 520.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            legend(ui, &d, &flown);
            for (group, title) in [("pids", ""), ("filters", tr("Filter sliders"))] {
                let rows: Vec<&SliderDef> = defs
                    .iter()
                    .filter(|x| x.group == group && !is_choice(x))
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                if !title.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(title).color(MUTED).size(12.0));
                }
                tracks(app, ui, &rows, &flown, track_w);
            }
        });
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.set_width(RESULT_W);
            gains(ui, &d, &result);
        });
    });
    ui.add_space(6.0);
    commands(ui, d.model.as_ref(), &result);
}

/// The position every visible log with the same sliders flew with, per
/// slider key; the docked log first.
fn flown_positions(app: &App, d: &Docked) -> Vec<(Slot, SliderValues)> {
    let mut out = Vec::new();
    let mut slots = vec![d.slot];
    slots.extend(app.visible_slots().into_iter().filter(|s| *s != d.slot));
    for slot in slots {
        let Some(session) = app.loaded(slot).and_then(Loaded::current) else {
            continue;
        };
        let Some(model) = crate::worker::formats().slider_model(session) else {
            continue;
        };
        if model.id() != d.model.id() {
            continue;
        }
        out.push((
            slot,
            model
                .sliders()
                .into_iter()
                .map(|x| (x.key, x.value))
                .collect(),
        ));
    }
    out
}

/// A small dot in the text line: filled for a log's as-flown mark, a ring
/// for the what-if handle.
fn dot(ui: &mut Ui, colour: egui::Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
    if filled {
        ui.painter().circle_filled(rect.center(), 3.5, colour);
    } else {
        ui.painter()
            .circle_stroke(rect.center(), 4.0, egui::Stroke::new(1.5, colour));
    }
}

/// What the marks and the handle are.
fn legend(ui: &mut Ui, d: &Docked, flown: &[(Slot, SliderValues)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // the ratio sliders start from the log, the firmware's from its defaults
        let scale = if d.model.id() == openpid_core::RatioSliders::ID {
            tr("position in % (100 = the gains as flown)")
        } else {
            tr("position in % (100 = as the firmware ships)")
        };
        ui.label(RichText::new(scale).color(MUTED).size(12.0));
        ui.add_space(10.0);
        for (slot, _) in flown {
            dot(ui, slot_color(*slot), true);
            ui.label(RichText::new(slot.name()).size(12.0));
        }
        ui.label(RichText::new(tr("as flown")).color(MUTED).size(12.0));
        ui.add_space(10.0);
        dot(ui, ui.visuals().text_color(), false);
        ui.label(RichText::new(format!("{} ({})", tr("what-if"), d.slot.name())).size(12.0));
    });
    ui.add_space(4.0);
}

/// One track per slider: label, the slider with the as-flown marks under
/// it, the number.
fn tracks(
    app: &mut App,
    ui: &mut Ui,
    rows: &[&SliderDef],
    flown: &[(Slot, SliderValues)],
    track_w: f32,
) {
    let touch = app.touch_mode;
    let row_h = if touch { 44.0 } else { 28.0 };
    for (i, d) in rows.iter().enumerate() {
        let v = app
            .slider_state
            .values
            .entry(d.key.clone())
            .or_insert(d.value);
        let changed = (*v - d.value).abs() > f64::EPSILON;
        let marks: Vec<(Slot, f64)> = flown
            .iter()
            .filter_map(|(s, values)| values.get(&d.key).map(|x| (*s, *x)))
            .collect();
        let hint = marks
            .iter()
            .map(|(s, x)| format!("{} {x}", s.name()))
            .collect::<Vec<_>>()
            .join(" · ");
        let hint = format!("{} {hint}", tr("as flown:"));
        ui.horizontal(|ui| {
            ui.set_height(row_h);
            let text = RichText::new(tr_label(&d.label));
            let l = ui
                .allocate_ui_with_layout(
                    egui::vec2(LABEL_W, row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(LABEL_W);
                        ui.add(
                            egui::Label::new(if changed { text.color(ACCENT) } else { text })
                                .truncate(),
                        )
                    },
                )
                .inner
                .on_hover_text(&hint);
            ui.spacing_mut().slider_width = track_w;
            let mut slider = egui::Slider::new(v, d.min..=d.max)
                .step_by(5.0)
                .show_value(false);
            if touch {
                slider = slider.trailing_fill(true);
            }
            let r = ui
                .add_sized([track_w, row_h], slider)
                .labelled_by(l.id)
                .on_hover_text(&hint);
            paint_marks(ui, r.rect, d, &marks, i + 1 == rows.len());
            ui.add_sized(
                [VALUE_W, ui.spacing().interact_size.y],
                egui::DragValue::new(v)
                    .range(d.min..=d.max)
                    .speed(1.0)
                    .fixed_decimals(0),
            )
            .labelled_by(l.id)
            .on_hover_text(&hint);
        });
        app.slider_state.marks.push((d.key.clone(), marks));
    }
    // room for the scale under the last track
    ui.add_space(12.0);
}

/// The as-flown marks under a track, and the scale under the last one.
fn paint_marks(ui: &Ui, rect: egui::Rect, d: &SliderDef, marks: &[(Slot, f64)], last: bool) {
    // the slider's handle travels between the ends shrunk by its radius
    let handle_r = ui.spacing().interact_size.y.min(rect.height()) / 2.5;
    let x_of = |value: f64| {
        let t = ((value - d.min) / (d.max - d.min)).clamp(0.0, 1.0) as f32;
        egui::lerp((rect.left() + handle_r)..=(rect.right() - handle_r), t)
    };
    let painter = ui.painter();
    let y = rect.center().y + handle_r + 4.0;
    // the docked log last, so its mark is on top where the logs agree
    for (slot, value) in marks.iter().rev() {
        painter.circle_filled(egui::pos2(x_of(*value), y), 3.5, slot_color(*slot));
    }
    if last {
        // every 50 %, from the first one the slider reaches
        for value in [0.0, 50.0, 100.0, 150.0, 200.0] {
            if value < d.min || value > d.max {
                continue;
            }
            painter.text(
                egui::pos2(x_of(value), rect.bottom() + 6.0),
                egui::Align2::CENTER_TOP,
                format!("{value:.0}"),
                egui::FontId::proportional(10.5),
                MUTED,
            );
        }
    }
}

/// A slider's label in the interface language (the models name them in English).
fn tr_label(label: &str) -> String {
    match label {
        "Master multiplier" => tr("Master multiplier").to_string(),
        "P & I gain" => tr("P & I gain").to_string(),
        "I gain" => tr("I gain").to_string(),
        "D gain" => tr("D gain").to_string(),
        "Feedforward gain" => tr("Feedforward gain").to_string(),
        "Pitch D gain (roll/pitch ratio)" => tr("Pitch D gain").to_string(),
        "Pitch P & I gain" => tr("Pitch P & I gain").to_string(),
        "D-max gain (4.x: dmax_gain)" => tr("D-max gain").to_string(),
        "P:D ratio" => tr("P:D ratio").to_string(),
        "P:I ratio" => tr("P:I ratio").to_string(),
        "Roll:pitch ratio" => tr("Roll:pitch ratio").to_string(),
        "Yaw ratio" => tr("Yaw ratio").to_string(),
        "D-term filter multiplier" => tr("D-term filter").to_string(),
        "Gyro filter multiplier" => tr("Gyro filter").to_string(),
        "PID sliders mode" => tr("PID mode").to_string(),
        "D-term filter sliders" => tr("D-term filter sliders").to_string(),
        other => other.to_string(),
    }
}

/// The gains the log flew with and the gains the sliders give.
fn gains(ui: &mut Ui, d: &Docked, result: &SliderResult) {
    let settings = &d.session.settings;
    let logged = d.model.logged();
    let name = d.slot.name();
    if let Some(l) = &logged {
        ui.label(
            RichText::new(format!("{name} · {}", tr("logged")))
                .color(MUTED)
                .size(12.0),
        );
        gain_table(ui, "gains-logged", settings, l, None);
        ui.add_space(8.0);
    }
    ui.label(
        RichText::new(format!("{name} · {}", tr("from the sliders")))
            .color(ACCENT)
            .size(12.0),
    );
    gain_table(ui, "gains-sliders", settings, &result.pid, logged.as_ref());
    let filters = filter_summary(result);
    if !filters.is_empty() {
        ui.add_space(6.0);
        for line in filters {
            ui.label(RichText::new(line).color(MUTED).size(12.0));
        }
    }
}

/// `P I D Dmax F` per axis; with `against`, the cells that differ stand out.
fn gain_table(
    ui: &mut Ui,
    id: &str,
    settings: &Settings,
    g: &PerAxis<PidGains>,
    against: Option<&PerAxis<PidGains>>,
) {
    let dm = |x: &PidGains| x.d_max.or(x.d_min);
    let has_dm = [&g.roll, &g.pitch, &g.yaw].iter().any(|x| dm(x).is_some());
    let d_name = if g.roll.d_min.is_some() && g.roll.d_max.is_none() {
        "Dmin"
    } else {
        "Dmax"
    };
    egui::Grid::new(id)
        .spacing([18.0, 3.0])
        .min_col_width(0.0)
        .show(ui, |ui| {
            let head = |ui: &mut Ui, t: &str| {
                ui.label(RichText::new(t).color(MUTED).size(12.0));
            };
            head(ui, tr("axis"));
            for t in ["P", "I", "D"] {
                head(ui, t);
            }
            if has_dm {
                head(ui, d_name);
            }
            head(ui, "F");
            ui.end_row();
            let rows = [("roll", &g.roll), ("pitch", &g.pitch), ("yaw", &g.yaw)];
            let before = against.map(|a| [&a.roll, &a.pitch, &a.yaw]);
            for (i, (axis, x)) in rows.iter().enumerate() {
                ui.label(*axis);
                let was = before.map(|b| b[i]);
                let mut cells = vec![
                    (Some(x.p), was.map(|w| Some(w.p))),
                    (Some(x.i), was.map(|w| Some(w.i))),
                    (Some(x.d), was.map(|w| Some(w.d))),
                ];
                if has_dm {
                    cells.push((dm(x), was.map(dm)));
                }
                cells.push((x.f, was.map(|w| w.f)));
                for (value, old) in cells {
                    let text = value.map_or_else(|| "–".to_string(), |v| settings.gain_text(v));
                    // a gain that is absent and a gain of 0 are the same setting
                    let differs = old.is_some_and(|o| o.unwrap_or(0.0) != value.unwrap_or(0.0));
                    ui.label(if differs {
                        RichText::new(text).color(ACCENT).strong()
                    } else if against.is_some() {
                        RichText::new(text)
                    } else {
                        RichText::new(text).color(MUTED)
                    });
                }
                ui.end_row();
            }
        });
}

/// The filter cut-offs the sliders give, a line per filter chain.
fn filter_summary(result: &SliderResult) -> Vec<String> {
    let mut gyro = Vec::new();
    if let Some((lo, hi)) = result.gyro_lpf1_hz {
        gyro.push(format!("lpf1 {lo}–{hi} Hz"));
    }
    if let Some(v) = result.gyro_lpf2_hz {
        gyro.push(format!("lpf2 {v} Hz"));
    }
    let mut dterm = Vec::new();
    if let Some((lo, hi)) = result.dterm_lpf1_hz {
        dterm.push(format!("lpf1 {lo}–{hi} Hz"));
    }
    if let Some(v) = result.dterm_lpf2_hz {
        dterm.push(format!("lpf2 {v} Hz"));
    }
    let mut out = Vec::new();
    if !gyro.is_empty() {
        out.push(format!("gyro {}", gyro.join(" · ")));
    }
    if !dterm.is_empty() {
        out.push(format!("dterm {}", dterm.join(" · ")));
    }
    out
}

/// The commands that apply the sliders, or, for a model without commands,
/// what to do with the gains.
fn commands(ui: &mut Ui, model: &dyn SliderModel, result: &SliderResult) {
    if result.cli.is_empty() {
        ui.label(
            RichText::new(tr(
                "These sliders scale the gains the log flew with; this firmware has no sliders of its own, so type the result into its configurator.",
            ))
            .color(MUTED),
        );
        return;
    }
    let l = ui.label(crate::views::apply_label(model.cli_name()));
    let mut cli = result.cli.join("\n");
    let rows = ((ui.available_height() - 8.0) / 15.0).clamp(4.0, 40.0) as usize;
    ui.add(
        egui::TextEdit::multiline(&mut cli)
            .code_editor()
            .desired_rows(rows)
            .desired_width(f32::INFINITY)
            .interactive(true),
    )
    .labelled_by(l.id);
}

/// The panel: the model, its choices and switches, the resets.
fn controls(app: &mut App, ui: &mut Ui, docked: Option<&Docked>, defs: &[SliderDef]) {
    panel::section(ui, tr("Model"));
    let Some(d) = docked else {
        ui.label(RichText::new(tr("no sliders for this log")).color(MUTED));
        return;
    };
    ui.label(d.model.id());
    ui.label(
        RichText::new(&d.session.firmware.revision)
            .color(MUTED)
            .size(12.0),
    );
    for def in defs.iter().filter(|x| is_choice(x)) {
        let v = app
            .slider_state
            .values
            .entry(def.key.clone())
            .or_insert(def.value);
        let label = tr_label(&def.label);
        let hint = format!("{} {}", tr("as flown:"), option_name(def, def.value));
        if def.options.len() == 2 {
            let mut on = *v > def.min;
            if panel::check(ui, &mut on, &label, &hint).changed() {
                *v = if on { def.max } else { def.min };
            }
        } else {
            let options: Vec<(i64, String)> = def
                .options
                .iter()
                .enumerate()
                .map(|(i, name)| (def.min as i64 + i as i64, name.clone()))
                .collect();
            let mut choice = v.round() as i64;
            panel::choice(ui, &def.key, &label, &hint, &mut choice, &options);
            *v = choice as f64;
        }
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tr("Reset to log"))
            .on_hover_text(tr("every slider back to the position the log flew with"))
            .clicked()
        {
            reset(app, defs, false);
        }
        // a model whose defaults are what the log flew with has nothing to offer here
        if defs
            .iter()
            .any(|x| (x.default - x.value).abs() > f64::EPSILON)
            && ui
                .button(tr("Firmware defaults"))
                .on_hover_text(tr("every slider to the position the firmware ships with"))
                .clicked()
        {
            reset(app, defs, true);
        }
    });
    let changed: Vec<String> = defs
        .iter()
        .filter(|x| {
            app.slider_state
                .values
                .get(&x.key)
                .is_some_and(|v| (v - x.value).abs() > f64::EPSILON)
        })
        .map(|x| tr_label(&x.label))
        .collect();
    panel::section(ui, tr("Changed"));
    if changed.is_empty() {
        ui.label(RichText::new(tr("nothing: the sliders are as flown")).color(MUTED));
    } else {
        for name in changed {
            ui.label(RichText::new(name).color(ACCENT));
        }
    }
}

/// The name of position `value` of a choice.
fn option_name(def: &SliderDef, value: f64) -> String {
    let i = (value - def.min).round().max(0.0) as usize;
    def.options
        .get(i)
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_summed_up_a_line_per_chain() {
        let r = SliderResult {
            pid: PerAxis::default(),
            gyro_lpf1_hz: Some((250.0, 500.0)),
            gyro_lpf2_hz: None,
            dterm_lpf1_hz: Some((75.0, 150.0)),
            dterm_lpf2_hz: Some(150.0),
            cli: Vec::new(),
        };
        assert_eq!(
            filter_summary(&r),
            ["gyro lpf1 250–500 Hz", "dterm lpf1 75–150 Hz · lpf2 150 Hz"]
        );
        let none = SliderResult {
            gyro_lpf1_hz: None,
            dterm_lpf1_hz: None,
            dterm_lpf2_hz: None,
            ..r
        };
        assert!(filter_summary(&none).is_empty());
    }

    #[test]
    fn a_choice_names_its_positions() {
        let def = SliderDef {
            key: "mode".into(),
            label: "PID sliders mode".into(),
            value: 2.0,
            min: 0.0,
            max: 2.0,
            default: 2.0,
            group: "mode".into(),
            options: vec!["off".into(), "RP".into(), "RPY".into()],
        };
        assert!(is_choice(&def));
        assert_eq!(option_name(&def, 1.0), "RP");
        assert_eq!(option_name(&def, 7.0), "7");
        assert_eq!(tr_label(&def.label), "PID mode");
        assert_eq!(tr_label("Something else"), "Something else");
    }
}
