//! The Log viewer in the axis-row layout (docs/design/log-tab, L2): roll,
//! pitch and yaw rows of the docked log at one shared scale, a row for the
//! throttle and the motors under them (and rows for motor speeds, debug and
//! other channels when chosen), the values at the cursor on every row, and
//! the quantities as a checklist in the panel on the right.

use crate::app::App;
use crate::axis_layout::control_panel;
use crate::colormap::MUTED;
use crate::i18n::tr;
use crate::panel;
use crate::views::{
    LONG_PRESS_S, LONG_PRESS_SLOP_PX, decimate, display_unit, draw_epochs, grab_radius,
    set_epoch_bound, set_epoch_from_cursor, shade_epochs,
};
use crate::worker::Slot;
use eframe::egui::{self, Color32, RichText, Ui};
use egui_plot::{Line, PlotPoints, PlotTransform, VLine};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage, PidTerm};

/// Width of the y-axis labels, the same on every row so the time axes line up.
const Y_AXIS_W: f32 = 64.0;
/// Height of an extra row (throttle / motors, RPM, debug, other) relative to
/// an axis row.
const EXTRA_WEIGHT: f32 = 0.6;

/// A quantity of the checklist: per axis (gyro … PID error) or a row of its
/// own (motors, throttle, motor speeds, debug).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogQuantity {
    Gyro,
    GyroPrefilt,
    PTerm,
    ITerm,
    DTerm,
    FTerm,
    Setpoint,
    PidSum,
    PidError,
    Motors,
    Throttle,
    MotorRpm,
    Debug,
}

impl LogQuantity {
    pub const ALL: [LogQuantity; 13] = [
        LogQuantity::Gyro,
        LogQuantity::GyroPrefilt,
        LogQuantity::PTerm,
        LogQuantity::ITerm,
        LogQuantity::DTerm,
        LogQuantity::FTerm,
        LogQuantity::Setpoint,
        LogQuantity::PidSum,
        LogQuantity::PidError,
        LogQuantity::Motors,
        LogQuantity::Throttle,
        LogQuantity::MotorRpm,
        LogQuantity::Debug,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LogQuantity::Gyro => tr("Gyro"),
            LogQuantity::GyroPrefilt => tr("Gyro prefilt"),
            LogQuantity::PTerm => tr("P-term"),
            LogQuantity::ITerm => tr("I-term"),
            LogQuantity::DTerm => tr("D-term"),
            LogQuantity::FTerm => tr("F-term"),
            LogQuantity::Setpoint => tr("Setpoint"),
            LogQuantity::PidSum => tr("PID sum"),
            LogQuantity::PidError => tr("PID error"),
            LogQuantity::Motors => tr("Motors"),
            LogQuantity::Throttle => tr("Throttle"),
            LogQuantity::MotorRpm => tr("Motor RPM"),
            LogQuantity::Debug => tr("Debug"),
        }
    }

    /// Short name for the values at the cursor.
    fn short(self) -> &'static str {
        match self {
            LogQuantity::Gyro => tr("gyro"),
            LogQuantity::GyroPrefilt => tr("prefilt"),
            LogQuantity::PTerm => "P",
            LogQuantity::ITerm => "I",
            LogQuantity::DTerm => "D",
            LogQuantity::FTerm => "F",
            LogQuantity::Setpoint => tr("setpoint"),
            LogQuantity::PidSum => tr("sum"),
            LogQuantity::PidError => tr("error"),
            LogQuantity::Motors => "M",
            LogQuantity::Throttle => tr("throttle"),
            LogQuantity::MotorRpm => "RPM",
            LogQuantity::Debug => tr("debug"),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            LogQuantity::Gyro => tr("the filtered gyro, what the PID controller sees"),
            LogQuantity::GyroPrefilt => {
                tr("the gyro before the software filters, where the noise shows")
            }
            LogQuantity::PTerm
            | LogQuantity::ITerm
            | LogQuantity::DTerm
            | LogQuantity::FTerm
            | LogQuantity::PidSum => {
                tr("a term of the PID output, in the firmware's units, on the axis rows")
            }
            LogQuantity::Setpoint => tr("the rate the pilot asked for"),
            LogQuantity::PidError => tr("setpoint minus gyro"),
            LogQuantity::Motors => tr("the motor outputs, in the row under the axes (%)"),
            LogQuantity::Throttle => tr("the throttle, in the row under the axes (%)"),
            LogQuantity::MotorRpm => {
                tr("the motor speeds from the ESC telemetry, in a row of their own")
            }
            LogQuantity::Debug => {
                tr("the debug values of the log's debug mode, in a row of their own")
            }
        }
    }

    /// The channel on `axis` for the per-axis quantities.
    fn axis_channel(self, axis: Axis) -> Option<ChannelId> {
        let term = |term| Some(ChannelId::PidTerm { axis, term });
        match self {
            LogQuantity::Gyro => Some(ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            }),
            LogQuantity::GyroPrefilt => Some(ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            }),
            LogQuantity::PTerm => term(PidTerm::P),
            LogQuantity::ITerm => term(PidTerm::I),
            LogQuantity::DTerm => term(PidTerm::D),
            LogQuantity::FTerm => term(PidTerm::F),
            LogQuantity::PidSum => term(PidTerm::Sum),
            LogQuantity::Setpoint => Some(ChannelId::Setpoint { axis }),
            LogQuantity::PidError => Some(ChannelId::PidError { axis }),
            _ => None,
        }
    }

    /// Every channel of the quantity (motors and debug: all their indices).
    fn channels(self) -> Vec<ChannelId> {
        let n = |f: fn(u8) -> ChannelId| (0..8).map(f).collect();
        match self {
            LogQuantity::Motors => n(|idx| ChannelId::Motor { idx }),
            LogQuantity::Throttle => vec![ChannelId::Throttle],
            LogQuantity::MotorRpm => n(|idx| ChannelId::MotorRpm { idx }),
            LogQuantity::Debug => n(|idx| ChannelId::Debug { idx }),
            q => Axis::ALL
                .iter()
                .filter_map(|a| q.axis_channel(*a))
                .collect(),
        }
    }

    /// Palette index of the line (Blackbox Explorer's colours: gyro yellow,
    /// setpoint red, as the reference tool).
    fn palette_index(self) -> usize {
        match self {
            LogQuantity::Gyro => 2,
            LogQuantity::GyroPrefilt => 8,
            LogQuantity::PTerm => 6,
            LogQuantity::ITerm => 4,
            LogQuantity::DTerm => 5,
            LogQuantity::FTerm => 7,
            LogQuantity::Setpoint => 0,
            LogQuantity::PidSum => 3,
            LogQuantity::PidError => 1,
            LogQuantity::Motors => 0,
            LogQuantity::Throttle => 8,
            LogQuantity::MotorRpm => 9,
            LogQuantity::Debug => 10,
        }
    }
}

