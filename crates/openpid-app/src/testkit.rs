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
//!   sizes and in touch mode, and `OPENPID_UI_SCREEN=1024x640 cargo test`
//!   runs every other test on another screen of [`SCREENS`].
//! - **The pointer only where the pointer is the subject**: plots (hover,
//!   click, drag, pinch), tooltips, and what an overlay must swallow
//!   ([`Ui::press`], [`Ui::hover`]).

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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Frames without work after which the UI counts as at rest.
const QUIET_FRAMES: usize = 3;
/// Frames for the pointer to rest before a tooltip is expected.
const TOOLTIP_FRAMES: usize = 6;

/// Step-response input threshold for the tests. The main fixture
/// (`betaflight-4.2.0.BFL`) has only gentle stick inputs, none above the
/// default 80 deg/s, so the default would give "not enough data" on it.
pub const GENTLE_STEP_INPUT: f64 = 20.0;

/// A window to run a test in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub name: &'static str,
    pub size: (f32, f32),
    pub touch: bool,
}

/// The design size: what a test runs on unless it says otherwise.
pub const DESK: Screen = Screen {
    name: "1280x800",
    size: (1280.0, 800.0),
    touch: false,
};

/// The windows [`at_every_size`] runs a test in: the design size, a small
/// laptop, a large monitor, and touch mode (larger controls).
pub const SCREENS: [Screen; 4] = [
    DESK,
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
        name: "touch",
        size: (1280.0, 800.0),
        touch: true,
    },
];

/// The screen of a test that names none: [`DESK`], or the entry of
/// [`SCREENS`] named in `OPENPID_UI_SCREEN` (to run the whole suite on
/// another screen).
pub fn default_screen() -> Screen {
    match std::env::var("OPENPID_UI_SCREEN") {
        Ok(name) => SCREENS
            .into_iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("OPENPID_UI_SCREEN: no screen named {name:?}")),
        Err(_) => DESK,
    }
}

/// Runs `test` once per [`SCREENS`] entry; a failure names the screen.
pub fn at_every_size(test: impl Fn(Screen)) {
    for screen in SCREENS {
        eprintln!("screen: {}", screen.name);
        test(screen);
    }
}

/// How the app under test starts.
#[derive(Debug, Clone)]
pub struct Setup {
    files: Vec<PathBuf>,
    tab: Option<Tab>,
    prefs: Option<PathBuf>,
    screen: Screen,
    tour: bool,
}

impl Setup {
    /// Files given on the command line: logs or a project.
    pub fn files<P: Into<PathBuf>>(mut self, files: impl IntoIterator<Item = P>) -> Self {
        self.files = files.into_iter().map(Into::into).collect();
        self
    }

    /// The tab asked on the command line.
    pub fn tab(mut self, tab: Tab) -> Self {
        self.tab = Some(tab);
        self
    }

    /// The preferences file (none: preferences stay in memory).
    pub fn prefs(mut self, path: &Path) -> Self {
        self.prefs = Some(path.to_path_buf());
        self
    }

    /// The window, when the test is about one (default: [`default_screen`]).
    pub fn screen(mut self, screen: Screen) -> Self {
        self.screen = screen;
        self
    }

    /// Leaves the first-run tour on (it is off in every other test).
    pub fn with_tour(mut self) -> Self {
        self.tour = true;
        self
    }

    /// Starts the app and waits until it is at rest.
    pub fn open(self) -> Ui {
        let Setup {
            files,
            tab,
            prefs,
            screen,
            tour,
        } = self;
        let mut init = Some((files, prefs));
        let h = Harness::builder()
            .with_size(egui::Vec2::new(screen.size.0, screen.size.1))
            .with_accessibility_check(true)
            .allow_missing_glyphs()
            .with_max_steps(500)
            .build_ui_state(
                move |ui, state: &mut Option<App>| {
                    let app = state.get_or_insert_with(|| {
                        let (files, prefs) = init.take().expect("built once");
                        let mut app = App::with_prefs_path(ui.ctx(), files, tab, prefs);
                        app.auto_tour = tour;
                        if screen.touch {
                            app.touch_mode = true;
                        }
                        app.tools.step.min_input = GENTLE_STEP_INPUT;
                        app.tools.recommend.step.min_input = GENTLE_STEP_INPUT;
                        app
                    });
                    app.frame(ui);
                },
                None,
            );
        let mut ui = Ui { h, screen };
        ui.settle();
        ui
    }
}

