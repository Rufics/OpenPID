//! The Betaflight releases the writer knows and what each of them logs.
//!
//! Everything here is a fact of the Blackbox format of one release, read
//! from the firmware source at that release's tag: which fields a frame can
//! have, in which order, with which predictor and encoding, on which
//! condition, and which bit of `fields_disabled_mask` switches a group off.
//! The firmware keeps these in the tables `blackboxMainFields`,
//! `blackboxGpsGFields`, `blackboxGpsHFields` and `blackboxSlowFields` and
//! in `testBlackboxConditionUncached` of `src/main/blackbox/blackbox.c`, and
//! the bits in `blackbox.h` (to 4.5) or `blackbox_fielddefs.h` (from
//! 2025.12). The tags read: `4.2.11`, `4.3.2`, `4.4.3`, `4.5.0` … `4.5.5`,
//! `2025.12.1` … `2025.12.5`, `2026.6.1`, `2026.6.2`, and `master` at
//! `4b1520e`. Where several patch releases of a version were read (4.5,
//! 2025.12, 2026.6) they do not differ in any of it.
//!
//! No firmware code is copied: the tables below are rebuilt from the names
//! and numbers, which are the format.

use crate::encoding as enc;
use crate::predictor as pred;
use openpid_core::FirmwareVersion;
use std::fmt;

/// The Betaflight release a log is written for.
///
/// A release differs from its neighbours in the fields of the frames, the
/// conditions they are logged on, the header lines and a few units; see
/// `docs/BLACKBOX-VERSIONS.md`. The patch releases of a version write the
/// same format, so a target is a version, not a patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Target {
    /// Betaflight 4.2 (read at the tag `4.2.11`).
    V4_2,
    /// Betaflight 4.3 (read at the tag `4.3.2`).
    V4_3,
    /// Betaflight 4.4 (`4.4.3`).
    V4_4,
    /// Betaflight 4.5 (`4.5.0` … `4.5.5`).
    V4_5,
    /// Betaflight 2025.12 (`2025.12.1` … `2025.12.5`): the default, the
    /// version the calibration probes are written for.
    #[default]
    V2025_12,
    /// Betaflight 2026.6 (`2026.6.1`, `2026.6.2`).
    V2026_6,
    /// The unreleased development branch, 2026.12-dev: `master` at
    /// `4b1520e`. It can still change before it is released.
    Master,
}

impl Target {
    /// Every target, oldest first.
    pub const ALL: [Target; 7] = [
        Target::V4_2,
        Target::V4_3,
        Target::V4_4,
        Target::V4_5,
        Target::V2025_12,
        Target::V2026_6,
        Target::Master,
    ];

    /// The version as the firmware numbers it; `master` is 2026.12.
    pub fn version(self) -> FirmwareVersion {
        match self {
            Target::V4_2 => FirmwareVersion::new(4, 2, 11),
            Target::V4_3 => FirmwareVersion::new(4, 3, 2),
            Target::V4_4 => FirmwareVersion::new(4, 4, 3),
            Target::V4_5 => FirmwareVersion::new(4, 5, 5),
            Target::V2025_12 => FirmwareVersion::new(2025, 12, 2),
            Target::V2026_6 => FirmwareVersion::new(2026, 6, 2),
            Target::Master => FirmwareVersion::new(2026, 12, 0),
        }
    }

