//! Betaflight (and Cleanflight) profile.

use super::common::{self, Log};
use crate::cli::BetaflightCli;
use crate::releases::{self, Release, Selection};
use openpid_core::{
    BoundProfile, ChannelId, Exporter, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor,
    FirmwareVersion, FlightMode, FlightSession, RawHeaders, Settings, SliderModel, Unit,
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

/// Main-frame fields only Betaflight logs under these names and in these
/// units; the names stay the channel keys they have always had.
///
/// - `pitot[0]`, `pitot[1]` (`master` at `4b1520e`, `blackbox.c`: the
///   indicated airspeed and the differential pressure of `sensors/pitot.h`,
///   "cm/s" and "Pa, zero-corrected");
/// - `servo[n]`: the pulse width of the servo output in microseconds
///   (`flight/servos.c`: centred on `DEFAULT_SERVO_MIDDLE`, 1500; the log
///   predicts 1500), `servo[5]` alone up to 4.5, `servo[0..7]` from 2025.12;
/// - `magADC[n]`: the magnetometer as its driver reads it, sensor counts
///   after board alignment (`sensors/compass.c`); no physical unit;
/// - `surfaceRaw`: the rangefinder distance in centimetres, −1 when out of
///   range (`sensors/rangefinder.c`, tags 4.2.11 to `master`).
fn own_field(field: &str) -> Option<FieldMapping> {
    let (base, idx) = common::split_indexed(field);
    let unit = match (base, idx) {
        ("pitot", Some(0)) => Unit::Custom("cm/s".into()),
        ("pitot", Some(1)) => Unit::Custom("Pa".into()),
        ("servo", Some(0..=7)) => Unit::Microseconds,
        ("magADC", Some(0..=2)) => Unit::Raw,
        ("surfaceRaw", None) => Unit::Centimeters,
        _ => return None,
    };
    Some(FieldMapping::new(
        ChannelId::Custom(field.to_string()),
        unit,
    ))
}

/// The fields of the frames Betaflight logs beside the main ones, from 4.0
/// on, with their units.
///
/// GPS frames (`blackbox.c`, `writeGPSFrame`, and the units of `gpsSol` in
/// `io/gps.h` and `io/gps.c`, tags 4.2.11 to `master`; the 4.5.0 fixture has
/// them): the fix time in the log's microseconds, the number of satellites,
/// the coordinates in 1e-7 degrees (logged against the home point, absolute
/// once decoded), the altitude in 0.1 m (`altCm / 10`), the speed in cm/s
/// (the 3D speed where the release logs it under `gps_use_3d_speed`, see
/// [`Release::gps_speed_3d`], else the speed over ground), the course over
/// ground in 0.1 degrees; from 2025.12 the velocity north, east and down in
/// cm/s; from 2026.6 the receiver's time of week in milliseconds.
///
/// Home frames (`writeGPSHomeFrame`): the coordinates of the home point in
/// 1e-7 degrees; from 2025.12 its altitude in 0.1 m; from 2026.6 the time it
/// was taken as Unix seconds (`gpsDateTimeToEpoch`, `io/gps.c`).
///
/// Slow frames (`blackboxSlowFields`, `loadSlowState`): `stateFlags` is the
/// firmware's state word (`fc/runtime_config.h`: bit 0 home point set, bit 1
/// GPS fix, bit 2 GPS fix ever); `failsafePhase` is the phase of
/// `flight/failsafe.h` (0 idle, 1 signal loss detected, 2 landing, 3 landed,
/// 4 monitoring the link, 5 link recovered, 6 GPS rescue, and 7 autopilot on
/// `master`); `rxSignalReceived` and `rxFlightChannelsValid` are 0 or 1.
/// `flightModeFlags` is not a channel: it becomes the session's flight
/// modes.
fn aux_field(release: &Release, h: &RawHeaders, table: &str, field: &str) -> Option<FieldMapping> {
    use openpid_core::GpsField as G;
    let gps = |f, unit, scale| FieldMapping::scaled(ChannelId::Gps(f), unit, scale);
    let named = |unit| FieldMapping::new(ChannelId::Custom(field.to_string()), unit);
    Some(match (table, field) {
        ("gps", "time") => gps(G::FixTime, Unit::Microseconds, 1.0),
        ("gps", "GPS_numSat") => gps(G::Satellites, Unit::Unitless, 1.0),
        ("gps", "GPS_coord[0]") => gps(G::Latitude, Unit::Degrees, 1.0e-7),
        ("gps", "GPS_coord[1]") => gps(G::Longitude, Unit::Degrees, 1.0e-7),
        ("gps", "GPS_altitude") => gps(G::Altitude, Unit::Meters, 0.1),
        ("gps", "GPS_speed") => {
            let three_d = release.gps_speed_3d && h.get_i64("gps_use_3d_speed") == Some(1);
            let which = if three_d { G::Speed3d } else { G::GroundSpeed };
            gps(which, Unit::MetersPerSecond, 0.01)
        }
        ("gps", "GPS_ground_course") => gps(G::Course, Unit::Degrees, 0.1),
        ("gps", "GPS_velned[0]") => gps(G::VelocityNorth, Unit::MetersPerSecond, 0.01),
        ("gps", "GPS_velned[1]") => gps(G::VelocityEast, Unit::MetersPerSecond, 0.01),
        ("gps", "GPS_velned[2]") => gps(G::VelocityDown, Unit::MetersPerSecond, 0.01),
        ("gps", "GPS_time") => gps(G::TimeOfWeek, Unit::Seconds, 0.001),
        ("gps_home", "GPS_home[0]") => gps(G::HomeLatitude, Unit::Degrees, 1.0e-7),
        ("gps_home", "GPS_home[1]") => gps(G::HomeLongitude, Unit::Degrees, 1.0e-7),
        ("gps_home", "GPS_home[2]") => gps(G::HomeAltitude, Unit::Meters, 0.1),
        ("gps_home", "GPS_home_epoch") => gps(G::HomeTime, Unit::Seconds, 1.0),
        ("slow", "stateFlags") => named(Unit::Flags),
        ("slow", "failsafePhase" | "rxSignalReceived" | "rxFlightChannelsValid") => {
            named(Unit::Unitless)
        }
        _ => return None,
    })
}

impl Bound<'_> {
    /// Betaflight from 4.0 on: the firmware whose fields this profile knows
    /// from its source. Earlier Betaflight and Cleanflight are read with
    /// the fields of the whole family only.
    fn described(&self) -> bool {
        self.log.fw.vendor == FirmwareVendor::Betaflight && !self.log.release.is_before_4_0()
    }
}

