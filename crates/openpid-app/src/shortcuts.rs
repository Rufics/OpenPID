//! Keyboard shortcuts (docs/design/hotkeys): one table of bindings, read at
//! the start of every frame, and the help window generated from it.
//!
//! Rules:
//! - Single-key bindings (no Cmd/Ctrl/Alt) are skipped while a text field has
//!   the keyboard, so typing `2` into a `DragValue` never switches the tab.
//! - Bindings are matched in table order and `Modifiers::matches_logically`
//!   ignores an extra Shift, so a Shift variant must precede its plain one.
//! - `Modifiers::COMMAND` is Cmd on macOS and Ctrl elsewhere; the labels come
//!   from `Context::format_shortcut`, so they follow the platform too.

use crate::app::{App, Tab};
use crate::i18n::tr;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Switch to the n-th tab of `Tab::ALL`.
    Tab(usize),
    NextTab,
    PrevTab,
    /// Open a log into the first empty slot, or a new one.
    OpenLog,
    /// Open a log into a new slot, to compare.
    OpenLogNext,
    /// Close the last opened slot.
    CloseSlot,
    SaveProject,
    OpenProject,
    ExportPng,
    /// The shortcuts window (`?`); `F1` opens it too until the tour exists (#104).
    Help,
    /// Close the help window.
    Cancel,
    PlayPause,
    StepBack,
    StepForward,
    ToStart,
    /// Every slot analyses its whole log.
    EpochWhole,
    TouchMode,
}

impl Action {
    /// Label for the help window and tooltips (English key, translated by `tr`).
    pub fn label(self) -> String {
        match self {
            Action::Tab(i) => format!(
                "{} {}: {}",
                tr("Tab"),
                i + 1,
                Tab::ALL.get(i).map_or("?", |t| t.title())
            ),
            Action::NextTab => tr("Next tab").into(),
            Action::PrevTab => tr("Previous tab").into(),
            Action::OpenLog => format!("{}… ({})", tr("Open log"), tr("first empty slot")),
            Action::OpenLogNext => format!("{}… ({})", tr("Open log"), tr("new slot, to compare")),
            Action::CloseSlot => tr("Close the last opened slot").into(),
            Action::SaveProject => format!("{}: {}", tr("Project"), tr("Save as…")),
            Action::OpenProject => format!("{}: {}", tr("Project"), tr("Open…")),
            Action::ExportPng => format!("{}: {}", tr("Export"), tr("Plot as PNG…")),
            Action::Help => tr("Keyboard shortcuts").into(),
            Action::Cancel => tr("Close the help window").into(),
            Action::PlayPause => format!("{}: {} / {}", tr("Player"), tr("Play"), tr("Pause")),
            Action::StepBack => format!("{}: {}", tr("Player"), tr("< step")),
            Action::StepForward => format!("{}: {}", tr("Player"), tr("step >")),
            Action::ToStart => format!("{}: {}", tr("Player"), tr("To start")),
            Action::EpochWhole => format!("{}: {}", tr("epoch"), tr("whole log, every slot")),
            Action::TouchMode => tr("Touch mode").into(),
        }
    }

    /// Player actions apply on the Freq × time tab only.
    fn is_player(self) -> bool {
        matches!(
            self,
            Action::PlayPause | Action::StepBack | Action::StepForward | Action::ToStart
        )
    }
}

/// One key binding.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub keys: KeyboardShortcut,
    pub action: Action,
}

const fn b(modifiers: Modifiers, key: Key, action: Action) -> Binding {
    Binding {
        keys: KeyboardShortcut::new(modifiers, key),
        action,
    }
}

const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

