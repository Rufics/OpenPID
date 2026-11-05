//! The tabs. Every view reads analysis outputs from the app cache and draws
//! with `egui_plot`; nothing here computes.

use crate::app::{App, Loaded, SlotState};
use crate::colormap::{ACCENT, MUTED, SLOT_A, SLOT_B};
use crate::prefs::{AccUnit, Prefs, RateUnit, Theme};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{
    Bar, BarChart, HLine, Heatmap, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, Text,
    VLine,
};
use openpid_analysis::{Epoch, FilterOverrides, RpmOverlay};
use openpid_core::analysis::Grid2D;
use openpid_core::settings::LowpassKind;
use openpid_core::settings::{PerAxis, PidGains};
use openpid_core::units::Unit;
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage};
use openpid_dsp::Peak;
use openpid_sim::{ChainModel, LowpassModel};
use std::sync::Arc;

/// Per-tab UI state that is not an analysis parameter.
#[derive(Debug, Default)]
pub struct ViewerState {
    /// Time (s) of the last click, tap or long press on the log viewer plot.
    pub cursor_s: Option<f64>,
    /// Vertical range of the last drawn log-viewer plot.
    pub y_bounds: Option<(f64, f64)>,
    /// A finger resting on the plot: when it started (s) and where, for the long press.
    pub press: Option<(f64, egui::Pos2)>,
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

/// Markers and peak settings of the spectrum tab.
#[derive(Debug)]
pub struct SpectrumState {
    /// Frequencies (Hz) the user marked by clicking the plot.
    pub markers: Vec<f64>,
    /// How many automatic peak markers per trace (0 = off).
    pub auto_peaks: usize,
    /// Minimum prominence (dB) for an automatic peak.
    pub min_prominence_db: f64,
    /// Peaks found on the last frame, per slot (for the read-out and tests).
    pub peaks: Vec<(Slot, Vec<Peak>)>,
    /// Screen rectangle of the plot area last frame.
    pub plot_frame: Option<egui::Rect>,
    /// Plot x range (Hz) last frame.
    pub x_bounds: Option<(f64, f64)>,
}

impl Default for SpectrumState {
    fn default() -> Self {
        SpectrumState {
            markers: Vec::new(),
            auto_peaks: 3,
            min_prominence_db: 6.0,
            peaks: Vec::new(),
            plot_frame: None,
            x_bounds: None,
        }
    }
}

/// Playback over the STFT columns of the freq × time heatmap (#30): the
/// position is the log-viewer cursor, so the two tabs stay in step.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    /// Advancing every frame.
    pub playing: bool,
    /// Playback speed relative to real time.
    pub speed: f64,
    /// Current position, session seconds (`None` until a log is shown).
    pub pos_s: Option<f64>,
    /// `egui` time of the last playing frame, for the elapsed step.
    pub last_frame_time: Option<f64>,
    /// The viewer cursor the player last read or wrote; a different value
    /// means the user moved the cursor elsewhere.
    pub synced_cursor: Option<f64>,
    /// Column of slot A's grid at the position (for the read-out and tests).
    pub column: Option<usize>,
    /// Screen rectangle of slot A's heatmap last frame.
    pub plot_frame: Option<egui::Rect>,
    /// Plot x range (seconds) of slot A's heatmap last frame.
    pub x_bounds: Option<(f64, f64)>,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState {
            playing: false,
            speed: 1.0,
            pos_s: None,
            last_frame_time: None,
            synced_cursor: None,
            column: None,
            plot_frame: None,
            x_bounds: None,
        }
    }
}

/// Index of the grid column nearest to `t` (`None` for an empty axis).
pub(crate) fn nearest_column(x: &[f64], t: f64) -> Option<usize> {
    x.iter()
        .enumerate()
        .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))
        .map(|(i, _)| i)
}

/// `123 Hz` / `1.23 kHz`.
fn hz(f: f64) -> String {
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{f:.1} Hz")
    }
}

/// Frequency bin of `freq` closest to `x`.
fn nearest_bin(freq: &[f64], x: f64) -> Option<usize> {
    freq.iter()
        .enumerate()
        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
        .map(|(i, _)| i)
}

