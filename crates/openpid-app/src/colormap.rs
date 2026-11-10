//! Plot colours: the log (slot) palettes and the heatmap colour map, as
//! Blackbox Explorer (default) or colour-blind safe (Okabe–Ito, viridis).

use crate::prefs::PlotColours;
use eframe::egui::Color32;
use openpid_analysis::render::Colormap;

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
/// Blackbox Explorer's line palette (`GraphConfig.PALETTE` in its
/// `graph_config.js`): red, cyan, yellow, purple, blue, orange, green, pink,
/// grey, dark purple, light green, dark yellow.
pub const EXPLORER_PALETTE: [Color32; 12] = [
    Color32::from_rgb(0xfb, 0x80, 0x72),
    Color32::from_rgb(0x8d, 0xd3, 0xc7),
    Color32::from_rgb(0xff, 0xff, 0xb3),
    Color32::from_rgb(0xbe, 0xba, 0xda),
    Color32::from_rgb(0x80, 0xb1, 0xd3),
    Color32::from_rgb(0xfd, 0xb4, 0x62),
    Color32::from_rgb(0xb3, 0xde, 0x69),
    Color32::from_rgb(0xfc, 0xcd, 0xe5),
    Color32::from_rgb(0xd9, 0xd9, 0xd9),
    Color32::from_rgb(0xbc, 0x80, 0xbd),
    Color32::from_rgb(0xcc, 0xeb, 0xc5),
    Color32::from_rgb(0xff, 0xed, 0x6f),
];

thread_local! {
    /// The plot colours this thread draws with, set every frame from the
    /// preferences (as the theme in `theme`, the language in `i18n`).
    static SCHEME: std::cell::Cell<PlotColours> = const { std::cell::Cell::new(PlotColours::Explorer) };
}

/// Record the plot colours this thread draws with.
pub fn set_scheme(p: PlotColours) {
    SCHEME.with(|s| s.set(p));
}

/// The plot colours this thread draws with.
pub fn scheme() -> PlotColours {
    SCHEME.with(std::cell::Cell::get)
}

/// The colours of the logs (slots) in the current scheme.
pub fn slot_palette() -> &'static [Color32] {
    match scheme() {
        PlotColours::Explorer => &EXPLORER_PALETTE,
        PlotColours::ColourBlindSafe => &SLOT_PALETTE,
    }
}

/// Colour `i` of the current scheme's palette, wrapping: the lines of one
/// plot that are not logs (the motors, the quantities of the Log viewer,
/// the parts of the filter chain).
pub fn line(i: usize) -> Color32 {
    let p = slot_palette();
    p[i % p.len()]
}

/// The heatmap colour map of the current scheme.
pub fn heatmap() -> Colormap {
    match scheme() {
        PlotColours::Explorer => Colormap::Explorer,
        PlotColours::ColourBlindSafe => Colormap::Viridis,
    }
}

/// The app accent (docs/design/app-shell): no slot uses it.
pub const ACCENT: Color32 = crate::theme::ACCENT;
pub const MUTED: Color32 = crate::theme::MUTED;

/// Colour for `t` in `0..=1` in the current heatmap colour map (`t <= 0` is
/// transparent so empty heatmap cells do not hide the grid).
pub fn heat(t: f64) -> Color32 {
    let [r, g, b, a] = heatmap().rgba(t);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Robust colour-scale maximum: the 99th percentile of the positive values.
pub fn robust_max(values: &[f64]) -> f64 {
    openpid_analysis::render::robust_max(values)
}