/// The whole table, in matching order (Shift variants first, see the module doc).
pub const TABLE: &[Binding] = &[
    b(Modifiers::NONE, Key::Num1, Action::Tab(0)),
    b(Modifiers::NONE, Key::Num2, Action::Tab(1)),
    b(Modifiers::NONE, Key::Num3, Action::Tab(2)),
    b(Modifiers::NONE, Key::Num4, Action::Tab(3)),
    b(Modifiers::NONE, Key::Num5, Action::Tab(4)),
    b(Modifiers::NONE, Key::Num6, Action::Tab(5)),
    b(Modifiers::NONE, Key::Num7, Action::Tab(6)),
    b(Modifiers::NONE, Key::Num8, Action::Tab(7)),
    b(Modifiers::NONE, Key::Num9, Action::Tab(8)),
    b(Modifiers::NONE, Key::Num0, Action::Tab(9)),
    b(Modifiers::NONE, Key::Minus, Action::Tab(10)),
    b(Modifiers::NONE, Key::Equals, Action::Tab(11)),
    b(CMD_SHIFT, Key::Tab, Action::PrevTab),
    b(Modifiers::COMMAND, Key::Tab, Action::NextTab),
    b(CMD_SHIFT, Key::O, Action::OpenLogNext),
    b(Modifiers::COMMAND, Key::O, Action::OpenLog),
    b(Modifiers::COMMAND, Key::W, Action::CloseSlot),
    b(CMD_SHIFT, Key::S, Action::OpenProject),
    b(Modifiers::COMMAND, Key::S, Action::SaveProject),
    b(Modifiers::COMMAND, Key::E, Action::ExportPng),
    b(Modifiers::NONE, Key::Questionmark, Action::Help),
    b(Modifiers::COMMAND, Key::Slash, Action::Help),
    b(Modifiers::NONE, Key::F1, Action::Help),
    b(Modifiers::NONE, Key::Escape, Action::Cancel),
    b(Modifiers::NONE, Key::Space, Action::PlayPause),
    b(Modifiers::NONE, Key::Comma, Action::StepBack),
    b(Modifiers::NONE, Key::Period, Action::StepForward),
    b(Modifiers::NONE, Key::Home, Action::ToStart),
    b(Modifiers::SHIFT, Key::A, Action::EpochWhole),
    b(Modifiers::COMMAND, Key::T, Action::TouchMode),
];

/// A binding without Cmd/Ctrl/Alt: it must not steal keys from text fields.
fn is_single_key(keys: KeyboardShortcut) -> bool {
    let m = keys.modifiers;
    !(m.command || m.ctrl || m.mac_cmd || m.alt)
}

/// A text field (`TextEdit`, or a `DragValue` being edited) has the keyboard.
pub fn text_input_focused(ctx: &egui::Context) -> bool {
    ctx.memory(egui::Memory::focused)
        .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
}

/// One shortcut for this platform: "Ctrl+O" ("Cmd+O" on macOS), "?", "Shift+A".
pub fn format(ctx: &egui::Context, keys: KeyboardShortcut) -> String {
    let key = match keys.logical_key {
        Key::Minus => "-",
        Key::Equals => "=",
        Key::Questionmark => "?",
        Key::Slash => "/",
        Key::Comma => ",",
        Key::Period => ".",
        k => k.name(),
    };
    // Names, not symbols: `Context::format_modifiers` needs loaded fonts to
    // decide on ⌘/⇧ and panics in a bare context (unit tests on macOS).
    let modifiers = egui::ModifierNames::NAMES.format(&keys.modifiers, ctx.os().is_mac());
    if modifiers.is_empty() {
        key.to_string()
    } else {
        format!("{modifiers}+{key}")
    }
}

/// Keys of an action, every binding joined: "? / Ctrl+/ / F1".
pub fn keys_of(ctx: &egui::Context, action: Action) -> String {
    TABLE
        .iter()
        .filter(|b| b.action == action)
        .map(|b| format(ctx, b.keys))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// `text (keys)` for a tooltip.
pub fn hint(ctx: &egui::Context, text: &str, action: Action) -> String {
    format!("{text} ({})", keys_of(ctx, action))
}

/// Consume the shortcuts pressed this frame, in table order.
pub fn pressed(ctx: &egui::Context, tab: Tab) -> Vec<Action> {
    let text_focus = text_input_focused(ctx);
    let mut out = Vec::new();
    for binding in TABLE {
        if text_focus && is_single_key(binding.keys) {
            continue;
        }
        if binding.action.is_player() && tab != Tab::TimeHeatmap {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&binding.keys)) {
            out.push(binding.action);
        }
    }
    out
}

