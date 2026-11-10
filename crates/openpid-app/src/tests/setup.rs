//! The Setup info tab (docs/design/setup-tab).

use super::*;
use crate::app::Tab;
use crate::setup_tab::{Line, Section};

/// The row named `name` of `section`: each log's value, and whether they differ.
fn row<'a>(ui: &'a Ui, section: Section, name: &str) -> Option<(Vec<Option<&'a str>>, bool)> {
    ui.app().setup_state.lines.iter().find_map(|l| match l {
        Line::Row {
            section: s,
            name: n,
            values,
            differs,
            ..
        } if *s == section && n == name => {
            Some((values.iter().map(Option::as_deref).collect(), *differs))
        }
        _ => None,
    })
}

/// The sections shown, with the number of rows under each title.
fn sections(ui: &Ui) -> Vec<(Section, usize)> {
    ui.app()
        .setup_state
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Title(s, n) => Some((*s, *n)),
            Line::Row { .. } => None,
        })
        .collect()
}

/// The rows shown: (name, differs).
fn rows(ui: &Ui) -> Vec<(String, bool)> {
    ui.app()
        .setup_state
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Row { name, differs, .. } => Some((name.clone(), *differs)),
            Line::Title(..) => None,
        })
        .collect()
}

#[test]
fn setup_info_has_a_column_per_log_and_marks_what_differs() {
    // A: 4.5.0, a 1 s log; B: 4.5.1, its 21 s session
    let mut ui = Ui::open(&[LOG_4_5, LOG_4_5_1], Tab::Setup);
    assert_eq!(ui.app().setup_state.logs, [Slot::A, Slot::B]);
    assert!(ui.sees("A · betaflight-4.5.0-gps-rescue"));
    assert!(ui.sees("B · betaflight-4.5.1-two-sessions"));
    let shown: Vec<Section> = sections(&ui).into_iter().map(|(s, _)| s).collect();
    assert_eq!(shown, Section::ALL);

    // the log check: the verdict, each check with its mark, a fix to copy
    assert_eq!(
        row(&ui, Section::LogCheck, "verdict"),
        Some((vec![Some("log unfit"), Some("log ok")], true))
    );
    assert!(ui.sees("A duration [x]"), "the mark is read out");
    assert!(ui.sees("B duration [ok]"));
    assert_eq!(
        row(&ui, Section::LogCheck, "duration: fix"),
        Some((vec![Some("fly and log for at least 20 s"), None], true)),
        "only the short log has something to fix"
    );
    assert!(ui.has("Copy A duration: fix"));
    assert!(!ui.has("Copy B duration: fix"));
    assert!(ui.sees("fly and log for at least 20 s"));

    // the decoder and the setup, from the typed model
    assert_eq!(
        row(&ui, Section::Decoder, "Profile"),
        Some((vec![Some("betaflight"), Some("betaflight")], false))
    );
    assert_eq!(
        row(&ui, Section::Decoder, "Unmapped fields"),
        Some((vec![Some("magADC[0], magADC[1], magADC[2]"), None], true))
    );
    assert_eq!(
        row(&ui, Section::Setup, "Firmware"),
        Some((
            vec![
                Some("Betaflight 4.5.0 (3aabaf365) STM32F7X2"),
                Some("Betaflight 4.5.1 (77d01ba3b) STM32F7X2")
            ],
            true
        ))
    );
    assert_eq!(
        row(&ui, Section::Setup, "Target"),
        Some((vec![Some("STM32F7X2"), Some("STM32F7X2")], false))
    );
    assert_eq!(
        row(&ui, Section::Setup, "Board"),
        Some((
            vec![Some("TMTR TMOTORF7"), Some("GEPR GEPRC_F722_AIO")],
            true
        ))
    );
    assert_eq!(
        row(&ui, Section::Setup, "Log date"),
        Some((vec![Some("2024-03-31T15:22:09.505+00:00"), None], true)),
        "the second flight controller had no clock"
    );
    // the tuning settings the firmware profile puts in words (#140)
    assert_eq!(
        row(&ui, Section::Setup, "Rates"),
        Some((
            vec![
                Some(
                    "Actual: centre 70 / 70 / 70 deg/s, max 670 / 670 / 670 deg/s, expo 0.15 / 0.15 / 0.15"
                ),
                Some(
                    "Actual: centre 70 / 70 / 70 deg/s, max 670 / 670 / 670 deg/s, expo 0.00 / 0.00 / 0.00"
                )
            ],
            true
        ))
    );
    assert_eq!(
        row(&ui, Section::Setup, "TPA"),
        Some((
            vec![
                Some("65 % above 1350, on D; 20 % below 1050"),
                Some("65 % above 1350, on D; 80 % below 1050")
            ],
            true
        ))
    );
    assert_eq!(
        row(&ui, Section::Setup, "I-term relax"),
        Some((
            vec![Some("roll and pitch, from the setpoint, cut-off 15 Hz"); 2],
            false
        ))
    );
    for (name, a, b) in [
        (
            "Anti-gravity",
            "gain 80, cut-off 5 Hz, P gain 100",
            "gain 100, cut-off 5 Hz, P gain 100",
        ),
        ("Throttle boost", "5, cut-off 15 Hz", "0, cut-off 15 Hz"),
        (
            "Motor idle",
            "5.5 %, dynamic idle off",
            "10.5 %, dynamic idle off",
        ),
        (
            "Battery",
            "cell min 3.25 V, warning 3.42 V, max 4.30 V; 24.58 V at arming",
            "cell min 3.30 V, warning 3.50 V, max 4.35 V; 15.92 V at arming",
        ),
    ] {
        assert_eq!(
            row(&ui, Section::Setup, name),
            Some((vec![Some(a), Some(b)], true)),
            "{name}"
        );
    }
    let (sliders, _) = row(&ui, Section::Setup, "Simplified tuning").unwrap();
    assert!(
        sliders[0]
            .unwrap()
            .starts_with("roll, pitch and yaw, master 75, ")
    );
    assert!(
        sliders[1]
            .unwrap()
            .starts_with("roll and pitch, master 130, ")
    );
    let (ff, _) = row(&ui, Section::Setup, "Feedforward").unwrap();
    assert!(ff[0].unwrap().contains("averaging 2 point"));
    assert!(ff[1].unwrap().contains("averaging off"));
    assert_eq!(
        row(&ui, Section::Setup, "Craft name"),
        Some((vec![None, Some("Cinelog 30 V3")], true)),
        "a row one log lacks is a difference"
    );
    assert_eq!(
        row(&ui, Section::Setup, "PID roll"),
        Some((
            vec![
                Some("P 33 · I 60 · D 31 · D min 23 · FF 90"),
                Some("P 49 · I 70 · D 47 · D min 42 · FF 109")
            ],
            true
        ))
    );
    assert_eq!(
        row(&ui, Section::Setup, "D-term low-pass 1"),
        Some((
            vec![
                Some("PT1 93–187 Hz by throttle"),
                Some("biquad 75–150 Hz by throttle")
            ],
            true
        ))
    );
    // every header as logged, matched by name
    assert_eq!(
        row(&ui, Section::Headers, "rollPID"),
        Some((vec![Some("33,60,31"), Some("49,70,47")], true))
    );
    let (differing, total) = ui.app().setup_state.differing;
    assert_eq!(total, rows(&ui).len(), "nothing is filtered by default");
    assert_eq!(differing, rows(&ui).iter().filter(|r| r.1).count());
    assert!(ui.sees(&format!("{differing} / {total} rows differ")));

    // "Differences only" leaves the rows the logs do not agree in
    ui.check("Differences only", true);
    let left = rows(&ui);
    assert_eq!(left.len(), differing);
    assert!(left.iter().all(|r| r.1));
    assert!(row(&ui, Section::Setup, "Target").is_none());
    assert!(row(&ui, Section::Setup, "Firmware").is_some());
    ui.check("Differences only", false);

    // the search looks in the names and the values, in any case
    ui.type_into("Search", "DSHOT");
    let found = rows(&ui);
    assert!(
        found.iter().any(|(name, _)| name == "ESC protocol"),
        "by value: DSHOT300 / DSHOT600"
    );
    assert!(
        found.iter().any(|(name, _)| name == "dshot_bidir"),
        "by name"
    );
    assert!(found.len() < 12, "{found:?}");
    assert!(ui.sees("DSHOT600"), "a row far down the table is in view");
    assert!(
        sections(&ui).iter().all(|(s, _)| *s != Section::LogCheck),
        "a section without a match has no title"
    );
    ui.type_into("Search", " no such thing");
    assert!(rows(&ui).is_empty());
    assert!(ui.sees("no row mentions this text"));
    // nothing but the keyboard clears a text field; the test sets it
    ui.app_mut().tools.setup_display.search.clear();
    ui.settle();

    // a section can be left out
    ui.check("All headers", false);
    ui.check("Log check", false);
    let shown: Vec<Section> = sections(&ui).into_iter().map(|(s, _)| s).collect();
    assert_eq!(shown, [Section::Decoder, Section::Setup]);
    assert!(!ui.sees("A duration [x]"));
    ui.check("Decoder", false);
    ui.check("Setup", false);
    assert!(ui.sees("every section is hidden"));
    ui.check("Setup", true);

    // the table as text: a column per log, the rows shown
    let copied = crate::setup_tab::as_text(ui.app());
    let mut lines = copied.lines();
    assert_eq!(
        lines.next(),
        Some("\tbetaflight-4.5.0-gps-rescue.bbl\tbetaflight-4.5.1-two-sessions.bbl")
    );
    assert_eq!(lines.next(), Some("# Setup"));
    assert_eq!(
        lines.next(),
        Some(
            "Firmware\tBetaflight 4.5.0 (3aabaf365) STM32F7X2\tBetaflight 4.5.1 (77d01ba3b) STM32F7X2"
        )
    );
    assert!(copied.contains("\nCraft name\t\tCinelog 30 V3\n"));
    ui.click("Copy the table");

    // a hidden log has no column, and one log has nothing to differ from
    ui.click("hide B");
    assert_eq!(ui.app().setup_state.logs, [Slot::A]);
    assert!(rows(&ui).iter().all(|r| !r.1));
    assert!(!ui.is_enabled("Differences only"));
    assert!(!ui.sees("rows differ"));
}

