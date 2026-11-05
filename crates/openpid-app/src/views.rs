//! The tabs. Every view reads analysis outputs from the app cache and draws
//! with `egui_plot`; nothing here computes.

use crate::app::{App, SlotState};
use crate::colormap::{ACCENT, MUTED, SLOT_A, SLOT_B};
use crate::prefs::{AccUnit, Prefs, RateUnit, Theme};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Heatmap, Legend, Line, Plot, PlotPoint, PlotPoints, VLine};
use openpid_analysis::Epoch;
use openpid_core::analysis::Grid2D;
use openpid_core::settings::{PerAxis, PidGains};
use openpid_core::units::Unit;
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage};
use std::sync::Arc;

/// Per-tab UI state that is not an analysis parameter.
#[derive(Debug, Default)]
pub struct ViewerState {
    /// Time (s) of the last click/tap on the log viewer plot.
    pub cursor_s: Option<f64>,
    pub channel_filter: String,
    /// Epoch handle (slot, `true` = start) under the pointer last frame.
    pub hover_handle: Option<(Slot, bool)>,
    /// Epoch handle being dragged.
    pub drag_handle: Option<(Slot, bool)>,
    /// Screen rectangle of the plot area last frame (for touch targets and tests).
    pub plot_frame: Option<egui::Rect>,
    /// Plot x range (seconds) last frame.
    pub x_bounds: Option<(f64, f64)>,
}

/// Pixel distance within which an epoch bound can be grabbed.
fn grab_radius(touch: bool) -> f32 {
    if touch { 22.0 } else { 8.0 }
}

/// Moves one epoch bound to `t` (clamped to the log), keeping start <= end.
fn set_epoch_bound(app: &mut App, slot: Slot, start: bool, t: f64) {
    let Some(l) = app.loaded_mut(slot) else {
        return;
    };
    let duration = l.current().map_or(t, |s| s.timebase.duration_s());
    let t = t.clamp(0.0, duration.max(0.0));
    let e = l.epoch.unwrap_or(Epoch {
        start_s: 0.0,
        end_s: duration,
    });
    l.epoch = Some(if start {
        Epoch {
            start_s: t.min(e.end_s),
            end_s: e.end_s,
        }
    } else {
        Epoch {
            start_s: e.start_s,
            end_s: t.max(e.start_s),
        }
    });
}

/// Shaded epoch regions and their draggable bounds; returns the handle under the pointer.
fn draw_epochs(pu: &mut egui_plot::PlotUi<'_>, app: &App, grab_px: f32) -> Option<(Slot, bool)> {
    let bounds = pu.plot_bounds();
    let (y0, y1) = (bounds.min()[1], bounds.max()[1]);
    let hover_pos = pu.response().hover_pos();
    let transform = *pu.transform();
    let mut hovered = None;
    for slot in [Slot::A, Slot::B] {
        let Some(e) = app.loaded(slot).and_then(|l| l.epoch) else {
            continue;
        };
        let fill = slot_color(slot).gamma_multiply(0.12);
        pu.polygon(
            egui_plot::Polygon::new(
                format!("{} epoch", slot.name()),
                PlotPoints::from(vec![
                    [e.start_s, y0],
                    [e.end_s, y0],
                    [e.end_s, y1],
                    [e.start_s, y1],
                ]),
            )
            .fill_color(fill)
            .stroke(egui::Stroke::NONE),
        );
        for (is_start, t) in [(true, e.start_s), (false, e.end_s)] {
            let near = hover_pos
                .is_some_and(|p| (transform.position_from_point_x(t) - p.x).abs() <= grab_px);
            let active = near || app.viewer.drag_handle == Some((slot, is_start));
            if near {
                hovered = Some((slot, is_start));
            }
            pu.vline(
                VLine::new(
                    format!(
                        "{} epoch {}",
                        slot.name(),
                        if is_start { "start" } else { "end" }
                    ),
                    t,
                )
                .color(slot_color(slot))
                .width(if active { 4.0 } else { 2.0 }),
            );
        }
    }
    hovered
}

fn slot_color(slot: Slot) -> Color32 {
    match slot {
        Slot::A => SLOT_A,
        Slot::B => SLOT_B,
    }
}

/// Min/max decimation so a line never carries more than ~2 points per pixel.
pub fn decimate(
    xs: &[f64],
    ys: &[f64],
    valid: impl Fn(usize) -> bool,
    max_points: usize,
) -> Vec<[f64; 2]> {
    let n = xs.len().min(ys.len());
    if n <= max_points {
        return (0..n)
            .filter(|&i| valid(i))
            .map(|i| [xs[i], ys[i]])
            .collect();
    }
    let bucket = n.div_ceil(max_points / 2);
    let mut out = Vec::with_capacity(max_points + 2);
    let mut i = 0;
    while i < n {
        let end = (i + bucket).min(n);
        let (mut lo, mut hi): (Option<usize>, Option<usize>) = (None, None);
        for j in i..end {
            if !valid(j) {
                continue;
            }
            if lo.is_none_or(|k| ys[j] < ys[k]) {
                lo = Some(j);
            }
            if hi.is_none_or(|k| ys[j] > ys[k]) {
                hi = Some(j);
            }
        }
        if let (Some(a), Some(b)) = (lo, hi) {
            let (first, second) = if a <= b { (a, b) } else { (b, a) };
            out.push([xs[first], ys[first]]);
            if second != first {
                out.push([xs[second], ys[second]]);
            }
        }
        i = end;
    }
    out
}

