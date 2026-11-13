//! What each Betaflight release line logs, and how the reader has to take
//! it: one description per line, chosen from the `Firmware revision` header
//! of a log.
//!
//! Everything about a log that depends on the release is here and nowhere
//! else: the units of its fields, the names of its mode boxes, what the GPS
//! speed means, how its servos are grouped, the defaults of settings it does
//! not log. A header of the log wins where it states the fact (a log may
//! come from a custom build); the description says what holds when the
//! header is absent. `docs/BLACKBOX-VERSIONS.md` is the readable form.
//!
//! Every fact names where it was read: a tag of the firmware source
//! (`src/main/` of <https://github.com/betaflight/betaflight>) and the file,
//! or the fixture in `tests/logs/` that shows it. The source was read at the
//! tags 4.2.11, 4.3.2, 4.4.3, 4.5.0, 4.5.5, 2025.12.5, 2026.6.2 and at
//! `master` `4b1520e` (which calls itself 2026.12). No tag of 4.0 or 4.1
//! was at hand: what is said about them is what the reader assumed before,
//! and is marked so.

use openpid_core::{FirmwareInfo, FirmwareVendor, FirmwareVersion};

/// One Betaflight release line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Release {
    /// The line, as its versions begin: `4.5`, `2025.12`.
    pub name: &'static str,
    /// Major and minor number of its first version.
    pub since: (u32, u32),
    /// The line is not released: it is described from the development
    /// branch, and the release may differ.
    pub provisional: bool,

    // --- how frames are decoded -------------------------------------------
    /// The servo array of a main frame is one group of the 8-value encoding
    /// of its own (see [`crate::headers::Headers::servo_group`]).
    pub servo_group: bool,

    // --- units of the main frame fields -----------------------------------
    /// Volts per unit of `vbatLatest`.
    pub vbat_volts: f64,
    /// Amperes per unit of `amperageLatest`.
    pub amperage_amps: f64,
    /// What `rcCommand` holds when the log has no `rc_command_scale`
    /// header: `None` for ±500 on roll, pitch and yaw and 1000 … 2000 on
    /// the throttle; `Some(scale)` for a fraction of full deflection (−1 …
    /// 1; the throttle 0 … 1) times `scale`.
    pub rc_command_scale: Option<f64>,
    /// The line has the `blackbox_high_resolution` header: gyro, setpoint
    /// and `rcCommand` are logged ten times finer when it is 1.
    pub high_resolution: bool,
    /// `debug_mode` code of `GYRO_SCALED`, with which `debug[0..2]` carry
    /// the gyro before the software filters in deg/s; `None` when the line
    /// has no such mode.
    pub debug_gyro_scaled: Option<u32>,

    // --- flight modes -----------------------------------------------------
    /// The mode boxes of `flightModeFlags` in bit order (the slow frame
    /// carries the first 32 bits of the firmware's mode activation mask,
    /// bit n being box n of `boxId_e`).
    pub mode_boxes: &'static [&'static str],

    // --- GPS --------------------------------------------------------------
    /// `GPS_speed` is the 3D speed when the `gps_use_3d_speed` header is 1;
    /// otherwise, and always on a line where this is `false`, it is the
    /// speed over ground.
    pub gps_speed_3d: bool,

    // --- settings the log may not state -----------------------------------
    /// The filter settings have the names of 4.3 (`gyro_lpf1_static_hz`)
    /// and not the earlier ones (`gyro_lowpass_hz`).
    pub lpf1_names: bool,
    /// Curve of the dynamic low-pass (`*_lpf1_dyn_expo`) when the log does
    /// not have the header.
    pub dyn_lpf_expo: f64,
    /// Fade-in range of the RPM notches, Hz, when the log does not have
    /// `rpm_filter_fade_range_hz`.
    pub rpm_fade_range_hz: f64,
    /// Number of dynamic notches when the log does not have
    /// `dyn_notch_count`.
    pub dyn_notch_count: u32,
    /// The D gains are a base and a D-max above it (`d_max`); before, a
    /// D-min below the base (`d_min`). Decides the firmware defaults the
    /// sliders start from.
    pub d_max_scheme: bool,
}