/// A dropped file, as the window system hands it over.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// The app under test.
pub struct Ui {
    h: Harness<'static, Option<App>>,
    screen: Screen,
}

impl Ui {
    /// How to start the app: `Ui::setup().files([LOG]).tab(Tab::Step).open()`.
    pub fn setup() -> Setup {
        Setup {
            files: Vec::new(),
            tab: None,
            prefs: None,
            screen: default_screen(),
            tour: false,
        }
    }

    /// The app with `files` open on `tab`, at rest.
    pub fn open(files: &[&str], tab: Tab) -> Ui {
        Ui::setup().files(files.iter().copied()).tab(tab).open()
    }

    /// The same in the window `screen`.
    pub fn open_on(files: &[&str], tab: Tab, screen: Screen) -> Ui {
        Ui::setup()
            .files(files.iter().copied())
            .tab(tab)
            .screen(screen)
            .open()
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

    /// The egui context (style, zoom).
    pub fn ctx(&self) -> &egui::Context {
        &self.h.ctx
    }

    /// The window this app runs in.
    pub fn screen(&self) -> Screen {
        self.screen
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

    /// Lets `frames` frames pass: for what advances with time (the player).
    pub fn advance(&mut self, frames: usize) {
        self.h.run_steps(frames);
    }

    // ------------------------------------------------------------ controls

    /// Clicks the control named `label` (a button, a tab, a menu entry).
    pub fn click(&mut self, label: &str) {
        self.h.get_by_label(label).click_accesskit();
        self.settle();
    }

    /// Clicks the button named `label`, when a text beside it has the same
    /// name (a panel row whose control is a button).
    pub fn click_button(&mut self, label: &str) {
        self.h
            .get_by_role_and_label(Role::Button, label)
            .click_accesskit();
        self.settle();
    }

    /// Clicks the one control whose name contains `text`.
    pub fn click_containing(&mut self, text: &str) {
        self.h.get_by_label_contains(text).click_accesskit();
        self.settle();
    }

    /// Switches to `tab`.
    pub fn tab(&mut self, tab: Tab) {
        self.click(tab.title());
        assert_eq!(self.app().tab, tab);
    }

    /// Clicks `item` of the top bar's "···" menu (Open log, Project,
    /// Export, Help, Touch mode), opening the menu when it is closed.
    pub fn menu(&mut self, item: &str) {
        self.open_menu();
        self.click_containing(item);
    }

    /// Opens the top bar's menu unless it is open.
    pub fn open_menu(&mut self) {
        if !self.has("Help") && !self.has(crate::i18n::tr("Help")) {
            self.click(crate::i18n::tr("Menu"));
        }
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

    /// The same for the first of several fields that share the name (the
    /// start and end of every log's window).
    pub fn set_first(&mut self, label: &str, value: f64) {
        self.h
            .get_all_by_role_and_label(Role::SpinButton, label)
            .next()
            .unwrap_or_else(|| panic!("no number field named {label:?}"))
            .set_value_accesskit(value);
        self.settle();
    }

    /// Moves the slider named `label` to `value`.
    pub fn slide(&mut self, label: &str, value: f64) {
        self.h
            .get_by_role_and_label(Role::Slider, label)
            .set_value_accesskit(value);
        self.settle();
    }

    /// Picks the option whose name contains `option` in the combo box named
    /// `label`.
    pub fn choose(&mut self, label: &str, option: &str) {
        self.h
            .get_by_role_and_label(Role::ComboBox, label)
            .click_accesskit();
        self.settle();
        self.click_containing(option);
    }

    /// Picks the option named exactly `option` in the combo box named
    /// `label` and closes its list: a click through `AccessKit` leaves the
    /// list open, and the next click on the box would only close it. For a
    /// test that picks more than once.
    pub fn pick(&mut self, label: &str, option: &str) {
        self.choose_exact(label, option);
        self.h
            .get_by_role_and_label(Role::ComboBox, label)
            .click_accesskit();
        self.settle();
    }

    /// Picks the option named exactly `option` in the combo box named
    /// `label` (when one option's name is part of another's: `RP`, `RPY`).
    pub fn choose_exact(&mut self, label: &str, option: &str) {
        self.h
            .get_by_role_and_label(Role::ComboBox, label)
            .click_accesskit();
        self.settle();
        self.click(option);
    }

    /// Starts editing the number field named `label`, as a click on it does.
    pub fn edit(&mut self, label: &str) {
        self.h
            .get_by_role_and_label(Role::SpinButton, label)
            .click_accesskit();
        self.settle();
    }

    /// Types `text` into the text field named `label`.
    pub fn type_into(&mut self, label: &str, text: &str) {
        let field = self.h.get_by_role_and_label(Role::TextInput, label);
        field.focus();
        field.type_text(text);
        self.settle();
    }

    /// Presses `key`.
    pub fn key(&mut self, key: egui::Key) {
        self.h.key_press(key);
        self.settle();
    }

    /// Presses `key` with `modifiers` held.
    pub fn key_with(&mut self, modifiers: egui::Modifiers, key: egui::Key) {
        self.h.key_press_modifiers(modifiers, key);
        self.settle();
    }

    /// Sends a raw input event (a wheel turn, a pinch, a typed character).
    pub fn send(&mut self, event: egui::Event) {
        self.h.event(event);
        self.settle();
    }

    /// Drops `paths` on the window.
    pub fn drop_files(&mut self, paths: &[&str]) {
        for p in paths {
            self.h
                .input_mut()
                .dropped_files
                .push(Arc::new(Dropped(PathBuf::from(p))));
        }
        self.settle();
    }

    // ------------------------------------------------------------- reading

    /// Whether some text on screen contains `text`.
    pub fn sees(&self, text: &str) -> bool {
        self.h.query_all_by_label_contains(text).next().is_some()
    }

    /// Whether a control or a text named exactly `label` is on screen.
    pub fn has(&self, label: &str) -> bool {
        self.h.query_all_by_label(label).next().is_some()
    }

    /// How many controls of `role` are on screen.
    pub fn count(&self, role: Role) -> usize {
        self.h.query_all_by_role(role).count()
    }

    /// The text of the one multi-line text box on screen.
    pub fn text_box(&self) -> String {
        self.h
            .get_by_role(Role::MultilineTextInput)
            .value()
            .expect("a text box has a value")
    }

    // ---------------------------------------------- where the pointer is the subject

    /// A pointer click on the control named `label`: for what a pointer can
    /// or cannot reach (an overlay that must swallow it). Everything else
    /// uses [`Ui::click`].
    pub fn press(&mut self, label: &str) {
        self.h.get_by_label(label).click();
        self.settle();
    }

    /// Moves the pointer over the control whose name contains `text` (for
    /// its tooltip), scrolling it into view first.
    pub fn hover(&mut self, text: &str) {
        self.h.get_by_label_contains(text).scroll_to_me();
        self.settle();
        self.h.get_by_label_contains(text).hover();
        self.rest_on_it();
    }

    /// The same for the control of `role` named exactly `label`.
    pub fn hover_role(&mut self, role: Role, label: &str) {
        self.h.get_by_role_and_label(role, label).scroll_to_me();
        self.settle();
        self.h.get_by_role_and_label(role, label).hover();
        self.rest_on_it();
    }

    /// Lets the pointer rest where it is until a tooltip is due: egui shows
    /// one after the pointer has been still for its delay, which a tab that
    /// settles in a few frames does not reach.
    fn rest_on_it(&mut self) {
        self.advance(TOOLTIP_FRAMES);
        self.settle();
    }

    /// Screen rectangle of the control of `role` named `label`: for the few
    /// tests whose subject is the layout.
    pub fn rect(&self, role: Role, label: &str) -> egui::Rect {
        self.h.get_by_role_and_label(role, label).rect()
    }

    /// Screen rectangle of the control named `label`.
    pub fn rect_of(&self, label: &str) -> egui::Rect {
        self.h.get_by_label(label).rect()
    }

    // --------------------------------------------------------------- plots

    /// The plot registered under `name` (see `probe.rs`).
    pub fn plot(&mut self, name: &str) -> Plot<'_> {
        Plot {
            ui: self,
            name: name.to_string(),
        }
    }

    /// Whether a plot named `name` was drawn this frame.
    pub fn has_plot(&self, name: &str) -> bool {
        self.app().probes.get(name).is_some()
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

    fn pointer_button(&mut self, pos: egui::Pos2, button: egui::PointerButton, pressed: bool) {
        self.h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        self.h.step();
    }

    fn touch(&mut self, finger: u64, phase: egui::TouchPhase, pos: egui::Pos2) {
        self.h.input_mut().events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(finger),
            phase,
            pos,
            force: None,
        });
    }
}

/// A plot on screen, addressed in its data units.
pub struct Plot<'a> {
    ui: &'a mut Ui,
    name: String,
}

