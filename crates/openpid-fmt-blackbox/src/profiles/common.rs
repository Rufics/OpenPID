//! Field mapping, settings extraction and derived channels shared by all
//! Cleanflight-descended firmwares. Each concrete profile only adjusts the
//! [`Scales`] and overrides the few fields that differ.

use crate::releases::Release;
use openpid_core::settings::{
    DebugMode, DynNotch, FilterChain, LoopRates, Lowpass, LowpassKind, MotorConfig, Notch, PerAxis,
    PidGains, RpmNotch,
};
use openpid_core::{
    Axis, BatteryField, Channel, ChannelId, FieldMapping, FirmwareInfo, FirmwareVendor,
    FlightSession, GyroStage, Origin, PidTerm, RawHeaders, Series, Settings, StickAxis, Unit,
};

/// `gyroADC[2]` -> (`gyroADC`, Some(2)).
pub fn split_indexed(field: &str) -> (&str, Option<usize>) {
    if let (Some(open), true) = (field.find('['), field.ends_with(']')) {
        let idx = field[open + 1..field.len() - 1].parse().ok();
        (&field[..open], idx)
    } else {
        (field, None)
    }
}

/// Multipliers turning raw integers into physical units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scales {
    /// Degrees per second per raw gyro unit.
    pub gyro: f64,
    /// Degrees per second per raw setpoint unit.
    pub setpoint: f64,
    /// Multiplier for `rcCommand` of roll, pitch and yaw: the channel is in
    /// the units Betaflight used up to 2026.6, ±500 at full deflection.
    pub rc: f64,
    /// Multiplier and, before it, offset for `rcCommand[3]`, the throttle:
    /// the channel runs from 1000 to 2000.
    pub rc_throttle: (f64, f64),
    /// G per raw accelerometer unit, `None` when `acc_1G` is unknown.
    pub acc: Option<f64>,
    /// Volts per raw battery unit.
    pub vbat: f64,
    /// Amperes per raw current unit.
    pub amperage: f64,
    /// Motor output that means 0 %.
    pub motor_low: f64,
    /// Motor output that means 100 %.
    pub motor_high: f64,
}

impl Scales {
    /// The scales of a log of the release line `release`. The headers of
    /// the log win where they state a scale (`blackbox_high_resolution`,
    /// `rc_command_scale`, `gyro_scale`, `acc_1G`, `motorOutput`); the
    /// release says what holds without them.
    pub fn of(release: &Release, h: &RawHeaders) -> Scales {
        let hires = if h.get_i64("blackbox_high_resolution") == Some(1) {
            10.0
        } else {
            1.0
        };
        // Betaflight after 2026.6 logs rcCommand as a fraction of full
        // deflection (−1 … 1; the throttle 0 … 1) times `rc_command_scale`,
        // and says so with that header. Earlier firmware logs ±500 and
        // 1000 … 2000 and has no such header. Both become the earlier units.
        let rc_fraction = h
            .get_f64("rc_command_scale")
            .filter(|s| *s > 0.0)
            .or(release.rc_command_scale)
            .map(|s| s * hires);
        let (rc, rc_throttle) = match rc_fraction {
            Some(one) => (RC_FULL / one, (RC_THROTTLE_SPAN / one, one)),
            None => (1.0 / hires, (1.0 / hires, 0.0)),
        };
        let (motor_low, motor_high) = motor_range(h);
        Scales {
            gyro: gyro_scale(h) / hires,
            setpoint: 1.0 / hires,
            rc,
            rc_throttle,
            acc: h.get_f64("acc_1G").filter(|g| *g > 0.0).map(|g| 1.0 / g),
            vbat: release.vbat_volts,
            amperage: release.amperage_amps,
            motor_low,
            motor_high,
        }
    }
}

/// What a profile of the Cleanflight family keeps for one log: its firmware
/// and headers, the scales read from them once, and the names of its mode
/// boxes in bit order. The profiles wrap it and override what differs.
#[derive(Debug)]
pub struct Log<'a> {
    /// The firmware that wrote the log.
    pub fw: &'a FirmwareInfo,
    /// The log's headers.
    pub h: &'a RawHeaders,
    /// The Betaflight release line the log is read as: the line of its
    /// version for Betaflight, the line of the same number for a fork (see
    /// [`Release::of_number`]).
    pub release: &'static Release,
    /// Raw-to-physical scale factors of the log.
    pub scales: Scales,
    /// The mode boxes of `flightModeFlags`, in bit order.
    pub boxes: std::borrow::Cow<'static, [&'static str]>,
}