/// Mode boxes of 4.0 and 4.1 as the reader has had them (from the
/// firmware's Blackbox Explorer); no tag of these lines was at hand. Bits 0
/// to 30 are those of 4.2; bit 31 is not verified.
const BOXES_4_0: [&str; 39] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "MAG",
    "HEADFREE",
    "PASSTHRU",
    "FAILSAFE",
    "GPSRESCUE",
    "ANTIGRAVITY",
    "HEADADJ",
    "CAMSTAB",
    "BEEPER",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "SERVO1",
    "SERVO2",
    "SERVO3",
    "BLACKBOX",
    "AIRMODE",
    "3D",
    "FPVANGLEMIX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "FLIPOVERAFTERCRASH",
    "PREARM",
    "BEEPGPSCOUNT",
    "VTXPITMODE",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
    "PIDAUDIO",
    "ACROTRAINER",
    "VTXCONTROLDISABLE",
    "LAUNCHCONTROL",
];

/// Mode boxes of 4.2 to 4.5: `boxId_e` of the tags 4.2.11, 4.3.2, 4.4.3 and
/// 4.5.5 (`fc/rc_modes.h`), the 32 boxes a log carries. Box 31 is PARALYZE;
/// the user boxes come after it and are not logged. (The reader had USER1
/// at bit 31, as the firmware's Blackbox Explorer names it.)
const BOXES_4_2: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "MAG",
    "HEADFREE",
    "PASSTHRU",
    "FAILSAFE",
    "GPSRESCUE",
    "ANTIGRAVITY",
    "HEADADJ",
    "CAMSTAB",
    "BEEPER",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "SERVO1",
    "SERVO2",
    "SERVO3",
    "BLACKBOX",
    "AIRMODE",
    "3D",
    "FPVANGLEMIX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "FLIPOVERAFTERCRASH",
    "PREARM",
    "BEEPGPSCOUNT",
    "VTXPITMODE",
    "PARALYZE",
];

/// Mode boxes of 2025.12: `boxId_e` of the tag 2025.12.5 (`fc/rc_modes.h`),
/// the 32 boxes a log carries. ALTHOLD, CHIRP and POSHOLD are new; the
/// fixture `betaflight-2025.12.2-btfl_027.bbl` shows ANGLE (bit 1) and
/// POSHOLD (bit 9) while armed.
const BOXES_2025_12: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "MAG",
    "ALTHOLD",
    "HEADFREE",
    "CHIRP",
    "PASSTHRU",
    "FAILSAFE",
    "POSHOLD",
    "GPSRESCUE",
    "ANTIGRAVITY",
    "HEADADJ",
    "CAMSTAB",
    "BEEPER",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "SERVO1",
    "SERVO2",
    "SERVO3",
    "BLACKBOX",
    "AIRMODE",
    "3D",
    "FPVANGLEMIX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "FLIPOVERAFTERCRASH",
    "PREARM",
];

/// Mode boxes of 2026.6 and of `master`: `boxId_e` of the tag 2026.6.2 and
/// of `4b1520e` (`fc/rc_modes.h`), the 32 boxes a log carries. AUTOPILOT
/// came in after GPSRESCUE, which pushed PREARM out of the 32 bits.
const BOXES_2026_6: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "MAG",
    "ALTHOLD",
    "HEADFREE",
    "CHIRP",
    "PASSTHRU",
    "FAILSAFE",
    "POSHOLD",
    "GPSRESCUE",
    "AUTOPILOT",
    "ANTIGRAVITY",
    "HEADADJ",
    "CAMSTAB",
    "BEEPER",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "SERVO1",
    "SERVO2",
    "SERVO3",
    "BLACKBOX",
    "AIRMODE",
    "3D",
    "FPVANGLEMIX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "FLIPOVERAFTERCRASH",
];