fn slot_header(app: &mut App, ui: &mut Ui, slot: Slot) -> bool {
    match app.slots.get(&slot) {
        Some(SlotState::Empty) | None => {
            ui.label(RichText::new(format!("Slot {}: empty", slot.name())).color(MUTED));
            false
        }
        Some(SlotState::Loading(p)) => {
            ui.spinner();
            ui.label(format!("Slot {}: loading {}", slot.name(), p.display()));
            false
        }
        Some(SlotState::Failed(p, e)) => {
            ui.colored_label(
                Color32::LIGHT_RED,
                format!("Slot {}: {} — {e}", slot.name(), p.display()),
            );
            false
        }
        Some(SlotState::Ready(_)) => {
            let mut changed = false;
            let loaded = app.loaded_mut(slot).expect("ready");
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Slot {}", slot.name()))
                        .color(slot_color(slot))
                        .strong(),
                );
                ui.label(
                    loaded
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
                let n = loaded.sessions.len();
                if n > 1 {
                    let before = loaded.session;
                    egui::ComboBox::from_id_salt(("session", slot.name()))
                        .selected_text(format!("session {}", loaded.session))
                        .show_ui(ui, |ui| {
                            for (i, s) in loaded.sessions.iter().enumerate() {
                                ui.selectable_value(
                                    &mut loaded.session,
                                    i,
                                    format!("session {i} ({:.1} s)", s.timebase.duration_s()),
                                );
                            }
                        });
                    if loaded.session != before {
                        loaded.epoch = None;
                        changed = true;
                    }
                }
                if let Some(s) = loaded.current() {
                    ui.label(format!(
                        "{} {}",
                        s.firmware.vendor.name(),
                        s.firmware
                            .version
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                    ));
                    ui.label(format!(
                        "{:.1} s @ {:.0} Hz",
                        s.timebase.duration_s(),
                        s.timebase.sample_rate_hz()
                    ));
                    if s.diagnostics.corrupt_frames > 0 {
                        ui.colored_label(
                            Color32::YELLOW,
                            format!("{} corrupt frames", s.diagnostics.corrupt_frames),
                        );
                    }
                }
                // epoch editor
                let duration = loaded.current().map_or(0.0, |s| s.timebase.duration_s());
                let mut e = loaded.epoch.unwrap_or(Epoch {
                    start_s: 0.0,
                    end_s: duration,
                });
                ui.label("epoch");
                let a = ui.add(
                    egui::DragValue::new(&mut e.start_s)
                        .speed(0.1)
                        .range(0.0..=duration)
                        .suffix(" s"),
                );
                ui.label("…");
                let b = ui.add(
                    egui::DragValue::new(&mut e.end_s)
                        .speed(0.1)
                        .range(0.0..=duration)
                        .suffix(" s"),
                );
                if a.changed() || b.changed() {
                    loaded.epoch = Some(Epoch {
                        start_s: e.start_s.min(e.end_s),
                        end_s: e.end_s.max(e.start_s),
                    });
                    changed = true;
                }
                if loaded.epoch.is_some() && ui.small_button("whole log").clicked() {
                    loaded.epoch = None;
                    changed = true;
                }
            });
            changed
        }
    }
}

fn axis_selector(ui: &mut Ui, axis: &mut Axis) -> bool {
    let before = *axis;
    ui.horizontal(|ui| {
        ui.label("Axis");
        for a in Axis::ALL {
            ui.selectable_value(axis, a, a.name());
        }
    });
    before != *axis
}

fn channel_picker(
    ui: &mut Ui,
    id: &str,
    session: Option<&FlightSession>,
    current: &mut ChannelId,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.key())
        .width(220.0)
        .show_ui(ui, |ui| {
            if let Some(s) = session {
                let mut keys: Vec<(String, ChannelId)> = s
                    .channels
                    .iter()
                    .map(|c| (c.id.key(), c.id.clone()))
                    .collect();
                keys.sort();
                for (k, id) in keys {
                    if ui.selectable_label(*current == id, k).clicked() {
                        *current = id;
                        changed = true;
                    }
                }
            }
        });
    changed
}

fn show_error(ui: &mut Ui, slot: Slot, e: &str) {
    ui.colored_label(Color32::LIGHT_RED, format!("{}: {e}", slot.name()));
}

/// Heatmap item for a grid; `max` = 0 picks a robust automatic colour maximum.
/// Cells at or below zero are transparent. `min_y_for_scale` excludes the
/// near-DC rows (stick motion) from the automatic scale so noise stays visible.
fn grid_heatmap(name: &str, g: &Grid2D, max: f64, min_y_for_scale: f64) -> Option<Heatmap> {
    if g.x.len() < 2 || g.y.len() < 2 {
        return None;
    }
    let dx = g.x[1] - g.x[0];
    let dy = g.y[1] - g.y[0];
    let max = if max > 0.0 {
        max
    } else {
        let nx = g.x.len();
        let rows: Vec<f64> =
            g.y.iter()
                .enumerate()
                .filter(|(_, y)| **y >= min_y_for_scale)
                .flat_map(|(iy, _)| g.z[iy * nx..(iy + 1) * nx].iter().copied())
                .collect();
        crate::colormap::robust_max(if rows.is_empty() { &g.z } else { &rows })
    };
    Some(
        Heatmap::new(g.z.clone(), g.x.len())
            .at(PlotPoint::new(g.x[0] - dx / 2.0, g.y[0] - dy / 2.0))
            .size(
                (dx * g.x.len() as f64) as f32,
                (dy * g.y.len() as f64) as f32,
            )
            .range(0.0, max)
            .custom_mapping(Box::new(move |v| crate::colormap::viridis(v / max)))
            .show_labels(false)
            .name(name),
    )
}

