//! The Freq × time tab in the axis-row layout (docs/design/time-tab, F2):
//! roll / pitch / yaw spectrograms of the docked log, the spectrum at the
//! player's position beside each row, and the throttle under them on the
//! same time axis; the colour in dB by default, as the reference tool. Every
//! control, the player's included, is in the panel on the right.

use crate::app::App;
use crate::axis_layout::control_panel;
use crate::colormap::{ACCENT, MUTED};
use crate::i18n::tr;
use crate::panel;
use crate::shortcuts::{self, Action};
use crate::spectrum_tab::Quantity;
use crate::views::{
    BAR_W, PlayerRequest, colour_bar_range, effective_max, heatmap_hover, hover_label,
    marker_hlines, nearest_column, show_error, slot_color,
};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, VLine};
use openpid_analysis::{StftOutput, StftParams};
use openpid_core::analysis::Grid2D;
use openpid_core::{Axis, ChannelId};
use std::collections::HashMap;
use std::sync::Arc;

/// Width of the spectrum beside each row.
const SLICE_W: f32 = 110.0;
/// Width of the y-axis labels, the same on every plot so the time axes of
/// the rows and of the throttle strip line up.
const Y_AXIS_W: f32 = 52.0;
/// Height of the throttle strip under the rows.
const STRIP_H: f32 = 74.0;
/// Rows below this frequency do not set the automatic colour range.
const SCALE_MIN_HZ: f64 = 20.0;
/// Percentiles of the dB values that make the automatic colour range: the
/// floor sits on the background noise, the top on the strongest lines.
const DB_LOW_PCT: f64 = 0.30;
const DB_HIGH_PCT: f64 = 0.995;

/// Display options of the tab (not analysis parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct TimeDisplay {
    /// For the gyro: the unfiltered gyro (as the reference tool's default),
    /// where the motor noise shows.
    pub prefilt: bool,
    /// Colour in dB (`20·log10` of the amplitude) instead of linear.
    pub db: bool,
    /// Fixed ends of the dB colour range; `None` = automatic.
    pub z_min: Option<f64>,
    pub z_max: Option<f64>,
}

impl Default for TimeDisplay {
    fn default() -> Self {
        TimeDisplay {
            prefilt: true,
            db: true,
            z_min: None,
            z_max: None,
        }
    }
}

/// Per-frame view state: the automatic dB ranges of the results shown.
#[derive(Debug, Default)]
pub struct TimeView {
    ranges: HashMap<usize, (f64, f64)>,
    /// Time span and column count of the docked log's grid, for the panel's
    /// player (drawn before the rows).
    pub span: Option<(f64, f64)>,
    pub columns: usize,
}

/// One row: the axis and its spectrogram.
struct Row {
    axis: Axis,
    out: Arc<Output>,
}

impl Row {
    fn stft(&self) -> Option<&StftOutput> {
        match self.out.as_ref() {
            Output::Stft(t) => Some(t),
            _ => None,
        }
    }
}

/// The channel a row shows.
fn channel(q: Quantity, d: &TimeDisplay, axis: Axis) -> ChannelId {
    q.prefilt(axis)
        .filter(|_| d.prefilt)
        .unwrap_or_else(|| q.channel(axis))
}

