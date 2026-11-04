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

/// Colour for `t` in `0..=1` along the viridis anchors (`t <= 0` is transparent
/// so empty heatmap cells do not hide the grid).
pub fn viridis(t: f64) -> Color32 {
    if t.is_nan() || t <= 0.0 {
        return Color32::TRANSPARENT;
    }
    let t = t.min(1.0) * (VIRIDIS.len() - 1) as f64;
    let i = (t.floor() as usize).min(VIRIDIS.len() - 2);
    let f = (t - i as f64) as f32;
    let (a, b) = (VIRIDIS[i], VIRIDIS[i + 1]);
    let mix = |x: u8, y: u8| (f32::from(x) * (1.0 - f) + f32::from(y) * f).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Robust colour-scale maximum: the 99th percentile of the positive values.
pub fn robust_max(values: &[f64]) -> f64 {
    let mut v: Vec<f64> = values
        .iter()
        .copied()
        .filter(|x| *x > 0.0 && x.is_finite())
        .collect();
    if v.is_empty() {
        return 1.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[((v.len() - 1) as f64 * 0.99) as usize].max(1e-12)
}