impl<'a> Log<'a> {
    /// A log with Betaflight 4 conventions and the mode boxes `boxes`.
    pub fn new(
        fw: &'a FirmwareInfo,
        h: &'a RawHeaders,
        boxes: impl Into<std::borrow::Cow<'static, [&'static str]>>,
    ) -> Self {
        let release = Release::of_number(fw.version);
        Log {
            fw,
            h,
            release,
            scales: Scales::of(release, h),
            boxes: boxes.into(),
        }
    }

    /// A field the whole family names and scales alike.
    pub fn map_field(&self, field: &str) -> Option<FieldMapping> {
        map_field(field, &self.scales)
    }

    /// The settings the whole family logs alike.
    pub fn settings(&self) -> Settings {
        settings(self.release, self.h)
    }

    /// The modes whose boxes are set in `word`.
    pub fn flight_modes(&self, word: i64) -> Vec<openpid_core::FlightMode> {
        openpid_core::modes::modes_from_mask(word, &self.boxes)
    }
}

/// `gyro_scale` header: IEEE-754 bits of the deg/s-per-unit factor (1.0 on Betaflight).
pub fn gyro_scale(h: &RawHeaders) -> f64 {
    h.first_of(&["gyro_scale", "gyro.scale"])
        .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .map(|bits| f64::from(f32::from_bits(bits)))
        .filter(|f| f.is_finite() && *f > 1.0e-9)
        .unwrap_or(1.0)
}

/// Motor output range from `motorOutput` or `minthrottle`/`maxthrottle`.
pub fn motor_range(h: &RawHeaders) -> (f64, f64) {
    if let Some(list) = h.get_list("motorOutput")
        && list.len() == 2
    {
        return (list[0], list[1]);
    }
    (
        h.get_f64("minthrottle").unwrap_or(1150.0),
        h.get_f64("maxthrottle").unwrap_or(1850.0),
    )
}

fn motor_mapping(idx: usize, s: &Scales) -> FieldMapping {
    let id = ChannelId::Motor { idx: idx as u8 };
    if s.motor_high > s.motor_low {
        FieldMapping {
            id,
            unit: Unit::Percent,
            scale: 100.0 / (s.motor_high - s.motor_low),
            offset: -s.motor_low,
        }
    } else {
        FieldMapping::new(id, Unit::Raw)
    }
}

/// Map a Cleanflight-family field name. Returns `None` for unknown fields.
pub fn map_field(field: &str, s: &Scales) -> Option<FieldMapping> {
    let (base, idx) = split_indexed(field);
    let axis = idx.and_then(Axis::from_index);
    Some(match base {
        "loopIteration" => FieldMapping::new(ChannelId::LoopIteration, Unit::Unitless),
        "time" => FieldMapping::new(ChannelId::Time, Unit::Microseconds),
        "gyroADC" => FieldMapping::scaled(
            ChannelId::Gyro {
                axis: axis?,
                stage: GyroStage::Filtered,
            },
            Unit::DegreesPerSecond,
            s.gyro,
        ),
        "gyroUnfilt" | "gyroRAW" => FieldMapping::scaled(
            ChannelId::Gyro {
                axis: axis?,
                stage: GyroStage::Raw,
            },
            Unit::DegreesPerSecond,
            s.gyro,
        ),
        "setpoint" => match idx? {
            0..=2 => FieldMapping::scaled(
                ChannelId::Setpoint { axis: axis? },
                Unit::DegreesPerSecond,
                s.setpoint,
            ),
            // Betaflight logs the mixer throttle as setpoint[3] = throttle * 1000.
            3 => FieldMapping::scaled(ChannelId::Custom("setpoint[3]".into()), Unit::Percent, 0.1),
            _ => return None,
        },
        "axisP" | "axisI" | "axisD" | "axisF" | "axisS" => {
            let term = match base {
                "axisP" => PidTerm::P,
                "axisI" => PidTerm::I,
                "axisD" => PidTerm::D,
                "axisF" => PidTerm::F,
                _ => PidTerm::S,
            };
            FieldMapping::new(ChannelId::PidTerm { axis: axis?, term }, Unit::Raw)
        }
        "rcCommand" => {
            let ch = idx.filter(|i| *i < 8)? as u8;
            let id = ChannelId::RcCommand { ch };
            if ch == 3 {
                let (scale, offset) = s.rc_throttle;
                FieldMapping {
                    id,
                    unit: Unit::Raw,
                    scale,
                    offset,
                }
            } else {
                FieldMapping::scaled(id, Unit::Raw, s.rc)
            }
        }
        "motor" => motor_mapping(idx.filter(|i| *i < 8)?, s),
        "eRPM" => FieldMapping::new(
            ChannelId::Erpm {
                idx: idx.filter(|i| *i < 8)? as u8,
            },
            Unit::Raw,
        ),
        "debug" => FieldMapping::new(
            ChannelId::Debug {
                idx: idx.filter(|i| *i < 8)? as u8,
            },
            Unit::Raw,
        ),
        "accSmooth" | "accADC" => match s.acc {
            Some(k) => FieldMapping::scaled(ChannelId::Acc { axis: axis? }, Unit::Gs, k),
            None => FieldMapping::new(ChannelId::Acc { axis: axis? }, Unit::Raw),
        },
        "vbatLatest" => FieldMapping::scaled(
            ChannelId::Battery(BatteryField::Voltage),
            Unit::Volts,
            s.vbat,
        ),
        "amperageLatest" => FieldMapping::scaled(
            ChannelId::Battery(BatteryField::Current),
            Unit::Amperes,
            s.amperage,
        ),
        "rssi" => FieldMapping::new(ChannelId::Rssi, Unit::Raw),
        // Betaflight 2025.12+: attitude quaternion x/y/z scaled to int16 (w is implied positive).
        "imuQuaternion" => FieldMapping::scaled(
            ChannelId::Custom(field.to_string()),
            Unit::Unitless,
            1.0 / 32767.0,
        ),
        "BaroAlt" | "baroAlt" => {
            FieldMapping::new(ChannelId::Custom(field.to_string()), Unit::Centimeters)
        }
        _ => return None,
    })
}