/// Colour of a quantity's line; `k` is the motor or debug index.
fn colour(q: LogQuantity, k: usize) -> Color32 {
    match q {
        // the motors in the palette's first four distinct colours, as Blackbox Explorer
        LogQuantity::Motors | LogQuantity::MotorRpm | LogQuantity::Debug => {
            crate::colormap::line(q.palette_index() + k)
        }
        LogQuantity::Throttle => crate::theme::ink(),
        _ => crate::colormap::line(q.palette_index()),
    }
}

/// Display options of the tab.
#[derive(Debug, Clone, PartialEq)]
pub struct LogDisplay {
    /// The quantities shown, in checklist order.
    pub quantities: Vec<LogQuantity>,
    /// The axis rows span ± this rate (deg/s).
    pub y_scale: f64,
    /// The stick card on the throttle / motors row (docs/design/log-sticks).
    pub sticks: bool,
    /// The flight modes on that card.
    pub modes: bool,
    /// Marks where the flight modes change, on the throttle / motors row
    /// (off by default, owner 2026-11-10).
    pub mode_marks: bool,
}

impl Default for LogDisplay {
    fn default() -> Self {
        LogDisplay {
            // the reference tool's default selection
            quantities: vec![
                LogQuantity::Gyro,
                LogQuantity::Setpoint,
                LogQuantity::Motors,
                LogQuantity::Throttle,
            ],
            y_scale: 500.0,
            sticks: true,
            modes: true,
            mode_marks: false,
        }
    }
}

impl LogDisplay {
    pub fn shows(&self, q: LogQuantity) -> bool {
        self.quantities.contains(&q)
    }

    fn set(&mut self, q: LogQuantity, on: bool) {
        self.quantities.retain(|x| *x != q);
        if on {
            self.quantities.push(q);
            self.quantities
                .sort_by_key(|x| LogQuantity::ALL.iter().position(|a| a == x));
        }
    }
}

/// One line of a row.
struct Series {
    name: String,
    id: ChannelId,
    colour: Color32,
}

/// One row of the tab.
struct RowSpec {
    key: &'static str,
    label: String,
    series: Vec<Series>,
    /// Fixed y range, else automatic.
    y: Option<(f64, f64)>,
    weight: f32,
}