pub(crate) fn slot_color(slot: Slot) -> Color32 {
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

pub(crate) fn slot_header(app: &mut App, ui: &mut Ui, slot: Slot) -> bool {
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
                    let l = ui.label("session");
                    let combo = egui::ComboBox::from_id_salt(("session", slot.name()))
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
                    combo.response.labelled_by(l.id);
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
    let l = ui.label("channel");
    let combo = egui::ComboBox::from_id_salt(id)
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
    combo.response.labelled_by(l.id);
    changed
}

pub(crate) fn show_error(ui: &mut Ui, slot: Slot, e: &str) {
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
        let l = ui.label("filter");
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("filter…")
                .desired_width(120.0),
        )
        .labelled_by(l.id);
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
    // many channels in touch mode would wrap over the whole panel: cap the
    // picker and let it scroll, the plot keeps its height
    egui::ScrollArea::vertical()
        .id_salt("viewer_channels")
        .max_height(if app.touch_mode { 150.0 } else { 180.0 })
        .show(ui, |ui| {
            viewer_channel_picker(app, ui, session_a.as_deref())
        });
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
    let mut y_bounds = None;
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
            // a click, or a long press on a touch screen, places the cursor;
            // hover_pos() is gone once egui counts the press as a drag, so the
            // long press reads the interact position instead
            if resp.clicked()
                && app.viewer.drag_handle.is_none()
                && let Some(p) = pu.pointer_coordinate()
            {
                cursor = Some(p.x);
            }
            if resp.long_touched()
                && app.viewer.drag_handle.is_none()
                && let Some(p) = resp.interact_pointer_pos()
            {
                cursor = Some(pu.transform().value_from_position(p).x);
            }
            // in touch mode a finger held still for a moment places the cursor too
            // (not every backend reports long touches)
            if touch && app.viewer.drag_handle.is_none() {
                let now = pu.ctx().input(|i| i.time);
                let down = resp.is_pointer_button_down_on();
                match (app.viewer.press, resp.interact_pointer_pos()) {
                    (None, Some(p)) if down => app.viewer.press = Some((now, p)),
                    (Some((t0, p0)), Some(p)) if down => {
                        if p.distance(p0) > LONG_PRESS_SLOP_PX {
                            app.viewer.press = None; // it is a pan
                        } else if now - t0 >= LONG_PRESS_S {
                            cursor = Some(pu.transform().value_from_position(p0).x);
                            app.viewer.press = None;
                        }
                    }
                    _ => app.viewer.press = None,
                }
            }
            let b = pu.plot_bounds();
            x_bounds = Some((b.min()[0], b.max()[0]));
            y_bounds = Some((b.min()[1], b.max()[1]));
            frame = Some(*pu.transform().frame());
        });
    app.viewer.cursor_s = cursor;
    app.viewer.hover_handle = hovered;
    app.viewer.plot_frame = frame;
    app.viewer.x_bounds = x_bounds;
    app.viewer.y_bounds = y_bounds;
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
        let l = ui.label("window");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.window_s)
                .speed(0.1)
                .range(0.5..=10.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
        let l = ui.label("cutoff");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.cutoff_hz)
                .speed(1.0)
                .range(5.0..=200.0)
                .suffix(" Hz"),
        )
        .labelled_by(l.id);
        let l = ui.label("min input");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.min_input)
                .speed(5.0)
                .range(0.0..=1000.0)
                .suffix(" °/s"),
        )
        .labelled_by(l.id);
        let l = ui.label("high input");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.high_input)
                .speed(10.0)
                .range(50.0..=2000.0)
                .suffix(" °/s"),
        )
        .labelled_by(l.id);
        let l = ui.label("smooth");
        ui.add(
            egui::DragValue::new(&mut app.tools.step.smooth_ms)
                .speed(1.0)
                .range(0.0..=60.0)
                .suffix(" ms"),
        )
        .labelled_by(l.id);
        ui.checkbox(&mut app.tools.step.y_offset, "Y-offset")
            .on_hover_text("normalise every window on its settled tail before averaging");
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
                        "{}: {} / {} windows · rise {} ms · delay (50 %) {} ms · latency (xcorr) {} ms · overshoot {} % · settling {} ms · final {:.2}",
                        slot.name(),
                        s.windows_used,
                        s.windows_total,
                        m.rise_time_ms.map_or("–".into(), |v| format!("{v:.0}")),
                        m.delay_ms.map_or("–".into(), |v| format!("{v:.0}")),
                        m.latency_ms.map_or("–".into(), |v| format!("{v:.1}")),
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
                HLine::new("1.0", 1.0)
                    .color(MUTED)
                    .style(LineStyle::dashed_dense()),
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
                            .style(LineStyle::dotted_dense()),
                    );
                }
                if let Some(hi) = &s.high {
                    pu.line(
                        Line::new(format!("{} high input", slot.name()), pts(hi))
                            .color(slot_color(*slot))
                            .width(1.0)
                            .style(LineStyle::dashed_dense()),
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
        let l = ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.psd.segment_s)
                .speed(0.01)
                .range(0.02..=2.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
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
        ui.separator();
        let l = ui.label("peaks");
        ui.add(egui::DragValue::new(&mut app.spectrum.auto_peaks).range(0..=10))
            .labelled_by(l.id);
        let l = ui.label("prominence");
        ui.add(
            egui::DragValue::new(&mut app.spectrum.min_prominence_db)
                .speed(0.5)
                .range(0.0..=60.0)
                .suffix(" dB"),
        )
        .labelled_by(l.id);
        if !app.spectrum.markers.is_empty() && ui.button("Clear markers").clicked() {
            app.spectrum.markers.clear();
        }
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
    // automatic peaks per trace (prominence is always judged in dB)
    let peaks: Vec<(Slot, Vec<Peak>)> = outputs
        .iter()
        .filter_map(|(slot, o)| match o.as_ref() {
            Output::Psd(p) => Some((
                *slot,
                p.peaks(app.spectrum.auto_peaks, app.spectrum.min_prominence_db),
            )),
            _ => None,
        })
        .collect();
    if let Some(txt) = marker_readout(&app.spectrum.markers, &peaks) {
        ui.label(RichText::new(txt).monospace());
    }
    let markers = app.spectrum.markers.clone();
    let mut clicked_at: Option<f64> = None;
    let mut frame = None;
    let mut x_bounds = None;
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
                let Some((_, pk)) = peaks.iter().find(|(s, _)| s == slot) else {
                    continue;
                };
                for peak in pk {
                    let y = ys.get(peak.index).copied().unwrap_or(peak.y);
                    pu.vline(
                        VLine::new(format!("{} peak {}", slot.name(), hz(peak.x)), peak.x)
                            .color(slot_color(*slot).gamma_multiply(0.6))
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                    pu.text(
                        Text::new(
                            format!("{} peak label {}", slot.name(), hz(peak.x)),
                            PlotPoint::new(peak.x, y),
                            format!("{} {:+.1} dB", hz(peak.x), peak.y),
                        )
                        .color(slot_color(*slot))
                        .anchor(egui::Align2::LEFT_BOTTOM),
                    );
                }
            }
            for (i, m) in markers.iter().enumerate() {
                pu.vline(
                    VLine::new(format!("marker {}", i + 1), *m)
                        .color(ACCENT)
                        .width(1.5),
                );
            }
            let resp = pu.response();
            if resp.clicked()
                && let Some(p) = pu.pointer_coordinate()
            {
                clicked_at = Some(p.x);
            }
            let b = pu.plot_bounds();
            x_bounds = Some((b.min()[0], b.max()[0]));
            frame = Some(*pu.transform().frame());
        });
    app.spectrum.peaks = peaks;
    app.spectrum.plot_frame = frame;
    app.spectrum.x_bounds = x_bounds;
    if let Some(x) = clicked_at
        && x >= 0.0
    {
        // snap to the nearest bin of the first trace so the read-out shows a real value
        let snapped = outputs
            .iter()
            .find_map(|(_, o)| match o.as_ref() {
                Output::Psd(p) => nearest_bin(&p.freq, x).map(|i| p.freq[i]),
                _ => None,
            })
            .unwrap_or(x);
        app.spectrum.markers.push(snapped);
    }
}

/// Manual spectrum markers as horizontal lines on a frequency-axis heatmap.
fn marker_hlines(pu: &mut egui_plot::PlotUi<'_>, markers: &[f64]) {
    for (i, m) in markers.iter().enumerate() {
        pu.hline(
            HLine::new(format!("marker {}", i + 1), *m)
                .color(ACCENT)
                .style(LineStyle::dashed_loose())
                .width(1.5),
        );
    }
}

/// Text line listing manual markers (with the delta of the last two) and the top peak per slot.
fn marker_readout(markers: &[f64], peaks: &[(Slot, Vec<Peak>)]) -> Option<String> {
    let mut parts = Vec::new();
    if !markers.is_empty() {
        let list: Vec<String> = markers.iter().map(|m| hz(*m)).collect();
        let delta = if let [.., a, b] = markers {
            format!("  Δ {}", hz((b - a).abs()))
        } else {
            String::new()
        };
        parts.push(format!("markers: {}{delta}", list.join(", ")));
    }
    for (slot, pk) in peaks {
        if let Some(top) = pk.first() {
            parts.push(format!(
                "{} top peak {} ({:+.1} dB, prominence {:.1} dB)",
                slot.name(),
                hz(top.x),
                top.y,
                top.prominence
            ));
        }
    }
    (!parts.is_empty()).then(|| parts.join("   ·   "))
}

