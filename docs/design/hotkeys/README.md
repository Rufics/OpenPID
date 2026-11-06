# Keyboard shortcuts

The owner's remark (2026-11-05): add keyboard shortcuts; work out the concept
first, make five or more options and pick the best.

## Cause

The application has no keyboard handling at all beyond text in fields: tabs,
opening logs, the player, the epoch are mouse only. For tuning that is a loop of
dozens of tab switches per log.

## What the sources say

- **Apple HIG / Microsoft Win UX:** never redefine the standard combinations
  (Cmd/Ctrl+O, Cmd/Ctrl+S, Cmd/Ctrl+W, F1, Esc); the platform modifier is Cmd
  on macOS, Ctrl on Windows/Linux.
- **VS Code / Figma / GitHub:** single-key actions only outside input fields; a
  key reference under `?` or `Ctrl+/`; a command palette for rare actions.
- **NN/g:** a key hint in a button's tooltip raises usage without training.

## Options

| | Option | Pros | Cons | Score |
|---|---|---|---|---|
| V0 | As is: mouse only | — | slow | 20 |
| V1 | **Digits and the standard keys**: `1`…`9`,`0`,`-`,`=` the tabs in order; `Ctrl+O` Open, `Ctrl+Shift+O` a second log, `Ctrl+S` project, `Ctrl+E` PNG export, `F1` help/tour, `Esc` cancel | as in browsers and Figma; learnt in a minute | digits ≠ names; with 12 tabs `-`/`=` are unusual | **48** |
| V2 | Mnemonic letters without a modifier: `L` viewer, `S` step, `P` spectrum, `T` throttle, `F` filter… (Vim/Gmail style) | fast for experts | conflicts with input fields; unfamiliar to most; i18n (the Ukrainian layout) | 36 |
| V3 | `Ctrl+Tab`/`Ctrl+Shift+Tab` cycle through the tabs, `Ctrl+1…9` | familiar from browsers | 12 tabs is a long cycle; navigation only | 42 |
| V4 | `Alt+letter` with underlined names (classic Windows) | self-documenting | does not work on macOS (Alt = Option, symbols); egui does not underline | 30 |
| V5 | A command palette `Ctrl+K`: search "spectrum", "open", "export…" plus a minimum of keys | scales to any action; nothing to remember | slower than one key for frequent actions; a separate widget | 43 |
| V6 | A configurable keymap in `prefs.json` | everyone's own | an editing UI costs; conflicts are the user's problem | 34 |

Recommendation: **V1 as the base + `Ctrl+Tab` from V3 + the `?` reference**;
V5 (the palette) as a separate topic after the sprint, when there are more actions.

## The owner's decision (2026-11-05)

V1 + V3 + the reference.

## Implementation requirements

**The key table** (`shortcuts.rs`, one table `[(Shortcut, Action, &'static str)]`;
`Modifiers::COMMAND` = Cmd on macOS, Ctrl elsewhere; labels for the reference and tooltips):

| Action | Key |
|---|---|
| Tabs in order | `1`…`9`, `0` (10th), `-` (11th), `=` (12th) |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Open a log into the first empty slot | `Cmd/Ctrl+O` |
| Open into the next slot (comparison) | `Cmd/Ctrl+Shift+O` |
| Close the active slot | `Cmd/Ctrl+W` |
| Project: save / open | `Cmd/Ctrl+S` / `Cmd/Ctrl+Shift+S` |
| Export the current tab as PNG | `Cmd/Ctrl+E` |
| Key reference | `?` (`Shift+/`) or `Cmd/Ctrl+/` |
| Tour / help | `F1` |
| Cancel (tour, dialog, selection) | `Esc` |
| Player (Freq × time): play/pause, step, to start | `Space`, `,` / `.`, `Home` |
| Epoch: Auto / whole log | `A` / `Shift+A` (outside input fields only) |
| Touch mode | `Cmd/Ctrl+T` |

**Rules.**
- Single-key actions (`1`…`=`, `?`, `A`, `Space`, `,`, `.`) work only when
  `!ctx.wants_keyboard_input()` (the focus is not in a text field / DragValue).
- Handling at the start of the frame in `App::frame` through
  `ctx.input_mut(|i| i.consume_shortcut(&sc))`, so widgets never see the key a second time.
- Every button with a key has the tooltip "Open log… (Ctrl+O)", the label from
  the table, so they never diverge.
- The reference: an `egui::Window` "Keyboard shortcuts" with two columns,
  generated from the table; an entry in the Help menu.
- Localisation: labels through `tr`; key names are not translated.
- The digits are bound to the order of `Tab::ALL`; when a tab is added, the
  order does not change without updating the user guide.

## Acceptance criteria

- Headless tests: `2` → Step response; `Ctrl+Tab` from the last tab → the first;
  `?` opens the reference; with a DragValue focused, `2` types a digit and does
  not switch the tab; `Space` on Freq × time starts the player; `Esc` closes the tour.
- Every tooltip of a button with a key contains its label (a test walks the table).
- User guide: a "Keys" section = the table above.

## Size

S (≈ 1 day); the reference ½ day of it.

## Implementation (2026-11-05, #101)

- `crates/openpid-app/src/shortcuts.rs`: the table `TABLE: &[Binding]` (key →
  `Action`); `pressed()` reads it at the start of `App::frame` through
  `consume_shortcut` (in table order; the Shift variants precede the plain ones
  because egui ignores an extra Shift); `perform()` performs the action;
  `help_window()` generates the "Keyboard shortcuts" window from the same table;
  the **Help** menu.
- Single-key actions are skipped while a text field has the focus
  (`TextEdit::load_state` for the focused id, which also catches a `DragValue`
  in edit mode), rather than by `wants_keyboard_input()`: that function is true
  for a focused button after a click as well.
- Every file dialog (buttons and keys) goes through one `App::dialog` queue that
  `run_dialog` runs at the end of the frame; in headless tests `dialogs = false`
  and the tests check which dialog was requested.
- Key labels: `shortcuts::format` (own symbols for `-`, `=`, `?`, `/`, `,`,
  `.`, because `Key::name()` gives "Minus", "Questionmark"); modifier names by
  platform (Cmd on macOS, written in Apple's order "Shift+Cmd").

Deviations from the table:

- `A` (epoch Auto) is not bound; auto-trim arrives with #102; `Shift+A` = the
  whole log for every slot exists.
- `F1` opens the key reference until the tour exists (#104); then `F1` → the
  tour, `?` → the reference. `Esc` closes only the reference for now.
- `Ctrl+W` closes the last opened slot (there is no notion of an "active" slot).
- `Ctrl+Shift+O` opens a log into the first empty slot after the last loaded one
  (or adds a new one); `Ctrl+O` into the first empty slot of all.
- The player keys act only on the Freq × time tab, so `Space` is not taken away
  from buttons on the other tabs.