/// The tab.
pub fn time(app: &mut App, ui: &mut Ui) {
    control_panel(app, ui, "time-panel", controls);
    let full_w = ui.available_width();
    ui.separator();
    let Some(slot) = app.dock_slot() else {
        return;
    };
    let q = Quantity::of(&app.tools.heatmap_channel);
    let display = app.tools.time_display.clone();
    // the export, the project and the tests read the roll row's job
    app.tools.stft.channel = channel(q, &display, Axis::Roll);
    let base = app.tools.stft.clone();
    let mut rows: Vec<Row> = Vec::new();
    for axis in crate::axis_layout::shown_axes(app) {
        let params = StftParams {
            channel: channel(q, &display, axis),
            ..base.clone()
        };
        match app.result(slot, Kind::Stft(params)) {
            Some(Ok(out)) => rows.push(Row { axis, out }),
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {}
        }
    }
    let Some(first) = rows.first().and_then(Row::stft) else {
        ui.spinner();
        return;
    };
    let columns = first.grid.x.clone();
    let span = match columns.as_slice() {
        [a, .., b] => (*a, *b),
        _ => return,
    };
    app.time_view.span = Some(span);
    app.time_view.columns = columns.len();
    let mut pos = advance(app, ui, span, &columns);
    let range = colour_range(app, &rows, &display);
    title(ui, slot, q, &display, pos);
    let strip_h = if throttle(app, slot).is_some() {
        STRIP_H
    } else {
        0.0
    };
    let gap = ui.spacing().item_spacing;
    let row_h = ((ui.available_height() - strip_h - 22.0) / rows.len() as f32 - gap.y).max(70.0);
    let plot_w = (full_w - SLICE_W - BAR_W - 2.0 * gap.x).max(200.0);
    app.heatmap_hover = None;
    let mut clicked_at = None;
    let column = nearest_column(&columns, pos);
    for (i, row) in rows.iter().enumerate() {
        let Some(t) = row.stft() else { continue };
        ui.horizontal(|ui| {
            let drawn = spectrogram(app, ui, slot, row, t, pos, range, egui::vec2(plot_w, row_h));
            clicked_at = clicked_at.or(drawn.clicked);
            if i == 0 {
                app.player.plot_frame = Some(drawn.frame);
                app.player.x_bounds = Some(drawn.x_bounds);
                app.player.column = column;
                app.heatmap_frames.insert(slot, drawn.frame);
            }
            slice(
                ui,
                slot,
                row.axis,
                t,
                column,
                range,
                &display,
                egui::vec2(SLICE_W, row_h),
            );
            colour_bar_range(ui, range.0, range.1, row_h);
        });
    }
    if strip_h > 0.0 {
        throttle_strip(app, ui, slot, pos, span, egui::vec2(plot_w, strip_h));
    }
    let hover = app
        .heatmap_hover
        .map(|(s, x, y, z)| (s, x, y, if display.db { db(z) } else { z }));
    hover_label(ui, hover, true);
    if let Some(x) = clicked_at {
        pos = x.clamp(span.0, span.1);
    }
    app.player.pos_s = Some(pos);
    app.player.synced_cursor = Some(pos);
    app.viewer.cursor_s = Some(pos);
}

fn db(v: f64) -> f64 {
    20.0 * v.max(1e-9).log10()
}

/// The player's position this frame: follows a cursor moved on the log
/// viewer, applies the panel's and the shortcuts' requests, and advances
/// while playing (looping at the end).
fn advance(app: &mut App, ui: &Ui, (x0, x1): (f64, f64), columns: &[f64]) -> f64 {
    if app.viewer.cursor_s != app.player.synced_cursor
        && let Some(c) = app.viewer.cursor_s
    {
        app.player.pos_s = Some(c.clamp(x0, x1));
        app.player.synced_cursor = app.viewer.cursor_s;
    }
    let mut pos = app.player.pos_s.unwrap_or(x0).clamp(x0, x1);
    let step = |pos: f64, dir: i64| -> f64 {
        nearest_column(columns, pos)
            .and_then(|c| columns.get((c as i64 + dir).max(0) as usize))
            .copied()
            .unwrap_or(pos)
    };
    match app.player.request.take() {
        Some(PlayerRequest::ToStart) => pos = x0,
        Some(PlayerRequest::StepBack) => pos = step(pos, -1),
        Some(PlayerRequest::StepForward) => pos = step(pos, 1),
        None => {}
    }
    let now = ui.input(|i| i.time);
    if app.player.playing {
        if let Some(last) = app.player.last_frame_time {
            pos += (now - last).max(0.0) * app.player.speed;
            if pos > x1 {
                pos = x0;
            }
        }
        app.player.last_frame_time = Some(now);
        ui.ctx().request_repaint();
    }
    pos
}

