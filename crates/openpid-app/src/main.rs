//! `openpid-gui`: desktop front-end. All math lives in `openpid-analysis`;
//! this crate only loads files, runs analyses on a worker thread and draws.
//!
//! Usage: `openpid-gui [--tab log|step|spectrum|throttle|time|surface|filter|motors|advice|sliders|diagnostics|settings] [log …]` (up to 8 logs, one slot each)

mod app;
mod axis_layout;
mod colormap;
mod export;
mod filter_tab;
mod grid;
mod i18n;
mod log_tab;
mod motors_tab;
mod panel;
mod prefs;
mod probe;
mod shortcuts;
mod spectrum_tab;
mod surface;
#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_kit;
mod theme;
mod throttle_tab;
mod time_tab;
mod tour;
mod views;
mod worker;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("OpenPID")
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    let (files, tab) = parse_args(std::env::args().skip(1));
    eframe::run_native(
        "OpenPID",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::with_context(&cc.egui_ctx, files, tab)))),
    )
}

// The only eframe-specific glue; everything else runs headless (ADR-0008).
impl eframe::App for app::App {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

/// Splits the command line into log files to open and the tab to start on.
fn parse_args(args: impl Iterator<Item = String>) -> (Vec<std::path::PathBuf>, Option<app::Tab>) {
    let mut files = Vec::new();
    let mut tab = None;
    let mut it = args;
    while let Some(a) = it.next() {
        if a == "--tab" {
            tab = it.next().as_deref().and_then(app::Tab::from_name);
        } else {
            files.push(a.into());
        }
    }
    (files, tab)
}
