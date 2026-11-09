//! The tabs. Every view reads analysis outputs from the app cache and draws
//! with `egui_plot`; nothing here computes.

use crate::app::{App, Loaded, SlotState};
use crate::colormap::{ACCENT, MUTED, SLOT_PALETTE};
use crate::i18n::{Lang, tr};
use crate::prefs::{AccUnit, Prefs, RateUnit, Theme};
use crate::shortcuts::{self, Action};
use crate::worker::{Kind, Output, Slot};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{HLine, Heatmap, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, VLine};
use openpid_analysis::{
    Epoch, FilterOverrides, Fitness, FitnessLevel, RpmOverlay, TargetPreset, Verdict, VerdictLevel,
};
use openpid_core::analysis::Grid2D;
use openpid_core::settings::LowpassKind;
use openpid_core::settings::{PerAxis, PidGains};
use openpid_core::units::Unit;
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage};
use openpid_dsp::Peak;
use openpid_sim::{ChainModel, LowpassModel};
use std::collections::HashMap;
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

/// Shades every slot's epoch on a time plot. Painted over the plot rather
/// than as a plot polygon: a polygon spanning the visible y range would feed
/// the auto-bounds and grow them by the margin fraction every frame.
fn shade_epochs(ui: &Ui, app: &App, transform: &egui_plot::PlotTransform) {
    let frame = *transform.frame();
    let painter = ui.painter().with_clip_rect(frame);
    for slot in app.slot_ids() {
        let Some(e) = app.loaded(slot).and_then(|l| l.epoch) else {
            continue;
        };
        let x0 = transform.position_from_point_x(e.start_s);
        let x1 = transform.position_from_point_x(e.end_s);
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, frame.top()), egui::pos2(x1, frame.bottom())),
            0.0,
            slot_color(slot).gamma_multiply(0.12),
        );
    }
}

/// Draggable epoch bounds of every slot; returns the handle under the pointer.
fn draw_epochs(pu: &mut egui_plot::PlotUi<'_>, app: &App, grab_px: f32) -> Option<(Slot, bool)> {
    let hover_pos = pu.response().hover_pos();
    let transform = *pu.transform();
    let mut hovered = None;
    for slot in app.slot_ids() {
        let Some(e) = app.loaded(slot).and_then(|l| l.epoch) else {
            continue;
        };
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
    /// Peaks found on the last frame, per slot and axis (for the read-out and tests).
    pub peaks: Vec<(Slot, Axis, Vec<Peak>)>,
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

/// A player move asked by a keyboard shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRequest {
    ToStart,
    StepBack,
    StepForward,
}

/// Playback over the STFT columns of the freq × time heatmap (#30): the
/// position is the log-viewer cursor, so the two tabs stay in step.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    /// Advancing every frame.
    pub playing: bool,
    /// A move asked by a shortcut, applied by the controls next frame.
    pub request: Option<PlayerRequest>,
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
            request: None,
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
pub(crate) fn hz(f: f64) -> String {
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{f:.1} Hz")
    }
}

/// Frequency bin of `freq` closest to `x`.
pub(crate) fn nearest_bin(freq: &[f64], x: f64) -> Option<usize> {
    freq.iter()
        .enumerate()
        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
        .map(|(i, _)| i)
}

pub(crate) fn slot_color(slot: Slot) -> Color32 {
    SLOT_PALETTE[slot.0 as usize % SLOT_PALETTE.len()]
}

/// Colour of a slot's simulated (what-if) trace on Filter Sim: distinct from
/// the slot's own colour, which draws the logged signal.
fn sim_color(slot: Slot) -> Color32 {
    match slot.0 {
        0 => ACCENT,
        1 => Color32::LIGHT_GREEN,
        n => SLOT_PALETTE[(n as usize + 3) % SLOT_PALETTE.len()],
    }
}

/// Width of one panel when `n` slots share the tab side by side: half the
/// tab for two, never below 480 px (the row scrolls) for more.
pub(crate) fn panel_width(ui: &Ui, n: usize) -> f32 {
    let n = n.max(1) as f32;
    let w = ui.available_width() / n;
    if n <= 2.0 { w } else { w.max(480.0) }
}

/// The log chips in the top bar (docs/design/app-shell, R4): one chip per
/// slot and "+" to add a log. The bar keeps its height however many logs
/// are open; a click picks the log the timeline dock shows.
pub(crate) fn log_chips(app: &mut App, ui: &mut Ui) {
    let top_left = ui.cursor().min;
    let active = app.dock_slot();
    let compact = app
        .slots
        .iter()
        .filter(|e| !matches!(e.state, SlotState::Empty))
        .count()
        > 2;
    for slot in app.slot_ids() {
        if matches!(app.slot_state(slot), Some(SlotState::Empty)) {
            continue;
        }
        if log_chip(app, ui, slot, active == Some(slot), compact).clicked() {
            app.dock = Some(slot);
        }
    }
    let full = app.slots.len() >= Slot::MAX;
    let add = ui.add_enabled(
        !full,
        egui::Button::new(RichText::new("+").strong()).min_size(egui::vec2(28.0, 0.0)),
    );
    add.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !full, tr("Add log")));
    let add_rect = add.rect;
    if add
        .on_hover_text(shortcuts::hint(
            ui.ctx(),
            tr("Add log: open another log to compare (up to 8), or drop it on the window"),
            Action::OpenLogNext,
        ))
        .clicked()
        && let Some(slot) = app.free_slot()
    {
        app.dialog = Some(crate::app::Dialog::OpenLog(slot));
    }
    app.anchors
        .insert("slot-bar", egui::Rect::from_min_max(top_left, add_rect.max));
}

/// One log chip: the slot's dot and letter, the file name (only the
/// active one when more than two logs are open) and the log check's mark.
fn log_chip(app: &App, ui: &mut Ui, slot: Slot, active: bool, compact: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let visible = app.slot_entry(slot).is_some_and(|e| e.visible);
    let (name, mark, accessible) = match app.slot_state(slot) {
        Some(SlotState::Ready(_)) => {
            let loaded = app.loaded(slot).expect("ready");
            let file = loaded
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = loaded
                .path
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let max = if compact { 14 } else { 18 };
            let short: String = if stem.chars().count() > max {
                stem.chars().take(max - 1).chain(['…']).collect()
            } else {
                stem
            };
            let mark = loaded.fitness().map(|f| f.overall);
            ((!compact || active).then_some(short), mark, file)
        }
        Some(SlotState::Loading(p)) => (
            Some(format!("{}…", tr("loading"))),
            None,
            format!("{} {}", tr("loading"), p.display()),
        ),
        Some(SlotState::Failed(p, e)) => (
            None,
            Some(FitnessLevel::Fail),
            format!("{} — {e}", p.display()),
        ),
        Some(SlotState::Empty) | None => (None, None, tr("empty").to_string()),
    };
    let strong = ui.visuals().strong_text_color();
    let letter = ui
        .painter()
        .layout_no_wrap(slot.name().to_string(), font.clone(), strong);
    let name = name.map(|n| {
        ui.painter()
            .layout_no_wrap(n, font.clone(), ui.visuals().text_color())
    });
    let mut w = 8.0 + 8.0 + 6.0 + letter.size().x + 8.0;
    if let Some(g) = &name {
        w += g.size().x + 6.0;
    }
    if mark.is_some() {
        w += 14.0;
    }
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
    let label = format!("{} {}: {accessible}", tr("Slot"), slot.name());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, &label));
    let v = ui.visuals();
    let (fill, stroke) = if active {
        (
            crate::theme::ACCENT.gamma_multiply(0.22),
            egui::Stroke::new(1.0, crate::theme::ACCENT.gamma_multiply(0.8)),
        )
    } else if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, v.widgets.hovered.bg_stroke)
    } else {
        (
            v.widgets.inactive.weak_bg_fill,
            v.widgets.inactive.bg_stroke,
        )
    };
    let p = ui.painter();
    p.rect(rect, 6.0, fill, stroke, egui::StrokeKind::Inside);
    let cy = rect.center().y;
    let mut x = rect.left() + 8.0;
    let dot = if visible {
        slot_color(slot)
    } else {
        slot_color(slot).gamma_multiply(0.3)
    };
    p.circle_filled(egui::pos2(x + 4.0, cy), 4.0, dot);
    x += 14.0;
    let lw = letter.size();
    p.galley(egui::pos2(x, cy - lw.y / 2.0), letter, strong);
    x += lw.x + 6.0;
    if let Some(g) = name {
        let gs = g.size();
        p.galley(egui::pos2(x, cy - gs.y / 2.0), g, strong);
        x += gs.x + 6.0;
    }
    if let Some(level) = mark {
        paint_mark(p, egui::pos2(x + 4.0, cy), level, level_color(level));
    }
    resp.on_hover_text(label)
}

