//! Flight modes from the slow frames' `flightModeFlags` (docs/design/log-sticks).
//!
//! On the Cleanflight family the field holds the mode-box activation mask:
//! bit `n` is box `n` of the firmware's box list, and the lists differ per
//! firmware and changed with the versions below. The names are the boxes'
//! names as the firmware and its Blackbox Explorer print them; they become
//! [`FlightMode`] values here, so nothing after the decoder reads the bits.

use openpid_core::modes::{FlightMode, ModeChange, push_change};
use openpid_core::session::{AuxTable, FirmwareVendor};
use openpid_core::{FirmwareInfo, FirmwareVersion};

/// Betaflight before 3.3, Cleanflight, Baseflight.
const BOXES_PRE_3_3: [&str; 37] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "BARO",
    "ANTIGRAVITY",
    "MAG",
    "HEADFREE",
    "HEADADJ",
    "CAMSTAB",
    "CAMTRIG",
    "GPSHOME",
    "GPSHOLD",
    "PASSTHRU",
    "BEEPER",
    "LEDMAX",
    "LEDLOW",
    "LLIGHTS",
    "CALIB",
    "GOV",
    "OSD",
    "TELEMETRY",
    "GTUNE",
    "SONAR",
    "SERVO1",
    "SERVO2",
    "SERVO3",
    "BLACKBOX",
    "FAILSAFE",
    "AIRMODE",
    "3DDISABLE",
    "FPVANGLEMIX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "FLIPOVERAFTERCRASH",
    "PREARM",
];

/// Betaflight 3.3 to 2025.12, before the removals of later versions.
const BOXES_BF_3_3: [&str; 48] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "MAG",
    "BARO",
    "GPSHOME",
    "GPSHOLD",
    "HEADFREE",
    "PASSTHRU",
    "RANGEFINDER",
    "FAILSAFE",
    "GPSRESCUE",
    "ANTIGRAVITY",
    "HEADADJ",
    "CAMSTAB",
    "CAMTRIG",
    "BEEPER",
    "LEDMAX",
    "LEDLOW",
    "LLIGHTS",
    "CALIB",
    "GOV",
    "OSD",
    "TELEMETRY",
    "GTUNE",
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

/// Betaflight 2025.12 on.
const BOXES_BF_2025_12: [&str; 42] = [
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

/// INAV: its RC modes by box id, the first 32 the field holds.
const BOXES_INAV: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "NAV ALTHOLD",
    "HEADING HOLD",
    "HEADFREE",
    "HEADADJ",
    "CAMSTAB",
    "NAV RTH",
    "NAV POSHOLD",
    "MANUAL",
    "BEEPER",
    "LEDS OFF",
    "LIGHTS",
    "NAV LAUNCH",
    "OSD OFF",
    "TELEMETRY",
    "BLACKBOX",
    "FAILSAFE",
    "NAV WP",
    "AIR MODE",
    "HOME RESET",
    "GCS NAV",
    "KILLSWITCH",
    "SURFACE",
    "FLAPERON",
    "TURN ASSIST",
    "SERVO AUTOTRIM",
    "AUTO TUNE",
    "CAMERA CONTROL 1",
    "CAMERA CONTROL 2",
    "CAMERA CONTROL 3",
];

/// Rotorflight 4.2.
const BOXES_RF_4_2: [&str; 31] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "PASSTHRU",
    "FAILSAFE",
    "RESCUE",
    "GPSRESCUE",
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
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "PREARM",
    "BEEPGPSCOUNT",
    "VTXPITMODE",
    "PARALYZE",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
    "ACROTRAINER",
    "VTXCONTROLDISABLE",
];

/// Rotorflight 4.3 to 4.5.
const BOXES_RF_4_3: [&str; 29] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "TRAINER",
    "ALTHOLD",
    "RESCUE",
    "GPSRESCUE",
    "FAILSAFE",
    "PREARM",
    "PARALYZE",
    "BEEPERON",
    "BEEPERMUTE",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "BEEPGPSCOUNT",
    "BLACKBOX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "VTXPITMODE",
    "VTXCONTROLDISABLE",
    "STICKCOMMANDDISABLE",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
];

