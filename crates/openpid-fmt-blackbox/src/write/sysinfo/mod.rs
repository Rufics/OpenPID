//! The system-information headers of each Betaflight release the writer
//! knows: the lines after the field definitions (`Firmware revision`,
//! `looptime`, the gains, the filters, …).
//!
//! `lines.txt`, beside this file, is the table of them: every header line
//! some release writes, each written once, in the firmware's order, with the
//! releases that write it and the build options and run-time conditions it
//! depends on. These are facts of the firmware source; the file names the
//! tag of each release and the function they were read from. It is read
//! once.
//!
//! The table says which lines a log of a release has and where each belongs.
//! It holds no values: the writer writes a line when it knows its value and
//! leaves it out otherwise (`config`).

use super::Target;
use std::sync::LazyLock;

/// Every header line, the releases that write it and its conditions.
const LINES: &str = include_str!("lines.txt");

/// The condition of the lines the firmware writes only when the battery
/// voltage is logged (`vbat_scale`, `vbatcellvoltage`, `vbatref`).
pub(super) const VBAT: &str = "VBAT";
/// The condition of the line the firmware writes only when the current
/// meter is the ADC (`currentSensor`).
pub(super) const CURRENT_METER_ADC: &str = "CURRENT_METER_ADC";

/// One header line of a release.
#[derive(Debug, Clone)]
pub(super) struct Line {
    /// Header name.
    pub name: &'static str,
    /// What must hold for the firmware to write the line, as `lines.txt`
    /// has it: the firmware's macro of a build option (`USE_GPS`,
    /// `!USE_WING`, `A|B`), or one of the two run-time conditions.
    pub when: Vec<&'static str>,
}

impl Line {
    /// Whether the line depends on `condition`.
    pub(super) fn needs(&self, condition: &str) -> bool {
        self.when.contains(&condition)
    }
}

/// The releases a span of `lines.txt` names: `all`, one release (`4.2`), a
/// span (`4.4-4.5`), or every release from one on (`4.3-`).
fn span(text: &str) -> Result<Vec<Target>, String> {
    let named = |name: &str| {
        let unknown = || format!("no release `{name}` in `{text}`");
        Target::from_name(name).ok_or_else(unknown)
    };
    let (first, last) = match text.trim() {
        "all" => (Target::ALL[0], Target::ALL[Target::ALL.len() - 1]),
        from if from.ends_with('-') => (
            named(&from[..from.len() - 1])?,
            Target::ALL[Target::ALL.len() - 1],
        ),
        // the names of the releases have a `-` of their own (`2026.12-dev`)
        one if Target::from_name(one).is_some() => (named(one)?, named(one)?),
        both => {
            let ends = both
                .match_indices('-')
                .map(|(at, _)| (&both[..at], &both[at + 1..]))
                .find(|(a, b)| Target::from_name(a).is_some() && Target::from_name(b).is_some());
            let (first, last) = ends.ok_or_else(|| format!("no span of releases: `{both}`"))?;
            (named(first)?, named(last)?)
        }
    };
    Ok(Target::ALL
        .into_iter()
        .filter(|t| (first..=last).contains(t))
        .collect())
}

/// The rows of the table: the lines that are not empty and not a comment.
fn rows(text: &'static str) -> impl Iterator<Item = &'static str> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
}

/// Whether `condition` is written as the table writes one: the names of
/// macros, each with or without a `!`, with `|` between alternatives.
fn is_condition(condition: &str) -> bool {
    condition.split('|').all(|option| {
        let name = option.strip_prefix('!').unwrap_or(option);
        let macro_like = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_';
        !name.is_empty() && name.chars().all(macro_like)
    })
}

/// The lines of `target` in the table `text`.
fn table(text: &'static str, target: Target) -> Result<Vec<Line>, String> {
    let mut out = Vec::new();
    for row in rows(text) {
        let mut cells = row.split(';').map(str::trim);
        let (Some(name), Some(releases)) = (cells.next(), cells.next()) else {
            return Err(format!("no `name ; releases`: `{row}`"));
        };
        let mut written = false;
        for part in releases.split(',') {
            written |= span(part)?.contains(&target);
        }
        let when: Vec<&str> = cells.next().unwrap_or("").split_whitespace().collect();
        if let Some(bad) = when.iter().find(|c| !is_condition(c)) {
            return Err(format!("no condition `{bad}`: `{row}`"));
        }
        if written {
            out.push(Line { name, when });
        }
    }
    Ok(out)
}

