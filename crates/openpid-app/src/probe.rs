//! Where the plots of the current frame are on screen and what they show.
//!
//! Every plot that takes pointer input registers itself under a name
//! (`log/roll`, `log/motors`, ...) each frame. Whoever needs "the screen
//! position of time 14.2 s on the roll row" asks the probe instead of
//! knowing the layout: the UI tests (`testkit`) point at plots in data
//! units through it, so a test does not change when a panel moves, the
//! window is another size or touch mode scales the controls.

use eframe::egui;
use egui_plot::PlotTransform;
use std::collections::HashMap;

/// One plot on screen: its frame and the data range it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotProbe {
    /// Screen rectangle of the plot area (inside the axes).
    pub frame: egui::Rect,
    /// Visible x range, data units.
    pub x: (f64, f64),
    /// Visible y range, data units.
    pub y: (f64, f64),
}

// read by the UI tests only, so far
#[cfg_attr(not(test), allow(dead_code))]
impl PlotProbe {
    /// Screen position of the data point (`x`, `y`).
    pub fn pos(&self, x: f64, y: f64) -> egui::Pos2 {
        let fx = (x - self.x.0) / (self.x.1 - self.x.0);
        let fy = (y - self.y.0) / (self.y.1 - self.y.0);
        egui::pos2(
            self.frame.left() + fx as f32 * self.frame.width(),
            self.frame.bottom() - fy as f32 * self.frame.height(),
        )
    }

    /// Screen position of `x` at the middle of the plot's height.
    pub fn pos_x(&self, x: f64) -> egui::Pos2 {
        egui::pos2(self.pos(x, self.y.0).x, self.frame.center().y)
    }

    /// Data units per screen point along x: the precision of a pointer.
    pub fn x_per_point(&self) -> f64 {
        (self.x.1 - self.x.0) / f64::from(self.frame.width().max(1.0))
    }
}

/// The plots of the current frame by name.
#[derive(Debug, Default)]
pub struct Probes(HashMap<String, PlotProbe>);

impl Probes {
    /// Forget the last frame's plots.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Register the plot drawn with `transform` under `name`.
    pub fn record(&mut self, name: impl Into<String>, transform: &PlotTransform) {
        let b = transform.bounds();
        self.0.insert(
            name.into(),
            PlotProbe {
                frame: *transform.frame(),
                x: (b.min()[0], b.max()[0]),
                y: (b.min()[1], b.max()[1]),
            },
        );
    }

    /// The plot registered under `name` this frame.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get(&self, name: &str) -> Option<PlotProbe> {
        self.0.get(name).copied()
    }

    /// The names registered this frame, sorted (for error messages).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.0.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_follow_the_frame_and_the_range() {
        let p = PlotProbe {
            frame: egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(300.0, 150.0)),
            x: (0.0, 40.0),
            y: (-500.0, 500.0),
        };
        assert_eq!(p.pos(20.0, 0.0), egui::pos2(200.0, 100.0));
        assert_eq!(p.pos(0.0, 500.0), egui::pos2(100.0, 50.0));
        assert_eq!(p.pos_x(40.0), egui::pos2(300.0, 100.0));
        assert!((p.x_per_point() - 0.2).abs() < 1e-12);
        let mut all = Probes::default();
        assert!(all.get("a").is_none() && all.names().is_empty());
        all.0.insert("b".into(), p);
        all.0.insert("a".into(), p);
        assert_eq!(all.names(), ["a", "b"]);
        assert_eq!(all.get("a"), Some(p));
        all.clear();
        assert!(all.names().is_empty());
    }
}
