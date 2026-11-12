//! The configuration of the craft the log is written for: its
//! system-information headers.
//!
//! The headers are the configuration. They come, later ones winning, from
//! the example craft of the target release (`sysinfo`), from the headers the
//! session carries when it was decoded from a Betaflight log, from the typed
//! settings of the session where they say something else than those
//! headers, and from the caller's overrides. A session decoded from a log of
//! the target release keeps its own header list, line for line.

use super::sysinfo::{self, Build};
use super::{Target, WriteOptions};
use crate::headers::LOG_START_MARKER;
use crate::profiles::common;
use openpid_core::settings::{FilterChain, Lowpass, LowpassKind, PidGains};
use openpid_core::{FirmwareInfo, FirmwareVendor, FlightSession, RawHeaders, Settings};
use std::collections::HashSet;

/// Lowest motor output of the example craft (`motorOutput`), `DShot` units.
const MOTOR_LOW: f64 = 158.0;
/// Highest motor output of the example craft.
const MOTOR_HIGH: f64 = 2047.0;
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

    /// `rcCommand` of the session's channels: as a fraction of full
    /// deflection when the log it was decoded from declared a scale, in the
    /// firmware's old units (±500, throttle 1000 … 2000) otherwise.
    pub(super) fn rc_is_normalised(&self) -> bool {
        self.betaflight
            && self
                .raw
                .get_f64("rc_command_scale")
                .is_some_and(|s| s > 0.0)
    }
}

/// The header lines after the field definitions, in order.
#[derive(Debug, Clone, Default)]
pub(super) struct Lines {
    lines: Vec<(String, String)>,
    /// Lines whose value the session's own log or the caller gave.
    given: HashSet<String>,
}

impl Lines {
    /// Value of the line `key`.
    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.lines
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

    /// Replaces the value of the line `key` if the release has the line.
    pub(super) fn replace(&mut self, key: &str, value: impl Into<String>) {
        if let Some(entry) = self.lines.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value.into();
        }
    }

    /// Replaces the line `key`, or appends it.
    fn set(&mut self, key: &str, value: &str) {
        match self.lines.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.to_string(),
            None => self.lines.push((key.to_string(), value.to_string())),
        }
    }

    /// Drops the line `key`.
    pub(super) fn remove(&mut self, key: &str) {
        self.lines.retain(|(k, _)| k != key);
    }

    /// The lines, in order.
    pub(super) fn into_vec(self) -> Vec<(String, String)> {
        self.lines
    }

    /// The lines as the decoder's profile reads them.
    pub(super) fn as_raw(&self) -> RawHeaders {
        RawHeaders(self.lines.clone())
    }
}

/// What the session carries that only some builds of the firmware log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Carried {
    /// A battery voltage.
    pub voltage: bool,
    /// A battery current.
    pub current: bool,
    /// The S term of a wing.
    pub wing: bool,
    /// GPS fixes.
    pub gps: bool,
    /// A compass.
    pub mag: bool,
    /// A barometer.
    pub baro: bool,
}

