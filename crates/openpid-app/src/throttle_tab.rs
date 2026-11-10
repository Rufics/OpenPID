//! The Freq × throttle tab in the axis-row layout (docs/design/throttle-tab,
//! T1): roll / pitch / yaw stacked; per log two columns, the unfiltered
//! counterpart (prefilt) and the chosen quantity, sharing one colour scale;
//! the throttle axis zoomed to the range the logs flew. Every control is in
//! the panel on the right.

use crate::app::App;
use crate::axis_layout::{PlotCell, control_panel};
use crate::i18n::tr;
use crate::panel;
use crate::spectrum_tab::Quantity;
use crate::views::{
    BAR_W, colour_bar, draw_rpm_overlay, effective_max, grid_image, heatmap_hover, hover_label,
    marker_hlines, show_error, slot_color, sparse_bins,
};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, RichText, Ui};
use openpid_analysis::{ThrottleHeatmapOutput, ThrottleHeatmapParams};
use openpid_core::{Axis, ChannelId};
use std::sync::Arc;

/// Narrowest heatmap cell; more logs than fit scroll sideways.
const MIN_CELL_W: f32 = 200.0;
/// Rows above 20 Hz set the automatic colour scale (below is the flying).
const SCALE_MIN_HZ: f64 = 20.0;
/// Height of a column title.
const TITLE_H: f32 = 18.0;
/// Height of the frequency label under the last row.
const X_LABEL_H: f32 = 18.0;

/// Display options of the tab (not analysis parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct ThrottleDisplay {
    /// The unfiltered counterpart beside the quantity.
    pub prefilt: bool,
    /// Zoom the throttle axis to the range the logs flew instead of the
    /// whole 0–100 % (off by default, as the reference tool).
    pub zoom_flown: bool,
}

impl Default for ThrottleDisplay {
    fn default() -> Self {
        ThrottleDisplay {
            prefilt: true,
            zoom_flown: false,
        }
    }
}

/// Per-frame view state.
#[derive(Debug, Default)]
pub struct ThrottleView {
    /// The throttle range the plots were last set to.
    pub x_range: Option<(f64, f64)>,
}

/// One heatmap: its analysis and whether it is the prefilt column.
struct Cell {
    out: Arc<Output>,
    prefilt: bool,
}

impl Cell {
    fn heatmap(&self) -> Option<&ThrottleHeatmapOutput> {
        match self.out.as_ref() {
            Output::Throttle(t) => Some(t),
            _ => None,
        }
    }
}

/// Everything drawn for one log: its rows (prefilt, quantity) and colour scale.
struct LogColumn {
    slot: Slot,
    rows: Vec<(Axis, Vec<Cell>)>,
    scale: f64,
}

/// The tab.
pub fn throttle(app: &mut App, ui: &mut Ui) {
    control_panel(app, ui, "throttle-panel", controls);
    let full_w = ui.available_width();
    ui.separator();
    let quantity = Quantity::of(&app.tools.heatmap_channel);
    let display = app.tools.throttle_display.clone();
    // the export and the project keep the selected axis
    app.tools.throttle.channel = quantity.channel(app.tools.axis);
    let base = app.tools.throttle.clone();
    let mut logs: Vec<LogColumn> = Vec::new();
    for slot in app.visible_slots() {
        let rows = crate::axis_layout::shown_axes(app)
            .into_iter()
            .map(|axis| (axis, cells(app, ui, slot, quantity, &display, &base, axis)))
            .collect();
        logs.push(LogColumn {
            slot,
            rows,
            scale: 0.0,
        });
    }
    scales(app, &mut logs);
    let range = throttle_range(&logs, app.tools.min_segments, !display.zoom_flown);
    let reset = app.throttle_view.x_range != Some(range);
    app.throttle_view.x_range = Some(range);
    app.heatmap_hover = None;
    let columns = logs
        .iter()
        .map(|l| l.rows.first().map_or(0, |r| r.1.len()))
        .max()
        .unwrap_or(1)
        .max(1);
    let gap = ui.spacing().item_spacing.x;
    let group_w =
        (full_w / logs.len().max(1) as f32).max(columns as f32 * (MIN_CELL_W + gap) + BAR_W);
    let h = ui.available_height() - TITLE_H - X_LABEL_H - 22.0;
    let n = crate::axis_layout::shown_axes(app).len() as f32;
    let row_h = (h / n - ui.spacing().item_spacing.y).max(60.0);
    egui::ScrollArea::horizontal()
        .id_salt("thr-groups")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for log in &logs {
                    let cell_w = ((group_w - BAR_W - gap) / columns as f32 - gap).max(MIN_CELL_W);
                    group(app, ui, log, cell_w, row_h, reset, range);
                }
            });
        });
    let hover = app.heatmap_hover;
    let min_segments = app.tools.min_segments;
    ui.horizontal_wrapped(|ui| {
        for log in &logs {
            if let Some(line) = sampled_summary(log, min_segments) {
                ui.label(RichText::new(line).color(crate::colormap::MUTED));
                ui.separator();
            }
        }
        hover_label(ui, hover, false);
    });
}