impl BoundProfile for Bound<'_> {
    fn map_field(&self, field: &str) -> Option<FieldMapping> {
        self.log
            .map_field(field)
            .or_else(|| self.described().then(|| own_field(field)).flatten())
    }

    fn map_aux_field(&self, table: &str, field: &str) -> Option<FieldMapping> {
        self.described()
            .then(|| aux_field(self.log.release, self.log.h, table, field))
            .flatten()
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
    fn fields_of_betaflight_alone_have_units() {
        use openpid_core::GpsField as G;
        let h = |pairs: &[(&str, &str)]| {
            RawHeaders(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            )
        };
        let (new, none) = (fw(FirmwareVendor::Betaflight, 2026, 12, 0), h(&[]));
        let log = Betaflight.bind(&new, &none);
        let unit = |field: &str| {
            let m = log.map_field(field).unwrap_or_else(|| panic!("{field}"));
            assert_eq!(m.id, ChannelId::Custom(field.into()), "the key stays");
            assert_eq!((m.scale, m.offset), (1.0, 0.0));
            m.unit
        };
        assert_eq!(unit("pitot[0]"), Unit::Custom("cm/s".into()));
        assert_eq!(unit("pitot[1]"), Unit::Custom("Pa".into()));
        assert_eq!(unit("servo[0]"), Unit::Microseconds);
        assert_eq!(unit("servo[7]"), Unit::Microseconds);
        assert_eq!(unit("magADC[2]"), Unit::Raw);
        assert_eq!(unit("surfaceRaw"), Unit::Centimeters);
        assert_eq!(log.map_field("pitot[2]"), None);
        assert_eq!(log.map_field("servo[8]"), None);
        assert_eq!(log.map_field("futureThing[3]"), None);
        // the GPS time and the home point's time of 2026.6 on
        let aux = |field: &str, table: &str| log.map_aux_field(table, field).map(|m| m.id);
        assert_eq!(aux("GPS_time", "gps"), Some(ChannelId::Gps(G::TimeOfWeek)));
        assert_eq!(
            aux("GPS_home_epoch", "gps_home"),
            Some(ChannelId::Gps(G::HomeTime))
        );
        assert_eq!(aux("GPS_numSat", "slow"), None, "a field of another frame");
        assert_eq!(aux("flightModeFlags", "slow"), None);
        assert_eq!(aux("GPS_future", "gps"), None);
        // what `GPS_speed` is: the 3D speed from 2025.12 on when the header
        // says so; the speed over ground otherwise, and always before
        let three_d = h(&[("gps_use_3d_speed", "1")]);
        let ground = h(&[("gps_use_3d_speed", "0")]);
        let speed = |v: &FirmwareInfo, h: &RawHeaders| {
            Betaflight
                .bind(v, h)
                .map_aux_field("gps", "GPS_speed")
                .map(|m| (m.id, m.unit, m.scale))
        };
        let is = |f| Some((ChannelId::Gps(f), Unit::MetersPerSecond, 0.01));
        let bf = |major, minor| fw(FirmwareVendor::Betaflight, major, minor, 0);
        assert_eq!(speed(&bf(2025, 12), &three_d), is(G::Speed3d));
        assert_eq!(speed(&bf(2026, 6), &three_d), is(G::Speed3d));
        assert_eq!(speed(&bf(2025, 12), &ground), is(G::GroundSpeed));
        assert_eq!(speed(&bf(2025, 12), &none), is(G::GroundSpeed));
        assert_eq!(speed(&bf(4, 5), &three_d), is(G::GroundSpeed));
        assert_eq!(speed(&bf(4, 4), &three_d), is(G::GroundSpeed));
        // Betaflight before 4.0 and Cleanflight are not described here
        for old in [bf(3, 5), fw(FirmwareVendor::Cleanflight, 2, 5, 0)] {
            let log = Betaflight.bind(&old, &none);
            assert_eq!(log.map_aux_field("gps", "GPS_speed"), None);
            assert_eq!(log.map_aux_field("slow", "stateFlags"), None);
            assert_eq!(log.map_field("servo[5]"), None);
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
        // the last bit a log carries is PARALYZE (`fc/rc_modes.h` of the
        // tags 4.2.11 to 4.5.5), not USER1 as the reader had it
        let (v, h) = (fw(bf.clone(), 4, 5, 1), RawHeaders::default());
        assert_eq!(
            Betaflight.bind(&v, &h).flight_modes(1 << 31),
            [FlightMode::Other("PARALYZE".into())]
        );
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
