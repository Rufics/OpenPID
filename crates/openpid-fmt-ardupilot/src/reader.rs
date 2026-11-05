//! Byte-level reader for `ArduPilot` `DataFlash` logs.
//!
//! A log is a stream of packets `0xA3 0x95 <type> <payload>`. Packet type 128
//! is `FMT`, which defines the name, length, column names and column types of
//! another packet type; every other type is decoded with the `FMT` seen for it.
//! `FMTU`/`UNIT`/`MULT` (newer logs) add a unit and a multiplier per column.
//! The format is documented in `libraries/AP_Logger/LogStructure.h` of
//! `ArduPilot`; this is an independent implementation of that description.

use openpid_core::FormatError;
use std::collections::BTreeMap;

/// First header byte of every packet.
pub const HEAD1: u8 = 0xA3;
/// Second header byte of every packet.
pub const HEAD2: u8 = 0x95;
/// Packet type of `FMT`, fixed by the firmware.
pub const FMT_TYPE: u8 = 128;
const FMT_LEN: usize = 89;

/// Definition of one packet type, from its `FMT` packet (and `FMTU` when present).
#[derive(Debug, Clone, PartialEq)]
pub struct FormatDef {
    /// Packet type byte.
    pub msg_type: u8,
    /// Whole packet length in bytes, header included.
    pub length: usize,
    /// Message name, e.g. `IMU`.
    pub name: String,
    /// One format character per column (see `LogStructure.h`).
    pub format: String,
    /// Column names.
    pub columns: Vec<String>,
    /// One unit id per column, from `FMTU`.
    pub units: Option<String>,
    /// One multiplier id per column, from `FMTU`.
    pub mults: Option<String>,
}

impl FormatDef {
    /// Index of a column by name.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// Format character of a column.
    pub fn format_char(&self, col: usize) -> Option<char> {
        self.format.chars().nth(col)
    }
}

/// One decoded column value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Any integer type, including the `*100` ones (unscaled) and lat/lon.
    Int(i64),
    /// `f` and `d`.
    Float(f64),
    /// `n`, `N`, `Z`.
    Text(String),
    /// `a` (`int16[32]`).
    Array(Vec<i16>),
}

impl Value {
    /// Numeric view; text and arrays have none.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            Value::Text(_) | Value::Array(_) => None,
        }
    }

    /// Text view.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// One decoded packet.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Index into [`RawLog::defs`].
    pub def: usize,
    /// Byte offset of the packet in the file.
    pub offset: usize,
    /// Column values in definition order.
    pub values: Vec<Value>,
}

/// Everything in the file, before any interpretation.
#[derive(Debug, Clone, Default)]
pub struct RawLog {
    /// Packet definitions in the order they were seen.
    pub defs: Vec<FormatDef>,
    /// Decoded packets in file order.
    pub messages: Vec<Message>,
    /// `UNIT` table: id → label (`k` → `deg/s`).
    pub units: BTreeMap<char, String>,
    /// `MULT` table: id → multiplier (`B` → `0.01`).
    pub mults: BTreeMap<char, f64>,
    /// Bytes that belonged to no packet (resync).
    pub skipped_bytes: u64,
    /// Packets whose definition was unknown when they appeared.
    pub unknown_packets: u64,
}

impl RawLog {
    /// Definition of a message by name.
    pub fn def(&self, name: &str) -> Option<(usize, &FormatDef)> {
        self.defs.iter().enumerate().find(|(_, d)| d.name == name)
    }

