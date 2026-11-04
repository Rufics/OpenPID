//! From decoded packets to one [`FlightSession`].
//!
//! `ArduPilot` logs every message at its own rate (the gyro at hundreds of Hz,
//! the rate controller at 10–400 Hz, battery at 10 Hz). `OpenPID` channels share
//! one timebase, so the fastest rate-loop message (`IMU` instance 0, else
//! `RATE`, else `PIDR`) becomes the timebase and every other message is
//! sample-and-held onto it: at each timebase sample a channel carries the last
//! value logged at or before that time, and is invalid before its first one.

use crate::profile::{ArdupilotProfile, Vehicle};
use crate::reader::{FormatDef, Message, RawLog, Value};
use openpid_core::session::{
    Channel, ChannelSet, Diagnostics, Event, EventKind, FrameStats, Origin, Timebase,
};
use openpid_core::{
    ChannelId, FirmwareInfo, FirmwareProfile, FirmwareVendor, FirmwareVersion, FlightSession,
    FormatError, OpenOptions, RawHeaders, Series, Unit,
};
use std::collections::BTreeMap;

/// Messages exported as channels, in priority order for the timebase.
const TIMEBASE_CANDIDATES: [(&str, Option<i64>); 3] =
    [("IMU", Some(0)), ("RATE", None), ("PIDR", None)];
/// Messages whose columns become channels (besides the timebase one).
const EXPORTED: [&str; 12] = [
    "IMU", "RATE", "PIDR", "PIDP", "PIDY", "RCOU", "RCIN", "CTUN", "ATT", "BAT", "CURR", "ESC",
];

/// A message's rows as `(time_us, message index)`, time-sorted.
struct Stream<'a> {
    def: &'a FormatDef,
    rows: Vec<(i64, &'a Message)>,
}

fn time_us_of(def: &FormatDef, m: &Message) -> Option<i64> {
    if let Some(c) = def.column("TimeUS") {
        return m.values.get(c)?.as_f64().map(|v| v as i64);
    }
    let c = def.column("TimeMS")?;
    m.values.get(c)?.as_f64().map(|v| (v * 1000.0) as i64)
}

/// Column holding the sensor instance: `Instance`, or `I` when it is a `uint8`
/// (`PIDR.I` is the integrator term, a float).
fn instance_column(def: &FormatDef) -> Option<usize> {
    def.column("Instance")
        .or_else(|| def.column("I").filter(|c| def.format_char(*c) == Some('B')))
}

/// Instance of a row; a message without an instance column is instance 0.
fn instance_of(def: &FormatDef, m: &Message) -> Option<i64> {
    match instance_column(def) {
        Some(c) => m.values.get(c)?.as_f64().map(|v| v as i64),
        None => Some(0),
    }
}

fn stream<'a>(raw: &'a RawLog, name: &str, instance: Option<i64>) -> Option<Stream<'a>> {
    let (idx, def) = raw.def(name)?;
    let mut rows: Vec<(i64, &Message)> = raw
        .messages
        .iter()
        .filter(|m| m.def == idx)
        .filter(|m| instance.is_none_or(|i| instance_of(def, m) == Some(i)))
        .filter_map(|m| Some((time_us_of(def, m)?, m)))
        .collect();
    rows.sort_by_key(|(t, _)| *t);
    (!rows.is_empty()).then_some(Stream { def, rows })
}

