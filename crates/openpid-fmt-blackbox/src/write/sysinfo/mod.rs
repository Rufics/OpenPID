//! The system-information headers of each Betaflight release the writer
//! knows: the lines after the field definitions (`Firmware revision`,
//! `looptime`, the gains, the filters, …).
//!
//! One table per release holds every line that release can write, in the
//! firmware's order, with what the line depends on; the tables are data read
//! from the firmware source at the release's tag (each file names it). A
//! flight controller writes a line when its build has the option and, for a
//! few, when a run-time condition holds; [`Build`] names both kinds.

mod master;
mod v2025_12;
mod v2026_6;
mod v4_3;
mod v4_4;
mod v4_5;

use super::Target;

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

/// One header line of a release.
#[derive(Debug, Clone, Copy)]
pub(super) struct Line {
    /// Header name.
    pub name: &'static str,
    /// Every one of these must hold for the firmware to write the line.
    pub when: &'static [Build],
    /// The value of the example configuration; `None` where the writer has
    /// no verified one, so the line is written only for a session that
    /// carries it.
    pub value: Option<&'static str>,
}

/// A line with the value of the example configuration.
const fn line(name: &'static str, when: &'static [Build], value: &'static str) -> Line {
    Line {
        name,
        when,
        value: Some(value),
    }
}

/// A line the writer has no value for.
const fn open(name: &'static str, when: &'static [Build]) -> Line {
    Line {
        name,
        when,
        value: None,
    }
}

/// Every header line `target` can write, in the firmware's order.
pub(super) fn lines(target: Target) -> &'static [Line] {
    match target {
        Target::V4_3 => v4_3::LINES,
        Target::V4_4 => v4_4::LINES,
        Target::V4_5 => v4_5::LINES,
        Target::V2025_12 => v2025_12::LINES,
        Target::V2026_6 => v2026_6::LINES,
        Target::Master => master::LINES,
    }
}

/// The build options of the craft the example configuration describes: what
/// a release build for a common flight controller has. To 4.5 that is
/// everything (the official builds of those versions carried every feature);
/// from 2025.12, where each build is assembled from options, a quadcopter
/// with the usual ones and no GPS, compass, barometer or wing code.
pub(super) fn stock_build(target: Target) -> Vec<Build> {
    use Build::{
        AbsoluteControl, Acc, Baro, BoardInfo, DMinMax, DshotTelemetry, DynIdle, DynLpf, DynNotch,
        Feedforward, Gps, GpsRescue, IntegratedYaw, ItermRelax, Mag, NotWing, RcSmoothing,
        RpmFilter, SagCompensation, SimplifiedTuning,
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
        on.extend([AbsoluteControl, IntegratedYaw, Baro, Mag, Gps, GpsRescue]);
    }
    on
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
                "fields_disabled_mask",
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