    /// All messages of one name.
    pub fn messages_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a Message> + 'a {
        let idx = self.def(name).map(|(i, _)| i);
        self.messages.iter().filter(move |m| Some(m.def) == idx)
    }

    /// Text of a column across messages (`MSG.Message`, `PARM.Name`, ...).
    pub fn texts(&self, msg: &str, column: &str) -> Vec<String> {
        let Some((idx, def)) = self.def(msg) else {
            return Vec::new();
        };
        let Some(col) = def.column(column) else {
            return Vec::new();
        };
        self.messages
            .iter()
            .filter(|m| m.def == idx)
            .filter_map(|m| {
                m.values
                    .get(col)
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }
}

/// Payload size of a format character.
fn size_of(c: char) -> Option<usize> {
    Some(match c {
        'b' | 'B' | 'M' => 1,
        'h' | 'H' | 'c' | 'C' => 2,
        'i' | 'I' | 'f' | 'e' | 'E' | 'L' => 4,
        'd' | 'q' | 'Q' => 8,
        'n' => 4,
        'N' => 16,
        'Z' => 64,
        'a' => 64,
        _ => return None,
    })
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn decode_value(c: char, b: &[u8]) -> Value {
    match c {
        'b' => Value::Int(i64::from(b[0] as i8)),
        'B' | 'M' => Value::Int(i64::from(b[0])),
        'h' | 'c' => Value::Int(i64::from(i16::from_le_bytes([b[0], b[1]]))),
        'H' | 'C' => Value::Int(i64::from(u16::from_le_bytes([b[0], b[1]]))),
        'i' | 'e' | 'L' => Value::Int(i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        'I' | 'E' => Value::Int(i64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        'q' => Value::Int(i64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ])),
        'Q' => Value::Int(
            i64::try_from(u64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ]))
            .unwrap_or(i64::MAX),
        ),
        'f' => Value::Float(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        'd' => Value::Float(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ])),
        'a' => Value::Array(
            b.chunks_exact(2)
                .map(|p| i16::from_le_bytes([p[0], p[1]]))
                .collect(),
        ),
        _ => Value::Text(text(b)),
    }
}

/// Parse one `FMT` packet payload (after the 3-byte header).
fn parse_fmt(p: &[u8]) -> Result<FormatDef, FormatError> {
    let msg_type = p[0];
    let length = usize::from(p[1]);
    let name = text(&p[2..6]);
    let format = text(&p[6..22]);
    let labels = text(&p[22..86]);
    let columns: Vec<String> = if labels.is_empty() {
        Vec::new()
    } else {
        labels.split(',').map(str::to_string).collect()
    };
    let mut payload = 0usize;
    for c in format.chars() {
        payload += size_of(c).ok_or_else(|| FormatError::Malformed {
            offset: 0,
            message: format!("FMT {name}: unknown format character {c:?}"),
        })?;
    }
    if length != payload + 3 || columns.len() != format.chars().count() {
        return Err(FormatError::Malformed {
            offset: 0,
            message: format!(
                "FMT {name}: length {length} and columns {} do not match format {format:?}",
                columns.len()
            ),
        });
    }
    Ok(FormatDef {
        msg_type,
        length,
        name,
        format,
        columns,
        units: None,
        mults: None,
    })
}

/// Whether the first bytes look like a `DataFlash` log (a `FMT` packet first).
pub fn looks_like_dataflash(head: &[u8]) -> bool {
    head.len() >= 3 && head[0] == HEAD1 && head[1] == HEAD2 && head[2] == FMT_TYPE
}

/// Byte offsets at which a new log starts inside `data`: every `FMT` packet
/// that defines `FMT` itself (type 128, length 89, name `FMT`), which is how
/// the firmware opens a log. The first is 0 for a well-formed file; a
/// logger restart or two concatenated logs give more.
pub fn log_starts(data: &[u8]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut i = 0usize;
    while i + FMT_LEN <= data.len() {
        if data[i] == HEAD1
            && data[i + 1] == HEAD2
            && data[i + 2] == FMT_TYPE
            && data[i + 3] == FMT_TYPE
            && usize::from(data[i + 4]) == FMT_LEN
            && &data[i + 5..i + 8] == b"FMT"
        {
            starts.push(i);
            i += FMT_LEN;
        } else {
            i += 1;
        }
    }
    starts
}

