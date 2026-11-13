//! The system-information headers of each Betaflight release the writer
//! knows: the lines after the field definitions (`Firmware revision`,
//! `looptime`, the gains, the filters, …).
//!
//! They are data, kept in two files beside this one and read once:
//!
//! - `lines.txt`: every header line some release writes, each written once,
//!   in the firmware's order, with the releases that write it and the build
//!   options and run-time conditions it depends on ([`Build`]). These are
//!   facts of the firmware source; the file names the tag of each release
//!   and the function they were read from.
//! - `crafts.txt`: the values of the example craft of each release, for a
//!   session that brings no configuration of its own. They are a
//!   configuration, not facts of a release; the file says where they are
//!   from.
//!
//! [`lines`] gives the table of one release: its lines, in its order, each
//! with its conditions and the example value.

use super::Target;
use std::sync::LazyLock;

/// Every header line, the releases that write it and its conditions.
const LINES: &str = include_str!("lines.txt");
/// The values of the example craft of each release.
const CRAFTS: &str = include_str!("crafts.txt");

/// What a header line depends on: a build option of the firmware (the
/// `USE_…` macro in the comment) or a run-time condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Build {
    /// `USE_BOARD_INFO`.
    BoardInfo,
    /// `USE_ACC`.
    Acc,
    /// Run time: the battery voltage is logged (a voltage meter is
    /// configured and the battery fields are not switched off).
    Vbat,
    /// Run time: the current meter is the ADC.
    CurrentAdc,
    /// `USE_WING`.
    Wing,
    /// Not `USE_WING`.
    NotWing,
    /// `USE_ADVANCED_TPA` (the firmware defines it for wing builds).
    AdvancedTpa,
    /// `USE_D_MIN` (to 4.5) or `USE_D_MAX` (from 2025.12).
    DMinMax,
    /// `USE_DYN_LPF`.
    DynLpf,
    /// `USE_ITERM_RELAX`.
    ItermRelax,
    /// `USE_ABSOLUTE_CONTROL`.
    AbsoluteControl,
    /// `USE_INTEGRATED_YAW_CONTROL`.
    IntegratedYaw,
    /// `USE_FEEDFORWARD`.
    Feedforward,
    /// `USE_INTERPOLATED_SP` (4.2: the feedforward from the interpolated
    /// setpoint).
    InterpolatedSetpoint,
    /// `USE_GYRO_DATA_ANALYSE` (4.2: the dynamic notch).
    GyroAnalysis,
    /// `USE_CHIRP`.
    Chirp,
    /// `USE_DYN_NOTCH_FILTER`.
    DynNotch,
    /// `USE_DSHOT_TELEMETRY`.
    DshotTelemetry,
    /// `USE_RPM_FILTER`.
    RpmFilter,
    /// `USE_BARO`.
    Baro,
    /// `USE_MAG`.
    Mag,
    /// `USE_OPTICALFLOW`.
    OpticalFlow,
    /// `USE_RC_SMOOTHING_FILTER`.
    RcSmoothing,
    /// `USE_BATTERY_VOLTAGE_SAG_COMPENSATION`.
    SagCompensation,
    /// `USE_DYN_IDLE`.
    DynIdle,
    /// `USE_SIMPLIFIED_TUNING`.
    SimplifiedTuning,
    /// `USE_GPS`.
    Gps,
    /// `USE_GPS_RESCUE`.
    GpsRescue,
    /// `USE_GPS_RESCUE` on a build that flies the rescue: not `USE_WING`, or
    /// `ENABLE_RESCUE_PLAN` (the condition of `master`).
    RescueFlown,
    /// `USE_ALTITUDE_HOLD`.
    AltHold,
    /// `USE_POSITION_HOLD`.
    PosHold,
    /// `BUILD_KEY` (a cloud build).
    CloudKey,
    /// `RELEASE_NAME` (a cloud build of a release).
    ReleaseName,
}

impl Build {
    /// Every condition.
    const ALL: [Build; 33] = [
        Build::BoardInfo,
        Build::Acc,
        Build::Vbat,
        Build::CurrentAdc,
        Build::Wing,
        Build::NotWing,
        Build::AdvancedTpa,
        Build::DMinMax,
        Build::DynLpf,
        Build::ItermRelax,
        Build::AbsoluteControl,
        Build::IntegratedYaw,
        Build::Feedforward,
        Build::InterpolatedSetpoint,
        Build::GyroAnalysis,
        Build::Chirp,
        Build::DynNotch,
        Build::DshotTelemetry,
        Build::RpmFilter,
        Build::Baro,
        Build::Mag,
        Build::OpticalFlow,
        Build::RcSmoothing,
        Build::SagCompensation,
        Build::DynIdle,
        Build::SimplifiedTuning,
        Build::Gps,
        Build::GpsRescue,
        Build::RescueFlown,
        Build::AltHold,
        Build::PosHold,
        Build::CloudKey,
        Build::ReleaseName,
    ];

