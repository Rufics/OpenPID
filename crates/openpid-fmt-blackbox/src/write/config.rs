//! The configuration of the craft the log is written for: its
//! system-information headers.
//!
//! The headers are the configuration, and a line is written only when its
//! value is known. The values come, later ones winning, from the format (the
//! few lines that are constants of a release), from the headers the session
//! carries when it was decoded from a Betaflight log, from the typed
//! settings of the session, from what the frames need (the schedule, the
//! field selection, the scales: `fields` and the writer add those), and from
//! the caller's overrides. Nothing else is written: the writer has no
//! configuration of its own to fill the rest in with. The table of the
//! release (`sysinfo`) puts every line in its place; a session decoded from
//! a log of the target release keeps its own header list, line for line.

use super::sysinfo::{self, Line};
use super::{Target, WriteOptions};
use crate::headers::LOG_START_MARKER;
use crate::profiles::common;
use openpid_core::settings::{FilterChain, Lowpass, LowpassKind, PidGains};
use openpid_core::{FirmwareInfo, FirmwareVendor, FlightSession, RawHeaders, Settings};
use std::collections::HashSet;

/// Lowest motor output the writer scales a motor given in percent to, when
/// the session names no range (`motorOutput`): the `DShot` range above a
/// 5.5 % idle.
pub(super) const MOTOR_LOW: i64 = 158;
/// Highest motor output of that range.
pub(super) const MOTOR_HIGH: i64 = 2047;
/// `gyro_scale`: the firmware logs the gyro in deg/s, so the factor is the
/// bits of the float 1.0 (`castFloatBytesToInt(1.0f)`, `blackboxWriteSysinfo`
/// in `src/main/blackbox/blackbox.c`, the same at all seven tags).
const GYRO_SCALE: &str = "0x3f800000";
/// `gyro_sync_denom`: a constant 1 in the same function at all seven tags.
const GYRO_SYNC_DENOM: &str = "1";
/// The board and the craft of a session that was not decoded from a log:
/// the log says it is no flight controller's.
const SYNTHETIC_BOARD: &str = "OPID SYNTHETIC";
const SYNTHETIC_CRAFT: &str = "OpenPID synthetic";
/// Bit of the `features` header that enables the GPS (`FEATURE_GPS`).
pub(super) const FEATURE_GPS: i64 = 1 << 7;

/// Lines that name the firmware build: never taken from a log of another
/// release.
const IDENTITY: [&str; 5] = [
    "Firmware type",
    "Firmware revision",
    "Firmware date",
    "Build key",
    "Release name",
];

/// Lines whose value follows from the target release and the frames
/// written, not from the craft a session of another release describes.
const COMPUTED: [&str; 8] = [
    "I interval",
    "P interval",
    "P ratio",
    "looptime",
    "pid_process_denom",
    "fields_disabled_mask",
    "rc_command_scale",
    "debug_mode_name",
];

/// Where the session comes from, as far as the writer cares.
#[derive(Debug)]
pub(super) struct Source {
    /// The headers the session carries.
    pub raw: RawHeaders,
    /// The firmware those headers name.
    pub firmware: FirmwareInfo,
    /// The session was decoded from a Blackbox log: its events, slow frames
    /// and loop counter are what that log had, and nothing is added.
    pub decoded: bool,
    /// Decoded from a Betaflight log: its headers describe the craft.
    pub betaflight: bool,
    /// Decoded from a log of the target release.
    pub same_release: bool,
}

impl Source {
    pub(super) fn of(session: &FlightSession, target: Target) -> Source {
        let raw = RawHeaders(session.raw_headers.clone());
        let marker = String::from_utf8_lossy(&LOG_START_MARKER[2..]);
        let product = marker.split_once(':').map_or("", |(_, v)| v);
        let decoded = raw.get("Product") == Some(product);
        let firmware = FirmwareInfo::parse_revision(raw.get("Firmware revision").unwrap_or(""));
        let betaflight = decoded && firmware.vendor == FirmwareVendor::Betaflight;
        let same_release = betaflight && firmware.version.is_some_and(|v| target.is_release_of(v));
        Source {
            raw,
            firmware,
            decoded,
            betaflight,
            same_release,
        }
    }
}

/// The header lines after the field definitions, in order.
#[derive(Debug, Clone)]
pub(super) struct Lines {
    /// The lines the target release can write, in the firmware's order.
    table: &'static [Line],
    written: Vec<(String, String)>,
    /// Lines whose value the session's own log or the caller gave.
    given: HashSet<String>,
}