    /// The name of the version: `4.5`, `2025.12`, `2026.12-dev`.
    pub fn name(self) -> &'static str {
        match self {
            Target::V4_2 => "4.2",
            Target::V4_3 => "4.3",
            Target::V4_4 => "4.4",
            Target::V4_5 => "4.5",
            Target::V2025_12 => "2025.12",
            Target::V2026_6 => "2026.6",
            Target::Master => "2026.12-dev",
        }
    }

    /// The target of a version name as [`Target::name`] gives it; `master`
    /// is the development branch too.
    pub fn from_name(name: &str) -> Option<Target> {
        let name = name.trim();
        if name.eq_ignore_ascii_case("master") {
            return Some(Target::Master);
        }
        Target::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Whether a log of firmware `version` is of this release.
    pub(super) fn is_release_of(self, version: FirmwareVersion) -> bool {
        let own = self.version();
        (own.major, own.minor) == (version.major, version.minor)
    }

    /// From 2025.12 a build is assembled from options (and versions are
    /// year.month); before, a release build carried every feature.
    pub(super) fn assembled_from_options(self) -> bool {
        self >= Target::V2025_12
    }

    /// Slots of the `debug[]` array (`DEBUG16_VALUE_COUNT`): four to 4.4,
    /// eight from 4.5.
    fn debug_slots(self) -> usize {
        if self >= Target::V4_5 { 8 } else { 4 }
    }

    /// The bit of `fields_disabled_mask` that switches `group` off, `None`
    /// where the release has no such bit.
    ///
    /// 4.2 has no mask at all. The enum `FlightLogFieldSelect_e` came with
    /// 4.3: to 4.4 twelve groups; 4.5 added
    /// RPM and GYROUNFILT at the end; 2025.12 put ATTITUDE before ACC (so
    /// ACC and everything after it moved up by one) and added SERVO;
    /// `master` added PITOT.
    pub(super) fn select_bit(self, group: Select) -> Option<u32> {
        use Select::{
            Acc, Altitude, Attitude, Battery, DebugLog, Gps, Gyro, GyroUnfilt, Mag, Motor, Pid,
            Pitot, RcCommands, Rpm, Rssi, Servo, Setpoint,
        };
        let order: &[Select] = match self {
            Target::V4_2 => &[],
            Target::V4_3 | Target::V4_4 => &[
                Pid, RcCommands, Setpoint, Battery, Mag, Altitude, Rssi, Gyro, Acc, DebugLog,
                Motor, Gps,
            ],
            Target::V4_5 => &[
                Pid, RcCommands, Setpoint, Battery, Mag, Altitude, Rssi, Gyro, Acc, DebugLog,
                Motor, Gps, Rpm, GyroUnfilt,
            ],
            Target::V2025_12 | Target::V2026_6 => &[
                Pid, RcCommands, Setpoint, Battery, Mag, Altitude, Rssi, Gyro, Attitude, Acc,
                DebugLog, Motor, Gps, Rpm, GyroUnfilt, Servo,
            ],
            Target::Master => &[
                Pid, RcCommands, Setpoint, Battery, Mag, Altitude, Rssi, Gyro, Attitude, Acc,
                DebugLog, Motor, Gps, Rpm, GyroUnfilt, Servo, Pitot,
            ],
        };
        order.iter().position(|g| *g == group).map(|i| i as u32)
    }

    /// Whether the servo fields ignore their bit of `fields_disabled_mask`.
    /// 2025.12 and 2026.6 test the number of the bit instead of the bit
    /// (`hasServos() && (FIELD_SELECT(SERVO))`, always true); `master`
    /// tests the bit.
    fn servo_bit_is_ignored(self) -> bool {
        matches!(self, Target::V2025_12 | Target::V2026_6)
    }

    /// Whether the GPS frames carry a time of their own
    /// (`FLIGHT_LOG_FIELD_CONDITION_NOT_LOGGING_EVERY_FRAME`). From 4.3 the
    /// firmware tests whether `P interval` differs from `I interval`; 4.2
    /// tests whether its `P ratio` setting is not 1.
    pub(super) fn gps_frames_have_time(
        self,
        i_interval: i64,
        p_interval: i64,
        p_ratio: i64,
    ) -> bool {
        if self == Target::V4_2 {
            p_ratio != 1
        } else {
            p_interval != i_interval
        }
    }

    /// Whether the `P interval` is one of five (1, 2, 4, 8, 16: the setting
    /// `sample_rate`, from 4.3). In 4.2 it is the `I interval` divided by
    /// any `P ratio`.
    pub(super) fn has_sample_rates(self) -> bool {
        self >= Target::V4_3
    }

    /// The target whose release wrote a log of firmware `version`.
    pub(super) fn of_version(version: FirmwareVersion) -> Option<Target> {
        Target::ALL.into_iter().find(|t| t.is_release_of(version))
    }
}

