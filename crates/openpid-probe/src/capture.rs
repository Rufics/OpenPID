//! The steps the capture script walks through for a round: which windows of
//! the reference tool to open, what to click and which pictures to save.
//!
//! The script is `tools/capture/capture.ps1`; it is carried inside every
//! pack, so a pack is all the owner needs. Positions are pixels of the
//! round 0 screenshots (`docs/calibration/REFERENCE-SCREENS.md`); the script
//! scales them to the window as it is.

use crate::catalogue::{Area, Probe};
use serde::Serialize;

/// The capture script, as committed.
const SCRIPT: &str = include_str!("../../../tools/capture/capture.ps1");

/// Title of the tool's main window.
const LOG_VIEWER: &str = "Log Viewer";
/// Title of its spectrum window.
const SPECTRUM: &str = "Spectral Analyzer";

/// The smoothing levels of the spectrum window with the row of each in the
/// open list, in the order they are captured: the default (medium) last, so
/// the tool is left as it was found.
const SMOOTHING: [(&str, u32); 5] = [
    ("low", 0),
    ("low-med", 1),
    ("med-high", 3),
    ("high", 4),
    ("medium", 2),
];

/// The frame of a reference screenshot: where the inside of its window is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frame {
    /// First column of the inside of the window.
    pub left: u32,
    /// First row below the title bar.
    pub top: u32,
    /// Width of the inside.
    pub width: u32,
    /// Height of the inside.
    pub height: u32,
}

/// The reference screenshots the positions are measured in.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frames {
    /// The main window, round 0.
    pub log: Frame,
    /// The spectrum window, round 0.
    pub spectrum: Frame,
}

/// One step of the script. Only the fields a step uses are written.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Step {
    /// What to do: `click`, `open_file`, `settle`, `shot`, `pause`, `windows`.
    pub action: &'static str,
    /// Part of the title of the window the step is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<&'static str>,
    /// What is clicked, for the log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The reference screenshot of `at`.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub frame: Option<&'static str>,
    /// Position in the reference screenshot, pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<(u32, u32)>,
    /// Remember what the plots look like before the click.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark: Option<bool>,
    /// Files for the "open" dialog, relative to the script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// Longest wait for the plots to change since the mark, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_s: Option<f64>,
    /// How long the plots must stay the same to count as drawn, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_s: Option<f64>,
    /// Longest wait for the window to come up, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_s: Option<u32>,
    /// Name of the picture, without the extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Length of a pause, milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u32>,
}

/// `steps.json` of a pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    /// What the run does, for the reader of the file.
    pub about: String,
    /// The reference screenshots.
    pub refs: Frames,
    /// The steps in order.
    pub steps: Vec<Step>,
}

fn click(window: &'static str, frame: &'static str, name: &str, at: (u32, u32)) -> Step {
    Step {
        action: "click",
        window: Some(window),
        name: Some(name.to_string()),
        frame: Some(frame),
        at: Some(at),
        ..Step::default()
    }
}

fn pause(ms: u32) -> Step {
    Step {
        action: "pause",
        ms: Some(ms),
        ..Step::default()
    }
}

fn settle(window: &'static str, change_s: Option<f64>, quiet_s: f64) -> Step {
    Step {
        action: "settle",
        window: Some(window),
        change_s,
        quiet_s: Some(quiet_s),
        ..Step::default()
    }
}

fn shot(window: &'static str, file: String) -> Step {
    Step {
        action: "shot",
        window: Some(window),
        file: Some(file),
        ..Step::default()
    }
}

/// Name of the picture of `probe` on the spectrum at a smoothing level.
pub fn spectrum_shot(round: u32, probe: &str, smoothing: &str) -> String {
    format!("r{round}-{probe}-spectrum-{smoothing}")
}

