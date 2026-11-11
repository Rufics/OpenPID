//! Writer of Betaflight Blackbox logs: a canonical session becomes the
//! bytes a flight controller would have logged for it.
//!
//! This is the inverse of the decoder and of the Betaflight profile: the
//! channels are turned back into the firmware's integers (gyro and setpoint
//! in whole deg/s, throttle in tenths of a percent, motors in `DShot` units,
//! eRPM / 100), the settings into headers, and the frames are packed with the
//! predictors and encodings Betaflight itself uses, in the order it writes
//! them: an `I` frame every `I interval` loop iterations and `P` frames
//! between, the arming beep, a slow frame, and the end-of-log event. A reader
//! of real logs therefore meets nothing it has not met before.
//!
//! It exists for synthetic flights (`openpid gen`, the calibration probes of
//! `docs/calibration`): a signal whose content is known exactly can be opened
//! in any tool that reads Blackbox logs. What that tool then sees is the
//! quantised signal; decoding the written log gives exactly those values.
//!
//! No firmware code is copied: the layout follows the format description
//! ("Blackbox Internals") and the headers of real logs.

mod template;

use crate::encoding::{self as enc, encode};
use crate::predictor;
use openpid_core::settings::{FilterChain, LoopRates, Lowpass, LowpassKind, PidGains};
use openpid_core::{Axis, BatteryField, ChannelId, FlightSession, GyroStage, PidTerm, Series};

/// Lowest motor output of the template (`motorOutput`), `DShot` units.
const MOTOR_LOW: f64 = 158.0;
/// Highest motor output of the template.
const MOTOR_HIGH: f64 = 2047.0;
/// Rate at full stick of the linear rates the writer logs, deg/s.
const FULL_STICK_RATE: f64 = 1000.0;
/// `I` frames between two slow frames, as the firmware spaces them.
const SLOW_EVERY_INTRA: i64 = 256;
/// Event: the beep at arming, which marks the start of the log.
const EVENT_SYNC_BEEP: u8 = 0;
/// Event: logging resumed after a pause.
const EVENT_LOGGING_RESUME: u8 = 14;
/// Event: the craft was disarmed.
const EVENT_DISARM: u8 = 15;
/// Event: end of the log.
const EVENT_LOG_END: u8 = 255;

/// What the caller may set beyond the session itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// Added to every timestamp, µs: a flight controller logs the time since
    /// it was powered, not since the log began.
    pub time_offset_us: i64,
    /// Headers that replace the written ones by name (or are appended), for
    /// what the session does not model: rates, the craft name, a debug mode.
    pub headers: Vec<(String, String)>,
    /// Whether the log ends with a disarm event, as after a normal landing.
    pub disarm: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            time_offset_us: 0,
            headers: Vec::new(),
            disarm: true,
        }
    }
}

/// Why a session cannot be written as a Betaflight log.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WriteError {
    /// No sample has a valid gyro reading on all three axes.
    #[error("the session has no sample with a gyro reading on every axis")]
    Empty,
    /// A channel the log cannot do without is absent.
    #[error("the session has no {0} channel")]
    MissingChannel(&'static str),
    /// The loop rates do not form a schedule the firmware can log.
    #[error(
        "a {log_hz} Hz log of a {pid_hz} Hz PID loop cannot be written: the PID rate must be a \
         whole multiple of the log rate that divides the I-frame interval"
    )]
    Rate {
        /// Rate of the logged frames.
        log_hz: f64,
        /// Rate of the PID loop.
        pid_hz: f64,
    },
    /// A timestamp is negative or does not fit the 32 bits of the format.
    #[error("the timestamp {0} µs does not fit a Blackbox log")]
    Time(i64),
}

