//! The Freq × time tab in the axis-row layout (docs/design/time-tab, F2):
//! roll / pitch / yaw spectrograms of the docked log, the spectrum at the
//! player's position beside each row, and the throttle under them on the
//! same time axis. Every map is the one of DESIGN §6.5, computed as the
//! reference tool computes its own: a power density in decibels, on
//! −30 … 10 dB unless another scale is chosen (there is no linear colour).
//! Every control, the player's included, is in the panel on the right.

use crate::app::App;
use crate::axis_layout::{PlotCell, control_panel};
use crate::colormap::{ACCENT, MUTED};
use crate::i18n::tr;
use crate::panel;
use crate::shortcuts::{self, Action};
use crate::spectrum_tab::Quantity;
use crate::views::{
    BAR_W, PlayerRequest, colour_bar_range, heatmap_hover, hover_label, marker_hlines,
    nearest_column, show_error, slot_color,
};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Line, PlotPoint, PlotPoints, VLine};
use openpid_analysis::render::Colormap;
use openpid_analysis::stft::DB_SCALE;
use openpid_analysis::{FreqSmoothing, StftOutput, StftParams, TimeSmoothing};
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
/// The lowest and the highest top of the frequency axis that can be typed in.
const MIN_TOP_HZ: f64 = 20.0;
const MAX_TOP_HZ: f64 = 16_000.0;
/// The ends of the colour scale that can be typed in, dB.
const TYPED_DB: std::ops::RangeInclusive<f64> = -300.0..=300.0;

/// What ends the colour scale of the maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColourScale {
    /// The reference tool's scale, −30 … 10 dB.
    #[default]
    Fixed,
    /// From the maps: the 30th and the 99.5th percentile of their decibels
    /// above 20 Hz.
    Automatic,
    /// The two ends typed in the panel.
    Typed,
}

/// Display options of the tab (not analysis parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct TimeDisplay {
    /// For the gyro: the unfiltered gyro (as the reference tool's default),
    /// where the motor noise shows.
    pub prefilt: bool,
    /// What ends the colour scale.
    pub scale: ColourScale,
    /// The ends of the typed colour scale, dB.
    pub z_min: f64,
    pub z_max: f64,
}

impl Default for TimeDisplay {
    fn default() -> Self {
        TimeDisplay {
            prefilt: true,
            scale: ColourScale::default(),
            z_min: DB_SCALE.0,
            z_max: DB_SCALE.1,
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
    /// The colour scale the maps were last drawn with, dB.
    pub range: Option<(f64, f64)>,
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
            let cell = PlotCell {
                slot,
                axis: row.axis,
                size: egui::vec2(plot_w, row_h),
                first: true,
                last: i + 1 == rows.len(),
            };
            let drawn = spectrogram(app, ui, cell, row, pos, range);
            clicked_at = clicked_at.or(drawn.clicked);
            if i == 0 {
                app.player.column = column;
            }
            if let Some(probe) = drawn.probe {
                app.probes
                    .insert(format!("time/{}", row.axis.name()), probe);
            }
            let beside = PlotCell {
                size: egui::vec2(SLICE_W, row_h),
                first: false,
                ..cell
            };
            slice(ui, beside, t, column, range);
            colour_bar_range(ui, range.0, range.1, row_h);
        });
    }
    if strip_h > 0.0 {
        throttle_strip(app, ui, slot, pos, span, egui::vec2(plot_w, strip_h));
    }
    hover_label(ui, app.heatmap_hover, true);
    if let Some(x) = clicked_at {
        pos = x.clamp(span.0, span.1);
    }
    app.player.pos_s = Some(pos);
    app.player.synced_cursor = Some(pos);
    app.viewer.cursor_s = Some(pos);
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