/// Height of the timeline dock's content.
pub(crate) const DOCK_H: f32 = 84.0;

/// The timeline dock at the bottom (docs/design/app-shell, R4): the
/// active log's name, firmware, length, rate, session, log check and its
/// show / solo / close icons over its timeline strip. Returns whether the
/// epoch or the session changed.
pub(crate) fn timeline_dock(app: &mut App, ui: &mut Ui) -> bool {
    let Some(slot) = app.dock_slot() else {
        return false;
    };
    match app.slot_state(slot) {
        Some(SlotState::Ready(_)) => dock_body(app, ui, slot),
        Some(SlotState::Loading(p)) => {
            let text = format!(
                "{} {}: {} {}",
                tr("Slot"),
                slot.name(),
                tr("loading"),
                p.display()
            );
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add(egui::Label::new(RichText::new(text).color(MUTED)).truncate());
            });
            false
        }
        Some(SlotState::Failed(p, e)) => {
            let text = format!("Slot {}: {} — {e}", slot.name(), p.display());
            ui.add(egui::Label::new(RichText::new(text).color(Color32::LIGHT_RED)).wrap());
            false
        }
        Some(SlotState::Empty) | None => {
            ui.label(
                RichText::new(format!("{} {}: {}", tr("Slot"), slot.name(), tr("empty")))
                    .color(MUTED),
            );
            false
        }
    }
}

fn dock_body(app: &mut App, ui: &mut Ui, slot: Slot) -> bool {
    let mut changed = false;
    let visible = app.slot_entry(slot).is_some_and(|e| e.visible);
    let mut action: Option<SlotAction> = None;
    let loaded = app.loaded_mut(slot).expect("ready");
    ui.horizontal(|ui| {
        let colour = slot_color(slot);
        slot_disc(
            ui,
            slot,
            if visible {
                colour
            } else {
                colour.gamma_multiply(0.35)
            },
        );
        let name = loaded
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        ui.label(RichText::new(name).strong());
        if let Some(s) = loaded.current() {
            let meta = [
                format!(
                    "{} {}",
                    s.firmware.vendor.name(),
                    s.firmware
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                ),
                format!("{:.1} s", s.timebase.duration_s()),
                format!("{:.0} Hz", s.timebase.sample_rate_hz()),
            ];
            ui.label(RichText::new(meta.join("  ·  ")).color(MUTED));
        }
        let n = loaded.sessions.len();
        if n > 1 {
            let before = loaded.session;
            let l = ui.label(RichText::new(tr("session")).color(MUTED));
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
                loaded.apply_auto_epoch();
                changed = true;
            }
        }
        if let Some(f) = loaded.fitness() {
            fitness_badges(ui, f);
        }
        if let Some(s) = loaded.current()
            && s.diagnostics.corrupt_frames > 0
        {
            pill(
                ui,
                &format!("{} corrupt frames", s.diagnostics.corrupt_frames),
                FitnessLevel::Warn,
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(
                ui,
                CardIcon::Close,
                format!("{} {}", tr("close"), slot.name()),
            )
            .clicked()
            {
                action = Some(SlotAction::Close);
            }
            if icon_button(
                ui,
                CardIcon::Solo,
                format!("{} {}", tr("solo"), slot.name()),
            )
            .clicked()
            {
                action = Some(SlotAction::Solo);
            }
            let eye = format!(
                "{} {}",
                if visible { tr("hide") } else { tr("show") },
                slot.name()
            );
            if icon_button(ui, CardIcon::Eye(visible), eye).clicked() {
                action = Some(SlotAction::Visible(!visible));
            }
        });
    });
    match action {
        Some(SlotAction::Visible(v)) => app.set_visible(slot, v),
        Some(SlotAction::Solo) => app.solo(slot),
        Some(SlotAction::Close) => app.close_slot(slot),
        None => {}
    }
    if app.loaded(slot).is_some() {
        changed |= timeline_strip(app, ui, slot);
    }
    changed
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

/// Colour of a fitness level (green / yellow / red, colour-blind safe).
fn level_color(level: FitnessLevel) -> Color32 {
    match level {
        FitnessLevel::Ok => Color32::from_rgb(0, 158, 115),
        FitnessLevel::Warn => crate::theme::warn(),
        FitnessLevel::Fail => Color32::from_rgb(213, 94, 0),
    }
}

/// Fitness badges in a slot row: the overall marker and raw / rpm / sticks
/// (docs/design/log-fitness, R5); hovering lists every check.
fn fitness_badges(ui: &mut Ui, f: &Fitness) {
    let summary: String = f
        .checks
        .iter()
        .map(|c| format!("{} {}: {}", c.level.marker(), c.title, c.detail))
        .collect::<Vec<_>>()
        .join("\n");
    let overall = match f.overall {
        FitnessLevel::Ok => tr("log ok"),
        FitnessLevel::Warn => tr("log usable"),
        FitnessLevel::Fail => tr("log unfit"),
    };
    pill(ui, overall, f.overall).on_hover_text(summary);
    for id in ["raw_gyro", "erpm", "sticks"] {
        if let Some(c) = f.check(id) {
            pill(ui, &c.title, c.level).on_hover_text(match &c.fix {
                Some(fix) => format!("{}\n{fix}", c.detail),
                None => c.detail.clone(),
            });
        }
    }
}

/// The log check's fix for a missing unfiltered gyro, for the tabs that need it.
fn raw_gyro_fix(app: &App, slot: Slot) -> String {
    app.loaded(slot)
        .and_then(|l| l.fitness())
        .and_then(|f| f.check("raw_gyro"))
        .and_then(|c| c.fix.clone())
        .unwrap_or_else(|| "log the unfiltered gyro".to_string())
}

/// The "Log check" card on Diagnostics: every check with its fix to copy.
fn fitness_card(ui: &mut Ui, slot: Slot, f: &Fitness) {
    ui.heading(format!("{} {}", tr("Log check"), f.overall.marker()));
    egui::Grid::new(("fitness", slot.name()))
        .striped(true)
        .show(ui, |ui| {
            for c in &f.checks {
                ui.label(RichText::new(c.level.marker()).color(level_color(c.level)));
                ui.label(RichText::new(&c.title).strong());
                ui.label(&c.detail);
                if let Some(fix) = &c.fix {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(fix).monospace());
                        if ui
                            .small_button(tr("Copy"))
                            .on_hover_text(format!("{} {}", tr("copy"), c.title))
                            .clicked()
                        {
                            ui.ctx().copy_text(fix.clone());
                        }
                    });
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    ui.add_space(6.0);
}

/// What the chip buttons of a slot header asked for.
enum SlotAction {
    Visible(bool),
    Solo,
    Close,
}

/// Paint the mark of a fitness level centred on `c`: a tick, an
/// exclamation mark or a cross, so the level reads without its colour.
pub(crate) fn paint_mark(p: &egui::Painter, c: egui::Pos2, level: FitnessLevel, colour: Color32) {
    let stroke = egui::Stroke::new(1.6, colour);
    match level {
        FitnessLevel::Ok => {
            p.add(egui::Shape::line(
                vec![
                    c + egui::vec2(-3.5, 0.0),
                    c + egui::vec2(-1.0, 2.8),
                    c + egui::vec2(3.8, -3.2),
                ],
                stroke,
            ));
        }
        FitnessLevel::Warn => {
            p.line_segment(
                [c + egui::vec2(0.0, -4.0), c + egui::vec2(0.0, 1.2)],
                stroke,
            );
            p.circle_filled(c + egui::vec2(0.0, 3.6), 0.9, colour);
        }
        FitnessLevel::Fail => {
            let d = 3.2;
            p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], stroke);
            p.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], stroke);
        }
    }
}

/// A pill: rounded tinted background, the text and the level's mark in its
/// colour (fitness badges); read out as `raw [ok]`.
pub(crate) fn pill(ui: &mut Ui, text: &str, level: FitnessLevel) -> egui::Response {
    let colour = level_color(level);
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), colour);
    let size = egui::vec2(galley.size().x + 28.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let label = format!("{text} {}", level.marker());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    let p = ui.painter();
    p.rect_filled(rect, 9.0, colour.gamma_multiply(0.16));
    p.galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    paint_mark(
        p,
        egui::pos2(rect.right() - 11.0, rect.center().y),
        level,
        colour,
    );
    resp
}

/// The small painted icons of a log card.
#[derive(Clone, Copy)]
enum CardIcon {
    /// Shown (`true`) or hidden.
    Eye(bool),
    Solo,
    Close,
}