/// "A: 76 of 101 throttle bins have fewer than 5 segments (dimmed)": how
/// much of the throttle range a log sampled.
fn sampled_summary(log: &LogColumn, min_segments: usize) -> Option<String> {
    let t = log
        .rows
        .iter()
        .flat_map(|(_, cells)| cells.iter())
        .find_map(Cell::heatmap)?;
    let below = t.counts.iter().filter(|&&c| c < min_segments).count();
    Some(if below > 0 {
        format!(
            "{}: {below} of {} throttle bins have fewer than {min_segments} segments (dimmed)",
            log.slot.name(),
            t.counts.len()
        )
    } else {
        format!(
            "{}: every throttle bin has at least {min_segments} segments",
            log.slot.name()
        )
    })
}

/// The heatmaps of one log and axis: the prefilt counterpart (when shown and
/// logged) and the quantity.
fn cells(
    app: &mut App,
    ui: &mut Ui,
    slot: Slot,
    q: Quantity,
    d: &ThrottleDisplay,
    base: &ThrottleHeatmapParams,
    axis: Axis,
) -> Vec<Cell> {
    let Some(session) = app.loaded(slot).and_then(|l| l.current().cloned()) else {
        return Vec::new();
    };
    let prefilt = q.prefilt(axis).filter(|_| d.prefilt).map(|c| (c, true));
    let wanted: Vec<(ChannelId, bool)> = prefilt
        .into_iter()
        .chain(std::iter::once((q.channel(axis), false)))
        .filter(|(c, _)| session.channel(c).is_some())
        .collect();
    let mut out = Vec::new();
    for (channel, prefilt) in wanted {
        let params = ThrottleHeatmapParams {
            channel,
            ..base.clone()
        };
        match app.result(slot, Kind::Throttle(params)) {
            Some(Ok(o)) => out.push(Cell { out: o, prefilt }),
            Some(Err(e)) => show_error(ui, slot, &e),
            None => {}
        }
    }
    out
}

/// Colour maximum per log: the larger of its heatmaps (prefilt and quantity
/// share it, so the filters' work reads as a darker map), or with "link
/// scales" the largest of all logs; a fixed colour max wins. Recorded in
/// `app.colour_scales`.
fn scales(app: &mut App, logs: &mut [LogColumn]) {
    let fixed = app.tools.heatmap_max;
    for log in logs.iter_mut() {
        log.scale = log
            .rows
            .iter()
            .flat_map(|(_, cells)| cells.iter())
            .filter_map(Cell::heatmap)
            .map(|t| effective_max(&t.grid, fixed, SCALE_MIN_HZ))
            .fold(0.0_f64, f64::max);
    }
    if app.tools.heatmap_link && logs.len() > 1 {
        let shared = logs.iter().map(|l| l.scale).fold(0.0_f64, f64::max);
        for log in logs.iter_mut() {
            log.scale = shared;
        }
    }
    app.colour_scales = logs.iter().map(|l| (l.slot, l.scale)).collect();
}