/// The colour range of every row: fixed ends from the panel, else the
/// automatic dB range of the rows (shared, so the rows compare); linear: 0
/// to the colour max (automatic: the largest robust maximum of the rows).
fn colour_range(app: &mut App, rows: &[Row], d: &TimeDisplay) -> (f64, f64) {
    let grids: Vec<(usize, &Grid2D)> = rows
        .iter()
        .filter_map(|r| r.stft().map(|t| (Arc::as_ptr(&r.out) as usize, &t.grid)))
        .collect();
    let range = if d.db {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        if app.time_view.ranges.len() > 64 {
            app.time_view.ranges.clear();
        }
        for (key, g) in &grids {
            let (a, b) = *app
                .time_view
                .ranges
                .entry(*key)
                .or_insert_with(|| auto_db_range(g));
            lo = lo.min(a);
            hi = hi.max(b);
        }
        let (lo, hi) = if lo.is_finite() {
            (lo, hi)
        } else {
            (-40.0, 0.0)
        };
        let lo = d.z_min.unwrap_or(lo);
        (lo, d.z_max.unwrap_or(hi).max(lo + 1.0))
    } else {
        let fixed = app.tools.heatmap_max;
        let hi = grids
            .iter()
            .map(|(_, g)| effective_max(g, fixed, SCALE_MIN_HZ))
            .fold(0.0_f64, f64::max);
        (0.0, hi.max(1e-12))
    };
    if let Some(slot) = app.dock_slot() {
        app.colour_scales = vec![(slot, range.1)];
    }
    range
}

/// Percentiles of the dB values above `SCALE_MIN_HZ`: the floor on the
/// background noise, the top on the strongest lines.
pub fn auto_db_range(g: &Grid2D) -> (f64, f64) {
    let nx = g.x.len();
    let mut v: Vec<f64> =
        g.y.iter()
            .enumerate()
            .filter(|(_, f)| **f >= SCALE_MIN_HZ)
            .flat_map(|(iy, _)| g.z[iy * nx..(iy + 1) * nx].iter().map(|z| db(*z)))
            .filter(|x| x.is_finite())
            .collect();
    if v.is_empty() {
        return (-40.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    let at = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    let (lo, hi) = (at(DB_LOW_PCT), at(DB_HIGH_PCT));
    (lo.floor(), hi.ceil().max(lo.floor() + 1.0))
}

/// "A prefilt · roll, pitch, yaw · spectrum at 24.00 s" over the rows.
fn title(ui: &mut Ui, slot: Slot, q: Quantity, d: &TimeDisplay, pos: f64) {
    let what = if d.prefilt && q.prefilt(Axis::Roll).is_some() {
        tr("prefilt").to_string()
    } else {
        q.name().to_string()
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} {what}", slot.name()))
                .color(slot_color(slot))
                .size(12.5),
        );
        ui.label(
            RichText::new(format!("{} {} {pos:.2} s", slot.name(), tr("spectrum at")))
                .color(MUTED)
                .size(12.5),
        );
    });
}

/// What a spectrogram reports back.
struct Drawn {
    frame: egui::Rect,
    x_bounds: (f64, f64),
    clicked: Option<f64>,
}

