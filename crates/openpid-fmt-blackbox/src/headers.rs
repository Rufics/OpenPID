//! Header block parsing (`H name:value` lines) and frame definitions.

use crate::encoding;
use crate::predictor;
use crate::releases;
use openpid_core::{FirmwareInfo, FirmwareVendor, RawHeaders};
use std::collections::BTreeMap;

/// Marker line that starts every log written by Cleanflight-family firmware.
pub const LOG_START_MARKER: &[u8] = b"H Product:Blackbox flight data recorder by Nicholas Sherlock";

/// Longest header line we are willing to parse (INAV field-name lines exceed 1 KiB).
const MAX_HEADER_LINE: usize = 16 * 1024;

/// Definition of the fields of one frame type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameDef {
    /// Field names in frame order.
    pub names: Vec<String>,
    /// Whether each field is signed.
    pub signed: Vec<bool>,
    /// Predictor id per field.
    pub predictors: Vec<u8>,
    /// Encoding id per field.
    pub encodings: Vec<u8>,
    /// Fields a group of the 8-value encoding ([`encoding::TAG8_8SVB`]) ends
    /// before although the field has that encoding too: the firmware wrote
    /// it as the first value of a group of its own. Empty when neighbouring
    /// fields of that encoding always share groups of eight.
    pub group_starts: Vec<usize>,
}

impl FrameDef {
    /// Number of fields.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the frame has no fields.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Index of a field by name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// Mark the servo array as a group of its own (see
    /// [`Headers::servo_group`]).
    fn split_before_servos(&mut self) {
        let is_servo = |name: &String| name.starts_with("servo[");
        self.group_starts = (1..self.len())
            .filter(|i| is_servo(&self.names[*i]) && !is_servo(&self.names[*i - 1]))
            .collect();
    }

    fn validate(&self, frame_type: u8) -> Result<(), HeaderError> {
        let ft = frame_type as char;
        if self.is_empty() {
            return Err(HeaderError::Inconsistent(ft, "no field names".into()));
        }
        if self.signed.len() != self.len() {
            return Err(HeaderError::Inconsistent(
                ft,
                "signed list length differs from names".into(),
            ));
        }
        if self.predictors.len() != self.len() {
            return Err(HeaderError::Inconsistent(
                ft,
                "predictor list length differs from names".into(),
            ));
        }
        if self.encodings.len() != self.len() {
            return Err(HeaderError::Inconsistent(
                ft,
                "encoding list length differs from names".into(),
            ));
        }
        // A frame with a predictor this crate cannot apply has no values:
        // every one of them would be rejected, and the log would come out
        // empty with nothing to say why.
        if let Some(i) = (0..self.len()).find(|i| !predictor::is_known(self.predictors[*i])) {
            return Err(HeaderError::Inconsistent(
                ft,
                format!(
                    "unsupported predictor {} for field {}",
                    self.predictors[i], self.names[i]
                ),
            ));
        }
        let mut i = 0;
        while i < self.len() {
            let e = self.encodings[i];
            if !encoding::is_known(e) {
                return Err(HeaderError::Inconsistent(
                    ft,
                    format!("unsupported encoding {e} for field {}", self.names[i]),
                ));
            }
            let g = encoding::group_size(e);
            if g > 1 && i + g > self.len() {
                return Err(HeaderError::Inconsistent(
                    ft,
                    format!("grouped encoding {e} runs past the last field"),
                ));
            }
            i += g;
        }
        Ok(())
    }
}

/// Sub-sampling of inter frames relative to loop iterations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PInterval {
    /// `num` P frames every `denom` iterations (Betaflight: `1/denom`).
    /// `num` P frames every `denom` iterations (Betaflight: `1/denom`).
    Fraction {
        /// Frames logged per `denom` iterations.
        num: u32,
        /// Iterations per `num` frames.
        denom: u32,
    },
    /// Only intra frames are logged.
    IntraOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