/// Rotorflight 4.6 on: the governor boxes before the user ones.
const BOXES_RF_4_6: [&str; 32] = [
    "ARM",
    "ANGLE",
    "HORIZON",
    "TRAINER",
    "ALTHOLD",
    "RESCUE",
    "GPSRESCUE",
    "FAILSAFE",
    "PREARM",
    "PARALYZE",
    "BEEPERON",
    "BEEPERMUTE",
    "LEDLOW",
    "CALIB",
    "OSD",
    "TELEMETRY",
    "BEEPGPSCOUNT",
    "BLACKBOX",
    "BLACKBOXERASE",
    "CAMERA1",
    "CAMERA2",
    "CAMERA3",
    "VTXPITMODE",
    "VTXCONTROLDISABLE",
    "STICKCOMMANDDISABLE",
    "GOVFALLBACK",
    "GOVSUSPEND",
    "GOVBYPASS",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
];

/// The box names of `fw` in bit order; empty where they are not known (a
/// bit then reads `bit n`).
pub fn box_names(fw: &FirmwareInfo) -> Vec<&'static str> {
    let v = fw.version.unwrap_or(FirmwareVersion::new(0, 0, 0));
    let at_least = |major, minor, patch| v >= FirmwareVersion::new(major, minor, patch);
    match fw.vendor {
        FirmwareVendor::Betaflight if at_least(3, 3, 0) => {
            let mut names: Vec<&'static str> = if at_least(2025, 12, 0) {
                BOXES_BF_2025_12.to_vec()
            } else {
                let mut removed: Vec<&str> = Vec::new();
                if !at_least(3, 4, 0) {
                    removed.push("GPSRESCUE");
                }
                if at_least(3, 5, 0) {
                    removed.extend([
                        "RANGEFINDER",
                        "CAMTRIG",
                        "LEDMAX",
                        "LLIGHTS",
                        "GOV",
                        "GTUNE",
                    ]);
                }
                if at_least(4, 0, 0) {
                    removed.extend(["BARO", "GPSHOME", "GPSHOLD"]);
                }
                BOXES_BF_3_3
                    .iter()
                    .copied()
                    .filter(|n| !removed.contains(n))
                    .collect()
            };
            if at_least(2026, 6, 0)
                && let Some(i) = names.iter().position(|n| *n == "GPSRESCUE")
            {
                names.insert(i + 1, "AUTOPILOT");
            }
            names
        }
        FirmwareVendor::Inav => BOXES_INAV.to_vec(),
        FirmwareVendor::Rotorflight if at_least(4, 6, 0) => BOXES_RF_4_6.to_vec(),
        FirmwareVendor::Rotorflight if at_least(4, 3, 0) => BOXES_RF_4_3.to_vec(),
        FirmwareVendor::Rotorflight if at_least(4, 2, 0) => BOXES_RF_4_2.to_vec(),
        // EmuFlight's own Explorer names none of its boxes, and its list is
        // not Betaflight's of the same year; every Cleanflight fork keeps
        // ARM on bit 0, the other bits stay numbered
        FirmwareVendor::Rotorflight | FirmwareVendor::Emuflight => vec!["ARM"],
        FirmwareVendor::Ardupilot => Vec::new(),
        _ => {
            let old_betaflight = fw.vendor == FirmwareVendor::Betaflight && !at_least(3, 1, 7);
            BOXES_PRE_3_3
                .iter()
                .copied()
                .filter(|n| !(old_betaflight && *n == "ANTIGRAVITY"))
                .collect()
        }
    }
}

/// The modes of the bits set in `flags`.
pub fn decode(flags: i64, names: &[&str]) -> Vec<FlightMode> {
    (0..64)
        .filter(|b| (flags >> b) & 1 == 1)
        .map(|b| match names.get(b as usize) {
            Some(n) => FlightMode::from_name(n),
            None => FlightMode::Other(format!("bit {b}")),
        })
        .collect()
}

