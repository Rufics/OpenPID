//! First-run tour: coach marks over the real interface
//! (docs/design/onboarding-tour, R2 with the sample log of R5).
//!
//! Widgets that a step points at register their rectangle every frame in
//! `App::anchors`; the tour dims everything but the anchor and shows a card
//! with the step text next to it. It runs at the end of the frame, so the
//! anchors of the current frame are already known.

use crate::app::{App, Tab};
use crate::i18n::tr;
use eframe::egui::{self, Color32, Key, Rect, RichText};

/// One coach mark.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    /// Tab the step shows.
    pub tab: Tab,
    /// Widget the step points at (an `App::anchors` key).
    pub anchor: &'static str,
    /// Used when `anchor` is not on screen (a log without the widget).
    pub fallback: &'static str,
    /// Text (English key for `tr`).
    pub text: &'static str,
}

/// The seven steps, in order.
pub const STEPS: [Step; 7] = [
    Step {
        tab: Tab::LogViewer,
        anchor: "slot-bar",
        fallback: "tab-log",
        text: "Your logs live here. Drop another one to compare.",
    },
    Step {
        tab: Tab::LogViewer,
        anchor: "epoch-handles",
        fallback: "tab-log",
        text: "Choose the analysis window: drag the handles, or press Auto.",
    },
    Step {
        tab: Tab::Spectrum,
        anchor: "tab-spectrum",
        fallback: "tab-spectrum",
        text: "Noise: peaks above 80 Hz are notch candidates.",
    },
    Step {
        tab: Tab::ThrottleHeatmap,
        anchor: "tab-throttle",
        fallback: "tab-throttle",
        text: "Noise over throttle, and the motor harmonics (RPM overlay).",
    },
    Step {
        tab: Tab::FilterSim,
        anchor: "tab-filter",
        fallback: "tab-filter",
        text: "Try new filters without flying.",
    },
    Step {
        tab: Tab::Step,
        anchor: "tab-step",
        fallback: "tab-step",
        text: "Stick response: overshoot and delay, judged against your targets.",
    },
    Step {
        tab: Tab::Advice,
        anchor: "advice-cli",
        fallback: "tab-advice",
        text: "Advice with its evidence and ready CLI commands. Copy and apply.",
    },
];

/// Padding around the anchor in the cut-out.
const CUTOUT_PAD: f32 = 6.0;
/// Width of the card.
const CARD_W: f32 = 320.0;

/// Tour state.
#[derive(Debug, Default)]
pub struct Tour {
    pub active: bool,
    /// Index into `STEPS`.
    pub step: usize,
    /// The cut-out drawn last frame (tests).
    pub cutout: Option<Rect>,
}

/// What the card asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nav {
    Back,
    Next,
    Skip,
}

impl App {
    /// Start (or restart) the tour on its first step. Without a loaded log
    /// the sample log is opened first, so every step has something to show.
    pub fn start_tour(&mut self) {
        if self.first_loaded().is_none() {
            self.open_sample();
        }
        self.tour = Tour {
            active: true,
            step: 0,
            cutout: None,
        };
        self.show_help = false;
    }

    /// Leave the tour and remember that it was seen.
    pub fn end_tour(&mut self) {
        self.tour.active = false;
        self.tour.cutout = None;
        if !self.prefs.tour_done {
            self.prefs.tour_done = true;
            self.save_prefs();
        }
    }