fn lowpass_kind(code: Option<f64>) -> Option<LowpassKind> {
    Some(match code? as i64 {
        0 => LowpassKind::Pt1,
        1 => LowpassKind::Biquad,
        2 => LowpassKind::Pt2,
        3 => LowpassKind::Pt3,
        other => LowpassKind::Other(other.to_string()),
    })
}

fn lowpass(
    h: &RawHeaders,
    hz_keys: &[&str],
    type_keys: &[&str],
    dyn_keys: &[&str],
) -> Option<Lowpass> {
    let cutoff_hz = hz_keys.iter().find_map(|k| h.get_f64(k));
    let dyn_range = dyn_keys
        .iter()
        .find_map(|k| h.get_list(k))
        .filter(|l| l.len() == 2);
    let dyn_active = dyn_range.as_ref().is_some_and(|l| l[0] > 0.0);
    let cutoff_hz = cutoff_hz
        .filter(|c| *c > 0.0)
        .or(if dyn_active { Some(0.0) } else { None })?;
    Some(Lowpass {
        kind: lowpass_kind(type_keys.iter().find_map(|k| h.get_f64(k))),
        cutoff_hz,
        dyn_min_hz: dyn_range.as_ref().filter(|_| dyn_active).map(|l| l[0]),
        dyn_max_hz: dyn_range.as_ref().filter(|_| dyn_active).map(|l| l[1]),
        dyn_expo: 0.0,
    })
}

/// What a filter chain carries beyond its cut-offs, so that nothing after
/// the decoder needs the headers or the firmware version (ADR-0015): the
/// curve of the dynamic low-pass, the fade-in, weights and motor-speed
/// smoothing of the RPM notches, the number of dynamic notches. From
/// Betaflight's headers and, where one is missing, Betaflight's default of
/// that release line: the expo curve since 4.2 (5), the fade-in (50 Hz) and
/// three dynamic notches since 4.3. The forks that log none of these
/// headers get the defaults of the line of their own version number, as
/// the filter model has always assumed. `prefix` is `gyro` or `dterm`.
pub fn filter_details(chain: &mut FilterChain, prefix: &str, release: &Release, h: &RawHeaders) {
    let expo = h
        .get_f64(&format!("{prefix}_lpf1_dyn_expo"))
        .or_else(|| h.get_f64(&format!("{prefix}_lowpass_dyn_expo")))
        .unwrap_or(release.dyn_lpf_expo)
        / 10.0;
    for l in [&mut chain.lowpass1, &mut chain.lowpass2]
        .into_iter()
        .flatten()
    {
        l.dyn_expo = expo;
    }
    if let Some(r) = &mut chain.rpm_notch {
        r.weights = h
            .get_list("rpm_filter_weights")
            .unwrap_or_default()
            .into_iter()
            .map(|w| (w / 100.0).clamp(0.0, 1.0))
            .collect();
        r.fade_range_hz = h
            .get_f64("rpm_filter_fade_range_hz")
            .unwrap_or(release.rpm_fade_range_hz);
        r.lpf_hz = h
            .get_f64("rpm_filter_lpf_hz")
            .or_else(|| h.get_f64("gyro_rpm_notch_lpf_hz"))
            .unwrap_or(150.0);
    }
    if let Some(d) = &mut chain.dyn_notch
        && d.count.is_none()
    {
        d.count = Some(release.dyn_notch_count);
    }
}