/// Betaflight before 4.0 and firmware without a version: the battery in
/// decivolts, no settings in words; its mode boxes depend on the minor
/// version and stay with the profile (`profiles::betaflight`). Not a
/// release line this module describes: it only keeps such logs reading as
/// they did.
pub const BEFORE_4_0: Release = Release {
    name: "before 4.0",
    since: (0, 0),
    provisional: false,
    servo_group: false,
    vbat_volts: 0.1,
    amperage_amps: 0.01,
    rc_command_scale: None,
    high_resolution: false,
    debug_gyro_scaled: None,
    mode_boxes: &[],
    gps_speed_3d: false,
    lpf1_names: false,
    dyn_lpf_expo: 0.0,
    rpm_fade_range_hz: 0.0,
    dyn_notch_count: 1,
    d_max_scheme: false,
};

/// 4.0. No tag at hand and no fixture: as the reader assumed before.
const V4_0: Release = Release {
    name: "4.0",
    since: (4, 0),
    // `vbatLatest` in 0.01 V and `amperageLatest` in 0.01 A:
    // `sensors/voltage.h` and `sensors/current.h` of the tags 4.2.11 and
    // 4.3.2 say "0.01V steps" and "centiampere"; the 4.2 fixtures show it
    // (`vbatref` 2277 on a 6S pack in `betaflight-4.2.0.BFL`). Assumed for
    // 4.0 and 4.1.
    vbat_volts: 0.01,
    // code 6 is `DEBUG_GYRO_SCALED` in `build/debug.h` of the tags 4.2.11
    // to 4.5.5; `betaflight-4.2.0.BFL` flew with it. Assumed for 4.0 and
    // 4.1.
    debug_gyro_scaled: Some(6),
    mode_boxes: &BOXES_4_0,
    ..BEFORE_4_0
};

/// 4.1. No tag at hand and no fixture: as 4.0.
const V4_1: Release = Release {
    name: "4.1",
    since: (4, 1),
    ..V4_0
};

/// 4.2. Tag 4.2.11; fixtures `betaflight-4.2.0.BFL`,
/// `betaflight-4.2.8-44-logs.bbl`, `betaflight-4.2.11-*.bbl`.
const V4_2: Release = Release {
    name: "4.2",
    since: (4, 2),
    // the dynamic low-pass has a curve with a default of 5: `flight/pid.c`
    // of the tag 4.2.11 (`dyn_lpf_curve_expo = 5`); the tag 4.3.2 sets
    // `gyro_lpf1_dyn_expo = 5` (`sensors/gyro.c`) and `dterm_lpf1_dyn_expo
    // = 5` (`flight/pid.c`). The header came in 4.5 (tag 4.5.5,
    // `blackbox.c`). That 4.0 and 4.1 have no curve is what the reader
    // assumed before.
    dyn_lpf_expo: 5.0,
    mode_boxes: &BOXES_4_2,
    ..V4_1
};

/// 4.3. Tag 4.3.2.
const V4_3: Release = Release {
    name: "4.3",
    since: (4, 3),
    // `blackbox.c`: the filter headers are `gyro_lpf1_static_hz` and so on,
    // where 4.2 logs `gyro_lowpass_hz` (fixtures)
    lpf1_names: true,
    // `blackbox.c` logs `rpm_filter_fade_range_hz` and `dyn_notch_count`;
    // the values are the defaults of `flight/rpm_filter.c` (50) and
    // `pg/dyn_notch.c` (3), for a log whose header block lost them
    rpm_fade_range_hz: 50.0,
    dyn_notch_count: 3,
    ..V4_2
};