// ---------------------------------------------------------------------------

/// Channel toggles for the log viewer (slot A's channel list, filtered).
fn viewer_channel_picker(app: &mut App, ui: &mut Ui, session: Option<&FlightSession>) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Channels:");
        let filter = &mut app.viewer.channel_filter;
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("filter…")
                .desired_width(120.0),
        );
        let Some(s) = session else { return };
        let mut keys: Vec<ChannelId> = s.channels.iter().map(|c| c.id.clone()).collect();
        keys.sort_by_key(ChannelId::key);
        for id in keys {
            let k = id.key();
            if !filter.is_empty() && !k.contains(filter.as_str()) {
                continue;
            }
            let mut on = app.tools.viewer_channels.contains(&id);
            if ui.toggle_value(&mut on, k).changed() {
                if on {
                    app.tools.viewer_channels.push(id);
                } else {
                    app.tools.viewer_channels.retain(|c| *c != id);
                }
            }
        }
    });
}

/// Moves one slot's epoch boundary to the cursor time `t`.
fn set_epoch_from_cursor(app: &mut App, slot: Slot, t: f64, start: bool) {
    let Some(l) = app.loaded_mut(slot) else {
        return;
    };
    let current = l.epoch;
    l.epoch = Some(if start {
        let end = current.map_or(l.current().map_or(t, |s| s.timebase.duration_s()), |e| {
            e.end_s
        });
        Epoch {
            start_s: t.min(end),
            end_s: end.max(t),
        }
    } else {
        let start_s = current.map_or(0.0, |e| e.start_s);
        Epoch {
            start_s: start_s.min(t),
            end_s: t.max(start_s),
        }
    });
}

/// Cursor read-out and the "epoch from cursor" buttons.
fn viewer_cursor_controls(app: &mut App, ui: &mut Ui) {
    ui.horizontal(|ui| {
        let Some(t) = app.viewer.cursor_s else {
            ui.label(
                RichText::new(
                    "Click or tap the plot to place the cursor, then set the epoch from it.",
                )
                .color(MUTED),
            );
            return;
        };
        ui.label(format!("cursor: {t:.3} s"));
        for slot in [Slot::A, Slot::B] {
            if app.loaded(slot).is_none() {
                continue;
            }
            if ui
                .button(format!("{} start = cursor", slot.name()))
                .clicked()
            {
                set_epoch_from_cursor(app, slot, t, true);
            }
            if ui.button(format!("{} end = cursor", slot.name())).clicked() {
                set_epoch_from_cursor(app, slot, t, false);
            }
        }
    });
}

/// Scale factor and symbol for showing a channel in the preferred display units
/// (angular rates and accelerations; everything else is shown as logged).
pub fn display_unit(unit: &Unit, prefs: &Prefs) -> (f64, String) {
    match unit {
        Unit::DegreesPerSecond => {
            let (k, sym) = prefs.rate_unit.scale_deg_per_s();
            (k, sym.to_string())
        }
        Unit::Gs => {
            let (k, sym) = prefs.acc_unit.scale_g();
            (k, sym.to_string())
        }
        other => (1.0, other.to_string()),
    }
}

/// Draws one slot's selected channels (decimated to the plot width) and its epoch lines.
fn viewer_plot_slot(
    pu: &mut egui_plot::PlotUi<'_>,
    app: &App,
    slot: Slot,
    channels: &[ChannelId],
    n_lines: f32,
    px: usize,
) {
    let Some(l) = app.loaded(slot) else { return };
    let Some(s) = l.current() else { return };
    let t0 = s.timebase.time_us.first().copied().unwrap_or(0);
    let xs: Vec<f64> = s
        .timebase
        .time_us
        .iter()
        .map(|t| (t - t0) as f64 / 1.0e6)
        .collect();
    for (ci, id) in channels.iter().enumerate() {
        let Some(c) = s.channel(id) else { continue };
        let (scale, symbol) = display_unit(&c.unit, &app.prefs);
        let mut pts = decimate(&xs, c.data.values(), |i| c.data.is_valid(i), px * 2);
        if scale != 1.0 {
            for p in &mut pts {
                p[1] *= scale;
            }
        }
        let hue = (ci as f32 / n_lines + if slot == Slot::B { 0.5 } else { 0.0 }) % 1.0;
        let color: Color32 = egui::ecolor::Hsva::new(hue, 0.7, 0.95, 1.0).into();
        pu.line(
            Line::new(
                format!("{} {} ({symbol})", slot.name(), id.key()),
                PlotPoints::from(pts),
            )
            .color(color)
            .width(1.2),
        );
    }
}