/// One row's spectrogram with the position line, markers and hover.
#[allow(clippy::too_many_arguments)]
fn spectrogram(
    app: &mut App,
    ui: &mut Ui,
    slot: Slot,
    row: &Row,
    t: &StftOutput,
    pos: f64,
    range: (f64, f64),
    size: egui::Vec2,
) -> Drawn {
    let image = texture(app, ui.ctx(), &row.out, &t.grid, range);
    let markers = app.spectrum.markers.clone();
    let touch = app.touch_mode;
    let link = egui::Id::unique("time-x");
    let mut drawn = Drawn {
        frame: egui::Rect::NOTHING,
        x_bounds: (0.0, 0.0),
        clicked: None,
    };
    let mut hover = None;
    let (x0, x1) = match t.grid.x.as_slice() {
        [a, .., b] => (*a, *b),
        _ => (0.0, 1.0),
    };
    Plot::new(("stft", slot.name(), row.axis.name()))
        .width(size.x)
        .height(size.y)
        .y_axis_min_width(Y_AXIS_W)
        .default_x_bounds(x0, x1)
        .set_margin_fraction(egui::Vec2::ZERO)
        .link_axis(link, [true, false])
        .link_cursor(link, [true, false])
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .y_axis_label(format!("{} (Hz)", row.axis.name()))
        .allow_drag(!touch)
        .allow_scroll(false)
        .show_x(false)
        .show_y(false)
        .show(ui, |pu| {
            if let Some(img) = image {
                pu.image(img);
            }
            marker_hlines(pu, &markers);
            pu.vline(VLine::new("position", pos).color(ACCENT).width(1.5));
            // click (or, on touch, drag) scrubs
            let r = pu.response();
            if (r.clicked() || (touch && r.dragged()))
                && let Some(p) = r.interact_pointer_pos()
            {
                drawn.clicked = Some(pu.transform().value_from_position(p).x);
            }
            let b = pu.plot_bounds();
            drawn.x_bounds = (b.min()[0], b.max()[0]);
            drawn.frame = *pu.transform().frame();
            hover = heatmap_hover(pu, slot, &t.grid);
        });
    if hover.is_some() {
        app.heatmap_hover = hover;
    }
    drawn
}