    /// The condition `lines.txt` calls `name`: the name of the variant.
    fn from_name(name: &str) -> Option<Build> {
        Build::ALL.into_iter().find(|b| format!("{b:?}") == name)
    }
}

/// One header line of a release.
#[derive(Debug, Clone)]
pub(super) struct Line {
    /// Header name.
    pub name: &'static str,
    /// Every one of these must hold for the firmware to write the line.
    pub when: Vec<Build>,
    /// The value of the example configuration; `None` where the writer has
    /// no verified one, so the line is written only for a session that
    /// carries it.
    pub value: Option<&'static str>,
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

/// The lines of a data file that hold data: not empty, not a comment.
fn data(text: &'static str) -> impl Iterator<Item = &'static str> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
}

/// The value of the example craft of `target` for each header name it has
/// one for (`crafts.txt`: sections of `name:value` under the releases they
/// are for, a later section overriding an earlier one).
fn craft(text: &'static str, target: Target) -> Result<Vec<(&'static str, &'static str)>, String> {
    let mut values: Vec<(&'static str, &'static str)> = Vec::new();
    let mut ours = false;
    for line in data(text) {
        if let Some(section) = line.strip_prefix('[') {
            let releases = section.trim_end_matches(']').split_whitespace();
            let releases: Result<Vec<Target>, String> = releases
                .map(|name| Target::from_name(name).ok_or_else(|| format!("no release `{name}`")))
                .collect();
            ours = releases?.contains(&target);
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| format!("no `name:value`: `{line}`"))?;
        if !ours {
            continue;
        }
        match values.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => values.push((name, value)),
        }
    }
    Ok(values)
}

/// The table of `target` from the two data files.
fn table(lines: &'static str, crafts: &'static str, target: Target) -> Result<Vec<Line>, String> {
    let values = craft(crafts, target)?;
    let mut out = Vec::new();
    for row in data(lines) {
        let mut cells = row.split('|').map(str::trim);
        let (Some(name), Some(releases)) = (cells.next(), cells.next()) else {
            return Err(format!("no `name | releases`: `{row}`"));
        };
        let mut written = false;
        for part in releases.split(',') {
            written |= span(part)?.contains(&target);
        }
        let when: Result<Vec<Build>, String> = cells
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(|c| Build::from_name(c).ok_or_else(|| format!("no condition `{c}`: `{row}`")))
            .collect();
        let when = when?;
        if written {
            let value = values.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
            out.push(Line { name, when, value });
        }
    }
    Ok(out)
}

/// The table of every release, in the order of [`Target::ALL`]. The data
/// files are part of the program and tested as it is built
/// (`the_data_files_are_well_formed`); a table that did not read is empty.
static TABLES: LazyLock<Vec<Vec<Line>>> = LazyLock::new(|| {
    Target::ALL
        .into_iter()
        .map(|target| table(LINES, CRAFTS, target).unwrap_or_default())
        .collect()
});

/// Every header line `target` can write, in the firmware's order.
pub(super) fn lines(target: Target) -> &'static [Line] {
    let index = Target::ALL.iter().position(|t| *t == target);
    index.and_then(|i| TABLES.get(i)).map_or(&[], Vec::as_slice)
}