/// The colour scale of every row (shared, so the rows compare): the
/// reference tool's fixed −30 … 10 dB, the two ends typed in the panel, or
/// the automatic range of the rows.
fn colour_range(app: &mut App, rows: &[Row], d: &TimeDisplay) -> (f64, f64) {
    let range = match d.scale {
        ColourScale::Fixed => DB_SCALE,
        ColourScale::Typed => (d.z_min, d.z_max.max(d.z_min + 1.0)),
        ColourScale::Automatic => automatic_range(app, rows),
    };
    if let Some(slot) = app.dock_slot() {
        app.colour_scales = vec![(slot, range.1)];
    }
    app.time_view.range = Some(range);
    range
}

/// The lowest floor and the highest top of the rows' own ranges
/// ([`auto_db_range`], once per result).
fn automatic_range(app: &mut App, rows: &[Row]) -> (f64, f64) {
    if app.time_view.ranges.len() > 64 {
        app.time_view.ranges.clear();
    }
    let ranges = &mut app.time_view.ranges;
    rows.iter()
        .filter_map(|row| {
            let grid = &row.stft()?.grid;
            let key = Arc::as_ptr(&row.out) as usize;
            Some(*ranges.entry(key).or_insert_with(|| auto_db_range(grid)))
        })
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        .unwrap_or(DB_SCALE)
}

/// Percentiles of the dB values above `SCALE_MIN_HZ`: the floor on the
/// background noise, the top on the strongest lines.
pub fn auto_db_range(g: &Grid2D) -> (f64, f64) {
    let nx = g.x.len();
    let mut v: Vec<f64> =
        g.y.iter()
            .enumerate()
            .filter(|(_, f)| **f >= SCALE_MIN_HZ)
            .flat_map(|(iy, _)| g.z[iy * nx..(iy + 1) * nx].iter().copied())
            .filter(|x| x.is_finite())
            .collect();
    if v.is_empty() {
        return DB_SCALE;
    }
    v.sort_by(f64::total_cmp);
    let at = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    let (lo, hi) = (at(DB_LOW_PCT), at(DB_HIGH_PCT));
    (lo.floor(), hi.ceil().max(lo.floor() + 1.0))
}

/// "A prefilt · power density (dB) · spectrum at 24.00 s" over the rows.
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
            RichText::new(tr("power density (dB)"))
                .color(MUTED)
                .size(12.5),
        )
        .on_hover_text(tr(
            "a cell is the power of the signal per Hz in decibels, smoothed along the frequency; it is not the amplitude of a tone",
        ));
        ui.label(
            RichText::new(format!("{} {} {pos:.2} s", slot.name(), tr("spectrum at")))
                .color(MUTED)
                .size(12.5),
        );
    });
}

/// What a spectrogram reports back.
struct Drawn {
    /// Where the plot is and what it shows.
    probe: Option<crate::probe::PlotProbe>,
    clicked: Option<f64>,
}

/// One row's spectrogram with the position line, markers and hover.
fn spectrogram(
    app: &mut App,
    ui: &mut Ui,
    cell: PlotCell,
    row: &Row,
    pos: f64,
    range: (f64, f64),
) -> Drawn {
    let PlotCell { slot, size, .. } = cell;
    let mut drawn = Drawn {
        probe: None,
        clicked: None,
    };
    let Some(t) = row.stft() else { return drawn };
    let image = texture(app, ui.ctx(), &row.out, &t.grid, range);
    let markers = app.spectrum.markers.clone();
    let touch = app.touch_mode;
    let link = egui::Id::unique("time-x");
    let mut hover = None;
    let (x0, x1) = match t.grid.x.as_slice() {
        [a, .., b] => (*a, *b),
        _ => (0.0, 1.0),
    };
    crate::grid::row(("stft", slot.name(), row.axis.name()), link, Y_AXIS_W)
        .width(size.x)
        .height(size.y)
        .default_x_bounds(x0, x1)
        .set_margin_fraction(egui::Vec2::ZERO)
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
            drawn.probe = Some(crate::probe::PlotProbe::of(pu.transform()));
            hover = heatmap_hover(pu, slot, &t.grid);
        });
    if hover.is_some() {
        app.heatmap_hover = hover;
    }
    drawn
}

