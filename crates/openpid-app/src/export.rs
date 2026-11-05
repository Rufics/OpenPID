//! Export of what the screen shows: the tab area as PNG (through the
//! viewport screenshot) and a slot's analysis result as JSON, serialised
//! exactly as `openpid run … ` prints it.

use crate::app::{App, Cached, Tab};
use crate::worker::{JobKey, Kind, Output, Slot};
use eframe::egui;
use openpid_analysis::render::png_rgba;
use std::path::Path;
use std::sync::Arc;

/// The JSON `openpid run` would print for this result.
pub fn output_json(o: &Output) -> Result<String, String> {
    match o {
        Output::Step(s) => serde_json::to_string_pretty(s),
        Output::Psd(p) => serde_json::to_string_pretty(p),
        Output::Throttle(t) => serde_json::to_string_pretty(t),
        Output::Stft(s) => serde_json::to_string_pretty(s),
        Output::FilterSim(f) => serde_json::to_string_pretty(f.as_ref()),
        Output::MotorNoise(m) => serde_json::to_string_pretty(m),
    }
    .map_err(|e| e.to_string())
}

/// Write `image` (or the part of it under `region`, in points at
/// `pixels_per_point`) as a PNG file.
pub fn write_png(
    image: &egui::ColorImage,
    region: Option<egui::Rect>,
    pixels_per_point: f32,
    path: &Path,
) -> Result<(), String> {
    let [w, h] = image.size;
    let (x0, y0, x1, y1) = match region {
        Some(r) if r.is_positive() => (
            ((r.min.x * pixels_per_point).floor().max(0.0) as usize).min(w),
            ((r.min.y * pixels_per_point).floor().max(0.0) as usize).min(h),
            ((r.max.x * pixels_per_point).ceil().max(0.0) as usize).min(w),
            ((r.max.y * pixels_per_point).ceil().max(0.0) as usize).min(h),
        ),
        _ => (0, 0, w, h),
    };
    if x1 <= x0 || y1 <= y0 {
        return Err("nothing to export: the plot area is empty".into());
    }
    let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        for x in x0..x1 {
            let c = image.pixels[y * w + x];
            rgba.extend_from_slice(&[c.r(), c.g(), c.b(), c.a()]);
        }
    }
    std::fs::write(path, png_rgba(x1 - x0, y1 - y0, &rgba))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

impl App {
    /// The analysis the current tab shows (`None` on tabs without one).
    pub fn current_kind(&self) -> Option<Kind> {
        Some(match self.tab {
            Tab::Step => Kind::Step(self.tools.step.clone()),
            Tab::Spectrum => Kind::Psd(self.tools.psd.clone()),
            Tab::ThrottleHeatmap | Tab::Surface => Kind::Throttle(self.tools.throttle.clone()),
            Tab::TimeHeatmap => Kind::Stft(self.tools.stft.clone()),
            Tab::FilterSim => Kind::FilterSim(self.tools.filter_sim.clone()),
            Tab::MotorNoise => Kind::MotorNoise(self.tools.motor_noise.clone()),
            Tab::LogViewer | Tab::Sliders | Tab::Diagnostics | Tab::Settings => return None,
        })
    }

    /// The current tab's result for `slot`, when it is computed.
    pub fn current_result(&self, slot: Slot) -> Option<Arc<Output>> {
        let kind = self.current_kind()?;
        let l = self.loaded(slot)?;
        match self
            .cache
            .get(&JobKey::new(slot, l.session, &kind, l.epoch))
        {
            Some(Cached::Ready(o)) => Some(o.clone()),
            _ => None,
        }
    }

    /// `<log>-<tab>-<epoch>.<ext>`: the log's file stem, the tab's short name
    /// and the epoch bounds (or `all`).
    pub fn export_file_name(&self, slot: Slot, ext: &str) -> String {
        let (stem, epoch) = match self.loaded(slot) {
            Some(l) => (
                l.path.file_stem().map_or_else(
                    || "openpid".to_string(),
                    |s| s.to_string_lossy().into_owned(),
                ),
                l.epoch.map_or_else(
                    || "all".to_string(),
                    |e| format!("{:.1}-{:.1}", e.start_s, e.end_s),
                ),
            ),
            None => ("openpid".to_string(), "all".to_string()),
        };
        format!("{stem}-{}-{epoch}.{ext}", self.tab.name())
    }

    /// Write the current tab's result for `slot` as JSON, byte for byte what
    /// `openpid run <analysis> --out` would write.
    pub fn export_json(&self, slot: Slot, path: &Path) -> Result<(), String> {
        let o = self
            .current_result(slot)
            .ok_or_else(|| format!("nothing to export for slot {} on this tab", slot.name()))?;
        std::fs::write(path, output_json(&o)?)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// Ask the viewport for a screenshot and save the tab area to `path`;
    /// the outcome lands in the status bar on the next frame.
    pub fn export_png(&self, ctx: &egui::Context, path: std::path::PathBuf) {
        let region = self.central_rect;
        let ppp = ctx.pixels_per_point();
        let message = self.export_message.clone();
        let repaint = ctx.clone();
        ctx.request_screenshot(move |image| {
            let msg = match write_png(&image, Some(region), ppp, &path) {
                Ok(()) => format!("wrote {}", path.display()),
                Err(e) => format!("export failed: {e}"),
            };
            if let Ok(mut m) = message.lock() {
                *m = Some(msg);
            }
            repaint.request_repaint();
        });
    }
}