/// The throttle range to show: from the first to the last bin with at least
/// `min_segments` segments in any heatmap, to the edges of those bins (a
/// wider margin would only add dimmed, unsampled bins); 0–100 % when asked
/// for or when nothing is sampled.
fn throttle_range(logs: &[LogColumn], min_segments: usize, full: bool) -> (f64, f64) {
    if full {
        return (0.0, 100.0);
    }
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for t in logs
        .iter()
        .flat_map(|l| l.rows.iter())
        .flat_map(|(_, cells)| cells.iter())
        .filter_map(Cell::heatmap)
    {
        let dx = match t.grid.x.as_slice() {
            [a, b, ..] => b - a,
            _ => 1.0,
        };
        for (x, c) in t.grid.x.iter().zip(&t.counts) {
            if *c >= min_segments {
                lo = lo.min(x - dx / 2.0);
                hi = hi.max(x + dx / 2.0);
            }
        }
    }
    if lo.is_finite() && hi > lo {
        (lo.max(0.0), hi.min(100.0))
    } else {
        (0.0, 100.0)
    }
}

/// One log's columns and its colour bar.
fn group(
    app: &mut App,
    ui: &mut Ui,
    log: &LogColumn,
    cell_w: f32,
    row_h: f32,
    reset: bool,
    range: (f64, f64),
) {
    let colour = slot_color(log.slot);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if let Some((_, cells)) = log.rows.first() {
                for c in cells {
                    let Some(t) = c.heatmap() else { continue };
                    let title = format!(
                        "{} {}",
                        log.slot.name(),
                        if c.prefilt {
                            tr("prefilt").to_string()
                        } else {
                            Quantity::of(&t.channel).name().to_string()
                        }
                    );
                    ui.add_sized(
                        [cell_w, TITLE_H],
                        egui::Label::new(RichText::new(title).color(colour).size(12.5)),
                    );
                }
            }
        });
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                for (i, (axis, cells)) in log.rows.iter().enumerate() {
                    let last = i + 1 == log.rows.len();
                    ui.horizontal(|ui| {
                        for (j, c) in cells.iter().enumerate() {
                            let at = PlotCell {
                                slot: log.slot,
                                axis: *axis,
                                size: egui::vec2(
                                    cell_w,
                                    if last { row_h + X_LABEL_H } else { row_h },
                                ),
                                first: j == 0,
                                last,
                            };
                            heat_cell(app, ui, at, c, log.scale, range, reset);
                        }
                    });
                }
            });
            let bar_h = log.rows.len() as f32 * row_h + X_LABEL_H;
            colour_bar(ui, log.scale, bar_h);
        });
    });
}

/// One heatmap cell with its overlays, markers and hover read-out.
/// `range` is the throttle range a reset or a double-click goes back to.
fn heat_cell(
    app: &mut App,
    ui: &mut Ui,
    at: PlotCell,
    cell: &Cell,
    scale: f64,
    range: (f64, f64),
    reset: bool,
) {
    let PlotCell {
        slot,
        axis,
        size,
        first,
        last,
    } = at;
    let Some(t) = cell.heatmap() else { return };
    let y_max = t.grid.y.last().copied().unwrap_or(0.0);
    let image = grid_image(app, ui.ctx(), &cell.out, &t.grid, scale, SCALE_MIN_HZ);
    let markers = app.spectrum.markers.clone();
    let overlay = app.overlay.clone();
    let min_segments = app.tools.min_segments;
    let link = egui::Id::unique("thr-cells");
    let mut plot = crate::grid::plot(("thr", slot.name(), axis.name(), cell.prefilt))
        .width(size.x)
        .height(size.y)
        .link_axis(link, [true, true])
        .link_cursor(link, [true, true])
        .allow_scroll(false)
        .show_x(false)
        .show_y(false);
    if first {
        plot = plot.y_axis_label(format!("{} (Hz)", axis.name()));
    }
    if last {
        plot = plot.x_axis_label(tr("throttle (%)"));
    }
    let mut hover = None;
    let mut probe = None;
    plot.show(ui, |pu| {
        if reset || pu.response().double_clicked() {
            pu.set_plot_bounds_x(range.0..=range.1);
            pu.set_plot_bounds_y(0.0..=y_max);
        }
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
        hover = heatmap_hover(pu, slot, &t.grid);
        probe = Some(crate::probe::PlotProbe::of(pu.transform()));
    });
    if hover.is_some() {
        app.heatmap_hover = hover;
    }
    if let Some(probe) = probe {
        app.probes.insert(
            format!(
                "throttle/{}/{}{}",
                slot.name(),
                axis.name(),
                if cell.prefilt { "/prefilt" } else { "" }
            ),
            probe,
        );
    }
}