impl PartialOrd for Target {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Target {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.version().cmp(&other.version())
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Betaflight {}", self.name())
    }
}

/// A group of fields that one bit of `fields_disabled_mask` switches off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Select {
    Pid,
    RcCommands,
    Setpoint,
    Battery,
    Mag,
    Altitude,
    Rssi,
    Gyro,
    Attitude,
    Acc,
    DebugLog,
    Motor,
    Gps,
    Rpm,
    GyroUnfilt,
    Servo,
    Pitot,
}

/// What the firmware tests before it logs a field (the
/// `FLIGHT_LOG_FIELD_CONDITION_…` of the field tables).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum When {
    /// Always logged.
    Always,
    /// The PID terms are not switched off.
    Pid,
    /// As [`When::Pid`], and the D gain of this axis is not zero.
    PidD(usize),
    /// As [`When::Pid`], and the S gain of this axis is not zero (wing builds).
    WingS(usize),
    /// The stick commands are not switched off.
    RcCommands,
    /// The setpoints are not switched off.
    Setpoint,
    /// A voltage meter is configured and the battery is not switched off.
    Vbat,
    /// A current meter that measures is configured, battery not switched off.
    Amperage,
    /// A compass is fitted and not switched off.
    Mag,
    /// A barometer is fitted and the altitude is not switched off.
    Baro,
    /// A rangefinder is fitted and the altitude is not switched off.
    Rangefinder,
    /// The RSSI is configured and not switched off.
    Rssi,
    /// The pitot fields are not switched off (builds with a pitot driver).
    Pitot,
    /// The gyro is not switched off.
    Gyro,
    /// The unfiltered gyro is not switched off.
    GyroUnfilt,
    /// An accelerometer is fitted and not switched off.
    Acc,
    /// An accelerometer is fitted and the attitude is not switched off.
    Attitude,
    /// A debug mode is set and the debug fields are not switched off.
    Debug,
    /// The craft has at least this many motors, motors not switched off.
    Motors(u8),
    /// The mixer is a tricopter's and the motors are not switched off.
    Tricopter,
    /// The mixer has servos (and, where the release tests it, they are not
    /// switched off).
    Servos,
    /// The craft has at least this many motors, the ESCs report their speed
    /// and the RPM fields are not switched off.
    Rpm(u8),
    /// GPS frames: the frame has a time of its own
    /// ([`Target::gps_frames_have_time`]).
    GpsTime,
}

/// The integer type the firmware keeps a value in before it is logged
/// (`blackboxMainState_t`): what the value is limited to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Store {
    I16,
    U16,
    I32,
    U32,
}

impl Store {
    /// `v` limited to what the type holds.
    pub(super) fn limit(self, v: i64) -> i64 {
        match self {
            Store::I16 => v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)),
            Store::U16 => v.clamp(0, i64::from(u16::MAX)),
            Store::I32 => v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)),
            Store::U32 => v.clamp(0, i64::from(u32::MAX)),
        }
    }
}

/// One field of a frame, as the firmware's field table defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FieldDef {
    /// Field name as logged (`axisP[0]`).
    pub name: String,
    /// Whether the header declares it signed.
    pub signed: bool,
    /// Predictor and encoding in an `I` frame (the only pair of the frames
    /// that have no deltas).
    pub intra: (u8, u8),
    /// Predictor and encoding in a `P` frame.
    pub inter: (u8, u8),
    /// The condition it is logged on.
    pub when: When,
    /// The type the firmware keeps it in.
    pub store: Store,
}

/// Builder of a field table.
struct Table(Vec<FieldDef>);

impl Table {
    #[allow(clippy::too_many_arguments, reason = "one row of the firmware's table")]
    fn add(
        &mut self,
        name: &str,
        index: Option<usize>,
        signed: bool,
        intra: (u8, u8),
        inter: (u8, u8),
        when: When,
        store: Store,
    ) {
        let name = match index {
            Some(i) => format!("{name}[{i}]"),
            None => name.to_string(),
        };
        self.0.push(FieldDef {
            name,
            signed,
            intra,
            inter,
            when,
            store,
        });
    }

