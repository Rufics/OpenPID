//! Application state and the top-level frame.

use crate::i18n::tr;
use crate::prefs::{Prefs, Theme};
use crate::shortcuts::{self, Action};
use crate::surface;
use crate::views;
use crate::worker::{Done, Job, JobKey, Kind, Output, Slot, Worker};
use eframe::egui;
use openpid_analysis::{
    Epoch, FilterSimParams, Fitness, Project, ProjectSlot, PsdParams, RecommendParams, StepParams,
    StftParams, ThrottleHeatmapParams,
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
    /// Freq × throttle × amplitude as an orbiting 3D surface.
    Surface,
    FilterSim,
    /// Rule-based proposals with their evidence.
    Advice,
    Sliders,
    Diagnostics,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 11] = [
        Tab::LogViewer,
        Tab::Step,
        Tab::Spectrum,
        Tab::ThrottleHeatmap,
        Tab::TimeHeatmap,
        Tab::Surface,
        Tab::FilterSim,
        Tab::Advice,
        Tab::Sliders,
        Tab::Diagnostics,
        Tab::Settings,
    ];

    /// The tour anchor of the tab button, for the tabs the tour visits.
    pub fn anchor(self) -> Option<&'static str> {
        Some(match self {
            Tab::LogViewer => "tab-log",
            Tab::Spectrum => "tab-spectrum",
            Tab::ThrottleHeatmap => "tab-throttle",
            Tab::FilterSim => "tab-filter",
            Tab::Step => "tab-step",
            Tab::Advice => "tab-advice",
            _ => return None,
        })
    }

    pub fn from_name(name: &str) -> Option<Tab> {
        Some(match name {
            "log" => Tab::LogViewer,
            "step" => Tab::Step,
            "spectrum" => Tab::Spectrum,
            "throttle" => Tab::ThrottleHeatmap,
            "time" => Tab::TimeHeatmap,
            "surface" => Tab::Surface,
            "filter" => Tab::FilterSim,
            "advice" => Tab::Advice,
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
            Tab::Surface => "surface",
            Tab::FilterSim => "filter",
            Tab::Advice => "advice",
            Tab::Sliders => "sliders",
            Tab::Diagnostics => "diagnostics",
            Tab::Settings => "settings",
        }
    }

    /// The tab's label in the top bar: the title, shortened where it is
    /// long so the bar stays one line (docs/design/app-shell).
    pub fn bar_title(self) -> &'static str {
        match self {
            Tab::LogViewer => tr("Log"),
            Tab::Surface => tr("3D"),
            Tab::Sliders => tr("Sliders"),
            t => t.title(),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::LogViewer => tr("Log viewer"),
            Tab::Step => tr("Step response"),
            Tab::Spectrum => tr("Spectrum"),
            Tab::ThrottleHeatmap => tr("Freq × throttle"),
            Tab::TimeHeatmap => tr("Freq × time"),
            Tab::Surface => tr("Surface 3D"),
            Tab::FilterSim => tr("Filter Sim"),
            Tab::Advice => tr("Advice"),
            Tab::Sliders => tr("PID sliders"),
            Tab::Diagnostics => tr("Diagnostics"),
            Tab::Settings => tr("Settings"),
        }
    }
}

/// A loaded file in a slot.
#[derive(Debug)]
pub struct Loaded {
    pub path: PathBuf,
    pub sessions: Vec<Arc<FlightSession>>,
    pub session: usize,
    pub epoch: Option<Epoch>,
    /// The epoch was set by the automatic trim, not by the user.
    pub epoch_auto: bool,
    /// Fitness of every session, computed once when the file was loaded.
    pub fitness: Vec<Fitness>,
}

impl Loaded {
    pub fn current(&self) -> Option<&Arc<FlightSession>> {
        self.sessions.get(self.session)
    }

    /// Set the epoch; `auto` marks it as the automatic trim.
    pub fn set_epoch(&mut self, epoch: Option<Epoch>, auto: bool) {
        self.epoch = epoch;
        self.epoch_auto = auto && epoch.is_some();
    }

    /// The automatic trim of the current session (take-off and landing cut),
    /// or the whole log when the session is too short to trim.
    pub fn apply_auto_epoch(&mut self) {
        let e = self
            .current()
            .and_then(|s| openpid_analysis::epoch::auto(s));
        self.set_epoch(e, true);
    }

    /// Fitness of the selected session.
    pub fn fitness(&self) -> Option<&Fitness> {
        self.fitness.get(self.session)
    }
}