impl Lines {
    /// No lines yet, for a log of `target`.
    pub(super) fn of(target: Target) -> Lines {
        Lines {
            table: sysinfo::lines(target),
            written: Vec::new(),
            given: HashSet::new(),
        }
    }

    /// Value of the line `key`.
    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.written
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The first number of the line `key`, zero when absent.
    pub(super) fn int(&self, key: &str) -> i64 {
        self.number(key, 0)
    }

    /// The `index`-th number of the comma-separated line `key`, zero when
    /// absent.
    pub(super) fn number(&self, key: &str, index: usize) -> i64 {
        self.get(key)
            .and_then(|v| v.split(',').nth(index))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Whether the session's log or the caller gave the line.
    pub(super) fn is_given(&self, key: &str) -> bool {
        self.given.contains(key)
    }

    /// Where the release writes the line `key` among its lines.
    fn place(&self, key: &str) -> Option<usize> {
        self.table.iter().position(|l| l.name == key)
    }

    /// Writes the line `key` if the release has such a line: in place of
    /// its old value, or where the firmware writes it among the others.
    pub(super) fn put(&mut self, key: &str, value: impl Into<String>) {
        if self.place(key).is_some() {
            self.set(key, value.into());
        }
    }

    /// Writes the line `key`: a line the release has, in its place; any
    /// other after all of them.
    fn set(&mut self, key: &str, value: String) {
        if let Some(entry) = self.written.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value;
            return;
        }
        let Some(ours) = self.place(key) else {
            // not a line of the firmware: it has no place, and goes last
            self.written.push((key.to_string(), value));
            return;
        };
        // before the first line the firmware writes later, and before the
        // lines that are not the firmware's
        let later = |name: &str| self.place(name).is_none_or(|theirs| theirs > ours);
        let at = self.written.iter().position(|(name, _)| later(name));
        let at = at.unwrap_or(self.written.len());
        self.written.insert(at, (key.to_string(), value));
    }

    /// Drops the lines the release writes only on `condition`.
    pub(super) fn remove_all_on(&mut self, condition: &str) {
        let table = self.table;
        let conditional = |name: &str| table.iter().any(|l| l.name == name && l.needs(condition));
        self.written.retain(|(name, _)| !conditional(name));
    }

    /// The lines as the decoder's profile reads them.
    pub(super) fn as_raw(&self) -> RawHeaders {
        RawHeaders(self.written.clone())
    }
}

fn number(v: f64) -> String {
    format!("{}", v.round() as i64)
}

fn list(values: &[f64]) -> String {
    values
        .iter()
        .map(|v| number(*v))
        .collect::<Vec<_>>()
        .join(",")
}

fn lowpass_code(l: &Lowpass) -> f64 {
    match l.kind {
        Some(LowpassKind::Biquad) => 1.0,
        Some(LowpassKind::Pt2) => 2.0,
        Some(LowpassKind::Pt3) => 3.0,
        _ => 0.0,
    }
}

/// Headers of one filter chain. A filter the chain does not have is written
/// as switched off, so the log says exactly what the session says. The
/// low-passes are written under the names of 4.3 and later and under those
/// of 4.2 (`gyro_lowpass_hz`, `dterm_filter_type`, …): each release has one
/// set of them.
fn filter_headers(out: &mut Lines, prefix: &str, chain: &FilterChain) {
    for (slot, old, lowpass) in [(1, "", &chain.lowpass1), (2, "2", &chain.lowpass2)] {
        let code = lowpass.as_ref().map_or(0.0, lowpass_code);
        let cutoff = lowpass.as_ref().map_or(0.0, |l| l.cutoff_hz);
        out.put(&format!("{prefix}_lpf{slot}_type"), number(code));
        out.put(&format!("{prefix}_lpf{slot}_static_hz"), number(cutoff));
        out.put(&format!("{prefix}_lowpass{old}_type"), number(code));
        out.put(&format!("{prefix}_filter{old}_type"), number(code));
        out.put(&format!("{prefix}_lowpass{old}_hz"), number(cutoff));
    }
    let first = chain.lowpass1.as_ref();
    let range = [
        first.and_then(|l| l.dyn_min_hz).unwrap_or(0.0),
        first.and_then(|l| l.dyn_max_hz).unwrap_or(0.0),
    ];
    out.put(&format!("{prefix}_lpf1_dyn_hz"), list(&range));
    out.put(&format!("{prefix}_lowpass_dyn_hz"), list(&range));
    if let Some(l) = first.filter(|l| l.dyn_min_hz.is_some()) {
        let key = format!("{prefix}_lpf1_dyn_expo");
        out.put(&key, number(l.dyn_expo * 10.0));
    }
    // the gyro has two notch slots, the D term one
    let slots = if prefix == "gyro" { 2 } else { 1 };
    let padded = |values: Vec<f64>| {
        let mut v = values;
        v.resize(v.len().max(slots), 0.0);
        list(&v)
    };
    let centres = chain.notches.iter().map(|n| n.center_hz).collect();
    let cutoffs = chain.notches.iter().map(|n| n.cutoff_hz).collect();
    out.put(&format!("{prefix}_notch_hz"), padded(centres));
    out.put(&format!("{prefix}_notch_cutoff"), padded(cutoffs));
}