/// The pixels of a grid on the colour scale `lo … hi`: width, height and
/// RGBA, the highest row first. One pixel per cell while the grid fits in
/// `max_side` pixels either way; a larger grid gets `group` cells along that
/// axis in a pixel, drawn in the colour of the largest of them, so that a
/// short burst of noise stays in sight. A cell below the scale is black, not
/// transparent.
pub(crate) fn pixels(
    g: &Grid2D,
    (lo, hi): (f64, f64),
    cmap: Colormap,
    max_side: usize,
) -> (usize, usize, Vec<u8>) {
    let (nx, ny) = (g.x.len(), g.y.len());
    let max_side = max_side.max(1);
    let (gx, gy) = (nx.div_ceil(max_side).max(1), ny.div_ceil(max_side).max(1));
    let (w, h) = (nx.div_ceil(gx), ny.div_ceil(gy));
    let mut rgba = Vec::with_capacity(w * h * 4);
    for py in (0..h).rev() {
        for px in 0..w {
            let mut v = f64::NEG_INFINITY;
            for iy in py * gy..((py + 1) * gy).min(ny) {
                for z in &g.z[iy * nx + px * gx..iy * nx + ((px + 1) * gx).min(nx)] {
                    v = v.max(*z);
                }
            }
            let t = ((v - lo) / (hi - lo)).max(1e-6);
            rgba.extend_from_slice(&cmap.rgba(t));
        }
    }
    (w, h, rgba)
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
    let cmap = crate::colormap::heatmap();
    let max_side = ctx.input(|i| i.max_texture_side);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        "time",
        Arc::as_ptr(owner) as usize,
        lo.to_bits(),
        hi.to_bits(),
        cmap,
    )
        .hash(&mut hasher);
    let key = hasher.finish();
    if !app.heatmap_textures.contains_key(&key) {
        if app.heatmap_textures.len() >= 96 {
            app.heatmap_textures.clear();
        }
        let (w, h, rgba) = pixels(g, (lo, hi), cmap, max_side);
        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let handle = ctx.load_texture(format!("time-{key:x}"), image, egui::TextureOptions::LINEAR);
        app.heatmap_textures.insert(key, handle);
    }
    let tex = &app.heatmap_textures[&key];
    // the cells a pixel holds (`pixels` makes at most `max_side` pixels of
    // equal groups, so the group is the cells over the pixels, rounded up);
    // the last pixel may reach past the grid
    let [w, h] = tex.size();
    let (gx, gy) = (nx.div_ceil(w), ny.div_ceil(h));
    let dx = g.x[1] - g.x[0];
    let dy = g.y[1] - g.y[0];
    let (x0, y0) = (g.x[0] - dx / 2.0, g.y[0] - dy / 2.0);
    let (x1, y1) = (x0 + dx * (w * gx) as f64, y0 + dy * (h * gy) as f64);
    Some(
        egui_plot::PlotImage::new(
            "power density",
            tex.id(),
            PlotPoint::new(f64::midpoint(x0, x1), f64::midpoint(y0, y1)),
            egui::vec2((x1 - x0) as f32, (y1 - y0) as f32),
        )
        .allow_hover(false),
    )
}