    /// The tour's keys, read before the other shortcuts: Right / Enter next,
    /// Left back, Escape skip.
    pub fn tour_keys(&mut self, ctx: &egui::Context) {
        if !self.tour.active {
            return;
        }
        let nav = ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                Some(Nav::Skip)
            } else if i.consume_key(egui::Modifiers::NONE, Key::ArrowRight)
                || i.consume_key(egui::Modifiers::NONE, Key::Enter)
            {
                Some(Nav::Next)
            } else if i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft) {
                Some(Nav::Back)
            } else {
                None
            }
        });
        if let Some(nav) = nav {
            self.tour_nav(nav);
        }
    }

    fn tour_nav(&mut self, nav: Nav) {
        match nav {
            Nav::Back => self.tour.step = self.tour.step.saturating_sub(1),
            Nav::Next => {
                if self.tour.step + 1 < STEPS.len() {
                    self.tour.step += 1;
                } else {
                    self.end_tour();
                }
            }
            Nav::Skip => self.end_tour(),
        }
    }

    /// Keep the tab on the step's tab; call at the start of the frame.
    pub fn tour_tab(&mut self) {
        if self.tour.active {
            self.tab = STEPS[self.tour.step].tab;
        }
    }

    /// Draw the dimming, the cut-out and the card; call after the panels.
    pub fn tour_overlay(&mut self, ctx: &egui::Context) {
        if !self.tour.active {
            return;
        }
        let step = STEPS[self.tour.step];
        let screen = ctx.content_rect();
        let anchor = self
            .anchors
            .get(step.anchor)
            .or_else(|| self.anchors.get(step.fallback))
            .copied()
            .unwrap_or_else(|| Rect::from_center_size(screen.center(), egui::vec2(1.0, 1.0)))
            .intersect(screen);
        let cutout = anchor.expand(CUTOUT_PAD).intersect(screen);
        self.tour.cutout = Some(cutout);
        let touch = self.touch_mode;
        let n = STEPS.len();
        let index = self.tour.step;
        let mut nav: Option<Nav> = None;
        egui::Area::new(egui::Id::unique("tour"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .interactable(true)
            .show(ctx, |ui| {
                // a full-screen widget swallows the clicks meant for the UI below
                let (_, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, tr("Tour")));
                // the area's own clip rect is its content so far: paint on the whole screen
                let painter = ui.painter().with_clip_rect(screen);
                let dim = Color32::from_black_alpha(140);
                // four rectangles around the cut-out
                painter.rect_filled(
                    Rect::from_min_max(screen.min, egui::pos2(screen.max.x, cutout.min.y)),
                    0.0,
                    dim,
                );
                painter.rect_filled(
                    Rect::from_min_max(egui::pos2(screen.min.x, cutout.max.y), screen.max),
                    0.0,
                    dim,
                );
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(screen.min.x, cutout.min.y),
                        egui::pos2(cutout.min.x, cutout.max.y),
                    ),
                    0.0,
                    dim,
                );
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(cutout.max.x, cutout.min.y),
                        egui::pos2(screen.max.x, cutout.max.y),
                    ),
                    0.0,
                    dim,
                );
                painter.rect_stroke(
                    cutout,
                    6.0,
                    egui::Stroke::new(2.0, Color32::from_rgb(86, 180, 233)),
                    egui::StrokeKind::Outside,
                );
                // the card: below the anchor when there is room, else above,
                // clamped to the screen
                let card_h = if touch { 170.0 } else { 140.0 };
                let x = (cutout.center().x - CARD_W / 2.0).clamp(
                    screen.min.x + 8.0,
                    (screen.max.x - CARD_W - 8.0).max(screen.min.x),
                );
                let y = if cutout.max.y + 8.0 + card_h <= screen.max.y {
                    cutout.max.y + 8.0
                } else {
                    (cutout.min.y - 8.0 - card_h).max(screen.min.y + 8.0)
                };
                let card = Rect::from_min_size(egui::pos2(x, y), egui::vec2(CARD_W, card_h));
                let title = format!("{} {} {} {}", tr("Step"), index + 1, tr("of"), n);
                let mut card_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(card)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                egui::Frame::popup(card_ui.style())
                    .inner_margin(12.0)
                    .show(&mut card_ui, |ui| {
                        ui.set_width(CARD_W - 24.0);
                        ui.label(RichText::new(&title).strong());
                        ui.label(tr(step.text));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let size = if touch {
                                egui::vec2(72.0, 44.0)
                            } else {
                                egui::vec2(64.0, 24.0)
                            };
                            if ui
                                .add_enabled(
                                    index > 0,
                                    egui::Button::new(tr("Back")).min_size(size),
                                )
                                .clicked()
                            {
                                nav = Some(Nav::Back);
                            }
                            let next = if index + 1 < n {
                                tr("Next")
                            } else {
                                tr("Done")
                            };
                            if ui.add(egui::Button::new(next).min_size(size)).clicked() {
                                nav = Some(Nav::Next);
                            }
                            if ui
                                .add(egui::Button::new(tr("Skip")).min_size(size))
                                .clicked()
                            {
                                nav = Some(Nav::Skip);
                            }
                            ui.label(
                                RichText::new(tr("keys: Right / Enter, Left, Esc"))
                                    .small()
                                    .weak(),
                            );
                        });
                    });
            });
        if let Some(nav) = nav {
            self.tour_nav(nav);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_cover_the_tuning_loop() {
        assert_eq!(STEPS.len(), 7);
        for s in STEPS {
            assert!(!s.text.is_empty() && !s.anchor.is_empty() && !s.fallback.is_empty());
        }
        let tabs: Vec<Tab> = STEPS.iter().map(|s| s.tab).collect();
        assert_eq!(tabs[0], Tab::LogViewer);
        assert_eq!(tabs[6], Tab::Advice);
        assert!(tabs.contains(&Tab::Step) && tabs.contains(&Tab::Spectrum));
    }
}