/// 4.4. Tag 4.4.3.
const V4_4: Release = Release {
    name: "4.4",
    since: (4, 4),
    // `blackbox.c`: `blackboxHighResolutionScale` is 10 with the setting,
    // applied to gyro, setpoint[0..2] and rcCommand, and the header is
    // written. The same file writes `gps_use_3d_speed` from here on, but
    // logs `gpsSol.groundSpeed` whatever it says (so does the tag 4.5.5).
    high_resolution: true,
    ..V4_3
};

/// 4.5. Tags 4.5.0 and 4.5.5; fixtures `betaflight-4.5.0-gps-rescue.bbl`
/// and `betaflight-4.5.1-two-sessions.bbl`.
const V4_5: Release = Release {
    name: "4.5",
    since: (4, 5),
    ..V4_4
};

/// 4.6, the name the development branch had before it was renumbered to
/// 2025.12. No tag at hand and no fixture: as the reader had it, 4.5
/// without the `GYRO_SCALED` debug mode.
const V4_6: Release = Release {
    name: "4.6",
    since: (4, 6),
    provisional: true,
    debug_gyro_scaled: None,
    ..V4_5
};

/// 2025.12. Tag 2025.12.5; fixture `betaflight-2025.12.2-btfl_027.bbl`.
const V2025_12: Release = Release {
    name: "2025.12",
    since: (2025, 12),
    provisional: false,
    // `blackbox.c`: `servo[0..7]` with the 8-value encoding, written by a
    // call of their own in `writeInterframe`
    servo_group: true,
    // `build/debug.h`: code 6 is `RC_INTERPOLATION`; there is no
    // `GYRO_SCALED` (the gyro before the filters is the field `gyroUnfilt`)
    debug_gyro_scaled: None,
    mode_boxes: &BOXES_2025_12,
    // `blackbox.c`, `writeGPSFrame`: `gpsSol.speed3d` when
    // `gpsConfig()->gps_use_3d_speed`, else `gpsSol.groundSpeed`
    gps_speed_3d: true,
    // `blackbox.c` logs `d_max` and no `d_min`; the fixture has `d_max`
    d_max_scheme: true,
    ..V4_5
};

/// 2026.6. Tag 2026.6.2; no fixture.
const V2026_6: Release = Release {
    name: "2026.6",
    since: (2026, 6),
    mode_boxes: &BOXES_2026_6,
    ..V2025_12
};

/// 2026.12, the name `master` has at `4b1520e`; not released, no fixture.
const V2026_12: Release = Release {
    name: "2026.12",
    since: (2026, 12),
    provisional: true,
    // `blackbox.c`: `rcCommand[i] * BLACKBOX_RC_COMMAND_SCALE` (1000), and
    // the header `rc_command_scale`; `fc/rc_controls.c`: rcCommand is −1 …
    // 1, the throttle 0 … 1
    rc_command_scale: Some(1000.0),
    ..V2026_6
};

/// The release lines from 4.0 on, oldest first.
pub const LINES: [Release; 10] = [
    V4_0, V4_1, V4_2, V4_3, V4_4, V4_5, V4_6, V2025_12, V2026_6, V2026_12,
];

impl Release {
    /// Whether this is the stand-in for Betaflight before 4.0 and firmware
    /// without a version.
    pub fn is_before_4_0(&self) -> bool {
        self.since < (4, 0)
    }

    /// The line a version number falls in: the last one that begins at or
    /// before it; [`BEFORE_4_0`] for an earlier number and for none.
    ///
    /// The forks of Betaflight are read with this too, by their own version
    /// numbers, for the unit of the battery voltage and the three filter
    /// defaults: that is what the reader has always assumed for them (a
    /// Rotorflight 4.3 log does not state its dynamic notch count either).
    /// Their profiles replace what differs.
    pub fn of_number(version: Option<FirmwareVersion>) -> &'static Release {
        let number = version.map_or((0, 0), |v| (v.major, v.minor));
        LINES
            .iter()
            .rev()
            .find(|r| r.since <= number)
            .unwrap_or(&BEFORE_4_0)
    }
}

