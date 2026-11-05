//! Project files (`*.openpid.json`): which logs are open, their session and
//! epoch, the active tab and every tool parameter, so a view can be saved
//! and reopened (by the GUI, and by `openpid run --project`).
//!
//! The schema is versioned and every field has a default, so an older or
//! newer `OpenPID` reads what it understands and ignores the rest.

use crate::recommend::RecommendParams;
use crate::{
    FilterSimParams, MotorNoiseParams, PsdParams, StepParams, StftParams, ThrottleHeatmapParams,
};
use openpid_core::analysis::Epoch;
use openpid_core::{Axis, ChannelId, GyroStage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Current schema version.
pub const PROJECT_VERSION: u32 = 2;
/// File name suffix that marks a project file.
pub const PROJECT_SUFFIX: &str = ".openpid.json";

/// One open log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSlot {
    /// Log path, relative to the project file when the log sits beside or
    /// below it, absolute otherwise.
    pub path: PathBuf,
    /// Session index within the file.
    pub session: usize,
    /// Selected epoch.
    pub epoch: Option<Epoch>,
    /// Slot letter as an index (0 = A); `None` (version 1 files) means "by
    /// position in the list".
    pub id: Option<u8>,
    /// Drawn on the plots (a hidden slot stays open but out of the way).
    pub visible: bool,
}

impl Default for ProjectSlot {
    fn default() -> Self {
        ProjectSlot {
            path: PathBuf::new(),
            session: 0,
            epoch: None,
            id: None,
            visible: true,
        }
    }
}

/// Everything a view needs to come back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// Schema version (see [`PROJECT_VERSION`]).
    pub version: u32,
    /// Open logs in slot-bar order (version 2: with their letter and visibility).
    pub slots: Vec<ProjectSlot>,
    /// Active tab by its short name (`log`, `step`, `spectrum`, …).
    pub tab: String,
    /// Touch mode.
    pub touch_mode: bool,
    /// Axis of the step response, spectra and Filter Sim.
    pub axis: Axis,
    /// Step response parameters.
    pub step: StepParams,
    /// Spectrum parameters.
    pub psd: PsdParams,
    /// Freq × throttle parameters.
    pub throttle: ThrottleHeatmapParams,
    /// Freq × time parameters.
    pub stft: StftParams,
    /// Filter Sim parameters and what-if overrides.
    pub filter_sim: FilterSimParams,
    /// Motor noise parameters.
    pub motor_noise: MotorNoiseParams,
    /// Advice (recommender) thresholds.
    pub recommend: RecommendParams,
    /// Channel shown on the spectrum tab.
    pub spectrum_channel: ChannelId,
    /// Channel shown on the heatmap tabs.
    pub heatmap_channel: ChannelId,
    /// Spectrum in dB.
    pub psd_db: bool,
    /// Heatmap colour maximum (0 = automatic).
    pub heatmap_max: f64,
    /// Throttle bins with fewer segments are greyed out.
    pub min_segments: usize,
    /// Channels shown in the log viewer.
    pub viewer_channels: Vec<ChannelId>,
}

impl Default for Project {
    fn default() -> Self {
        let gyro = ChannelId::Gyro {
            axis: Axis::Roll,
            stage: GyroStage::Filtered,
        };
        Project {
            version: PROJECT_VERSION,
            slots: Vec::new(),
            tab: "log".into(),
            touch_mode: false,
            axis: Axis::Roll,
            step: StepParams::default(),
            psd: PsdParams::default(),
            throttle: ThrottleHeatmapParams::default(),
            stft: StftParams::default(),
            filter_sim: FilterSimParams::default(),
            motor_noise: MotorNoiseParams::default(),
            recommend: RecommendParams::default(),
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

impl Project {
    /// Whether `path` names a project file.
    pub fn is_project_path(path: &Path) -> bool {
        path.to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(PROJECT_SUFFIX)
    }

    /// Read and parse a project file.
    ///
    /// # Errors
    /// The file cannot be read, is not JSON, or comes from a newer schema.
    pub fn load(path: &Path) -> Result<Project, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let p: Project = serde_json::from_str(&text)
            .map_err(|e| format!("{}: not a project file: {e}", path.display()))?;
        if p.version > PROJECT_VERSION {
            return Err(format!(
                "{}: project version {} is newer than this OpenPID ({PROJECT_VERSION})",
                path.display(),
                p.version
            ));
        }
        Ok(p)
    }

    /// Write the project as pretty JSON.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// A slot's log path, resolved against the project file's directory.
    pub fn resolve(project_path: &Path, log: &Path) -> PathBuf {
        if log.is_absolute() {
            return log.to_path_buf();
        }
        project_path
            .parent()
            .map_or_else(|| log.to_path_buf(), |dir| dir.join(log))
    }

    /// A log path as stored: relative to the project file's directory when
    /// the log lies inside it, absolute otherwise.
    pub fn relativise(project_path: &Path, log: &Path) -> PathBuf {
        match project_path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => log
                .strip_prefix(dir)
                .map_or_else(|_| log.to_path_buf(), Path::to_path_buf),
            _ => log.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_defaults_and_paths() {
        let dir = std::env::temp_dir().join(format!("openpid-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("flight.openpid.json");
        assert!(Project::is_project_path(&file));
        assert!(Project::is_project_path(Path::new("X.OPENPID.JSON")));
        assert!(!Project::is_project_path(Path::new("flight.json")));
        let mut p = Project::default();
        p.slots.push(ProjectSlot {
            path: Project::relativise(&file, &dir.join("logs").join("a.bbl")),
            session: 1,
            epoch: Some(Epoch {
                start_s: 1.0,
                end_s: 2.0,
            }),
            id: None,
            visible: true,
        });
        p.step.cutoff_hz = 33.0;
        p.tab = "step".into();
        assert_eq!(p.slots[0].path, PathBuf::from("logs").join("a.bbl"));
        assert_eq!(
            Project::resolve(&file, &p.slots[0].path),
            dir.join("logs").join("a.bbl")
        );
        // an absolute path outside the project directory stays absolute (on
        // Windows a bare "/x" is not absolute, so build one from the temp dir)
        let outside = std::env::temp_dir().join("elsewhere").join("b.bbl");
        assert!(outside.is_absolute());
        assert_eq!(Project::relativise(&file, &outside), outside);
        assert_eq!(Project::resolve(&file, &outside), outside);
        assert_eq!(
            Project::resolve(Path::new("p.openpid.json"), Path::new("a.bbl")),
            PathBuf::from("a.bbl")
        );
        p.save(&file).unwrap();
        assert_eq!(Project::load(&file).unwrap(), p);
        // unknown and missing fields are tolerated, newer versions refused
        std::fs::write(&file, r#"{"version":1,"tab":"spectrum","future":true}"#).unwrap();
        let loaded = Project::load(&file).unwrap();
        assert_eq!(loaded.tab, "spectrum");
        assert_eq!(loaded.step, StepParams::default());
        std::fs::write(&file, r#"{"version":99}"#).unwrap();
        assert!(Project::load(&file).unwrap_err().contains("newer"));
        std::fs::write(&file, "nope").unwrap();
        assert!(
            Project::load(&file)
                .unwrap_err()
                .contains("not a project file")
        );
        assert!(Project::load(&dir.join("missing.openpid.json")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