pub(crate) fn heatmap_controls(app: &mut App, ui: &mut Ui) {
    let session_a = app.loaded(Slot::A).and_then(|l| l.current().cloned());
    ui.horizontal_wrapped(|ui| {
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
        let l = ui.label("colour max (0 = auto)");
        ui.add(
            egui::DragValue::new(&mut app.tools.heatmap_max)
                .speed(0.5)
                .range(0.0..=10000.0),
        )
        .labelled_by(l.id);
    });
}

/// Height of the segments-per-bin bar under the freq × throttle heatmap.
const COUNT_BAR_HEIGHT: f32 = 56.0;

/// Textures kept before the cache is emptied (one per result and colour scale).
const MAX_HEATMAP_TEXTURES: usize = 32;

/// A heatmap grid as a texture, rasterised once per result and colour scale
/// and redrawn as one image instead of a cell mesh every frame. `owner` is
/// the result the grid belongs to (its address keys the texture while it is
/// cached). Returns the image item positioned over the grid.
fn grid_image(
    app: &mut App,
    ctx: &egui::Context,
    owner: &Arc<Output>,
    g: &Grid2D,
    max: f64,
    min_y_for_scale: f64,
) -> Option<egui_plot::PlotImage> {
    use std::hash::{Hash, Hasher};
    if g.x.len() < 2 || g.y.len() < 2 {
        return None;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (Arc::as_ptr(owner) as usize).hash(&mut hasher);
    max.to_bits().hash(&mut hasher);
    min_y_for_scale.to_bits().hash(&mut hasher);
    let key = hasher.finish();
    if !app.heatmap_textures.contains_key(&key) {
        if app.heatmap_textures.len() >= MAX_HEATMAP_TEXTURES {
            app.heatmap_textures.clear();
        }
        let (w, h, rgba) = openpid_analysis::render::grid_rgba(g, max, min_y_for_scale);
        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let handle = ctx.load_texture(
            format!("heatmap-{key:x}"),
            image,
            egui::TextureOptions::NEAREST,
        );
        app.heatmap_textures.insert(key, handle);
    }
    let tex = &app.heatmap_textures[&key];
    let dx = g.x[1] - g.x[0];
    let dy = g.y[1] - g.y[0];
    let (x0, x1) = (g.x[0] - dx / 2.0, g.x[g.x.len() - 1] + dx / 2.0);
    let (y0, y1) = (g.y[0] - dy / 2.0, g.y[g.y.len() - 1] + dy / 2.0);
    Some(
        egui_plot::PlotImage::new(
            "amplitude",
            tex.id(),
            PlotPoint::new(f64::midpoint(x0, x1), f64::midpoint(y0, y1)),
            egui::vec2((x1 - x0) as f32, (y1 - y0) as f32),
        )
        .allow_hover(false),
    )
}

/// How long a finger must rest on the log viewer to place the cursor, seconds.
const LONG_PRESS_S: f64 = 0.6;
/// Movement that turns a resting finger into a pan, logical pixels.
const LONG_PRESS_SLOP_PX: f32 = 8.0;

/// A slot's analysis result: `None` while it is still computing.
pub(crate) type SlotResult = (Slot, Option<Result<Arc<Output>, String>>);

/// What of the RPM / notch overlay is drawn on the freq × throttle heatmap.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayState {
    /// Draw the motor harmonics at all.
    pub show: bool,
    /// Per motor (index order); a motor past the end is shown.
    pub motors: Vec<bool>,
    /// Harmonics to draw; 0 = as many as the firmware's RPM filter removes.
    pub harmonics: u32,
    /// Draw the static notches, the dynamic-notch band and the RPM filter minimum.
    pub notches: bool,
}

impl Default for OverlayState {
    fn default() -> Self {
        OverlayState {
            show: true,
            motors: Vec::new(),
            harmonics: 0,
            notches: true,
        }
    }
}

pub(crate) fn motor_color(m: usize) -> Color32 {
    let hue = (m as f32 * 0.23 + 0.08) % 1.0;
    egui::ecolor::Hsva::new(hue, 0.85, 1.0, 1.0).into()
}

/// RPM overlay controls (shown once the motor count of a result is known);
/// shared by the freq × throttle heatmap and the 3D surface.
pub(crate) fn overlay_controls(app: &mut App, ui: &mut Ui, results: &[SlotResult]) {
    let motors = results
        .iter()
        .filter_map(|(_, r)| match r {
            Some(Ok(o)) => match o.as_ref() {
                Output::Throttle(t) => t.overlay.as_ref().map(|ov| ov.motors.len()),
                _ => None,
            },
            _ => None,
        })
        .max();
    if let Some(n) = motors {
        let ov = &mut app.overlay;
        ov.motors.resize(n, true);
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut ov.show, "RPM overlay");
            for (m, on) in ov.motors.iter_mut().enumerate() {
                ui.checkbox(on, format!("M{}", m + 1));
            }
            let l = ui.label("harmonics");
            ui.add(egui::DragValue::new(&mut ov.harmonics).range(0..=8))
                .labelled_by(l.id)
                .on_hover_text("0 = as many as the RPM filter removes");
            ui.checkbox(&mut ov.notches, "notches");
        });
    }
}

/// Harmonic frequencies to draw for `o` under `ov`: `(motor, harmonic, per-x Hz)`
/// with `None` where the bin has no motor speed or the harmonic is above `y_max`.
pub(crate) fn overlay_harmonics(
    o: &RpmOverlay,
    y_max: f64,
    ov: &OverlayState,
) -> Vec<(usize, u32, Vec<Option<f64>>)> {
    if !ov.show {
        return Vec::new();
    }
    let harmonics = if ov.harmonics == 0 {
        o.harmonics
    } else {
        ov.harmonics
    };
    let mut out = Vec::new();
    for (m, bins) in o.motors.iter().enumerate() {
        if !ov.motors.get(m).copied().unwrap_or(true) {
            continue;
        }
        for k in 1..=harmonics {
            let f: Vec<Option<f64>> = bins
                .iter()
                .map(|rpm| rpm.map(|r| r / 60.0 * f64::from(k)).filter(|f| *f <= y_max))
                .collect();
            out.push((m, k, f));
        }
    }
    out
}