/// Decode every packet of the file.
pub fn read(data: &[u8]) -> Result<RawLog, FormatError> {
    let mut log = RawLog::default();
    // packet type → index in defs
    let mut by_type: [Option<usize>; 256] = [None; 256];
    let mut i = 0usize;
    let n = data.len();
    let mut fmt_ok = false;
    while i + 3 <= n {
        if data[i] != HEAD1 || data[i + 1] != HEAD2 {
            log.skipped_bytes += 1;
            i += 1;
            continue;
        }
        let t = data[i + 2];
        if t == FMT_TYPE {
            if i + FMT_LEN > n {
                break;
            }
            match parse_fmt(&data[i + 3..i + FMT_LEN]) {
                Ok(def) => {
                    // a re-definition of the same type replaces the earlier one
                    let idx = log.defs.len();
                    by_type[usize::from(def.msg_type)] = Some(idx);
                    log.defs.push(def);
                    fmt_ok = true;
                }
                Err(e) => {
                    if !fmt_ok {
                        return Err(match e {
                            FormatError::Malformed { message, .. } => {
                                FormatError::Malformed { offset: i, message }
                            }
                            other => other,
                        });
                    }
                    log.skipped_bytes += 1;
                    i += 1;
                    continue;
                }
            }
            i += FMT_LEN;
            continue;
        }
        let Some(idx) = by_type[usize::from(t)] else {
            log.unknown_packets += 1;
            log.skipped_bytes += 1;
            i += 1;
            continue;
        };
        let def = &log.defs[idx];
        if i + def.length > n {
            break; // truncated last packet
        }
        let mut values = Vec::with_capacity(def.columns.len());
        let mut p = i + 3;
        for c in def.format.chars() {
            let sz = size_of(c).unwrap_or(0);
            values.push(decode_value(c, &data[p..p + sz]));
            p += sz;
        }
        log.messages.push(Message {
            def: idx,
            offset: i,
            values,
        });
        i += def.length;
    }
    if !fmt_ok {
        return Err(FormatError::Unrecognized);
    }
    attach_units(&mut log);
    Ok(log)
}