    /// A signed field: no prediction in `I` frames, the previous frame's
    /// value in `P` frames.
    fn delta(&mut self, name: &str, index: Option<usize>, p_enc: u8, when: When, store: Store) {
        self.add(
            name,
            index,
            true,
            (pred::ZERO, enc::SIGNED_VB),
            (pred::PREVIOUS, p_enc),
            when,
            store,
        );
    }

    /// Three signed fields predicted from the mean of the two frames before
    /// (the sensors).
    fn sensor(&mut self, name: &str, count: usize, when: When) {
        for i in 0..count {
            self.add(
                name,
                Some(i),
                true,
                (pred::ZERO, enc::SIGNED_VB),
                (pred::AVERAGE_2, enc::SIGNED_VB),
                when,
                Store::I16,
            );
        }
    }
}

/// The fields of the main (`I` and `P`) frames of `target`, in the order of
/// `blackboxMainFields`, every field the release can log.
pub(super) fn main_fields(target: Target) -> Vec<FieldDef> {
    let mut t = Table(Vec::new());
    t.add(
        "loopIteration",
        None,
        false,
        (pred::ZERO, enc::UNSIGNED_VB),
        (pred::INC, enc::NULL),
        When::Always,
        Store::U32,
    );
    t.add(
        "time",
        None,
        false,
        (pred::ZERO, enc::UNSIGNED_VB),
        (pred::STRAIGHT_LINE, enc::SIGNED_VB),
        When::Always,
        Store::U32,
    );
    for i in 0..3 {
        t.delta("axisP", Some(i), enc::SIGNED_VB, When::Pid, Store::I32);
    }
    for i in 0..3 {
        t.delta("axisI", Some(i), enc::TAG2_3S32, When::Pid, Store::I32);
    }
    for i in 0..3 {
        t.delta("axisD", Some(i), enc::SIGNED_VB, When::PidD(i), Store::I32);
    }
    for i in 0..3 {
        t.delta("axisF", Some(i), enc::SIGNED_VB, When::Pid, Store::I32);
    }
    if target >= Target::V2025_12 {
        // `USE_WING` builds
        for i in 0..3 {
            t.delta("axisS", Some(i), enc::SIGNED_VB, When::WingS(i), Store::I32);
        }
    }
    for i in 0..3 {
        t.delta(
            "rcCommand",
            Some(i),
            enc::TAG8_4S16,
            When::RcCommands,
            Store::I16,
        );
    }
    t.add(
        "rcCommand",
        Some(3),
        false,
        (pred::ZERO, enc::UNSIGNED_VB),
        (pred::PREVIOUS, enc::TAG8_4S16),
        When::RcCommands,
        Store::I16,
    );
    for i in 0..4 {
        t.delta(
            "setpoint",
            Some(i),
            enc::TAG8_4S16,
            When::Setpoint,
            Store::I16,
        );
    }
    t.add(
        "vbatLatest",
        None,
        false,
        (pred::VBATREF, enc::NEG_14BIT),
        (pred::PREVIOUS, enc::TAG8_8SVB),
        When::Vbat,
        Store::U16,
    );
    t.delta(
        "amperageLatest",
        None,
        enc::TAG8_8SVB,
        When::Amperage,
        Store::I32,
    );
    for i in 0..3 {
        t.delta("magADC", Some(i), enc::TAG8_8SVB, When::Mag, Store::I16);
    }
    // renamed in 4.4
    let baro = if target >= Target::V4_4 {
        "baroAlt"
    } else {
        "BaroAlt"
    };
    t.delta(baro, None, enc::TAG8_8SVB, When::Baro, Store::I32);
    t.delta(
        "surfaceRaw",
        None,
        enc::TAG8_8SVB,
        When::Rangefinder,
        Store::I32,
    );
    t.add(
        "rssi",
        None,
        false,
        (pred::ZERO, enc::UNSIGNED_VB),
        (pred::PREVIOUS, enc::TAG8_8SVB),
        When::Rssi,
        Store::U16,
    );
    if target == Target::Master {
        // `USE_PITOT` builds: the airspeed and the differential pressure
        for i in 0..2 {
            t.delta("pitot", Some(i), enc::SIGNED_VB, When::Pitot, Store::I32);
        }
    }
    t.sensor("gyroADC", 3, When::Gyro);
    if target >= Target::V4_5 {
        t.sensor("gyroUnfilt", 3, When::GyroUnfilt);
    }
    t.sensor("accSmooth", 3, When::Acc);
    if target >= Target::V2025_12 {
        t.sensor("imuQuaternion", 3, When::Attitude);
    }
    t.sensor("debug", target.debug_slots(), When::Debug);
    t.add(
        "motor",
        Some(0),
        false,
        (pred::MINMOTOR, enc::UNSIGNED_VB),
        (pred::AVERAGE_2, enc::SIGNED_VB),
        When::Motors(1),
        Store::I16,
    );
    for i in 1..8 {
        t.add(
            "motor",
            Some(i),
            false,
            (pred::MOTOR_0, enc::SIGNED_VB),
            (pred::AVERAGE_2, enc::SIGNED_VB),
            When::Motors(i as u8 + 1),
            Store::I16,
        );
    }
    if target >= Target::V2025_12 {
        for i in 0..8 {
            t.add(
                "servo",
                Some(i),
                false,
                (pred::P1500, enc::TAG8_8SVB),
                (pred::PREVIOUS, enc::TAG8_8SVB),
                When::Servos,
                Store::I16,
            );
        }
    } else {
        // the tail servo of a tricopter
        t.add(
            "servo",
            Some(5),
            false,
            (pred::P1500, enc::SIGNED_VB),
            (pred::PREVIOUS, enc::SIGNED_VB),
            When::Tricopter,
            Store::I16,
        );
    }
    if target >= Target::V4_5 {
        // `USE_DSHOT_TELEMETRY`: eRPM / 100
        for i in 0..8 {
            t.add(
                "eRPM",
                Some(i),
                false,
                (pred::ZERO, enc::UNSIGNED_VB),
                (pred::PREVIOUS, enc::SIGNED_VB),
                When::Rpm(i as u8 + 1),
                Store::I16,
            );
        }
    }
    t.0
}