/// The gains. To 4.5 the firmware logs `d_min` beside the D gain, from
/// 2025.12 `d_max`; each release gets the line it has.
fn gains_headers(out: &mut Lines, gains: [&PidGains; 3]) {
    for (key, g) in ["rollPID", "pitchPID", "yawPID"].into_iter().zip(gains) {
        out.put(key, list(&[g.p, g.i, g.d]));
    }
    out.put("d_max", list(&gains.map(|g| g.d_max.unwrap_or(0.0))));
    out.put("d_min", list(&gains.map(|g| g.d_min.unwrap_or(0.0))));
    let feedforward = list(&gains.map(|g| g.f.unwrap_or(0.0)));
    out.put("ff_weight", feedforward.clone());
    // the name of 4.2
    out.put("feedforward_weight", feedforward);
}

/// Headers of the notches that follow the motors.
fn notch_headers(out: &mut Lines, chain: &FilterChain) {
    let notch = chain.dyn_notch;
    let count = notch.map_or(0, |d| d.count.unwrap_or(1));
    out.put("dyn_notch_count", count.to_string());
    if let Some(d) = notch {
        out.put("dyn_notch_min_hz", number(d.min_hz));
        out.put("dyn_notch_max_hz", number(d.max_hz));
        out.put("dyn_notch_q", number(d.q * 100.0));
    }
    let rpm = chain.rpm_notch.as_ref();
    let harmonics = rpm.map_or(0, |r| r.harmonics);
    out.put("rpm_filter_harmonics", harmonics.to_string());
    // the names of 4.2
    out.put("gyro_rpm_notch_harmonics", harmonics.to_string());
    let Some(r) = rpm else {
        return;
    };
    out.put("rpm_filter_q", number(r.q * 100.0));
    out.put("rpm_filter_min_hz", number(r.min_hz));
    out.put("gyro_rpm_notch_q", number(r.q * 100.0));
    out.put("gyro_rpm_notch_min", number(r.min_hz));
    if r.lpf_hz > 0.0 {
        out.put("rpm_notch_lpf", number(r.lpf_hz));
    }
    out.put("rpm_filter_fade_range_hz", number(r.fade_range_hz));
    if r.lpf_hz > 0.0 {
        out.put("rpm_filter_lpf_hz", number(r.lpf_hz));
    }
    if !r.weights.is_empty() {
        let weights: Vec<f64> = r.weights.iter().map(|w| w * 100.0).collect();
        out.put("rpm_filter_weights", list(&weights));
    }
}