#[test]
fn setup_info_follows_the_session_and_the_firmware() {
    // the table is of the session shown
    let mut ui = Ui::open(&[LOG_4_5_1], Tab::Setup);
    assert!(!ui.is_enabled("Differences only"), "one log");
    let samples = [Some("20968 over 20.7 s")];
    assert_eq!(row(&ui, Section::Decoder, "Samples").unwrap().0, samples);
    assert!(ui.sees("A verdict [ok]"));
    ui.choose("session", "session 0 (");
    assert_ne!(
        row(&ui, Section::Decoder, "Samples").unwrap().0,
        samples,
        "session 0 is a short stub"
    );
    assert!(ui.sees("A verdict [x]"));

    // ArduPilot: gains at its step, its messages as frames, its notes
    let ui = Ui::open(&[LOG_ARDUCOPTER], Tab::Setup);
    assert_eq!(
        row(&ui, Section::Setup, "PID roll").unwrap().0,
        [Some("P 0.1288 · I 0.1288 · D 0.0072")]
    );
    assert_eq!(
        row(&ui, Section::Decoder, "Profile").unwrap().0,
        [Some("ardupilot")]
    );
    let notes = rows(&ui).iter().filter(|r| r.0 == "Note").count();
    assert_eq!(notes, 2, "each note of the decoder has its row");
    assert!(
        row(&ui, Section::Setup, "ESC protocol").is_none(),
        "what the log does not say has no row"
    );
    for name in ["Rates", "TPA", "Board", "Log date"] {
        assert!(
            row(&ui, Section::Setup, name).is_none(),
            "{name}: Betaflight's words are not put on another firmware"
        );
    }

    // without a log there is no table
    let ui = Ui::open(&[], Tab::Setup);
    assert!(ui.app().setup_state.lines.is_empty());
}