#[derive(Debug)]
pub enum SlotState {
    Empty,
    Loading(PathBuf),
    Failed(PathBuf, String),
    Ready(Loaded),
}

/// One entry of the slot bar: a slot, what it holds and whether it is drawn.
#[derive(Debug)]
pub struct SlotEntry {
    pub id: Slot,
    pub state: SlotState,
    /// Drawn on the plots; a hidden slot keeps its log and results.
    pub visible: bool,
}

impl SlotEntry {
    pub fn empty(id: Slot) -> Self {
        SlotEntry {
            id,
            state: SlotState::Empty,
            visible: true,
        }
    }
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
    /// Advice tab thresholds.
    pub recommend: RecommendParams,
    pub spectrum_channel: ChannelId,
    pub heatmap_channel: ChannelId,
    pub psd_db: bool,
    pub heatmap_max: f64,
    /// Heatmap panels share one colour scale (docs/design/colour-scale, R5).
    pub heatmap_link: bool,
    /// Throttle bins with fewer segments than this are greyed out on the freq × throttle heatmap.
    pub min_segments: usize,
    /// Log viewer: channels shown besides the quantities ("Other channels").
    pub viewer_channels: Vec<ChannelId>,
    /// Log viewer display options (docs/design/log-tab).
    pub log_display: crate::log_tab::LogDisplay,
    /// The one axis the axis-row tabs show at full height; `None` = all
    /// three (docs/design/axis-focus).
    pub axis_focus: Option<Axis>,
    /// Step tab display options (docs/design/axis-layout).
    pub step_display: crate::axis_layout::StepDisplay,
    /// Spectrum tab display options (docs/design/spectrum-tab).
    pub spectrum_display: crate::spectrum_tab::SpectrumDisplay,
    /// Freq × throttle display options (docs/design/throttle-tab).
    pub throttle_display: crate::throttle_tab::ThrottleDisplay,
    /// Freq × time display options (docs/design/time-tab).
    pub time_display: crate::time_tab::TimeDisplay,
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
            recommend: RecommendParams::default(),
            spectrum_channel: gyro.clone(),
            heatmap_channel: gyro.clone(),
            psd_db: true,
            heatmap_max: 0.0,
            heatmap_link: true,
            min_segments: 3,
            viewer_channels: Vec::new(),
            log_display: crate::log_tab::LogDisplay::default(),
            axis_focus: None,
            step_display: crate::axis_layout::StepDisplay::default(),
            spectrum_display: crate::spectrum_tab::SpectrumDisplay::default(),
            throttle_display: crate::throttle_tab::ThrottleDisplay::default(),
            time_display: crate::time_tab::TimeDisplay::default(),
        }
    }
}

pub struct App {
    pub worker: Worker,
    /// Open logs in slot-bar order (docs/design/multi-slot, R2).
    pub slots: Vec<SlotEntry>,
    /// The log the timeline dock shows (docs/design/app-shell, R4); the
    /// last one opened unless a chip was clicked since.
    pub dock: Option<Slot>,
    pub cache: HashMap<JobKey, Cached>,
    /// Heatmap grids rasterised once per result and colour scale (DESIGN §7.2),
    /// keyed by the result's address and the scale.
    pub heatmap_textures: HashMap<u64, egui::TextureHandle>,
    /// Colour-scale maximum drawn for every heatmap panel last frame.
    pub colour_scales: Vec<(Slot, f64)>,
    /// Heatmap cell under the pointer last frame: slot, x, y, value.
    pub heatmap_hover: Option<(Slot, f64, f64, f64)>,
    /// Freq × throttle view state (the throttle range last applied).
    pub throttle_view: crate::throttle_tab::ThrottleView,
    /// Freq × time view state.
    pub time_view: crate::time_tab::TimeView,
    pub tab: Tab,
    pub tools: Tools,
    pub touch_mode: bool,
    pub touch_applied: bool,
    pub status: String,
    pub viewer: views::ViewerState,
    pub spectrum: views::SpectrumState,
    pub slider_state: crate::sliders_tab::SliderState,
    /// RPM / notch overlay on the freq × throttle heatmap.
    pub overlay: views::OverlayState,
    /// Camera and hover state of the 3D surface tab.
    pub surface: surface::SurfaceState,
    /// Playback state of the freq × time tab.
    pub player: views::PlayerState,
    /// Timeline strips under the slot rows: drag state, rectangles, envelopes.
    pub strip: views::StripState,
    /// What the Step tab drew last frame (band, advice line; tests).
    pub step_view: views::StepViewState,
    /// The first-run tour.
    pub tour: crate::tour::Tour,
    /// Start the tour on the first log of a fresh install (off in most tests).
    pub auto_tour: bool,
    /// Rectangles of the widgets the tour points at, registered every frame.
    pub anchors: HashMap<&'static str, egui::Rect>,
    /// The plots of the current frame by name (`probe.rs`).
    pub probes: crate::probe::Probes,
    /// Persisted preferences (Settings tab).
    pub prefs: Prefs,
    /// Where `prefs` are saved; `None` = no config directory, keep them for the session.
    pub prefs_path: Option<PathBuf>,
    /// Theme the egui visuals currently follow.
    theme_applied: Option<Theme>,
    /// Session and epoch to apply to a slot once its file has loaded (project files).
    pending: HashMap<Slot, (Option<usize>, Option<Epoch>)>,
    /// Screen rectangle of the tab area, for the PNG export.
    pub central_rect: egui::Rect,
    /// Outcome of the last PNG export, filled by the screenshot callback.
    pub export_message: Arc<std::sync::Mutex<Option<String>>>,
    /// The "Keyboard shortcuts" window is open.
    pub show_help: bool,
    /// Native file dialogs run; `false` in headless tests, where `dialog` is
    /// left in place for inspection.
    pub dialogs: bool,
    /// A file dialog asked for this frame by a button or a shortcut.
    pub dialog: Option<Dialog>,
    /// The last shortcut action performed (tests).
    pub last_action: Option<Action>,
}

