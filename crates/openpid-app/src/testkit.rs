//! A driver for the headless UI tests (ADR-0008): a test says what a user
//! does and what must hold, never where a widget is.
//!
//! - **Controls by their accessible name**, through AccessKit actions
//!   (`click_accesskit`, `set_value_accesskit`) instead of pointer clicks at
//!   a widget's centre: a control works wherever it sits, also scrolled out
//!   of view in a long panel, at any window size and in touch mode.
//! - **Plots in data units**, through the frame's [`crate::probe::Probes`]:
//!   `ui.plot("log/roll").click(14.2)` finds the pixel itself.
//! - **No frame counting**: every action ends with [`Ui::settle`], which runs
//!   frames until the worker is idle and the UI has had time to follow.
//! - **Every size**: [`at_every_size`] runs one test body on several window
//!   sizes and in touch mode.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed lookup is the failure"
)]
#![allow(dead_code, reason = "a toolkit: not every helper has a test yet")]

use crate::app::{App, Cached, SlotState, Tab};
use crate::probe::PlotProbe;
use eframe::egui::{self, accesskit::Role, accesskit::Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Frames without work after which the UI counts as at rest.
const QUIET_FRAMES: usize = 3;

/// A window to run a test in.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub name: &'static str,
    pub size: (f32, f32),
    pub touch: bool,
}

/// The windows [`at_every_size`] runs a test in: the design size, a small
/// laptop, a large monitor, and touch mode (larger controls).
pub const SCREENS: [Screen; 4] = [
    Screen {
        name: "1280x800",
        size: (1280.0, 800.0),
        touch: false,
    },
    Screen {
        name: "1024x640",
        size: (1024.0, 640.0),
        touch: false,
    },
    Screen {
        name: "1920x1080",
        size: (1920.0, 1080.0),
        touch: false,
    },
    Screen {
        name: "1280x800 touch",
        size: (1280.0, 800.0),
        touch: true,
    },
];

/// Runs `test` once per [`SCREENS`] entry; a failure names the screen.
pub fn at_every_size(test: impl Fn(Screen)) {
    for screen in SCREENS {
        eprintln!("screen: {}", screen.name);
        test(screen);
    }
}

/// The app under test.
pub struct Ui {
    h: Harness<'static, Option<App>>,
}

impl Ui {
    /// The app with `files` open on `tab`, at rest.
    pub fn open(files: &[&str], tab: Tab, screen: Screen) -> Ui {
        let mut init = Some((
            files.iter().map(PathBuf::from).collect::<Vec<_>>(),
            screen.touch,
        ));
        let h = Harness::builder()
            .with_size(egui::Vec2::new(screen.size.0, screen.size.1))
            .with_accessibility_check(true)
            .allow_missing_glyphs()
            .with_max_steps(500)
            .build_ui_state(
                move |ui, state: &mut Option<App>| {
                    let app = state.get_or_insert_with(|| {
                        let (files, touch) = init.take().expect("built once");
                        let mut app = App::with_prefs_path(ui.ctx(), files, Some(tab), None);
                        // the first-run tour is tested on its own
                        app.auto_tour = false;
                        app.touch_mode = touch;
                        app
                    });
                    app.frame(ui);
                },
                None,
            );
        let mut ui = Ui { h };
        ui.settle();
        ui
    }

    /// The app's state, for assertions.
    pub fn app(&self) -> &App {
        self.h.state().as_ref().expect("app built on first frame")
    }

    /// The app's state, to set up what has no control.
    pub fn app_mut(&mut self) -> &mut App {
        self.h
            .state_mut()
            .as_mut()
            .expect("app built on first frame")
    }

    /// Runs frames until no log is loading and no analysis is pending, and
    /// the UI has drawn the result.
    pub fn settle(&mut self) {
        let t0 = Instant::now();
        let mut quiet = 0;
        while quiet < QUIET_FRAMES {
            self.h.step();
            let app = self.app();
            let busy = app
                .slots
                .iter()
                .any(|e| matches!(e.state, SlotState::Loading(_)))
                || app.cache.values().any(|c| matches!(c, Cached::Pending));
            if busy {
                quiet = 0;
                assert!(
                    t0.elapsed() < Duration::from_secs(120),
                    "timed out waiting for the worker"
                );
                std::thread::sleep(Duration::from_millis(2));
            } else {
                quiet += 1;
            }
        }
    }

    // ------------------------------------------------------------ controls

    /// Clicks the control named `label` (a button, a tab, a menu entry).
    pub fn click(&mut self, label: &str) {
        self.h.get_by_label(label).click_accesskit();
        self.settle();
    }

