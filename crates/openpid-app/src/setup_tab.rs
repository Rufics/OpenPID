//! Setup info (docs/design/setup-tab, U3): one table for every visible log,
//! a column per log and a row per fact, in four sections: the log check,
//! what the decoder read, the setup in plain words and every header as
//! logged. A row whose logs do not say the same is tinted, as the reference
//! tool tints the differing rows of its Setup Info; "Differences only"
//! leaves those, the search leaves the rows that mention a text.
//!
//! The rows come from `openpid_core::describe` and the log check; the tab
//! only lays them out.

use crate::app::{App, Loaded};
use crate::axis_layout::control_panel;
use crate::colormap::MUTED;
use crate::i18n::{self, Lang, tr};
use crate::panel;
use crate::views::{level_color, overall_text, paint_mark, slot_color};
use crate::worker::Slot;
use eframe::egui::{self, Color32, RichText, Ui};
use openpid_analysis::{Fitness, FitnessLevel};
use openpid_core::FlightSession;
use openpid_core::describe::{self, Compared};
use std::borrow::Cow;
use std::sync::Arc;

/// Height of a table row.
const ROW_H: f32 = 22.0;
/// Width of the column with the row names.
const NAME_W: f32 = 210.0;
/// The narrowest column of a log; more logs than fit scroll sideways.
const MIN_LOG_W: f32 = 230.0;

/// The sections of the table, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Whether the log is fit for analysis (docs/design/log-fitness).
    LogCheck,
    /// What the decoder read.
    Decoder,
    /// The setup in plain words, from the typed settings.
    Setup,
    /// Every header as logged.
    Headers,
}

impl Section {
    /// Every section, in table order.
    pub const ALL: [Section; 4] = [
        Section::LogCheck,
        Section::Decoder,
        Section::Setup,
        Section::Headers,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::LogCheck => tr("Log check"),
            Section::Decoder => tr("Decoder"),
            Section::Setup => tr("Setup"),
            Section::Headers => tr("All headers"),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Section::LogCheck => tr(
                "whether the log is fit for analysis: its rate, length, what was logged, and the fix where something is missing",
            ),
            Section::Decoder => {
                tr("what was read from the file: samples, frames, and what could not be read")
            }
            Section::Setup => tr(
                "how the craft was set up, in plain words: firmware, rates, PID gains, filters, motors",
            ),
            Section::Headers => tr("every header of the log, as the firmware wrote it"),
        }
    }
}

/// Display options of the tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDisplay {
    /// Only the rows whose name or value contains this (any case).
    pub search: String,
    /// Only the rows the logs differ in.
    pub differences_only: bool,
    /// The sections shown, in the order of [`Section::ALL`].
    pub sections: [bool; 4],
}

impl Default for SetupDisplay {
    fn default() -> Self {
        SetupDisplay {
            search: String::new(),
            differences_only: false,
            sections: [true; 4],
        }
    }
}

/// What a cell of the log check shows besides its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// The level of a check.
    Level(FitnessLevel),
    /// A setting to copy that fixes the check.
    Fix,
}

/// One line of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A section title with the number of its rows shown.
    Title(Section, usize),
    /// A row: its name, a value per log, and whether the logs differ.
    Row {
        /// The section the row belongs to.
        section: Section,
        /// The row's name.
        name: String,
        /// The value of each log (`None`: the log has no such row).
        values: Vec<Option<String>>,
        /// What the cells of a log-check row carry.
        marks: Vec<Option<Mark>>,
        /// The logs do not say the same.
        differs: bool,
    },
}

/// What the table was built from: when it changes the table is built again.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    logs: Vec<(Slot, usize)>,
    lang: Lang,
    display: SetupDisplay,
}

/// State of the tab: the table as drawn.
#[derive(Debug, Default)]
pub struct SetupState {
    key: Option<Key>,
    /// The sessions the table was built from, by address.
    sessions: Vec<usize>,
    /// The logs of the columns.
    pub logs: Vec<Slot>,
    /// The lines shown, after the search and "Differences only".
    pub lines: Vec<Line>,
    /// Rows the logs differ in, of all rows, before the search.
    pub differing: (usize, usize),
}