/// The fields of the GPS (`G`) frames of `target` (`blackboxGpsGFields`).
pub(super) fn gps_fields(target: Target) -> Vec<FieldDef> {
    let mut t = Table(Vec::new());
    let plain = |t: &mut Table, name: &str, index: Option<usize>, signed: bool| {
        let coding = if signed {
            (pred::ZERO, enc::SIGNED_VB)
        } else {
            (pred::ZERO, enc::UNSIGNED_VB)
        };
        t.add(
            name,
            index,
            signed,
            coding,
            coding,
            When::Always,
            Store::I32,
        );
    };
    let time = (pred::LAST_MAIN_FRAME_TIME, enc::UNSIGNED_VB);
    t.add("time", None, false, time, time, When::GpsTime, Store::U32);
    plain(&mut t, "GPS_numSat", None, false);
    let home = (pred::HOME_COORD, enc::SIGNED_VB);
    for i in 0..2 {
        t.add(
            "GPS_coord",
            Some(i),
            true,
            home,
            home,
            When::Always,
            Store::I32,
        );
    }
    // unsigned to 4.4, signed from 4.5 (in tenths of a metre in both)
    plain(&mut t, "GPS_altitude", None, target >= Target::V4_5);
    plain(&mut t, "GPS_speed", None, false);
    plain(&mut t, "GPS_ground_course", None, false);
    if target >= Target::V2025_12 {
        for i in 0..3 {
            plain(&mut t, "GPS_velned", Some(i), true);
        }
    }
    if target >= Target::V2026_6 {
        plain(&mut t, "GPS_time", None, false);
    }
    t.0
}