pub fn log_viewer(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    let session_a: Option<Arc<FlightSession>> =
        app.loaded(Slot::A).and_then(|l| l.current().cloned());
    viewer_channel_picker(app, ui, session_a.as_deref());
    viewer_cursor_controls(app, ui);
    let touch = app.touch_mode;
    let channels = app.tools.viewer_channels.clone();
    let mut cursor = app.viewer.cursor_s;
    let plot_h = ui.available_height() - 4.0;
    let mut n_lines = channels.len().max(1) as f32;
    if app.loaded(Slot::B).is_some() {
        n_lines *= 2.0;
    }
    // a drag that starts on an epoch bound moves the bound, not the plot
    let prev_hover = app.viewer.hover_handle;
    let handle_interaction = prev_hover.is_some() || app.viewer.drag_handle.is_some();
    let mut hovered = None;
    let mut drag_to: Option<(Slot, bool, f64)> = None;
    let mut drag_started = false;
    let mut drag_stopped = false;
    let mut frame = None;
    let mut x_bounds = None;
    Plot::new("log_viewer")
        .legend(Legend::default())
        .x_axis_label("time (s)")
        .height(plot_h)
        .allow_boxed_zoom(!touch)
        .allow_drag(!handle_interaction)
        .show(ui, |pu| {
            let px = pu.response().rect.width().max(100.0) as usize;
            for slot in [Slot::A, Slot::B] {
                viewer_plot_slot(pu, app, slot, &channels, n_lines, px);
            }
            hovered = draw_epochs(pu, app, grab_radius(touch));
            if let Some(t) = cursor {
                pu.vline(VLine::new("cursor", t).color(ACCENT).width(1.5));
            }
            let resp = pu.response();
            drag_started = resp.drag_started();
            drag_stopped = resp.drag_stopped();
            // hover_pos() is unreliable mid-drag; interact_pointer_pos() follows the drag
            if let Some(handle) = app.viewer.drag_handle
                && resp.dragged()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let t = pu.transform().value_from_position(p).x;
                drag_to = Some((handle.0, handle.1, t));
            }
            if resp.clicked()
                && app.viewer.drag_handle.is_none()
                && let Some(p) = pu.pointer_coordinate()
            {
                cursor = Some(p.x);
            }
            let b = pu.plot_bounds();
            x_bounds = Some((b.min()[0], b.max()[0]));
            frame = Some(*pu.transform().frame());
        });
    app.viewer.cursor_s = cursor;
    app.viewer.hover_handle = hovered;
    app.viewer.plot_frame = frame;
    app.viewer.x_bounds = x_bounds;
    // the drag is recognised a few pixels after the press: use the handle hovered at the press
    if drag_started && let Some(h) = prev_hover.or(hovered) {
        app.viewer.drag_handle = Some(h);
    }
    if let Some((slot, start, t)) = drag_to {
        set_epoch_bound(app, slot, start, t);
    }
    if drag_stopped {
        app.viewer.drag_handle = None;
    }
}

pub fn step(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        axis_selector(ui, &mut app.tools.axis);
        app.tools.step.axis = app.tools.axis;
        ui.separator();
        ui.label("window");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.window_s)
                .speed(0.1)
                .range(0.5..=10.0)
                .suffix(" s"),
        );
        ui.label("cutoff");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.cutoff_hz)
                .speed(1.0)
                .range(5.0..=200.0)
                .suffix(" Hz"),
        );
        ui.label("min input");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.min_input)
                .speed(5.0)
                .range(0.0..=1000.0)
                .suffix(" °/s"),
        );
        ui.label("high input");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.high_input)
                .speed(10.0)
                .range(50.0..=2000.0)
                .suffix(" °/s"),
        );
        ui.label("smooth");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.smooth_ms)
                .speed(1.0)
                .range(0.0..=60.0)
                .suffix(" ms"),
        );
    });
    let params = app.tools.step.clone();
    let mut outputs: Vec<(Slot, Arc<Output>)> = Vec::new();
    for slot in [Slot::A, Slot::B] {
        match app.result(slot, Kind::Step(params.clone())) {
            Some(Ok(o)) => outputs.push((slot, o)),
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {}
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (slot, o) in &outputs {
            if let Output::Step(s) = o.as_ref() {
                let m = s.metrics;
                ui.label(
                    RichText::new(format!(
                        "{}: {} / {} windows · rise {} ms · delay {} ms · overshoot {} % · settling {} ms · final {:.2}",
                        slot.name(),
                        s.windows_used,
                        s.windows_total,
                        m.rise_time_ms.map_or("–".into(), |v| format!("{v:.0}")),
                        m.delay_ms.map_or("–".into(), |v| format!("{v:.0}")),
                        m.overshoot_pct.map_or("–".into(), |v| format!("{v:.0}")),
                        m.settling_ms.map_or("–".into(), |v| format!("{v:.0}")),
                        m.final_value
                    ))
                    .color(slot_color(*slot)),
                );
            }
        }
    });
    let h = ui.available_height();
    Plot::new("step")
        .legend(Legend::default())
        .x_axis_label("time (ms)")
        .y_axis_label("response")
        .height(h)
        .include_y(0.0)
        .include_y(1.5)
        .show(ui, |pu| {
            pu.hline(
                egui_plot::HLine::new("1.0", 1.0)
                    .color(MUTED)
                    .style(egui_plot::LineStyle::dashed_dense()),
            );
            for (slot, o) in &outputs {
                let Output::Step(s) = o.as_ref() else {
                    continue;
                };
                if *slot == Slot::A
                    && let Some(hm) = grid_heatmap("A responses", &s.histogram, 0.0, f64::MIN)
                {
                    pu.heatmap(hm);
                }
                let pts = |v: &[f64]| {
                    PlotPoints::from(
                        s.time_ms
                            .iter()
                            .zip(v)
                            .map(|(t, y)| [*t, *y])
                            .collect::<Vec<_>>(),
                    )
                };
                pu.line(
                    Line::new(format!("{} mean", slot.name()), pts(&s.mean))
                        .color(slot_color(*slot))
                        .width(2.5),
                );
                if let Some(l) = &s.low {
                    pu.line(
                        Line::new(format!("{} low input", slot.name()), pts(l))
                            .color(slot_color(*slot))
                            .width(1.0)
                            .style(egui_plot::LineStyle::dotted_dense()),
                    );
                }
                if let Some(hi) = &s.high {
                    pu.line(
                        Line::new(format!("{} high input", slot.name()), pts(hi))
                            .color(slot_color(*slot))
                            .width(1.0)
                            .style(egui_plot::LineStyle::dashed_dense()),
                    );
                }
            }
        });
}

