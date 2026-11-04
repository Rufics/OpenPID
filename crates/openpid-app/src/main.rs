//! `openpid-gui`: desktop front-end. All math lives in `openpid-analysis`;
//! this crate only loads files, runs analyses on a worker thread and draws.

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
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    eframe::run_native(
        "OpenPID",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, args)))),
    )
}
