//! The "Surface 3D" tab: the freq × throttle × amplitude grid as an orbiting
//! surface drawn with egui's painter (no GPU dependency, DESIGN §6.6, #26).
//! Geometry comes from `openpid_analysis::surface`; this module only handles
//! input, the axes frame, the overlay lines and the hover read-out.

use crate::app::App;
use crate::colormap::MUTED;
use crate::i18n::tr;
use crate::views::{
    BAR_W, OverlayState, SlotResult, colour_bar, heatmap_controls, linked_scales, motor_color,
    overlay_controls, overlay_harmonics, panel_width, show_error, slot_color,
};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Ui, Vec2};
use openpid_analysis::ThrottleHeatmapOutput;
use openpid_analysis::surface::{
    Camera, SurfaceOptions, build, colour_max, decimate, model_point, value_at,
};
use openpid_core::analysis::Grid2D;

/// Radians of orbit per logical pixel of drag.
const ORBIT_PER_PX: f32 = 0.008;
/// Pointer distance (logical pixels) within which a node is "under" it.
const HOVER_PX: f32 = 12.0;
/// Rows below this frequency (Hz) do not drive the automatic colour maximum.
const SCALE_FLOOR_HZ: f64 = 20.0;

/// Camera, pan and hover state of the tab.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceState {
    /// Orbit camera; `center` is set every frame from the panel and `pan`.
    pub camera: Camera,
    /// Pan offset from the panel centre, logical pixels.
    pub pan: Vec2,
    /// Lambertian shading of the faces.
    pub shade: bool,
    /// The zoom was fitted to the panel once.
    pub fitted: bool,
    /// `(throttle %, Hz, amplitude)` of the node under the pointer last frame.
    pub hover: Option<(f64, f64, f64)>,
    /// Screen rectangle of the (first) surface panel last frame.
    pub frame: Option<egui::Rect>,
    /// Cells of the last mesh drawn (after decimation).
    pub cells: usize,
}

impl Default for SurfaceState {
    fn default() -> Self {
        SurfaceState {
            camera: Camera::default(),
            pan: Vec2::ZERO,
            shade: true,
            fitted: false,
            hover: None,
            frame: None,
            cells: 0,
        }
    }
}

impl SurfaceState {
    /// Back to the default view.
    pub fn reset(&mut self) {
        let shade = self.shade;
        *self = SurfaceState {
            shade,
            ..SurfaceState::default()
        };
    }
}

/// The tab.
pub fn surface(app: &mut App, ui: &mut Ui) {
    ui.separator();
    heatmap_controls(app, ui);
    ui.horizontal_wrapped(|ui| {
        let l = ui.label(tr("height"));
        ui.add(
            egui::Slider::new(&mut app.surface.camera.height, 0.05..=1.0)
                .show_value(false),
        )
        .labelled_by(l.id)
        .on_hover_text(tr("height of the colour maximum"));
        ui.checkbox(&mut app.surface.shade, tr("shading"));
        if ui.button(tr("Reset view")).clicked() {
            app.surface.reset();
        }
        ui.label(
            RichText::new(tr("drag: orbit · wheel / pinch: zoom · shift-drag, right-drag or two fingers: pan · double-click: reset"),
            )
            .color(MUTED),
        );
    });
    app.tools.throttle.channel = app.tools.heatmap_channel.clone();
    let params = app.tools.throttle.clone();
    let max = app.tools.heatmap_max;
    let slots: Vec<Slot> = app.visible_slots();
    let results: Vec<SlotResult> = slots
        .iter()
        .map(|slot| (*slot, app.result(*slot, Kind::Throttle(params.clone()))))
        .collect();
    overlay_controls(app, ui, &results);
    let overlay = app.overlay.clone();
    let grids: Vec<(Slot, &Grid2D)> = results
        .iter()
        .filter_map(|(s, r)| match r {
            Some(Ok(o)) => match o.as_ref() {
                Output::Throttle(t) => Some((*s, &t.grid)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let scales = linked_scales(app, &grids, SCALE_FLOOR_HZ);
    let w = panel_width(ui, slots.len());
    let h = ui.available_height();
    app.surface.hover = None;
    egui::ScrollArea::horizontal()
        .id_salt("surface-panels")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, (slot, result)) in results.into_iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.set_width(w - 8.0);
                        match result {
                            Some(Ok(o)) => {
                                if let Output::Throttle(t) = o.as_ref() {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {} ({}), {} segments",
                                            slot.name(),
                                            t.channel.key(),
                                            t.unit,
                                            t.segments
                                        ))
                                        .color(slot_color(slot)),
                                    );
                                    let scale = scales.get(&slot).copied().unwrap_or(max);
                                    let size = Vec2::new(w - 8.0 - BAR_W, (h - 24.0).max(50.0));
                                    ui.horizontal(|ui| {
                                        draw_surface(
                                            app,
                                            ui,
                                            slot,
                                            t,
                                            scale,
                                            &overlay,
                                            size,
                                            i == 0,
                                        );
                                        colour_bar(ui, scale, size.y);
                                    });
                                }
                            }
                            Some(Err(e)) => show_error(ui, slot, &e),
                            None => {
                                ui.spinner();
                            }
                        }
                    });
                }
            });
        });
}