/// Why a header block could not be used.
pub enum HeaderError {
    #[error("log has no header block")]
    /// No `H ` lines at the start of the log.
    NoHeader,
    #[error("no field definitions for main frames")]
    /// No `Field I` definitions.
    MissingMainDef,
    #[error("main frames lack the `{0}` field")]
    /// The main frame definition lacks a required field.
    MissingMainField(&'static str),
    #[error("field definition for frame '{0}' is inconsistent: {1}")]
    /// A frame definition is internally inconsistent (frame letter, reason).
    Inconsistent(char, String),
}

/// Parsed header block of one log.
#[derive(Debug, Clone)]
pub struct Headers {
    /// Every header as logged, in order.
    pub raw: RawHeaders,
    /// Frame definitions keyed by frame letter (`I`, `P`, `S`, `G`, `H`).
    pub defs: BTreeMap<u8, FrameDef>,
    /// `Data version` header (2 for every modern firmware).
    pub data_version: u32,
    /// Loop iterations between intra frames.
    pub i_interval: u32,
    /// Inter-frame sub-sampling.
    pub p_interval: PInterval,
    /// `minthrottle` header (default 1150).
    pub minthrottle: i64,
    /// Lowest motor output (`motorOutput` header, else `minthrottle`).
    pub motor_output_low: i64,
    /// `vbatref` header (default 4095).
    pub vbatref: i64,
    /// Firmware identity from the `Firmware revision` header.
    pub firmware: FirmwareInfo,
    /// The firmware writes the servo array of a main frame as one group of
    /// the 8-value encoding of its own, whatever stands before it; from the
    /// release line of the log ([`crate::releases::Release::servo_group`]).
    ///
    /// Betaflight since 2025.12 logs `servo[0..7]` that way (tags 2025.12.2
    /// and 2026.6.2, `blackbox.c`: `writeInterframe` writes the optional
    /// fields `vbatLatest` … `rssi` with one call and the servos with a
    /// second one), and warns in its field table that a decoder which takes
    /// neighbouring fields of that encoding in eights reads them wrong when
    /// fewer than eight of the optional fields stand right before the
    /// servos, which they do with gyro, accelerometer, debug and motor
    /// logging all switched off. Up to 4.5 it logged one servo, `servo[5]`,
    /// as a plain number (tags 4.3.2, 4.4.3, 4.5.5).
    pub servo_group: bool,
    /// Byte offset where frame data begins.
    pub data_start: usize,
}

impl Headers {
    /// Parse the header block at the start of `data`.
    pub fn parse(data: &[u8]) -> Result<Headers, HeaderError> {
        let (raw, pos) = read_lines(data);
        if raw.is_empty() {
            return Err(HeaderError::NoHeader);
        }
        let raw = RawHeaders(raw);

        let mut defs = frame_defs(&raw);
        // P frames share names and signedness with I frames.
        let i_def = defs
            .get(&b'I')
            .cloned()
            .ok_or(HeaderError::MissingMainDef)?;
        if let Some(p) = defs.get_mut(&b'P') {
            p.names.clone_from(&i_def.names);
            p.signed.clone_from(&i_def.signed);
        }
        let mut firmware = FirmwareInfo::parse_revision(raw.get("Firmware revision").unwrap_or(""));
        firmware.build_date = raw.get("Firmware date").map(str::to_string);
        let servo_group = firmware.vendor == FirmwareVendor::Betaflight
            && releases::select(&firmware).release.servo_group;
        if servo_group {
            for ft in *b"IP" {
                if let Some(def) = defs.get_mut(&ft) {
                    def.split_before_servos();
                }
            }
        }
        for (ft, def) in &defs {
            def.validate(*ft)?;
        }
        check_references(&defs, &i_def)?;
        if i_def.index_of("loopIteration").is_none() {
            return Err(HeaderError::MissingMainField("loopIteration"));
        }
        if i_def.index_of("time").is_none() {
            return Err(HeaderError::MissingMainField("time"));
        }

        // Clamp instead of truncating: a damaged "4294967296" must not become 0.
        let i_interval = raw
            .get_i64("I interval")
            .map_or(32, |v| v.clamp(1, i64::from(u32::MAX)) as u32);
        let p_interval = p_interval(&raw);
        let minthrottle = raw.get_i64("minthrottle").unwrap_or(1150);
        let motor_output_low = raw
            .get("motorOutput")
            .and_then(|v| v.split(',').next()?.trim().parse::<i64>().ok())
            .unwrap_or(minthrottle);
        let vbatref = raw.get_i64("vbatref").unwrap_or(4095);
        let data_version = raw.get_i64("Data version").map_or(1, |v| v as u32);
        Ok(Headers {
            raw,
            defs,
            data_version,
            i_interval,
            p_interval,
            minthrottle,
            motor_output_low,
            vbatref,
            firmware,
            servo_group,
            data_start: pos,
        })
    }

