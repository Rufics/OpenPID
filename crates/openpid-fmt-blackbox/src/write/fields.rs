//! From the session to the fields of the log: which of the fields the target
//! release can log this craft does log, and their values as the firmware's
//! integers.
//!
//! The headers decide, as on a flight controller: a field is logged when the
//! release's condition for it holds for the configuration the headers
//! describe. The session supplies the values. Where it has none for a group
//! the configuration would log, the group is switched off in
//! `fields_disabled_mask` (or, for a sensor, the craft has no such sensor),
//! so the headers never promise what the frames do not hold. The values are
//! found through the decoder's own field mapping, inverted: what the reader
//! makes of a log is what the writer takes back.

use super::config::{FEATURE_GPS, Lines, MOTOR_HIGH, MOTOR_LOW, Source};
use super::sysinfo::{CURRENT_METER_ADC, VBAT};
use super::version::{self, Craft, FieldDef, Select, When};
use super::{Schedule, Target, WriteError};
use crate::profiles::common::{self, Scales};
use openpid_core::session::AuxTable;
use openpid_core::{
    Axis, BatteryField, Channel, ChannelId, FirmwareInfo, FlightSession, GyroStage, Origin, PidTerm,
};

/// Rate at full stick of the linear rates the writer assumes, deg/s:
/// the stick a setpoint stands for when the session has no stick channel.
const FULL_STICK_RATE: f64 = 1000.0;

/// Counts per g the writer logs the accelerometer in when the session's
/// headers name no `acc_1G`: the resolution of the common ±16 g sensors.
const ACC_1G: i64 = 2048;
/// Motor poles the writer makes eRPM of a mechanical speed with when the
/// session names none.
const MOTOR_POLES: i64 = 14;

/// Kind of the decoder's table of GPS frames.
pub(super) const GPS_TABLE: &str = "gps";
/// Kind of the decoder's table of GPS home frames.
pub(super) const HOME_TABLE: &str = "gps_home";
/// Kind of the decoder's table of slow frames.
pub(super) const SLOW_TABLE: &str = "slow";

/// The channel `id` when it is a source of its own: not one the decoder's
/// profile derived from other channels of the same log (the unfiltered gyro
/// it takes from a debug mode, for one), which the log did not hold.
fn primary<'a>(s: &'a FlightSession, id: &ChannelId) -> Option<&'a Channel> {
    s.channel(id)
        .filter(|c| !matches!(&c.origin, Origin::Derived { from, .. } if !from.is_empty()))
}

fn has(s: &FlightSession, id: &ChannelId) -> bool {
    primary(s, id).is_some()
}

fn custom(name: &str) -> ChannelId {
    ChannelId::Custom(name.to_string())
}

fn has_custom(s: &FlightSession, name: &str, count: usize) -> bool {
    (0..count).any(|i| has(s, &custom(&format!("{name}[{i}]"))))
}

/// The table of kind `kind` among the session's low-rate tables.
pub(super) fn table<'a>(s: &'a FlightSession, kind: &str) -> Option<&'a AuxTable> {
    s.aux.iter().find(|t| t.kind == kind)
}

fn has_rows(s: &FlightSession, kind: &str) -> bool {
    table(s, kind).is_some_and(|t| !t.rows.is_empty())
}

/// Number of motors the session carries (0 … 8).
fn motor_count(s: &FlightSession) -> u8 {
    (0..8u8)
        .take_while(|idx| has(s, &ChannelId::Motor { idx: *idx }))
        .count() as u8
}

/// Whether the motors report a speed: an eRPM channel as a log has it, or a
/// mechanical speed that is not zero throughout (a simulated craft at rest
/// has ESCs that report nothing).
fn has_rpm(s: &FlightSession, motors: u8) -> bool {
    (0..motors).any(|idx| {
        has(s, &ChannelId::Erpm { idx })
            || s.channel(&ChannelId::MotorRpm { idx })
                .is_some_and(|c| c.data.iter().flatten().any(|v| v > 0.0))
    })
}