/// The lines of every release, in the order of [`Target::ALL`]. The table
/// is part of the program and tested as it is built
/// (`the_table_is_well_formed`); one that did not read is empty.
static TABLES: LazyLock<Vec<Vec<Line>>> = LazyLock::new(|| {
    Target::ALL
        .into_iter()
        .map(|target| table(LINES, target).unwrap_or_default())
        .collect()
});

/// Every header line `target` can write, in the firmware's order.
pub(super) fn lines(target: Target) -> &'static [Line] {
    let index = Target::ALL.iter().position(|t| *t == target);
    index.and_then(|i| TABLES.get(i)).map_or(&[], Vec::as_slice)
}

impl Target {
    /// The names of the system-information header lines the firmware of
    /// this release can write, in the order it writes them. The writer
    /// writes those whose value it knows; a caller that knows more gives
    /// them in [`WriteOptions::headers`](super::WriteOptions::headers), and
    /// each lands in the place it has here.
    pub fn header_names(self) -> Vec<&'static str> {
        lines(self).iter().map(|l| l.name).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_table_is_well_formed() {
        // every line any release can write, as counted in the firmware source
        let sizes = Target::ALL.map(|t| table(LINES, t).unwrap().len());
        assert_eq!(sizes, [112, 136, 172, 191, 242, 233, 275]);
        for target in Target::ALL {
            let lines = lines(target);
            let names: HashSet<&str> = lines.iter().map(|l| l.name).collect();
            assert_eq!(names.len(), lines.len(), "{target}: a line twice");
            for name in [
                "Firmware revision",
                "I interval",
                "P interval",
                "looptime",
                "pid_process_denom",
                "rollPID",
                "motorOutput",
                "features",
                "debug_mode",
            ] {
                assert!(names.contains(name), "{target}: no {name}");
            }
        }
        assert_eq!(span("all").unwrap().len(), 7);
        assert_eq!(span("4.4-4.5").unwrap(), [Target::V4_4, Target::V4_5]);
        assert_eq!(
            span(" 2026.6- ").unwrap(),
            [Target::V2026_6, Target::Master]
        );
        assert_eq!(span("2026.12-dev").unwrap(), [Target::Master]);
        assert_eq!(span("2026.12-dev-").unwrap(), [Target::Master]);
        assert_eq!(span("4.5-2026.12-dev").unwrap().len(), 4);
        assert!(span("4.1-").is_err() && span("4.2-5.0").is_err() && span("").is_err());
        // what a damaged table is refused for
        let bad = |text: &'static str| table(text, Target::V4_5).unwrap_err();
        assert!(bad("looptime").contains("name ; releases"));
        assert!(bad("looptime ; 4.5 ; Turbo").contains("no condition `Turbo`"));
        assert!(bad("looptime ; 4.5 ; USE_A|").contains("no condition"));
        assert!(bad("looptime ; 3.9").contains("no span of releases: `3.9`"));
        let line = &table("vbatref ; all ; VBAT USE_X", Target::V4_5).unwrap()[0];
        assert!(line.needs(VBAT) && !line.needs(CURRENT_METER_ADC));
    }

    /// Whether a build with the options `defined` writes a line with the
    /// condition `condition`.
    fn holds(condition: &str, defined: &[&str]) -> bool {
        condition
            .split('|')
            .any(|option| match option.strip_prefix('!') {
                Some(name) => !defined.contains(&name),
                None => defined.contains(&option),
            })
    }

