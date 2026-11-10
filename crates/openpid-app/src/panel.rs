//! The rows of a tab's control panel (docs/design/axis-layout, "Control
//! panel rules"): the label on the left, the control right-aligned in one
//! column, and an explanation on hover over either. Every field of every
//! panel goes through these helpers, so the layout and the hints cannot be
//! forgotten on a new one.

use crate::colormap::MUTED;
use crate::i18n::tr;
use eframe::egui::{self, Response, RichText, Ui};

/// Width of the value column: combo boxes are this wide, numbers right-align in it.
pub const VALUE_W: f32 = 112.0;
/// Width of the unit column beside panel numbers, so the numbers line up.
pub const UNIT_W: f32 = 30.0;
/// Height of a row.
const ROW_H: f32 = 24.0;

/// A number with its unit beside it, not inside: the unit stays in view
/// while the value is typed (`DragValue::suffix` disappears in edit mode).
/// Follows the direction of the enclosing layout.
pub fn with_unit(ui: &mut Ui, value: egui::DragValue<'_>, unit: &str) -> Response {
    unit_pair(ui, value, unit, None)
}

/// [`with_unit`] with the unit in a column `w` wide (empty units included),
/// so the numbers of a panel end at the same x.
fn unit_pair(ui: &mut Ui, value: egui::DragValue<'_>, unit: &str, w: Option<f32>) -> Response {
    let rtl = ui.layout().prefer_right_to_left();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if rtl {
            unit_label(ui, unit.trim(), w);
            ui.add(value)
        } else {
            let r = ui.add(value);
            unit_label(ui, unit.trim(), w);
            r
        }
    })
    .inner
}

fn unit_label(ui: &mut Ui, unit: &str, w: Option<f32>) {
    let text = RichText::new(unit).color(MUTED);
    match w {
        Some(w) => {
            ui.allocate_ui_with_layout(
                egui::vec2(w, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(w);
                    if !unit.is_empty() {
                        ui.label(text);
                    }
                },
            );
        }
        None if !unit.is_empty() => {
            ui.label(text);
        }
        None => {}
    }
}

/// A section title: small, upper case, muted, with room above it.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.0)
            .strong()
            .color(MUTED),
    );
    ui.add_space(2.0);
}

/// One row: `label` on the left, the control `add` draws right-aligned, and
/// `hint` on hover over both. The control is named by the label for screen
/// readers and tests.
pub fn row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    add: impl FnOnce(&mut Ui) -> Response,
) -> Response {
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(w, ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let l = ui
                .add(egui::Label::new(label).sense(egui::Sense::hover()))
                .on_hover_text(hint);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                add(ui).labelled_by(l.id).on_hover_text(hint)
            })
            .inner
        },
    )
    .inner
}

/// A number: label on the left, the value right-aligned.
pub fn drag(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> Response {
    row(ui, label, hint, |ui| {
        unit_pair(
            ui,
            egui::DragValue::new(value).speed(speed).range(range),
            suffix,
            Some(UNIT_W),
        )
    })
}

/// A number with a [`egui::DragValue`] built by the caller (decimals and the
/// like), as [`drag`].
pub fn drag_value(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: egui::DragValue<'_>,
    unit: &str,
) -> Response {
    row(ui, label, hint, |ui| {
        unit_pair(ui, value, unit, Some(UNIT_W))
    })
}

/// A whole number, as [`drag`].
pub fn drag_usize(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut usize,
    range: std::ops::RangeInclusive<usize>,
) -> Response {
    row(ui, label, hint, |ui| {
        unit_pair(
            ui,
            egui::DragValue::new(value).range(range),
            "",
            Some(UNIT_W),
        )
    })
}

/// A choice among `options` (value, text): a combo box in the value column.
pub fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    label: &str,
    hint: &str,
    value: &mut T,
    options: &[(T, String)],
) -> Response {
    let current = options
        .iter()
        .find(|(v, _)| v == value)
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    row(ui, label, hint, |ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(current)
            .width(VALUE_W)
            .show_ui(ui, |ui| {
                for (v, text) in options {
                    ui.selectable_value(value, *v, text);
                }
            })
            .response
    })
}

/// An on/off option: the checkbox with its text, and the hint on hover.
pub fn check(ui: &mut Ui, value: &mut bool, label: &str, hint: &str) -> Response {
    check_enabled(ui, true, value, label, hint)
}

/// [`check`] that can be greyed out; the hint says why when it is.
pub fn check_enabled(
    ui: &mut Ui,
    enabled: bool,
    value: &mut bool,
    label: &str,
    hint: &str,
) -> Response {
    ui.add_enabled(enabled, egui::Checkbox::new(value, label))
        .on_hover_text(hint)
        .on_disabled_hover_text(hint)
}

/// An optional bound: "auto" until set (a click fixes it at `start`), else
/// the value and a small "auto" to go back.
pub fn optional(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut Option<f64>,
    start: f64,
    unit: &str,
) -> Response {
    row(ui, label, hint, |ui| {
        if let Some(mut v) = *value {
            let back = ui
                .small_button(tr("auto"))
                .on_hover_text(tr("back to the automatic range"))
                .clicked();
            let r = unit_pair(
                ui,
                egui::DragValue::new(&mut v).speed(1.0),
                unit,
                Some(UNIT_W),
            );
            *value = if back { None } else { Some(v) };
            r
        } else {
            let r = ui.button(tr("auto"));
            if r.clicked() {
                *value = Some(start);
            }
            r
        }
    })
}