/// A log of the table.
struct Column<'a> {
    session: &'a FlightSession,
    fitness: Option<&'a Fitness>,
}

/// The rows of the log check: the overall verdict, then each check and,
/// under it, the fix where a log has one.
fn check_rows(logs: &[Column]) -> Vec<Line> {
    let n = logs.len();
    if logs.iter().all(|c| c.fitness.is_none()) {
        return Vec::new();
    }
    let differs = |values: &[Option<String>]| values.windows(2).any(|w| w[0] != w[1]);
    let overall: Vec<Option<FitnessLevel>> =
        logs.iter().map(|c| c.fitness.map(|f| f.overall)).collect();
    let values: Vec<Option<String>> = overall
        .iter()
        .map(|l| l.map(|l| overall_text(l).to_string()))
        .collect();
    let mut out = vec![Line::Row {
        section: Section::LogCheck,
        name: tr("verdict").to_string(),
        differs: differs(&values),
        values,
        marks: overall.iter().map(|l| l.map(Mark::Level)).collect(),
    }];
    // the checks in the order of the first log that has each
    let mut titles: Vec<&str> = Vec::new();
    for f in logs.iter().filter_map(|c| c.fitness) {
        for c in &f.checks {
            if !titles.contains(&c.title.as_str()) {
                titles.push(&c.title);
            }
        }
    }
    for title in titles {
        let checks: Vec<_> = logs
            .iter()
            .map(|c| {
                c.fitness
                    .and_then(|f| f.checks.iter().find(|x| x.title == title))
            })
            .collect();
        let values: Vec<Option<String>> =
            checks.iter().map(|c| c.map(|c| c.detail.clone())).collect();
        out.push(Line::Row {
            section: Section::LogCheck,
            name: title.to_string(),
            differs: differs(&values),
            values,
            marks: checks
                .iter()
                .map(|c| c.map(|c| Mark::Level(c.level)))
                .collect(),
        });
        let fixes: Vec<Option<String>> = checks
            .iter()
            .map(|c| c.and_then(|c| c.fix.clone()))
            .collect();
        if fixes.iter().any(Option::is_some) {
            out.push(Line::Row {
                section: Section::LogCheck,
                name: format!("{title}: {}", tr("fix")),
                differs: differs(&fixes),
                marks: fixes
                    .iter()
                    .map(|f| f.as_ref().map(|_| Mark::Fix))
                    .collect(),
                values: fixes,
            });
        }
    }
    debug_assert!(out.iter().all(|l| match l {
        Line::Row { values, .. } => values.len() == n,
        Line::Title(..) => true,
    }));
    out
}

fn plain<L: ToString>(
    section: Section,
    rows: Vec<Compared<L>>,
    label: impl Fn(&L) -> String,
) -> Vec<Line> {
    rows.into_iter()
        .map(|r| Line::Row {
            section,
            name: label(&r.label),
            marks: vec![None; r.values.len()],
            values: r.values,
            differs: r.differs,
        })
        .collect()
}

/// Every row of every section for `logs`.
fn all_rows(logs: &[Column]) -> Vec<Line> {
    let mut out = check_rows(logs);
    let facts =
        |of: fn(&FlightSession) -> Vec<describe::Fact>| -> Vec<Vec<(Cow<'static, str>, String)>> {
            logs.iter().map(|c| describe::rows(of(c.session))).collect()
        };
    out.extend(plain(
        Section::Decoder,
        describe::compare(&facts(describe::decoder)),
        |l| i18n::tr_text(l),
    ));
    out.extend(plain(
        Section::Setup,
        describe::compare(&facts(describe::setup)),
        |l| i18n::tr_text(l),
    ));
    let headers: Vec<Vec<(String, String)>> =
        logs.iter().map(|c| c.session.raw_headers.clone()).collect();
    out.extend(plain(
        Section::Headers,
        describe::compare(&headers),
        Clone::clone,
    ));
    out
}

