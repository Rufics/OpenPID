//! The look of the app (docs/design/app-shell, style S1'): a graphite dark
//! theme with an indigo accent that no slot colour uses, rounded surfaces and
//! roomier spacing; the light theme shares the accent and the shapes.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

/// Accent: indigo-violet, outside the Okabe–Ito slot palette.
pub const ACCENT: Color32 = Color32::from_rgb(139, 124, 246);
/// Background of the window behind the surfaces (dark).
pub const BG: Color32 = Color32::from_rgb(20, 22, 27);
/// Panels and cards (dark).
pub const SURFACE: Color32 = Color32::from_rgb(28, 31, 38);
/// Controls on a surface (dark).
pub const SURFACE2: Color32 = Color32::from_rgb(35, 39, 51);
/// Hairlines (dark).
pub const LINE: Color32 = Color32::from_rgb(44, 49, 60);
/// Plot background (dark).
pub const PLOT_BG: Color32 = Color32::from_rgb(14, 15, 18);
/// Secondary text (both themes).
pub const MUTED: Color32 = Color32::from_rgb(142, 150, 164);

thread_local! {
    /// The theme this thread draws in, for colours picked without a `Ui`
    /// (as the language in `i18n`).
    static DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Record the theme this thread draws in (every frame, before drawing).
pub fn set_dark(dark: bool) {
    DARK.with(|d| d.set(dark));
}

/// Whether this thread draws in the dark theme.
pub fn is_dark() -> bool {
    DARK.with(std::cell::Cell::get)
}

/// The "warning" yellow of verdicts and log checks: Okabe–Ito yellow on the
/// dark theme, a dark amber that stays readable on the light one.
pub fn warn() -> Color32 {
    if DARK.with(std::cell::Cell::get) {
        Color32::from_rgb(240, 228, 66)
    } else {
        Color32::from_rgb(158, 118, 0)
    }
}

/// Rounding of cards and panels.
pub const CARD_RADIUS: u8 = 10;
/// Rounding of controls.
const CONTROL_RADIUS: u8 = 6;

/// Visuals for the dark (`true`) or light theme.
pub fn visuals(dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let r = CornerRadius::same(CONTROL_RADIUS);
    v.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.45 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(CARD_RADIUS);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    if dark {
        v.panel_fill = SURFACE;
        v.window_fill = SURFACE;
        v.extreme_bg_color = PLOT_BG;
        v.faint_bg_color = Color32::from_rgb(24, 27, 33);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(214, 218, 226));
        v.widgets.inactive.weak_bg_fill = SURFACE2;
        v.widgets.inactive.bg_fill = SURFACE2;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(231, 233, 238));
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(45, 50, 64);
        v.widgets.hovered.bg_fill = Color32::from_rgb(45, 50, 64);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.7));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(52, 58, 74);
        v.widgets.active.bg_fill = Color32::from_rgb(52, 58, 74);
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.open.weak_bg_fill = Color32::from_rgb(45, 50, 64);
    }
    v
}

/// Spacing and text sizes on top of the visuals.
pub fn style(style: &mut egui::Style) {
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(9.0, 4.0);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.window_margin = egui::Margin::same(12);
    if let Some(h) = style.text_styles.get_mut(&egui::TextStyle::Heading) {
        h.size = 17.0;
    }
}

/// A card: a rounded surface with a hairline, as the log cards use.
pub fn card(ui: &egui::Ui) -> egui::Frame {
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(if dark { SURFACE } else { Color32::WHITE })
        .stroke(Stroke::new(
            1.0,
            if dark {
                LINE
            } else {
                Color32::from_rgb(214, 218, 225)
            },
        ))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::symmetric(10, 8))
}

/// The window background behind the cards.
pub fn background(dark: bool) -> Color32 {
    if dark {
        BG
    } else {
        Color32::from_rgb(236, 239, 243)
    }
}

/// "Open" + "PID" in the accent, the app's word mark.
pub fn word_mark(ui: &mut egui::Ui) {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::proportional(18.0);
    job.append(
        "Open",
        0.0,
        egui::TextFormat::simple(font.clone(), ui.visuals().strong_text_color()),
    );
    job.append("PID", 0.0, egui::TextFormat::simple(font, ACCENT));
    ui.label(job);
}

/// A dashed rounded outline (drop zones).
pub fn dashed_outline(ui: &egui::Ui, rect: egui::Rect) {
    let stroke = Stroke::new(1.5, MUTED.gamma_multiply(0.6));
    let p = ui.painter();
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    for seg in [
        [egui::pos2(l, t), egui::pos2(r, t)],
        [egui::pos2(r, t), egui::pos2(r, b)],
        [egui::pos2(r, b), egui::pos2(l, b)],
        [egui::pos2(l, b), egui::pos2(l, t)],
    ] {
        p.extend(egui::Shape::dashed_line(&seg, stroke, 6.0, 4.0));
    }
}