/// The frames of the log: the fields of each kind this craft logs.
#[derive(Debug, Clone)]
pub(super) struct Layout {
    /// The fields of the main frames.
    pub main: Vec<FieldDef>,
    /// The fields of the GPS home and GPS frames, when the GPS is logged.
    pub gps: Option<(Vec<FieldDef>, Vec<FieldDef>)>,
    /// The fields of the slow frames.
    pub slow: Vec<FieldDef>,
}

/// Whether the session has a channel of its own for any axis of `id`.
fn any_axis(s: &FlightSession, id: impl Fn(Axis) -> ChannelId) -> bool {
    Axis::ALL.into_iter().any(|axis| has(s, &id(axis)))
}

/// The craft the headers describe, before the session has had its say: the
/// field selection, the gains that switch the D and S terms on, the debug
/// mode, and whether the GPS frames carry a time.
fn configured(s: &FlightSession, target: Target, schedule: Schedule, lines: &Lines) -> Craft {
    let mut craft = Craft {
        mask: lines.int("fields_disabled_mask") as u32,
        gps_time: target.gps_frames_have_time(
            schedule.i_interval,
            schedule.p_interval,
            lines.int("P ratio"),
        ),
        debug: lines.int("debug_mode") != 0,
        ..Craft::default()
    };
    let gains = ["rollPID", "pitchPID", "yawPID"];
    for (axis, key) in Axis::ALL.into_iter().zip(gains) {
        let term = PidTerm::D;
        craft.d_gain[axis.index()] = match lines.get(key) {
            Some(_) => lines.number(key, 2) != 0,
            // The gains are not known. The firmware logs a D term where the
            // gain is not zero; its own gains have one on roll and pitch and
            // none on yaw (`PID_ROLL_DEFAULT`, `PID_PITCH_DEFAULT`,
            // `PID_YAW_DEFAULT` in `src/main/flight/pid.h`, in 4.2 the
            // defaults of `pid.c`: the same at all seven tags). A session
            // with a D term on an axis had a gain there.
            None => axis != Axis::Yaw || has(s, &ChannelId::PidTerm { axis, term }),
        };
    }
    for (axis, key) in ["s_roll", "s_pitch", "s_yaw"].into_iter().enumerate() {
        craft.s_gain[axis] = lines.int(key) != 0;
    }
    craft
}

/// The sensors and outputs the craft has: those the session has values of.
fn fit_out(craft: &mut Craft, s: &FlightSession) {
    craft.vbat = has(s, &ChannelId::Battery(BatteryField::Voltage));
    craft.amperage = has(s, &ChannelId::Battery(BatteryField::Current));
    craft.mag = has_custom(s, "magADC", 3);
    craft.baro = has(s, &custom("baroAlt")) || has(s, &custom("BaroAlt"));
    craft.rangefinder = has(s, &custom("surfaceRaw"));
    craft.rssi = has(s, &ChannelId::Rssi);
    craft.pitot = has_custom(s, "pitot", 2);
    craft.motors = motor_count(s);
    craft.tricopter = has(s, &custom("servo[5]"));
    craft.servos = has_custom(s, "servo", 8);
}