/// The rows the docked log can show with the current choice.
fn rows(app: &App, s: &FlightSession) -> Vec<RowSpec> {
    let d = &app.tools.log_display;
    let has = |id: &ChannelId| s.channel(id).is_some();
    let rate = display_unit(&openpid_core::units::Unit::DegreesPerSecond, &app.prefs);
    let mut out: Vec<RowSpec> = crate::axis_layout::shown_axes(app)
        .iter()
        .map(|axis| RowSpec {
            key: axis.name(),
            label: format!("{} ({})", axis.name(), rate.1),
            // the setpoint under the rest, as the reference tool draws it
            series: d
                .quantities
                .iter()
                .filter(|q| **q == LogQuantity::Setpoint)
                .chain(d.quantities.iter().filter(|q| **q != LogQuantity::Setpoint))
                .filter_map(|q| {
                    let id = q.axis_channel(*axis)?;
                    has(&id).then(|| Series {
                        name: q.short().to_string(),
                        id,
                        colour: colour(*q, 0),
                    })
                })
                .collect(),
            y: Some((-d.y_scale * rate.0, d.y_scale * rate.0)),
            weight: 1.0,
        })
        .collect();
    let indexed = |q: LogQuantity, prefix: &str| -> Vec<Series> {
        q.channels()
            .into_iter()
            .enumerate()
            .filter(|(_, id)| has(id))
            .map(|(k, id)| Series {
                name: format!("{prefix}{}", k + 1),
                id,
                colour: colour(q, k),
            })
            .collect()
    };
    let mut motors = Vec::new();
    if d.shows(LogQuantity::Throttle) && has(&ChannelId::Throttle) {
        motors.push(Series {
            name: LogQuantity::Throttle.short().to_string(),
            id: ChannelId::Throttle,
            colour: colour(LogQuantity::Throttle, 0),
        });
    }
    if d.shows(LogQuantity::Motors) {
        motors.extend(indexed(LogQuantity::Motors, "M"));
    }
    let extra = |key, label: String, series: Vec<Series>, y| RowSpec {
        key,
        label,
        series,
        y,
        weight: EXTRA_WEIGHT,
    };
    if !motors.is_empty() {
        out.push(extra("motors", "%".to_string(), motors, Some((0.0, 100.0))));
    }
    if d.shows(LogQuantity::MotorRpm) {
        let rpm = indexed(LogQuantity::MotorRpm, "RPM ");
        if !rpm.is_empty() {
            out.push(extra("rpm", "rpm".to_string(), rpm, None));
        }
    }
    if d.shows(LogQuantity::Debug) {
        let debug = indexed(LogQuantity::Debug, "debug ");
        if !debug.is_empty() {
            out.push(extra("debug", tr("debug").to_string(), debug, None));
        }
    }
    // other channels, without those a quantity already draws
    let drawn: Vec<ChannelId> = d.quantities.iter().flat_map(|q| q.channels()).collect();
    let other: Vec<Series> = app
        .tools
        .viewer_channels
        .iter()
        .filter(|id| has(id) && !drawn.contains(id))
        .enumerate()
        .map(|(k, id)| Series {
            name: id.key(),
            id: id.clone(),
            colour: crate::colormap::line(k),
        })
        .collect();
    if !other.is_empty() {
        out.push(extra("other", tr("other").to_string(), other, None));
    }
    out
}

/// What the pointer did on the rows this frame.
#[derive(Default)]
struct Interaction {
    cursor: Option<f64>,
    /// Time under the pointer, when it is over a row.
    hover: Option<f64>,
    hovered: Option<(Slot, bool)>,
    drag_to: Option<(Slot, bool, f64)>,
    drag_started: bool,
    drag_stopped: bool,
}

