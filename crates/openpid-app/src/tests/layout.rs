//! The few tests whose subject is the layout or the pointer itself: the
//! columns of the control panel and the tooltips. They read rectangles and
//! hover with the pointer; every other test names controls and plots.

use super::{LOG_4_2, LOG_2025};
use crate::app::Tab;
use crate::shortcuts::{Action, keys_of};
use crate::testkit::Ui;
use eframe::egui::{Key, accesskit::Role};

/// Tooltips at once, not after egui's delay.
fn instant_tooltips(ui: &mut Ui) {
    ui.ctx()
        .all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    ui.settle();
}

#[test]
fn panel_rows_put_values_in_one_column_and_explain_on_hover() {
    // docs/design/axis-layout, "Control panel rules": the label on the left,
    // the value right-aligned in one column, a hint on every field
    let mut ui = Ui::open(&[LOG_2025], Tab::Step);
    instant_tooltips(&mut ui);
    let fields = ["Y max", "Window", "Cut-off", "Min input", "High input"];
    let rights: Vec<f32> = fields
        .iter()
        .map(|f| ui.rect(Role::SpinButton, f).right())
        .collect();
    for (f, r) in fields.iter().zip(&rights) {
        assert!((r - rights[0]).abs() <= 1.0, "{f}: {r} vs {}", rights[0]);
        let label = ui.rect(Role::Label, f);
        assert!(label.right() < *r - 40.0, "{f}: label left, value right");
    }
    // the units sit beside the numbers, in a column of their own up to the
    // panel's edge, where the combo boxes end
    let combo = ui.rect(Role::ComboBox, "Smoothing");
    let unit_col = combo.right() - rights[0];
    assert!(
        (crate::panel::UNIT_W..=crate::panel::UNIT_W + 8.0).contains(&unit_col),
        "unit column {unit_col}"
    );
    // a unit stays in view while the number is typed
    ui.edit("Cut-off");
    assert!(ui.has("Hz"), "unit beside the field in edit mode");
    ui.key(Key::Escape);
    // a hint on hover over the label and over the value
    ui.hover_role(Role::Label, "Cut-off");
    assert!(ui.sees("low-pass on setpoint and gyro"));
    ui.hover_role(Role::SpinButton, "Min input");
    assert!(ui.sees("too small a step to judge"));
    // the same rows on the Spectrum tab
    ui.tab(Tab::Spectrum);
    // the curve is one piece and has no segment; the density has
    assert!(!ui.sees("Segment"));
    ui.app_mut().tools.psd.method = openpid_analysis::PsdMethod::Density;
    ui.settle();
    let seg = ui.rect(Role::SpinButton, "Segment");
    let ch = ui.rect(Role::ComboBox, "Channel");
    assert!((ch.right() - seg.right() - crate::panel::UNIT_W).abs() <= 8.0);
    ui.hover_role(Role::Label, "Prominence");
    assert!(ui.sees("stand above its surroundings"));
}

#[test]
fn every_button_with_a_shortcut_shows_it_in_the_tooltip() {
    let mut ui = Ui::open(&[LOG_4_2], Tab::TimeHeatmap);
    instant_tooltips(&mut ui);
    let checks: Vec<(&str, Action)> = vec![
        ("Add log", Action::OpenLogNext),
        ("Play", Action::PlayPause),
        ("To start", Action::ToStart),
        ("< step", Action::StepBack),
        ("step >", Action::StepForward),
        ("Step response", Action::Tab(1)),
        // last: these two are in the top bar's menu, which stays open
        ("Open log", Action::OpenLog),
        ("Touch mode", Action::TouchMode),
    ];
    for (label, action) in checks {
        if ["Open log", "Touch mode"].contains(&label) {
            ui.open_menu();
        }
        ui.hover(label);
        let keys = keys_of(ui.ctx(), action);
        assert!(ui.sees(&keys), "{label}: tooltip should show {keys}");
    }
}