/// The spectrum at the player's column beside a row: frequency up, decibels
/// across, on the colour range of the maps.
fn slice(ui: &mut Ui, cell: PlotCell, t: &StftOutput, column: Option<usize>, (lo, hi): (f64, f64)) {
    let PlotCell {
        slot, axis, size, ..
    } = cell;
    let nx = t.grid.x.len();
    let pts: Vec<[f64; 2]> = column
        .map(|c| {
            t.grid
                .y
                .iter()
                .enumerate()
                .map(|(iy, f)| [t.grid.z[iy * nx + c], *f])
                .collect()
        })
        .unwrap_or_default();
    let y_max = t.grid.y.last().copied().unwrap_or(0.0);
    // egui_plot's own grid, not `grid::plot`: the slice has no axes, its
    // lines are only a faint backdrop (the look the owner approved)
    egui_plot::Plot::new(("stft-slice", slot.name(), axis.name()))
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
    crate::grid::row(("time-throttle", slot.name()), link, Y_AXIS_W)
        .width(size.x)
        .height(size.y)
        .default_x_bounds(x0, x1)
        .set_margin_fraction(egui::Vec2::ZERO)
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
    colour_scale_rows(app, ui);
    player_rows(app, ui);
    panel::section(ui, tr("Analysis"));
    level_choice(
        ui,
        "time-smooth-freq",
        tr("Smooth frequency"),
        tr(
            "how many rows a cell is smoothed over along the frequency: off none, low 5, medium 11, high 21; higher reads calmer and lowers and widens the lines",
        ),
        &mut app.tools.stft.smooth_freq,
        &FreqSmoothing::ALL.map(|level| (level, level.name())),
    );
    level_choice(
        ui,
        "time-smooth-time",
        tr("Smooth time"),
        tr(
            "how close the columns are: each is the spectrum of 0.3 s of the log, 0.3 s apart (off), 0.15 (low), 0.06 (medium) or 0.03 s (high); nothing is averaged along the time",
        ),
        &mut app.tools.stft.smooth_time,
        &TimeSmoothing::ALL.map(|level| (level, level.name())),
    );
    let mut top = app.tools.stft.max_hz;
    panel::optional(
        ui,
        tr("Max frequency"),
        tr("top of the frequency axis; auto: half the logging rate, as far as the map goes"),
        &mut top,
        1000.0,
        "Hz",
    );
    app.tools.stft.max_hz = top.map(|hz| hz.clamp(MIN_TOP_HZ, MAX_TOP_HZ));
}

/// One of the two lists of levels (off, low, medium, high), named in the
/// interface language.
fn level_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    label: &str,
    hint: &str,
    value: &mut T,
    levels: &[(T, &'static str)],
) {
    let options: Vec<(T, String)> = levels
        .iter()
        .map(|(level, name)| (*level, tr(name).to_string()))
        .collect();
    panel::choice(ui, id, label, hint, value, &options);
}

/// "Colour scale" and, for a typed one, its two ends.
fn colour_scale_rows(app: &mut App, ui: &mut Ui) {
    panel::section(ui, tr("Colour"));
    let d = &mut app.tools.time_display;
    let was = d.scale;
    let options = [
        (
            ColourScale::Fixed,
            format!("{:.0} … {:.0} dB", DB_SCALE.0, DB_SCALE.1).replace('-', "−"),
        ),
        (ColourScale::Automatic, tr("automatic").to_string()),
        (ColourScale::Typed, tr("typed in").to_string()),
    ];
    panel::choice(
        ui,
        "time-scale",
        tr("Colour scale"),
        tr(
            "where the colours end, in decibels: the fixed scale, on which every log reads alike; automatic, from the background noise to the strongest lines of the maps, for a quiet log; or two ends typed in",
        ),
        &mut d.scale,
        &options,
    );
    if d.scale != ColourScale::Typed {
        return;
    }
    if was != ColourScale::Typed
        && let Some((lo, hi)) = app.time_view.range
    {
        // start from the scale on screen
        (d.z_min, d.z_max) = (lo, hi);
    }
    panel::drag(
        ui,
        tr("Z min"),
        tr("the value drawn black"),
        &mut d.z_min,
        1.0,
        TYPED_DB,
        "dB",
    );
    panel::drag(
        ui,
        tr("Z max"),
        tr("the value drawn brightest; at least 1 dB above Z min"),
        &mut d.z_max,
        1.0,
        TYPED_DB,
        "dB",
    );
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