/// Every committed fixture has its table: the firmware in words, the
/// decoder's counts, and each of its headers in "All headers" (#140).
#[test]
fn setup_info_shows_every_fixture() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/logs");
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            ["bbl", "bfl", "txt", "bin"].contains(&ext.to_lowercase().as_str())
        })
        .collect();
    logs.sort();
    assert!(logs.len() >= 12, "{logs:?}");
    for log in logs {
        let name = log.file_name().unwrap().to_string_lossy().into_owned();
        let ui = Ui::open(&[log.to_str().unwrap()], Tab::Setup);
        let session = ui
            .app()
            .loaded(Slot::A)
            .and_then(|l| l.current())
            .unwrap_or_else(|| panic!("{name}: no session"))
            .clone();
        let firmware = row(&ui, Section::Setup, "Firmware").map(|r| r.0);
        assert_eq!(
            firmware,
            Some(vec![Some(session.firmware.revision.as_str())]),
            "{name}"
        );
        assert!(row(&ui, Section::Decoder, "Samples").is_some(), "{name}");
        let headers = sections(&ui)
            .into_iter()
            .find(|(s, _)| *s == Section::Headers)
            .map_or(0, |(_, n)| n);
        assert_eq!(
            headers,
            session.raw_headers.len(),
            "{name}: no header is dropped"
        );
    }
}