/// Writes the typed settings of the session into the lines. For a session
/// decoded from a log of the target release only the settings that no longer
/// say what its own headers say are written (someone changed them): the rest
/// is in those headers already, to the letter. For any other session all of
/// them are: a log of another release spells some of them differently.
fn settings_headers(out: &mut Lines, session: &FlightSession, source: &Source) {
    let settings = &session.settings;
    let logged = source
        .same_release
        .then(|| common::settings(&source.firmware, &source.raw));
    let changed = |same: &dyn Fn(&Settings) -> bool| logged.as_ref().is_none_or(|l| !same(l));
    if let Some(pid) = &settings.pid
        && changed(&|l| l.pid.as_ref() == Some(pid))
    {
        gains_headers(out, [&pid.roll, &pid.pitch, &pid.yaw]);
    }
    if changed(&|l| l.gyro_filters == settings.gyro_filters) {
        filter_headers(out, "gyro", &settings.gyro_filters);
        notch_headers(out, &settings.gyro_filters);
    }
    if changed(&|l| l.dterm_filters == settings.dterm_filters) {
        filter_headers(out, "dterm", &settings.dterm_filters);
    }
    if changed(&|l| l.motor == settings.motor) {
        if let Some(poles) = settings.motor.poles {
            out.put("motor_poles", poles.to_string());
        }
        if let Some(kv) = settings.motor.kv {
            out.put("motor_kv", number(kv));
        }
        let protocol = settings.motor.protocol.as_deref();
        if let Some(code) = protocol.and_then(common::protocol_code) {
            out.put("motor_pwm_protocol", code.to_string());
        }
        // the range the motors are scaled to, when the session names one
        // (`fields` gives the writer's own to a session that does not)
        if let (Some(low), Some(high)) = (settings.motor.output_low, settings.motor.output_high)
            && high > low
        {
            out.put("motorOutput", list(&[low, high]));
        }
    }
    if let Some(mode) = &settings.debug_mode
        && changed(&|l| l.debug_mode.as_ref() == Some(mode))
    {
        out.put("debug_mode", mode.code.to_string());
    }
    for (key, value, same) in [
        (
            "Craft name",
            &settings.craft_name,
            logged.as_ref().map(|l| &l.craft_name),
        ),
        (
            "Board information",
            &settings.board,
            logged.as_ref().map(|l| &l.board),
        ),
        (
            "Log start datetime",
            &settings.log_start,
            logged.as_ref().map(|l| &l.log_start),
        ),
    ] {
        if let Some(v) = value.as_ref().filter(|_| same != Some(value)) {
            out.put(key, v.clone());
        }
    }
}

/// The `fields_disabled_mask` of a log of another release in the bit order
/// of `target`: the groups keep their meaning, the bits moved.
fn translated_mask(source: &Source, target: Target) -> u32 {
    use super::version::Select::{
        Acc, Altitude, Attitude, Battery, DebugLog, Gps, Gyro, GyroUnfilt, Mag, Motor, Pid, Pitot,
        RcCommands, Rpm, Rssi, Servo, Setpoint,
    };
    let Some(from) = source.firmware.version.and_then(Target::of_version) else {
        return 0;
    };
    let mask = source.raw.get_i64("fields_disabled_mask").unwrap_or(0);
    [
        Pid, RcCommands, Setpoint, Battery, Mag, Altitude, Rssi, Gyro, Attitude, Acc, DebugLog,
        Motor, Gps, Rpm, GyroUnfilt, Servo, Pitot,
    ]
    .into_iter()
    .filter(|group| {
        from.select_bit(*group)
            .is_some_and(|bit| mask & (1 << bit) != 0)
    })
    .filter_map(|group| target.select_bit(group))
    .fold(0, |mask, bit| mask | (1 << bit))
}