/// The motor harmonics and notch bands of `o` over a heatmap whose frequency axis ends at `y_max`.
fn draw_rpm_overlay(
    pu: &mut egui_plot::PlotUi<'_>,
    o: &RpmOverlay,
    x: &[f64],
    y_max: f64,
    ov: &OverlayState,
) {
    if ov.show {
        let harmonics = if ov.harmonics == 0 {
            o.harmonics
        } else {
            ov.harmonics
        };
        for (m, bins) in o.motors.iter().enumerate() {
            if !ov.motors.get(m).copied().unwrap_or(true) {
                continue;
            }
            for k in 1..=harmonics {
                // one line per run of consecutive bins with a motor speed
                let mut run: Vec<[f64; 2]> = Vec::new();
                let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
                for (xi, rpm) in x.iter().zip(bins) {
                    match rpm.map(|r| r / 60.0 * f64::from(k)).filter(|f| *f <= y_max) {
                        Some(f) => run.push([*xi, f]),
                        None if !run.is_empty() => runs.push(std::mem::take(&mut run)),
                        None => {}
                    }
                }
                if !run.is_empty() {
                    runs.push(run);
                }
                for pts in runs {
                    let mut line = Line::new(format!("M{} ×{k}", m + 1), PlotPoints::from(pts))
                        .color(motor_color(m))
                        .width(if k == 1 { 1.8 } else { 1.0 });
                    if k > 1 {
                        line = line.style(LineStyle::dashed_dense());
                    }
                    pu.line(line);
                }
            }
        }
    }
    if ov.notches {
        let (x0, x1) = (
            x.first().copied().unwrap_or(0.0),
            x.last().copied().unwrap_or(100.0),
        );
        let band =
            |pu: &mut egui_plot::PlotUi<'_>, name: String, lo: f64, hi: f64, colour: Color32| {
                let (lo, hi) = (lo.max(0.0), hi.min(y_max));
                if hi <= lo {
                    return;
                }
                pu.polygon(
                    egui_plot::Polygon::new(
                        name,
                        PlotPoints::from(vec![[x0, lo], [x1, lo], [x1, hi], [x0, hi]]),
                    )
                    .fill_color(colour)
                    .stroke(egui::Stroke::NONE),
                );
            };
        for (i, (centre, cutoff)) in o.notches.iter().enumerate() {
            let half = (centre - cutoff).abs();
            band(
                pu,
                format!("notch {} band", i + 1),
                centre - half,
                centre + half,
                Color32::from_rgba_unmultiplied(255, 255, 255, 40),
            );
            pu.hline(
                HLine::new(format!("notch {} {}", i + 1, hz(*centre)), *centre)
                    .color(Color32::WHITE)
                    .style(LineStyle::dashed_loose())
                    .width(1.0),
            );
        }
        if let Some((lo, hi)) = o.dyn_notch_hz {
            // an outline, not a fill: the band spans most of the useful range
            // and a tint over it would wash out the heatmap
            band(
                pu,
                "dyn notch band".into(),
                lo,
                hi,
                ACCENT.gamma_multiply(0.04),
            );
            for (name, f) in [("dyn notch min", lo), ("dyn notch max", hi)] {
                if f < y_max {
                    pu.hline(
                        HLine::new(format!("{name} {}", hz(f)), f)
                            .color(ACCENT)
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                }
            }
        }
        if let Some(min) = o.rpm_min_hz {
            pu.hline(
                HLine::new(format!("RPM filter min {}", hz(min)), min)
                    .color(MUTED)
                    .style(LineStyle::dotted_dense())
                    .width(1.0),
            );
        }
    }
}

/// Grey columns over the throttle bins with fewer than `min_segments`
/// segments, so an unsampled bin does not read as a quiet one.
fn sparse_bins(
    g: &Grid2D,
    counts: &[usize],
    min_segments: usize,
) -> Vec<egui_plot::Polygon<'static>> {
    if g.x.len() < 2 || g.y.len() < 2 {
        return Vec::new();
    }
    let dx = g.x[1] - g.x[0];
    let dy = g.y[1] - g.y[0];
    let (y0, y1) = (g.y[0] - dy / 2.0, g.y[g.y.len() - 1] + dy / 2.0);
    let grey = Color32::from_rgba_unmultiplied(128, 128, 128, 170);
    g.x.iter()
        .zip(counts)
        .filter(|(_, c)| **c < min_segments)
        .map(|(&x, _)| {
            let (x0, x1) = (x - dx / 2.0, x + dx / 2.0);
            egui_plot::Polygon::new(
                "unsampled",
                PlotPoints::from(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]),
            )
            .fill_color(grey)
            .stroke(egui::Stroke::NONE)
        })
        .collect()
}

/// Thin bar chart of the segment count per throttle bin, x-linked to the heatmap.
fn segment_count_bar(
    ui: &mut Ui,
    slot: Slot,
    link: egui::Id,
    g: &Grid2D,
    counts: &[usize],
    min_segments: usize,
) {
    let dx = if g.x.len() >= 2 { g.x[1] - g.x[0] } else { 1.0 };
    let bars: Vec<Bar> =
        g.x.iter()
            .zip(counts)
            .map(|(&x, &c)| {
                Bar::new(x, c as f64).width(dx).fill(if c < min_segments {
                    Color32::GRAY
                } else {
                    slot_color(slot)
                })
            })
            .collect();
    Plot::new(("thr-count", slot.name()))
        .x_axis_label("throttle (%)")
        .y_axis_label("segments")
        .link_axis(link, [true, false])
        .height(COUNT_BAR_HEIGHT)
        .include_y(0.0)
        .allow_zoom([true, false])
        .allow_drag([true, false])
        .show(ui, |pu| {
            pu.bar_chart(
                BarChart::new("segments per bin", bars).element_formatter(Box::new(|b, _| {
                    format!("throttle {:.0} %: {} segments", b.argument, b.value)
                })),
            );
        });
}