/// The description a Betaflight log is read with, and what there is to say
/// about the choice.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// The release line.
    pub release: &'static Release,
    /// Set when the version of the log is not one this module describes
    /// from a release: for the session's diagnostics.
    pub note: Option<String>,
}

/// Choose the description for a log of Betaflight (or of Cleanflight, which
/// is read as Betaflight before 4.0) from its firmware revision.
///
/// A version newer than every line is read as the newest line; one between
/// two lines as the earlier of them. Neither fails: the note says what was
/// assumed.
pub fn select(firmware: &FirmwareInfo) -> Selection {
    let release = Release::of_number(firmware.version);
    let newest = &LINES[LINES.len() - 1];
    let note = match firmware.version {
        _ if firmware.vendor != FirmwareVendor::Betaflight => None,
        None => Some(
            "the log does not say which Betaflight version wrote it: it is read as Betaflight \
             before 4.0"
                .to_string(),
        ),
        Some(_) if release.is_before_4_0() => None,
        Some(v) if (v.major, v.minor) != release.since && std::ptr::eq(release, newest) => {
            Some(format!(
                "Betaflight {v} is newer than the release lines this reader knows (up to {}): \
                 it is read as {}; units, modes and fields that changed since are not known",
                newest.name, newest.name
            ))
        }
        Some(v) if (v.major, v.minor) != release.since => Some(format!(
            "Betaflight {v} is not a release line this reader knows: it is read as {}",
            release.name
        )),
        Some(v) if release.provisional => Some(format!(
            "Betaflight {v} is not a released version: the {} line is described from the \
             development branch and the release may differ",
            release.name
        )),
        Some(_) => None,
    };
    Selection { release, note }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn betaflight(revision: &str) -> FirmwareInfo {
        FirmwareInfo::parse_revision(revision)
    }

    #[test]
    fn the_line_is_chosen_from_the_version() {
        for (revision, line) in [
            ("Betaflight 4.0.6 (2a64051a2) STM32F405", "4.0"),
            ("Betaflight 4.1.7 (f4ee8b0d9) STM32F7X2", "4.1"),
            ("Betaflight 4.2.11 (948ba6339) STM32F7X2", "4.2"),
            ("Betaflight 4.3.2 (f156481e9) STM32F405", "4.3"),
            ("Betaflight 4.4.3 (738127e7e) STM32H743", "4.4"),
            ("Betaflight 4.5.2 (024f8e13d) STM32F7X2", "4.5"),
            ("Betaflight 2025.12.2 (c6b3f0a) STM32F7X2", "2025.12"),
            ("Betaflight 2025.12.0-RC4 (37800445d) STM32F7X2", "2025.12"),
            ("Betaflight 2026.6.1 (0123abc) STM32H743", "2026.6"),
        ] {
            let s = select(&betaflight(revision));
            assert_eq!(s.release.name, line, "{revision}");
            assert_eq!(s.note, None, "{revision}");
        }
        // before 4.0 and Cleanflight: the stand-in, and nothing to say
        for revision in [
            "Betaflight 3.5.7 (e9130527c) OMNIBUSF4SD",
            "Cleanflight 2.5.0 (abc) SPRACINGF3",
        ] {
            let s = select(&betaflight(revision));
            assert!(s.release.is_before_4_0(), "{revision}");
            assert_eq!(s.note, None, "{revision}");
        }
    }

    #[test]
    fn a_version_the_reader_does_not_know_falls_back_and_says_so() {
        // newer than every line: the newest one
        let s = select(&betaflight("Betaflight 2027.6.0 (fedcba9) STM32H743"));
        assert_eq!(s.release.name, "2026.12");
        let note = s.note.expect("a note");
        assert!(
            note.contains("2027.6.0 is newer than the release lines this reader knows")
                && note.contains("read as 2026.12"),
            "{note}"
        );
        // between two lines: the earlier one
        let s = select(&betaflight("Betaflight 2026.9.0 (fedcba9) STM32H743"));
        assert_eq!(s.release.name, "2026.6");
        assert!(
            s.note
                .expect("a note")
                .contains("not a release line this reader knows: it is read as 2026.6")
        );
        // a line known from the development branch only
        for (revision, line) in [
            ("Betaflight 2026.12.0 (4b1520e) STM32H743", "2026.12"),
            ("Betaflight 4.6.0 (abcdef0) STM32F7X2", "4.6"),
        ] {
            let s = select(&betaflight(revision));
            assert_eq!(s.release.name, line);
            assert!(
                s.note
                    .expect("a note")
                    .contains("described from the development branch"),
                "{revision}"
            );
        }
        // no version at all
        let s = select(&betaflight("Betaflight"));
        assert!(s.release.is_before_4_0());
        assert!(s.note.expect("a note").contains("does not say which"));
        // other firmwares get the line of their number, and no remark
        let s = select(&betaflight("INAV 9.0.1 (abc) MATEKF405"));
        assert_eq!((s.release.name, s.note), ("4.6", None));
    }

    #[test]
    fn the_lines_are_in_order_and_change_what_their_sources_say() {
        assert!(LINES.windows(2).all(|w| w[0].since < w[1].since));
        let line = |name: &str| {
            *LINES
                .iter()
                .find(|r| r.name == name)
                .unwrap_or_else(|| panic!("no line {name}"))
        };
        // centivolts from 4.0 on
        assert!(LINES.iter().all(|r| r.vbat_volts == 0.01));
        assert_eq!(BEFORE_4_0.vbat_volts, 0.1);
        // the filter defaults
        assert_eq!(
            (line("4.1").dyn_lpf_expo, line("4.2").dyn_lpf_expo),
            (0.0, 5.0)
        );
        assert_eq!(
            (line("4.2").rpm_fade_range_hz, line("4.2").dyn_notch_count),
            (0.0, 1)
        );
        assert_eq!(
            (line("4.3").rpm_fade_range_hz, line("4.3").dyn_notch_count),
            (50.0, 3)
        );
        assert!(!line("4.2").lpf1_names && line("4.3").lpf1_names);
        assert!(!line("4.3").high_resolution && line("4.4").high_resolution);
        // GYRO_SCALED is debug mode 6 from 4.0 to 4.5
        assert_eq!(line("4.5").debug_gyro_scaled, Some(6));
        assert_eq!(line("4.6").debug_gyro_scaled, None);
        assert_eq!(line("2025.12").debug_gyro_scaled, None);
        // 2025.12: D-max, the servo group, the 3D speed, new boxes
        assert!(!line("4.5").d_max_scheme && line("2025.12").d_max_scheme);
        assert!(!line("4.5").servo_group && line("2025.12").servo_group);
        assert!(!line("4.5").gps_speed_3d && line("2025.12").gps_speed_3d);
        assert_eq!(line("4.5").mode_boxes[4], "HEADFREE");
        // 4.2 to 4.5: the 32nd box is PARALYZE, not the first user box
        for name in ["4.2", "4.3", "4.4", "4.5"] {
            assert_eq!(line(name).mode_boxes.len(), 32, "{name}");
            assert_eq!(line(name).mode_boxes[30..], ["VTXPITMODE", "PARALYZE"]);
        }
        assert_eq!(line("2025.12").mode_boxes[4], "ALTHOLD");
        assert_eq!(line("2025.12").mode_boxes[9], "POSHOLD");
        // 2026.6: AUTOPILOT after GPSRESCUE
        assert_eq!(
            line("2026.6").mode_boxes[10..12],
            ["GPSRESCUE", "AUTOPILOT"]
        );
        // after 2026.6: rcCommand as a fraction
        assert_eq!(line("2026.6").rc_command_scale, None);
        assert_eq!(line("2026.12").rc_command_scale, Some(1000.0));
        assert!(line("2026.12").provisional && !line("2026.6").provisional);
    }
}