/// Switches off, in the mask, the groups the configuration would log and
/// the session has no values for: the stick commands, the setpoints, the
/// unfiltered gyro, the accelerometer or the attitude when only the other
/// one is there, the debug values of a set debug mode, the motors.
fn switch_off_what_is_missing(
    craft: &mut Craft,
    s: &FlightSession,
    target: Target,
) -> Result<(), WriteError> {
    let stick = (0..4u8).any(|ch| has(s, &ChannelId::RcCommand { ch }));
    let setpoint = any_axis(s, |axis| ChannelId::Setpoint { axis });
    let throttle = has(s, &custom("setpoint[3]")) || s.channel(&ChannelId::Throttle).is_some();
    if !(stick || setpoint) {
        craft.switch_off(target, Select::RcCommands);
    }
    if !(setpoint || throttle) {
        craft.switch_off(target, Select::Setpoint);
    }
    let gyro = |stage: GyroStage| any_axis(s, |axis| ChannelId::Gyro { axis, stage });
    if !gyro(GyroStage::Filtered) && !craft.is_off(target, Select::Gyro) {
        return Err(WriteError::MissingChannel("filtered gyro"));
    }
    if !gyro(GyroStage::Raw) {
        craft.switch_off(target, Select::GyroUnfilt);
    }
    // one sensor behind two groups: the accelerometer and the attitude
    let acc = any_axis(s, |axis| ChannelId::Acc { axis });
    let attitude = has_custom(s, "imuQuaternion", 3);
    craft.acc = acc || attitude;
    if craft.acc && !acc {
        craft.switch_off(target, Select::Acc);
    }
    if craft.acc && !attitude {
        craft.switch_off(target, Select::Attitude);
    }
    if craft.debug && !(0..8u8).any(|idx| has(s, &ChannelId::Debug { idx })) {
        craft.switch_off(target, Select::DebugLog);
    }
    if craft.motors == 0 {
        craft.switch_off(target, Select::Motor);
    }
    Ok(())
}

/// Whether the ESCs report their speed: `dshot_bidir`, which follows the
/// session unless its log or the caller gave it. Telemetry without speeds to
/// log has the RPM fields switched off.
fn telemetry(craft: &mut Craft, s: &FlightSession, target: Target, lines: &mut Lines) {
    let speeds = has_rpm(s, craft.motors);
    if !lines.is_given("dshot_bidir") {
        lines.put("dshot_bidir", u8::from(speeds).to_string());
    }
    craft.dshot_telemetry = lines.int("dshot_bidir") != 0;
    if craft.dshot_telemetry && !speeds {
        craft.switch_off(target, Select::Rpm);
    }
    // eRPM made from a mechanical speed: the log states the pole count the
    // writer made it with
    let made = (0..craft.motors).any(|idx| !has(s, &ChannelId::Erpm { idx }));
    if craft.logs(target, When::Rpm(1)) && made && lines.get("motor_poles").is_none() {
        lines.put("motor_poles", MOTOR_POLES.to_string());
    }
}

/// Whether the GPS frames are logged. A decoded log has the tables of the
/// GPS frames exactly when its headers defined them; any other session logs
/// the GPS when it has fixes (the feature is then switched on in `features`)
/// and the mask does not switch it off.
fn gps_is_logged(
    craft: &Craft,
    s: &FlightSession,
    source: &Source,
    target: Target,
    lines: &mut Lines,
) -> bool {
    let fixes = has_rows(s, GPS_TABLE) || has_rows(s, HOME_TABLE);
    if fixes && !lines.is_given("features") {
        lines.put(
            "features",
            (lines.int("features") | FEATURE_GPS).to_string(),
        );
    }
    if source.decoded {
        table(s, GPS_TABLE).is_some() || table(s, HOME_TABLE).is_some()
    } else {
        lines.int("features") & FEATURE_GPS != 0 && !craft.is_off(target, Select::Gps)
    }
}

/// Makes the headers say what the craft logs: the mask; the scales of the
/// motors and of the accelerometer when the session named none (the writer
/// then logs in its own: the `DShot` range, and 2048 counts per g); and no
/// battery lines for a craft whose voltage or current is not logged.
fn state(craft: &Craft, target: Target, lines: &mut Lines) {
    lines.put("fields_disabled_mask", craft.mask.to_string());
    if craft.logs(target, When::Motors(1)) && lines.get("motorOutput").is_none() {
        lines.put("motorOutput", format!("{MOTOR_LOW},{MOTOR_HIGH}"));
    }
    if craft.logs(target, When::Acc) && lines.get("acc_1G").is_none() {
        lines.put("acc_1G", ACC_1G.to_string());
    }
    if !craft.logs(target, When::Vbat) {
        // the firmware skips these with the field
        lines.remove_all_on(VBAT);
    }
    if !craft.logs(target, When::Amperage) && !lines.is_given("currentSensor") {
        // a craft without a current meter has no ADC to describe
        lines.remove_all_on(CURRENT_METER_ADC);
    }
}