/// The loop rates a Betaflight craft would most likely run to log at
/// `log_hz`: an 8 kHz gyro with a 4 kHz PID loop logged at every first,
/// second, fourth, … iteration, and one loop at the log rate for anything
/// else (an 8 kHz log among them).
pub fn typical_rates(log_hz: f64) -> LoopRates {
    let ratio = 4000.0 / log_hz;
    let whole = ratio.round();
    let (gyro_hz, pid_hz) = if (ratio - whole).abs() < 1e-9
        && (1.0..=32.0).contains(&whole)
        && (whole as u32).is_power_of_two()
    {
        (8000.0, 4000.0)
    } else {
        (log_hz, log_hz)
    };
    LoopRates {
        gyro_hz: Some(gyro_hz),
        pid_hz: Some(pid_hz),
        log_hz: Some(log_hz),
    }
}

/// One main-frame field: its name and how the firmware packs it.
#[derive(Debug, Clone)]
struct Field {
    /// Field name as logged.
    name: String,
    /// Whether the value is signed.
    signed: bool,
    /// Predictor and encoding in an `I` frame.
    intra: (u8, u8),
    /// Predictor and encoding in a `P` frame.
    inter: (u8, u8),
    /// One value per sample.
    values: Vec<i64>,
}

impl Field {
    /// A signed field that the firmware predicts from the previous frame.
    fn signed(name: String, inter_encoding: u8, values: Vec<i64>) -> Field {
        Field {
            name,
            signed: true,
            intra: (predictor::ZERO, enc::SIGNED_VB),
            inter: (predictor::PREVIOUS, inter_encoding),
            values,
        }
    }

    /// A signed field predicted as the mean of the two previous frames
    /// (the sensors: they are noisy, and the mean halves the residual).
    fn sensor(name: String, values: Vec<i64>) -> Field {
        Field {
            name,
            signed: true,
            intra: (predictor::ZERO, enc::SIGNED_VB),
            inter: (predictor::AVERAGE_2, enc::SIGNED_VB),
            values,
        }
    }

    /// An unsigned field predicted from the previous frame.
    fn unsigned(name: String, inter_encoding: u8, values: Vec<i64>) -> Field {
        Field {
            name,
            signed: false,
            intra: (predictor::ZERO, enc::UNSIGNED_VB),
            inter: (predictor::PREVIOUS, inter_encoding),
            values,
        }
    }
}

/// The schedule of the frames: which loop iterations are logged.
#[derive(Debug, Clone, Copy)]
struct Schedule {
    /// Loop iterations between two `I` frames.
    i_interval: i64,
    /// Loop iterations between two logged frames.
    p_interval: i64,
    /// Gyro loop period, µs.
    looptime_us: i64,
    /// Gyro loops per PID loop.
    pid_denom: i64,
}

impl Schedule {
    fn new(rates: &LoopRates, sampled_hz: f64) -> Result<Schedule, WriteError> {
        let log_hz = rates.log_hz.unwrap_or(sampled_hz);
        let pid_hz = rates.pid_hz.unwrap_or(log_hz);
        let gyro_hz = rates.gyro_hz.unwrap_or(pid_hz);
        let bad = WriteError::Rate { log_hz, pid_hz };
        if !(log_hz > 0.0 && pid_hz >= log_hz && gyro_hz >= pid_hz) {
            return Err(bad);
        }
        let ratio = pid_hz / log_hz;
        let p_interval = ratio.round() as i64;
        let pid_ratio = gyro_hz / pid_hz;
        let pid_denom = pid_ratio.round() as i64;
        // the firmware: 32 ms of PID loops between two I frames
        let i_interval = ((32.0 * pid_hz / 1000.0).floor() as i64).max(1);
        if (ratio - p_interval as f64).abs() > 1e-6
            || (pid_ratio - pid_denom as f64).abs() > 1e-6
            || i_interval % p_interval != 0
        {
            return Err(bad);
        }
        Ok(Schedule {
            i_interval,
            p_interval,
            looptime_us: (1.0e6 / gyro_hz).round() as i64,
            pid_denom,
        })
    }
}