pub fn spectrum(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    let session_a = app.loaded(Slot::A).and_then(|l| l.current().cloned());
    ui.horizontal_wrapped(|ui| {
        ui.label("Channel");
        channel_picker(
            ui,
            "psd_channel",
            session_a.as_deref(),
            &mut app.tools.spectrum_channel,
        );
        if axis_selector(ui, &mut app.tools.axis) {
            app.tools.spectrum_channel = ChannelId::Gyro {
                axis: app.tools.axis,
                stage: GyroStage::Filtered,
            };
        }
        ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.psd.segment_s)
                .speed(0.01)
                .range(0.02..=2.0)
                .suffix(" s"),
        );
        ui.label("max");
        let mut max = app.tools.psd.max_hz.unwrap_or(0.0);
        if ui
            .add(
                egui::DragValue::new(&mut max)
                    .speed(10.0)
                    .range(0.0..=8000.0)
                    .suffix(" Hz"),
            )
            .changed()
        {
            app.tools.psd.max_hz = if max > 0.0 { Some(max) } else { None };
        }
        ui.checkbox(&mut app.tools.psd_db, "dB");
    });
    app.tools.psd.channel = app.tools.spectrum_channel.clone();
    let params = app.tools.psd.clone();
    let mut outputs: Vec<(Slot, Arc<Output>)> = Vec::new();
    for slot in [Slot::A, Slot::B] {
        match app.result(slot, Kind::Psd(params.clone())) {
            Some(Ok(o)) => outputs.push((slot, o)),
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {}
        }
    }
    let db = app.tools.psd_db;
    let h = ui.available_height();
    Plot::new("psd")
        .legend(Legend::default())
        .x_axis_label("frequency (Hz)")
        .y_axis_label(if db { "dB" } else { "power / Hz" })
        .height(h)
        .show(ui, |pu| {
            for (slot, o) in &outputs {
                let Output::Psd(p) = o.as_ref() else { continue };
                let ys = if db { &p.power_db } else { &p.power };
                let pts: Vec<[f64; 2]> = p.freq.iter().zip(ys).map(|(f, y)| [*f, *y]).collect();
                pu.line(
                    Line::new(
                        format!(
                            "{} {} ({}², {} seg)",
                            slot.name(),
                            p.channel.key(),
                            p.unit,
                            p.segments
                        ),
                        PlotPoints::from(pts),
                    )
                    .color(slot_color(*slot))
                    .width(1.5),
                );
            }
        });
}

fn heatmap_controls(app: &mut App, ui: &mut Ui) {
    let session_a = app.loaded(Slot::A).and_then(|l| l.current().cloned());
    ui.horizontal_wrapped(|ui| {
        ui.label("Channel");
        channel_picker(
            ui,
            "hm_channel",
            session_a.as_deref(),
            &mut app.tools.heatmap_channel,
        );
        if axis_selector(ui, &mut app.tools.axis) {
            app.tools.heatmap_channel = ChannelId::Gyro {
                axis: app.tools.axis,
                stage: GyroStage::Filtered,
            };
        }
        ui.label("colour max (0 = auto)");
        ui.add(
            egui::DragValue::new(&mut app.tools.heatmap_max)
                .speed(0.5)
                .range(0.0..=10000.0),
        );
    });
}