impl App {
    /// Read and perform this frame's shortcuts; call before the panels are drawn.
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        for action in pressed(ctx, self.tab) {
            self.perform(action);
        }
    }

    /// Perform one shortcut action.
    pub fn perform(&mut self, action: Action) {
        self.last_action = Some(action);
        let n = Tab::ALL.len();
        let index = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        match action {
            Action::Tab(i) => {
                if let Some(t) = Tab::ALL.get(i) {
                    self.tab = *t;
                }
            }
            Action::NextTab => self.tab = Tab::ALL[(index + 1) % n],
            Action::PrevTab => self.tab = Tab::ALL[(index + n - 1) % n],
            Action::OpenLog => {
                if let Some(slot) = self.free_slot() {
                    self.dialog = Some(crate::app::Dialog::OpenLog(slot));
                }
            }
            Action::OpenLogNext => {
                if let Some(slot) = self.next_slot() {
                    self.dialog = Some(crate::app::Dialog::OpenLog(slot));
                }
            }
            Action::CloseSlot => {
                if let Some(slot) = self.loaded_slots().last().copied() {
                    self.close_slot(slot);
                }
            }
            Action::SaveProject => self.dialog = Some(crate::app::Dialog::SaveProject),
            Action::OpenProject => self.dialog = Some(crate::app::Dialog::OpenProject),
            Action::ExportPng => self.dialog = Some(crate::app::Dialog::ExportPng),
            Action::Help => self.show_help = !self.show_help,
            Action::Cancel => self.show_help = false,
            Action::PlayPause => {
                self.player.playing = !self.player.playing;
                self.player.last_frame_time = None;
            }
            Action::StepBack => self.player.request = Some(crate::views::PlayerRequest::StepBack),
            Action::StepForward => {
                self.player.request = Some(crate::views::PlayerRequest::StepForward);
            }
            Action::ToStart => self.player.request = Some(crate::views::PlayerRequest::ToStart),
            Action::EpochWhole => {
                for slot in self.loaded_slots() {
                    if let Some(l) = self.loaded_mut(slot)
                        && l.epoch.is_some()
                    {
                        l.set_epoch(None, false);
                        self.cache.retain(|k, _| k.slot != slot);
                    }
                }
            }
            Action::TouchMode => self.touch_mode = !self.touch_mode,
        }
    }

    /// The "Keyboard shortcuts" window: one row per action, generated from the table.
    pub fn help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Keyboard shortcuts"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(tr(
                    "Single keys work while no text field has the keyboard; Cmd on macOS, Ctrl elsewhere.",
                ));
                egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                    let mut seen: Vec<Action> = Vec::new();
                    for binding in TABLE {
                        if seen.contains(&binding.action) {
                            continue;
                        }
                        seen.push(binding.action);
                        ui.label(binding.action.label());
                        ui.label(egui::RichText::new(keys_of(ctx, binding.action)).monospace());
                        ui.end_row();
                    }
                });
            });
        self.show_help = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_every_tab_and_no_duplicate_keys() {
        for i in 0..Tab::ALL.len() {
            assert!(TABLE.iter().any(|b| b.action == Action::Tab(i)), "tab {i}");
        }
        for (n, a) in TABLE.iter().enumerate() {
            for b in &TABLE[n + 1..] {
                assert!(
                    a.keys != b.keys,
                    "{:?} bound twice: {:?} and {:?}",
                    a.keys,
                    a.action,
                    b.action
                );
            }
        }
        // a Shift variant precedes its plain one (matching ignores an extra Shift)
        let pos = |m: Modifiers, k: Key| {
            TABLE
                .iter()
                .position(|b| b.keys == KeyboardShortcut::new(m, k))
                .unwrap()
        };
        assert!(pos(CMD_SHIFT, Key::Tab) < pos(Modifiers::COMMAND, Key::Tab));
        assert!(pos(CMD_SHIFT, Key::O) < pos(Modifiers::COMMAND, Key::O));
        assert!(pos(CMD_SHIFT, Key::S) < pos(Modifiers::COMMAND, Key::S));
    }

    #[test]
    fn labels_and_keys_are_non_empty() {
        let ctx = egui::Context::default();
        for b in TABLE {
            assert!(!b.action.label().is_empty());
            assert!(!keys_of(&ctx, b.action).is_empty());
        }
        // egui orders the names the Apple way on macOS: "Shift+Cmd+Tab"
        let (cmd, cmd_shift) = if ctx.os().is_mac() {
            ("Cmd", "Shift+Cmd")
        } else {
            ("Ctrl", "Ctrl+Shift")
        };
        assert_eq!(keys_of(&ctx, Action::Help), format!("? / {cmd}+/ / F1"));
        assert_eq!(keys_of(&ctx, Action::Tab(10)), "-");
        assert_eq!(keys_of(&ctx, Action::PrevTab), format!("{cmd_shift}+Tab"));
        assert_eq!(keys_of(&ctx, Action::EpochWhole), "Shift+A");
        assert!(hint(&ctx, "Open log…", Action::OpenLog).contains("O)"));
        assert!(is_single_key(KeyboardShortcut::new(
            Modifiers::SHIFT,
            Key::A
        )));
        assert!(!is_single_key(KeyboardShortcut::new(
            Modifiers::COMMAND,
            Key::A
        )));
    }
}