/// Orbit, zoom and pan from the panel's response.
fn interact(st: &mut SurfaceState, ui: &Ui, resp: &egui::Response) {
    let shift = ui.input(|i| i.modifiers.shift);
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        if shift {
            st.pan += d;
        } else {
            st.camera.yaw += d.x * ORBIT_PER_PX;
            st.camera.pitch += d.y * ORBIT_PER_PX;
        }
    }
    if resp.dragged_by(egui::PointerButton::Secondary)
        || resp.dragged_by(egui::PointerButton::Middle)
    {
        st.pan += resp.drag_delta();
    }
    if resp.double_clicked() {
        st.reset();
    }
    if resp.hovered() {
        let (zoom, scroll, two_fingers) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta,
                i.multi_touch().map(|t| t.translation_delta),
            )
        });
        if (zoom - 1.0).abs() > f32::EPSILON {
            st.camera.zoom *= zoom;
        } else if scroll.y.abs() > 0.0 {
            st.camera.zoom *= (scroll.y * 0.003).exp();
        }
        if let Some(t) = two_fingers {
            st.pan += t;
        }
    }
    st.camera.normalise();
}

#[allow(
    clippy::too_many_arguments,
    reason = "one call site, all state is named"
)]
fn draw_surface(
    app: &mut App,
    ui: &mut Ui,
    slot: Slot,
    t: &ThrottleHeatmapOutput,
    max: f64,
    overlay: &OverlayState,
    size: Vec2,
    first: bool,
) {
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    let st = &mut app.surface;
    if !st.fitted {
        st.camera.zoom = rect.width().min(rect.height()) * 0.95;
        st.fitted = true;
    }
    interact(st, ui, &resp);
    st.camera.center = [rect.center().x + st.pan.x, rect.center().y + st.pan.y];
    let opts = SurfaceOptions {
        max,
        min_y_for_scale: SCALE_FLOOR_HZ,
        ..SurfaceOptions::default()
    };
    let grid = decimate(&t.grid, opts.max_cols, opts.max_rows);
    let max = colour_max(&grid, &opts);
    let cam = st.camera;
    let m = build(&grid, max, &cam, st.shade);
    if first {
        st.cells = m.cols.saturating_sub(1) * m.rows.saturating_sub(1);
        st.frame = Some(rect);
    }
    // the surface
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(m.positions.len());
    for (p, c) in m.positions.iter().zip(&m.colors) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(p[0], p[1]),
            uv: egui::epaint::WHITE_UV,
            color: Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
        });
    }
    mesh.indices = m.indices;
    painter.add(egui::Shape::mesh(mesh));
    axes_frame(&painter, &cam, &grid, ui.visuals().text_color());
    overlay_lines(&painter, &cam, &grid, max, t, overlay);
    // hover read-out: the nearest node
    let hover = resp.hover_pos().and_then(|pos| {
        let (i, d2) = m
            .positions
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (p[0] - pos.x).powi(2) + (p[1] - pos.y).powi(2)))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        (d2 <= HOVER_PX * HOVER_PX).then(|| {
            let (ix, iy) = (i % m.cols, i / m.cols);
            (grid.x[ix], grid.y[iy], grid.z[i])
        })
    });
    if let Some((x, y, z)) = hover {
        painter.text(
            rect.left_top() + Vec2::new(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            format!("{}: {x:.0} %, {y:.0} Hz, {z:.3} {}", slot.name(), t.unit),
            egui::FontId::proportional(13.0),
            ui.visuals().strong_text_color(),
        );
        if first {
            app.surface.hover = Some((x, y, z));
        }
    }
}