fn notches(h: &RawHeaders, hz_key: &str, cutoff_key: &str) -> Vec<Notch> {
    let hz = h.get_list(hz_key).unwrap_or_default();
    let cut = h.get_list(cutoff_key).unwrap_or_default();
    hz.iter()
        .zip(cut.iter().chain(std::iter::repeat(&0.0)))
        .filter(|(c, _)| **c > 0.0)
        .map(|(c, k)| Notch {
            center_hz: *c,
            cutoff_hz: *k,
        })
        .collect()
}

fn pid_axis(h: &RawHeaders, key: &str, axis: usize) -> Option<PidGains> {
    let l = h.get_list(key)?;
    if l.len() < 3 {
        return None;
    }
    let ff = h
        .first_of(&["ff_weight", "feedforward_weight"])
        .and_then(|_| {
            h.get_list("ff_weight")
                .or_else(|| h.get_list("feedforward_weight"))
                .and_then(|l| l.get(axis).copied())
        });
    Some(PidGains {
        p: l[0],
        i: l[1],
        d: l[2],
        f: ff.or_else(|| l.get(3).copied()),
        d_min: h
            .get_list("d_min")
            .and_then(|l| l.get(axis).copied())
            .filter(|v| *v > 0.0),
        d_max: h
            .get_list("d_max")
            .and_then(|l| l.get(axis).copied())
            .filter(|v| *v > 0.0),
    })
}

/// Logged main frames per loop iteration, from the `I`/`P interval` headers.
pub fn iterations_per_frame(h: &RawHeaders) -> f64 {
    let i_interval = h
        .get_f64("I interval")
        .filter(|v| *v >= 1.0)
        .unwrap_or(32.0);
    match h.get("P interval").map(str::trim) {
        Some(v) if v.contains('/') => {
            let (a, b) = v.split_once('/').unwrap_or(("1", "1"));
            let num: f64 = a.trim().parse().unwrap_or(1.0);
            let denom: f64 = b.trim().parse().unwrap_or(1.0);
            if num <= 0.0 || denom <= 0.0 {
                i_interval
            } else {
                denom / num
            }
        }
        Some(v) => match v.parse::<f64>() {
            Ok(d) if d > 0.0 => d,
            Ok(_) => i_interval,
            Err(_) => 1.0,
        },
        None => 1.0,
    }
}

fn protocol_name(code: f64) -> String {
    match code as i64 {
        0 => "PWM",
        1 => "ONESHOT125",
        2 => "ONESHOT42",
        3 => "MULTISHOT",
        4 => "BRUSHED",
        5 => "DSHOT150",
        6 => "DSHOT300",
        7 => "DSHOT600",
        8 => "DSHOT1200",
        9 => "PROSHOT1000",
        other => return format!("protocol {other}"),
    }
    .to_string()
}