/// The mode changes of the slow frames' `flightModeFlags`; empty when the
/// log has no such field.
pub fn changes(fw: &FirmwareInfo, slow: &AuxTable) -> Vec<ModeChange> {
    let Some(col) = slow.columns.iter().position(|c| c == "flightModeFlags") else {
        return Vec::new();
    };
    let names = box_names(fw);
    let mut out = Vec::new();
    for row in &slow.rows {
        if let Some(flags) = row.values.get(col) {
            push_change(&mut out, row.sample_index, decode(*flags, &names));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fw(vendor: FirmwareVendor, major: u32, minor: u32, patch: u32) -> FirmwareInfo {
        FirmwareInfo {
            vendor,
            version: Some(FirmwareVersion::new(major, minor, patch)),
            ..FirmwareInfo::unknown()
        }
    }

    #[test]
    fn box_names_follow_the_firmware_and_its_version() {
        let bf = FirmwareVendor::Betaflight;
        // 2025.12: ALTHOLD fourth, POSHOLD after FAILSAFE
        let n = box_names(&fw(bf.clone(), 2025, 12, 2));
        assert_eq!(&n[..5], ["ARM", "ANGLE", "HORIZON", "MAG", "ALTHOLD"]);
        assert_eq!(n[9], "POSHOLD");
        // 4.x: BARO, GPSHOME, GPSHOLD and the 3.5 removals gone
        let n = box_names(&fw(bf.clone(), 4, 5, 1));
        assert_eq!(
            &n[..8],
            [
                "ARM",
                "ANGLE",
                "HORIZON",
                "MAG",
                "HEADFREE",
                "PASSTHRU",
                "FAILSAFE",
                "GPSRESCUE"
            ]
        );
        assert!(!n.contains(&"GTUNE") && n.contains(&"AIRMODE"));
        // 3.3: no GPSRESCUE yet, RANGEFINDER still there
        let n = box_names(&fw(bf.clone(), 3, 3, 0));
        assert!(!n.contains(&"GPSRESCUE") && n.contains(&"RANGEFINDER"));
        // 2026.6: AUTOPILOT after GPSRESCUE
        let n = box_names(&fw(bf.clone(), 2026, 6, 0));
        let g = n.iter().position(|x| *x == "GPSRESCUE").unwrap_or(0);
        assert_eq!(n[g + 1], "AUTOPILOT");
        // early Betaflight and the other Cleanflight forks: the old list
        assert_eq!(box_names(&fw(bf.clone(), 3, 2, 0))[3], "BARO");
        assert_eq!(
            box_names(&fw(bf, 3, 1, 6))[4],
            "MAG",
            "no ANTIGRAVITY to 3.1.6"
        );
        assert_eq!(
            box_names(&fw(FirmwareVendor::Cleanflight, 2, 0, 0))[4],
            "ANTIGRAVITY"
        );
        assert_eq!(box_names(&fw(FirmwareVendor::Emuflight, 3, 7, 0)), ["ARM"]);
        // INAV and Rotorflight have lists of their own
        assert_eq!(
            box_names(&fw(FirmwareVendor::Inav, 9, 0, 1))[9],
            "NAV POSHOLD"
        );
        assert_eq!(
            box_names(&fw(FirmwareVendor::Rotorflight, 4, 3, 0))[5],
            "RESCUE"
        );
        assert_eq!(
            box_names(&fw(FirmwareVendor::Rotorflight, 4, 6, 0))[25],
            "GOVFALLBACK"
        );
        assert_eq!(
            box_names(&fw(FirmwareVendor::Rotorflight, 4, 2, 0))[3],
            "PASSTHRU"
        );
        assert_eq!(
            box_names(&fw(FirmwareVendor::Rotorflight, 4, 1, 0)),
            ["ARM"]
        );
    }

    #[test]
    fn bits_become_common_modes() {
        let inav = box_names(&fw(FirmwareVendor::Inav, 9, 0, 1));
        assert_eq!(
            decode(1 | 1 << 9 | 1 << 20, &inav),
            [FlightMode::Arm, FlightMode::PosHold, FlightMode::AirMode]
        );
        assert_eq!(decode(1 << 40, &inav), [FlightMode::Other("bit 40".into())]);
        assert_eq!(
            decode(1 << 13, &BOXES_RF_4_3),
            [FlightMode::Other("CALIB".into())]
        );
    }
}