/// The tab.
pub fn log_viewer(app: &mut App, ui: &mut Ui) {
    control_panel(app, ui, "log-panel", controls);
    ui.separator();
    let Some(slot) = app.dock_slot() else {
        return;
    };
    let Some(s) = app.loaded(slot).and_then(|l| l.current().cloned()) else {
        return;
    };
    let specs = rows(app, &s);
    let gap = ui.spacing().item_spacing.y;
    let weights: f32 = specs.iter().map(|r| r.weight).sum();
    let unit_h = ((ui.available_height() - gap * specs.len() as f32 - 20.0) / weights).max(40.0);
    let mut act = Interaction {
        cursor: app.viewer.cursor_s,
        ..Interaction::default()
    };
    let t0 = s.timebase.time_us.first().copied().unwrap_or(0);
    let xs: Vec<f64> = s
        .timebase
        .time_us
        .iter()
        .map(|t| (t - t0) as f64 / 1.0e6)
        .collect();
    let last = specs.len() - 1;
    let mut first: Option<PlotTransform> = None;
    // the sticks sit on the throttle / motors row, else on the last row
    let sticks_row = specs.iter().position(|r| r.key == "motors").unwrap_or(last);
    let mut sticks_frame = None;
    let mut sticks_transform = None;
    // the plot frames of all the rows together: the mode-change lines span it
    let mut rows_rect: Option<egui::Rect> = None;
    for (i, spec) in specs.iter().enumerate() {
        let h = unit_h * spec.weight + if i == last { 20.0 } else { 0.0 };
        let t = row(app, ui, (slot, &s, &xs), spec, h, i == last, &mut act);
        first.get_or_insert(t);
        rows_rect = Some(rows_rect.map_or(*t.frame(), |r| r.union(*t.frame())));
        if i == sticks_row {
            sticks_frame = Some(*t.frame());
            sticks_transform = Some(t);
        }
    }
    app.viewer.mode_marks = 0;
    app.viewer.mode_lines = None;
    app.viewer.mode_hover = None;
    if app.tools.log_display.mode_marks
        && let Some(t) = sticks_transform
        && let Some(rows_rect) = rows_rect
    {
        let marks = draw_mode_marks(ui, &t, rows_rect, &s.mode_changes());
        app.viewer.mode_marks = marks.drawn;
        app.viewer.mode_lines = (marks.drawn > 0).then_some((rows_rect.top(), rows_rect.bottom()));
        app.viewer.mode_hover = marks.hovered;
    }
    // the sticks under the pointer while it is over the rows, else at the cursor
    let at = act.hover.or(act.cursor);
    let d = &app.tools.log_display;
    app.viewer.sticks = at.filter(|_| d.sticks).and_then(|t| s.sticks_at(t));
    app.viewer.modes = at
        .filter(|_| d.modes)
        .and_then(|t| s.flight_modes_at(t))
        .map(<[openpid_core::FlightMode]>::to_vec);
    app.viewer.sticks_rect = None;
    if let Some(frame) = sticks_frame
        && (app.viewer.sticks.is_some() || app.viewer.modes.is_some())
    {
        let modes = app.viewer.modes.as_ref().map(|m| mode_line(m));
        app.viewer.sticks_rect = Some(draw_sticks(
            ui,
            frame,
            app.viewer.sticks.as_ref(),
            modes.as_deref(),
        ));
    }
    let prev_hover = app.viewer.hover_handle;
    app.viewer.cursor_s = act.cursor;
    app.viewer.hover_handle = act.hovered;
    // the drag is recognised a few pixels after the press: use the handle hovered at the press
    if act.drag_started
        && let Some(h) = prev_hover.or(act.hovered)
    {
        app.viewer.drag_handle = Some(h);
    }
    if let Some((slot, start, t)) = act.drag_to {
        set_epoch_bound(app, slot, start, t);
    }
    if act.drag_stopped {
        app.viewer.drag_handle = None;
    }
}

/// One row: its lines, the epoch bounds and the cursor; clicks, long presses
/// and drags on it go into `act`.
fn row(
    app: &mut App,
    ui: &mut Ui,
    (slot, s, xs): (Slot, &FlightSession, &[f64]),
    spec: &RowSpec,
    height: f32,
    last: bool,
    act: &mut Interaction,
) -> PlotTransform {
    let touch = app.touch_mode;
    let handle_interaction = app.viewer.hover_handle.is_some() || app.viewer.drag_handle.is_some();
    let link = egui::Id::unique("log-x");
    let duration = s.timebase.duration_s();
    let mut plot = crate::grid::row(
        ("log-row", spec.key, spec.y.map(|y| y.1.to_bits())),
        link,
        Y_AXIS_W,
    )
    .height(height)
    .show_axes([last, true])
    .default_x_bounds(0.0, duration.max(0.1))
    .y_axis_label(spec.label.clone())
    .allow_boxed_zoom(false)
    .allow_drag([!handle_interaction, false])
    .allow_zoom([true, false])
    .allow_scroll([true, false]);
    if last {
        plot = plot.x_axis_label(tr("time (s)"));
    }
    if let Some((lo, hi)) = spec.y {
        plot = plot
            .default_y_bounds(lo, hi)
            .set_margin_fraction(egui::vec2(0.0, 0.0));
    }
    let mut transform = None;
    let shown = plot.show(ui, |pu| {
        let px = pu.response().rect.width().max(100.0) as usize;
        let b = pu.plot_bounds();
        let (lo, hi) = (b.min()[0], b.max()[0]);
        // only what is on screen, decimated to the width
        let from = xs.partition_point(|x| *x < lo).saturating_sub(1);
        let to = (xs.partition_point(|x| *x <= hi) + 1).min(xs.len());
        for line in &spec.series {
            let Some(c) = s.channel(&line.id) else {
                continue;
            };
            let (scale, symbol) = display_unit(&c.unit, &app.prefs);
            let v = c.data.values();
            let to = to.min(v.len());
            let from = from.min(to);
            let mut pts = decimate(
                &xs[from..to],
                &v[from..to],
                |i| c.data.is_valid(i + from),
                px * 2,
            );
            if scale != 1.0 {
                for p in &mut pts {
                    p[1] *= scale;
                }
            }
            pu.line(
                Line::new(format!("{} ({symbol})", line.name), PlotPoints::from(pts))
                    .color(line.colour)
                    .width(1.2),
            );
        }
        let hovered = draw_epochs(pu, app, &[slot], grab_radius(touch));
        if hovered.is_some() {
            act.hovered = hovered;
        }
        if let Some(t) = act.cursor {
            pu.vline(
                VLine::new("cursor", t)
                    .color(crate::colormap::ACCENT)
                    .width(1.5),
            );
        }
        pointer(app, pu, touch, act);
        transform = Some(*pu.transform());
    });
    shade_epochs(ui, app, &[slot], &shown.transform);
    read_out(ui, app, s, spec, act.cursor, &shown.transform, xs);
    app.probes
        .record(format!("log/{}", spec.key), &shown.transform);
    transform.unwrap_or(shown.transform)
}