/// Settings common to Betaflight-like header layouts (old and new key names).
pub fn settings(release: &Release, h: &RawHeaders) -> Settings {
    let pid = match (
        pid_axis(h, "rollPID", 0),
        pid_axis(h, "pitchPID", 1),
        pid_axis(h, "yawPID", 2),
    ) {
        (Some(roll), Some(pitch), Some(yaw)) => Some(PerAxis { roll, pitch, yaw }),
        _ => None,
    };
    let looptime_us = h.get_f64("looptime").filter(|v| *v > 0.0);
    let gyro_hz = looptime_us.map(|lt| 1.0e6 / lt);
    let denom = h
        .get_f64("pid_process_denom")
        .filter(|v| *v >= 1.0)
        .unwrap_or(1.0);
    let pid_hz = gyro_hz.map(|g| g / denom);
    let log_hz = pid_hz.map(|p| p / iterations_per_frame(h));
    let (low, high) = motor_range(h);
    let gyro_filters = FilterChain {
        lowpass1: lowpass(
            h,
            &["gyro_lpf1_static_hz", "gyro_lowpass_hz"],
            &["gyro_lpf1_type", "gyro_lowpass_type"],
            &["gyro_lpf1_dyn_hz", "gyro_lowpass_dyn_hz"],
        ),
        lowpass2: lowpass(
            h,
            &["gyro_lpf2_static_hz", "gyro_lowpass2_hz"],
            &["gyro_lpf2_type", "gyro_lowpass2_type"],
            &[],
        ),
        notches: notches(h, "gyro_notch_hz", "gyro_notch_cutoff"),
        dyn_notch: match (h.get_f64("dyn_notch_min_hz"), h.get_f64("dyn_notch_max_hz")) {
            (Some(min_hz), Some(max_hz)) if max_hz > 0.0 => Some(DynNotch {
                min_hz,
                max_hz,
                q: h.get_f64("dyn_notch_q").map_or(0.0, |q| q / 100.0),
                count: h.get_f64("dyn_notch_count").map(|c| c as u32),
            }),
            _ => None,
        },
        rpm_notch: h
            .first_of(&["rpm_filter_harmonics", "gyro_rpm_notch_harmonics"])
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0)
            .map(|harmonics| RpmNotch {
                harmonics,
                q: h.first_of(&["rpm_filter_q", "gyro_rpm_notch_q"])
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .map_or(0.0, |q| q / 100.0),
                min_hz: h
                    .first_of(&["rpm_filter_min_hz", "gyro_rpm_notch_min"])
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0.0),
                fade_range_hz: 0.0,
                weights: Vec::new(),
                lpf_hz: 0.0,
            }),
    };
    let mut gyro_filters = gyro_filters;
    filter_details(&mut gyro_filters, "gyro", release, h);
    let dterm_filters = FilterChain {
        lowpass1: lowpass(
            h,
            &["dterm_lpf1_static_hz", "dterm_lowpass_hz"],
            &["dterm_lpf1_type", "dterm_filter_type"],
            &["dterm_lpf1_dyn_hz", "dterm_lowpass_dyn_hz"],
        ),
        lowpass2: lowpass(
            h,
            &["dterm_lpf2_static_hz", "dterm_lowpass2_hz"],
            &["dterm_lpf2_type", "dterm_filter2_type"],
            &[],
        ),
        notches: notches(h, "dterm_notch_hz", "dterm_notch_cutoff"),
        dyn_notch: None,
        rpm_notch: None,
    };
    let mut dterm_filters = dterm_filters;
    filter_details(&mut dterm_filters, "dterm", release, h);
    let extra =
        h.0.iter()
            .filter(|(k, _)| !k.starts_with("Field "))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    Settings {
        pid,
        // the Cleanflight family sets its gains as integers
        pid_step: Some(1.0),
        loop_rates: LoopRates {
            gyro_hz,
            pid_hz,
            log_hz,
        },
        motor: MotorConfig {
            // Rotorflight 4.3 prints a pointer here (0x2000_xxxx): keep only plausible counts
            poles: h
                .get_f64("motor_poles")
                .map(|p| p as u32)
                .filter(|p| (2..=64).contains(p)),
            protocol: h.get_f64("motor_pwm_protocol").map(protocol_name),
            output_low: Some(low),
            output_high: Some(high),
            kv: h.get_f64("motor_kv").filter(|k| *k > 0.0),
            count: None,
        },
        gyro_filters,
        dterm_filters,
        debug_mode: h.get_f64("debug_mode").map(|code| DebugMode {
            code: code as u32,
            name: h.get("debug_mode_name").map(|s| s.trim().to_string()),
        }),
        craft_name: h
            .get("Craft name")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        board: h
            .get("Board information")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        // a flight controller without a clock logs the year 0000
        log_start: h
            .get("Log start datetime")
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with("0000"))
            .map(str::to_string),
        details: Vec::new(),
        extra,
    }
}

fn derived(
    id: ChannelId,
    name: &str,
    unit: Unit,
    data: Series,
    from: Vec<ChannelId>,
    by: &str,
) -> Channel {
    Channel {
        id,
        name: name.to_string(),
        unit,
        data,
        origin: Origin::Derived {
            from,
            by: by.to_string(),
        },
    }
}

/// `gyro.raw.*` from `debug[0..3]` when the log flew with `debug_mode
/// GYRO_SCALED` (Betaflight 4.0 to 4.5, code 6: the gyro before the
/// software filters, in deg/s; [`Release::debug_gyro_scaled`]) and did not
/// log `gyroUnfilt` itself.
fn derive_gyro_raw_from_debug(s: &mut FlightSession) {
    let gyro_scaled = (s.firmware.vendor == FirmwareVendor::Betaflight)
        .then(|| Release::of_number(s.firmware.version).debug_gyro_scaled)
        .flatten();
    let code = s.settings.debug_mode.as_ref().map(|d| d.code);
    if gyro_scaled.is_none() || code != gyro_scaled {
        return;
    }
    for axis in Axis::ALL {
        let raw = ChannelId::Gyro {
            axis,
            stage: GyroStage::Raw,
        };
        let dbg = ChannelId::Debug {
            idx: axis.index() as u8,
        };
        if s.channels.contains(&raw) {
            continue;
        }
        let Some(c) = s.channels.get(&dbg) else {
            continue;
        };
        let data = c.data.clone();
        s.channels.insert(derived(
            raw,
            &format!("debug[{}] (gyro scaled)", axis.index()),
            Unit::DegreesPerSecond,
            data,
            vec![dbg],
            "debug_mode GYRO_SCALED: gyro before the software filters",
        ));
    }
}

