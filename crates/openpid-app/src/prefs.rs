//! User preferences that survive restarts: theme, language, touch mode, display units.
//!
//! Stored as JSON in the platform configuration directory
//! (`$XDG_CONFIG_HOME/openpid` or `~/.config/openpid` on Linux,
//! `~/Library/Application Support/openpid` on macOS, `%APPDATA%\openpid` on
//! Windows), or wherever `OPENPID_CONFIG_DIR` points. Unknown fields are
//! ignored and missing ones take their defaults, so older and newer versions
//! can share the file.

use crate::i18n::Lang;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Dark (default).
    #[default]
    Dark,
    /// Light.
    Light,
}

/// The colours of the plots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlotColours {
    /// As Blackbox Explorer (owner's choice, 2026-11-09): its 12-colour line
    /// palette and the black → red → white heatmap ramp.
    #[default]
    Explorer,
    /// Okabe–Ito lines and the viridis heatmap, readable with colour-vision
    /// deficiencies.
    ColourBlindSafe,
}

/// Unit for angular rates in the log viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateUnit {
    /// Degrees per second (what the firmware logs).
    #[default]
    DegPerS,
    /// Radians per second.
    RadPerS,
}

impl RateUnit {
    /// Multiplier from deg/s and the symbol to show.
    pub fn scale_deg_per_s(self) -> (f64, &'static str) {
        match self {
            RateUnit::DegPerS => (1.0, "deg/s"),
            RateUnit::RadPerS => (std::f64::consts::PI / 180.0, "rad/s"),
        }
    }
}

/// Unit for accelerations in the log viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccUnit {
    /// Standard gravity.
    #[default]
    G,
    /// Metres per second squared.
    MPerS2,
}

impl AccUnit {
    /// Multiplier from g and the symbol to show.
    pub fn scale_g(self) -> (f64, &'static str) {
        match self {
            AccUnit::G => (1.0, "g"),
            AccUnit::MPerS2 => (9.806_65, "m/s²"),
        }
    }
}

/// Everything the Settings tab edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Colour theme.
    pub theme: Theme,
    /// Interface language.
    pub lang: Lang,
    /// Start in touch mode (larger controls, touch gestures).
    pub touch_mode: bool,
    /// Angular-rate unit in the log viewer.
    pub rate_unit: RateUnit,
    /// Acceleration unit in the log viewer.
    pub acc_unit: AccUnit,
    /// Step-response targets (verdict thresholds; Settings → Targets).
    pub targets: openpid_analysis::StepTargets,
    /// The first-run tour was seen (or skipped).
    pub tour_done: bool,
    /// Start the tour on the first opened log of a fresh install.
    pub show_tour: bool,
    /// Recently opened logs, newest first (at most `MAX_RECENT`).
    pub recent: Vec<RecentFile>,
    /// The right-hand control panel of the analysis tabs is open
    /// (docs/design/axis-layout, R4).
    pub panel_open: bool,
    /// The colours of the plots (Settings → Appearance).
    pub plot_colours: PlotColours,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: Theme::default(),
            lang: Lang::default(),
            touch_mode: false,
            rate_unit: RateUnit::default(),
            acc_unit: AccUnit::default(),
            targets: openpid_analysis::StepTargets::default(),
            tour_done: false,
            show_tour: true,
            recent: Vec::new(),
            panel_open: true,
            plot_colours: PlotColours::default(),
        }
    }
}

/// How many recent files the start screen lists.
pub const MAX_RECENT: usize = 8;

/// A recently opened log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    /// Day it was last opened, `YYYY-MM-DD` (UTC).
    pub opened: String,
}