/// Whether a row passes the display options.
fn shown(line: &Line, display: &SetupDisplay, needle: &str) -> bool {
    let Line::Row {
        section,
        name,
        values,
        differs,
        ..
    } = line
    else {
        return false;
    };
    let at = Section::ALL.iter().position(|s| s == section).unwrap_or(0);
    if !display.sections[at] || (display.differences_only && !differs) {
        return false;
    }
    needle.is_empty()
        || name.to_lowercase().contains(needle)
        || values
            .iter()
            .flatten()
            .any(|v| v.to_lowercase().contains(needle))
}

/// The table for `logs` under `display`: section titles over their rows;
/// a section without rows is left out. Also returns (differing, all) rows.
fn table(logs: &[Column], display: &SetupDisplay) -> (Vec<Line>, (usize, usize)) {
    let rows = all_rows(logs);
    let differing = rows
        .iter()
        .filter(|l| matches!(l, Line::Row { differs: true, .. }))
        .count();
    let total = rows.len();
    let needle = display.search.trim().to_lowercase();
    let mut out = Vec::new();
    for section in Section::ALL {
        let of: Vec<Line> = rows
            .iter()
            .filter(|l| matches!(l, Line::Row { section: s, .. } if *s == section))
            .filter(|l| shown(l, display, &needle))
            .cloned()
            .collect();
        if !of.is_empty() {
            out.push(Line::Title(section, of.len()));
            out.extend(of);
        }
    }
    (out, (differing, total))
}

/// The visible logs with a session, in slot order.
fn columns(app: &App) -> Vec<(Slot, usize, Arc<FlightSession>, Option<Fitness>)> {
    app.visible_slots()
        .into_iter()
        .filter_map(|slot| {
            let l = app.loaded(slot)?;
            let s = l.current()?.clone();
            Some((slot, l.session, s, l.fitness().cloned()))
        })
        .collect()
}

/// Builds the table again when the logs, the language or the options changed.
fn refresh(app: &mut App) {
    let cols = columns(app);
    let key = Key {
        logs: cols
            .iter()
            .map(|(slot, session, ..)| (*slot, *session))
            .collect(),
        lang: i18n::current(),
        display: app.tools.setup_display.clone(),
    };
    // a file opened again in the same slot is another log under the same key
    let same_files = app.setup_state.sessions
        == cols
            .iter()
            .map(|c| Arc::as_ptr(&c.2) as usize)
            .collect::<Vec<_>>();
    if app.setup_state.key.as_ref() == Some(&key) && same_files {
        return;
    }
    let logs: Vec<Column> = cols
        .iter()
        .map(|(_, _, s, f)| Column {
            session: s,
            fitness: f.as_ref(),
        })
        .collect();
    let (lines, differing) = table(&logs, &key.display);
    app.setup_state = SetupState {
        key: Some(key),
        sessions: cols.iter().map(|c| Arc::as_ptr(&c.2) as usize).collect(),
        logs: cols.iter().map(|c| c.0).collect(),
        lines,
        differing,
    };
}

/// The tint of a row the logs differ in.
fn diff_tint() -> Color32 {
    if crate::theme::is_dark() {
        Color32::from_rgb(70, 42, 44)
    } else {
        Color32::from_rgb(252, 222, 224)
    }
}

/// A child `Ui` over `rect`, its content centred vertically.
fn cell(ui: &mut Ui, rect: egui::Rect) -> Ui {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}