/// The plan of a round: load every log at once, then for every spectrum
/// probe the spectrum at each smoothing level.
pub fn plan(round: u32, probes: &[Probe]) -> Plan {
    let mut steps = Vec::new();
    // the main window: all logs through one "open" dialog
    steps.push(Step {
        mark: Some(true),
        ..click(LOG_VIEWER, "log", "Open File", (3468, 222))
    });
    steps.push(Step {
        action: "open_file",
        paths: Some(probes.iter().map(|p| format!("{}.bbl", p.id)).collect()),
        ..Step::default()
    });
    // decoding takes several seconds a log
    steps.push(settle(LOG_VIEWER, Some(900.0), 6.0));
    steps.push(Step {
        action: "windows",
        ..Step::default()
    });
    steps.push(shot(LOG_VIEWER, format!("r{round}-log-loaded")));

    steps.push(click(LOG_VIEWER, "log", "Spectral Analyzer", (3556, 706)));
    steps.push(Step {
        wait_s: Some(60),
        ..settle(SPECTRUM, None, 1.5)
    });
    steps.push(shot(SPECTRUM, format!("r{round}-spectrum-open")));
    let spectrum: Vec<&Probe> = probes.iter().filter(|p| p.area == Area::Spectrum).collect();
    for probe in spectrum {
        // the logs are listed in the order of their names, which is the
        // order of the catalogue; the legend of every picture names its log
        let row = probes.iter().position(|p| p.id == probe.id).unwrap_or(0) as u32;
        for (k, (level, item)) in SMOOTHING.into_iter().enumerate() {
            if k == 0 {
                // a click selects this log alone
                let y = 161 + (59 * row) / 2;
                steps.push(Step {
                    mark: Some(true),
                    ..click(
                        SPECTRUM,
                        "spectrum",
                        &format!("log {}", probe.id),
                        (3483, y),
                    )
                });
                steps.push(pause(250));
                steps.push(click(SPECTRUM, "spectrum", "smoothing list", (3641, 1314)));
            } else {
                steps.push(Step {
                    mark: Some(true),
                    ..click(SPECTRUM, "spectrum", "smoothing list", (3641, 1314))
                });
            }
            steps.push(pause(350));
            let name = format!("smoothing {level}");
            steps.push(click(
                SPECTRUM,
                "spectrum",
                &name,
                (3647, 1347 + (67 * item) / 2),
            ));
            steps.push(pause(200));
            steps.push(click(SPECTRUM, "spectrum", "Run", (3470, 1004)));
            steps.push(settle(SPECTRUM, Some(20.0), 1.2));
            steps.push(shot(SPECTRUM, spectrum_shot(round, probe.id, level)));
        }
    }
    steps.push(Step {
        action: "windows",
        ..Step::default()
    });
    Plan {
        about: format!(
            "Round {round}: every log through one open dialog, then the spectrum of each at \
             the five smoothing levels."
        ),
        refs: Frames {
            log: Frame {
                left: 1,
                top: 47,
                width: 3776,
                height: 2424,
            },
            spectrum: Frame {
                left: 1,
                top: 47,
                width: 3776,
                height: 2424,
            },
        },
        steps,
    }
}

/// Text with the line ends Windows tools expect.
fn crlf(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\n")
        .replace('\n', "\r\n")
        .into_bytes()
}