/// A native file dialog, run once at the end of the frame (`App::run_dialog`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// Pick a log for the slot.
    OpenLog(Slot),
    OpenProject,
    SaveProject,
    ExportPng,
    /// Save the slot's current result as JSON.
    ExportJson(Slot),
}

const LOG_EXTENSIONS: &[&str] = &[
    "bbl", "bfl", "txt", "csv", "bin", "BBL", "BFL", "TXT", "CSV", "BIN",
];

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
        let mut tools = Tools::default();
        tools.recommend.targets = prefs.targets;
        let mut app = App {
            worker,
            slots: vec![SlotEntry::empty(Slot::A), SlotEntry::empty(Slot::B)],
            dock: None,
            cache: HashMap::new(),
            heatmap_textures: HashMap::new(),
            colour_scales: Vec::new(),
            heatmap_hover: None,
            throttle_view: crate::throttle_tab::ThrottleView::default(),
            time_view: crate::time_tab::TimeView::default(),
            tab: tab.unwrap_or(Tab::LogViewer),
            tools,
            touch_mode: prefs.touch_mode,
            touch_applied: false,
            status: "Open a blackbox log (Open log…, Open sample log, or drop it here)".into(),
            viewer: views::ViewerState::default(),
            spectrum: views::SpectrumState::default(),
            slider_state: crate::sliders_tab::SliderState::default(),
            overlay: views::OverlayState::default(),
            surface: surface::SurfaceState::default(),
            player: views::PlayerState::default(),
            strip: views::StripState::default(),
            step_view: views::StepViewState::default(),
            tour: crate::tour::Tour::default(),
            auto_tour: true,
            anchors: HashMap::new(),
            probes: crate::probe::Probes::default(),
            prefs,
            prefs_path,
            theme_applied: None,
            pending: HashMap::new(),
            central_rect: egui::Rect::NOTHING,
            export_message: Arc::new(std::sync::Mutex::new(None)),
            show_help: false,
            dialogs: true,
            dialog: None,
            last_action: None,
        };
        // OPENPID_TOUR_STEP=n starts on tour step n (screenshots, demos)
        if let Some(step) = std::env::var("OPENPID_TOUR_STEP")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| (1..=crate::tour::STEPS.len()).contains(n))
        {
            app.start_tour();
            app.tour.step = step - 1;
        }
        // a project file on the command line replaces the log list
        if let Some(project) = open_on_start.iter().find(|p| Project::is_project_path(p)) {
            app.open_project(project.clone());
        } else {
            for p in open_on_start.into_iter().take(Slot::MAX) {
                if let Some(slot) = app.free_slot() {
                    app.open(slot, p);
                }
            }
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
        crate::theme::set_dark(self.prefs.theme == Theme::Dark);
        crate::colormap::set_scheme(self.prefs.plot_colours);
        if self.theme_applied != Some(self.prefs.theme) {
            self.theme_applied = Some(self.prefs.theme);
            ctx.set_visuals(self.prefs.visuals());
            ctx.all_styles_mut(crate::theme::style);
        }
    }

    pub fn open(&mut self, slot: Slot, path: PathBuf) {
        self.pending.remove(&slot);
        self.status = format!("Loading {} into slot {}…", path.display(), slot.name());
        self.set_state(slot, SlotState::Loading(path.clone()));
        self.cache.retain(|k, _| k.slot != slot);
        self.worker.submit(Job::Load { slot, path });
        self.dock = Some(slot);
    }

    /// The slot the timeline dock shows: the chosen one while it holds a
    /// log (or is loading one), else the first that does.
    pub fn dock_slot(&self) -> Option<Slot> {
        let used = |e: &&SlotEntry| !matches!(e.state, SlotState::Empty);
        self.dock
            .and_then(|d| self.slots.iter().filter(used).find(|e| e.id == d))
            .or_else(|| self.slots.iter().find(used))
            .map(|e| e.id)
    }

    // ---------------------------------------------------------------- slot bar

    /// Every slot in bar order, loaded or not.
    pub fn slot_ids(&self) -> Vec<Slot> {
        self.slots.iter().map(|e| e.id).collect()
    }

    /// Slots with a log in them, in bar order.
    pub fn loaded_slots(&self) -> Vec<Slot> {
        self.slots
            .iter()
            .filter(|e| matches!(e.state, SlotState::Ready(_)))
            .map(|e| e.id)
            .collect()
    }

    /// Loaded slots that are drawn (not hidden), in bar order: what the
    /// analysis tabs iterate.
    pub fn visible_slots(&self) -> Vec<Slot> {
        self.slots
            .iter()
            .filter(|e| e.visible && matches!(e.state, SlotState::Ready(_)))
            .map(|e| e.id)
            .collect()
    }

    /// The first loaded slot: the reference for channel pickers, the slider
    /// tool and export names.
    pub fn first_loaded(&self) -> Option<Slot> {
        self.loaded_slots().first().copied()
    }

    pub fn slot_entry(&self, slot: Slot) -> Option<&SlotEntry> {
        self.slots.iter().find(|e| e.id == slot)
    }

    pub fn slot_state(&self, slot: Slot) -> Option<&SlotState> {
        self.slot_entry(slot).map(|e| &e.state)
    }

    /// Replace a slot's state, creating the entry when the slot is new.
    pub fn set_state(&mut self, slot: Slot, state: SlotState) {
        match self.slots.iter_mut().find(|e| e.id == slot) {
            Some(e) => e.state = state,
            None => self.slots.push(SlotEntry {
                id: slot,
                state,
                visible: true,
            }),
        }
    }

    /// Add an empty slot with the lowest free letter; `None` at the limit.
    pub fn add_slot(&mut self) -> Option<Slot> {
        let id = (0..Slot::MAX as u8)
            .map(Slot)
            .find(|s| self.slot_entry(*s).is_none())?;
        self.slots.push(SlotEntry::empty(id));
        Some(id)
    }

    /// Where the next opened log goes: the first empty slot, else a new one.
    pub fn free_slot(&mut self) -> Option<Slot> {
        let empty = self
            .slots
            .iter()
            .find(|e| matches!(e.state, SlotState::Empty))
            .map(|e| e.id);
        if let Some(s) = empty {
            return Some(s);
        }
        let added = self.add_slot();
        if added.is_none() {
            self.status = format!("at most {} logs can be open at once", Slot::MAX);
        }
        added
    }

    /// Open the built-in synthetic log (the generator's default flight with
    /// an underdamped plant, so the step response overshoots and Advice has
    /// something to say) into the first empty slot. No file is involved.
    pub fn open_sample(&mut self) {
        use openpid_sim::signal::{Plant, SignalSpec};
        let Some(slot) = self.free_slot() else { return };
        let mut session = SignalSpec {
            duration_s: 30.0,
            plant: Plant {
                damping: Some(0.4),
                ..Plant::default()
            },
            ..SignalSpec::default()
        }
        .build();
        // Betaflight-like gains, so the Advice D rule has something to raise
        let gains = |p: f64, i: f64, d: f64| openpid_core::settings::PidGains {
            p,
            i,
            d,
            f: Some(120.0),
            d_min: None,
            d_max: None,
        };
        session.settings.pid = Some(openpid_core::settings::PerAxis {
            roll: gains(45.0, 80.0, 40.0),
            pitch: gains(47.0, 84.0, 46.0),
            yaw: gains(45.0, 80.0, 0.0),
        });
        let fitness = vec![openpid_analysis::check_session(&session)];
        self.pending.remove(&slot);
        self.cache.retain(|k, _| k.slot != slot);
        self.strip.forget(slot);
        let mut loaded = Loaded {
            path: PathBuf::from("sample-log (synthetic)"),
            sessions: vec![Arc::new(session)],
            session: 0,
            epoch: None,
            epoch_auto: false,
            fitness,
        };
        loaded.apply_auto_epoch();
        self.set_state(slot, SlotState::Ready(loaded));
        self.status = format!(
            "slot {}: the sample log (synthetic 30 s flight, roll steps, underdamped plant)",
            slot.name()
        );
        self.maybe_start_tour();
    }

    /// The first opened log of a fresh install starts the tour once.
    fn maybe_start_tour(&mut self) {
        if self.auto_tour && self.prefs.show_tour && !self.prefs.tour_done && !self.tour.active {
            self.start_tour();
        }
    }

    /// Every slot empty (nothing loaded, loading or failed): the start
    /// screen replaces the tab.
    pub fn start_screen_wanted(&self) -> bool {
        self.tab != Tab::Settings
            && self
                .slots
                .iter()
                .all(|e| matches!(e.state, SlotState::Empty))
    }

    /// Where a log to compare goes: the first empty slot after the last
    /// loaded one, else a new slot (`free_slot` when nothing is loaded).
    pub fn next_slot(&mut self) -> Option<Slot> {
        let Some(last) = self.loaded_slots().last().copied() else {
            return self.free_slot();
        };
        let after = self
            .slots
            .iter()
            .skip_while(|e| e.id != last)
            .find(|e| matches!(e.state, SlotState::Empty))
            .map(|e| e.id);
        if after.is_some() {
            return after;
        }
        let added = self.add_slot();
        if added.is_none() {
            self.status = format!("at most {} logs can be open at once", Slot::MAX);
        }
        added
    }

    /// Close a slot: its log, results and bar entry go; other letters stay.
    pub fn close_slot(&mut self, slot: Slot) {
        self.slots.retain(|e| e.id != slot);
        self.cache.retain(|k, _| k.slot != slot);
        self.pending.remove(&slot);
        self.strip.forget(slot);
        if self.slots.is_empty() {
            self.slots.push(SlotEntry::empty(Slot::A));
        }
    }

    pub fn set_visible(&mut self, slot: Slot, visible: bool) {
        if let Some(e) = self.slots.iter_mut().find(|e| e.id == slot) {
            e.visible = visible;
        }
    }

    /// Show only `slot`; a second solo on the same slot shows every slot again.
    pub fn solo(&mut self, slot: Slot) {
        let already = self
            .slots
            .iter()
            .all(|e| e.visible == (e.id == slot) || !matches!(e.state, SlotState::Ready(_)));
        for e in &mut self.slots {
            e.visible = already || e.id == slot;
        }
    }

    /// Load a log and, once it is in, select `session` and `epoch`.
    pub fn open_with(
        &mut self,
        slot: Slot,
        path: PathBuf,
        session: Option<usize>,
        epoch: Option<Epoch>,
    ) {
        self.open(slot, path);
        self.pending.insert(slot, (session, epoch));
    }

    /// The current view as a project, with log paths relative to `project_path`.
    pub fn project(&self, project_path: &std::path::Path) -> Project {
        let t = &self.tools;
        let mut slots = Vec::new();
        for e in &self.slots {
            let SlotState::Ready(l) = &e.state else {
                continue;
            };
            slots.push(ProjectSlot {
                path: Project::relativise(project_path, &l.path),
                session: l.session,
                epoch: l.epoch,
                id: Some(e.id.0),
                visible: e.visible,
            });
        }
        Project {
            slots,
            tab: self.tab.name().into(),
            touch_mode: self.touch_mode,
            axis: t.axis,
            step: t.step.clone(),
            psd: t.psd.clone(),
            throttle: t.throttle.clone(),
            stft: t.stft.clone(),
            filter_sim: t.filter_sim.clone(),
            recommend: t.recommend.clone(),
            spectrum_channel: t.spectrum_channel.clone(),
            heatmap_channel: t.heatmap_channel.clone(),
            psd_db: t.psd_db,
            heatmap_max: t.heatmap_max,
            heatmap_link: t.heatmap_link,
            min_segments: t.min_segments,
            viewer_channels: t.viewer_channels.clone(),
            ..Project::default()
        }
    }

    /// Save the current view as a project file.
    pub fn save_project(&mut self, path: &std::path::Path) {
        self.status = match self.project(path).save(path) {
            Ok(()) => format!("saved project {}", path.display()),
            Err(e) => e,
        };
    }

    /// Apply a project: tool parameters, tab, touch mode, and the logs with
    /// their session and epoch (loaded in the background).
    pub fn apply_project(&mut self, p: &Project, project_path: &std::path::Path) {
        self.tools = Tools {
            axis: p.axis,
            step: p.step.clone(),
            psd: p.psd.clone(),
            throttle: p.throttle.clone(),
            stft: p.stft.clone(),
            filter_sim: p.filter_sim.clone(),
            recommend: p.recommend.clone(),
            spectrum_channel: p.spectrum_channel.clone(),
            heatmap_channel: p.heatmap_channel.clone(),
            psd_db: p.psd_db,
            heatmap_max: p.heatmap_max,
            heatmap_link: p.heatmap_link,
            min_segments: p.min_segments,
            viewer_channels: p.viewer_channels.clone(),
            // display options are the viewer's, not the project's
            log_display: self.tools.log_display.clone(),
            axis_focus: self.tools.axis_focus,
            step_display: self.tools.step_display.clone(),
            spectrum_display: self.tools.spectrum_display.clone(),
            throttle_display: self.tools.throttle_display.clone(),
            time_display: self.tools.time_display.clone(),
        };
        self.tab = Tab::from_name(&p.tab).unwrap_or(Tab::LogViewer);
        self.touch_mode = p.touch_mode;
        self.cache.clear();
        self.slots.clear();
        // version 1 files have no letters: by position
        for (i, s) in p.slots.iter().take(Slot::MAX).enumerate() {
            let slot = Slot(s.id.unwrap_or(i as u8) % Slot::MAX as u8);
            self.set_state(slot, SlotState::Empty);
            self.set_visible(slot, s.visible);
            self.open_with(
                slot,
                Project::resolve(project_path, &s.path),
                Some(s.session),
                s.epoch,
            );
        }
        while self.slots.len() < 2 {
            if self.add_slot().is_none() {
                break;
            }
        }
        self.status = format!("opened project {}", project_path.display());
    }

    /// Load and apply a project file; errors go to the status bar.
    pub fn open_project(&mut self, path: PathBuf) {
        match Project::load(&path) {
            Ok(p) => self.apply_project(&p, &path),
            Err(e) => self.status = e,
        }
    }

    pub fn loaded(&self, slot: Slot) -> Option<&Loaded> {
        match self.slot_state(slot) {
            Some(SlotState::Ready(l)) => Some(l),
            _ => None,
        }
    }

    pub fn loaded_mut(&mut self, slot: Slot) -> Option<&mut Loaded> {
        match self.slots.iter_mut().find(|e| e.id == slot) {
            Some(SlotEntry {
                state: SlotState::Ready(l),
                ..
            }) => Some(l),
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
                Done::Loaded {
                    slot,
                    path,
                    result,
                    fitness,
                } => match result {
                    Ok(file) => {
                        let sessions: Vec<Arc<FlightSession>> =
                            file.sessions.iter().cloned().map(Arc::new).collect();
                        // pick the longest session by default, or what the project asked for
                        let from_project = self.pending.contains_key(&slot);
                        let (wanted, epoch) = self.pending.remove(&slot).unwrap_or((None, None));
                        let session = wanted.filter(|w| *w < sessions.len()).unwrap_or_else(|| {
                            sessions
                                .iter()
                                .enumerate()
                                .max_by_key(|(_, s)| s.len())
                                .map_or(0, |(i, _)| i)
                        });
                        self.status = format!(
                            "{}: {} session(s), {} samples in session {}",
                            path.display(),
                            sessions.len(),
                            sessions.get(session).map_or(0, |s| s.len()),
                            session
                        );
                        let mut loaded = Loaded {
                            path,
                            sessions,
                            session,
                            epoch,
                            epoch_auto: false,
                            fitness,
                        };
                        // a fresh log opens on its automatic trim; a project keeps its own epoch
                        if !from_project {
                            loaded.apply_auto_epoch();
                        }
                        self.strip.forget(slot);
                        self.prefs.push_recent(&loaded.path);
                        self.save_prefs();
                        self.set_state(slot, SlotState::Ready(loaded));
                        self.maybe_start_tour();
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", path.display());
                        self.set_state(slot, SlotState::Failed(path, e));
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
            // 44 pt is the usual minimum touch target
            s.spacing.interact_size.y = if touch { 44.0 } else { 20.0 };
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
        if let Some(p) = dropped.iter().find(|p| Project::is_project_path(p)) {
            self.open_project(p.clone());
            return;
        }
        // every dropped file gets a slot: the first empty one, else a new letter
        for p in dropped {
            let Some(slot) = self.free_slot() else { break };
            self.open(slot, p);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // docs/design/app-shell (R4): word mark, the log chips, the tabs as
        // one pill group, the menu; one line whatever the number of logs
        egui::Panel::top("top")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::theme::word_mark(ui);
                    ui.add_space(10.0);
                    if !self.start_screen_wanted() {
                        views::log_chips(self, ui);
                        ui.add_space(10.0);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.main_menu(ui);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("tabs")
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .show(ui, |ui| self.tab_group(ui, &ctx));
                        });
                    });
                });
            });
    }

    /// The tabs as one pill group; the active one carries an accent
    /// underline. Each tab is read out by its full title.
    fn tab_group(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dark = ui.visuals().dark_mode;
        egui::Frame::new()
            .fill(crate::theme::background(dark))
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.spacing_mut().button_padding = egui::vec2(7.0, 3.0);
                    ui.style_mut().override_font_id = Some(egui::FontId::proportional(13.0));
                    for (i, tab) in Tab::ALL.into_iter().enumerate() {
                        let selected = self.tab == tab;
                        let r = ui.add(egui::Button::selectable(selected, tab.bar_title()));
                        r.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::Role::Button,
                                true,
                                selected,
                                tab.title(),
                            )
                        });
                        let hover = if tab.bar_title() == tab.title() {
                            shortcuts::keys_of(ctx, Action::Tab(i))
                        } else {
                            format!(
                                "{}  {}",
                                tab.title(),
                                shortcuts::keys_of(ctx, Action::Tab(i))
                            )
                        };
                        let r = r.on_hover_text(hover);
                        if r.clicked() {
                            self.tab = tab;
                        }
                        if selected {
                            let y = r.rect.bottom() - 1.0;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(r.rect.left() + 6.0, y),
                                    egui::pos2(r.rect.right() - 6.0, y),
                                ],
                                egui::Stroke::new(2.0, crate::theme::ACCENT),
                            );
                        }
                        if let Some(id) = tab.anchor() {
                            self.anchors.insert(id, r.rect);
                        }
                    }
                });
            });
    }

    /// The "⋯" menu: opening a log, Project, Export, Help and touch mode.
    fn main_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let r = egui::containers::menu::MenuButton::new(egui::RichText::new("···").strong()).ui(
            ui,
            |ui| {
                if ui
                    .button(shortcuts::hint(
                        &ctx,
                        &format!("{}…", tr("Open log")),
                        Action::OpenLog,
                    ))
                    .clicked()
                {
                    ui.close();
                    if let Some(slot) = self.free_slot() {
                        self.dialog = Some(Dialog::OpenLog(slot));
                    }
                }
                ui.separator();
                self.project_menu(ui);
                self.export_menu(ui);
                self.help_menu(ui);
                ui.separator();
                ui.checkbox(&mut self.touch_mode, tr("Touch mode"))
                    .on_hover_text(shortcuts::keys_of(&ctx, Action::TouchMode));
            },
        );
        r.0.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr("Menu")));
        r.0.on_hover_text(tr("Menu"));
    }

    /// "Help" menu: the shortcuts window.
    fn help_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button(tr("Help"), |ui| {
            if ui
                .button(shortcuts::hint(
                    &ctx,
                    tr("Keyboard shortcuts"),
                    Action::Help,
                ))
                .clicked()
            {
                ui.close();
                self.show_help = true;
            }
            if ui.button(tr("Show tour")).clicked() {
                ui.close();
                self.start_tour();
            }
        });
    }

    /// Run the file dialog asked for this frame, if any (`dialogs` is off in tests).
    fn run_dialog(&mut self, ctx: &egui::Context) {
        if !self.dialogs {
            return;
        }
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match dialog {
            Dialog::OpenLog(slot) => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Flight logs", LOG_EXTENSIONS)
                    .pick_file()
                {
                    self.open(slot, p);
                }
            }
            Dialog::OpenProject => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OpenPID project", &["json"])
                    .pick_file()
                {
                    self.open_project(p);
                }
            }
            Dialog::SaveProject => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OpenPID project", &["json"])
                    .set_file_name(format!(
                        "{}.openpid.json",
                        self.export_file_name(self.first_loaded().unwrap_or(Slot::A), "")
                            .trim_end_matches('.')
                    ))
                    .save_file()
                {
                    self.save_project(&p);
                }
            }
            Dialog::ExportPng => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .set_file_name(
                        self.export_file_name(self.first_loaded().unwrap_or(Slot::A), "png"),
                    )
                    .save_file()
                {
                    self.export_png(ctx, path);
                }
            }
            Dialog::ExportJson(slot) => {
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
    }

    /// "Project" menu: open or save a `.openpid.json`.
    fn project_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button(tr("Project"), |ui| {
            if ui
                .button(tr("Open…"))
                .on_hover_text(shortcuts::keys_of(&ctx, Action::OpenProject))
                .clicked()
            {
                ui.close();
                self.dialog = Some(Dialog::OpenProject);
            }
            if ui
                .button(tr("Save as…"))
                .on_hover_text(shortcuts::keys_of(&ctx, Action::SaveProject))
                .clicked()
            {
                ui.close();
                self.dialog = Some(Dialog::SaveProject);
            }
        });
    }

    /// "Export" menu: the tab area as PNG, a slot's result as JSON.
    fn export_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button(tr("Export"), |ui| {
            if ui
                .button(tr("Plot as PNG…"))
                .on_hover_text(shortcuts::keys_of(&ctx, Action::ExportPng))
                .clicked()
            {
                ui.close();
                self.dialog = Some(Dialog::ExportPng);
            }
            for slot in self.loaded_slots() {
                let ready = self.current_result(slot).is_some();
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(format!("{} result as JSON…", slot.name())),
                    )
                    .clicked()
                {
                    ui.close();
                    self.dialog = Some(Dialog::ExportJson(slot));
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
        crate::i18n::set(self.prefs.lang);
        self.probes.clear();
        let ctx = ui.ctx().clone();
        if !self.touch_mode && ctx.input(egui::InputState::any_touches) {
            self.touch_mode = true;
        }
        self.apply_theme(&ctx);
        self.apply_touch_mode(&ctx);
        self.drain_worker();
        self.handle_drops(&ctx);
        self.tour_keys(&ctx);
        self.handle_shortcuts(&ctx);
        self.tour_tab();
        self.top_bar(ui);
        self.status_bar(ui);
        if self.tab != Tab::Settings && !self.start_screen_wanted() {
            egui::Panel::bottom("timeline-dock")
                .resizable(false)
                .exact_size(views::DOCK_H)
                .frame(
                    egui::Frame::side_top_panel(ui.style())
                        .inner_margin(egui::Margin::symmetric(12, 6)),
                )
                .show(ui, |ui| views::timeline_dock(self, ui));
        }
        if let Some(msg) = self.export_message.lock().ok().and_then(|mut m| m.take()) {
            self.status = msg;
        }
        let dark = ui.visuals().dark_mode;
        let central = egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style())
                    .fill(crate::theme::background(dark))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| match self.tab {
                _ if self.start_screen_wanted() => views::start_screen(self, ui),
                Tab::LogViewer => crate::log_tab::log_viewer(self, ui),
                Tab::Step => crate::axis_layout::step(self, ui),
                Tab::Spectrum => crate::spectrum_tab::spectrum(self, ui),
                Tab::ThrottleHeatmap => crate::throttle_tab::throttle(self, ui),
                Tab::TimeHeatmap => crate::time_tab::time(self, ui),
                Tab::Surface => surface::surface(self, ui),
                Tab::FilterSim => crate::filter_tab::filter_sim(self, ui),
                Tab::Advice => views::advice(self, ui),
                Tab::Sliders => crate::sliders_tab::sliders(self, ui),
                Tab::Diagnostics => views::diagnostics(self, ui),
                Tab::Settings => views::settings(self, ui),
            });
        self.central_rect = central.response.rect;
        self.help_window(&ctx);
        self.tour_overlay(&ctx);
        self.run_dialog(&ctx);
    }
}