/// A frameless 22 px icon button with an accessible name.
fn icon_button(ui: &mut Ui, icon: CardIcon, name: String) -> egui::Response {
    let size = egui::vec2(22.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let visuals = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, visuals.weak_bg_fill);
    }
    let c = rect.center();
    let colour = if resp.hovered() {
        visuals.fg_stroke.color
    } else {
        MUTED
    };
    let stroke = egui::Stroke::new(1.4, colour);
    let p = ui.painter();
    match icon {
        CardIcon::Eye(shown) => {
            let pts: Vec<egui::Pos2> = (0..=24)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 24.0;
                    c + egui::vec2(7.0 * t.cos(), 4.2 * t.sin())
                })
                .collect();
            p.add(egui::Shape::line(pts, stroke));
            if shown {
                p.circle_filled(c, 2.2, colour);
            } else {
                p.line_segment(
                    [c + egui::vec2(-7.0, 6.0), c + egui::vec2(7.0, -6.0)],
                    stroke,
                );
            }
        }
        CardIcon::Solo => {
            p.circle_stroke(c, 6.5, stroke);
            p.circle_filled(c, 2.5, colour);
        }
        CardIcon::Close => {
            let d = 4.5;
            p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], stroke);
            p.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], stroke);
        }
    }
    resp.on_hover_text(name)
}

/// The slot letter on a disc in the slot's colour; named "Slot A" for
/// screen readers.
fn slot_disc(ui: &mut Ui, slot: Slot, colour: Color32) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Label,
            true,
            format!("{} {}", tr("Slot"), slot.name()),
        )
    });
    ui.painter().circle_filled(rect.center(), 10.0, colour);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        slot.name(),
        egui::FontId::proportional(12.0),
        Color32::from_rgb(16, 18, 22),
    );
}

/// What a drag on a timeline strip moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StripDrag {
    Start,
    End,
    /// The whole window; the pointer was this far (s) from its start.
    Move {
        grab_offset_s: f64,
    },
}

/// State of the timeline strips (docs/design/analysis-window, R2).
#[derive(Debug, Default)]
pub struct StripState {
    /// The handle or window being dragged, if any.
    pub drag: Option<(Slot, StripDrag)>,
    /// Screen rectangle of every strip last frame (tests, hit tests).
    pub rects: HashMap<Slot, egui::Rect>,
    /// Throttle (or gyro) min/max envelope per pixel column, by slot,
    /// session and width.
    envelopes: HashMap<(Slot, usize, u32), Envelope>,
}

/// Min/max per pixel column, 0..=1.
type Envelope = Arc<Vec<[f32; 2]>>;

impl StripState {
    /// Drop everything cached for a slot (closed or reloaded).
    pub fn forget(&mut self, slot: Slot) {
        self.rects.remove(&slot);
        self.envelopes.retain(|k, _| k.0 != slot);
        if self.drag.is_some_and(|(s, _)| s == slot) {
            self.drag = None;
        }
    }

    /// Min/max of the throttle (percent of its range) or, without throttle,
    /// of |gyro.filtered.roll|, for `columns` pixel columns; values in 0..=1.
    fn envelope(
        &mut self,
        slot: Slot,
        session_index: usize,
        session: &FlightSession,
        columns: u32,
    ) -> Envelope {
        let key = (slot, session_index, columns);
        if let Some(e) = self.envelopes.get(&key) {
            return e.clone();
        }
        let values: Vec<f64> = match session.channel(&ChannelId::Throttle) {
            Some(c) => c.data.values().iter().map(|v| v / 100.0).collect(),
            None => session
                .channel(&ChannelId::Gyro {
                    axis: Axis::Roll,
                    stage: GyroStage::Filtered,
                })
                .map(|c| {
                    let v = c.data.values();
                    let max = v.iter().fold(0.0_f64, |m, x| m.max(x.abs())).max(1e-9);
                    v.iter().map(|x| x.abs() / max).collect()
                })
                .unwrap_or_default(),
        };
        let n = values.len();
        let cols = columns.max(1) as usize;
        let mut out = vec![[1.0_f32, 0.0_f32]; cols];
        for (i, v) in values.iter().enumerate() {
            if !v.is_finite() {
                continue;
            }
            let c = (i * cols / n.max(1)).min(cols - 1);
            let v = v.clamp(0.0, 1.0) as f32;
            out[c][0] = out[c][0].min(v);
            out[c][1] = out[c][1].max(v);
        }
        for e in &mut out {
            if e[0] > e[1] {
                *e = [0.0, 0.0];
            }
        }
        let e = Arc::new(out);
        self.envelopes.insert(key, e.clone());
        e
    }
}

/// Height of a timeline strip.
const STRIP_H: f32 = 36.0;

/// The timeline strip of a slot: the throttle envelope with the epoch window
/// and its two handles, `start`/`end` fields, Auto and All. Returns whether
/// the epoch changed.
fn timeline_strip(app: &mut App, ui: &mut Ui, slot: Slot) -> bool {
    let touch = app.touch_mode;
    let Some(loaded) = app.loaded(slot) else {
        return false;
    };
    let Some(session) = loaded.current().cloned() else {
        return false;
    };
    let duration = session.timebase.duration_s();
    if duration <= 0.0 {
        return false;
    }
    let session_index = loaded.session;
    let whole = Epoch {
        start_s: 0.0,
        end_s: duration,
    };
    let mut e = loaded.epoch.unwrap_or(whole);
    let was_auto = loaded.epoch_auto;
    // (new epoch, auto) once something changed it
    let mut update: Option<(Option<Epoch>, bool)> = None;
    let mut status: Option<String> = None;
    ui.horizontal(|ui| {
        // room for start, end, Auto, All and the summary; the summary
        // truncates rather than push the row past the window
        // (docs/design/axis-layout: a wider row stretched the whole tab)
        let fields_w = if touch { 540.0 } else { 470.0 };
        let width = (ui.available_width() - fields_w).max(120.0);
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, STRIP_H), egui::Sense::click_and_drag());
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Group,
                true,
                format!("{} {}", slot.name(), tr("timeline")),
            )
        });
        let resp = resp.on_hover_text(tr("drag a handle or the window; double-click = whole log"));
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals();
        painter.rect_filled(rect, 2.0, visuals.extreme_bg_color);
        let cols = rect.width().max(1.0).round() as u32;
        let env = app.strip.envelope(slot, session_index, &session, cols);
        let grey = visuals.weak_text_color();
        for (i, [lo, hi]) in env.iter().enumerate() {
            let x = rect.left() + i as f32 + 0.5;
            let y0 = rect.bottom() - lo * rect.height();
            let y1 = rect.bottom() - hi * rect.height();
            painter.line_segment(
                [egui::pos2(x, y0 - 1.0), egui::pos2(x, y1)],
                egui::Stroke::new(1.0, grey),
            );
        }
        let x_of = |t: f64| rect.left() + (t / duration) as f32 * rect.width();
        let t_of =
            |x: f32| (f64::from((x - rect.left()) / rect.width()) * duration).clamp(0.0, duration);
        let colour = slot_color(slot);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x_of(e.start_s), rect.top()),
                egui::pos2(x_of(e.end_s), rect.bottom()),
            ),
            0.0,
            colour.gamma_multiply(0.25),
        );
        let grab = grab_radius(touch);
        let dragging = app.strip.drag.filter(|(s, _)| *s == slot).map(|(_, d)| d);
        for (t, which) in [(e.start_s, StripDrag::Start), (e.end_s, StripDrag::End)] {
            let x = x_of(t);
            let active = dragging == Some(which)
                || resp.hover_pos().is_some_and(|p| (p.x - x).abs() <= grab);
            painter.rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(x, rect.center().y),
                    egui::vec2(if active { 8.0 } else { 4.0 }, rect.height()),
                ),
                1.0,
                colour,
            );
        }
        // interaction
        // the drag is recognised a few pixels after the press: test the press origin
        if resp.drag_started()
            && let Some(p) = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| resp.interact_pointer_pos())
        {
            let d = if (p.x - x_of(e.start_s)).abs() <= grab {
                Some(StripDrag::Start)
            } else if (p.x - x_of(e.end_s)).abs() <= grab {
                Some(StripDrag::End)
            } else if p.x > x_of(e.start_s) && p.x < x_of(e.end_s) {
                Some(StripDrag::Move {
                    grab_offset_s: t_of(p.x) - e.start_s,
                })
            } else {
                None
            };
            app.strip.drag = d.map(|d| (slot, d));
        }
        if resp.dragged()
            && let Some((s, d)) = app.strip.drag
            && s == slot
            && let Some(p) = resp.interact_pointer_pos()
        {
            let t = t_of(p.x);
            let next = match d {
                StripDrag::Start => Epoch {
                    start_s: t.min(e.end_s),
                    end_s: e.end_s,
                },
                StripDrag::End => Epoch {
                    start_s: e.start_s,
                    end_s: t.max(e.start_s),
                },
                StripDrag::Move { grab_offset_s } => {
                    let len = e.end_s - e.start_s;
                    let start = (t - grab_offset_s).clamp(0.0, duration - len);
                    Epoch {
                        start_s: start,
                        end_s: start + len,
                    }
                }
            };
            if next != e {
                e = next;
                update = Some((Some(e), false));
            }
        }
        if resp.drag_stopped() {
            app.strip.drag = None;
        }
        if resp.double_clicked() {
            update = Some((None, false));
        }
        app.strip.rects.insert(slot, rect);
        if app.first_loaded() == Some(slot) {
            app.anchors.insert("epoch-handles", rect);
        }
        // fields
        let l = ui.label(tr("start"));
        let a = crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut e.start_s)
                .speed(0.1)
                .range(0.0..=duration),
            " s",
        )
        .labelled_by(l.id);
        let l = ui.label(tr("end"));
        let b = crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut e.end_s)
                .speed(0.1)
                .range(0.0..=duration),
            " s",
        )
        .labelled_by(l.id);
        if a.changed() || b.changed() {
            update = Some((
                Some(Epoch {
                    start_s: e.start_s.min(e.end_s),
                    end_s: e.end_s.max(e.start_s),
                }),
                false,
            ));
        }
        ui.menu_button(tr("Auto"), |ui| {
            if ui.button(tr("Trim take-off and landing")).clicked() {
                ui.close();
                match openpid_analysis::epoch::auto(&session) {
                    Some(auto) => update = Some((Some(auto), true)),
                    None => {
                        status = Some(format!("{}: {}", slot.name(), tr("log too short to trim")))
                    }
                }
            }
            if ui.button(tr("Most active 20 s")).clicked() {
                ui.close();
                match openpid_analysis::epoch::most_active(
                    &session,
                    openpid_analysis::epoch::ACTIVE_WINDOW_S,
                ) {
                    Some(active) => update = Some((Some(active), false)),
                    None => {
                        status = Some(format!("{}: {}", slot.name(), tr("log shorter than 20 s")))
                    }
                }
            }
        });
        if ui
            .button(tr("All"))
            .on_hover_text(shortcuts::hint(
                ui.ctx(),
                tr("whole log"),
                Action::EpochWhole,
            ))
            .clicked()
        {
            update = Some((None, false));
        }
        let mut text = format!(
            "{:.1}–{:.1} s · {:.1} s",
            e.start_s,
            e.end_s,
            e.end_s - e.start_s
        );
        if was_auto && update.is_none() {
            text.push_str(" · auto");
        }
        ui.add(egui::Label::new(RichText::new(text).color(MUTED)).truncate());
    });
    if let Some(s) = status {
        app.status = s;
    }
    let Some((epoch, auto)) = update else {
        return false;
    };
    if let Some(l) = app.loaded_mut(slot) {
        l.set_epoch(epoch, auto);
    }
    true
}