/// The tab.
pub fn setup(app: &mut App, ui: &mut Ui) {
    refresh(app);
    control_panel(app, ui, "setup-panel", controls);
    // the options may have changed in the panel
    refresh(app);
    ui.separator();
    let logs = app.setup_state.logs.clone();
    if logs.is_empty() {
        return;
    }
    let lines = std::mem::take(&mut app.setup_state.lines);
    // with only the differences shown every row would be tinted
    let tint = !app.tools.setup_display.differences_only;
    let n = logs.len() as f32;
    let log_w = ((ui.available_width() - NAME_W) / n).max(MIN_LOG_W).floor();
    let width = NAME_W + log_w * n;
    egui::ScrollArea::horizontal()
        .id_salt("setup-sideways")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            head(app, ui, &logs, log_w);
            if lines.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new(nothing_shown(app)).color(MUTED));
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("setup-rows")
                .auto_shrink([false, false])
                .show_rows(ui, ROW_H, lines.len(), |ui, range| {
                    ui.set_width(width);
                    for line in &lines[range] {
                        draw_line(ui, &logs, line, log_w, tint);
                    }
                });
        });
    app.setup_state.lines = lines;
}

/// Why the table is empty.
fn nothing_shown(app: &App) -> &'static str {
    let d = &app.tools.setup_display;
    if !d.search.trim().is_empty() {
        tr("no row mentions this text")
    } else if d.differences_only {
        tr("the logs do not differ in the sections shown")
    } else {
        tr("every section is hidden")
    }
}

/// The column titles: the slot's dot, its letter and the file.
fn head(app: &App, ui: &mut Ui, logs: &[Slot], log_w: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(NAME_W + log_w * logs.len() as f32, ROW_H + 4.0),
        egui::Sense::hover(),
    );
    for (i, slot) in logs.iter().enumerate() {
        let x = rect.left() + NAME_W + log_w * i as f32;
        let r =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(log_w, rect.height()));
        ui.painter()
            .circle_filled(egui::pos2(x + 12.0, r.center().y), 4.0, slot_color(*slot));
        let mut c = cell(ui, r.with_min_x(x + 14.0));
        let file = app
            .loaded(*slot)
            .and_then(|l: &Loaded| l.path.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        c.add(
            egui::Label::new(RichText::new(format!("{} · {file}", slot.name())).strong())
                .truncate(),
        );
    }
}

fn draw_line(ui: &mut Ui, logs: &[Slot], line: &Line, log_w: f32, tint: bool) {
    let width = NAME_W + log_w * logs.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    match line {
        Line::Title(section, count) => {
            let mut c = cell(ui, rect);
            c.with_layout(egui::Layout::left_to_right(egui::Align::BOTTOM), |ui| {
                ui.label(
                    RichText::new(format!("{} · {count}", section.title().to_uppercase()))
                        .size(11.0)
                        .strong()
                        .color(MUTED),
                );
            });
        }
        Line::Row {
            section,
            name,
            values,
            marks,
            differs,
        } => {
            if *differs && tint {
                ui.painter()
                    .rect_filled(rect.shrink2(egui::vec2(0.0, 0.5)), 2.0, diff_tint());
            }
            let name_rect = egui::Rect::from_min_size(rect.min, egui::vec2(NAME_W, ROW_H));
            let mut c = cell(ui, name_rect);
            let label = c.add(egui::Label::new(name).truncate());
            if *differs {
                label.on_hover_text(tr("the logs differ in this row"));
            }
            for (i, slot) in logs.iter().enumerate() {
                let r = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + NAME_W + log_w * i as f32, rect.top()),
                    egui::vec2(log_w, ROW_H),
                );
                let mut c = cell(ui, r);
                value_cell(
                    &mut c,
                    *slot,
                    name,
                    values[i].as_deref(),
                    marks[i].as_ref(),
                    *section,
                );
            }
        }
    }
}