/// Mode boxes every Cleanflight-family firmware starts its list with, in
/// the bit order of `flightModeFlags`.
pub const BOXES_COMMON: [&str; 3] = ["ARM", "ANGLE", "HORIZON"];

/// Full deflection of `rcCommand` on roll, pitch and yaw.
const RC_FULL: f64 = 500.0;
/// The range of `rcCommand[3]`, the throttle, which starts at 1000.
const RC_THROTTLE_SPAN: f64 = 1000.0;

/// The stick channels from `rcCommand`, in the canonical convention of
/// [`ChannelId::Stick`]: roll and pitch as logged (±500 is full deflection),
/// yaw negated (the Cleanflight family stores it inverted; its Blackbox
/// Explorers negate it to draw the stick), the throttle from
/// `rcCommand[3]` (1000…2000) where it is one.
fn derive_sticks(s: &mut FlightSession) {
    for (ch, axis, sign) in [
        (0, StickAxis::Roll, 1.0),
        (1, StickAxis::Pitch, 1.0),
        (2, StickAxis::Yaw, -1.0),
    ] {
        let rc = ChannelId::RcCommand { ch };
        if let Some(c) = s.channels.get(&rc) {
            // `+ 0.0` turns the −0.0 of a negated zero into 0.0
            let data = c
                .data
                .map(|v| (sign * v / RC_FULL * 100.0).clamp(-100.0, 100.0) + 0.0);
            s.channels.insert(derived(
                ChannelId::Stick { axis },
                &format!("stick {}", axis.name()),
                Unit::Percent,
                data,
                vec![rc],
                if sign < 0.0 {
                    "-rcCommand / 5"
                } else {
                    "rcCommand / 5"
                },
            ));
        }
    }
    let rc3 = ChannelId::RcCommand { ch: 3 };
    if let Some(c) = s.channels.get(&rc3) {
        let max = c.data.iter().flatten().fold(f64::MIN, f64::max);
        if max > 900.0 {
            let data = c.data.map(|v| ((v - 1000.0) / 10.0).clamp(0.0, 100.0));
            s.channels.insert(derived(
                ChannelId::Stick {
                    axis: StickAxis::Throttle,
                },
                "stick throttle",
                Unit::Percent,
                data,
                vec![rc3],
                "(rcCommand[3] - 1000) / 10",
            ));
        }
    }
}

/// The throttle stick of a helicopter: the collective, `rcCommand[3]`
/// (±500), as 0…100 %.
pub fn derive_collective_stick(s: &mut FlightSession) {
    let rc3 = ChannelId::RcCommand { ch: 3 };
    if let Some(c) = s.channels.get(&rc3) {
        let data = c
            .data
            .map(|v| ((v + RC_FULL) / (2.0 * RC_FULL) * 100.0).clamp(0.0, 100.0));
        s.channels.insert(derived(
            ChannelId::Stick {
                axis: StickAxis::Throttle,
            },
            "stick collective",
            Unit::Percent,
            data,
            vec![rc3],
            "(rcCommand[3] + 500) / 10",
        ));
    }
}