/// Instances present in a message (`I` / `Instance` column), sorted.
fn instances(raw: &RawLog, name: &str) -> Vec<i64> {
    let Some((idx, def)) = raw.def(name) else {
        return Vec::new();
    };
    if instance_column(def).is_none() {
        return Vec::new();
    }
    let mut v: Vec<i64> = raw
        .messages
        .iter()
        .filter(|m| m.def == idx)
        .filter_map(|m| instance_of(def, m))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// For every timebase sample, the row of `s` to hold (the last one at or before it).
fn hold_index(times: &[i64], s: &Stream<'_>) -> Vec<Option<usize>> {
    let mut out = Vec::with_capacity(times.len());
    let mut j = 0usize;
    for &t in times {
        while j + 1 < s.rows.len() && s.rows[j + 1].0 <= t {
            j += 1;
        }
        out.push((s.rows[j].0 <= t).then_some(j));
    }
    out
}

/// Scale implied by the format character alone (old logs without `FMTU`).
fn format_scale(c: char) -> f64 {
    match c {
        'c' | 'C' | 'e' | 'E' => 0.01,
        'L' => 1e-7,
        _ => 1.0,
    }
}

/// Unit and scale of a column from `FMTU`, falling back to the format character.
fn generic_unit_scale(raw: &RawLog, def: &FormatDef, col: usize) -> (Unit, f64) {
    let c = def.format_char(col).unwrap_or('-');
    let unit = def
        .units
        .as_ref()
        .and_then(|u| u.chars().nth(col))
        .and_then(|id| raw.units.get(&id))
        .filter(|label| !label.is_empty() && *label != "UNKNOWN")
        .map_or(Unit::Raw, |label| Unit::Custom(label.clone()));
    let scale = def
        .mults
        .as_ref()
        .and_then(|m| m.chars().nth(col))
        .and_then(|id| raw.mults.get(&id))
        .copied()
        .filter(|m| *m != 0.0)
        .unwrap_or_else(|| format_scale(c));
    (unit, scale)
}

fn firmware_string(raw: &RawLog) -> Option<String> {
    raw.texts("VER", "FWS")
        .into_iter()
        .find(|s| !s.is_empty())
        .or_else(|| {
            raw.texts("MSG", "Message").into_iter().find(|s| {
                let l = s.to_ascii_lowercase();
                (l.starts_with("ardu") || l.starts_with("apm:"))
                    && l.contains(" v")
                    && l.chars().any(|c| c.is_ascii_digit())
            })
        })
}

fn firmware_info(raw: &RawLog) -> (FirmwareInfo, Vehicle) {
    let fw = firmware_string(raw).unwrap_or_default();
    let vehicle = Vehicle::from_firmware(&fw);
    let version = raw
        .messages_named("VER")
        .next()
        .and_then(|m| {
            let def = &raw.defs[m.def];
            let get = |c: &str| m.values.get(def.column(c)?)?.as_f64();
            Some(FirmwareVersion::new(
                get("Maj")? as u32,
                get("Min")? as u32,
                get("Pat")? as u32,
            ))
        })
        .or_else(|| {
            fw.split_whitespace()
                .find(|w| w.starts_with('V') && w[1..].starts_with(|c: char| c.is_ascii_digit()))
                .and_then(FirmwareVersion::parse)
        });
    (
        FirmwareInfo {
            vendor: FirmwareVendor::Ardupilot,
            version,
            revision: fw,
            target: None,
            build_date: None,
        },
        vehicle,
    )
}

fn headers(raw: &RawLog, firmware: &FirmwareInfo, vehicle: Vehicle) -> RawHeaders {
    let mut h: Vec<(String, String)> = vec![
        ("vehicle".into(), vehicle.name().into()),
        ("firmware".into(), firmware.revision.clone()),
        (
            "has_rate_msg".into(),
            if raw.def("RATE").is_some() { "1" } else { "0" }.into(),
        ),
    ];
    if let Some((idx, def)) = raw.def("PARM")
        && let (Some(name_c), Some(val_c)) = (def.column("Name"), def.column("Value"))
    {
        for m in raw.messages.iter().filter(|m| m.def == idx) {
            if let (Some(name), Some(v)) = (
                m.values.get(name_c).and_then(Value::as_str),
                m.values.get(val_c).and_then(Value::as_f64),
            ) {
                // parameters are f32 in the log; print them as such
                h.push((name.to_string(), format!("{}", v as f32)));
            }
        }
    }
    for msg in raw.texts("MSG", "Message") {
        h.push(("MSG".into(), msg));
    }
    RawHeaders(h)
}

fn axis_channel_name(msg: &str, col: &str, instance: Option<i64>) -> String {
    match instance {
        Some(i) => format!("{msg}[{i}].{col}"),
        None => format!("{msg}.{col}"),
    }
}

struct Export<'a> {
    name: &'a str,
    instance: Option<i64>,
}

/// Which `(message, instance)` pairs become channels.
fn exports(raw: &RawLog) -> Vec<Export<'static>> {
    let mut out = Vec::new();
    for name in EXPORTED {
        if raw.def(name).is_none() {
            continue;
        }
        let inst = instances(raw, name);
        match name {
            "ESC" => out.extend(inst.into_iter().map(|i| Export {
                name,
                instance: Some(i),
            })),
            _ if inst.is_empty() => out.push(Export {
                name,
                instance: None,
            }),
            // one instance (the first) of multi-instance sensors
            _ => out.push(Export {
                name,
                instance: inst.first().copied(),
            }),
        }
    }
    out
}