/// The files that make a pack runnable on the owner's machine: the script,
/// its launcher and a note on how to run it.
pub fn files(round: u32, shots: usize) -> Vec<(String, Vec<u8>)> {
    let launcher = "@echo off\n\
                    cd /d \"%~dp0\"\n\
                    powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0capture.ps1\" \
                    -Steps \"steps.json\"\n\
                    echo.\n\
                    pause\n";
    let note = format!(
        "Calibration round {round}\n\
         \n\
         1. Unpack everything into one folder.\n\
         2. Start the reference tool and wait for its main window (Log Viewer).\n   \
         Open nothing in it and leave the window as it comes up.\n\
         3. Double-click run.cmd. After five seconds the script takes the mouse and the\n   \
         keyboard: it loads the logs of this folder and saves {shots} pictures of the\n   \
         tool's windows. Leave both alone until it beeps twice.\n\
         4. Send back capture-results.zip, which appears next to the script.\n\
         \n\
         To stop it, move the mouse: it stops before its next click.\n\
         \n\
         What the files are: *.bbl are the probe logs; manifest.json says what is true\n\
         about each; ours.json is what OpenPID computes for them; steps.json is what the\n\
         script does, step by step; capture.ps1 is the script.\n"
    );
    vec![
        ("capture.ps1".to_string(), crlf(SCRIPT)),
        ("run.cmd".to_string(), crlf(launcher)),
        ("README.txt".to_string(), crlf(&note)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue;

    #[test]
    fn round_1_takes_the_spectrum_of_every_log_at_every_level() {
        let probes = catalogue::round(1).unwrap_or_default();
        let plan = plan(1, &probes);
        let shots: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == "shot")
            .filter_map(|s| s.file.as_deref())
            .collect();
        assert_eq!(shots.len(), 2 + probes.len() * 5);
        assert_eq!(shots[0], "r1-log-loaded");
        assert_eq!(shots[1], "r1-spectrum-open");
        assert_eq!(shots[2], "r1-s1-levels-2k-spectrum-low");
        assert_eq!(shots[6], "r1-s1-levels-2k-spectrum-medium");
        assert_eq!(
            *shots.last().unwrap_or(&""),
            "r1-s9-bands-2k-spectrum-medium"
        );
        let mut unique = shots.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), shots.len(), "a picture is saved twice");

        // one dialog for all logs, in the order of the catalogue
        let open: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|s| s.action == "open_file")
            .collect();
        assert_eq!(open.len(), 1);
        let paths = open[0].paths.clone().unwrap_or_default();
        assert_eq!(paths.len(), probes.len());
        assert_eq!(paths[0], "s1-levels-2k.bbl");
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(sorted, paths, "the list of the tool is sorted by name");

        // every Run is waited for against a mark taken before the change
        let mut marked = false;
        for step in &plan.steps {
            marked |= step.mark == Some(true);
            if step.action == "settle" && step.change_s.is_some() {
                assert!(marked, "a wait for a change without a mark");
                marked = false;
            }
        }
        // every click is inside its reference screenshot
        for step in plan.steps.iter().filter(|s| s.action == "click") {
            let (x, y) = step.at.unwrap_or((0, 0));
            assert!(x > 3300 && x < 3777, "{step:?}");
            assert!(y > 47 && y < 2471, "{step:?}");
            assert!(step.window.is_some() && step.frame.is_some());
        }
        // the rows of the log list: 29.5 pixels apart
        let rows: Vec<u32> = plan
            .steps
            .iter()
            .filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("log ")))
            .filter_map(|s| s.at.map(|a| a.1))
            .collect();
        assert_eq!(rows, [161, 190, 220, 249, 279, 308, 338, 367, 397]);
        // the five rows of the smoothing list
        let levels: Vec<(String, u32)> = plan
            .steps
            .iter()
            .filter(|s| {
                s.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("smoothing "))
            })
            .filter(|s| s.name.as_deref() != Some("smoothing list"))
            .take(5)
            .filter_map(|s| Some((s.name.clone()?, s.at?.1)))
            .collect();
        let expected = [
            ("smoothing low", 1347),
            ("smoothing low-med", 1380),
            ("smoothing med-high", 1447),
            ("smoothing high", 1481),
            ("smoothing medium", 1414),
        ];
        for (got, want) in levels.iter().zip(expected) {
            assert_eq!((got.0.as_str(), got.1), want);
        }
        assert_eq!(levels.len(), 5);
    }

    #[test]
    fn the_plan_is_what_the_script_reads() {
        let probes = catalogue::round(1).unwrap_or_default();
        let json = serde_json::to_value(plan(1, &probes)).unwrap_or_default();
        assert_eq!(json["refs"]["spectrum"]["width"], 3776);
        let first = &json["steps"][0];
        assert_eq!(first["action"], "click");
        assert_eq!(first["window"], "Log Viewer");
        assert_eq!(first["ref"], "log");
        assert_eq!(first["at"][0], 3468);
        assert_eq!(first["mark"], true);
        // a step carries only what it uses
        assert!(first.get("file").is_none() && first.get("ms").is_none());
        assert_eq!(json["steps"][1]["paths"][8], "s9-bands-2k.bbl");
        // every action is one the script knows
        let script = SCRIPT;
        for step in json["steps"].as_array().map_or(&[][..], Vec::as_slice) {
            let action = step["action"].as_str().unwrap_or("");
            assert!(
                script.contains(&format!("$action -eq \"{action}\"")),
                "{action}"
            );
        }
    }

    #[test]
    fn the_script_travels_with_the_pack() {
        let files = files(1, 47);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["capture.ps1", "run.cmd", "README.txt"]);
        for (name, bytes) in &files {
            let text = String::from_utf8(bytes.clone()).unwrap_or_default();
            // Windows PowerShell reads a script without a byte-order mark in
            // the system code page: the files stay ASCII
            assert!(text.is_ascii(), "{name} is not ASCII");
            assert!(
                text.contains("\r\n") && !text.replace("\r\n", "").contains('\n'),
                "{name}"
            );
        }
        let script = String::from_utf8(files[0].1.clone()).unwrap_or_default();
        assert!(script.contains("function Invoke-Step"));
        assert!(String::from_utf8_lossy(&files[1].1).contains("-Steps \"steps.json\""));
        assert!(String::from_utf8_lossy(&files[2].1).contains("saves 47 pictures"));
    }
}
