//! Plot grid lines and tick labels on the 1–2–5 series, for the plots of the
//! analysis tabs.
//!
//! `egui_plot`'s default spacer steps by powers of ten (1, 10, 100…). On the
//! stacked, narrower plots of the axis-row layout the labelled step then
//! falls between 10 and 100 px apart: a 0–55 % throttle axis or a −50…20 dB
//! power axis was left with the single label "0". Its default label
//! formatter takes the decimals from `round(-log10(step))`, which prints 1.5
//! as "2" on a 0.5 step, so the spacers here come with [`label`].

use eframe::egui;
use egui_plot::{GridInput, GridMark, Plot};
use std::ops::RangeInclusive;

/// The next step of the 1–2–5 series at or above `x` (x > 0).
pub fn nice_at_least(x: f64) -> f64 {
    let decade = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * decade)
        .find(|s| *s >= x * (1.0 - 1e-9))
        .unwrap_or(10.0 * decade)
}

/// The next finer step that divides `s` evenly: 1 → 0.5, 2 → 1, 5 → 1.
/// Nested steps keep every mark on one series: 0.5 and 0.2 together would
/// label 0.4, 0.5 and 0.6 side by side.
fn finer(s: f64) -> f64 {
    let decade = 10f64.powf(s.log10().floor());
    let mantissa = (s / decade).round();
    if mantissa == 5.0 { s / 5.0 } else { s / 2.0 }
}

/// Three nested grid steps: the labelled one about `top` times the finest
/// visible spacing (`base_step_size`, about 8 px) on the 1–2–5 series, and
/// the two finer ones that divide it.
fn steps(input: GridInput, top: f64) -> [f64; 3] {
    let labelled = nice_at_least(input.base_step_size.max(1e-12) * top);
    let mid = finer(labelled);
    [finer(mid), mid, labelled]
}

/// Steps for an x axis: labels need 60–80 px there, so the labelled step is
/// about ten times the finest spacing.
pub fn steps_x(input: GridInput) -> [f64; 3] {
    steps(input, 10.0)
}

/// Steps for a y axis: labels need 20–30 px.
pub fn steps_y(input: GridInput) -> [f64; 3] {
    steps(input, 4.0)
}

/// The spacer for `x_grid_spacer`.
pub fn spacer_x() -> impl Fn(GridInput) -> Vec<GridMark> {
    egui_plot::uniform_grid_spacer(steps_x)
}

/// The spacer for `y_grid_spacer`.
pub fn spacer_y() -> impl Fn(GridInput) -> Vec<GridMark> {
    egui_plot::uniform_grid_spacer(steps_y)
}

/// A plot with the app's grid on both axes: every plot of the app starts
/// here, so none is left with `egui_plot`'s power-of-ten steps.
pub fn plot<'a>(id: impl std::hash::Hash + std::fmt::Debug) -> Plot<'a> {
    Plot::new(id)
        .x_grid_spacer(spacer_x())
        .x_axis_formatter(label)
        .y_grid_spacer(spacer_y())
        .y_axis_formatter(label)
}

/// One row of a stack of plots that share the x axis: a y axis `y_axis_w`
/// wide, so the rows line up, and zoom and cursor in step along x under
/// `link`.
pub fn row<'a>(
    id: impl std::hash::Hash + std::fmt::Debug,
    link: egui::Id,
    y_axis_w: f32,
) -> Plot<'a> {
    plot(id)
        .y_axis_min_width(y_axis_w)
        .link_axis(link, [true, false])
        .link_cursor(link, [true, false])
}

/// A tick label with as many decimals as its step needs (0.5 → one, 20 →
/// none), for `x_axis_formatter` / `y_axis_formatter`.
pub fn label(mark: GridMark, _range: &RangeInclusive<f64>) -> String {
    let decimals = (-mark.step_size.log10() - 1e-9).ceil().max(0.0) as usize;
    let v = if mark.value.abs() < mark.step_size * 1e-6 {
        0.0
    } else {
        mark.value
    };
    format!("{v:.decimals$}")
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn steps_follow_one_two_five() {
        assert_eq!(nice_at_least(3.0), 5.0);
        assert_eq!(nice_at_least(5.0), 5.0);
        assert_eq!(nice_at_least(7.0), 10.0);
        assert_eq!(nice_at_least(0.03), 0.05);
        let input = || GridInput {
            bounds: (0.0, 55.0),
            base_step_size: 1.3,
        };
        assert_eq!(steps_x(input()), [5.0, 10.0, 20.0]);
        assert_eq!(steps_y(input()), [1.0, 5.0, 10.0]);
        // every step divides the next: one series of marks
        for base in [0.013, 0.07, 0.3, 2.2, 17.0] {
            let [a, b, c] = steps_y(GridInput {
                bounds: (0.0, 1.0),
                base_step_size: base,
            });
            for (fine, coarse) in [(a, b), (b, c)] {
                let k = coarse / fine;
                assert!((k - k.round()).abs() < 1e-9, "{fine} {coarse}");
            }
        }
    }

    #[test]
    fn labels_keep_the_decimals_of_their_step() {
        let mark = |value, step_size| GridMark { value, step_size };
        assert_eq!(label(mark(1.5, 0.5), &(0.0..=2.0)), "1.5");
        assert_eq!(label(mark(0.25, 0.05), &(0.0..=1.0)), "0.25");
        assert_eq!(label(mark(40.0, 20.0), &(0.0..=100.0)), "40");
        assert_eq!(label(mark(-1e-17, 0.2), &(-1.0..=1.0)), "0.0");
    }
}