pub fn throttle_heatmap(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    heatmap_controls(app, ui);
    ui.horizontal_wrapped(|ui| {
        ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.throttle.segment_s)
                .speed(0.01)
                .range(0.05..=1.0)
                .suffix(" s"),
        );
        ui.label("throttle window ±");
        ui.add(
            egui::DragValue::new(&mut app.tools.throttle.half_window)
                .speed(0.5)
                .range(0.0..=25.0)
                .suffix(" %"),
        );
        ui.label("max");
        ui.add(
            egui::DragValue::new(&mut app.tools.throttle.max_hz)
                .speed(10.0)
                .range(50.0..=8000.0)
                .suffix(" Hz"),
        );
    });
    app.tools.throttle.channel = app.tools.heatmap_channel.clone();
    let params = app.tools.throttle.clone();
    let max = app.tools.heatmap_max;
    let slots: Vec<Slot> = [Slot::A, Slot::B]
        .into_iter()
        .filter(|s| app.loaded(*s).is_some())
        .collect();
    let w = ui.available_width() / slots.len().max(1) as f32;
    let h = ui.available_height();
    ui.horizontal(|ui| {
        for slot in slots {
            ui.vertical(|ui| {
                ui.set_width(w - 8.0);
                match app.result(slot, Kind::Throttle(params.clone())) {
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
                            Plot::new(("thr", slot.name()))
                                .x_axis_label("throttle (%)")
                                .y_axis_label("frequency (Hz)")
                                .height(h - 24.0)
                                .show(ui, |pu| {
                                    if let Some(hm) = grid_heatmap("amplitude", &t.grid, max, 20.0)
                                    {
                                        pu.heatmap(hm);
                                    }
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
}

pub fn time_heatmap(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    heatmap_controls(app, ui);
    ui.horizontal_wrapped(|ui| {
        ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.stft.segment_s)
                .speed(0.01)
                .range(0.02..=1.0)
                .suffix(" s"),
        );
        let mut max = app.tools.stft.max_hz.unwrap_or(0.0);
        ui.label("max");
        if ui
            .add(
                egui::DragValue::new(&mut max)
                    .speed(10.0)
                    .range(0.0..=8000.0)
                    .suffix(" Hz"),
            )
            .changed()
        {
            app.tools.stft.max_hz = if max > 0.0 { Some(max) } else { None };
        }
    });
    app.tools.stft.channel = app.tools.heatmap_channel.clone();
    let params = app.tools.stft.clone();
    let max = app.tools.heatmap_max;
    let slots: Vec<Slot> = [Slot::A, Slot::B]
        .into_iter()
        .filter(|s| app.loaded(*s).is_some())
        .collect();
    let h = ui.available_height() / slots.len().max(1) as f32;
    for slot in slots {
        match app.result(slot, Kind::Stft(params.clone())) {
            Some(Ok(o)) => {
                if let Output::Stft(t) = o.as_ref() {
                    ui.label(
                        RichText::new(format!("{}: {} ({})", slot.name(), t.channel.key(), t.unit))
                            .color(slot_color(slot)),
                    );
                    Plot::new(("stft", slot.name()))
                        .x_axis_label("time (s)")
                        .y_axis_label("frequency (Hz)")
                        .height(h - 24.0)
                        .show(ui, |pu| {
                            if let Some(hm) = grid_heatmap("amplitude", &t.grid, max, 20.0) {
                                pu.heatmap(hm);
                            }
                        });
                }
            }
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {
                ui.spinner();
            }
        }
    }
}

pub fn diagnostics(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for slot in [Slot::A, Slot::B] {
            let Some(l) = app.loaded(slot) else { continue };
            let Some(s) = l.current() else { continue };
            ui.heading(format!("Slot {} — {}", slot.name(), s.firmware.revision));
            egui::Grid::new(("diag", slot.name()))
                .striped(true)
                .show(ui, |ui| {
                    ui.label("profile");
                    ui.label(&s.profile_id);
                    ui.end_row();
                    ui.label("samples");
                    ui.label(format!(
                        "{} over {:.2} s, median dt {} µs, {} gaps",
                        s.len(),
                        s.timebase.duration_s(),
                        s.timebase.median_dt_us,
                        s.timebase.gaps.len()
                    ));
                    ui.end_row();
                    ui.label("frames");
                    ui.label(
                        s.diagnostics
                            .frames
                            .iter()
                            .map(|(k, v)| format!("{k}:{}", v.valid))
                            .collect::<Vec<_>>()
                            .join("  "),
                    );
                    ui.end_row();
                    ui.label("corrupt / desync / skipped bytes");
                    ui.label(format!(
                        "{} / {} / {}",
                        s.diagnostics.corrupt_frames,
                        s.diagnostics.desync_frames,
                        s.diagnostics.skipped_bytes
                    ));
                    ui.end_row();
                    if let Some(pid) = &s.settings.pid {
                        ui.label("PID roll / pitch / yaw");
                        ui.label(format!(
                            "{}/{}/{}  {}/{}/{}  {}/{}/{}",
                            pid.roll.p,
                            pid.roll.i,
                            pid.roll.d,
                            pid.pitch.p,
                            pid.pitch.i,
                            pid.pitch.d,
                            pid.yaw.p,
                            pid.yaw.i,
                            pid.yaw.d
                        ));
                        ui.end_row();
                    }
                    if !s.diagnostics.unmapped_fields.is_empty() {
                        ui.label("unmapped fields");
                        ui.label(s.diagnostics.unmapped_fields.join(", "));
                        ui.end_row();
                    }
                    for n in &s.diagnostics.notes {
                        ui.label("note");
                        ui.label(n);
                        ui.end_row();
                    }
                });
            ui.collapsing(format!("All {} headers", s.raw_headers.len()), |ui| {
                egui::Grid::new(("headers", slot.name()))
                    .striped(true)
                    .show(ui, |ui| {
                        for (k, v) in &s.raw_headers {
                            ui.label(k);
                            ui.label(v);
                            ui.end_row();
                        }
                    });
            });
            ui.separator();
        }
    });
}
/// Slider state for the PID sliders tab.
#[derive(Debug, Default)]
pub struct SliderState {
    /// Slot the sliders were loaded from, with its session index.
    pub loaded_for: Option<(Slot, usize)>,
    /// Current slider positions keyed by firmware parameter name.
    pub values: openpid_core::SliderValues,
}

/// One row per slider: label, slider widget and the logged value.
fn slider_grid(app: &mut App, ui: &mut Ui, defs: &[openpid_core::SliderDef]) {
    let touch = app.touch_mode;
    egui::Grid::new("sliders")
        .num_columns(3)
        .spacing([16.0, if touch { 14.0 } else { 6.0 }])
        .show(ui, |ui| {
            for d in defs {
                let v = app
                    .slider_state
                    .values
                    .entry(d.key.clone())
                    .or_insert(d.value);
                ui.label(&d.label);
                let step = if d.max - d.min <= 2.0 { 1.0 } else { 5.0 };
                let mut slider = egui::Slider::new(v, d.min..=d.max)
                    .step_by(step)
                    .fixed_decimals(0);
                if touch {
                    slider = slider.trailing_fill(true);
                }
                ui.add_sized([360.0, if touch { 36.0 } else { 20.0 }], slider);
                let changed = (*v - d.value).abs() > f64::EPSILON;
                ui.label(
                    RichText::new(format!(
                        "log {}{}",
                        d.value,
                        if changed { "  ⟵ changed" } else { "" }
                    ))
                    .color(if changed { ACCENT } else { MUTED }),
                );
                ui.end_row();
            }
        });
}

/// `P I D F` plus D-min/D-max as one monospace line.
fn format_gains(g: &PidGains) -> String {
    format!(
        "P {:>3}  I {:>3}  D {:>3}  F {:>4}{}",
        g.p,
        g.i,
        g.d,
        g.f.map_or("-".into(), |v| v.to_string()),
        g.d_min
            .map(|v| format!("  Dmin {v}"))
            .or_else(|| g.d_max.map(|v| format!("  Dmax {v}")))
            .unwrap_or_default()
    )
}

/// Computed vs. logged gains per axis.
fn slider_gain_table(
    ui: &mut Ui,
    result: &openpid_core::SliderResult,
    logged: Option<PerAxis<PidGains>>,
) {
    egui::Grid::new("slider_gains")
        .num_columns(3)
        .striped(true)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            ui.label(RichText::new("axis").strong());
            ui.label(RichText::new("from sliders").strong());
            ui.label(RichText::new("logged").strong());
            ui.end_row();
            for (name, now, was) in [
                ("roll", result.pid.roll, logged.map(|l| l.roll)),
                ("pitch", result.pid.pitch, logged.map(|l| l.pitch)),
                ("yaw", result.pid.yaw, logged.map(|l| l.yaw)),
            ] {
                ui.label(name);
                let differs = was.is_some_and(|w| (w.p, w.i, w.d) != (now.p, now.i, now.d));
                ui.label(
                    RichText::new(format_gains(&now))
                        .monospace()
                        .color(if differs { ACCENT } else { SLOT_A }),
                );
                ui.label(
                    RichText::new(was.map_or("-".into(), |w| format_gains(&w)))
                        .monospace()
                        .color(MUTED),
                );
                ui.end_row();
            }
        });
}

/// Filter cut-offs from the sliders as one summary line (empty when no filter slider applies).
fn filter_summary(result: &openpid_core::SliderResult) -> String {
    let mut filters = Vec::new();
    if let Some((lo, hi)) = result.gyro_lpf1_hz {
        filters.push(format!("gyro lpf1 {lo}–{hi} Hz"));
    }
    if let Some(v) = result.gyro_lpf2_hz {
        filters.push(format!("gyro lpf2 {v} Hz"));
    }
    if let Some((lo, hi)) = result.dterm_lpf1_hz {
        filters.push(format!("dterm lpf1 {lo}–{hi} Hz"));
    }
    if let Some(v) = result.dterm_lpf2_hz {
        filters.push(format!("dterm lpf2 {v} Hz"));
    }
    filters.join("   ·   ")
}

pub fn sliders(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    let Some((session_idx, session)) = app
        .loaded(Slot::A)
        .and_then(|l| l.current().cloned().map(|s| (l.session, s)))
    else {
        ui.label(
            RichText::new("Load a Betaflight 4.3+ log into slot A to use the PID sliders.")
                .color(MUTED),
        );
        return;
    };
    let Some(model) = openpid_fmt_blackbox::slider_model_for(&session) else {
        ui.label(
            RichText::new(format!(
                "{}: no simplified-tuning headers in this log.",
                session.firmware.revision
            ))
            .color(MUTED),
        );
        return;
    };
    let defs = model.sliders();
    if app.slider_state.loaded_for != Some((Slot::A, session_idx)) {
        app.slider_state.loaded_for = Some((Slot::A, session_idx));
        app.slider_state.values = defs.iter().map(|d| (d.key.clone(), d.value)).collect();
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(model.id()).strong());
        ui.label(&session.firmware.revision);
        if ui.button("Reset to log").clicked() {
            app.slider_state.values = defs.iter().map(|d| (d.key.clone(), d.value)).collect();
        }
        if ui.button("Firmware defaults").clicked() {
            app.slider_state.values = defs.iter().map(|d| (d.key.clone(), d.default)).collect();
        }
    });
    slider_grid(app, ui, &defs);
    let result = model.compute(&app.slider_state.values);
    ui.separator();
    slider_gain_table(ui, &result, model.logged());
    let filters = filter_summary(&result);
    if !filters.is_empty() {
        ui.label(filters);
    }
    ui.separator();
    ui.label("Betaflight CLI to apply (select all, copy):");
    let mut cli = result.cli.join("\n");
    ui.add(
        egui::TextEdit::multiline(&mut cli)
            .code_editor()
            .desired_rows(6)
            .desired_width(f32::INFINITY)
            .interactive(true),
    );
}