/// `YYYY-MM-DD` of a Unix time (days since the epoch, proleptic Gregorian).
pub fn ymd(unix_secs: u64) -> String {
    // Howard Hinnant's civil_from_days
    let days = (unix_secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

impl Prefs {
    /// Put `path` first in the recent list (dated today), dropping duplicates
    /// and anything beyond `MAX_RECENT`.
    pub fn push_recent(&mut self, path: &Path) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.recent.retain(|r| r.path != path);
        self.recent.insert(
            0,
            RecentFile {
                path: path.to_path_buf(),
                opened: ymd(now),
            },
        );
        self.recent.truncate(MAX_RECENT);
    }
}

impl Prefs {
    /// Default file: `<config dir>/openpid/prefs.json`.
    pub fn default_path() -> Option<PathBuf> {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self::path_from(
            var("OPENPID_CONFIG_DIR"),
            var("APPDATA"),
            var("HOME"),
            var("XDG_CONFIG_HOME"),
        )
    }

    /// `default_path` on explicit environment values (pure, for the tests).
    fn path_from(
        override_dir: Option<PathBuf>,
        appdata: Option<PathBuf>,
        home: Option<PathBuf>,
        xdg_config_home: Option<PathBuf>,
    ) -> Option<PathBuf> {
        if let Some(dir) = override_dir {
            return Some(dir.join("prefs.json"));
        }
        let base = if cfg!(target_os = "windows") {
            appdata
        } else if cfg!(target_os = "macos") {
            home.map(|h| h.join("Library/Application Support"))
        } else {
            xdg_config_home.or_else(|| home.map(|h| h.join(".config")))
        }?;
        Some(base.join("openpid").join("prefs.json"))
    }

    /// Load from `path`; a missing or unreadable file gives the defaults.
    pub fn load(path: &Path) -> Prefs {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write to `path`, creating the directory.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// egui visuals for the theme.
    pub fn visuals(&self) -> eframe::egui::Visuals {
        crate::theme::visuals(self.theme == Theme::Dark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_unknown_fields() {
        let dir = std::env::temp_dir().join(format!("openpid-prefs-{}", std::process::id()));
        let path = dir.join("nested").join("prefs.json");
        assert_eq!(
            Prefs::load(&path),
            Prefs::default(),
            "missing file = defaults"
        );
        let p = Prefs {
            theme: Theme::Light,
            lang: Lang::Ua,
            touch_mode: true,
            rate_unit: RateUnit::RadPerS,
            acc_unit: AccUnit::MPerS2,
            targets: openpid_analysis::TargetPreset::Racing.targets(),
            tour_done: true,
            show_tour: false,
            recent: vec![RecentFile {
                path: PathBuf::from("/logs/a.bbl"),
                opened: "2026-11-06".into(),
            }],
            panel_open: false,
            plot_colours: PlotColours::ColourBlindSafe,
        };
        p.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), p);
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(1_793_923_200), "2026-11-06");
        assert_eq!(ymd(951_782_400), "2000-02-29");
        let mut q = Prefs::default();
        assert!(q.show_tour && !q.tour_done);
        for i in 0..10 {
            q.push_recent(Path::new(&format!("/l/{i}.bbl")));
        }
        q.push_recent(Path::new("/l/3.bbl"));
        assert_eq!(q.recent.len(), MAX_RECENT);
        assert_eq!(q.recent[0].path, PathBuf::from("/l/3.bbl"));
        assert_eq!(
            q.recent
                .iter()
                .filter(|r| r.path.ends_with("3.bbl"))
                .count(),
            1
        );
        std::fs::write(&path, r#"{"theme":"light","future_field":42}"#).unwrap();
        let loaded = Prefs::load(&path);
        assert_eq!(loaded.theme, Theme::Light);
        assert_eq!(loaded.rate_unit, RateUnit::DegPerS);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default(), "garbage = defaults");
        assert!(!p.visuals().dark_mode);
        assert!(Prefs::default().visuals().dark_mode);
        assert_eq!(RateUnit::RadPerS.scale_deg_per_s().1, "rad/s");
        assert!((AccUnit::MPerS2.scale_g().0 - 9.806_65).abs() < 1e-12);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn default_path_honours_the_override() {
        let home = PathBuf::from("/home/pilot");
        let all = |o: Option<&str>| {
            Prefs::path_from(
                o.map(PathBuf::from),
                Some(home.join("appdata")),
                Some(home.clone()),
                Some(home.join("xdg")),
            )
        };
        assert_eq!(
            all(Some("/etc/openpid")),
            Some(PathBuf::from("/etc/openpid/prefs.json"))
        );
        let p = all(None).expect("a base directory exists");
        assert!(p.starts_with(&home), "{}", p.display());
        assert!(p.ends_with("openpid/prefs.json") || p.ends_with("openpid\\prefs.json"));
        assert_eq!(
            Prefs::path_from(None, None, None, None),
            None,
            "no home, no file"
        );
        // the real one only needs some home directory
        assert!(Prefs::default_path().is_none_or(|p| p.ends_with("prefs.json")));
    }
}