const PRIMARY: egui::PointerButton = egui::PointerButton::Primary;

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

    /// The middle of the visible x range.
    pub fn x_mid(&self) -> f64 {
        let (a, b) = self.x_range();
        f64::midpoint(a, b)
    }

    /// What a pointer can resolve along x: two screen points, in data units.
    pub fn x_tolerance(&self) -> f64 {
        2.0 * self.probe().x_per_point()
    }

    /// Moves the pointer over `x`, at the middle of the plot's height.
    pub fn hover(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        self.ui.settle();
    }

    /// Moves the pointer over the middle of the plot.
    pub fn hover_middle(&mut self) {
        let pos = self.probe().frame.center();
        self.ui.pointer_move(pos);
        self.ui.settle();
    }

    /// Clicks at `x`.
    pub fn click(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        self.ui.pointer_button(pos, PRIMARY, true);
        self.ui.pointer_button(pos, PRIMARY, false);
        self.ui.settle();
    }

    /// Double-clicks at `x`.
    pub fn double_click(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        for _ in 0..2 {
            for pressed in [true, false] {
                self.ui
                    .h
                    .input_mut()
                    .events
                    .push(egui::Event::PointerButton {
                        pos,
                        button: PRIMARY,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
            }
            self.ui.h.step();
        }
        self.ui.settle();
    }

    /// Drags from `from` to `to` along x.
    pub fn drag(&mut self, from: f64, to: f64) {
        self.grab(from, to);
        self.release(to);
    }

    /// The first half of a drag: presses at `from` and moves towards
    /// `towards` just past egui's drag threshold, so a test can look at the
    /// state while the button is down. [`Plot::release`] ends it.
    pub fn grab(&mut self, from: f64, towards: f64) {
        let probe = self.probe();
        let (a, b) = (probe.pos_x(from), probe.pos_x(towards));
        let nudge = egui::vec2(if b.x >= a.x { 12.0 } else { -12.0 }, 0.0);
        self.ui.pointer_move(a);
        self.ui.pointer_button(a, PRIMARY, true);
        self.ui.pointer_move(a + nudge);
    }

    /// Moves the pressed pointer to `x` without releasing it.
    pub fn drag_to(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
    }

    /// Ends a drag at `x`.
    pub fn release(&mut self, x: f64) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        self.ui.pointer_button(pos, PRIMARY, false);
        self.ui.settle();
    }

    /// Presses at `x` and holds still for `frames` frames (a long press).
    pub fn hold(&mut self, x: f64, frames: usize) {
        let pos = self.probe().pos_x(x);
        self.ui.pointer_move(pos);
        self.ui.pointer_button(pos, PRIMARY, true);
        self.ui.advance(frames);
        self.ui.pointer_button(pos, PRIMARY, false);
        self.ui.settle();
    }

    /// Drags `button` from the middle of the plot by `delta` screen points
    /// (for what is not along an axis: orbiting and panning the surface).
    pub fn drag_by(&mut self, button: egui::PointerButton, delta: egui::Vec2) {
        let c = self.probe().frame.center();
        self.ui.pointer_move(c);
        self.ui.pointer_button(c, button, true);
        self.ui.pointer_move(c + delta * 0.5);
        self.ui.pointer_move(c + delta);
        self.ui.pointer_button(c + delta, button, false);
        self.ui.settle();
    }

    /// Two fingers on the middle of the plot, `from` screen points apart
    /// along x, moved to `to` points apart.
    pub fn pinch_x(&mut self, from: f32, to: f32) {
        use egui::TouchPhase::{End, Move, Start};
        let c = self.probe().frame.center();
        let at = |gap: f32, side: f32| egui::pos2(c.x + side * gap / 2.0, c.y);
        self.ui.pointer_move(c);
        for (phase, gap) in [(Start, from), (Move, to)] {
            self.ui.touch(1, phase, at(gap, -1.0));
            self.ui.touch(2, phase, at(gap, 1.0));
            self.ui.h.step();
        }
        self.ui.h.step();
        self.ui.touch(1, End, at(to, -1.0));
        self.ui.touch(2, End, at(to, 1.0));
        self.ui.settle();
    }

    /// Moves the pointer off the plots.
    pub fn leave(&mut self) {
        self.ui.pointer_move(egui::pos2(2.0, 2.0));
        self.ui.settle();
    }
}