// ---------------------------------------------------------------------------

/// Settings tab: theme, touch mode and display units, saved on every change.
pub fn settings(app: &mut App, ui: &mut Ui) {
    let mut changed = false;
    ui.heading("Appearance");
    ui.horizontal(|ui| {
        ui.label("Theme");
        changed |= ui
            .radio_value(&mut app.prefs.theme, Theme::Dark, "Dark")
            .changed();
        changed |= ui
            .radio_value(&mut app.prefs.theme, Theme::Light, "Light")
            .changed();
    });
    if ui
        .checkbox(
            &mut app.prefs.touch_mode,
            "Touch mode (larger controls, touch gestures)",
        )
        .changed()
    {
        app.touch_mode = app.prefs.touch_mode;
        changed = true;
    }
    ui.add_space(8.0);
    ui.heading("Units");
    ui.horizontal(|ui| {
        ui.label("Angular rate");
        changed |= ui
            .radio_value(&mut app.prefs.rate_unit, RateUnit::DegPerS, "deg/s")
            .changed();
        changed |= ui
            .radio_value(&mut app.prefs.rate_unit, RateUnit::RadPerS, "rad/s")
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("Acceleration");
        changed |= ui
            .radio_value(&mut app.prefs.acc_unit, AccUnit::G, "g")
            .changed();
        changed |= ui
            .radio_value(&mut app.prefs.acc_unit, AccUnit::MPerS2, "m/s²")
            .changed();
    });
    ui.label(
        RichText::new("Units apply to the log viewer traces; analyses stay in deg/s.").color(MUTED),
    );
    ui.add_space(8.0);
    if ui.button("Reset to defaults").clicked() {
        app.prefs = Prefs::default();
        app.touch_mode = app.prefs.touch_mode;
        changed = true;
    }
    ui.add_space(8.0);
    match &app.prefs_path {
        Some(p) => ui.label(RichText::new(format!("Saved in {}", p.display())).color(MUTED)),
        None => ui.label(
            RichText::new("No configuration directory: settings last until the app closes.")
                .color(MUTED),
        ),
    };
    if changed {
        app.save_prefs();
    }
}