/// One value: a check's mark before its text, a fix in monospace with its
/// Copy button, anything else as text; the whole text on hover when it is cut.
fn value_cell(
    ui: &mut Ui,
    slot: Slot,
    name: &str,
    value: Option<&str>,
    mark: Option<&Mark>,
    section: Section,
) {
    let Some(value) = value else {
        ui.label(RichText::new("—").color(MUTED))
            .on_hover_text(tr("this log has no such row"));
        return;
    };
    match mark {
        Some(Mark::Level(level)) => {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(12.0, ROW_H), egui::Sense::hover());
            let colour = level_color(*level);
            paint_mark(ui.painter(), r.center(), *level, colour);
            let label = format!("{} {name} {}", slot.name(), level.marker());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
            ui.add(egui::Label::new(value).truncate());
        }
        Some(Mark::Fix) => {
            let copy = ui
                .small_button(tr("Copy"))
                .on_hover_text(format!("{}: {value}", tr("copy")));
            let label = format!("{} {} {name}", tr("Copy"), slot.name());
            copy.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            if copy.clicked() {
                ui.ctx().copy_text(value.to_string());
            }
            ui.add(egui::Label::new(RichText::new(value).monospace()).truncate());
        }
        None => {
            let text = RichText::new(value);
            let text = if section == Section::Headers {
                text.monospace()
            } else {
                text
            };
            ui.add(egui::Label::new(text).truncate());
        }
    }
}

/// The shown rows as tab-separated text: the row names, then a column per log.
pub(crate) fn as_text(app: &App) -> String {
    let st = &app.setup_state;
    let files = st.logs.iter().map(|slot| {
        app.loaded(*slot)
            .and_then(|l| l.path.file_name())
            .map_or_else(
                || slot.name().to_string(),
                |s| s.to_string_lossy().into_owned(),
            )
    });
    let head: Vec<String> = std::iter::once(String::new()).chain(files).collect();
    let mut out = vec![head.join("\t")];
    for line in &st.lines {
        out.push(match line {
            Line::Title(section, _) => format!("# {}", section.title()),
            Line::Row { name, values, .. } => std::iter::once(name.as_str())
                .chain(values.iter().map(|v| v.as_deref().unwrap_or("")))
                .collect::<Vec<_>>()
                .join("\t"),
        });
    }
    out.join("\n") + "\n"
}

/// The panel: the search, "Differences only", the sections, the logs.
fn controls(app: &mut App, ui: &mut Ui) {
    let several = app.setup_state.logs.len() > 1;
    let d = &mut app.tools.setup_display;
    panel::section(ui, tr("Show"));
    let hint = tr("only the rows whose name or value contains this text, any case");
    panel::row(ui, tr("Search"), hint, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut d.search)
                .hint_text(tr("name or value"))
                .desired_width(panel::VALUE_W + panel::UNIT_W),
        )
    });
    if !several {
        d.differences_only = false;
    }
    panel::check_enabled(
        ui,
        several,
        &mut d.differences_only,
        tr("Differences only"),
        if several {
            tr("only the rows the logs do not agree in: the tinted ones")
        } else {
            tr("open a second log to compare the setups")
        },
    );
    panel::section(ui, tr("Sections"));
    for (i, section) in Section::ALL.into_iter().enumerate() {
        panel::check(ui, &mut d.sections[i], section.title(), section.hint());
    }
    panel::section(ui, tr("Logs"));
    let st = &app.setup_state;
    if st.logs.is_empty() {
        ui.label(RichText::new(tr("no log shown")).color(MUTED));
        return;
    }
    for slot in &st.logs {
        let Some(s) = app.loaded(*slot).and_then(Loaded::current) else {
            continue;
        };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(r.center(), 4.0, slot_color(*slot));
            ui.label(format!(
                "{} · {} {}",
                slot.name(),
                s.raw_headers.len(),
                tr("headers")
            ))
            .on_hover_text(&s.firmware.revision);
        });
    }
    if several {
        let (differing, total) = st.differing;
        ui.label(
            RichText::new(format!("{differing} / {total} {}", tr("rows differ"))).color(MUTED),
        );
    }
    ui.add_space(6.0);
    if ui
        .button(tr("Copy the table"))
        .on_hover_text(tr(
            "copy the rows shown as tab-separated text, to paste into a spreadsheet or a message",
        ))
        .clicked()
    {
        ui.ctx().copy_text(as_text(app));
    }
}