/// The panel of the tab.
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
        "throttle-quantity",
        tr("Channel"),
        tr("what the rows show, on roll, pitch and yaw"),
        &mut q,
        &quantities,
    );
    app.tools.heatmap_channel = q.channel(Axis::Roll);
    panel::check_enabled(
        ui,
        q.prefilt(Axis::Roll).is_some(),
        &mut app.tools.throttle_display.prefilt,
        tr("Prefilt beside it"),
        tr("the unfiltered gyro in a column of its own, on the same colour scale; the gyro only"),
    );
    panel::section(ui, tr("Colour"));
    let mut max = (app.tools.heatmap_max > 0.0).then_some(app.tools.heatmap_max);
    panel::optional(
        ui,
        tr("Colour max"),
        tr("the amplitude drawn in the brightest colour; auto: the 99th percentile above 20 Hz"),
        &mut max,
        10.0,
        "",
    );
    app.tools.heatmap_max = max.unwrap_or(0.0);
    let several = app.visible_slots().len() > 1;
    panel::check_enabled(
        ui,
        several,
        &mut app.tools.heatmap_link,
        tr("Link scales"),
        tr(
            "one colour scale for every log (the largest automatic maximum), so the same colour means the same value",
        ),
    );
    overlay_rows(app, ui);
    panel::section(ui, tr("Range"));
    panel::check(
        ui,
        &mut app.tools.throttle_display.zoom_flown,
        tr("Zoom to the flown range"),
        tr("the throttle axis from the first to the last sampled bin instead of 0–100 %"),
    );
    panel::section(ui, tr("Analysis"));
    panel::drag(
        ui,
        tr("Segment"),
        tr(
            "length of each spectrum along the log: longer gives finer frequency steps, fewer segments per throttle bin",
        ),
        &mut app.tools.throttle.segment_s,
        0.01,
        0.05..=1.0,
        "s",
    );
    panel::drag(
        ui,
        tr("Throttle window ±"),
        tr("each throttle bin averages the segments within this distance of it"),
        &mut app.tools.throttle.half_window,
        0.5,
        0.0..=25.0,
        "%",
    );
    panel::drag(
        ui,
        tr("Max frequency"),
        tr("top of the frequency axis"),
        &mut app.tools.throttle.max_hz,
        10.0,
        50.0..=8000.0,
        "Hz",
    );
    panel::drag_usize(
        ui,
        tr("Min segments"),
        tr("throttle bins with fewer segments are dimmed: they are unsampled, not quiet"),
        &mut app.tools.min_segments,
        1..=1000,
    );
}

/// The motor-harmonic overlay rows, when a visible log has motor speeds.
fn overlay_rows(app: &mut App, ui: &mut Ui) {
    let motors = app
        .cache
        .values()
        .filter_map(|c| match c {
            crate::app::Cached::Ready(o) => match o.as_ref() {
                Output::Throttle(t) => t.overlay.as_ref().map(|ov| ov.motors.len()),
                _ => None,
            },
            _ => None,
        })
        .max();
    let Some(n) = motors else { return };
    panel::section(ui, tr("Overlays"));
    let ov = &mut app.overlay;
    ov.motors.resize(n, true);
    panel::check(
        ui,
        &mut ov.show,
        tr("RPM overlay"),
        tr(
            "the motors' speed and its harmonics over the map: noise that follows them is motor noise",
        ),
    );
    panel::check(
        ui,
        &mut ov.per_motor,
        tr("Per motor"),
        tr("a coloured line per motor and harmonic instead of one line at the mean of the motors"),
    );
    if ov.per_motor {
        ui.horizontal(|ui| {
            for (m, on) in ov.motors.iter_mut().enumerate() {
                ui.checkbox(on, format!("M{}", m + 1))
                    .on_hover_text(tr("show this motor's lines"));
            }
        });
    }
    let mut harmonics = ov.harmonics as usize;
    panel::drag_usize(
        ui,
        tr("Harmonics"),
        tr("how many harmonics per motor; 0 = as many as the RPM filter removes"),
        &mut harmonics,
        0..=8,
    );
    ov.harmonics = u32::try_from(harmonics).unwrap_or(0);
    panel::check(
        ui,
        &mut ov.notches,
        tr("Notches"),
        tr("the static notches, the dynamic-notch band and the RPM filter minimum"),
    );
}