/// Decides which fields are logged and makes the headers say so: writes
/// `fields_disabled_mask`, `dshot_bidir` and `features`, and drops the
/// battery lines of a craft whose voltage is not logged.
pub(super) fn layout(
    s: &FlightSession,
    source: &Source,
    target: Target,
    schedule: Schedule,
    lines: &mut Lines,
) -> Result<Layout, WriteError> {
    let mut craft = configured(s, target, schedule, lines);
    fit_out(&mut craft, s);
    switch_off_what_is_missing(&mut craft, s, target)?;
    telemetry(&mut craft, s, target, lines);
    let gps = gps_is_logged(&craft, s, source, target, lines);
    state(&craft, target, lines);
    let logged = |fields: Vec<FieldDef>| -> Vec<FieldDef> {
        fields
            .into_iter()
            .filter(|f| craft.logs(target, f.when))
            .collect()
    };
    Ok(Layout {
        main: logged(version::main_fields(target)),
        gps: gps.then(|| {
            (
                logged(version::home_fields(target)),
                logged(version::gps_fields(target)),
            )
        }),
        slow: version::slow_fields(),
    })
}

/// Rounds the way the firmware stores a float in a log field (`lrintf`).
fn quantise(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// What the values of the fields are computed with.
#[derive(Debug)]
struct Units {
    /// The scales the decoder's profile reads from the headers written.
    scales: Scales,
    /// `rc_command_scale` of the log written, when it has one.
    rc_scale: Option<f64>,
    /// 10 in high-resolution mode, else 1.
    high_resolution: f64,
    /// Motor poles (`motor_poles`).
    poles: f64,
}

/// Values of `id` through `f`, or `fallback` where the channel has none.
fn series(c: &Channel, n: usize, fallback: i64, f: impl Fn(f64) -> f64) -> Vec<i64> {
    (0..n)
        .map(|i| c.data.get(i).map_or(fallback, |v| quantise(f(v))))
        .collect()
}

/// `rcCommand[ch]`. From the session's stick channel, which is in the
/// firmware's old units whatever log it came from (±500, the throttle
/// 1000 … 2000: the reader brings a log with `rc_command_scale` back to
/// them), as a fraction of full deflection for a target that logs one;
/// without a stick channel,
/// from the setpoint through linear rates (full stick asks for
/// [`FULL_STICK_RATE`]) and from the throttle.
fn rc_command(s: &FlightSession, ch: u8, n: usize, units: &Units) -> Vec<i64> {
    let hires = units.high_resolution;
    // a fraction of full deflection in the units of the target
    let scaled = |fraction: f64| match units.rc_scale {
        Some(scale) => fraction * scale * hires,
        None if ch == 3 => (1000.0 + fraction * 1000.0) * hires,
        None => fraction * 500.0 * hires,
    };
    if let Some(c) = primary(s, &ChannelId::RcCommand { ch }) {
        return match units.rc_scale {
            Some(_) if ch == 3 => series(c, n, 0, |v| scaled((v - 1000.0) / 1000.0)),
            Some(_) => series(c, n, 0, |v| scaled(v / 500.0)),
            // the same units: the inverse of the decoder's scale
            None => series(c, n, 0, |v| v * (1.0 / units.scales.rc)),
        };
    }
    if ch == 3 {
        let Some(c) = s.channel(&ChannelId::Throttle) else {
            return vec![quantise(scaled(0.0)); n];
        };
        return match units.rc_scale {
            Some(_) => series(c, n, 0, |v| scaled(v.clamp(0.0, 100.0) / 100.0)),
            // tenths of a percent above 1000, as the setpoint logs them
            None => series(c, n, quantise(1000.0 * hires), |v| {
                (1000.0 + (v.clamp(0.0, 100.0) * 10.0).round_ties_even()) * hires
            }),
        };
    }
    let Some(axis) = Axis::from_index(usize::from(ch)) else {
        return vec![0; n];
    };
    match primary(s, &ChannelId::Setpoint { axis }) {
        Some(c) => series(c, n, 0, |v| scaled((v / FULL_STICK_RATE).clamp(-1.0, 1.0))),
        None => vec![0; n],
    }
}

/// The values of one field for every sample of the session.
fn values(s: &FlightSession, def: &FieldDef, n: usize, units: &Units) -> Vec<i64> {
    let (base, index) = common::split_indexed(&def.name);
    let column = match (base, index) {
        ("rcCommand", Some(ch)) => rc_command(s, ch as u8, n, units),
        ("setpoint", Some(3)) if !has(s, &custom("setpoint[3]")) => {
            // the mixer throttle in tenths of a percent
            match s.channel(&ChannelId::Throttle) {
                Some(c) => series(c, n, 0, |v| v.clamp(0.0, 100.0) * 10.0),
                None => vec![0; n],
            }
        }
        ("motor", Some(idx)) => {
            let (low, high) = (units.scales.motor_low, units.scales.motor_high);
            match primary(s, &ChannelId::Motor { idx: idx as u8 }) {
                // percent between the outputs the header declares
                Some(c) if high > low => series(c, n, low as i64, |v| {
                    (low + v / 100.0 * (high - low)).clamp(0.0, high)
                }),
                Some(c) => series(c, n, 0, |v| v),
                None => vec![low as i64; n],
            }
        }
        ("eRPM", Some(idx)) => {
            let idx = idx as u8;
            if let Some(c) = primary(s, &ChannelId::Erpm { idx }) {
                series(c, n, 0, |v| v.max(0.0))
            } else if let Some(c) = s.channel(&ChannelId::MotorRpm { idx }) {
                // mechanical rpm → electrical rpm / 100
                series(c, n, 0, |v| v.max(0.0) * units.poles / 200.0)
            } else {
                vec![0; n]
            }
        }
        // one field under the two names it has had
        ("baroAlt" | "BaroAlt", None) => {
            match primary(s, &custom("baroAlt")).or_else(|| primary(s, &custom("BaroAlt"))) {
                Some(c) => series(c, n, 0, |v| v),
                None => vec![0; n],
            }
        }
        _ => {
            // the decoder's mapping of the field, inverted; a field it does
            // not map is a custom channel under its own name
            let (id, scale, offset) = match common::map_field(&def.name, &units.scales) {
                Some(m) => (m.id, m.scale, m.offset),
                None => (custom(&def.name), 1.0, 0.0),
            };
            // times the reciprocal: 1 / 0.01 is exactly 100, and a value
            // logged as 16.75 V must come back as 1675, not 1674.99…
            let per_unit = 1.0 / scale;
            match primary(s, &id) {
                Some(c) => series(c, n, 0, |v| v * per_unit - offset),
                None => vec![0; n],
            }
        }
    };
    column.into_iter().map(|v| def.store.limit(v)).collect()
}

/// One column of integers per field of the main frames. The first two
/// (`loopIteration` and `time`) are left to the caller, who knows the
/// schedule.
pub(super) fn columns(
    s: &FlightSession,
    layout: &Layout,
    lines: &Lines,
    n: usize,
) -> Vec<Vec<i64>> {
    let raw = lines.as_raw();
    let firmware = FirmwareInfo::parse_revision(raw.get("Firmware revision").unwrap_or(""));
    let poles = lines.int("motor_poles");
    let units = Units {
        scales: Scales::betaflight(&firmware, &raw),
        rc_scale: raw.get_f64("rc_command_scale").filter(|s| *s > 0.0),
        high_resolution: if lines.int("blackbox_high_resolution") == 1 {
            10.0
        } else {
            1.0
        },
        poles: if poles > 0 { poles } else { MOTOR_POLES } as f64,
    };
    layout
        .main
        .iter()
        .enumerate()
        .map(|(i, def)| {
            if i < 2 {
                vec![0; n]
            } else {
                values(s, def, n, &units)
            }
        })
        .collect()
}