/// The grid as a texture in the colour range, once per result and range.
fn texture(
    app: &mut App,
    ctx: &egui::Context,
    owner: &Arc<Output>,
    g: &Grid2D,
    (lo, hi): (f64, f64),
) -> Option<egui_plot::PlotImage> {
    use std::hash::{Hash, Hasher};
    let (nx, ny) = (g.x.len(), g.y.len());
    if nx < 2 || ny < 2 {
        return None;
    }
    let db_scale = app.tools.time_display.db;
    let cmap = crate::colormap::heatmap();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        "time",
        Arc::as_ptr(owner) as usize,
        lo.to_bits(),
        hi.to_bits(),
        db_scale,
        cmap,
    )
        .hash(&mut hasher);
    let key = hasher.finish();
    if !app.heatmap_textures.contains_key(&key) {
        if app.heatmap_textures.len() >= 96 {
            app.heatmap_textures.clear();
        }
        let mut rgba = Vec::with_capacity(nx * ny * 4);
        for iy in (0..ny).rev() {
            for ix in 0..nx {
                let z = g.z[iy * nx + ix];
                let v = if db_scale { db(z) } else { z };
                // below the range is black, not transparent
                let t = ((v - lo) / (hi - lo)).max(1e-6);
                rgba.extend_from_slice(&cmap.rgba(t));
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([nx, ny], &rgba);
        let handle = ctx.load_texture(format!("time-{key:x}"), image, egui::TextureOptions::LINEAR);
        app.heatmap_textures.insert(key, handle);
    }
    let tex = &app.heatmap_textures[&key];
    let dx = g.x[1] - g.x[0];
    let dy = g.y[1] - g.y[0];
    let (x0, x1) = (g.x[0] - dx / 2.0, g.x[nx - 1] + dx / 2.0);
    let (y0, y1) = (g.y[0] - dy / 2.0, g.y[ny - 1] + dy / 2.0);
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

/// The spectrum at the player's column beside a row: frequency up, value
/// across, on the colour range of the maps.
#[allow(clippy::too_many_arguments)]
fn slice(
    ui: &mut Ui,
    slot: Slot,
    axis: Axis,
    t: &StftOutput,
    column: Option<usize>,
    (lo, hi): (f64, f64),
    d: &TimeDisplay,
    size: egui::Vec2,
) {
    let nx = t.grid.x.len();
    let pts: Vec<[f64; 2]> = column
        .map(|c| {
            t.grid
                .y
                .iter()
                .enumerate()
                .map(|(iy, f)| {
                    let z = t.grid.z[iy * nx + c];
                    [if d.db { db(z) } else { z }, *f]
                })
                .collect()
        })
        .unwrap_or_default();
    let y_max = t.grid.y.last().copied().unwrap_or(0.0);
    Plot::new(("stft-slice", slot.name(), axis.name()))
        .width(size.x)
        .height(size.y)
        .show_axes([false, false])
        .show_x(false)
        .show_y(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .include_x(lo)
        .include_x(hi)
        .include_y(0.0)
        .include_y(y_max)
        .set_margin_fraction(egui::Vec2::ZERO)
        .show(ui, |pu| {
            pu.line(Line::new("", PlotPoints::from(pts)).color(slot_color(slot)));
        });
}

/// The docked log's throttle over time, if it has one.
fn throttle(app: &App, slot: Slot) -> Option<Vec<[f64; 2]>> {
    let s = app.loaded(slot)?.current()?;
    let c = s.channel(&ChannelId::Throttle)?;
    let t0 = s.timebase.time_us.first().copied().unwrap_or(0);
    let v = c.data.values();
    let n = v.len().min(s.timebase.time_us.len());
    let step = (n / 2000).max(1);
    Some(
        (0..n)
            .step_by(step)
            .filter(|i| v[*i].is_finite())
            .map(|i| [(s.timebase.time_us[i] - t0) as f64 / 1e6, v[i]])
            .collect(),
    )
}

/// The throttle strip under the rows, on the rows' time axis.
fn throttle_strip(
    app: &App,
    ui: &mut Ui,
    slot: Slot,
    pos: f64,
    (x0, x1): (f64, f64),
    size: egui::Vec2,
) {
    let Some(pts) = throttle(app, slot) else {
        return;
    };
    let pts: Vec<[f64; 2]> = pts
        .into_iter()
        .filter(|p| p[0] >= x0 && p[0] <= x1)
        .collect();
    let link = egui::Id::unique("time-x");
    Plot::new(("time-throttle", slot.name()))
        .width(size.x)
        .height(size.y)
        .y_axis_min_width(Y_AXIS_W)
        .default_x_bounds(x0, x1)
        .set_margin_fraction(egui::Vec2::ZERO)
        .link_axis(link, [true, false])
        .link_cursor(link, [true, false])
        .x_grid_spacer(crate::grid::spacer_x())
        .x_axis_formatter(crate::grid::label)
        .y_grid_spacer(crate::grid::spacer_y())
        .y_axis_formatter(crate::grid::label)
        .x_axis_label(tr("time (s)"))
        .y_axis_label(tr("throttle (%)"))
        .default_y_bounds(0.0, 100.0)
        .allow_drag([true, false])
        .allow_zoom([true, false])
        .allow_scroll(false)
        .show(ui, |pu| {
            pu.line(
                Line::new(tr("throttle (%)"), PlotPoints::from(pts))
                    .color(Color32::from_gray(200))
                    .width(1.2),
            );
            pu.vline(VLine::new("position", pos).color(ACCENT).width(1.5));
        });
}

/// The panel: data, colour, player, analysis.
fn controls(app: &mut App, ui: &mut Ui) {
    crate::axis_layout::axes_row(app, ui);
    panel::section(ui, tr("Data"));
    let mut q = Quantity::of(&app.tools.heatmap_channel);
    let quantities: Vec<(Quantity, String)> = Quantity::ALL
        .iter()
        .map(|v| (*v, v.name().to_string()))
        .collect();
    panel::choice(
        ui,
        "time-quantity",
        tr("Channel"),
        tr("what the rows show, on roll, pitch and yaw"),
        &mut q,
        &quantities,
    );
    app.tools.heatmap_channel = q.channel(Axis::Roll);
    let d = &mut app.tools.time_display;
    panel::check_enabled(
        ui,
        q.prefilt(Axis::Roll).is_some(),
        &mut d.prefilt,
        tr("Prefilt"),
        tr("the unfiltered gyro, where the motor noise shows; off: the filtered gyro"),
    );
    panel::section(ui, tr("Colour"));
    panel::choice(
        ui,
        "time-scale",
        tr("Scale"),
        tr(
            "dB (20·log10 of the amplitude) shows weak and strong lines at once, as the reference tool; linear shows the strongest only",
        ),
        &mut d.db,
        &[(true, "dB".to_string()), (false, tr("linear").to_string())],
    );
    if d.db {
        panel::optional(
            ui,
            tr("Z min"),
            tr("the value drawn black; auto: the background noise of the maps"),
            &mut d.z_min,
            -40.0,
            "",
        );
        panel::optional(
            ui,
            tr("Z max"),
            tr("the value drawn brightest; auto: the strongest lines of the maps"),
            &mut d.z_max,
            0.0,
            "",
        );
    } else {
        let mut max = (app.tools.heatmap_max > 0.0).then_some(app.tools.heatmap_max);
        panel::optional(
            ui,
            tr("Colour max"),
            tr(
                "the amplitude drawn in the brightest colour; auto: the 99th percentile above 20 Hz",
            ),
            &mut max,
            10.0,
            "",
        );
        app.tools.heatmap_max = max.unwrap_or(0.0);
    }
    player_rows(app, ui);
    panel::section(ui, tr("Analysis"));
    panel::drag(
        ui,
        tr("Segment"),
        tr(
            "length of each spectrum along the log: longer gives finer frequency steps and coarser time steps",
        ),
        &mut app.tools.stft.segment_s,
        0.01,
        0.02..=1.0,
        "s",
    );
    let mut top = app.tools.stft.max_hz.unwrap_or(1000.0);
    if panel::drag(
        ui,
        tr("Max frequency"),
        tr("top of the frequency axis, at most half the logging rate"),
        &mut top,
        10.0,
        50.0..=8000.0,
        "Hz",
    )
    .changed()
    {
        app.tools.stft.max_hz = Some(top);
    }
}

/// The player's controls: play / pause, to start, a column either way, the
/// speed and the position (the rows apply them next frame).
fn player_rows(app: &mut App, ui: &mut Ui) {
    panel::section(ui, tr("Player"));
    let ctx = ui.ctx().clone();
    let Some((x0, x1)) = app.time_view.span else {
        return;
    };
    let p = &mut app.player;
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(if p.playing { tr("Pause") } else { tr("Play") })
            .on_hover_text(shortcuts::keys_of(&ctx, Action::PlayPause))
            .clicked()
        {
            p.playing = !p.playing;
            p.last_frame_time = None;
        }
        if ui
            .button(tr("To start"))
            .on_hover_text(shortcuts::keys_of(&ctx, Action::ToStart))
            .clicked()
        {
            p.request = Some(PlayerRequest::ToStart);
        }
        if ui
            .button(tr("< step"))
            .on_hover_text(shortcuts::keys_of(&ctx, Action::StepBack))
            .clicked()
        {
            p.request = Some(PlayerRequest::StepBack);
        }
        if ui
            .button(tr("step >"))
            .on_hover_text(shortcuts::keys_of(&ctx, Action::StepForward))
            .clicked()
        {
            p.request = Some(PlayerRequest::StepForward);
        }
    });
    panel::drag(
        ui,
        tr("Speed"),
        tr("playback speed relative to the flight"),
        &mut p.speed,
        0.1,
        0.1..=16.0,
        "×",
    );
    let mut pos = p.pos_s.unwrap_or(x0);
    let r = panel::row(
        ui,
        tr("Position"),
        tr("the time the cursor and the spectra show"),
        |ui| ui.add(egui::Slider::new(&mut pos, x0..=x1).show_value(false)),
    );
    if r.changed() {
        p.pos_s = Some(pos);
    }
    let columns = app.time_view.columns;
    let column = p.column.map_or(0, |c| c + 1);
    ui.label(
        RichText::new(format!("{pos:.2} s · column {column} of {columns}"))
            .small()
            .color(MUTED),
    );
}