/// The build options of the craft the example configuration describes: what
/// a release build for a common flight controller has. To 4.5 that is
/// everything (the official builds of those versions carried every feature);
/// from 2025.12, where each build is assembled from options, a quadcopter
/// with the usual ones and no GPS, compass, barometer or wing code.
pub(super) fn stock_build(target: Target) -> Vec<Build> {
    use Build::{
        AbsoluteControl, Acc, Baro, BoardInfo, DMinMax, DshotTelemetry, DynIdle, DynLpf, DynNotch,
        Feedforward, Gps, GpsRescue, GyroAnalysis, IntegratedYaw, InterpolatedSetpoint, ItermRelax,
        Mag, NotWing, RcSmoothing, RpmFilter, SagCompensation, SimplifiedTuning,
    };
    let mut on = vec![
        BoardInfo,
        Acc,
        DMinMax,
        DynLpf,
        ItermRelax,
        Feedforward,
        DynNotch,
        DshotTelemetry,
        RpmFilter,
        RcSmoothing,
        SagCompensation,
        DynIdle,
        SimplifiedTuning,
        NotWing,
    ];
    if !target.assembled_from_options() {
        on.extend([
            AbsoluteControl,
            IntegratedYaw,
            InterpolatedSetpoint,
            GyroAnalysis,
            Baro,
            Mag,
            Gps,
            GpsRescue,
        ]);
    }
    on
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_data_files_are_well_formed() {
        for target in Target::ALL {
            let table = table(LINES, CRAFTS, target).unwrap();
            assert_eq!(table.len(), lines(target).len(), "{target}");
            // every value belongs to a line some release writes
            for (name, _) in craft(CRAFTS, target).unwrap() {
                let known =
                    data(LINES).any(|row| row.split('|').next().map(str::trim) == Some(name));
                assert!(
                    known,
                    "{target}: a value for `{name}`, which no release writes"
                );
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
        // what a damaged file is refused for
        let bad = |lines: &'static str, crafts: &'static str| {
            table(lines, crafts, Target::V4_5).unwrap_err()
        };
        assert!(bad("looptime", "").contains("name | releases"));
        assert!(bad("looptime | 4.5 | Turbo", "").contains("no condition `Turbo`"));
        assert!(bad("looptime | 3.9", "").contains("no span of releases: `3.9`"));
        assert!(bad("", "[4.5]\nlooptime").contains("name:value"));
        assert!(bad("", "[4.6]\nlooptime:125").contains("no release `4.6`"));
        for condition in Build::ALL {
            assert_eq!(Build::from_name(&format!("{condition:?}")), Some(condition));
        }
        assert_eq!(Build::from_name("Acc "), None);
    }

    #[test]
    fn every_table_is_a_list_of_distinct_lines_with_values_for_the_stock_build() {
        for target in Target::ALL {
            let lines = lines(target);
            let names: HashSet<&str> = lines.iter().map(|l| l.name).collect();
            assert_eq!(names.len(), lines.len(), "{target}: a line twice");
            let mut build = stock_build(target);
            // what a session can switch on by what it carries
            build.extend([
                Build::Vbat,
                Build::CurrentAdc,
                Build::Gps,
                Build::Mag,
                Build::Baro,
            ]);
            for l in lines {
                if l.when.iter().all(|b| build.contains(b)) {
                    assert!(l.value.is_some(), "{target}: {} has no value", l.name);
                }
            }
            // a wing build instead of a quadcopter
            if target.assembled_from_options() {
                let wing: Vec<Build> = build
                    .iter()
                    .copied()
                    .filter(|b| *b != Build::NotWing)
                    .chain([Build::Wing, Build::AdvancedTpa])
                    .collect();
                for l in lines {
                    if l.when.iter().all(|b| wing.contains(b)) {
                        assert!(l.value.is_some(), "{target} wing: {}", l.name);
                    }
                }
            }
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

    fn written_by(target: Target, build: &[Build]) -> Vec<&'static str> {
        lines(target)
            .iter()
            .filter(|l| l.when.iter().all(|b| build.contains(b)))
            .map(|l| l.name)
            .collect()
    }

    /// The tables against what flight controllers wrote: the names and the
    /// order of every header line of the release builds among the fixtures.
    #[test]
    fn the_tables_list_what_real_builds_write() {
        use Build::{
            Acc, AdvancedTpa, Baro, BoardInfo, Chirp, CurrentAdc, DMinMax, DshotTelemetry, DynIdle,
            DynLpf, DynNotch, Feedforward, Gps, ItermRelax, Mag, RcSmoothing, RpmFilter,
            SagCompensation, SimplifiedTuning, Vbat, Wing,
        };
        // 4.5.1, an official build: every option, and a battery with an ADC
        let mut everything = stock_build(Target::V4_5);
        everything.extend([Vbat, CurrentAdc]);
        assert_eq!(
            logged("betaflight-4.5.1-two-sessions.bbl"),
            written_by(Target::V4_5, &everything)
        );
        // 4.2: all three fixtures are official builds; the craft of 4.2.8
        // has no current meter on an ADC
        let mut all = stock_build(Target::V4_2);
        all.push(Vbat);
        let without_adc = written_by(Target::V4_2, &all);
        all.push(CurrentAdc);
        let with_adc = written_by(Target::V4_2, &all);
        assert_eq!(logged("betaflight-4.2.11-two-logs.bbl"), with_adc);
        assert_eq!(logged("betaflight-4.2.0.BFL"), with_adc);
        assert_eq!(logged("betaflight-4.2.8-44-logs.bbl"), without_adc);
        // 2025.12.2, a cloud build for a wing with GPS, compass and barometer
        let wing = [
            BoardInfo,
            Acc,
            Vbat,
            CurrentAdc,
            Wing,
            AdvancedTpa,
            DMinMax,
            DynLpf,
            ItermRelax,
            Feedforward,
            Chirp,
            DynNotch,
            DshotTelemetry,
            RpmFilter,
            Baro,
            Mag,
            RcSmoothing,
            SagCompensation,
            DynIdle,
            SimplifiedTuning,
            Gps,
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
    }

    /// The numbers of `docs/BLACKBOX-VERSIONS.md`: every line a release
    /// can write, and those of the stock build with a battery.
    #[test]
    fn the_size_of_the_tables() {
        let counts = Target::ALL.map(|t| {
            let mut build = stock_build(t);
            build.extend([Build::Vbat, Build::CurrentAdc]);
            (lines(t).len(), written_by(t, &build).len())
        });
        let expected = [
            (112, 112),
            (136, 136),
            (172, 172),
            (191, 191),
            (242, 174),
            (233, 176),
            (275, 180),
        ];
        assert_eq!(counts, expected);
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
        let first = |t: Target| lines(t)[0].value;
        assert_eq!(first(Target::V4_5), Some("Cleanflight"));
        assert_eq!(first(Target::V2025_12), Some("Betaflight"));
    }
}
