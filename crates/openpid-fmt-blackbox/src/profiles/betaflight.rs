//! Betaflight (and Cleanflight) profile.

use super::common::{self, Log};
use crate::cli::BetaflightCli;
use crate::releases::{self, Release, Selection};
use openpid_core::{
    BoundProfile, Exporter, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor,
    FirmwareVersion, FlightMode, FlightSession, RawHeaders, Settings, SliderModel,
};

/// Mode boxes of Betaflight before 3.3 and of Cleanflight, in bit order.
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

/// Mode boxes of Betaflight 3.3 to 3.5, before the removals of its later
/// versions.
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

/// The mode boxes of `fw` in the bit order of `flightModeFlags`. From 4.0
/// on they are a fact of the release line ([`Release::mode_boxes`]); for
/// Betaflight before 4.0 and for Cleanflight, which have no release line
/// here, they are the lists the firmware's Blackbox Explorer names.
fn box_names(fw: &FirmwareInfo, release: &Release) -> Vec<&'static str> {
    if fw.vendor == FirmwareVendor::Betaflight && !release.is_before_4_0() {
        return release.mode_boxes.to_vec();
    }
    let v = fw.version.unwrap_or(FirmwareVersion::new(0, 0, 0));
    let at_least = |major, minor, patch| v >= FirmwareVersion::new(major, minor, patch);
    if fw.vendor != FirmwareVendor::Betaflight || !at_least(3, 3, 0) {
        // Betaflight gained ANTIGRAVITY after 3.1.6; Cleanflight has it
        let without = fw.vendor == FirmwareVendor::Betaflight && !at_least(3, 1, 7);
        return BOXES_PRE_3_3
            .iter()
            .copied()
            .filter(|n| !(without && *n == "ANTIGRAVITY"))
            .collect();
    }
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
    BOXES_BF_3_3
        .iter()
        .copied()
        .filter(|n| !removed.contains(n))
        .collect()
}

#[derive(Debug, Default, Clone, Copy)]
/// Betaflight 4.x and 2025.12+ (also used for Cleanflight).
pub struct Betaflight;

impl FirmwareProfile for Betaflight {
    fn id(&self) -> &'static str {
        "betaflight"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        match fw.vendor {
            FirmwareVendor::Betaflight => Some(100),
            FirmwareVendor::Cleanflight => Some(60),
            _ => None,
        }
    }

    fn bind<'a>(&self, fw: &'a FirmwareInfo, h: &'a RawHeaders) -> Box<dyn BoundProfile + 'a> {
        // the release line of the log: everything that depends on the
        // version is read from it
        let Selection { release, note } = releases::select(fw);
        let log = Log::new(fw, h, box_names(fw, release));
        debug_assert!(std::ptr::eq(log.release, release));
        Box::new(Bound { log, note })
    }
}

/// The profile of one Betaflight log.
struct Bound<'a> {
    log: Log<'a>,
    /// What there is to say about the release line the log is read as.
    note: Option<String>,
}

impl BoundProfile for Bound<'_> {
    fn map_field(&self, field: &str) -> Option<FieldMapping> {
        self.log.map_field(field)
    }

    fn settings(&self) -> Settings {
        let mut s = self.log.settings();
        // Cleanflight is read with this profile too; the words are Betaflight's
        if self.log.fw.vendor == FirmwareVendor::Betaflight {
            s.details = super::details::betaflight(self.log.fw, self.log.h);
        }
        s
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
        session.diagnostics.notes.extend(self.note.clone());
    }

    fn flight_modes(&self, word: i64) -> Vec<FlightMode> {
        self.log.flight_modes(word)
    }

    fn slider_model(&self) -> Option<Box<dyn SliderModel>> {
        super::sliders::BetaflightSliders::from_headers(self.log.fw, self.log.h)
            .map(|m| Box::new(m) as Box<dyn SliderModel>)
    }

    fn exporter(&self) -> Option<Box<dyn Exporter>> {
        // Cleanflight is read with this profile too, but its CLI has other
        // setting names
        (self.log.fw.vendor == FirmwareVendor::Betaflight)
            .then(|| Box::new(BetaflightCli::for_log(self.log.fw, self.log.h)) as Box<dyn Exporter>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_names(fw: &FirmwareInfo) -> Vec<&'static str> {
        super::box_names(fw, releases::select(fw).release)
    }

    fn fw(vendor: FirmwareVendor, major: u32, minor: u32, patch: u32) -> FirmwareInfo {
        FirmwareInfo {
            vendor,
            version: Some(FirmwareVersion::new(major, minor, patch)),
            ..FirmwareInfo::unknown()
        }
    }

    #[test]
    fn mode_boxes_follow_the_version() {
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
        // early Betaflight and Cleanflight: the old list
        assert_eq!(box_names(&fw(bf.clone(), 3, 2, 0))[3], "BARO");
        assert_eq!(
            box_names(&fw(bf.clone(), 3, 1, 6))[4],
            "MAG",
            "no ANTIGRAVITY to 3.1.6"
        );
        assert_eq!(
            box_names(&fw(FirmwareVendor::Cleanflight, 2, 0, 0))[4],
            "ANTIGRAVITY"
        );
        // the profile turns the mask into modes: 0x201 on 2025.12 is ARM and POSHOLD
        let (fw, h) = (fw(bf, 2025, 12, 2), RawHeaders::default());
        let log = Betaflight.bind(&fw, &h);
        assert_eq!(
            log.flight_modes(0x201),
            [FlightMode::Arm, FlightMode::PosHold]
        );
        // the box table of a log is built once, when the profile is bound
        assert_eq!(log.flight_modes(0x1), [FlightMode::Arm]);
    }
}
