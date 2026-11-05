//! Perceptually ordered palette for heatmaps (viridis-like anchor colours).

use eframe::egui::Color32;

/// Slot colours: the Okabe–Ito palette (distinguishable with every common
/// colour-vision deficiency), slot A sky blue, slot B orange, then green,
/// purple, yellow, blue, vermilion, grey.
pub const SLOT_PALETTE: [Color32; 8] = [
    Color32::from_rgb(86, 180, 233),
    Color32::from_rgb(230, 159, 0),
    Color32::from_rgb(0, 158, 115),
    Color32::from_rgb(204, 121, 167),
    Color32::from_rgb(240, 228, 66),
    Color32::from_rgb(0, 114, 178),
    Color32::from_rgb(213, 94, 0),
    Color32::from_rgb(153, 153, 153),
];
pub const ACCENT: Color32 = Color32::from_rgb(204, 121, 167);
pub const MUTED: Color32 = Color32::from_rgb(140, 140, 140);

/// Colour for `t` in `0..=1` along the viridis anchors (`t <= 0` is transparent
/// so empty heatmap cells do not hide the grid). Shared with the CLI's PNG.
pub fn viridis(t: f64) -> Color32 {
    let [r, g, b, a] = openpid_analysis::render::viridis(t);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Robust colour-scale maximum: the 99th percentile of the positive values.
pub fn robust_max(values: &[f64]) -> f64 {
    openpid_analysis::render::robust_max(values)
}