/// Base rectangle, one vertical edge and the axis labels.
fn axes_frame(painter: &egui::Painter, cam: &Camera, g: &Grid2D, colour: Color32) {
    let stroke = Stroke::new(1.0, colour.gamma_multiply(0.5));
    let p = |x: f32, y: f32, z: f32| {
        let (s, _) = cam.project([x, y, z]);
        Pos2::new(s[0], s[1])
    };
    let corners = [
        p(-0.5, -0.5, 0.0),
        p(0.5, -0.5, 0.0),
        p(0.5, 0.5, 0.0),
        p(-0.5, 0.5, 0.0),
    ];
    for i in 0..4 {
        painter.line_segment([corners[i], corners[(i + 1) % 4]], stroke);
    }
    painter.line_segment([corners[0], p(-0.5, -0.5, cam.height)], stroke);
    let font = egui::FontId::proportional(12.0);
    let label = |pos: Pos2, text: String, align: egui::Align2| {
        painter.text(pos, align, text, font.clone(), colour);
    };
    let fmt = |v: f64| {
        if v.abs() >= 100.0 {
            format!("{v:.0}")
        } else {
            format!("{v:.1}")
        }
    };
    let (x0, x1) = (
        g.x.first().copied().unwrap_or(0.0),
        g.x.last().copied().unwrap_or(0.0),
    );
    let (y0, y1) = (
        g.y.first().copied().unwrap_or(0.0),
        g.y.last().copied().unwrap_or(0.0),
    );
    label(
        p(0.0, -0.56, 0.0),
        format!("{} {}…{}", g.x_label, fmt(x0), fmt(x1)),
        egui::Align2::CENTER_CENTER,
    );
    label(
        p(0.56, 0.0, 0.0),
        format!("{} {}…{}", g.y_label, fmt(y0), fmt(y1)),
        egui::Align2::CENTER_CENTER,
    );
    label(
        p(-0.5, -0.5, cam.height + 0.04),
        g.z_label.clone(),
        egui::Align2::CENTER_BOTTOM,
    );
}

/// Motor harmonics and notch bands drawn on the surface.
fn overlay_lines(
    painter: &egui::Painter,
    cam: &Camera,
    g: &Grid2D,
    max: f64,
    t: &ThrottleHeatmapOutput,
    ov: &OverlayState,
) {
    let Some(o) = &t.overlay else {
        return;
    };
    let y_max = g.y.last().copied().unwrap_or(0.0);
    // a point just above the surface so the line is not hidden by it
    let on_surface = |x: f64, f: f64| -> Option<Pos2> {
        let z = value_at(g, x, f)?;
        let mp = model_point(g, x, f, z, max, cam.height);
        let (s, _) = cam.project([mp[0], mp[1], mp[2] + 0.01]);
        Some(Pos2::new(s[0], s[1]))
    };
    for (m, k, freqs) in overlay_harmonics(o, y_max, ov) {
        let stroke = Stroke::new(if k == 1 { 2.0 } else { 1.0 }, motor_color(m));
        let mut run: Vec<Pos2> = Vec::new();
        for (x, f) in t.grid.x.iter().zip(&freqs) {
            if let Some(p) = f.and_then(|f| on_surface(*x, f)) {
                run.push(p);
            } else {
                if run.len() > 1 {
                    painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
                }
                run.clear();
            }
        }
        if run.len() > 1 {
            painter.add(egui::Shape::line(run, stroke));
        }
    }
    if ov.notches {
        let band = |lo: f64, hi: f64, colour: Color32| {
            for f in [lo, hi] {
                if f <= 0.0 || f > y_max {
                    continue;
                }
                let pts: Vec<Pos2> = g.x.iter().filter_map(|x| on_surface(*x, f)).collect();
                if pts.len() > 1 {
                    painter.add(egui::Shape::line(pts, Stroke::new(1.0, colour)));
                }
            }
        };
        for (centre, cutoff) in &o.notches {
            let half = (centre - cutoff).abs();
            band(
                centre - half,
                centre + half,
                Color32::from_rgba_unmultiplied(255, 255, 255, 120),
            );
        }
        if let Some((lo, hi)) = o.dyn_notch_hz {
            band(lo, hi, Color32::from_rgba_unmultiplied(255, 200, 80, 160));
        }
        if let Some(f) = o.rpm_min_hz {
            band(f, f, Color32::from_rgba_unmultiplied(120, 220, 255, 160));
        }
    }
}
