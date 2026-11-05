//! Application state and the top-level frame.

use crate::prefs::{Prefs, Theme};
use crate::views;
use crate::worker::{Done, Job, JobKey, Kind, Output, Slot, Worker};
use eframe::egui;
use openpid_analysis::{
    Epoch, FilterSimParams, MotorNoiseParams, PsdParams, StepParams, StftParams,
    ThrottleHeatmapParams,
};
use openpid_core::{Axis, ChannelId, FlightSession, GyroStage};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    LogViewer,
    Step,
    Spectrum,
    ThrottleHeatmap,
    TimeHeatmap,
    FilterSim,
    MotorNoise,
    Sliders,
    Diagnostics,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 10] = [
        Tab::LogViewer,
        Tab::Step,
        Tab::Spectrum,
        Tab::ThrottleHeatmap,
        Tab::TimeHeatmap,
        Tab::FilterSim,
        Tab::MotorNoise,
        Tab::Sliders,
        Tab::Diagnostics,
        Tab::Settings,
    ];

    pub fn from_name(name: &str) -> Option<Tab> {
        Some(match name {
            "log" => Tab::LogViewer,
            "step" => Tab::Step,
            "spectrum" => Tab::Spectrum,
            "throttle" => Tab::ThrottleHeatmap,
            "time" => Tab::TimeHeatmap,
            "filter" => Tab::FilterSim,
            "motors" => Tab::MotorNoise,
            "sliders" => Tab::Sliders,
            "diagnostics" => Tab::Diagnostics,
            "settings" => Tab::Settings,
            _ => return None,
        })
    }

    /// Short name used on the command line and in exported file names.
    pub fn name(self) -> &'static str {
        match self {
            Tab::LogViewer => "log",
            Tab::Step => "step",
            Tab::Spectrum => "spectrum",
            Tab::ThrottleHeatmap => "throttle",
            Tab::TimeHeatmap => "time",
            Tab::FilterSim => "filter",
            Tab::MotorNoise => "motors",
            Tab::Sliders => "sliders",
            Tab::Diagnostics => "diagnostics",
            Tab::Settings => "settings",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::LogViewer => "Log viewer",
            Tab::Step => "Step response",
            Tab::Spectrum => "Spectrum",
            Tab::ThrottleHeatmap => "Freq × throttle",
            Tab::TimeHeatmap => "Freq × time",
            Tab::FilterSim => "Filter Sim",
            Tab::MotorNoise => "Motor noise",
            Tab::Sliders => "PID sliders",
            Tab::Diagnostics => "Diagnostics",
            Tab::Settings => "Settings",
        }
    }
}

/// A loaded file in slot A or B.
#[derive(Debug)]
pub struct Loaded {
    pub path: PathBuf,
    pub sessions: Vec<Arc<FlightSession>>,
    pub session: usize,
    pub epoch: Option<Epoch>,
}

impl Loaded {
    pub fn current(&self) -> Option<&Arc<FlightSession>> {
        self.sessions.get(self.session)
    }
}

#[derive(Debug)]
pub enum SlotState {
    Empty,
    Loading(PathBuf),
    Failed(PathBuf, String),
    Ready(Loaded),
}

#[derive(Debug)]
pub enum Cached {
    Pending,
    Ready(Arc<Output>),
    Failed(String),
}

/// Tool parameters that the user edits; analysis params are derived from them.
#[derive(Debug, Clone)]
pub struct Tools {
    pub axis: Axis,
    pub step: StepParams,
    pub psd: PsdParams,
    pub throttle: ThrottleHeatmapParams,
    pub stft: StftParams,
    /// Filter Sim axis, spectra and what-if overrides.
    pub filter_sim: FilterSimParams,
    /// Motor noise table parameters.
    pub motor_noise: MotorNoiseParams,
    pub spectrum_channel: ChannelId,
    pub heatmap_channel: ChannelId,
    pub psd_db: bool,
    pub heatmap_max: f64,
    /// Throttle bins with fewer segments than this are greyed out on the freq × throttle heatmap.
    pub min_segments: usize,
    pub viewer_channels: Vec<ChannelId>,
}

impl Default for Tools {
    fn default() -> Self {
        let gyro = ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Filtered,
        };
        Tools {
            axis: Axis::Roll,
            step: StepParams::default(),
            psd: PsdParams::default(),
            throttle: ThrottleHeatmapParams::default(),
            stft: StftParams::default(),
            filter_sim: FilterSimParams::default(),
            motor_noise: MotorNoiseParams::default(),
            spectrum_channel: gyro.clone(),
            heatmap_channel: gyro.clone(),
            psd_db: true,
            heatmap_max: 0.0,
            min_segments: 3,
            viewer_channels: vec![
                gyro,
                ChannelId::Setpoint { axis: Axis::Roll },
                ChannelId::Throttle,
            ],
        }
    }
}