/// Fill the `UNIT`/`MULT` tables and the per-definition unit/multiplier ids.
fn attach_units(log: &mut RawLog) {
    let unit_rows: Vec<(char, String)> = log
        .messages_named("UNIT")
        .filter_map(|m| {
            let def = &log.defs[m.def];
            let id = m.values.get(def.column("Id")?)?.as_f64()? as u8 as char;
            let label = m.values.get(def.column("Label")?)?.as_str()?.to_string();
            Some((id, label))
        })
        .collect();
    log.units.extend(unit_rows);
    let mult_rows: Vec<(char, f64)> = log
        .messages_named("MULT")
        .filter_map(|m| {
            let def = &log.defs[m.def];
            let id = m.values.get(def.column("Id")?)?.as_f64()? as u8 as char;
            let mult = m.values.get(def.column("Mult")?)?.as_f64()?;
            Some((id, mult))
        })
        .collect();
    log.mults.extend(mult_rows);
    let fmtu: Vec<(u8, String, String)> = log
        .messages_named("FMTU")
        .filter_map(|m| {
            let def = &log.defs[m.def];
            let t = m.values.get(def.column("FmtType")?)?.as_f64()? as u8;
            let units = m.values.get(def.column("UnitIds")?)?.as_str()?.to_string();
            let mults = m.values.get(def.column("MultIds")?)?.as_str()?.to_string();
            Some((t, units, mults))
        })
        .collect();
    for (t, units, mults) in fmtu {
        if let Some(def) = log.defs.iter_mut().rev().find(|d| d.msg_type == t) {
            def.units = Some(units);
            def.mults = Some(mults);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt_packet(t: u8, len: u8, name: &str, format: &str, labels: &str) -> Vec<u8> {
        let mut p = vec![HEAD1, HEAD2, FMT_TYPE, t, len];
        let mut field = |s: &str, width: usize| {
            let mut b = s.as_bytes().to_vec();
            b.resize(width, 0);
            p.extend(b);
        };
        field(name, 4);
        field(format, 16);
        field(labels, 64);
        p
    }

    #[test]
    fn decodes_defined_packets_and_skips_junk() {
        let mut data = fmt_packet(200, 3 + 4 + 2 + 4 + 4, "TST", "IhfL", "TimeMS,A,B,Lat");
        data.extend([HEAD1, HEAD2, 200]);
        data.extend(1000u32.to_le_bytes());
        data.extend((-7i16).to_le_bytes());
        data.extend(2.5f32.to_le_bytes());
        data.extend(123_456_789i32.to_le_bytes());
        data.extend(b"junk"); // resync bytes
        data.extend([HEAD1, HEAD2, 201]); // unknown type
        let log = read(&data).unwrap();
        assert_eq!(log.defs.len(), 1);
        assert_eq!(log.messages.len(), 1);
        let m = &log.messages[0];
        assert_eq!(m.values[0], Value::Int(1000));
        assert_eq!(m.values[1], Value::Int(-7));
        assert_eq!(m.values[2], Value::Float(2.5));
        assert_eq!(m.values[3], Value::Int(123_456_789));
        assert!(log.skipped_bytes >= 4);
        assert_eq!(log.unknown_packets, 1);
    }

    #[test]
    fn rejects_non_dataflash_bytes() {
        assert!(matches!(
            read(b"H Product:Blackbox"),
            Err(FormatError::Unrecognized)
        ));
        assert!(!looks_like_dataflash(b"H Pro"));
        assert!(looks_like_dataflash(&[HEAD1, HEAD2, FMT_TYPE, 0]));
    }

    #[test]
    fn bad_fmt_first_is_an_error() {
        let data = fmt_packet(200, 10, "TST", "I?", "TimeMS,A");
        assert!(matches!(read(&data), Err(FormatError::Malformed { .. })));
    }

    #[test]
    fn units_and_multipliers_are_attached() {
        let mut data = fmt_packet(200, 3 + 4 + 2, "TST", "Ic", "TimeMS,Ang");
        data.extend(fmt_packet(
            201,
            3 + 8 + 1 + 16 + 16,
            "FMTU",
            "QBNN",
            "TimeUS,FmtType,UnitIds,MultIds",
        ));
        data.extend(fmt_packet(
            202,
            3 + 8 + 1 + 64,
            "UNIT",
            "QbZ",
            "TimeUS,Id,Label",
        ));
        data.extend(fmt_packet(
            203,
            3 + 8 + 1 + 8,
            "MULT",
            "Qbd",
            "TimeUS,Id,Mult",
        ));
        let mut fmtu = vec![HEAD1, HEAD2, 201];
        fmtu.extend(0u64.to_le_bytes());
        fmtu.push(200);
        let mut s = b"sd".to_vec();
        s.resize(16, 0);
        fmtu.extend(&s);
        let mut s = b"FB".to_vec();
        s.resize(16, 0);
        fmtu.extend(&s);
        data.extend(fmtu);
        let mut unit = vec![HEAD1, HEAD2, 202];
        unit.extend(0u64.to_le_bytes());
        unit.push(b'd');
        let mut s = b"deg".to_vec();
        s.resize(64, 0);
        unit.extend(&s);
        data.extend(unit);
        let mut mult = vec![HEAD1, HEAD2, 203];
        mult.extend(0u64.to_le_bytes());
        mult.push(b'B');
        mult.extend(0.01f64.to_le_bytes());
        data.extend(mult);
        let log = read(&data).unwrap();
        let (_, def) = log.def("TST").unwrap();
        assert_eq!(def.units.as_deref(), Some("sd"));
        assert_eq!(def.mults.as_deref(), Some("FB"));
        assert_eq!(log.units.get(&'d').map(String::as_str), Some("deg"));
        assert_eq!(log.mults.get(&'B'), Some(&0.01));
    }
}