pub(crate) fn axis_selector(ui: &mut Ui, axis: &mut Axis) -> bool {
    let before = *axis;
    ui.horizontal(|ui| {
        ui.label(tr("Axis"));
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
    let l = ui.label(tr("channel"));
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
pub(crate) fn grid_heatmap(
    name: &str,
    g: &Grid2D,
    max: f64,
    min_y_for_scale: f64,
) -> Option<Heatmap> {
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
        ui.label(tr("Channels:"));
        let filter = &mut app.viewer.channel_filter;
        let l = ui.label(tr("filter"));
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text(tr("filter…"))
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
                RichText::new(tr(
                    "Click or tap the plot to place the cursor, then set the epoch from it.",
                ))
                .color(MUTED),
            );
            return;
        };
        ui.label(format!("cursor: {t:.3} s"));
        for slot in app.loaded_slots() {
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
        let hue = (ci as f32 / n_lines + f32::from(slot.0) * 0.37) % 1.0;
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
    ui.separator();
    let session_a: Option<Arc<FlightSession>> = app
        .first_loaded()
        .and_then(|s| app.loaded(s))
        .and_then(|l| l.current().cloned());
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
    let visible = app.visible_slots();
    let n_lines = channels.len().max(1) as f32 * visible.len().max(1) as f32;
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
    let plot = Plot::new("log_viewer")
        .legend(Legend::default())
        .x_axis_label("time (s)")
        .height(plot_h)
        .allow_boxed_zoom(!touch)
        .allow_drag(!handle_interaction)
        .show(ui, |pu| {
            let px = pu.response().rect.width().max(100.0) as usize;
            for slot in &visible {
                viewer_plot_slot(pu, app, *slot, &channels, n_lines, px);
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
    shade_epochs(ui, app, &plot.transform);
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

/// Columns of the Step tab verdict table, in `verdicts()` order.
pub(crate) const STEP_COLUMNS: [&str; 7] = [
    "windows",
    "rise",
    "delay",
    "latency",
    "overshoot",
    "settling",
    "final",
];

/// What the Step tab drew last frame (tests).
#[derive(Debug, Default)]
pub struct StepViewState {
    /// Target band: from (ms), low, high.
    pub band: Option<(f64, f64, f64)>,
    /// The Advice line under the table, if the recommender proposed a PID change on the axis.
    pub advice: Option<String>,
    /// Verdicts per slot and axis, as in the table.
    pub rows: Vec<(Slot, Axis, Vec<Verdict>)>,
}

/// Colour of a verdict level (Okabe–Ito green / yellow / vermilion, grey).
pub(crate) fn verdict_color(level: VerdictLevel) -> Color32 {
    match level {
        VerdictLevel::Ok => Color32::from_rgb(0, 158, 115),
        VerdictLevel::Warn => crate::theme::warn(),
        VerdictLevel::Fail => Color32::from_rgb(213, 94, 0),
        VerdictLevel::Unknown => MUTED,
    }
}

/// Manual spectrum markers as horizontal lines on a frequency-axis heatmap.
pub(crate) fn marker_hlines(pu: &mut egui_plot::PlotUi<'_>, markers: &[f64]) {
    for (i, m) in markers.iter().enumerate() {
        pu.hline(
            HLine::new(format!("marker {}", i + 1), *m)
                .color(ACCENT)
                .style(LineStyle::dashed_loose())
                .width(1.5),
        );
    }
}

pub(crate) fn heatmap_controls(app: &mut App, ui: &mut Ui) {
    let session_a = app
        .first_loaded()
        .and_then(|s| app.loaded(s))
        .and_then(|l| l.current().cloned());
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
        let l = ui.label(tr("colour max (0 = auto)"));
        ui.add(
            egui::DragValue::new(&mut app.tools.heatmap_max)
                .speed(0.5)
                .range(0.0..=10000.0),
        )
        .labelled_by(l.id);
        let several = app.visible_slots().len() > 1;
        ui.add_enabled(
            several,
            egui::Checkbox::new(&mut app.tools.heatmap_link, tr("link scales")),
        )
        .on_hover_text(tr(
            "one colour scale for every log (the largest automatic maximum), so the same colour means the same value",
        ));
    });
}

/// Width taken by the colour scale beside a heatmap.
pub(crate) const BAR_W: f32 = 56.0;

/// The colour maximum a grid is drawn with: `max` when set, else the robust
/// automatic maximum over the rows at or above `min_y`.
pub(crate) fn effective_max(g: &Grid2D, max: f64, min_y: f64) -> f64 {
    openpid_analysis::surface::colour_max(
        g,
        &openpid_analysis::surface::SurfaceOptions {
            max,
            min_y_for_scale: min_y,
            ..openpid_analysis::surface::SurfaceOptions::default()
        },
    )
}

/// Colour maximum per slot for one tab: each grid's own, or, with "link
/// scales", the largest of them for all; recorded in `app.colour_scales`.
pub(crate) fn linked_scales(
    app: &mut App,
    grids: &[(Slot, &Grid2D)],
    min_y: f64,
) -> HashMap<Slot, f64> {
    let max = app.tools.heatmap_max;
    let mut per: Vec<(Slot, f64)> = grids
        .iter()
        .map(|(slot, g)| (*slot, effective_max(g, max, min_y)))
        .collect();
    if app.tools.heatmap_link && per.len() > 1 {
        let shared = per.iter().map(|(_, m)| *m).fold(0.0_f64, f64::max);
        for (_, m) in &mut per {
            *m = shared;
        }
    }
    app.colour_scales.clone_from(&per);
    per.into_iter().collect()
}

/// A vertical colour scale: the viridis gradient from 0 (bottom) to `max`
/// (top) with five ticks and the unit (docs/design/colour-scale, R1).
pub(crate) fn colour_bar(ui: &mut Ui, max: f64, unit: &str, height: f32) -> egui::Response {
    use openpid_analysis::render::VIRIDIS;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(BAR_W, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let bar = egui::Rect::from_min_size(
        rect.min + egui::vec2(2.0, 8.0),
        egui::vec2(14.0, (height - 28.0).max(10.0)),
    );
    let mut mesh = egui::Mesh::default();
    let n = VIRIDIS.len();
    for (i, c) in VIRIDIS.iter().enumerate() {
        let t = i as f32 / (n - 1) as f32;
        let y = bar.max.y - t * bar.height();
        let colour = Color32::from_rgb(c[0], c[1], c[2]);
        mesh.colored_vertex(egui::pos2(bar.min.x, y), colour);
        mesh.colored_vertex(egui::pos2(bar.max.x, y), colour);
        if i > 0 {
            let b = (2 * i) as u32;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(
        bar,
        0.0,
        egui::Stroke::new(1.0, MUTED),
        egui::StrokeKind::Outside,
    );
    let font = egui::FontId::proportional(10.0);
    let colour = ui.visuals().text_color();
    let fmt = |v: f64| {
        if v >= 100.0 {
            format!("{v:.0}")
        } else if v >= 10.0 {
            format!("{v:.1}")
        } else {
            format!("{v:.2}")
        }
    };
    for k in 0..=4 {
        let f = k as f32 / 4.0;
        let y = bar.max.y - f * bar.height();
        painter.line_segment(
            [egui::pos2(bar.max.x, y), egui::pos2(bar.max.x + 3.0, y)],
            egui::Stroke::new(1.0, colour),
        );
        painter.text(
            egui::pos2(bar.max.x + 5.0, y),
            egui::Align2::LEFT_CENTER,
            fmt(max * f64::from(f)),
            font.clone(),
            colour,
        );
    }
    painter.text(
        egui::pos2(bar.min.x, bar.max.y + 4.0),
        egui::Align2::LEFT_TOP,
        unit,
        font,
        colour,
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Label,
            true,
            format!("colour scale 0 to {} {unit}", fmt(max)),
        )
    });
    resp
}

/// The grid cell under the plot pointer: `(slot, x, y, value)`.
pub(crate) fn heatmap_hover(
    pu: &egui_plot::PlotUi<'_>,
    slot: Slot,
    g: &Grid2D,
) -> Option<(Slot, f64, f64, f64)> {
    let c = pu.pointer_coordinate()?;
    let z = openpid_analysis::surface::value_at(g, c.x, c.y)?;
    Some((slot, c.x, c.y, z))
}

/// Read-out of the hovered cell under a heatmap (`x` in percent or seconds).
pub(crate) fn hover_label(
    ui: &mut Ui,
    hover: Option<(Slot, f64, f64, f64)>,
    unit: &str,
    x_is_time: bool,
) {
    let text = match hover {
        Some((_, x, y, z)) if x_is_time => format!("{x:.2} s, {y:.0} Hz: {z:.3} {unit}"),
        Some((_, x, y, z)) => format!("{x:.0} %, {y:.0} Hz: {z:.3} {unit}"),
        None => tr("hover the map for a value").to_string(),
    };
    ui.label(RichText::new(text).color(MUTED));
}

/// Textures kept before the cache is emptied (one per result and colour scale).
const MAX_HEATMAP_TEXTURES: usize = 96;

/// A heatmap grid as a texture, rasterised once per result and colour scale
/// and redrawn as one image instead of a cell mesh every frame. `owner` is
/// the result the grid belongs to (its address keys the texture while it is
/// cached). Returns the image item positioned over the grid.
pub(crate) fn grid_image(
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
            ui.checkbox(&mut ov.show, tr("RPM overlay"));
            for (m, on) in ov.motors.iter_mut().enumerate() {
                ui.checkbox(on, format!("M{}", m + 1));
            }
            let l = ui.label(tr("harmonics"));
            ui.add(egui::DragValue::new(&mut ov.harmonics).range(0..=8))
                .labelled_by(l.id)
                .on_hover_text(tr("0 = as many as the RPM filter removes"));
            ui.checkbox(&mut ov.notches, tr("notches"));
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
pub(crate) fn draw_rpm_overlay(
    pu: &mut egui_plot::PlotUi<'_>,
    o: &RpmOverlay,
    x: &[f64],
    y_max: f64,
    ov: &OverlayState,
) {
    if ov.show {
        draw_harmonics(pu, o, x, y_max, ov);
    }
    if ov.notches {
        draw_notches(pu, o, x, y_max);
    }
}

/// Runs of consecutive throttle bins where harmonic `k` of a motor is known
/// and below `y_max`: one line each.
fn harmonic_runs(x: &[f64], bins: &[Option<f64>], k: u32, y_max: f64) -> Vec<Vec<[f64; 2]>> {
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut run: Vec<[f64; 2]> = Vec::new();
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
    runs
}

/// Every shown motor's harmonic lines: the fundamental solid, the rest dashed.
fn draw_harmonics(
    pu: &mut egui_plot::PlotUi<'_>,
    o: &RpmOverlay,
    x: &[f64],
    y_max: f64,
    ov: &OverlayState,
) {
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
            for pts in harmonic_runs(x, bins, k, y_max) {
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

/// A filled horizontal band from `lo` to `hi` across the throttle range.
fn notch_band(
    pu: &mut egui_plot::PlotUi<'_>,
    (x0, x1): (f64, f64),
    name: String,
    (lo, hi): (f64, f64),
    colour: Color32,
) {
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
}

/// The static notches, the dynamic-notch band and the RPM filter minimum.
fn draw_notches(pu: &mut egui_plot::PlotUi<'_>, o: &RpmOverlay, x: &[f64], y_max: f64) {
    let span = (
        x.first().copied().unwrap_or(0.0),
        x.last().copied().unwrap_or(100.0),
    );
    for (i, (centre, cutoff)) in o.notches.iter().enumerate() {
        let half = (centre - cutoff).abs();
        notch_band(
            pu,
            span,
            format!("notch {} band", i + 1),
            ((centre - half).max(0.0), (centre + half).min(y_max)),
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
        notch_band(
            pu,
            span,
            "dyn notch band".into(),
            (lo.max(0.0), hi.min(y_max)),
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

/// Grey columns over the throttle bins with fewer than `min_segments`
/// segments, so an unsampled bin does not read as a quiet one.
pub(crate) fn sparse_bins(
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

pub fn time_heatmap(app: &mut App, ui: &mut Ui) {
    let markers = app.spectrum.markers.clone();
    ui.separator();
    heatmap_controls(app, ui);
    ui.horizontal_wrapped(|ui| {
        let l = ui.label(tr("segment"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.stft.segment_s)
                .speed(0.01)
                .range(0.02..=1.0),
            " s",
        )
        .labelled_by(l.id);
        let mut max = app.tools.stft.max_hz.unwrap_or(0.0);
        let l = ui.label(tr("max"));
        if crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut max)
                .speed(10.0)
                .range(0.0..=8000.0),
            " Hz",
        )
        .labelled_by(l.id)
        .changed()
        {
            app.tools.stft.max_hz = if max > 0.0 { Some(max) } else { None };
        }
    });
    app.tools.stft.channel = app.tools.heatmap_channel.clone();
    let params = app.tools.stft.clone();
    let max = app.tools.heatmap_max;
    let touch = app.touch_mode;
    let slots: Vec<Slot> = app.visible_slots();
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
    let grids: Vec<(Slot, &Grid2D)> = results
        .iter()
        .filter_map(|(s, r)| match r {
            Some(Ok(o)) => match o.as_ref() {
                Output::Stft(t) => Some((*s, &t.grid)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let scales = linked_scales(app, &grids, 20.0);
    app.heatmap_hover = None;
    // two panels share the height; more scroll at a readable minimum
    let h = (ui.available_height() / slots.len().max(1) as f32).max(260.0);
    let mut clicked_at: Option<f64> = None;
    egui::ScrollArea::vertical()
        .id_salt("stft-panels")
        .show(ui, |ui| {
            for (i, (slot, result)) in results.into_iter().enumerate() {
                match result {
                    Some(Ok(o)) => {
                        if let Output::Stft(t) = o.as_ref() {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {} ({})",
                                    slot.name(),
                                    t.channel.key(),
                                    t.unit
                                ))
                                .color(slot_color(slot)),
                            );
                            let scale = scales.get(&slot).copied().unwrap_or(max);
                            let image = grid_image(app, ui.ctx(), &o, &t.grid, scale, 20.0);
                            let column = pos.and_then(|p| nearest_column(&t.grid.x, p));
                            let spectrum_h = ((h - 24.0) * 0.3).max(60.0);
                            let plot_h = h - 24.0 - spectrum_h - 20.0;
                            let plot_w = ui.available_width() - BAR_W;
                            let mut frame = None;
                            let mut x_bounds = None;
                            let mut hover = None;
                            ui.horizontal(|ui| {
                                Plot::new(("stft", slot.name()))
                                    // the map fills the frame: no blank
                                    // margin around the epoch
                                    .set_margin_fraction(egui::Vec2::ZERO)
                                    .x_grid_spacer(crate::grid::spacer_x())
                                    .x_axis_formatter(crate::grid::label)
                                    .y_grid_spacer(crate::grid::spacer_y())
                                    .y_axis_formatter(crate::grid::label)
                                    .x_axis_label("time (s)")
                                    .y_axis_label("frequency (Hz)")
                                    .width(plot_w)
                                    .height(plot_h)
                                    .allow_drag(!touch)
                                    .show(ui, |pu| {
                                        if let Some(img) = image {
                                            pu.image(img);
                                        }
                                        marker_hlines(pu, &markers);
                                        if let Some(p) = pos {
                                            pu.vline(
                                                VLine::new("position", p).color(ACCENT).width(1.5),
                                            );
                                        }
                                        // click (or, on touch, drag) scrubs
                                        // (`interact_pointer_pos`: the hover position is gone during a drag)
                                        let r = pu.response();
                                        if (r.clicked() || (touch && r.dragged()))
                                            && let Some(p) = r.interact_pointer_pos()
                                        {
                                            clicked_at =
                                                Some(pu.transform().value_from_position(p).x);
                                        }
                                        let b = pu.plot_bounds();
                                        x_bounds = Some((b.min()[0], b.max()[0]));
                                        frame = Some(*pu.transform().frame());
                                        hover = heatmap_hover(pu, slot, &t.grid);
                                    });
                                colour_bar(ui, scale, &t.unit, plot_h);
                            });
                            if hover.is_some() {
                                app.heatmap_hover = hover;
                            }
                            if let Some(f) = frame {
                                app.heatmap_frames.insert(slot, f);
                            }
                            hover_label(ui, hover, &t.unit, true);
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
        });
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
    let ctx = ui.ctx().clone();
    match app.player.request.take() {
        Some(PlayerRequest::ToStart) => pos = x0,
        Some(PlayerRequest::StepBack) => pos = step(pos, -1),
        Some(PlayerRequest::StepForward) => pos = step(pos, 1),
        None => {}
    }
    ui.horizontal_wrapped(|ui| {
        let p = &mut app.player;
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
            pos = x0;
        }
        if ui
            .button(tr("< step"))
            .on_hover_text(shortcuts::keys_of(&ctx, Action::StepBack))
            .clicked()
        {
            pos = step(pos, -1);
        }
        if ui
            .button(tr("step >"))
            .on_hover_text(shortcuts::keys_of(&ctx, Action::StepForward))
            .clicked()
        {
            pos = step(pos, 1);
        }
        let l = ui.label(tr("speed"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut p.speed)
                .speed(0.1)
                .range(0.1..=16.0),
            "×",
        )
        .labelled_by(l.id);
        let l = ui.label(tr("position"));
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
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for slot in app.loaded_slots() {
            let Some(l) = app.loaded(slot) else { continue };
            let Some(s) = l.current() else { continue };
            ui.heading(format!("Slot {} — {}", slot.name(), s.firmware.revision));
            if let Some(f) = l.fitness() {
                fitness_card(ui, slot, f);
            }
            egui::Grid::new(("diag", slot.name()))
                .striped(true)
                .show(ui, |ui| {
                    ui.label(tr("profile"));
                    ui.label(&s.profile_id);
                    ui.end_row();
                    ui.label(tr("samples"));
                    ui.label(format!(
                        "{} over {:.2} s, median dt {} µs, {} gaps",
                        s.len(),
                        s.timebase.duration_s(),
                        s.timebase.median_dt_us,
                        s.timebase.gaps.len()
                    ));
                    ui.end_row();
                    ui.label(tr("frames"));
                    ui.label(
                        s.diagnostics
                            .frames
                            .iter()
                            .map(|(k, v)| format!("{k}:{}", v.valid))
                            .collect::<Vec<_>>()
                            .join("  "),
                    );
                    ui.end_row();
                    ui.label(tr("corrupt / desync / skipped bytes"));
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
                        ui.label(tr("unmapped fields"));
                        ui.label(s.diagnostics.unmapped_fields.join(", "));
                        ui.end_row();
                    }
                    for n in &s.diagnostics.notes {
                        ui.label(tr("note"));
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
            ui.label(RichText::new(tr("axis")).strong());
            ui.label(RichText::new(tr("from sliders")).strong());
            ui.label(RichText::new(tr("logged")).strong());
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
                        .color(if differs { ACCENT } else { slot_color(Slot::A) }),
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
    ui.separator();
    let first = app.first_loaded().unwrap_or(Slot::A);
    let Some((session_idx, session)) = app
        .loaded(first)
        .and_then(|l| l.current().cloned().map(|s| (l.session, s)))
    else {
        ui.label(
            RichText::new(tr(
                "Load a Betaflight 4.3+ log into slot A to use the PID sliders.",
            ))
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
    if app.slider_state.loaded_for != Some((first, session_idx)) {
        app.slider_state.loaded_for = Some((first, session_idx));
        app.slider_state.values = defs.iter().map(|d| (d.key.clone(), d.value)).collect();
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(model.id()).strong());
        ui.label(&session.firmware.revision);
        if ui.button(tr("Reset to log")).clicked() {
            app.slider_state.values = defs.iter().map(|d| (d.key.clone(), d.value)).collect();
        }
        if ui.button(tr("Firmware defaults")).clicked() {
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
    ui.label(tr("Betaflight CLI to apply (select all, copy):"));
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

/// Advice tab: the recommender's proposals with their evidence, and the
/// Betaflight CLI commands that apply them.
pub fn advice(app: &mut App, ui: &mut Ui) {
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        let l = ui.label(tr("overshoot limit"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.recommend.targets.overshoot_warn_pct)
                .speed(1.0)
                .range(1.0..=100.0),
            " %",
        )
        .labelled_by(l.id)
        .on_hover_text(tr(
            "the same target as on the Step tab (Settings → Targets)",
        ));
        let l = ui.label(tr("D step"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.recommend.d_step_pct)
                .speed(1.0)
                .range(1.0..=50.0),
            " %",
        )
        .labelled_by(l.id);
        let l = ui.label(tr("peak prominence"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.recommend.peak_min_prominence_db)
                .speed(0.5)
                .range(1.0..=40.0),
            " dB",
        )
        .labelled_by(l.id);
        let l = ui.label(tr("LPF headroom"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.recommend.lpf_headroom_db)
                .speed(0.5)
                .range(1.0..=40.0),
            " dB",
        )
        .labelled_by(l.id);
    });
    ui.label(
        RichText::new(tr(
            "Rules are hints with their evidence, not an autotune: apply one change, fly, compare the new log.",
        ))
        .color(MUTED),
    );
    let params = app.tools.recommend.clone();
    let slots: Vec<Slot> = app.visible_slots();
    let results: Vec<SlotResult> = slots
        .iter()
        .map(|slot| (*slot, app.result(*slot, Kind::Recommend(params.clone()))))
        .collect();
    let w = panel_width(ui, slots.len());
    let mut cli_anchor: Option<egui::Rect> = None;
    egui::ScrollArea::both()
        .id_salt("advice-panels")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (slot, result) in results {
                    ui.vertical(|ui| {
                        ui.set_width(w - 8.0);
                        match result {
                            Some(Ok(o)) => {
                                if let Output::Recommend(r) = o.as_ref()
                                    && let Some(rect) = advice_slot(app, ui, slot, r)
                                {
                                    cli_anchor = Some(rect);
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
    if let Some(rect) = cli_anchor {
        app.anchors.insert("advice-cli", rect);
    }
}

fn advice_slot(
    app: &App,
    ui: &mut Ui,
    slot: Slot,
    r: &openpid_analysis::RecommendOutput,
) -> Option<egui::Rect> {
    use openpid_core::Exporter as _;
    let mut cli_rect: Option<egui::Rect> = None;
    ui.label(
        RichText::new(format!("{} {}", tr("Slot"), slot.name()))
            .color(slot_color(slot))
            .strong(),
    );
    {
        let ui = &mut *ui;
        {
            ui.heading(tr("Proposals"));
            if r.proposals.is_empty() {
                ui.label(
                    RichText::new(tr("nothing to change: every rule is satisfied")).color(MUTED),
                );
            } else {
                egui::Grid::new(("proposals", slot.name()))
                    .striped(true)
                    .show(ui, |ui| {
                        for p in &r.proposals {
                            ui.label(RichText::new(&p.path).strong());
                            ui.label(format!("{} -> {}", p.from.as_deref().unwrap_or("?"), p.to));
                            ui.label(format!("{:.0} %", p.confidence * 100.0));
                            ui.label(&p.rationale);
                            ui.end_row();
                        }
                    });
            }
            ui.add_space(6.0);
            ui.heading(tr("Evidence"));
            egui::Grid::new(("findings", slot.name()))
                .striped(true)
                .show(ui, |ui| {
                    for f in &r.findings {
                        ui.label(if f.proposed { "*" } else { "-" });
                        ui.label(
                            RichText::new(format!(
                                "{}{}",
                                f.rule,
                                f.axis.map_or(String::new(), |a| format!(" {}", a.name()))
                            ))
                            .color(MUTED),
                        );
                        ui.label(&f.text);
                        ui.end_row();
                    }
                });
            if !r.proposals.is_empty()
                && let Some(session) = app.loaded(slot).and_then(Loaded::current)
            {
                ui.add_space(6.0);
                let l = ui.label(tr("Betaflight CLI to apply (select all, copy):"));
                let mut cli =
                    match openpid_fmt_blackbox::BetaflightCli.export(session, &r.proposals) {
                        Ok(text) => text,
                        Err(e) => format!("# {e}"),
                    };
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut cli)
                        .code_editor()
                        .desired_rows(6)
                        .desired_width(f32::INFINITY)
                        .interactive(true),
                );
                cli_rect = Some(edit.rect);
                edit.labelled_by(l.id);
            }
        }
    }
    cli_rect
}

// ---------------------------------------------------------------------------

/// The start screen: shown instead of a tab while nothing is loaded
/// (docs/design/onboarding-tour, styled per docs/design/app-shell): a card
/// with the drop zone and "Open log…", the sample log, the tour, recent
/// files and the user guide.
pub fn start_screen(app: &mut App, ui: &mut Ui) {
    const CARD_W: f32 = 560.0;
    let touch = app.touch_mode;
    let avail = ui.available_rect_before_wrap();
    let w = CARD_W.min(avail.width());
    let x = avail.center().x - w / 2.0;
    let y = (avail.top() + avail.height() * 0.10).max(avail.top() + 8.0);
    let area = egui::Rect::from_min_size(
        egui::pos2(x.max(avail.left()), y),
        egui::vec2(w, avail.bottom() - y),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut child;
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::proportional(34.0);
    job.append(
        "Open",
        0.0,
        egui::TextFormat::simple(font.clone(), ui.visuals().strong_text_color()),
    );
    job.append("PID", 0.0, egui::TextFormat::simple(font, ACCENT));
    ui.label(job);
    ui.label(
        RichText::new(tr(
            "Blackbox analysis for tuning: noise, filters, step response, advice.",
        ))
        .color(MUTED),
    );
    ui.add_space(18.0);
    crate::theme::card(ui)
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            drop_zone(app, ui, touch);
            ui.add_space(12.0);
            let size = if touch {
                egui::vec2(200.0, 40.0)
            } else {
                egui::vec2(180.0, 28.0)
            };
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 2.0 * size.x - 8.0).max(0.0) / 2.0);
                if ui
                    .add(egui::Button::new(tr("Open sample log")).min_size(size))
                    .on_hover_text(tr("a synthetic 30 s flight: no file needed"))
                    .clicked()
                {
                    app.open_sample();
                }
                if ui
                    .add(egui::Button::new(tr("Start the tour")).min_size(size))
                    .on_hover_text(tr("seven steps through the tuning loop, on the sample log"))
                    .clicked()
                {
                    app.start_tour();
                }
            });
            if !app.prefs.recent.is_empty() {
                ui.add_space(14.0);
                ui.separator();
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.label(RichText::new(tr("Recent files")).strong());
                    let recent = app.prefs.recent.clone();
                    for r in &recent {
                        recent_row(app, ui, r, touch);
                    }
                });
            }
        });
    ui.add_space(12.0);
    ui.hyperlink_to(
        tr("User guide"),
        "https://github.com/Rufics/OpenPID/blob/master/docs/USER-GUIDE.md",
    );
}

/// The dashed drop zone with the primary "Open log…" button in it; it
/// lights up in the accent while a file is dragged over the window.
fn drop_zone(app: &mut App, ui: &mut Ui, touch: bool) {
    let h = if touch { 170.0 } else { 150.0 };
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    let dropping = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    if dropping {
        ui.painter()
            .rect_filled(rect, crate::theme::CARD_RADIUS, ACCENT.gamma_multiply(0.10));
    }
    crate::theme::dashed_outline(ui, rect);
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(16.0))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut inner;
    ui.add_space(6.0);
    paint_tray(ui, if dropping { ACCENT } else { MUTED });
    ui.add_space(6.0);
    ui.label(
        RichText::new(tr("Drop a log here, or"))
            .size(15.0)
            .color(if dropping {
                ACCENT
            } else {
                ui.visuals().text_color()
            }),
    );
    ui.add_space(6.0);
    let open = egui::Button::new(
        RichText::new(format!("{}…", tr("Open log")))
            .strong()
            .color(Color32::WHITE),
    )
    .fill(ACCENT)
    .min_size(if touch {
        egui::vec2(200.0, 40.0)
    } else {
        egui::vec2(180.0, 30.0)
    });
    if ui
        .add(open)
        .on_hover_text(shortcuts::keys_of(ui.ctx(), Action::OpenLog))
        .clicked()
        && let Some(slot) = app.free_slot()
    {
        app.dialog = Some(crate::app::Dialog::OpenLog(slot));
    }
    ui.add_space(4.0);
    ui.label(
        RichText::new(".bbl  ·  .bfl  ·  .txt  —  Betaflight, INAV, EmuFlight, Rotorflight")
            .size(11.5)
            .color(MUTED),
    );
}

/// A tray with an arrow into it: the drop zone's icon.
fn paint_tray(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 28.0), egui::Sense::hover());
    let p = ui.painter();
    let s = egui::Stroke::new(1.8, colour);
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    p.add(egui::Shape::line(
        vec![
            egui::pos2(l + 2.0, b - 10.0),
            egui::pos2(l + 2.0, b - 1.0),
            egui::pos2(r - 2.0, b - 1.0),
            egui::pos2(r - 2.0, b - 10.0),
        ],
        s,
    ));
    let cx = rect.center().x;
    p.line_segment([egui::pos2(cx, t + 1.0), egui::pos2(cx, b - 7.0)], s);
    p.add(egui::Shape::line(
        vec![
            egui::pos2(cx - 6.0, b - 13.0),
            egui::pos2(cx, b - 7.0),
            egui::pos2(cx + 6.0, b - 13.0),
        ],
        s,
    ));
}

/// One recent file: the name, its folder in muted text and when it was
/// opened; a click opens it.
fn recent_row(app: &mut App, ui: &mut Ui, r: &crate::prefs::RecentFile, touch: bool) {
    let name = r
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let h = if touch { 44.0 } else { 28.0 };
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name}   {}", r.opened))
    });
    let v = ui.visuals();
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, v.widgets.hovered.weak_bg_fill);
    }
    let p = ui.painter();
    let cy = rect.center().y;
    p.text(
        egui::pos2(rect.left() + 8.0, cy),
        egui::Align2::LEFT_CENTER,
        &name,
        egui::FontId::proportional(13.5),
        v.strong_text_color(),
    );
    p.text(
        egui::pos2(rect.right() - 8.0, cy),
        egui::Align2::RIGHT_CENTER,
        &r.opened,
        egui::FontId::proportional(12.0),
        MUTED,
    );
    if resp.on_hover_text(r.path.display().to_string()).clicked()
        && let Some(slot) = app.free_slot()
    {
        app.open(slot, r.path.clone());
    }
}

/// Settings tab: theme, touch mode and display units, saved on every change.
pub fn settings(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .id_salt("settings")
        .show(ui, |ui| settings_body(app, ui));
}

fn settings_body(app: &mut App, ui: &mut Ui) {
    let mut changed = false;
    ui.heading(tr("Appearance"));
    ui.horizontal(|ui| {
        ui.label(tr("Theme"));
        changed |= ui
            .radio_value(&mut app.prefs.theme, Theme::Dark, tr("Dark"))
            .changed();
        changed |= ui
            .radio_value(&mut app.prefs.theme, Theme::Light, tr("Light"))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(tr("Language"));
        for lang in Lang::ALL {
            changed |= ui
                .radio_value(&mut app.prefs.lang, lang, lang.native_name())
                .changed();
        }
    });
    if ui
        .checkbox(
            &mut app.prefs.touch_mode,
            tr("Touch mode (larger controls, touch gestures)"),
        )
        .changed()
    {
        app.touch_mode = app.prefs.touch_mode;
        changed = true;
    }
    ui.add_space(8.0);
    ui.heading(tr("Units"));
    ui.horizontal(|ui| {
        ui.label(tr("Angular rate"));
        changed |= ui
            .radio_value(&mut app.prefs.rate_unit, RateUnit::DegPerS, "deg/s")
            .changed();
        changed |= ui
            .radio_value(&mut app.prefs.rate_unit, RateUnit::RadPerS, "rad/s")
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(tr("Acceleration"));
        changed |= ui
            .radio_value(&mut app.prefs.acc_unit, AccUnit::G, "g")
            .changed();
        changed |= ui
            .radio_value(&mut app.prefs.acc_unit, AccUnit::MPerS2, "m/s²")
            .changed();
    });
    ui.label(
        RichText::new(tr(
            "Units apply to the log viewer traces; analyses stay in deg/s.",
        ))
        .color(MUTED),
    );
    ui.add_space(8.0);
    let mut targets_changed = false;
    egui::CollapsingHeader::new(tr("Targets"))
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                RichText::new(tr(
                    "Thresholds behind the step-response verdicts and the Advice overshoot rule.",
                ))
                .color(MUTED),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Preset"));
                let current = TargetPreset::matching(&app.tools.recommend.targets);
                for p in TargetPreset::ALL {
                    if ui
                        .selectable_label(current == Some(p), tr(p.name()))
                        .clicked()
                    {
                        app.tools.recommend.targets = p.targets();
                        targets_changed = true;
                    }
                }
                if current.is_none() {
                    ui.label(RichText::new(tr("Custom")).color(MUTED));
                }
            });
            egui::Grid::new("targets").show(ui, |ui| {
                let t = &mut app.tools.recommend.targets;
                let mut field = |ui: &mut Ui,
                                 label: &'static str,
                                 v: &mut f64,
                                 speed: f64,
                                 range: std::ops::RangeInclusive<f64>,
                                 suffix: &str| {
                    let l = ui.label(tr(label));
                    let r = crate::panel::with_unit(
                        ui,
                        egui::DragValue::new(v).speed(speed).range(range),
                        suffix,
                    )
                    .labelled_by(l.id);
                    targets_changed |= r.changed();
                    ui.end_row();
                };
                field(
                    ui,
                    "overshoot warn",
                    &mut t.overshoot_warn_pct,
                    1.0,
                    1.0..=100.0,
                    " %",
                );
                field(
                    ui,
                    "overshoot fail",
                    &mut t.overshoot_fail_pct,
                    1.0,
                    1.0..=200.0,
                    " %",
                );
                field(ui, "rise max", &mut t.rise_ms_max, 1.0, 5.0..=300.0, " ms");
                field(
                    ui,
                    "settling max",
                    &mut t.settling_ms_max,
                    1.0,
                    10.0..=500.0,
                    " ms",
                );
                field(
                    ui,
                    "final tolerance",
                    &mut t.final_tol,
                    0.01,
                    0.01..=0.5,
                    "",
                );
                let mut windows = |ui: &mut Ui, label: &'static str, v: &mut usize| {
                    let l = ui.label(tr(label));
                    let r = ui
                        .add(egui::DragValue::new(v).speed(1.0).range(1..=100))
                        .labelled_by(l.id);
                    targets_changed |= r.changed();
                    ui.end_row();
                };
                windows(ui, "min windows ok", &mut t.min_windows_ok);
                windows(ui, "min windows weak", &mut t.min_windows_weak);
            });
        });
    if targets_changed {
        app.prefs.targets = app.tools.recommend.targets;
        changed = true;
    }
    ui.add_space(8.0);
    if ui.button(tr("Reset to defaults")).clicked() {
        app.prefs = Prefs::default();
        app.touch_mode = app.prefs.touch_mode;
        app.tools.recommend.targets = app.prefs.targets;
        changed = true;
    }
    ui.add_space(8.0);
    match &app.prefs_path {
        Some(p) => ui.label(RichText::new(format!("Saved in {}", p.display())).color(MUTED)),
        None => ui.label(
            RichText::new(tr(
                "No configuration directory: settings last until the app closes.",
            ))
            .color(MUTED),
        ),
    };
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .checkbox(
                &mut app.prefs.show_tour,
                tr("Show the tour on the first opened log"),
            )
            .changed();
        if ui.button(tr("Reset first-run state")).clicked() {
            app.prefs.tour_done = false;
            changed = true;
        }
        if ui.button(tr("Show tour")).clicked() {
            app.start_tour();
        }
    });
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
    let mut w = egui::DragValue::new(&mut v).speed(speed).range(range);
    if integer {
        w = w.max_decimals(0);
    }
    if crate::panel::with_unit(ui, w, suffix)
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
    let l = ui.label(tr("type"));
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
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        axis_selector(ui, &mut app.tools.axis);
        app.tools.filter_sim.axis = app.tools.axis;
        ui.separator();
        let l = ui.label(tr("segment"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.filter_sim.segment_s)
                .speed(0.01)
                .range(0.02..=2.0),
            " s",
        )
        .labelled_by(l.id);
        let l = ui.label(tr("max"));
        let mut max = app.tools.filter_sim.max_hz.unwrap_or(0.0);
        if crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut max)
                .speed(10.0)
                .range(0.0..=8000.0),
            " Hz",
        )
        .labelled_by(l.id)
        .changed()
        {
            app.tools.filter_sim.max_hz = if max > 0.0 { Some(max) } else { None };
        }
        if !app.tools.filter_sim.overrides.is_empty() && ui.button(tr("Reset to log")).clicked() {
            app.tools.filter_sim.overrides = FilterOverrides::default();
        }
    });
    let axis = app.tools.axis;
    let params = app.tools.filter_sim.clone();
    let mut outputs: Vec<(Slot, Arc<Output>)> = Vec::new();
    for slot in app.visible_slots() {
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
                    "{}: no unfiltered gyro in this log; {}",
                    slot.name(),
                    raw_gyro_fix(app, slot)
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
                    .color(sim_color(*slot))
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
                        sim_color(*slot),
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
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if axis_selector(ui, &mut app.tools.axis) {
            app.tools.motor_noise.channel = ChannelId::Gyro {
                axis: app.tools.axis,
                stage: GyroStage::Raw,
            };
        }
        ui.separator();
        let l = ui.label(tr("segment"));
        crate::panel::with_unit(
            ui,
            egui::DragValue::new(&mut app.tools.motor_noise.segment_s)
                .speed(0.01)
                .range(0.05..=2.0),
            " s",
        )
        .labelled_by(l.id);
        let l = ui.label(tr("harmonics"));
        ui.add(egui::DragValue::new(&mut app.tools.motor_noise.harmonics).range(1..=8))
            .labelled_by(l.id);
    });
    let axis = app.tools.axis;
    let params = app.tools.motor_noise.clone();
    let mut outputs: Vec<(Slot, Arc<Output>)> = Vec::new();
    for slot in app.visible_slots() {
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
                    "{}: no unfiltered gyro in this log (the filtered one has the motor peaks removed); {}",
                    slot.name(),
                    raw_gyro_fix(app, slot)
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
    let w = panel_width(ui, outputs.len());
    egui::ScrollArea::horizontal()
        .id_salt("motor-panels")
        .show(ui, |ui| {
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
                                ui.label(RichText::new(tr("motor")).strong());
                                ui.label(RichText::new(tr("rpm")).strong());
                                for k in 1..=params.harmonics {
                                    ui.label(RichText::new(format!("×{k} amp (ratio)")).strong());
                                }
                                ui.label(RichText::new(tr("total")).strong());
                                ui.end_row();
                                for motor in &m.motors {
                                    let loud = m.loudest == Some(motor.idx) && m.motors.len() > 1;
                                    let colour = if loud {
                                        ACCENT
                                    } else {
                                        motor_color(usize::from(motor.idx))
                                    };
                                    ui.label(
                                        RichText::new(format!("M{}", motor.idx + 1)).color(colour),
                                    );
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