/// The header lines of the log, before the frames have had their say (the
/// loop rates, the field selection, the scales and the reference voltage
/// follow).
pub(super) fn lines(
    session: &FlightSession,
    source: &Source,
    target: Target,
    options: &WriteOptions,
) -> Lines {
    let mut out = Lines::of(target);
    if source.same_release {
        // the craft's own list: whatever its build has, in its order
        for (key, value) in &source.raw.0 {
            if key != "Product" && key != "Data version" && !key.starts_with("Field ") {
                out.written.push((key.clone(), value.clone()));
                out.given.insert(key.clone());
            }
        }
    } else {
        // what the release itself fixes
        out.put("Firmware type", target.firmware_type());
        out.put("Firmware revision", target.revision());
        out.put("gyro_scale", GYRO_SCALE);
        out.put("gyro_sync_denom", GYRO_SYNC_DENOM);
        // the writer logs in the normal resolution unless told otherwise
        out.put("blackbox_high_resolution", "0");
        if let Some(scale) = target.rc_command_scale() {
            out.put("rc_command_scale", scale.to_string());
        }
        if source.betaflight {
            // the craft of a log of another release, line by line
            let carried = |name: &str| !IDENTITY.contains(&name) && !COMPUTED.contains(&name);
            for (key, value) in source.raw.0.iter().filter(|(key, _)| carried(key)) {
                out.put(key, value.clone());
                if out.get(key).is_some() {
                    out.given.insert(key.clone());
                }
            }
            let mask = translated_mask(source, target);
            out.put("fields_disabled_mask", mask.to_string());
        }
        if !source.decoded {
            out.put("Board information", SYNTHETIC_BOARD);
            out.put("Craft name", SYNTHETIC_CRAFT);
        }
    }
    settings_headers(&mut out, session, source);
    for (key, value) in &options.headers {
        out.set(key, value.clone());
        out.given.insert(key.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::FirmwareVersion;

    fn source(lines: &[(&str, &str)]) -> Source {
        let mut session = openpid_sim_free_session();
        session.raw_headers = lines
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        Source::of(&session, Target::V2025_12)
    }

    /// An empty session: enough for the functions that read the headers.
    fn openpid_sim_free_session() -> FlightSession {
        FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware: FirmwareInfo::unknown(),
            profile_id: "synthetic".into(),
            settings: Settings::default(),
            raw_headers: Vec::new(),
            timebase: openpid_core::Timebase::default(),
            channels: openpid_core::ChannelSet::new(),
            events: Vec::new(),
            aux: Vec::new(),
            flight_modes: Vec::new(),
            diagnostics: openpid_core::session::Diagnostics::default(),
        }
    }

    const PRODUCT: (&str, &str) = (
        "Product",
        "Blackbox flight data recorder by Nicholas Sherlock",
    );

    #[test]
    fn where_a_session_comes_from() {
        let synthetic = source(&[("signal_spec", "{}")]);
        assert!(!synthetic.decoded && !synthetic.betaflight && !synthetic.same_release);
        let same = source(&[
            PRODUCT,
            ("Firmware revision", "Betaflight 2025.12.5 (x) STM32H743"),
        ]);
        assert!(same.decoded && same.betaflight && same.same_release);
        assert_eq!(
            same.firmware.version,
            Some(FirmwareVersion::new(2025, 12, 5))
        );
        let older = source(&[
            PRODUCT,
            ("Firmware revision", "Betaflight 4.5.1 (x) STM32F7X2"),
            ("rc_command_scale", "1000"),
        ]);
        assert!(older.betaflight && !older.same_release);
        let other = source(&[PRODUCT, ("Firmware revision", "INAV 7.0.0 (x) MATEKF405")]);
        assert!(other.decoded && !other.betaflight);
    }

    #[test]
    fn the_mask_of_another_release_keeps_its_groups() {
        // 4.5: ACC is bit 8, DEBUG 9, RPM 12, GYROUNFILT 13
        let from_4_5 = source(&[
            PRODUCT,
            ("Firmware revision", "Betaflight 4.5.1 (x) STM32F7X2"),
            ("fields_disabled_mask", "13057"),
        ]);
        // 13057 = PID | ACC | DEBUG | RPM | GYROUNFILT
        assert_eq!(13057, 1 | 1 << 8 | 1 << 9 | 1 << 12 | 1 << 13);
        assert_eq!(
            translated_mask(&from_4_5, Target::V2025_12),
            1 | 1 << 9 | 1 << 10 | 1 << 13 | 1 << 14
        );
        // 4.4 has neither RPM nor GYROUNFILT
        assert_eq!(
            translated_mask(&from_4_5, Target::V4_4),
            1 | 1 << 8 | 1 << 9
        );
        let unknown = source(&[PRODUCT, ("fields_disabled_mask", "1")]);
        assert_eq!(translated_mask(&unknown, Target::V4_5), 0);
    }

    fn names(lines: &Lines) -> Vec<&str> {
        lines
            .written
            .iter()
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// A session that says nothing gets the lines the release fixes, the
    /// marks of a synthetic log, and its filters as it has them: none.
    #[test]
    fn only_lines_with_a_known_value_are_written() {
        let session = openpid_sim_free_session();
        let options = WriteOptions::default();
        let lines_of = |target: Target| {
            let source = Source::of(&session, target);
            lines(&session, &source, target, &options)
        };
        let new = lines_of(Target::Master);
        assert_eq!(new.get("Firmware type"), Some("Betaflight"));
        assert_eq!(
            new.get("Firmware revision"),
            Some(Target::Master.revision())
        );
        assert_eq!(new.get("Board information"), Some("OPID SYNTHETIC"));
        assert_eq!(new.get("Craft name"), Some("OpenPID synthetic"));
        assert_eq!(new.get("gyro_scale"), Some("0x3f800000"));
        assert_eq!(new.get("blackbox_high_resolution"), Some("0"));
        assert_eq!(new.get("rc_command_scale"), Some("1000"));
        assert_eq!(new.get("gyro_lpf1_static_hz"), Some("0"));
        assert_eq!(new.get("rpm_filter_harmonics"), Some("0"));
        // nothing of a craft the session does not describe
        for unknown in [
            "Firmware date",
            "DeviceUID",
            "Log start datetime",
            "rollPID",
            "rates",
            "motorOutput",
            "vbatref",
            "acc_1G",
            "features",
            "debug_mode",
            "ap_hover_throttle",
            "s_roll",
            "motor_poles",
        ] {
            assert_eq!(new.get(unknown), None, "{unknown}");
        }
        // every line is where the firmware writes it
        let table = Target::Master.header_names();
        let places: Vec<usize> = names(&new)
            .iter()
            .map(|name| table.iter().position(|n| n == name).unwrap())
            .collect();
        assert!(places.windows(2).all(|w| w[0] < w[1]), "{:?}", names(&new));
        // 4.2: its own names, and no line of a later release
        let old = lines_of(Target::V4_2);
        assert_eq!(old.get("Firmware type"), Some("Cleanflight"));
        assert_eq!(old.get("gyro_lowpass_hz"), Some("0"));
        assert_eq!(old.get("gyro_lpf1_static_hz"), None);
        assert_eq!(old.get("blackbox_high_resolution"), None);
        assert_eq!(old.get("rc_command_scale"), None);
        assert_eq!(old.number("gyro_notch_hz", 1), 0);
        assert_eq!(old.int("no such line"), 0);
    }

    /// A line goes where the firmware writes it, whenever it is written; a
    /// line that is not the firmware's goes after them all.
    #[test]
    fn a_line_lands_in_its_place() {
        for target in Target::ALL {
            // every other line first, then the rest, last to first
            let table = target.header_names();
            let mut lines = Lines::of(target);
            for name in table.iter().step_by(2).chain(table.iter().rev()) {
                lines.set(name, "x".into());
            }
            assert_eq!(names(&lines), table, "{target}");
        }
        let target = Target::V2025_12;
        let table = target.header_names();
        let mut lines = Lines::of(target);
        lines.set("probe_id", "first".into());
        for name in table.iter().rev() {
            lines.set(name, "x".into());
        }
        lines.set("note", "second".into());
        lines.set("looptime", "125".into());
        let mut expected = table.clone();
        expected.extend(["probe_id", "note"]);
        assert_eq!(names(&lines), expected);
        assert_eq!(lines.get("looptime"), Some("125"));
        // `put` is for the lines of the release only
        lines.put("minthrottle", "1070");
        lines.put("probe_id", "again");
        assert_eq!(lines.get("minthrottle"), None);
        assert_eq!(lines.get("probe_id"), Some("first"));
        // the lines of a condition go together
        lines.remove_all_on(sysinfo::VBAT);
        for gone in ["vbat_scale", "vbatcellvoltage", "vbatref"] {
            assert_eq!(lines.get(gone), None);
        }
        assert_eq!(names(&lines).len(), expected.len() - 3);
    }

    /// The headers of a log of another release are carried line by line:
    /// those the target has too, except the ones that name the build or
    /// follow from the frames.
    #[test]
    fn the_craft_of_a_log_of_another_release_is_carried() {
        let mut session = openpid_sim_free_session();
        session.raw_headers = [
            PRODUCT,
            (
                "Firmware revision",
                "Betaflight 4.5.1 (77d01ba3b) STM32F7X2",
            ),
            ("Firmware date", "Apr  1 2024 10:00:00"),
            ("Craft name", "Maia"),
            ("I interval", "256"),
            ("minthrottle", "1070"),
            ("thr_mid", "40"),
            ("gps_rescue_min_sats", "8"),
            ("fields_disabled_mask", "256"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .to_vec();
        let target = Target::V2025_12;
        let source = Source::of(&session, target);
        let carried = lines(&session, &source, target, &WriteOptions::default());
        assert_eq!(carried.get("Craft name"), Some("Maia"));
        assert_eq!(carried.get("thr_mid"), Some("40"));
        assert_eq!(carried.get("gps_rescue_min_sats"), Some("8"));
        assert!(carried.is_given("thr_mid") && !carried.is_given("gyro_scale"));
        // the build is the target's, the schedule is computed
        assert_eq!(carried.get("Firmware revision"), Some(target.revision()));
        assert_eq!(carried.get("Firmware date"), None);
        assert_eq!(carried.get("I interval"), None);
        // 2025.12 has no such line; the mask is in its bit order (ACC: 8 → 9)
        assert_eq!(carried.get("minthrottle"), None);
        assert_eq!(carried.get("fields_disabled_mask"), Some("512"));
        // a real craft is not marked as synthetic
        assert_eq!(carried.get("Board information"), None);
    }
}