pub struct App {
    pub worker: Worker,
    pub slots: HashMap<Slot, SlotState>,
    pub cache: HashMap<JobKey, Cached>,
    pub tab: Tab,
    pub tools: Tools,
    pub touch_mode: bool,
    pub touch_applied: bool,
    pub status: String,
    pub viewer: views::ViewerState,
    pub spectrum: views::SpectrumState,
    pub slider_state: views::SliderState,
    /// RPM / notch overlay on the freq × throttle heatmap.
    pub overlay: views::OverlayState,
    /// Persisted preferences (Settings tab).
    pub prefs: Prefs,
    /// Where `prefs` are saved; `None` = no config directory, keep them for the session.
    pub prefs_path: Option<PathBuf>,
    /// Theme the egui visuals currently follow.
    theme_applied: Option<Theme>,
    /// Screen rectangle of the tab area, for the PNG export.
    pub central_rect: egui::Rect,
    /// Outcome of the last PNG export, filled by the screenshot callback.
    pub export_message: Arc<std::sync::Mutex<Option<String>>>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl App {
    /// Builds the app on an egui context: the eframe window's in `main`, the
    /// test harness's in the headless tests (ADR-0008).
    pub fn with_context(
        ctx: &egui::Context,
        open_on_start: Vec<PathBuf>,
        tab: Option<Tab>,
    ) -> Self {
        Self::with_prefs_path(ctx, open_on_start, tab, Prefs::default_path())
    }

    /// `with_context` with the preferences file given explicitly (`None` = do
    /// not persist).
    pub fn with_prefs_path(
        ctx: &egui::Context,
        open_on_start: Vec<PathBuf>,
        tab: Option<Tab>,
        prefs_path: Option<PathBuf>,
    ) -> Self {
        let prefs = prefs_path.as_deref().map(Prefs::load).unwrap_or_default();
        let ctx = ctx.clone();
        let worker = Worker::spawn(move || ctx.request_repaint());
        let mut app = App {
            worker,
            slots: HashMap::from([(Slot::A, SlotState::Empty), (Slot::B, SlotState::Empty)]),
            cache: HashMap::new(),
            tab: tab.unwrap_or(Tab::LogViewer),
            tools: Tools::default(),
            touch_mode: prefs.touch_mode,
            touch_applied: false,
            status: "Open a blackbox log (File → Open, or drop it here)".into(),
            viewer: views::ViewerState::default(),
            spectrum: views::SpectrumState::default(),
            slider_state: views::SliderState::default(),
            overlay: views::OverlayState::default(),
            prefs,
            prefs_path,
            theme_applied: None,
            central_rect: egui::Rect::NOTHING,
            export_message: Arc::new(std::sync::Mutex::new(None)),
        };
        for (i, p) in open_on_start.into_iter().take(2).enumerate() {
            app.open(if i == 0 { Slot::A } else { Slot::B }, p);
        }
        app
    }

    /// Writes the preferences to `prefs_path`; a failure goes to the status bar.
    pub fn save_prefs(&mut self) {
        let Some(path) = &self.prefs_path else { return };
        if let Err(e) = self.prefs.save(path) {
            self.status = format!("cannot save {}: {e}", path.display());
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.theme_applied != Some(self.prefs.theme) {
            self.theme_applied = Some(self.prefs.theme);
            ctx.set_visuals(self.prefs.visuals());
        }
    }

    pub fn open(&mut self, slot: Slot, path: PathBuf) {
        self.status = format!("Loading {} into slot {}…", path.display(), slot.name());
        self.slots.insert(slot, SlotState::Loading(path.clone()));
        self.cache.retain(|k, _| k.slot != slot);
        self.worker.submit(Job::Load { slot, path });
    }

    pub fn loaded(&self, slot: Slot) -> Option<&Loaded> {
        match self.slots.get(&slot) {
            Some(SlotState::Ready(l)) => Some(l),
            _ => None,
        }
    }

    pub fn loaded_mut(&mut self, slot: Slot) -> Option<&mut Loaded> {
        match self.slots.get_mut(&slot) {
            Some(SlotState::Ready(l)) => Some(l),
            _ => None,
        }
    }

    /// Result of an analysis for a slot, scheduling it on first request.
    pub fn result(&mut self, slot: Slot, kind: Kind) -> Option<Result<Arc<Output>, String>> {
        let loaded = self.loaded(slot)?;
        let session = loaded.current()?.clone();
        let key = JobKey::new(slot, loaded.session, &kind, loaded.epoch);
        let epoch = loaded.epoch;
        match self.cache.get(&key) {
            Some(Cached::Ready(o)) => Some(Ok(o.clone())),
            Some(Cached::Failed(e)) => Some(Err(e.clone())),
            Some(Cached::Pending) => None,
            None => {
                self.cache.insert(key.clone(), Cached::Pending);
                self.worker.submit(Job::Analyze {
                    key,
                    session,
                    kind,
                    epoch,
                });
                None
            }
        }
    }

    fn drain_worker(&mut self) {
        for done in self.worker.poll() {
            match done {
                Done::Loaded { slot, path, result } => match result {
                    Ok(file) => {
                        let sessions: Vec<Arc<FlightSession>> =
                            file.sessions.iter().cloned().map(Arc::new).collect();
                        // pick the longest session by default
                        let session = sessions
                            .iter()
                            .enumerate()
                            .max_by_key(|(_, s)| s.len())
                            .map_or(0, |(i, _)| i);
                        self.status = format!(
                            "{}: {} session(s), {} samples in session {}",
                            path.display(),
                            sessions.len(),
                            sessions.get(session).map_or(0, |s| s.len()),
                            session
                        );
                        self.slots.insert(
                            slot,
                            SlotState::Ready(Loaded {
                                path,
                                sessions,
                                session,
                                epoch: None,
                            }),
                        );
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", path.display());
                        self.slots.insert(slot, SlotState::Failed(path, e));
                    }
                },
                Done::Analyzed { key, result } => {
                    let entry = match result {
                        Ok(o) => Cached::Ready(Arc::from(o)),
                        Err(e) => Cached::Failed(e),
                    };
                    self.cache.insert(key, entry);
                }
            }
        }
    }

    fn apply_touch_mode(&mut self, ctx: &egui::Context) {
        if self.touch_mode == self.touch_applied {
            return;
        }
        self.touch_applied = self.touch_mode;
        let touch = self.touch_mode;
        ctx.set_zoom_factor(if touch { 1.25 } else { 1.0 });
        ctx.all_styles_mut(|s| {
            s.spacing.interact_size.y = if touch { 40.0 } else { 20.0 };
            s.spacing.button_padding = if touch {
                egui::vec2(12.0, 8.0)
            } else {
                egui::vec2(6.0, 2.0)
            };
            s.spacing.item_spacing = if touch {
                egui::vec2(12.0, 10.0)
            } else {
                egui::vec2(8.0, 4.0)
            };
        });
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        let n = dropped.len().min(2);
        for (i, p) in dropped.into_iter().take(2).enumerate() {
            let a_free = self.loaded(Slot::A).is_none();
            let b_free = self.loaded(Slot::B).is_none();
            // one file: fill the free slot (A first); two files: A then B
            let slot = if i == 0 && (a_free || !b_free || n == 2) {
                Slot::A
            } else {
                Slot::B
            };
            self.open(slot, p);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("OpenPID");
                ui.separator();
                for slot in [Slot::A, Slot::B] {
                    if ui.button(format!("Open {}…", slot.name())).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter(
                                "Flight logs",
                                &[
                                    "bbl", "bfl", "txt", "csv", "bin", "BBL", "BFL", "TXT", "CSV",
                                    "BIN",
                                ],
                            )
                            .pick_file()
                    {
                        self.open(slot, p);
                    }
                }
                ui.separator();
                for tab in Tab::ALL {
                    ui.selectable_value(&mut self.tab, tab, tab.title());
                }
                ui.separator();
                ui.checkbox(&mut self.touch_mode, "Touch mode");
                ui.separator();
                self.export_menu(ui);
            });
        });
    }

    /// "Export" menu: the tab area as PNG, a slot's result as JSON.
    fn export_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("Export", |ui| {
            if ui.button("Plot as PNG…").clicked() {
                ui.close();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .set_file_name(self.export_file_name(Slot::A, "png"))
                    .save_file()
                {
                    self.export_png(&ctx, path);
                }
            }
            for slot in [Slot::A, Slot::B] {
                let ready = self.current_result(slot).is_some();
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(format!("{} result as JSON…", slot.name())),
                    )
                    .clicked()
                {
                    ui.close();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .set_file_name(self.export_file_name(slot, "json"))
                        .save_file()
                    {
                        self.status = match self.export_json(slot, &path) {
                            Ok(()) => format!("wrote {}", path.display()),
                            Err(e) => format!("export failed: {e}"),
                        };
                    }
                }
            }
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                let pending = self
                    .cache
                    .values()
                    .filter(|c| matches!(c, Cached::Pending))
                    .count();
                if pending > 0 {
                    ui.spinner();
                    ui.label(format!("{pending} analysis job(s) running"));
                }
            });
        });
    }
}

impl App {
    /// One frame of the whole UI: input handling, worker results, panels and the active tab.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.touch_mode && ctx.input(egui::InputState::any_touches) {
            self.touch_mode = true;
        }
        self.apply_theme(&ctx);
        self.apply_touch_mode(&ctx);
        self.drain_worker();
        self.handle_drops(&ctx);
        self.top_bar(ui);
        self.status_bar(ui);
        if let Some(msg) = self.export_message.lock().ok().and_then(|mut m| m.take()) {
            self.status = msg;
        }
        let central = egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::LogViewer => views::log_viewer(self, ui),
            Tab::Step => views::step(self, ui),
            Tab::Spectrum => views::spectrum(self, ui),
            Tab::ThrottleHeatmap => views::throttle_heatmap(self, ui),
            Tab::TimeHeatmap => views::time_heatmap(self, ui),
            Tab::FilterSim => views::filter_sim(self, ui),
            Tab::MotorNoise => views::motor_noise(self, ui),
            Tab::Sliders => views::sliders(self, ui),
            Tab::Diagnostics => views::diagnostics(self, ui),
            Tab::Settings => views::settings(self, ui),
        });
        self.central_rect = central.response.rect;
    }
}