    /// Definition of the intra (`I`) frames.
    ///
    /// # Panics
    /// Never: [`Headers::parse`] rejects logs without an `I` definition.
    pub fn main_def(&self) -> &FrameDef {
        &self.defs[&b'I']
    }

    /// Definition of the frame kind with the given letter.
    pub fn def(&self, frame_type: u8) -> Option<&FrameDef> {
        self.defs.get(&frame_type)
    }

    /// Whether the firmware logs a main frame for loop iteration `iteration`.
    pub fn should_have_frame(&self, iteration: u32) -> bool {
        match self.p_interval {
            PInterval::IntraOnly => iteration.is_multiple_of(self.i_interval),
            PInterval::Fraction { num, denom } => {
                // u64: a damaged header may carry `num` close to u32::MAX
                (u64::from(iteration % self.i_interval) + u64::from(num) - 1) % u64::from(denom)
                    < u64::from(num)
            }
        }
    }

    /// Average number of loop iterations per logged main frame.
    pub fn iterations_per_frame(&self) -> f64 {
        match self.p_interval {
            PInterval::IntraOnly => f64::from(self.i_interval),
            PInterval::Fraction { num, denom } => f64::from(denom) / f64::from(num),
        }
    }
}

/// The `H name:value` lines at the start of `data`, in order, and the byte
/// offset just after the last of them. The block ends at the first line
/// that is not a header line, is longer than [`MAX_HEADER_LINE`] or holds a
/// zero byte; a line without a colon is passed over.
fn read_lines(data: &[u8]) -> (Vec<(String, String)>, usize) {
    let mut raw: Vec<(String, String)> = Vec::new();
    let mut pos = 0usize;
    while data.len() >= pos + 2 && data[pos] == b'H' && data[pos + 1] == b' ' {
        let line_start = pos + 2;
        let limit = (line_start + MAX_HEADER_LINE).min(data.len());
        let Some(nl) = data[line_start..limit].iter().position(|b| *b == b'\n') else {
            break;
        };
        let line = &data[line_start..line_start + nl];
        if line.contains(&0) {
            break;
        }
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end_matches('\r');
        if let Some((name, value)) = line.split_once(':') {
            raw.push((name.to_string(), value.to_string()));
        }
        pos = line_start + nl + 1;
    }
    (raw, pos)
}

/// The frame definitions of the `Field <letter> <kind>` headers, by frame
/// letter, as logged (the P frames still without the names and signs they
/// share with the I frames).
fn frame_defs(raw: &RawHeaders) -> BTreeMap<u8, FrameDef> {
    let mut defs: BTreeMap<u8, FrameDef> = BTreeMap::new();
    for (name, value) in &raw.0 {
        let Some((ft, kind)) = field_header(name) else {
            continue;
        };
        let def = defs.entry(ft).or_default();
        match kind {
            "name" => def.names = value.split(',').map(|s| s.trim().to_string()).collect(),
            "signed" => def.signed = parse_ints(value).into_iter().map(|v| v != 0).collect(),
            "predictor" => {
                def.predictors = parse_ints(value).into_iter().map(|v| v as u8).collect()
            }
            "encoding" => def.encodings = parse_ints(value).into_iter().map(|v| v as u8).collect(),
            _ => {}
        }
    }
    defs
}

/// The frame letter and the kind (`name`, `signed`, `predictor`,
/// `encoding`) of a `Field <letter> <kind>` header name.
fn field_header(name: &str) -> Option<(u8, &str)> {
    let (ft, kind) = name.strip_prefix("Field ")?.split_once(' ')?;
    Some((*ft.as_bytes().first()?, kind))
}

/// A predictor that refers to a field the log does not have leaves a frame
/// without values as an unknown predictor does: `motor[0]` for predictor 5
/// in the main frames, the home coordinates for predictor 7 in the GPS
/// frames.
fn check_references(defs: &BTreeMap<u8, FrameDef>, i_def: &FrameDef) -> Result<(), HeaderError> {
    let lacks = |ft: u8, predictor: u8, field: &str| {
        let def = defs.get(&ft)?;
        let i = def.predictors.iter().position(|p| *p == predictor)?;
        Some(HeaderError::Inconsistent(
            ft as char,
            format!(
                "predictor {predictor} for field {} needs the field {field}, which the log does \
                 not define",
                def.names[i]
            ),
        ))
    };
    if i_def.index_of("motor[0]").is_none() {
        for ft in *b"IP" {
            lacks(ft, predictor::MOTOR_0, "motor[0]").map_or(Ok(()), Err)?;
        }
    }
    let home = defs.get(&b'H');
    for field in ["GPS_home[0]", "GPS_home[1]"] {
        if home.is_none_or(|h| h.index_of(field).is_none()) {
            lacks(b'G', predictor::HOME_COORD, field).map_or(Ok(()), Err)?;
        }
    }
    Ok(())
}

/// The `P interval` header: `1/denom`, a bare denominator, or 0 for a log
/// of intra frames only; one P frame per iteration when it is absent or
/// unreadable.
fn p_interval(raw: &RawHeaders) -> PInterval {
    match raw.get("P interval").map(str::trim) {
        Some(v) if v.contains('/') => {
            let (a, b) = v.split_once('/').unwrap_or(("1", "1"));
            let num = a.trim().parse().unwrap_or(1u32);
            let denom = b.trim().parse().unwrap_or(1u32);
            if num == 0 || denom == 0 {
                PInterval::IntraOnly
            } else {
                PInterval::Fraction { num, denom }
            }
        }
        Some(v) => match v.parse::<u32>() {
            Ok(0) => PInterval::IntraOnly,
            Ok(d) => PInterval::Fraction { num: 1, denom: d },
            Err(_) => PInterval::Fraction { num: 1, denom: 1 },
        },
        None => PInterval::Fraction { num: 1, denom: 1 },
    }
}

fn parse_ints(value: &str) -> Vec<i64> {
    value
        .split(',')
        .map(|s| s.trim().parse::<i64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"H Product:Blackbox flight data recorder by Nicholas Sherlock\n\
H Data version:2\n\
H I interval:256\n\
H P interval:16\n\
H P ratio:16\n\
H Field I name:loopIteration,time,axisP[0],axisP[1],axisP[2],motor[0],motor[1]\n\
H Field I signed:0,0,1,1,1,0,0\n\
H Field I predictor:0,0,0,0,0,11,5\n\
H Field I encoding:1,1,0,0,0,1,0\n\
H Field P predictor:6,2,1,1,1,3,3\n\
H Field P encoding:9,0,7,7,7,0,0\n\
H Field S name:flightModeFlags,stateFlags,failsafePhase,rxSignalReceived,rxFlightChannelsValid\n\
H Field S signed:0,0,0,0,0\n\
H Field S predictor:0,0,0,0,0\n\
H Field S encoding:1,1,7,7,7\n\
H Firmware revision:Betaflight 4.2.11 (948ba6339) STM32F7X2\n\
H minthrottle:1070\n\
H motorOutput:158,2047\n\
H vbatref:2466\n\
I\x00";

    #[test]
    fn parses_sample() {
        let h = Headers::parse(SAMPLE).unwrap();
        assert_eq!(h.data_version, 2);
        assert_eq!(h.i_interval, 256);
        assert_eq!(h.p_interval, PInterval::Fraction { num: 1, denom: 16 });
        assert_eq!(h.minthrottle, 1070);
        assert_eq!(h.motor_output_low, 158);
        assert_eq!(h.vbatref, 2466);
        assert_eq!(h.main_def().len(), 7);
        assert_eq!(h.def(b'P').unwrap().names, h.main_def().names);
        assert_eq!(h.def(b'P').unwrap().encodings[2], encoding::TAG2_3S32);
        assert_eq!(h.def(b'S').unwrap().len(), 5);
        assert_eq!(h.data_start, SAMPLE.len() - 2);
        assert_eq!(&SAMPLE[h.data_start..], b"I\x00");
        assert!(h.should_have_frame(0));
        assert!(h.should_have_frame(16));
        assert!(!h.should_have_frame(17));
        assert!((h.iterations_per_frame() - 16.0).abs() < 1e-12);
    }

    #[test]
    fn inav_fraction_interval() {
        let text = SAMPLE.to_vec();
        let text =
            String::from_utf8_lossy(&text).replace("H P interval:16\n", "H P interval:1/2\n");
        let h = Headers::parse(text.as_bytes()).unwrap();
        assert_eq!(h.p_interval, PInterval::Fraction { num: 1, denom: 2 });
        assert!(h.should_have_frame(2));
        assert!(!h.should_have_frame(3));
        let text = text.replace("H P interval:1/2\n", "H P interval:0\n");
        let h = Headers::parse(text.as_bytes()).unwrap();
        assert_eq!(h.p_interval, PInterval::IntraOnly);
        assert!(h.should_have_frame(256));
        assert!(!h.should_have_frame(255));
    }

    /// Found by fuzzing (ADR-0013): an interval that does not fit
    /// in 32 bits truncated to 0 and `should_have_frame` divided by zero; a
    /// numerator near `u32::MAX` overflowed the modulo arithmetic.
    #[test]
    fn damaged_intervals_never_panic() {
        let text = String::from_utf8_lossy(SAMPLE).into_owned();
        for interval in ["4294967296", "-1", "0", "18446744073709551616", "x"] {
            let t = text.replace("H I interval:256\n", &format!("H I interval:{interval}\n"));
            let h = Headers::parse(t.as_bytes()).unwrap();
            assert!(h.i_interval >= 1, "{interval}");
            assert!(h.should_have_frame(0));
            let _ = h.should_have_frame(u32::MAX);
        }
        for p in [
            "4294967295/4294967295",
            "4294967295/1",
            "1/4294967295",
            "4294967296/2",
        ] {
            let t = text.replace("H P interval:16\n", &format!("H P interval:{p}\n"));
            let h = Headers::parse(t.as_bytes()).unwrap();
            let _ = (h.should_have_frame(u32::MAX), h.iterations_per_frame());
        }
    }

    #[test]
    fn rejects_inconsistent_defs() {
        let text = String::from_utf8_lossy(SAMPLE).replace(
            "H Field I encoding:1,1,0,0,0,1,0\n",
            "H Field I encoding:1,1,0,0\n",
        );
        assert!(matches!(
            Headers::parse(text.as_bytes()),
            Err(HeaderError::Inconsistent('I', _))
        ));
        let text = String::from_utf8_lossy(SAMPLE).replace(
            "H Field P encoding:9,0,7,7,7,0,0\n",
            "H Field P encoding:9,0,0,0,0,0,7\n",
        );
        assert!(matches!(
            Headers::parse(text.as_bytes()),
            Err(HeaderError::Inconsistent('P', _))
        ));
        assert!(matches!(
            Headers::parse(b"garbage"),
            Err(HeaderError::NoHeader)
        ));
    }

    /// Issue #183 G: a predictor id this crate does not know made every
    /// frame fail, and the log came out empty with no explanation; an
    /// encoding it does not know has always been an error that says so.
    #[test]
    fn rejects_predictors_it_cannot_apply() {
        let error = |from: &str, to: &str| {
            let text = String::from_utf8_lossy(SAMPLE).replace(from, to);
            assert_ne!(text.as_bytes(), SAMPLE, "{from}");
            Headers::parse(text.as_bytes()).unwrap_err().to_string()
        };
        assert_eq!(
            error(
                "H Field I predictor:0,0,0,0,0,11,5\n",
                "H Field I predictor:0,0,0,0,12,11,5\n"
            ),
            "field definition for frame 'I' is inconsistent: unsupported predictor 12 for field \
             axisP[2]"
        );
        assert!(
            error(
                "H Field P predictor:6,2,1,1,1,3,3\n",
                "H Field P predictor:6,2,1,1,1,3,200\n"
            )
            .contains("frame 'P' is inconsistent: unsupported predictor 200 for field motor[1]")
        );
        assert!(
            error(
                "H Field S predictor:0,0,0,0,0\n",
                "H Field S predictor:0,0,0,0,99\n"
            )
            .contains("frame 'S'")
        );
        // a predictor that refers to a field the log does not have
        assert!(error("motor[0],motor[1]\n", "motorA,motorB\n").contains(
            "predictor 5 for field motorB needs the field motor[0], which the log does not \
                 define"
        ));
        let gps = "H Field G name:GPS_numSat,GPS_coord[0],GPS_coord[1]\n\
                   H Field G signed:0,1,1\n\
                   H Field G predictor:0,7,7\n\
                   H Field G encoding:1,0,0\n";
        let home = "H Field H name:GPS_home[0],GPS_home[1]\n\
                    H Field H signed:1,1\n\
                    H Field H predictor:0,0\n\
                    H Field H encoding:0,0\n";
        let marker = "H Firmware revision:";
        assert!(error(marker, &format!("{gps}{marker}")).contains(
            "frame 'G' is inconsistent: predictor 7 for field GPS_coord[0] needs the field \
                 GPS_home[0]"
        ));
        let text = String::from_utf8_lossy(SAMPLE).replace(marker, &format!("{gps}{home}{marker}"));
        assert!(Headers::parse(text.as_bytes()).is_ok());
    }
}