/// Rounds the way the firmware stores a float in a log field.
fn quantise(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// Values of `id` through `f`, or `fallback` where the channel is absent or
/// has no value.
fn column(
    s: &FlightSession,
    id: &ChannelId,
    n: usize,
    fallback: i64,
    f: impl Fn(f64) -> f64,
) -> Vec<i64> {
    let data = s.channel(id).map(|c| &c.data);
    (0..n)
        .map(|i| {
            data.and_then(|d| d.get(i))
                .map_or(fallback, |v| quantise(f(v)))
        })
        .collect()
}

fn has(s: &FlightSession, id: &ChannelId) -> bool {
    s.channel(id).is_some()
}

/// Number of motors the session carries (0 … 8).
fn motor_count(s: &FlightSession) -> u8 {
    (0..8u8)
        .take_while(|idx| has(s, &ChannelId::Motor { idx: *idx }))
        .count() as u8
}

/// Whether any motor reports a speed: without one the firmware has no
/// bidirectional `DShot` and logs no eRPM.
fn has_rpm(s: &FlightSession, motors: u8) -> bool {
    (0..motors).any(|idx| {
        [ChannelId::Erpm { idx }, ChannelId::MotorRpm { idx }]
            .iter()
            .filter_map(|id| s.channel(id))
            .any(|c| c.data.iter().flatten().any(|v| v > 0.0))
    })
}

/// Lowest and highest motor output the log declares.
fn motor_range(s: &FlightSession) -> (f64, f64) {
    match (s.settings.motor.output_low, s.settings.motor.output_high) {
        (Some(low), Some(high)) if high > low => (low, high),
        _ => (MOTOR_LOW, MOTOR_HIGH),
    }
}

/// The PID terms, sticks and setpoints: what the controller computed.
fn controller_fields(s: &FlightSession, n: usize, out: &mut Vec<Field>) {
    // the firmware logs no yaw D term
    for (term, letter, axes, encoding) in [
        (PidTerm::P, 'P', 3, enc::SIGNED_VB),
        (PidTerm::I, 'I', 3, enc::TAG2_3S32),
        (PidTerm::D, 'D', 2, enc::SIGNED_VB),
        (PidTerm::F, 'F', 3, enc::SIGNED_VB),
    ] {
        for axis in Axis::ALL.into_iter().take(axes) {
            let id = ChannelId::PidTerm { axis, term };
            out.push(Field::signed(
                format!("axis{letter}[{}]", axis.index()),
                encoding,
                column(s, &id, n, 0, |v| v),
            ));
        }
    }
    let throttle = column(s, &ChannelId::Throttle, n, 0, |v| {
        v.clamp(0.0, 100.0) * 10.0
    });
    for axis in Axis::ALL {
        let ch = axis.index() as u8;
        let values = if has(s, &ChannelId::RcCommand { ch }) {
            column(s, &ChannelId::RcCommand { ch }, n, 0, |v| v)
        } else {
            // linear rates: full stick asks for `FULL_STICK_RATE`
            column(s, &ChannelId::Setpoint { axis }, n, 0, |v| {
                (v / FULL_STICK_RATE * 500.0).clamp(-500.0, 500.0)
            })
        };
        out.push(Field::signed(
            format!("rcCommand[{ch}]"),
            enc::TAG8_4S16,
            values,
        ));
    }
    let rc_throttle = if has(s, &ChannelId::RcCommand { ch: 3 }) {
        column(s, &ChannelId::RcCommand { ch: 3 }, n, 1000, |v| v)
    } else {
        throttle.iter().map(|t| 1000 + t).collect()
    };
    out.push(Field::unsigned(
        "rcCommand[3]".into(),
        enc::TAG8_4S16,
        rc_throttle,
    ));
    for axis in Axis::ALL {
        out.push(Field::signed(
            format!("setpoint[{}]", axis.index()),
            enc::TAG8_4S16,
            column(s, &ChannelId::Setpoint { axis }, n, 0, |v| v),
        ));
    }
    out.push(Field::signed(
        "setpoint[3]".into(),
        enc::TAG8_4S16,
        throttle,
    ));
}

/// The battery, the radio link and the inertial sensors.
fn sensor_fields(s: &FlightSession, n: usize, out: &mut Vec<Field>) {
    let voltage = ChannelId::Battery(BatteryField::Voltage);
    out.push(Field {
        name: "vbatLatest".into(),
        signed: false,
        intra: (predictor::VBATREF, enc::NEG_14BIT),
        inter: (predictor::PREVIOUS, enc::TAG8_8SVB),
        values: column(s, &voltage, n, 1600, |v| v * 100.0),
    });
    let current = ChannelId::Battery(BatteryField::Current);
    out.push(Field::signed(
        "amperageLatest".into(),
        enc::TAG8_8SVB,
        column(s, &current, n, 0, |v| v * 100.0),
    ));
    out.push(Field::unsigned(
        "rssi".into(),
        enc::TAG8_8SVB,
        vec![1000; n],
    ));
    for (stage, name) in [
        (GyroStage::Filtered, "gyroADC"),
        (GyroStage::Raw, "gyroUnfilt"),
    ] {
        if !Axis::ALL
            .into_iter()
            .all(|axis| has(s, &ChannelId::Gyro { axis, stage }))
        {
            continue;
        }
        for axis in Axis::ALL {
            out.push(Field::sensor(
                format!("{name}[{}]", axis.index()),
                column(s, &ChannelId::Gyro { axis, stage }, n, 0, |v| v),
            ));
        }
    }
    // level and at rest: one g on the vertical axis, the attitude not rotated
    for (i, g) in [0, 0, 2048].into_iter().enumerate() {
        out.push(Field::sensor(format!("accSmooth[{i}]"), vec![g; n]));
    }
    for i in 0..3 {
        out.push(Field::sensor(format!("imuQuaternion[{i}]"), vec![0; n]));
    }
}

/// The motor outputs and, when the motors report it, their speed.
fn motor_fields(s: &FlightSession, n: usize, out: &mut Vec<Field>) {
    let motors = motor_count(s);
    let (low, high) = motor_range(s);
    for idx in 0..motors {
        let values = column(s, &ChannelId::Motor { idx }, n, low as i64, |v| {
            low + v.clamp(0.0, 100.0) / 100.0 * (high - low)
        });
        // the first motor against the lowest output, the others against the first
        let intra = if idx == 0 {
            (predictor::MINMOTOR, enc::UNSIGNED_VB)
        } else {
            (predictor::MOTOR_0, enc::SIGNED_VB)
        };
        out.push(Field {
            name: format!("motor[{idx}]"),
            signed: false,
            intra,
            inter: (predictor::AVERAGE_2, enc::SIGNED_VB),
            values,
        });
    }
    if !has_rpm(s, motors) {
        return;
    }
    let poles = f64::from(s.settings.motor.poles.unwrap_or(14));
    for idx in 0..motors {
        let values = if has(s, &ChannelId::Erpm { idx }) {
            column(s, &ChannelId::Erpm { idx }, n, 0, |v| v.max(0.0))
        } else {
            // mechanical rpm → electrical rpm / 100
            column(s, &ChannelId::MotorRpm { idx }, n, 0, |v| {
                v.max(0.0) * poles / 200.0
            })
        };
        out.push(Field::unsigned(
            format!("eRPM[{idx}]"),
            enc::SIGNED_VB,
            values,
        ));
    }
}

/// The main-frame fields of the session, in the firmware's order. `first` is
/// the sample of the first logged frame: the loop counter starts there.
fn fields(s: &FlightSession, first: usize, schedule: Schedule, time: Vec<i64>) -> Vec<Field> {
    let n = time.len();
    let mut out = vec![
        Field {
            name: "loopIteration".into(),
            signed: false,
            intra: (predictor::ZERO, enc::UNSIGNED_VB),
            inter: (predictor::INC, enc::NULL),
            values: (0..n)
                .map(|k| k.saturating_sub(first) as i64 * schedule.p_interval)
                .collect(),
        },
        Field {
            name: "time".into(),
            signed: false,
            intra: (predictor::ZERO, enc::UNSIGNED_VB),
            inter: (predictor::STRAIGHT_LINE, enc::SIGNED_VB),
            values: time,
        },
    ];
    controller_fields(s, n, &mut out);
    sensor_fields(s, n, &mut out);
    motor_fields(s, n, &mut out);
    out
}

/// What the predictors refer to besides the frames.
#[derive(Debug, Clone, Copy)]
struct Constants {
    /// Index of `motor[0]`.
    motor0: Option<usize>,
    /// Lowest motor output (`motorOutput`).
    motor_low: i64,
    /// Reference battery voltage (`vbatref`).
    vbatref: i64,
}

/// Packs one main frame: the inverse of the decoder's field loop. With no
/// `history` the frame is an `I` frame.
fn write_frame(
    buf: &mut Vec<u8>,
    fields: &[Field],
    row: &[i64],
    history: Option<(&[i64], &[i64])>,
    constants: Constants,
) {
    let intra = history.is_none();
    buf.push(if intra { b'I' } else { b'P' });
    let how = |i: usize| {
        if intra {
            fields[i].intra
        } else {
            fields[i].inter
        }
    };
    let (previous, previous2) = history.unwrap_or((row, row));
    let residual = |i: usize| -> i32 {
        let predicted = match how(i).0 {
            predictor::PREVIOUS => previous[i],
            predictor::STRAIGHT_LINE => 2 * previous[i] - previous2[i],
            predictor::AVERAGE_2 => previous[i].midpoint(previous2[i]),
            predictor::MOTOR_0 => constants.motor0.map_or(0, |m| row[m]),
            predictor::MINMOTOR => constants.motor_low,
            predictor::VBATREF => constants.vbatref,
            _ => 0,
        };
        (row[i] - predicted) as i32
    };
    let n = fields.len();
    let mut i = 0;
    while i < n {
        let encoding = how(i).1;
        let run = (i..n).take_while(|j| how(*j).1 == encoding).count();
        match encoding {
            enc::TAG8_8SVB => {
                let count = run.min(8);
                let values: Vec<i32> = (i..i + count).map(residual).collect();
                encode::write_tag8_8svb(buf, &values);
                i += count;
            }
            enc::TAG2_3S32 if run >= 3 => {
                encode::write_tag2_3s32(buf, [residual(i), residual(i + 1), residual(i + 2)]);
                i += 3;
            }
            enc::TAG8_4S16 if run >= 4 => {
                let group = [
                    residual(i),
                    residual(i + 1),
                    residual(i + 2),
                    residual(i + 3),
                ];
                encode::write_tag8_4s16(buf, group);
                i += 4;
            }
            enc::UNSIGNED_VB => {
                encode::write_uvb(buf, residual(i) as u32);
                i += 1;
            }
            enc::NEG_14BIT => {
                encode::write_neg14(buf, residual(i));
                i += 1;
            }
            enc::NULL => i += 1,
            _ => {
                encode::write_svb(buf, residual(i));
                i += 1;
            }
        }
    }
}

fn write_event(buf: &mut Vec<u8>, code: u8, values: &[i64]) {
    buf.push(b'E');
    buf.push(code);
    for v in values {
        encode::write_uvb(buf, *v as u32);
    }
}

/// Armed, in acro, with a healthy radio link.
fn write_slow_frame(buf: &mut Vec<u8>) {
    buf.push(b'S');
    encode::write_uvb(buf, 1); // flightModeFlags: ARM
    encode::write_uvb(buf, 0); // stateFlags
    encode::write_tag2_3s32(buf, [0, 1, 1]); // failsafe phase, rx signal, rx channels valid
}

/// Replaces the header `key`, or appends it.
fn set(headers: &mut Vec<(String, String)>, key: &str, value: impl Into<String>) {
    let value = value.into();
    match headers.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = value,
        None => headers.push((key.to_string(), value)),
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
/// as switched off, so the log says exactly what the session says.
fn filter_headers(headers: &mut Vec<(String, String)>, prefix: &str, chain: &FilterChain) {
    for (slot, lowpass) in [(1, &chain.lowpass1), (2, &chain.lowpass2)] {
        let code = lowpass.as_ref().map_or(0.0, lowpass_code);
        let cutoff = lowpass.as_ref().map_or(0.0, |l| l.cutoff_hz);
        set(headers, &format!("{prefix}_lpf{slot}_type"), number(code));
        let key = format!("{prefix}_lpf{slot}_static_hz");
        set(headers, &key, number(cutoff));
    }
    let first = chain.lowpass1.as_ref();
    let range = [
        first.and_then(|l| l.dyn_min_hz).unwrap_or(0.0),
        first.and_then(|l| l.dyn_max_hz).unwrap_or(0.0),
    ];
    set(headers, &format!("{prefix}_lpf1_dyn_hz"), list(&range));
    if let Some(l) = first.filter(|l| l.dyn_min_hz.is_some()) {
        let key = format!("{prefix}_lpf1_dyn_expo");
        set(headers, &key, number(l.dyn_expo * 10.0));
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
    set(headers, &format!("{prefix}_notch_hz"), padded(centres));
    set(headers, &format!("{prefix}_notch_cutoff"), padded(cutoffs));
}

fn gains_headers(headers: &mut Vec<(String, String)>, gains: [&PidGains; 3]) {
    for (key, g) in ["rollPID", "pitchPID", "yawPID"].into_iter().zip(gains) {
        set(headers, key, list(&[g.p, g.i, g.d]));
    }
    let d_max = gains.map(|g| g.d_max.unwrap_or(0.0));
    set(headers, "d_max", list(&d_max));
    let feedforward = gains.map(|g| g.f.unwrap_or(0.0));
    set(headers, "ff_weight", list(&feedforward));
}

/// Headers of the notches that follow the motors.
fn notch_headers(out: &mut Vec<(String, String)>, chain: &FilterChain) {
    let notch = chain.dyn_notch;
    let count = notch.map_or(0, |d| d.count.unwrap_or(1));
    set(out, "dyn_notch_count", count.to_string());
    if let Some(d) = notch {
        set(out, "dyn_notch_min_hz", number(d.min_hz));
        set(out, "dyn_notch_max_hz", number(d.max_hz));
        set(out, "dyn_notch_q", number(d.q * 100.0));
    }
    let rpm = chain.rpm_notch.as_ref();
    let harmonics = rpm.map_or(0, |r| r.harmonics);
    set(out, "rpm_filter_harmonics", harmonics.to_string());
    let Some(r) = rpm else {
        return;
    };
    set(out, "rpm_filter_q", number(r.q * 100.0));
    set(out, "rpm_filter_min_hz", number(r.min_hz));
    set(out, "rpm_filter_fade_range_hz", number(r.fade_range_hz));
    if r.lpf_hz > 0.0 {
        set(out, "rpm_filter_lpf_hz", number(r.lpf_hz));
    }
    if !r.weights.is_empty() {
        let weights: Vec<f64> = r.weights.iter().map(|w| w * 100.0).collect();
        set(out, "rpm_filter_weights", list(&weights));
    }
}

/// The whole header block of the log.
fn headers(
    s: &FlightSession,
    fields: &[Field],
    schedule: Schedule,
    options: &WriteOptions,
) -> Vec<(String, String)> {
    let join = |f: &dyn Fn(&Field) -> String| fields.iter().map(f).collect::<Vec<_>>().join(",");
    let field_headers = [
        (
            "Product",
            "Blackbox flight data recorder by Nicholas Sherlock".to_string(),
        ),
        ("Data version", "2".to_string()),
        ("Field I name", join(&|f| f.name.clone())),
        ("Field I signed", join(&|f| u8::from(f.signed).to_string())),
        ("Field I predictor", join(&|f| f.intra.0.to_string())),
        ("Field I encoding", join(&|f| f.intra.1.to_string())),
        ("Field P predictor", join(&|f| f.inter.0.to_string())),
        ("Field P encoding", join(&|f| f.inter.1.to_string())),
        (
            "Field S name",
            "flightModeFlags,stateFlags,failsafePhase,rxSignalReceived,rxFlightChannelsValid"
                .to_string(),
        ),
        ("Field S signed", "0,0,0,0,0".to_string()),
        ("Field S predictor", "0,0,0,0,0".to_string()),
        ("Field S encoding", "1,1,7,7,7".to_string()),
    ];
    let mut out: Vec<(String, String)> = field_headers
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .chain(
            template::HEADERS
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string())),
        )
        .collect();
    set(&mut out, "I interval", schedule.i_interval.to_string());
    set(&mut out, "P interval", schedule.p_interval.to_string());
    let ratio = schedule.i_interval / schedule.p_interval;
    set(&mut out, "P ratio", ratio.to_string());
    set(&mut out, "looptime", schedule.looptime_us.to_string());
    set(
        &mut out,
        "pid_process_denom",
        schedule.pid_denom.to_string(),
    );
    let settings = &s.settings;
    if let Some(pid) = &settings.pid {
        gains_headers(&mut out, [&pid.roll, &pid.pitch, &pid.yaw]);
    }
    filter_headers(&mut out, "gyro", &settings.gyro_filters);
    filter_headers(&mut out, "dterm", &settings.dterm_filters);
    notch_headers(&mut out, &settings.gyro_filters);
    let rpm = fields.iter().any(|f| f.name == "eRPM[0]");
    set(&mut out, "dshot_bidir", u8::from(rpm).to_string());
    if let Some(poles) = settings.motor.poles {
        set(&mut out, "motor_poles", poles.to_string());
    }
    let (low, high) = motor_range(s);
    set(&mut out, "motorOutput", list(&[low, high]));
    if let Some(mode) = &settings.debug_mode {
        set(&mut out, "debug_mode", mode.code.to_string());
    }
    for (key, value) in [
        ("Craft name", &settings.craft_name),
        ("Board information", &settings.board),
        ("Log start datetime", &settings.log_start),
    ] {
        if let Some(v) = value {
            set(&mut out, key, v.clone());
        }
    }
    for (key, value) in &options.headers {
        set(&mut out, key, value.clone());
    }
    out
}

/// A header value as an integer, for the constants the predictors use.
fn header_int(headers: &[(String, String)], key: &str) -> i64 {
    headers
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.split(',').next())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

/// Timestamps of the session with the offset, checked against the format.
fn timestamps(s: &FlightSession, offset_us: i64) -> Result<Vec<i64>, WriteError> {
    let time = s
        .channel(&ChannelId::Time)
        .ok_or(WriteError::MissingChannel("time"))?;
    time.data
        .values()
        .iter()
        .map(|t| {
            let t = quantise(*t) + offset_us;
            if (0..=i64::from(u32::MAX)).contains(&t) {
                Ok(t)
            } else {
                Err(WriteError::Time(t))
            }
        })
        .collect()
}

/// Samples that can be logged: the gyro has a value on every axis.
fn loggable(s: &FlightSession, n: usize) -> Result<Vec<bool>, WriteError> {
    let gyro: Vec<&Series> = Axis::ALL
        .into_iter()
        .map(|axis| {
            let id = ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            };
            s.channel(&id)
                .map(|c| &c.data)
                .ok_or(WriteError::MissingChannel("filtered gyro"))
        })
        .collect::<Result<_, _>>()?;
    Ok((0..n)
        .map(|i| gyro.iter().all(|g| g.get(i).is_some()))
        .collect())
}