/// Clicks, long presses and epoch drags on a row.
fn pointer(app: &mut App, pu: &egui_plot::PlotUi<'_>, touch: bool, act: &mut Interaction) {
    let resp = pu.response();
    if resp.hovered()
        && let Some(p) = pu.pointer_coordinate()
    {
        act.hover = Some(p.x);
    }
    act.drag_started |= resp.drag_started();
    act.drag_stopped |= resp.drag_stopped();
    // hover_pos() is unreliable mid-drag; interact_pointer_pos() follows the drag
    if let Some(handle) = app.viewer.drag_handle
        && resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let t = pu.transform().value_from_position(p).x;
        act.drag_to = Some((handle.0, handle.1, t));
    }
    if app.viewer.drag_handle.is_some() {
        return;
    }
    // a click, or a long press on a touch screen, places the cursor
    if resp.clicked()
        && let Some(p) = pu.pointer_coordinate()
    {
        act.cursor = Some(p.x);
    }
    if resp.long_touched()
        && let Some(p) = resp.interact_pointer_pos()
    {
        act.cursor = Some(pu.transform().value_from_position(p).x);
    }
    // in touch mode a finger held still for a moment places the cursor too
    // (not every backend reports long touches)
    if !touch {
        return;
    }
    let now = pu.ctx().input(|i| i.time);
    let down = resp.is_pointer_button_down_on();
    match (app.viewer.press, resp.interact_pointer_pos()) {
        (None, Some(p)) if down => app.viewer.press = Some((now, p)),
        (Some((t0, p0)), Some(p)) if down => {
            if p.distance(p0) > LONG_PRESS_SLOP_PX {
                app.viewer.press = None; // it is a pan
            } else if now - t0 >= LONG_PRESS_S {
                act.cursor = Some(pu.transform().value_from_position(p0).x);
                app.viewer.press = None;
            }
        }
        // released: only the row the press started on forgets it
        (Some((_, p0)), _) if !down && resp.rect.contains(p0) => app.viewer.press = None,
        _ => {}
    }
}