/// The fields of the GPS home (`H`) frames of `target`
/// (`blackboxGpsHFields`).
pub(super) fn home_fields(target: Target) -> Vec<FieldDef> {
    let mut t = Table(Vec::new());
    let signed = (pred::ZERO, enc::SIGNED_VB);
    let coordinates = if target >= Target::V2025_12 { 3 } else { 2 };
    for i in 0..coordinates {
        t.add(
            "GPS_home",
            Some(i),
            true,
            signed,
            signed,
            When::Always,
            Store::I32,
        );
    }
    if target >= Target::V2026_6 {
        let unsigned = (pred::ZERO, enc::UNSIGNED_VB);
        t.add(
            "GPS_home_epoch",
            None,
            false,
            unsigned,
            unsigned,
            When::Always,
            Store::U32,
        );
    }
    t.0
}

/// The fields of the slow (`S`) frames (`blackboxSlowFields`): the same in
/// every release.
pub(super) fn slow_fields() -> Vec<FieldDef> {
    let mut t = Table(Vec::new());
    let unsigned = (pred::ZERO, enc::UNSIGNED_VB);
    let packed = (pred::ZERO, enc::TAG2_3S32);
    for name in ["flightModeFlags", "stateFlags"] {
        t.add(
            name,
            None,
            false,
            unsigned,
            unsigned,
            When::Always,
            Store::U32,
        );
    }
    for name in ["failsafePhase", "rxSignalReceived", "rxFlightChannelsValid"] {
        t.add(name, None, false, packed, packed, When::Always, Store::U32);
    }
    t.0
}

/// The configuration of a craft, as far as the conditions of the field
/// tables test it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Craft {
    /// `fields_disabled_mask`, in the bit order of the target.
    pub mask: u32,
    /// Motors of the mixer.
    pub motors: u8,
    /// Whether the ESCs report their speed (bidirectional `DShot`).
    pub dshot_telemetry: bool,
    /// Whether the mixer has servos.
    pub servos: bool,
    /// Whether the mixer is a tricopter's.
    pub tricopter: bool,
    /// Whether the D gain of each axis is not zero.
    pub d_gain: [bool; 3],
    /// Whether the S gain of each axis is not zero (wing builds).
    pub s_gain: [bool; 3],
    /// A voltage meter is configured.
    pub vbat: bool,
    /// A measuring current meter is configured.
    pub amperage: bool,
    /// A compass is fitted.
    pub mag: bool,
    /// A barometer is fitted.
    pub baro: bool,
    /// A rangefinder is fitted.
    pub rangefinder: bool,
    /// The RSSI is configured.
    pub rssi: bool,
    /// The build has a pitot driver.
    pub pitot: bool,
    /// An accelerometer is fitted.
    pub acc: bool,
    /// A debug mode is set.
    pub debug: bool,
    /// The GPS frames have a time of their own.
    pub gps_time: bool,
}

impl Craft {
    /// Whether `group` is switched off in `fields_disabled_mask`. A group
    /// the release has no bit for cannot be switched off.
    pub(super) fn is_off(&self, target: Target, group: Select) -> bool {
        target
            .select_bit(group)
            .is_some_and(|bit| self.mask & (1 << bit) != 0)
    }

    /// Switches `group` off, where the release has a bit for it.
    pub(super) fn switch_off(&mut self, target: Target, group: Select) {
        if let Some(bit) = target.select_bit(group) {
            self.mask |= 1 << bit;
        }
    }