    /// Switches to `tab`.
    pub fn tab(&mut self, tab: Tab) {
        self.click(tab.title());
        assert_eq!(self.app().tab, tab);
    }

    /// Whether the check box named `label` is checked.
    pub fn is_checked(&self, label: &str) -> bool {
        let node = self.h.get_by_role_and_label(Role::CheckBox, label);
        node.accesskit_node().toggled() == Some(Toggled::True)
    }

    /// Whether the control named `label` can be used.
    pub fn is_enabled(&self, label: &str) -> bool {
        !self.h.get_by_label(label).accesskit_node().is_disabled()
    }

    /// Sets the check box named `label` to `on`.
    pub fn check(&mut self, label: &str, on: bool) {
        if self.is_checked(label) != on {
            self.h
                .get_by_role_and_label(Role::CheckBox, label)
                .click_accesskit();
            self.settle();
        }
        assert_eq!(self.is_checked(label), on, "check box {label}");
    }

    /// Types `value` into the number field named `label`.
    pub fn set(&mut self, label: &str, value: f64) {
        self.h
            .get_by_role_and_label(Role::SpinButton, label)
            .set_value_accesskit(value);
        self.settle();
    }

    /// Picks `option` in the combo box named `label`.
    pub fn choose(&mut self, label: &str, option: &str) {
        self.h
            .get_by_role_and_label(Role::ComboBox, label)
            .click_accesskit();
        self.settle();
        self.click(option);
    }

    /// Presses `key`.
    pub fn key(&mut self, key: egui::Key) {
        self.h.key_press(key);
        self.settle();
    }

    /// Whether some text on screen contains `text`.
    pub fn sees(&self, text: &str) -> bool {
        self.h.query_all_by_label_contains(text).next().is_some()
    }

    /// Whether a control named exactly `label` exists.
    pub fn has(&self, label: &str) -> bool {
        self.h.query_by_label(label).is_some()
    }

    // --------------------------------------------------------------- plots

    /// The plot registered under `name` (see `probe.rs`).
    pub fn plot(&mut self, name: &str) -> Plot<'_> {
        Plot {
            ui: self,
            name: name.to_string(),
        }
    }

    fn probe(&self, name: &str) -> PlotProbe {
        self.app().probes.get(name).unwrap_or_else(|| {
            panic!(
                "no plot named {name:?} on screen; this frame has {:?}",
                self.app().probes.names()
            )
        })
    }

    fn pointer_move(&mut self, pos: egui::Pos2) {
        self.h
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(pos));
        self.h.step();
    }

    fn pointer_button(&mut self, pos: egui::Pos2, pressed: bool) {
        self.h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        self.h.step();
    }
}

/// A plot on screen, addressed in its data units.
pub struct Plot<'a> {
    ui: &'a mut Ui,
    name: String,
}

impl Plot<'_> {
    /// Where the plot is and what it shows, this frame.
    pub fn probe(&self) -> PlotProbe {
        self.ui.probe(&self.name)
    }

    /// Height of the plot area on screen.
    pub fn height(&self) -> f32 {
        self.probe().frame.height()
    }

    /// The visible x range.
    pub fn x_range(&self) -> (f64, f64) {
        self.probe().x
    }

    /// The visible y range.
    pub fn y_range(&self) -> (f64, f64) {
        self.probe().y
    }

    /// What a pointer can resolve along x: two screen points, in data units.
    pub fn x_tolerance(&self) -> f64 {
        2.0 * self.probe().x_per_point()
    }

    /// Moves the pointer over `x`.
    pub fn hover(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        self.ui.settle();
    }

    /// Clicks at `x`.
    pub fn click(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        self.ui.pointer_button(pos, true);
        self.ui.pointer_button(pos, false);
        self.ui.settle();
    }

    /// Drags from `from` to `to` along x.
    pub fn drag(&mut self, from: f64, to: f64) {
        let probe = self.probe();
        let (a, b) = (probe.pos_x(from), probe.pos_x(to));
        // a first small move past egui's drag threshold, towards the target
        let nudge = egui::vec2(if b.x >= a.x { 12.0 } else { -12.0 }, 0.0);
        self.ui.pointer_move(a);
        self.ui.pointer_button(a, true);
        self.ui.pointer_move(a + nudge);
        self.ui.pointer_move(b);
        self.ui.pointer_button(b, false);
        self.ui.settle();
    }

    /// Moves the pointer off the plots.
    pub fn leave(&mut self) {
        self.ui.pointer_move(egui::pos2(2.0, 2.0));
        self.ui.settle();
    }
}
