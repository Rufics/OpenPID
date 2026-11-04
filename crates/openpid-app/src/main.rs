//! `openpid-gui`: desktop front-end. All math lives in `openpid-analysis`;
//! this crate only loads files, runs analyses on a worker thread and draws.
//!
//! Usage: `openpid-gui [--tab log|step|spectrum|throttle|time|diagnostics] [logA] [logB]`

mod app;
mod colormap;
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
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut tab: Option<app::Tab> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--tab" {
            tab = it.next().as_deref().and_then(app::Tab::from_name);
        } else {
            files.push(a.into());
        }
    }
    eframe::run_native(
        "OpenPID",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, files, tab)))),
    )
}