    /// The header names of a fixture after its field definitions.
    fn logged(file: &str) -> Vec<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/logs")
            .join(file);
        let data = std::fs::read(path).unwrap_or_default();
        let headers = crate::Headers::parse(&data);
        let raw = headers.map(|h| h.raw.0).unwrap_or_default();
        raw.into_iter()
            .map(|(name, _)| name)
            .filter(|n| n != "Product" && n != "Data version" && !n.starts_with("Field "))
            .collect()
    }

    /// The lines `target` writes in a build with the options `defined`.
    fn written_by(target: Target, defined: &[&str]) -> Vec<&'static str> {
        lines(target)
            .iter()
            .filter(|l| l.when.iter().all(|c| holds(c, defined)))
            .map(|l| l.name)
            .collect()
    }

    /// Every option the lines of `target` name: the official builds of 4.2
    /// to 4.5 had them all.
    fn every_option(target: Target) -> Vec<&'static str> {
        let conditions = lines(target).iter().flat_map(|l| l.when.iter());
        conditions
            .flat_map(|c| c.split('|'))
            .filter(|option| !option.starts_with('!'))
            .collect()
    }

    /// The table against what flight controllers wrote: the names and the
    /// order of every header line of the release builds among the fixtures.
    #[test]
    fn the_table_lists_what_real_builds_write() {
        // 4.5.1, an official build: every option, and a battery with an ADC
        let everything = every_option(Target::V4_5);
        assert_eq!(
            logged("betaflight-4.5.1-two-sessions.bbl"),
            written_by(Target::V4_5, &everything)
        );
        // 4.2: all three fixtures are official builds; the craft of 4.2.8
        // has no current meter on an ADC
        let with_adc = every_option(Target::V4_2);
        let without_adc: Vec<&str> = with_adc
            .iter()
            .copied()
            .filter(|option| *option != CURRENT_METER_ADC)
            .collect();
        let of_4_2 = |defined: &[&str]| written_by(Target::V4_2, defined);
        assert_eq!(logged("betaflight-4.2.11-two-logs.bbl"), of_4_2(&with_adc));
        assert_eq!(logged("betaflight-4.2.0.BFL"), of_4_2(&with_adc));
        assert_eq!(logged("betaflight-4.2.8-44-logs.bbl"), of_4_2(&without_adc));
        // 2025.12.2, a cloud build for a wing with GPS, compass and barometer
        let wing = [
            "USE_BOARD_INFO",
            "USE_ACC",
            VBAT,
            CURRENT_METER_ADC,
            "USE_WING",
            "USE_ADVANCED_TPA",
            "USE_D_MAX",
            "USE_DYN_LPF",
            "USE_ITERM_RELAX",
            "USE_FEEDFORWARD",
            "USE_CHIRP",
            "USE_DYN_NOTCH_FILTER",
            "USE_DSHOT_TELEMETRY",
            "USE_RPM_FILTER",
            "USE_BARO",
            "USE_MAG",
            "USE_RC_SMOOTHING_FILTER",
            "USE_BATTERY_VOLTAGE_SAG_COMPENSATION",
            "USE_DYN_IDLE",
            "USE_SIMPLIFIED_TUNING",
            "USE_GPS",
        ];
        assert_eq!(
            logged("betaflight-2025.12.2-btfl_027.bbl"),
            written_by(Target::V2025_12, &wing)
        );
        // the "4.5.0" fixture was built before the release: one line short
        let before: Vec<&str> = written_by(Target::V4_5, &everything)
            .into_iter()
            .filter(|n| *n != "ez_landing_speed")
            .collect();
        assert_eq!(logged("betaflight-4.5.0-gps-rescue.bbl"), before);
        // a multirotor of 2025.12 has the autopilot lines a wing has not
        let quad: Vec<&str> = wing.into_iter().filter(|o| *o != "USE_WING").collect();
        let lines = written_by(Target::V2025_12, &quad);
        assert!(lines.contains(&"ap_hover_throttle") && !lines.contains(&"s_roll"));
    }

    #[test]
    fn what_differs_between_the_releases() {
        let has = |t: Target, name: &str| lines(t).iter().any(|l| l.name == name);
        // the throttle limits of the PWM era went with 4.5
        assert!(has(Target::V4_5, "minthrottle") && !has(Target::V2025_12, "minthrottle"));
        assert!(!has(Target::V4_5, "DeviceUID") && has(Target::V2025_12, "DeviceUID"));
        assert!(!has(Target::V4_3, "blackbox_high_resolution"));
        // the mask of the fields came with 4.3, and the new names of the filters
        assert!(!has(Target::V4_2, "fields_disabled_mask"));
        assert!(has(Target::V4_3, "fields_disabled_mask"));
        assert!(has(Target::V4_2, "gyro_lowpass_hz") && has(Target::V4_3, "gyro_lpf1_static_hz"));
        assert!(has(Target::V4_4, "blackbox_high_resolution"));
        assert!(has(Target::V4_5, "d_min") && has(Target::V2025_12, "d_max"));
        assert!(
            !has(Target::V2026_6, "rc_command_scale") && has(Target::Master, "rc_command_scale")
        );
        assert!(!has(Target::V2026_6, "debug_mode_name") && has(Target::Master, "debug_mode_name"));
        assert_eq!(lines(Target::V4_5)[0].name, "Firmware type");
    }
}