pub fn throttle_heatmap(app: &mut App, ui: &mut Ui) {
    let markers = app.spectrum.markers.clone();
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    heatmap_controls(app, ui);
    ui.horizontal_wrapped(|ui| {
        let l = ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.throttle.segment_s)
                .speed(0.01)
                .range(0.05..=1.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
        let l = ui.label("throttle window ±");
        ui.add(
            egui::DragValue::new(&mut app.tools.throttle.half_window)
                .speed(0.5)
                .range(0.0..=25.0)
                .suffix(" %"),
        )
        .labelled_by(l.id);
        let l = ui.label("max");
        ui.add(
            egui::DragValue::new(&mut app.tools.throttle.max_hz)
                .speed(10.0)
                .range(50.0..=8000.0)
                .suffix(" Hz"),
        )
        .labelled_by(l.id);
        let l = ui.label("min segments");
        ui.add(
            egui::DragValue::new(&mut app.tools.min_segments)
                .speed(1.0)
                .range(1..=1000),
        )
        .on_hover_text(
            "throttle bins with fewer segments are greyed out: they are unsampled, not quiet",
        )
        .labelled_by(l.id);
    });
    app.tools.throttle.channel = app.tools.heatmap_channel.clone();
    let params = app.tools.throttle.clone();
    let max = app.tools.heatmap_max;
    let min_segments = app.tools.min_segments;
    let slots: Vec<Slot> = [Slot::A, Slot::B]
        .into_iter()
        .filter(|s| app.loaded(*s).is_some())
        .collect();
    let results: Vec<SlotResult> = slots
        .iter()
        .map(|slot| (*slot, app.result(*slot, Kind::Throttle(params.clone()))))
        .collect();
    overlay_controls(app, ui, &results);
    let overlay = app.overlay.clone();
    let w = ui.available_width() / slots.len().max(1) as f32;
    let h = ui.available_height();
    ui.horizontal(|ui| {
        for (slot, result) in results {
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
                            let link = ui.scope_id().with(("thr-link", slot.name()));
                            let y_max = t.grid.y.last().copied().unwrap_or(0.0);
                            let image = grid_image(app, ui.ctx(), &o, &t.grid, max, 20.0);
                            Plot::new(("thr", slot.name()))
                                .legend(Legend::default())
                                .y_axis_label("frequency (Hz)")
                                .link_axis(link, [true, false])
                                .height(h - 24.0 - COUNT_BAR_HEIGHT - 24.0)
                                .show(ui, |pu| {
                                    if let Some(img) = image {
                                        pu.image(img);
                                    }
                                    for poly in sparse_bins(&t.grid, &t.counts, min_segments) {
                                        pu.polygon(poly);
                                    }
                                    if let Some(o) = &t.overlay {
                                        draw_rpm_overlay(pu, o, &t.grid.x, y_max, &overlay);
                                    }
                                    marker_hlines(pu, &markers);
                                });
                            segment_count_bar(ui, slot, link, &t.grid, &t.counts, min_segments);
                            let below = t.counts.iter().filter(|&&c| c < min_segments).count();
                            ui.label(
                                RichText::new(if below > 0 {
                                    format!(
                                        "{below} of {} throttle bins have fewer than {min_segments} segments (greyed)",
                                        t.counts.len()
                                    )
                                } else {
                                    format!(
                                        "every throttle bin has at least {min_segments} segments"
                                    )
                                })
                                .color(MUTED),
                            );
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
    let markers = app.spectrum.markers.clone();
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    heatmap_controls(app, ui);
    ui.horizontal_wrapped(|ui| {
        let l = ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.stft.segment_s)
                .speed(0.01)
                .range(0.02..=1.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
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
    let touch = app.touch_mode;
    let slots: Vec<Slot> = [Slot::A, Slot::B]
        .into_iter()
        .filter(|s| app.loaded(*s).is_some())
        .collect();
    let results: Vec<SlotResult> = slots
        .iter()
        .map(|slot| (*slot, app.result(*slot, Kind::Stft(params.clone()))))
        .collect();
    // the player runs over slot A's columns (B follows the same time)
    let range = results.iter().find_map(|(_, r)| match r {
        Some(Ok(o)) => match o.as_ref() {
            Output::Stft(t) if t.grid.x.len() > 1 => {
                Some((t.grid.x[0], t.grid.x[t.grid.x.len() - 1]))
            }
            _ => None,
        },
        _ => None,
    });
    let columns: Vec<f64> = results
        .iter()
        .find_map(|(_, r)| match r {
            Some(Ok(o)) => match o.as_ref() {
                Output::Stft(t) => Some(t.grid.x.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_default();
    let mut pos = player_controls(app, ui, range, &columns);
    let h = ui.available_height() / slots.len().max(1) as f32;
    let mut clicked_at: Option<f64> = None;
    for (i, (slot, result)) in results.into_iter().enumerate() {
        match result {
            Some(Ok(o)) => {
                if let Output::Stft(t) = o.as_ref() {
                    ui.label(
                        RichText::new(format!("{}: {} ({})", slot.name(), t.channel.key(), t.unit))
                            .color(slot_color(slot)),
                    );
                    let image = grid_image(app, ui.ctx(), &o, &t.grid, max, 20.0);
                    let column = pos.and_then(|p| nearest_column(&t.grid.x, p));
                    let spectrum_h = ((h - 24.0) * 0.3).max(60.0);
                    let mut frame = None;
                    let mut x_bounds = None;
                    Plot::new(("stft", slot.name()))
                        .x_axis_label("time (s)")
                        .y_axis_label("frequency (Hz)")
                        .height(h - 24.0 - spectrum_h)
                        .allow_drag(!touch)
                        .show(ui, |pu| {
                            if let Some(img) = image {
                                pu.image(img);
                            }
                            marker_hlines(pu, &markers);
                            if let Some(p) = pos {
                                pu.vline(VLine::new("position", p).color(ACCENT).width(1.5));
                            }
                            // click (or, on touch, drag) scrubs
                            // (`interact_pointer_pos`: the hover position is gone during a drag)
                            let r = pu.response();
                            if (r.clicked() || (touch && r.dragged()))
                                && let Some(p) = r.interact_pointer_pos()
                            {
                                clicked_at = Some(pu.transform().value_from_position(p).x);
                            }
                            let b = pu.plot_bounds();
                            x_bounds = Some((b.min()[0], b.max()[0]));
                            frame = Some(*pu.transform().frame());
                        });
                    if i == 0 {
                        app.player.plot_frame = frame;
                        app.player.x_bounds = x_bounds;
                        app.player.column = column;
                    }
                    column_spectrum(ui, slot, t, column, max, spectrum_h);
                }
            }
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {
                ui.spinner();
            }
        }
    }
    if let Some(t) = clicked_at
        && let Some((x0, x1)) = range
    {
        pos = Some(t.clamp(x0, x1));
    }
    if pos != app.player.pos_s {
        app.player.pos_s = pos;
        app.player.synced_cursor = pos;
        app.viewer.cursor_s = pos;
    }
}

/// Play / pause, stepping, speed and the scrub slider; returns the position
/// after this frame's input and playback.
fn player_controls(
    app: &mut App,
    ui: &mut Ui,
    range: Option<(f64, f64)>,
    columns: &[f64],
) -> Option<f64> {
    let Some((x0, x1)) = range else {
        app.player.playing = false;
        return None;
    };
    // the log-viewer cursor moved elsewhere: follow it
    if app.viewer.cursor_s != app.player.synced_cursor
        && let Some(c) = app.viewer.cursor_s
    {
        app.player.pos_s = Some(c.clamp(x0, x1));
        app.player.synced_cursor = app.viewer.cursor_s;
    }
    let mut pos = app.player.pos_s.unwrap_or(x0).clamp(x0, x1);
    let now = ui.input(|i| i.time);
    let step = |pos: f64, dir: i64| -> f64 {
        nearest_column(columns, pos)
            .and_then(|c| columns.get((c as i64 + dir).max(0) as usize))
            .copied()
            .unwrap_or(pos)
    };
    ui.horizontal_wrapped(|ui| {
        let p = &mut app.player;
        if ui
            .button(if p.playing { "⏸ Pause" } else { "▶ Play" })
            .clicked()
        {
            p.playing = !p.playing;
            p.last_frame_time = None;
        }
        if ui.button("⏮ To start").clicked() {
            pos = x0;
        }
        if ui.button("◀ step").clicked() {
            pos = step(pos, -1);
        }
        if ui.button("step ▶").clicked() {
            pos = step(pos, 1);
        }
        let l = ui.label("speed");
        ui.add(
            egui::DragValue::new(&mut p.speed)
                .speed(0.1)
                .range(0.1..=16.0)
                .suffix("×"),
        )
        .labelled_by(l.id);
        let l = ui.label("position");
        ui.add(
            egui::Slider::new(&mut pos, x0..=x1)
                .suffix(" s")
                .fixed_decimals(2),
        )
        .labelled_by(l.id);
        if let Some(c) = nearest_column(columns, pos) {
            ui.label(RichText::new(format!("column {} of {}", c + 1, columns.len())).color(MUTED));
        }
    });
    if app.player.playing {
        if let Some(last) = app.player.last_frame_time {
            pos += (now - last).max(0.0) * app.player.speed;
            if pos > x1 {
                pos = x0; // loop
            }
        }
        app.player.last_frame_time = Some(now);
        ui.ctx().request_repaint();
    }
    Some(pos)
}

/// The spectrum of one STFT column as a line: amplitude over frequency.
fn column_spectrum(
    ui: &mut Ui,
    slot: Slot,
    t: &openpid_analysis::StftOutput,
    column: Option<usize>,
    max: f64,
    height: f32,
) {
    let nx = t.grid.x.len();
    let y_max = if max > 0.0 {
        max
    } else {
        openpid_analysis::render::robust_max(&t.grid.z)
    };
    let points: Vec<[f64; 2]> = column
        .map(|c| {
            t.grid
                .y
                .iter()
                .enumerate()
                .map(|(iy, f)| [*f, t.grid.z[iy * nx + c]])
                .collect()
        })
        .unwrap_or_default();
    let title = match column {
        Some(c) => format!("{} spectrum at {:.2} s", slot.name(), t.grid.x[c]),
        None => format!("{} spectrum", slot.name()),
    };
    ui.label(RichText::new(title.clone()).color(MUTED));
    Plot::new(("stft-column", slot.name()))
        .x_axis_label("frequency (Hz)")
        .y_axis_label(t.unit.clone())
        .height(height)
        .include_y(0.0)
        .include_y(y_max)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show(ui, |pu| {
            pu.line(Line::new(title, PlotPoints::from(points)).color(slot_color(slot)));
        });
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
                let l = ui.label(&d.label);
                let step = if d.max - d.min <= 2.0 { 1.0 } else { 5.0 };
                let mut slider = egui::Slider::new(v, d.min..=d.max)
                    .step_by(step)
                    .fixed_decimals(0);
                if touch {
                    slider = slider.trailing_fill(true);
                }
                ui.add_sized([360.0, if touch { 36.0 } else { 20.0 }], slider)
                    .labelled_by(l.id);
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

// ---------------------------------------------------------------------------

/// Low-pass topology names for the what-if editor.
const LOWPASS_KINDS: [(LowpassKind, &str); 4] = [
    (LowpassKind::Pt1, "PT1"),
    (LowpassKind::Biquad, "biquad"),
    (LowpassKind::Pt2, "PT2"),
    (LowpassKind::Pt3, "PT3"),
];

fn kind_name(k: &LowpassKind) -> String {
    LOWPASS_KINDS
        .iter()
        .find(|(kind, _)| kind == k)
        .map_or_else(|| format!("{k:?}"), |(_, n)| (*n).to_string())
}

/// A what-if number: shows the log's value until the user edits it, then the
/// override (label turns accent-coloured).
#[allow(clippy::too_many_arguments, reason = "one widget, seven knobs")]
fn whatif_value(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<f64>,
    log_value: f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
    integer: bool,
) {
    let colour = if value.is_some() { ACCENT } else { MUTED };
    let l = ui.label(RichText::new(label).color(colour));
    let mut v = value.unwrap_or(log_value);
    let mut w = egui::DragValue::new(&mut v)
        .speed(speed)
        .range(range)
        .suffix(suffix);
    if integer {
        w = w.max_decimals(0);
    }
    if ui
        .add(w)
        .labelled_by(l.id)
        .on_hover_text(format!("log: {log_value}"))
        .changed()
    {
        *value = Some(if integer { v.round() } else { v });
    }
}

/// A what-if low-pass topology.
fn whatif_kind(
    ui: &mut Ui,
    id: &str,
    value: &mut Option<LowpassKind>,
    log_kind: Option<&LowpassKind>,
) {
    let current: Option<LowpassKind> = value.clone().or_else(|| log_kind.cloned());
    let l = ui.label("type");
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(current.as_ref().map_or_else(|| "—".to_string(), kind_name))
        .width(80.0)
        .show_ui(ui, |ui| {
            for (kind, name) in &LOWPASS_KINDS {
                if ui
                    .selectable_label(current.as_ref() == Some(kind), *name)
                    .clicked()
                {
                    *value = Some(kind.clone());
                }
            }
        });
    combo.response.labelled_by(l.id);
}

/// Topology, static cutoff and dynamic range of a low-pass as flown (zeros when absent).
fn lowpass_seed(l: Option<&LowpassModel>) -> (Option<LowpassKind>, f64, f64, f64) {
    match l {
        Some(l) => (
            Some(l.kind.clone()),
            l.cutoff_hz,
            l.dynamic.as_ref().map_or(0.0, |d| d.min_hz),
            l.dynamic.as_ref().map_or(0.0, |d| d.max_hz),
        ),
        None => (None, 0.0, 0.0, 0.0),
    }
}

/// The what-if editor, seeded with the chain as flown (`seed`) when a result exists.
fn whatif_editor(ui: &mut Ui, o: &mut FilterOverrides, seed: Option<&ChainModel>) {
    let (k1, hz1, dmin, dmax) = lowpass_seed(seed.and_then(|m| m.lowpass1.as_ref()));
    let (k2, hz2, _, _) = lowpass_seed(seed.and_then(|m| m.lowpass2.as_ref()));
    let (k1, k2) = (k1.as_ref(), k2.as_ref());
    let rpm = seed.and_then(|m| m.rpm_notch.as_ref());
    let dn = seed.and_then(|m| m.dyn_notch.as_ref());
    let notch = seed.and_then(|m| m.notches.first());
    ui.horizontal_wrapped(|ui| {
        ui.label("LPF1");
        whatif_kind(ui, "fsim_lpf1_kind", &mut o.lpf1_type, k1);
        whatif_value(
            ui,
            "cutoff",
            &mut o.lpf1_hz,
            hz1,
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
        let mut dyn_min = o.lpf1_dyn_hz.map(|d| d.0);
        let mut dyn_max = o.lpf1_dyn_hz.map(|d| d.1);
        whatif_value(
            ui,
            "dyn min",
            &mut dyn_min,
            dmin,
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
        whatif_value(
            ui,
            "dyn max",
            &mut dyn_max,
            dmax,
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
        if dyn_min.is_some() || dyn_max.is_some() {
            o.lpf1_dyn_hz = Some((dyn_min.unwrap_or(dmin), dyn_max.unwrap_or(dmax)));
        }
        ui.separator();
        ui.label("LPF2");
        whatif_kind(ui, "fsim_lpf2_kind", &mut o.lpf2_type, k2);
        whatif_value(
            ui,
            "cutoff",
            &mut o.lpf2_hz,
            hz2,
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("RPM notch");
        let mut harmonics = o.rpm_harmonics.map(f64::from);
        whatif_value(
            ui,
            "harmonics",
            &mut harmonics,
            rpm.map_or(0.0, |r| f64::from(r.harmonics)),
            1.0,
            0.0..=10.0,
            "",
            true,
        );
        o.rpm_harmonics = harmonics.map(|h| h as u32);
        whatif_value(
            ui,
            "Q",
            &mut o.rpm_q,
            rpm.map_or(5.0, |r| r.q),
            0.1,
            0.1..=20.0,
            "",
            false,
        );
        whatif_value(
            ui,
            "min",
            &mut o.rpm_min_hz,
            rpm.map_or(100.0, |r| r.min_hz),
            5.0,
            0.0..=500.0,
            " Hz",
            false,
        );
        ui.separator();
        ui.label("Dyn notch");
        let mut count = o.dyn_count.map(f64::from);
        whatif_value(
            ui,
            "count",
            &mut count,
            dn.map_or(0.0, |d| f64::from(d.count)),
            1.0,
            0.0..=8.0,
            "",
            true,
        );
        o.dyn_count = count.map(|c| c as u32);
        whatif_value(
            ui,
            "Q",
            &mut o.dyn_q,
            dn.map_or(3.0, |d| d.q),
            0.1,
            0.1..=20.0,
            "",
            false,
        );
        whatif_value(
            ui,
            "min",
            &mut o.dyn_min_hz,
            dn.map_or(150.0, |d| d.min_hz),
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
        whatif_value(
            ui,
            "max",
            &mut o.dyn_max_hz,
            dn.map_or(600.0, |d| d.max_hz),
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
        ui.separator();
        ui.label("Notch");
        whatif_value(
            ui,
            "centre",
            &mut o.notch_hz,
            notch.map_or(0.0, |n| n.center_hz),
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
        whatif_value(
            ui,
            "cutoff",
            &mut o.notch_cutoff_hz,
            notch.map_or(0.0, |n| n.cutoff_hz),
            5.0,
            0.0..=1000.0,
            " Hz",
            false,
        );
    });
}

/// Filter Sim: the unfiltered gyro through the modelled chain, validated
/// against the logged filtered gyro, and through the what-if chain.
pub fn filter_sim(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        axis_selector(ui, &mut app.tools.axis);
        app.tools.filter_sim.axis = app.tools.axis;
        ui.separator();
        let l = ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.filter_sim.segment_s)
                .speed(0.01)
                .range(0.02..=2.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
        ui.label("max");
        let mut max = app.tools.filter_sim.max_hz.unwrap_or(0.0);
        if ui
            .add(
                egui::DragValue::new(&mut max)
                    .speed(10.0)
                    .range(0.0..=8000.0)
                    .suffix(" Hz"),
            )
            .changed()
        {
            app.tools.filter_sim.max_hz = if max > 0.0 { Some(max) } else { None };
        }
        if !app.tools.filter_sim.overrides.is_empty() && ui.button("Reset to log").clicked() {
            app.tools.filter_sim.overrides = FilterOverrides::default();
        }
    });
    let axis = app.tools.axis;
    let params = app.tools.filter_sim.clone();
    let mut outputs: Vec<(Slot, Arc<Output>)> = Vec::new();
    for slot in [Slot::A, Slot::B] {
        let has_raw = app.loaded(slot).and_then(Loaded::current).is_some_and(|s| {
            s.channel(&ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            })
            .is_some()
        });
        if app.loaded(slot).is_some() && !has_raw {
            ui.label(
                RichText::new(format!(
                    "{}: no unfiltered gyro in this log (Betaflight: log gyroUnfilt or set debug_mode GYRO_SCALED; ArduPilot: enable the batch sampler)",
                    slot.name()
                ))
                .color(MUTED),
            );
            continue;
        }
        if !has_raw {
            continue;
        }
        match app.result(slot, Kind::FilterSim(params.clone())) {
            Some(Ok(o)) => outputs.push((slot, o)),
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {
                ui.spinner();
            }
        }
    }
    let seed: Option<ChainModel> = outputs.iter().find_map(|(_, o)| match o.as_ref() {
        Output::FilterSim(f) => Some(f.model.clone()),
        _ => None,
    });
    whatif_editor(ui, &mut app.tools.filter_sim.overrides, seed.as_ref());
    for (slot, o) in &outputs {
        let Output::FilterSim(f) = o.as_ref() else {
            continue;
        };
        ui.label(
            RichText::new(format!("{}: {}", slot.name(), f.description.join(" · ")))
                .color(slot_color(*slot)),
        );
        let fit = match &f.fit {
            Some(fit) => format!(
                "fit: residual {:.0} % of the filtered RMS (unfiltered {:.0} %), delay {:+.1} ms",
                fit.residual_rel * 100.0,
                fit.unfiltered_rel * 100.0,
                fit.delay_ms
            ),
            None => "no gyro.filtered in the log to validate against".to_string(),
        };
        ui.label(
            RichText::new(format!(
                "{}: {fit}; throttle {}, {} motor(s) with rpm",
                slot.name(),
                if f.throttle_driven {
                    "drives LPF1"
                } else {
                    "not logged"
                },
                f.motors
            ))
            .color(MUTED),
        );
    }
    let h = ui.available_height();
    let whatif = !params.overrides.is_empty();
    Plot::new("fsim-psd")
        .legend(Legend::default())
        .x_axis_label("frequency (Hz)")
        .y_axis_label("dB")
        .height(h * 0.55)
        .show(ui, |pu| {
            for (slot, o) in &outputs {
                let Output::FilterSim(f) = o.as_ref() else {
                    continue;
                };
                let line = |name: String, ys: &[f64]| {
                    let pts: Vec<[f64; 2]> = f.freq.iter().zip(ys).map(|(x, y)| [*x, *y]).collect();
                    Line::new(name, PlotPoints::from(pts))
                };
                pu.line(
                    line(format!("{} raw", slot.name()), &f.psd_raw_db)
                        .color(slot_color(*slot).gamma_multiply(0.45))
                        .width(1.0),
                );
                if let Some(l) = &f.psd_logged_db {
                    pu.line(
                        line(format!("{} logged filtered", slot.name()), l)
                            .color(slot_color(*slot))
                            .width(1.5),
                    );
                }
                if whatif {
                    pu.line(
                        line(format!("{} sim as flown", slot.name()), &f.psd_as_flown_db)
                            .color(slot_color(*slot))
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                }
                pu.line(
                    line(
                        format!(
                            "{} sim{}",
                            slot.name(),
                            if whatif { " what-if" } else { "" }
                        ),
                        &f.psd_sim_db,
                    )
                    .color(if *slot == Slot::A {
                        ACCENT
                    } else {
                        Color32::LIGHT_GREEN
                    })
                    .width(1.5),
                );
            }
        });
    let px = ui.available_width() as usize;
    Plot::new("fsim-time")
        .legend(Legend::default())
        .x_axis_label("time (s)")
        .y_axis_label("deg/s")
        .height(h * 0.45 - 8.0)
        .show(ui, |pu| {
            for (slot, o) in &outputs {
                let Output::FilterSim(f) = o.as_ref() else {
                    continue;
                };
                let series: Vec<(String, &[f64], Color32, f32)> = [
                    Some((
                        format!("{} raw", slot.name()),
                        f.raw.as_slice(),
                        slot_color(*slot).gamma_multiply(0.45),
                        1.0,
                    )),
                    f.logged.as_deref().map(|l| {
                        (
                            format!("{} logged filtered", slot.name()),
                            l,
                            slot_color(*slot),
                            1.2,
                        )
                    }),
                    Some((
                        format!("{} sim", slot.name()),
                        f.sim.as_slice(),
                        if *slot == Slot::A {
                            ACCENT
                        } else {
                            Color32::LIGHT_GREEN
                        },
                        1.2,
                    )),
                ]
                .into_iter()
                .flatten()
                .collect();
                for (name, ys, colour, width) in series {
                    let pts = decimate(&f.time_s, ys, |_| true, px.max(100) * 2);
                    pu.line(
                        Line::new(name, PlotPoints::from(pts))
                            .color(colour)
                            .width(width),
                    );
                }
            }
        });
}

// ---------------------------------------------------------------------------

/// Motor noise: a table of every motor's harmonic amplitudes on the gyro and
/// the spectrum with the harmonics marked per motor.
pub fn motor_noise(app: &mut App, ui: &mut Ui) {
    for slot in [Slot::A, Slot::B] {
        slot_header(app, ui, slot);
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if axis_selector(ui, &mut app.tools.axis) {
            app.tools.motor_noise.channel = ChannelId::Gyro {
                axis: app.tools.axis,
                stage: GyroStage::Raw,
            };
        }
        ui.separator();
        let l = ui.label("segment");
        ui.add(
            egui::DragValue::new(&mut app.tools.motor_noise.segment_s)
                .speed(0.01)
                .range(0.05..=2.0)
                .suffix(" s"),
        )
        .labelled_by(l.id);
        let l = ui.label("harmonics");
        ui.add(egui::DragValue::new(&mut app.tools.motor_noise.harmonics).range(1..=8))
            .labelled_by(l.id);
    });
    let axis = app.tools.axis;
    let params = app.tools.motor_noise.clone();
    let mut outputs: Vec<(Slot, Arc<Output>)> = Vec::new();
    for slot in [Slot::A, Slot::B] {
        let has_raw = app.loaded(slot).and_then(Loaded::current).is_some_and(|s| {
            s.channel(&ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            })
            .is_some()
        });
        if app.loaded(slot).is_some() && !has_raw {
            ui.label(
                RichText::new(format!(
                    "{}: no unfiltered gyro in this log; the filtered one has the motor peaks removed",
                    slot.name()
                ))
                .color(MUTED),
            );
            continue;
        }
        if !has_raw {
            continue;
        }
        match app.result(slot, Kind::MotorNoise(params.clone())) {
            Some(Ok(o)) => outputs.push((slot, o)),
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {
                ui.spinner();
            }
        }
    }
    let w = ui.available_width() / outputs.len().max(1) as f32;
    ui.horizontal(|ui| {
        for (slot, o) in &outputs {
            let Output::MotorNoise(m) = o.as_ref() else {
                continue;
            };
            ui.vertical(|ui| {
                ui.set_width(w - 8.0);
                ui.label(
                    RichText::new(format!(
                        "{}: {} ({}), rpm {}, {} frames{}",
                        slot.name(),
                        m.channel.key(),
                        m.unit,
                        m.rpm_source,
                        m.frames,
                        m.loudest
                            .filter(|_| m.motors.len() > 1)
                            .map_or(String::new(), |i| format!(", loudest M{}", i + 1))
                    ))
                    .color(slot_color(*slot)),
                );
                egui::Grid::new(("motor-noise", slot.name()))
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("motor").strong());
                        ui.label(RichText::new("rpm").strong());
                        for k in 1..=params.harmonics {
                            ui.label(RichText::new(format!("×{k} amp (ratio)")).strong());
                        }
                        ui.label(RichText::new("total").strong());
                        ui.end_row();
                        for motor in &m.motors {
                            let loud = m.loudest == Some(motor.idx) && m.motors.len() > 1;
                            let colour = if loud {
                                ACCENT
                            } else {
                                motor_color(usize::from(motor.idx))
                            };
                            ui.label(RichText::new(format!("M{}", motor.idx + 1)).color(colour));
                            ui.label(format!("{:.0}", motor.mean_rpm));
                            for h in &motor.harmonics {
                                let text = format!("{:.2} ({:.2})", h.amplitude, h.ratio);
                                ui.label(if h.ratio > 1.3 {
                                    RichText::new(text).color(ACCENT)
                                } else {
                                    RichText::new(text)
                                });
                            }
                            ui.label(format!("{:.2}", motor.total));
                            ui.end_row();
                        }
                    });
            });
        }
    });
    let h = ui.available_height();
    Plot::new("motor-noise-psd")
        .legend(Legend::default())
        .x_axis_label("frequency (Hz)")
        .y_axis_label("dB")
        .height(h)
        .show(ui, |pu| {
            for (slot, o) in &outputs {
                let Output::MotorNoise(m) = o.as_ref() else {
                    continue;
                };
                let pts: Vec<[f64; 2]> = m
                    .freq
                    .iter()
                    .zip(&m.psd_db)
                    .map(|(f, y)| [*f, *y])
                    .collect();
                pu.line(
                    Line::new(
                        format!("{} {}", slot.name(), m.channel.key()),
                        PlotPoints::from(pts),
                    )
                    .color(slot_color(*slot))
                    .width(1.2),
                );
                for motor in &m.motors {
                    for hn in &motor.harmonics {
                        if hn.mean_hz <= 0.0 {
                            continue;
                        }
                        let mut line = VLine::new(
                            format!("{} M{} ×{}", slot.name(), motor.idx + 1, hn.k),
                            hn.mean_hz,
                        )
                        .color(motor_color(usize::from(motor.idx)))
                        .width(if hn.k == 1 { 1.5 } else { 1.0 });
                        if hn.k > 1 {
                            line = line.style(LineStyle::dashed_dense());
                        }
                        pu.vline(line);
                    }
                }
            }
        });
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