/// The build options of the firmware that logs what the session carries.
fn build(target: Target, carried: Carried) -> Vec<Build> {
    let mut on = sysinfo::stock_build(target);
    let mut add = |wanted: bool, option: Build| {
        if wanted && !on.contains(&option) {
            on.push(option);
        }
    };
    add(carried.voltage, Build::Vbat);
    add(carried.current, Build::CurrentAdc);
    add(carried.gps, Build::Gps);
    add(carried.mag, Build::Mag);
    add(carried.baro, Build::Baro);
    if carried.wing && target.assembled_from_options() {
        on.retain(|b| *b != Build::NotWing);
        on.extend([Build::Wing, Build::AdvancedTpa]);
    }
    on
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
/// as switched off, so the log says exactly what the session says.
fn filter_headers(out: &mut Lines, prefix: &str, chain: &FilterChain) {
    for (slot, lowpass) in [(1, &chain.lowpass1), (2, &chain.lowpass2)] {
        let code = lowpass.as_ref().map_or(0.0, lowpass_code);
        let cutoff = lowpass.as_ref().map_or(0.0, |l| l.cutoff_hz);
        out.replace(&format!("{prefix}_lpf{slot}_type"), number(code));
        out.replace(&format!("{prefix}_lpf{slot}_static_hz"), number(cutoff));
    }
    let first = chain.lowpass1.as_ref();
    let range = [
        first.and_then(|l| l.dyn_min_hz).unwrap_or(0.0),
        first.and_then(|l| l.dyn_max_hz).unwrap_or(0.0),
    ];
    out.replace(&format!("{prefix}_lpf1_dyn_hz"), list(&range));
    if let Some(l) = first.filter(|l| l.dyn_min_hz.is_some()) {
        let key = format!("{prefix}_lpf1_dyn_expo");
        out.replace(&key, number(l.dyn_expo * 10.0));
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
    out.replace(&format!("{prefix}_notch_hz"), padded(centres));
    out.replace(&format!("{prefix}_notch_cutoff"), padded(cutoffs));
}

/// The gains. To 4.5 the firmware logs `d_min` beside the D gain, from
/// 2025.12 `d_max`; each release gets the line it has.
fn gains_headers(out: &mut Lines, gains: [&PidGains; 3]) {
    for (key, g) in ["rollPID", "pitchPID", "yawPID"].into_iter().zip(gains) {
        out.replace(key, list(&[g.p, g.i, g.d]));
    }
    out.replace("d_max", list(&gains.map(|g| g.d_max.unwrap_or(0.0))));
    out.replace("d_min", list(&gains.map(|g| g.d_min.unwrap_or(0.0))));
    out.replace("ff_weight", list(&gains.map(|g| g.f.unwrap_or(0.0))));
}

/// Headers of the notches that follow the motors.
fn notch_headers(out: &mut Lines, chain: &FilterChain) {
    let notch = chain.dyn_notch;
    let count = notch.map_or(0, |d| d.count.unwrap_or(1));
    out.replace("dyn_notch_count", count.to_string());
    if let Some(d) = notch {
        out.replace("dyn_notch_min_hz", number(d.min_hz));
        out.replace("dyn_notch_max_hz", number(d.max_hz));
        out.replace("dyn_notch_q", number(d.q * 100.0));
    }
    let rpm = chain.rpm_notch.as_ref();
    let harmonics = rpm.map_or(0, |r| r.harmonics);
    out.replace("rpm_filter_harmonics", harmonics.to_string());
    let Some(r) = rpm else {
        return;
    };
    out.replace("rpm_filter_q", number(r.q * 100.0));
    out.replace("rpm_filter_min_hz", number(r.min_hz));
    out.replace("rpm_filter_fade_range_hz", number(r.fade_range_hz));
    if r.lpf_hz > 0.0 {
        out.replace("rpm_filter_lpf_hz", number(r.lpf_hz));
    }
    if !r.weights.is_empty() {
        let weights: Vec<f64> = r.weights.iter().map(|w| w * 100.0).collect();
        out.replace("rpm_filter_weights", list(&weights));
    }
}

/// Lowest and highest motor output the settings declare.
fn motor_range(settings: &Settings) -> (f64, f64) {
    match (settings.motor.output_low, settings.motor.output_high) {
        (Some(low), Some(high)) if high > low => (low, high),
        _ => (MOTOR_LOW, MOTOR_HIGH),
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
            out.replace("motor_poles", poles.to_string());
        }
        let (low, high) = motor_range(settings);
        out.replace("motorOutput", list(&[low, high]));
    }
    if let Some(mode) = &settings.debug_mode
        && changed(&|l| l.debug_mode.as_ref() == Some(mode))
    {
        out.replace("debug_mode", mode.code.to_string());
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
            out.replace(key, v.clone());
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
/// loop rates, the field selection and the reference voltage follow).
pub(super) fn lines(
    session: &FlightSession,
    source: &Source,
    target: Target,
    carried: Carried,
    options: &WriteOptions,
) -> Lines {
    let mut out = Lines::default();
    if source.same_release {
        // the craft's own list: whatever its build has, in its order
        for (key, value) in &source.raw.0 {
            if key != "Product" && key != "Data version" && !key.starts_with("Field ") {
                out.lines.push((key.clone(), value.clone()));
                out.given.insert(key.clone());
            }
        }
    } else {
        let build = build(target, carried);
        for line in sysinfo::lines(target) {
            let Some(value) = line.value else {
                continue;
            };
            if !line.when.iter().all(|b| build.contains(b)) {
                continue;
            }
            let carried_over = source
                .raw
                .get(line.name)
                .filter(|_| source.betaflight)
                .filter(|_| !IDENTITY.contains(&line.name) && !COMPUTED.contains(&line.name));
            if carried_over.is_some() {
                out.given.insert(line.name.to_string());
            }
            let value = carried_over.unwrap_or(value);
            out.lines.push((line.name.to_string(), value.to_string()));
        }
        if source.betaflight {
            let mask = translated_mask(source, target);
            out.replace("fields_disabled_mask", mask.to_string());
        }
    }
    settings_headers(&mut out, session, source);
    for (key, value) in &options.headers {
        out.set(key, value);
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
        assert!(older.betaflight && !older.same_release && older.rc_is_normalised());
        let other = source(&[PRODUCT, ("Firmware revision", "INAV 7.0.0 (x) MATEKF405")]);
        assert!(other.decoded && !other.betaflight && !other.rc_is_normalised());
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

    #[test]
    fn a_wing_build_has_the_wing_lines_instead_of_the_multirotor_ones() {
        let session = openpid_sim_free_session();
        let src = Source::of(&session, Target::Master);
        let options = WriteOptions::default();
        let quad = lines(&session, &src, Target::Master, Carried::default(), &options);
        let wing = Carried {
            wing: true,
            ..Carried::default()
        };
        let wing = lines(&session, &src, Target::Master, wing, &options);
        assert!(quad.get("ap_hover_throttle").is_some() && quad.get("s_roll").is_none());
        assert!(wing.get("ap_hover_throttle").is_none() && wing.get("s_roll").is_some());
        assert!(wing.get("ap_wing_cruise_speed").is_some());
        // no voltage: no battery lines; a compass brings its own
        assert!(quad.get("vbatref").is_none() && quad.get("mag_hardware").is_none());
        let carried = Carried {
            voltage: true,
            mag: true,
            ..Carried::default()
        };
        let with = lines(&session, &src, Target::Master, carried, &options);
        assert!(with.get("vbatref").is_some() && with.get("mag_hardware").is_some());
        assert_eq!(with.number("vbatcellvoltage", 2), 430);
        assert_eq!(with.int("no such line"), 0);
    }
}