/// Derived channels every Cleanflight-family log can have: throttle percent,
/// the sticks, PID error, PID sum and mechanical motor RPM.
///
/// # Panics
/// Never: every channel it reads was checked to exist just before.
pub fn derive(s: &mut FlightSession) {
    derive_gyro_raw_from_debug(s);
    derive_sticks(s);
    let sp3 = ChannelId::Custom("setpoint[3]".into());
    if let Some(c) = s.channels.get(&sp3) {
        let data = c.data.clone();
        s.channels.insert(derived(
            ChannelId::Throttle,
            "throttle",
            Unit::Percent,
            data,
            vec![sp3],
            "setpoint[3] / 10",
        ));
    } else if let Some(c) = s.channels.get(&ChannelId::RcCommand { ch: 3 }) {
        let max = c.data.iter().flatten().fold(f64::MIN, f64::max);
        if max > 900.0 {
            let data = c.data.map(|v| ((v - 1000.0) / 10.0).clamp(0.0, 100.0));
            s.channels.insert(derived(
                ChannelId::Throttle,
                "throttle",
                Unit::Percent,
                data,
                vec![ChannelId::RcCommand { ch: 3 }],
                "(rcCommand[3] - 1000) / 10",
            ));
        }
    }
    for axis in Axis::ALL {
        let sp = ChannelId::Setpoint { axis };
        let gy = ChannelId::Gyro {
            axis,
            stage: GyroStage::Filtered,
        };
        if let (Some(a), Some(b)) = (s.channels.get(&sp), s.channels.get(&gy))
            && a.unit == b.unit
        {
            let data = a.data.zip_with(&b.data, |x, y| x - y);
            let name = format!("pidError[{}]", axis.index());
            s.channels.insert(derived(
                ChannelId::PidError { axis },
                &name,
                a.unit.clone(),
                data,
                vec![sp, gy],
                "setpoint - gyro",
            ));
        }
        let terms: Vec<ChannelId> = [PidTerm::P, PidTerm::I, PidTerm::D, PidTerm::F, PidTerm::S]
            .into_iter()
            .map(|term| ChannelId::PidTerm { axis, term })
            .filter(|id| s.channels.contains(id))
            .collect();
        if terms.len() >= 2 {
            let mut acc: Option<Series> = None;
            for id in &terms {
                let d = &s.channels.get(id).expect("checked").data;
                acc = Some(match acc {
                    None => d.clone(),
                    Some(a) => a.zip_with(d, |x, y| x + y),
                });
            }
            let name = format!("pidSum[{}]", axis.index());
            s.channels.insert(derived(
                ChannelId::PidTerm {
                    axis,
                    term: PidTerm::Sum,
                },
                &name,
                Unit::Raw,
                acc.expect("at least two terms"),
                terms,
                "P + I + D + F (+ S)",
            ));
        }
    }
    if let Some(poles) = s.settings.motor.poles.filter(|p| *p > 0) {
        let k = 100.0 * 2.0 / f64::from(poles);
        for idx in 0..8u8 {
            let id = ChannelId::Erpm { idx };
            if let Some(c) = s.channels.get(&id) {
                let data = c.data.map(|v| v * k);
                let name = format!("motorRPM[{idx}]");
                s.channels.insert(derived(
                    ChannelId::MotorRpm { idx },
                    &name,
                    Unit::Rpm,
                    data,
                    vec![id],
                    "eRPM * 100 * 2 / poles",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::FirmwareVersion;

    #[test]
    fn rc_command_comes_out_in_one_set_of_units() {
        let line = |major, minor| Release::of_number(Some(FirmwareVersion::new(major, minor, 0)));
        let headers = |pairs: &[(&str, &str)]| {
            RawHeaders(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            )
        };
        let value_of = |release: &Release, h: &RawHeaders, field: &str, raw: f64| {
            let m = map_field(field, &Scales::of(release, h)).unwrap();
            (raw + m.offset) * m.scale
        };
        // the header states the scale, whatever the version
        let value = |h: &RawHeaders, field: &str, raw: f64| {
            let (old, new) = (
                value_of(line(2026, 6), h, field, raw),
                value_of(line(2025, 12), h, field, raw),
            );
            assert_eq!(old, new, "{field}");
            old
        };
        // up to 2026.6: ±500 and 1000 … 2000 as logged, ten times that in a
        // high-resolution log
        let old = headers(&[]);
        assert_eq!(value(&old, "rcCommand[0]", -500.0), -500.0);
        assert_eq!(value(&old, "rcCommand[3]", 1500.0), 1500.0);
        let old_fine = headers(&[("blackbox_high_resolution", "1")]);
        assert_eq!(value(&old_fine, "rcCommand[1]", 2500.0), 250.0);
        assert_eq!(value(&old_fine, "rcCommand[3]", 15_000.0), 1500.0);
        // after it: a fraction of full deflection times the scale
        let new = headers(&[("rc_command_scale", "1000")]);
        assert_eq!(value(&new, "rcCommand[0]", 1000.0), 500.0);
        assert_eq!(value(&new, "rcCommand[2]", -300.0), -150.0);
        assert_eq!(value(&new, "rcCommand[3]", 0.0), 1000.0);
        assert_eq!(value(&new, "rcCommand[3]", 1000.0), 2000.0);
        let new_fine = headers(&[
            ("rc_command_scale", "1000"),
            ("blackbox_high_resolution", "1"),
        ]);
        assert_eq!(value(&new_fine, "rcCommand[0]", 10_000.0), 500.0);
        assert_eq!(value(&new_fine, "rcCommand[3]", 5000.0), 1500.0);
        // a scale of nothing is no scale
        let broken = headers(&[("rc_command_scale", "0")]);
        assert_eq!(value(&broken, "rcCommand[0]", 500.0), 500.0);
        // without the header the release line says what rcCommand is: a
        // fraction times 1000 after 2026.6
        let after = line(2026, 12);
        for h in [&old, &broken] {
            assert_eq!(value_of(after, h, "rcCommand[0]", 1000.0), 500.0);
            assert_eq!(value_of(after, h, "rcCommand[3]", 500.0), 1500.0);
        }
        assert_eq!(value_of(after, &old_fine, "rcCommand[1]", 5000.0), 250.0);
        // and the header still wins there
        let other = headers(&[("rc_command_scale", "500")]);
        assert_eq!(value_of(after, &other, "rcCommand[0]", 500.0), 500.0);
    }

    #[test]
    fn filter_details_come_from_the_headers_or_the_version() {
        let fw = |major, minor| Release::of_number(Some(FirmwareVersion::new(major, minor, 0)));
        let base = |extra: &[(&str, &str)]| {
            let mut v: Vec<(String, String)> = [
                ("gyro_lpf1_dyn_hz", "250,500"),
                ("dterm_lpf1_dyn_hz", "75,150"),
                ("dyn_notch_min_hz", "100"),
                ("dyn_notch_max_hz", "600"),
                ("rpm_filter_harmonics", "3"),
            ]
            .iter()
            .chain(extra)
            .map(|(k, x)| ((*k).to_string(), (*x).to_string()))
            .collect();
            v.dedup();
            RawHeaders(v)
        };
        // the headers win
        let s = settings(
            fw(2025, 12),
            &base(&[
                ("gyro_lpf1_dyn_expo", "7"),
                ("dterm_lpf1_dyn_expo", "3"),
                ("rpm_filter_fade_range_hz", "30"),
                ("rpm_filter_weights", "100,50,0"),
                ("rpm_filter_lpf_hz", "120"),
                ("dyn_notch_count", "2"),
            ]),
        );
        assert_eq!(
            s.gyro_filters.lowpass1.as_ref().map(|l| l.dyn_expo),
            Some(0.7)
        );
        assert_eq!(
            s.dterm_filters.lowpass1.as_ref().map(|l| l.dyn_expo),
            Some(0.3)
        );
        let r = s.gyro_filters.rpm_notch.as_ref().expect("rpm notch");
        assert_eq!((r.fade_range_hz, r.lpf_hz), (30.0, 120.0));
        assert_eq!(r.weights, [1.0, 0.5, 0.0]);
        assert_eq!(s.gyro_filters.dyn_notch.and_then(|d| d.count), Some(2));
        // 4.3 without them: expo 5, a 50 Hz fade, three dynamic notches
        let s = settings(fw(4, 3), &base(&[]));
        assert_eq!(
            s.gyro_filters.lowpass1.as_ref().map(|l| l.dyn_expo),
            Some(0.5)
        );
        let r = s.gyro_filters.rpm_notch.as_ref().expect("rpm notch");
        assert_eq!((r.fade_range_hz, r.lpf_hz), (50.0, 150.0));
        assert!(r.weights.is_empty(), "every harmonic at full weight");
        assert_eq!(s.gyro_filters.dyn_notch.and_then(|d| d.count), Some(3));
        // 4.1: a linear curve, no fade, one dynamic notch
        let s = settings(fw(4, 1), &base(&[]));
        assert_eq!(
            s.gyro_filters.lowpass1.as_ref().map(|l| l.dyn_expo),
            Some(0.0)
        );
        assert_eq!(
            s.gyro_filters.rpm_notch.as_ref().map(|r| r.fade_range_hz),
            Some(0.0)
        );
        assert_eq!(s.gyro_filters.dyn_notch.and_then(|d| d.count), Some(1));
    }

    #[test]
    fn indexed_names() {
        assert_eq!(split_indexed("gyroADC[2]"), ("gyroADC", Some(2)));
        assert_eq!(split_indexed("time"), ("time", None));
        assert_eq!(split_indexed("weird[x]"), ("weird", None));
    }

    #[test]
    fn gyro_scale_header() {
        let h = RawHeaders(vec![("gyro_scale".into(), "0x3f800000".into())]);
        assert_eq!(gyro_scale(&h), 1.0);
        let h = RawHeaders(vec![("gyro.scale".into(), "0x3e8".into())]);
        assert_eq!(gyro_scale(&h), 1.0); // denormal -> fallback
        assert_eq!(gyro_scale(&RawHeaders::default()), 1.0);
    }

    #[test]
    fn frame_rate_from_intervals() {
        let h = RawHeaders(vec![
            ("I interval".into(), "256".into()),
            ("P interval".into(), "16".into()),
        ]);
        assert_eq!(iterations_per_frame(&h), 16.0);
        let h = RawHeaders(vec![
            ("I interval".into(), "32".into()),
            ("P interval".into(), "1/2".into()),
        ]);
        assert_eq!(iterations_per_frame(&h), 2.0);
        let h = RawHeaders(vec![
            ("I interval".into(), "32".into()),
            ("P interval".into(), "0".into()),
        ]);
        assert_eq!(iterations_per_frame(&h), 32.0);
    }
}