#[cfg(test)]
mod tests {
    use super::{decimate, filter_summary, format_gains};
    use openpid_core::SliderResult;
    use openpid_core::settings::{PerAxis, PidGains};

    #[test]
    fn decimation_keeps_extremes() {
        let xs: Vec<f64> = (0..1000).map(f64::from).collect();
        let ys: Vec<f64> = xs.iter().map(|x| (x / 7.0).sin() * 10.0).collect();
        let d = decimate(&xs, &ys, |_| true, 100);
        assert!(d.len() <= 102 && d.len() >= 50);
        let max = d.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        let min = d.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        assert!(max > 9.9 && min < -9.9);
        assert!(d.windows(2).all(|w| w[1][0] >= w[0][0]), "x monotonic");
        let small = decimate(&xs[..10], &ys[..10], |i| i != 3, 100);
        assert_eq!(small.len(), 9);
    }

    #[test]
    fn gain_and_filter_formatting() {
        let g = PidGains {
            p: 45.0,
            i: 80.0,
            d: 40.0,
            f: Some(120.0),
            d_min: Some(30.0),
            d_max: None,
        };
        assert_eq!(format_gains(&g), "P  45  I  80  D  40  F  120  Dmin 30");
        let g2 = PidGains {
            f: None,
            d_min: None,
            d_max: Some(46.0),
            ..g
        };
        assert!(format_gains(&g2).ends_with("F    -  Dmax 46"));
        let r = SliderResult {
            pid: PerAxis::default(),
            gyro_lpf1_hz: Some((250.0, 500.0)),
            gyro_lpf2_hz: None,
            dterm_lpf1_hz: None,
            dterm_lpf2_hz: Some(150.0),
            cli: Vec::new(),
        };
        assert_eq!(
            filter_summary(&r),
            "gyro lpf1 250–500 Hz   ·   dterm lpf2 150 Hz"
        );
        let none = SliderResult {
            gyro_lpf1_hz: None,
            dterm_lpf2_hz: None,
            ..r
        };
        assert!(filter_summary(&none).is_empty());
    }
}
