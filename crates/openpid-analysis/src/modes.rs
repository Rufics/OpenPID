//! The flight modes active at a moment of the log (the slow frames'
//! `flightModeFlags`), for the Log viewer (docs/design/log-sticks).
//!
//! On the Betaflight family the field holds the mode-box activation mask:
//! bit `n` is box `n` of the firmware's box list, which changed with the
//! versions below. The names are the boxes' names as Betaflight and Blackbox
//! Explorer print them.

use crate::sticks::sample_at;
use openpid_core::session::FirmwareVendor;
use openpid_core::{FirmwareInfo, FirmwareVersion, FlightSession};

/// Boxes before Betaflight 3.3 (and on the other Cleanflight forks).
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

/// Boxes of Betaflight 3.3 to 2025.12 (some removed by the versions below).
const BOXES_3_3: [&str; 48] = [
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

/// Boxes of Betaflight 2025.12 on.
const BOXES_2025_12: [&str; 42] = [
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

/// The box names of `fw` in bit order; `None` where the field means
/// something else (INAV, Rotorflight, `ArduPilot`, unknown firmware).
pub fn flight_mode_names(fw: &FirmwareInfo) -> Option<Vec<&'static str>> {
    let v = fw.version.unwrap_or(FirmwareVersion::new(0, 0, 0));
    let at_least = |major, minor, patch| v >= FirmwareVersion::new(major, minor, patch);
    match fw.vendor {
        FirmwareVendor::Betaflight if at_least(3, 3, 0) => {
            let mut names: Vec<&'static str> = if at_least(2025, 12, 0) {
                BOXES_2025_12.to_vec()
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
                BOXES_3_3
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
            Some(names)
        }
        FirmwareVendor::Betaflight | FirmwareVendor::Cleanflight | FirmwareVendor::Emuflight => {
            let old_betaflight = fw.vendor == FirmwareVendor::Betaflight && !at_least(3, 1, 7);
            Some(
                BOXES_PRE_3_3
                    .iter()
                    .copied()
                    .filter(|n| !(old_betaflight && *n == "ANTIGRAVITY"))
                    .collect(),
            )
        }
        _ => None,
    }
}

/// The names of the bits set in `flags`; a bit without a name prints as `bit n`.
pub fn decode_flight_modes(flags: i64, names: &[&str]) -> Vec<String> {
    (0..64)
        .filter(|b| (flags >> b) & 1 == 1)
        .map(|b| {
            names
                .get(b as usize)
                .map_or_else(|| format!("bit {b}"), |n| (*n).to_string())
        })
        .collect()
}

/// The flight modes active at `t_s` seconds from the start of the session:
/// the last slow frame at or before that moment. `None` when the log has
/// no flight-mode field or the firmware gives it another meaning.
pub fn flight_modes_at(session: &FlightSession, t_s: f64) -> Option<Vec<String>> {
    let names = flight_mode_names(&session.firmware)?;
    let slow = session.aux.iter().find(|t| t.kind == "slow")?;
    let col = slow.columns.iter().position(|c| c == "flightModeFlags")?;
    let i = sample_at(session, t_s)?;
    let row = slow
        .rows
        .iter()
        .take_while(|r| r.sample_index <= i)
        .last()
        .or_else(|| slow.rows.first())?;
    Some(decode_flight_modes(*row.values.get(col)?, &names))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]
    use super::*;
    use openpid_core::{LogFormat, OpenOptions};
    use openpid_fmt_blackbox::BlackboxFormat;

    fn fw(vendor: FirmwareVendor, major: u32, minor: u32, patch: u32) -> FirmwareInfo {
        FirmwareInfo {
            vendor,
            version: Some(FirmwareVersion::new(major, minor, patch)),
            ..FirmwareInfo::unknown()
        }
    }

    #[test]
    fn box_names_follow_the_firmware_version() {
        let names = |f: &FirmwareInfo| flight_mode_names(f).unwrap();
        let bf = FirmwareVendor::Betaflight;
        // 2025.12: ALTHOLD fourth, POSHOLD and GPSRESCUE after FAILSAFE
        let n = names(&fw(bf.clone(), 2025, 12, 2));
        assert_eq!(&n[..5], ["ARM", "ANGLE", "HORIZON", "MAG", "ALTHOLD"]);
        assert_eq!(n[23], "AIRMODE");
        // 4.x: BARO, GPSHOME, GPSHOLD and the 3.5 removals gone
        let n = names(&fw(bf.clone(), 4, 5, 1));
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
        let n = names(&fw(bf.clone(), 3, 3, 0));
        assert!(!n.contains(&"GPSRESCUE") && n.contains(&"RANGEFINDER"));
        // 2026.6: AUTOPILOT after GPSRESCUE
        let n = names(&fw(bf.clone(), 2026, 6, 0));
        let g = n.iter().position(|x| *x == "GPSRESCUE").unwrap();
        assert_eq!(n[g + 1], "AUTOPILOT");
        // older Betaflight and the other Cleanflight forks: the old list
        assert_eq!(names(&fw(bf.clone(), 3, 2, 0))[3], "BARO");
        assert_eq!(
            names(&fw(bf, 3, 1, 6))[4],
            "MAG",
            "no ANTIGRAVITY up to 3.1.6"
        );
        assert_eq!(
            names(&fw(FirmwareVendor::Emuflight, 0, 4, 0))[4],
            "ANTIGRAVITY"
        );
        // INAV gives the field another meaning
        assert!(flight_mode_names(&fw(FirmwareVendor::Inav, 7, 0, 0)).is_none());
        assert_eq!(
            decode_flight_modes(0b11 | 1 << 50, &["ARM", "ANGLE"]),
            ["ARM", "ANGLE", "bit 50"]
        );
    }

    #[test]
    fn modes_of_a_real_flight() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/logs/betaflight-2025.12.2-btfl_027.bbl");
        let log = BlackboxFormat::new()
            .open(&std::fs::read(path).unwrap(), &OpenOptions::default())
            .unwrap();
        let s = &log.sessions[0];
        let d = s.timebase.duration_s();
        let mut seen: Vec<String> = Vec::new();
        for k in 0..100 {
            for m in flight_modes_at(s, d * f64::from(k) / 100.0).unwrap() {
                if !seen.contains(&m) {
                    seen.push(m);
                }
            }
        }
        // armed in ANGLE, then POSHOLD from 12.8 s (0x3, then 0x201)
        assert_eq!(seen, ["ARM", "ANGLE", "POSHOLD"]);
        assert!(seen.iter().all(|m| !m.starts_with("bit ")), "{seen:?}");
    }
}