/// The modes on one line: "ARM · ANGLE", or "no mode".
fn mode_line(modes: &[openpid_core::FlightMode]) -> String {
    if modes.is_empty() {
        tr("no mode").to_string()
    } else {
        modes
            .iter()
            .map(openpid_core::FlightMode::name)
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// What [`draw_mode_marks`] drew.
struct ModeMarks {
    /// How many changes are in view.
    drawn: usize,
    /// The label of the change under the pointer: its time and its modes.
    hovered: Option<String>,
}

/// How near the pointer must be to a mode-change line to get its label.
const MODE_HOVER_PX: f32 = 5.0;

/// Marks where the flight modes change, as the reference tool does: a
/// dashed line through every row (`rows`), and the modes from then on at
/// the foot of the row of `transform`, the throttle / motors row. The
/// pointer on a line gets the time and the whole set of modes beside it,
/// which the foot label cuts where two changes are close.
fn draw_mode_marks(
    ui: &Ui,
    transform: &PlotTransform,
    rows: egui::Rect,
    changes: &[(f64, &[openpid_core::FlightMode])],
) -> ModeMarks {
    let frame = *transform.frame();
    let painter = ui.painter().with_clip_rect(rows);
    let text = ui.visuals().text_color();
    let line = egui::Stroke::new(1.0, text.gamma_multiply(0.6));
    let font = egui::FontId::proportional(10.5);
    let hover_pos = ui.ctx().pointer_hover_pos().filter(|p| rows.contains(*p));
    let mut marks = ModeMarks {
        drawn: 0,
        hovered: None,
    };
    let mut nearest: Option<(f32, egui::Pos2, String)> = None;
    for (i, (t, modes)) in changes.iter().enumerate() {
        let x = transform.position_from_point_x(*t);
        if x < frame.left() || x > frame.right() {
            continue;
        }
        painter.add(egui::Shape::dashed_line(
            &[egui::pos2(x, rows.top()), egui::pos2(x, rows.bottom())],
            line,
            4.0,
            3.0,
        ));
        // the foot label ends where the next change begins
        let next = changes
            .get(i + 1)
            .map_or(frame.right(), |(t, _)| transform.position_from_point_x(*t));
        let room = egui::Rect::from_min_max(
            egui::pos2(x, frame.top()),
            egui::pos2(next.max(x).min(frame.right()), frame.bottom()),
        );
        painter.with_clip_rect(room).text(
            egui::pos2(x + 3.0, frame.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            mode_line(modes),
            font.clone(),
            text,
        );
        marks.drawn += 1;
        if let Some(p) = hover_pos {
            let d = (p.x - x).abs();
            if d <= MODE_HOVER_PX && nearest.as_ref().is_none_or(|(best, ..)| d < *best) {
                nearest = Some((d, p, format!("{t:.1} s · {}", mode_line(modes))));
            }
        }
    }
    if let Some((_, p, label)) = nearest {
        let galley = painter.layout_no_wrap(label.clone(), egui::FontId::proportional(12.0), text);
        let size = galley.size() + egui::vec2(12.0, 8.0);
        // beside the pointer, on the side that has room
        let left = if p.x + 12.0 + size.x <= rows.right() {
            p.x + 12.0
        } else {
            p.x - 12.0 - size.x
        };
        let top = (p.y - size.y - 6.0).max(rows.top());
        let rect = egui::Rect::from_min_size(egui::pos2(left, top), size);
        let overlay = ui.painter().with_clip_rect(ui.clip_rect());
        overlay.rect_filled(
            rect,
            5.0,
            ui.visuals().extreme_bg_color.gamma_multiply(0.92),
        );
        overlay.rect_stroke(rect, 5.0, line, egui::StrokeKind::Inside);
        overlay.galley(rect.min + egui::vec2(6.0, 4.0), galley, text);
        marks.hovered = Some(label);
    }
    marks
}

/// Side of a stick box, points.
const STICK_BOX: f32 = 54.0;

/// The card at the bottom right of `frame`, as the reference tool shows it
/// beside its plots: the flight modes on a line, and under it the two
/// sticks (mode 2: throttle and yaw on the left, pitch and roll on the
/// right). A card taller than the row reaches up over the row above it.
/// Returns the card's rectangle.
fn draw_sticks(
    ui: &Ui,
    frame: egui::Rect,
    st: Option<&openpid_core::Sticks>,
    modes: Option<&str>,
) -> egui::Rect {
    let painter = ui.painter().with_clip_rect(ui.clip_rect());
    let dark = crate::theme::is_dark();
    let (bg, line, text) = if dark {
        (
            Color32::from_rgba_unmultiplied(20, 22, 27, 215),
            Color32::from_gray(90),
            Color32::from_gray(200),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(250, 250, 252, 225),
            Color32::from_gray(150),
            Color32::from_gray(50),
        )
    };
    let font = egui::FontId::proportional(11.0);
    let gap = 8.0;
    let label_h = 14.0;
    let modes_galley = modes.map(|m| painter.layout_no_wrap(m.to_string(), font.clone(), text));
    let modes_w = modes_galley.as_ref().map_or(0.0, |g| g.size().x);
    let modes_h = modes_galley.as_ref().map_or(0.0, |g| g.size().y + 4.0);
    let sticks_w = if st.is_some() {
        2.0 * STICK_BOX + gap
    } else {
        0.0
    };
    let sticks_h = if st.is_some() {
        STICK_BOX + label_h
    } else {
        0.0
    };
    let card_size = egui::vec2(sticks_w.max(modes_w) + 12.0, modes_h + sticks_h + 10.0);
    let card = egui::Rect::from_min_size(
        egui::pos2(
            frame.right() - card_size.x - 6.0,
            frame.bottom() - card_size.y - 4.0,
        ),
        card_size,
    );
    painter.rect_filled(card, 6.0, bg);
    let inner = card.shrink(5.0);
    // the modes centred over the sticks
    if let Some(g) = modes_galley {
        let x = inner.center().x - g.size().x / 2.0;
        painter.galley(egui::pos2(x, inner.top()), g, text);
    }
    let Some(st) = st else { return card };
    // throttle 0..1 runs bottom to top; the others are −1..1 around the centre
    let throttle = st.throttle.map_or(0.0, |t| t * 2.0 - 1.0);
    let boxes = [(st.yaw, throttle, "T / Y"), (st.roll, st.pitch, "P / R")];
    let left0 = inner.center().x - sticks_w / 2.0;
    for (k, (x, y, name)) in boxes.into_iter().enumerate() {
        let left = left0 + k as f32 * (STICK_BOX + gap);
        let r = egui::Rect::from_min_size(
            egui::pos2(left, inner.top() + modes_h),
            egui::vec2(STICK_BOX, STICK_BOX),
        );
        painter.rect_stroke(
            r,
            2.0,
            egui::Stroke::new(1.0, line),
            egui::StrokeKind::Inside,
        );
        let c = r.center();
        let faint = egui::Stroke::new(1.0, line.gamma_multiply(0.5));
        painter.line_segment(
            [egui::pos2(c.x, r.top()), egui::pos2(c.x, r.bottom())],
            faint,
        );
        painter.line_segment(
            [egui::pos2(r.left(), c.y), egui::pos2(r.right(), c.y)],
            faint,
        );
        let half = STICK_BOX / 2.0 - 4.0;
        let dot = egui::pos2(c.x + x as f32 * half, c.y - y as f32 * half);
        painter.circle_filled(dot, 4.5, crate::colormap::ACCENT);
        painter.text(
            egui::pos2(c.x, r.bottom() + 2.0),
            egui::Align2::CENTER_TOP,
            name,
            font.clone(),
            text,
        );
    }
    card
}

/// The names of the row's lines in their colours, with the value at the
/// cursor once there is one: the legend and the read-out in one.
fn read_out(
    ui: &Ui,
    app: &App,
    s: &FlightSession,
    spec: &RowSpec,
    cursor: Option<f64>,
    transform: &PlotTransform,
    xs: &[f64],
) {
    let frame = *transform.frame();
    let painter = ui.painter().with_clip_rect(frame);
    let at = cursor.map(|t| {
        xs.partition_point(|x| *x < t)
            .min(xs.len().saturating_sub(1))
    });
    let font = egui::FontId::proportional(11.5);
    let bg = if crate::theme::is_dark() {
        Color32::from_rgba_unmultiplied(16, 17, 20, 210)
    } else {
        Color32::from_rgba_unmultiplied(250, 250, 252, 220)
    };
    let mut x = frame.left() + 6.0;
    for line in &spec.series {
        let text = match (at, s.channel(&line.id)) {
            (Some(i), Some(c)) if c.data.is_valid(i) => {
                let (scale, _) = display_unit(&c.unit, &app.prefs);
                format!("{} {:.0}", line.name, c.data.values()[i] * scale)
            }
            _ => line.name.clone(),
        };
        let galley = painter.layout_no_wrap(text, font.clone(), line.colour);
        let r = egui::Rect::from_min_size(egui::pos2(x, frame.top() + 3.0), galley.size())
            .expand2(egui::vec2(3.0, 1.0));
        painter.rect_filled(r, 3.0, bg);
        painter.galley(egui::pos2(x, frame.top() + 3.0), galley, line.colour);
        x = r.right() + 8.0;
    }
}

/// The panel: quantities, other channels, scale, cursor.
fn controls(app: &mut App, ui: &mut Ui) {
    let session = app
        .dock_slot()
        .and_then(|s| app.loaded(s))
        .and_then(|l| l.current().cloned());
    crate::axis_layout::axes_row(app, ui);
    let rate = display_unit(&openpid_core::units::Unit::DegreesPerSecond, &app.prefs);
    let mut y = app.tools.log_display.y_scale * rate.0;
    let r = panel::drag(
        ui,
        tr("Y scale ±"),
        tr(
            "the roll, pitch and yaw rows span ± this rate; the same on every row, so the axes compare",
        ),
        &mut y,
        5.0 * rate.0,
        (10.0 * rate.0)..=(5000.0 * rate.0),
        &rate.1,
    );
    if r.changed() {
        app.tools.log_display.y_scale = y / rate.0;
    }
    panel::section(ui, tr("Quantities"));
    for q in LogQuantity::ALL {
        let logged = session
            .as_ref()
            .is_none_or(|s| q.channels().iter().any(|id| s.channel(id).is_some()));
        let mut on = app.tools.log_display.shows(q);
        let hint = if logged {
            q.hint()
        } else {
            tr("not in this log")
        };
        let text = RichText::new(q.name()).color(if logged { colour(q, 0) } else { MUTED });
        let r = ui
            .add_enabled(logged, egui::Checkbox::new(&mut on, text))
            .on_hover_text(hint)
            .on_disabled_hover_text(hint);
        if r.changed() {
            app.tools.log_display.set(q, on);
        }
    }
    other_channels(app, ui, session.as_deref());
    cursor_rows(app, ui);
    panel::check(
        ui,
        &mut app.tools.log_display.sticks,
        tr("Sticks"),
        tr(
            "the sticks at the pointer or the cursor, at the bottom right of the throttle / motors row",
        ),
    );
    panel::check(
        ui,
        &mut app.tools.log_display.modes,
        tr("Flight modes"),
        tr("the flight modes active at the pointer or the cursor, on the same card"),
    );
    panel::check(
        ui,
        &mut app.tools.log_display.mode_marks,
        tr("Mode changes"),
        tr(
            "a dashed line where the flight modes change, with the modes from then on, on the throttle / motors row",
        ),
    );
}

/// "Other channels": any channel of the log, in a row of their own.
fn other_channels(app: &mut App, ui: &mut Ui, session: Option<&FlightSession>) {
    let n = app.tools.viewer_channels.len();
    let title = if n == 0 {
        tr("Other channels").to_string()
    } else {
        format!("{} ({n})", tr("Other channels"))
    };
    egui::CollapsingHeader::new(title)
        .id_salt("log-other")
        .show(ui, |ui| {
            let l = ui.label(tr("filter"));
            ui.add(
                egui::TextEdit::singleline(&mut app.viewer.channel_filter)
                    .hint_text(tr("filter…"))
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(l.id)
            .on_hover_text(tr("show only the channels whose name contains this"));
            let Some(s) = session else { return };
            let mut keys: Vec<ChannelId> = s.channels.iter().map(|c| c.id.clone()).collect();
            keys.sort_by_key(ChannelId::key);
            let filter = app.viewer.channel_filter.clone();
            egui::ScrollArea::vertical()
                .id_salt("log-other-list")
                .max_height(220.0)
                .show(ui, |ui| {
                    for id in keys {
                        let k = id.key();
                        if !filter.is_empty() && !k.contains(filter.as_str()) {
                            continue;
                        }
                        let mut on = app.tools.viewer_channels.contains(&id);
                        if ui.checkbox(&mut on, k).changed() {
                            if on {
                                app.tools.viewer_channels.push(id);
                            } else {
                                app.tools.viewer_channels.retain(|c| *c != id);
                            }
                        }
                    }
                });
        });
}

/// The rows of the docked log and the names of their lines, for the tests.
#[cfg(test)]
pub(crate) fn row_summary(app: &App) -> Vec<(&'static str, Vec<String>)> {
    let Some(s) = app
        .dock_slot()
        .and_then(|s| app.loaded(s))
        .and_then(|l| l.current().cloned())
    else {
        return Vec::new();
    };
    rows(app, &s)
        .into_iter()
        .map(|r| (r.key, r.series.into_iter().map(|l| l.name).collect()))
        .collect()
}

/// The cursor's time and the epoch bounds set from it.
fn cursor_rows(app: &mut App, ui: &mut Ui) {
    panel::section(ui, tr("Cursor"));
    let Some(mut t) = app.viewer.cursor_s else {
        ui.label(
            RichText::new(tr(
                "Click or tap the plot to place the cursor, then set the epoch from it.",
            ))
            .color(MUTED),
        );
        return;
    };
    panel::drag_value(
        ui,
        tr("Position"),
        tr("the time of the cursor; the values at it are on every row"),
        egui::DragValue::new(&mut t)
            .speed(0.001)
            .range(0.0..=f64::MAX)
            .fixed_decimals(3),
        "s",
    );
    app.viewer.cursor_s = Some(t);
    let Some(slot) = app.dock_slot() else { return };
    ui.horizontal(|ui| {
        if ui
            .button(tr("start = cursor"))
            .on_hover_text(tr("the analysis window of the docked log starts here"))
            .clicked()
        {
            set_epoch_from_cursor(app, slot, t, true);
        }
        if ui
            .button(tr("end = cursor"))
            .on_hover_text(tr("the analysis window of the docked log ends here"))
            .clicked()
        {
            set_epoch_from_cursor(app, slot, t, false);
        }
    });
}
