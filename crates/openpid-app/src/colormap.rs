//! Perceptually ordered palette for heatmaps (viridis-like anchor colours).

use eframe::egui::Color32;

pub const VIRIDIS: [Color32; 8] = [
    Color32::from_rgb(68, 1, 84),
    Color32::from_rgb(70, 50, 126),
    Color32::from_rgb(54, 92, 141),
    Color32::from_rgb(39, 127, 142),
    Color32::from_rgb(31, 161, 135),
    Color32::from_rgb(74, 194, 109),
    Color32::from_rgb(159, 218, 58),
    Color32::from_rgb(253, 231, 37),
];

/// Series colours for slot A / slot B and secondary lines.
pub const SLOT_A: Color32 = Color32::from_rgb(86, 180, 233);
pub const SLOT_B: Color32 = Color32::from_rgb(230, 159, 0);
pub const ACCENT: Color32 = Color32::from_rgb(204, 121, 167);
pub const MUTED: Color32 = Color32::from_rgb(140, 140, 140);