    /// Whether `target` logs a field with the condition `when` on this
    /// craft (`testBlackboxConditionUncached`).
    pub(super) fn logs(&self, target: Target, when: When) -> bool {
        let on = |group: Select| !self.is_off(target, group);
        match when {
            When::Always => true,
            When::Pid => on(Select::Pid),
            When::PidD(axis) => self.d_gain[axis] && on(Select::Pid),
            When::WingS(axis) => self.s_gain[axis] && on(Select::Pid),
            When::RcCommands => on(Select::RcCommands),
            When::Setpoint => on(Select::Setpoint),
            When::Vbat => self.vbat && on(Select::Battery),
            When::Amperage => self.amperage && on(Select::Battery),
            When::Mag => self.mag && on(Select::Mag),
            When::Baro => self.baro && on(Select::Altitude),
            When::Rangefinder => self.rangefinder && on(Select::Altitude),
            When::Rssi => self.rssi && on(Select::Rssi),
            When::Pitot => self.pitot && on(Select::Pitot),
            When::Gyro => on(Select::Gyro),
            When::GyroUnfilt => on(Select::GyroUnfilt),
            When::Acc => self.acc && on(Select::Acc),
            When::Attitude => self.acc && on(Select::Attitude),
            When::Debug => self.debug && on(Select::DebugLog),
            When::Motors(n) => self.motors >= n && on(Select::Motor),
            When::Tricopter => self.tricopter && on(Select::Motor),
            When::Servos => self.servos && (target.servo_bit_is_ignored() || on(Select::Servo)),
            When::Rpm(n) => self.motors >= n && self.dshot_telemetry && on(Select::Rpm),
            When::GpsTime => self.gps_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(fields: &[FieldDef]) -> Vec<&str> {
        fields.iter().map(|f| f.name.as_str()).collect()
    }

    /// The number of fields of each table at each tag, as counted in the
    /// firmware source.
    #[test]
    fn the_tables_have_the_fields_of_each_release() {
        for (target, main, gps, home) in [
            (Target::V4_2, 49, 7, 2),
            (Target::V4_3, 49, 7, 2),
            (Target::V4_4, 49, 7, 2),
            (Target::V4_5, 64, 7, 2),
            (Target::V2025_12, 77, 10, 3),
            (Target::V2026_6, 77, 11, 4),
            (Target::Master, 79, 11, 4),
        ] {
            assert_eq!(main_fields(target).len(), main, "{target}");
            assert_eq!(gps_fields(target).len(), gps, "{target}");
            assert_eq!(home_fields(target).len(), home, "{target}");
        }
        assert_eq!(slow_fields().len(), 5);
        let old = main_fields(Target::V4_3);
        assert!(names(&old).contains(&"BaroAlt") && !names(&old).contains(&"gyroUnfilt[0]"));
        assert_eq!(names(&old).last(), Some(&"servo[5]"));
        let new = main_fields(Target::Master);
        assert!(names(&new).contains(&"baroAlt") && names(&new).contains(&"pitot[1]"));
        assert_eq!(names(&new).last(), Some(&"eRPM[7]"));
        // the altitude of a fix became signed with 4.5
        let altitude = |t: Target| gps_fields(t)[4].clone();
        assert_eq!(altitude(Target::V4_4).intra, (pred::ZERO, enc::UNSIGNED_VB));
        assert_eq!(altitude(Target::V4_5).intra, (pred::ZERO, enc::SIGNED_VB));
    }

    /// Every field, sign, predictor and encoding against the lines extracted
    /// from the firmware's tables (`field-tables.txt`).
    #[test]
    fn the_tables_are_the_firmware_s() {
        let facts = include_str!("field-tables.txt");
        let mut checked = 0;
        for line in facts.lines().filter(|l| !l.starts_with('#')) {
            let (key, values) = line.split_once(": ").unwrap();
            let mut words = key.splitn(4, ' ');
            let target = Target::from_name(words.next().unwrap()).unwrap();
            let (_tag, frame, row) = (
                words.next().unwrap(),
                words.next().unwrap(),
                words.next().unwrap(),
            );
            let fields = match frame {
                "I" => main_fields(target),
                "G" => gps_fields(target),
                _ => home_fields(target),
            };
            let numbers = |f: &dyn Fn(&FieldDef) -> u8| {
                let v: Vec<String> = fields.iter().map(|d| f(d).to_string()).collect();
                v.join(",")
            };
            let mine = match row {
                "name" => names(&fields).join(","),
                "signed" => numbers(&|d| u8::from(d.signed)),
                "predictor" => numbers(&|d| d.intra.0),
                "encoding" => numbers(&|d| d.intra.1),
                "P predictor" => numbers(&|d| d.inter.0),
                _ => numbers(&|d| d.inter.1),
            };
            assert_eq!(mine, values, "{key}");
            checked += 1;
        }
        assert_eq!(checked, 7 * (6 + 4 + 4));
    }

    #[test]
    fn the_bits_of_the_mask_moved_between_releases() {
        assert_eq!(Target::V4_5.select_bit(Select::Acc), Some(8));
        assert_eq!(Target::V2025_12.select_bit(Select::Acc), Some(9));
        assert_eq!(Target::V2025_12.select_bit(Select::Attitude), Some(8));
        assert_eq!(Target::V4_4.select_bit(Select::Rpm), None);
        assert_eq!(Target::V4_2.select_bit(Select::Pid), None, "no mask in 4.2");
        assert!(Target::V4_2.gps_frames_have_time(32, 32, 0));
        assert!(!Target::V4_2.gps_frames_have_time(32, 32, 1));
        assert!(!Target::V4_3.gps_frames_have_time(32, 32, 1));
        assert!(Target::V4_3.gps_frames_have_time(32, 0, 0));
        assert_eq!(Target::V4_5.select_bit(Select::GyroUnfilt), Some(13));
        assert_eq!(Target::V2026_6.select_bit(Select::Pitot), None);
        assert_eq!(Target::Master.select_bit(Select::Pitot), Some(16));
    }

    #[test]
    fn conditions_follow_the_craft_and_the_mask() {
        let mut craft = Craft {
            motors: 4,
            dshot_telemetry: true,
            d_gain: [true, true, false],
            acc: true,
            servos: true,
            ..Craft::default()
        };
        let t = Target::V2025_12;
        assert!(craft.logs(t, When::PidD(0)) && !craft.logs(t, When::PidD(2)));
        assert!(craft.logs(t, When::Motors(4)) && !craft.logs(t, When::Motors(5)));
        assert!(craft.logs(t, When::Rpm(4)) && craft.logs(t, When::Attitude));
        assert!(!craft.logs(t, When::Debug) && !craft.logs(t, When::Vbat));
        craft.switch_off(t, Select::Pid);
        craft.switch_off(t, Select::Servo);
        craft.switch_off(t, Select::Pitot);
        assert_eq!(craft.mask, 1 | 1 << 15, "no pitot bit before master");
        assert!(!craft.logs(t, When::Pid) && !craft.logs(t, When::PidD(0)));
        // 2025.12 and 2026.6 log the servos whatever the mask says
        assert!(craft.logs(Target::V2026_6, When::Servos));
        assert!(!craft.logs(Target::Master, When::Servos));
    }

    #[test]
    fn names_and_order() {
        assert_eq!(Target::default(), Target::V2025_12);
        assert_eq!(Target::from_name("4.5"), Some(Target::V4_5));
        assert_eq!(Target::from_name("master"), Some(Target::Master));
        assert_eq!(Target::from_name(" 2026.12-dev "), Some(Target::Master));
        assert_eq!(Target::from_name("4.2"), Some(Target::V4_2));
        assert_eq!(Target::from_name("4.1"), None);
        assert_eq!(Target::Master.to_string(), "Betaflight 2026.12-dev");
        assert!(Target::V4_5 < Target::V2025_12 && Target::V2026_6 < Target::Master);
        assert_eq!(
            Target::of_version(FirmwareVersion::new(4, 5, 0)),
            Some(Target::V4_5)
        );
        assert_eq!(
            Target::of_version(FirmwareVersion::new(4, 2, 0)),
            Some(Target::V4_2)
        );
        assert_eq!(Target::of_version(FirmwareVersion::new(4, 1, 7)), None);
        assert_eq!(Store::I16.limit(40_000), 32_767);
        assert_eq!(Store::U16.limit(-1), 0);
        assert_eq!(Store::I32.limit(1 << 40), i64::from(i32::MAX));
        assert_eq!(Store::U32.limit(-5), 0);
    }
}