/// Sample rate of the timestamps, Hz (from the median step).
fn sampled_hz(time: &[i64]) -> f64 {
    let mut steps: Vec<i64> = time.windows(2).map(|w| w[1] - w[0]).collect();
    steps.sort_unstable();
    match steps.get(steps.len() / 2) {
        Some(step) if *step > 0 => 1.0e6 / *step as f64,
        _ => 1000.0,
    }
}

/// Writes `session` as one Betaflight Blackbox log.
///
/// The session needs a time channel and the filtered gyro; everything else
/// is optional and written as zero (or as a resting sensor) when absent. A
/// sample without a gyro value is not logged: the log pauses there and
/// resumes, with the firmware's "logging resumed" event, at the next `I`
/// frame, as a flight controller does when its storage falls behind.
///
/// # Errors
/// [`WriteError`] when there is nothing to log, a needed channel is absent,
/// or the loop rates of the settings cannot be scheduled.
pub fn betaflight(session: &FlightSession, options: &WriteOptions) -> Result<Vec<u8>, WriteError> {
    let time = timestamps(session, options.time_offset_us)?;
    let n = time.len();
    let valid = loggable(session, n)?;
    let first = valid.iter().position(|v| *v).ok_or(WriteError::Empty)?;
    let schedule = Schedule::new(&session.settings.loop_rates, sampled_hz(&time))?;
    let fields = fields(session, first, schedule, time);
    let headers = headers(session, &fields, schedule, options);
    let constants = Constants {
        motor0: fields.iter().position(|f| f.name == "motor[0]"),
        motor_low: header_int(&headers, "motorOutput"),
        vbatref: header_int(&headers, "vbatref"),
    };
    let mut buf = Vec::new();
    for (key, value) in &headers {
        buf.extend_from_slice(format!("H {key}:{value}\n").as_bytes());
    }
    // the two frames before the current one; `None` while the log is paused
    let mut history: Option<(Vec<i64>, Vec<i64>)> = None;
    for (k, logged) in valid.iter().enumerate().skip(first) {
        if !logged {
            history = None;
            continue;
        }
        let current: Vec<i64> = fields.iter().map(|f| f.values[k]).collect();
        let (iteration, time_us) = (current[0], current[1]);
        let on_intra = iteration % schedule.i_interval == 0;
        if history.is_none() && k > first {
            // the log paused; the firmware resumes it on an I frame only
            if !on_intra {
                continue;
            }
            write_event(&mut buf, EVENT_LOGGING_RESUME, &[iteration, time_us]);
        }
        let references = history
            .as_ref()
            .filter(|_| !on_intra)
            .map(|(p, p2)| (p.as_slice(), p2.as_slice()));
        let intra = references.is_none();
        write_frame(&mut buf, &fields, &current, references, constants);
        if k == first {
            write_event(&mut buf, EVENT_SYNC_BEEP, &[time_us]);
        }
        if intra && iteration % (schedule.i_interval * SLOW_EVERY_INTRA) == 0 {
            write_slow_frame(&mut buf);
        }
        history = Some(match history.take() {
            Some((previous, _)) if !intra => (current, previous),
            _ => (current.clone(), current),
        });
    }
    if options.disarm {
        // reason 4: the arming switch
        write_event(&mut buf, EVENT_DISARM, &[4]);
    }
    buf.push(b'E');
    buf.push(EVENT_LOG_END);
    buf.extend_from_slice(b"End of log\0");
    Ok(buf)
}