/// Build the session; `data_len` is the file size for diagnostics.
pub(crate) fn build(
    raw: &RawLog,
    data_len: u64,
    options: &OpenOptions,
    profile: ArdupilotProfile,
) -> Result<FlightSession, FormatError> {
    let (firmware, vehicle) = firmware_info(raw);
    let h = headers(raw, &firmware, vehicle);

    let primary = TIMEBASE_CANDIDATES
        .iter()
        .filter_map(|(name, inst)| stream(raw, name, *inst).map(|s| (*name, s)))
        .max_by_key(|(_, s)| s.rows.len())
        .ok_or_else(|| FormatError::Malformed {
            offset: 0,
            message: "no IMU, RATE or PIDR messages: nothing to put on a timebase".into(),
        })?;
    let (primary_name, primary_stream) = primary;
    let mut dropped = 0usize;
    let mut times: Vec<i64> = Vec::with_capacity(primary_stream.rows.len());
    for (t, _) in &primary_stream.rows {
        if times.last().is_some_and(|last| t <= last) {
            dropped += 1;
        } else {
            times.push(*t);
        }
    }
    if times.len() < 2 {
        return Err(FormatError::Malformed {
            offset: 0,
            message: format!("{primary_name}: fewer than two timestamped rows"),
        });
    }
    let timebase = Timebase::from_times(times.clone(), None);

    let use_profile = !options.raw;
    let mut channels = ChannelSet::new();
    let mut unmapped = Vec::new();
    let mut frames: BTreeMap<String, FrameStats> = BTreeMap::new();
    for def in &raw.defs {
        let n = raw
            .messages
            .iter()
            .filter(|m| raw.defs[m.def].name == def.name)
            .count() as u64;
        if n > 0 {
            frames.insert(
                def.name.clone(),
                FrameStats {
                    valid: n,
                    corrupt: 0,
                    desync: 0,
                    bytes: n * def.length as u64,
                },
            );
        }
    }

    for ex in exports(raw) {
        let Some(s) = stream(raw, ex.name, ex.instance) else {
            continue;
        };
        let hold = hold_index(&times, &s);
        let def = s.def;
        for (col, col_name) in def.columns.iter().enumerate() {
            if matches!(col_name.as_str(), "TimeUS" | "TimeMS") || Some(col) == instance_column(def)
            {
                continue;
            }
            if def
                .format_char(col)
                .is_some_and(|c| matches!(c, 'n' | 'N' | 'Z' | 'a'))
            {
                continue;
            }
            let field = format!("{}.{}", ex.name, col_name);
            let mapped = if use_profile {
                profile.map_field(&field, &firmware, &h)
            } else {
                None
            };
            let (id, unit, scale, offset) = if let Some(m) = mapped {
                let id = match (m.id, ex.instance) {
                    (ChannelId::Motor { .. } | ChannelId::MotorRpm { .. }, Some(i)) => {
                        ChannelId::MotorRpm { idx: i as u8 }
                    }
                    (id, _) => id,
                };
                (id, m.unit, m.scale, m.offset)
            } else {
                let (unit, scale) = if use_profile {
                    generic_unit_scale(raw, def, col)
                } else {
                    (Unit::Raw, 1.0)
                };
                let name = axis_channel_name(ex.name, col_name, ex.instance);
                let id = if ex.name == "ESC" && col_name == "RPM" && use_profile {
                    ChannelId::MotorRpm {
                        idx: ex.instance.unwrap_or(0) as u8,
                    }
                } else {
                    if use_profile {
                        unmapped.push(name.clone());
                    }
                    ChannelId::Custom(name)
                };
                (id, unit, scale, 0.0)
            };
            let data = Series::from_options(hold.iter().map(|hi| {
                hi.and_then(|j| s.rows[j].1.values.get(col))
                    .and_then(Value::as_f64)
                    .map(|v| (v + offset) * scale)
            }));
            if channels.contains(&id) {
                continue; // first mapping wins (e.g. RATE.RDes over PIDR.Tar)
            }
            channels.insert(Channel {
                id,
                name: axis_channel_name(ex.name, col_name, ex.instance),
                unit,
                data,
                origin: Origin::Logged { field },
            });
        }
    }
    // the timebase itself
    channels.insert(Channel {
        id: ChannelId::Time,
        name: format!("{primary_name}.time"),
        unit: Unit::Microseconds,
        data: Series::from_vec(times.iter().map(|t| *t as f64).collect()),
        origin: Origin::Logged {
            field: format!("{primary_name}.TimeUS"),
        },
    });
    channels.insert(Channel {
        id: ChannelId::LoopIteration,
        name: "sample".into(),
        unit: Unit::Unitless,
        data: Series::from_vec((0..times.len()).map(|i| i as f64).collect()),
        origin: Origin::Derived {
            from: vec![ChannelId::Time],
            by: "row number of the timebase message".into(),
        },
    });
    // throttle from the RC input when the controller did not log one
    if use_profile
        && !channels.contains(&ChannelId::Throttle)
        && let Some(rc) = channels.get(&ChannelId::RcCommand { ch: 2 })
    {
        let data = rc.data.map(|us| ((us - 1000.0) / 10.0).clamp(0.0, 100.0));
        channels.insert(Channel {
            id: ChannelId::Throttle,
            name: "throttle (RCIN.C3)".into(),
            unit: Unit::Percent,
            data,
            origin: Origin::Derived {
                from: vec![ChannelId::RcCommand { ch: 2 }],
                by: "(pwm - 1000) / 10".into(),
            },
        });
    }
    channels.reindex();

    let mut settings = if use_profile {
        profile.settings(&firmware, &h)
    } else {
        openpid_core::Settings::default()
    };
    settings.loop_rates.log_hz = Some(timebase.sample_rate_hz());

    let events = events(raw, &times);

    let exported: Vec<&str> = EXPORTED
        .iter()
        .copied()
        .filter(|n| raw.def(n).is_some())
        .collect();
    let skipped: Vec<String> = raw
        .defs
        .iter()
        .filter(|d| frames.contains_key(&d.name) && !EXPORTED.contains(&d.name.as_str()))
        .filter(|d| {
            !matches!(
                d.name.as_str(),
                "FMT" | "FMTU" | "UNIT" | "MULT" | "PARM" | "MSG" | "VER"
            )
        })
        .map(|d| format!("{}:{}", d.name, frames[&d.name].valid))
        .collect();
    let mut notes = vec![format!(
        "timebase: {primary_name} at {:.1} Hz; other messages ({}) are sample-and-held onto it",
        timebase.sample_rate_hz(),
        exported.join(", ")
    )];
    if dropped > 0 {
        notes.push(format!(
            "{dropped} {primary_name} rows with non-increasing time dropped"
        ));
    }
    if !skipped.is_empty() {
        notes.push(format!(
            "messages not exported as channels: {}",
            skipped.join(" ")
        ));
    }
    let diagnostics = Diagnostics {
        frames,
        corrupt_frames: 0,
        desync_frames: 0,
        skipped_bytes: raw.skipped_bytes,
        total_bytes: data_len,
        unmapped_fields: unmapped,
        notes,
    };

    let mut session = FlightSession {
        index: 0,
        offset_bytes: 0,
        firmware,
        profile_id: if use_profile { profile.id() } else { "raw" }.to_string(),
        settings,
        raw_headers: h.0,
        timebase,
        channels,
        events,
        aux: Vec::new(),
        diagnostics,
    };
    if use_profile {
        profile.derive(&mut session);
    }
    Ok(session)
}

/// `MODE` and `EV` messages as events on the timebase.
fn events(raw: &RawLog, times: &[i64]) -> Vec<Event> {
    let mut out = Vec::new();
    for name in ["MODE", "EV"] {
        let Some(s) = stream(raw, name, None) else {
            continue;
        };
        for (t, m) in &s.rows {
            let sample_index = times.partition_point(|x| x < t).min(times.len() - 1);
            let data: BTreeMap<String, i64> = s
                .def
                .columns
                .iter()
                .zip(&m.values)
                .filter(|(c, _)| !matches!(c.as_str(), "TimeUS" | "TimeMS"))
                .filter_map(|(c, v)| v.as_f64().map(|x| (c.clone(), x as i64)))
                .collect();
            out.push(Event {
                sample_index,
                kind: EventKind::Other {
                    name: name.to_string(),
                    data,
                },
            });
        }
    }
    out.sort_by_key(|e| e.sample_index);
    out
}
