//! Header block parsing (`H name:value` lines) and frame definitions.

use crate::encoding;
use openpid_core::{FirmwareInfo, RawHeaders};
use std::collections::BTreeMap;

/// Marker line that starts every log written by Cleanflight-family firmware.
pub const LOG_START_MARKER: &[u8] = b"H Product:Blackbox flight data recorder by Nicholas Sherlock";

/// Longest header line we are willing to parse (INAV field-name lines exceed 1 KiB).
const MAX_HEADER_LINE: usize = 16 * 1024;

/// Definition of the fields of one frame type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameDef {
    pub names: Vec<String>,
    pub signed: Vec<bool>,
    pub predictors: Vec<u8>,
    pub encodings: Vec<u8>,
}

impl FrameDef {
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
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
    Fraction { num: u32, denom: u32 },
    /// Only intra frames are logged.
    IntraOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    #[error("log has no header block")]
    NoHeader,
    #[error("no field definitions for main frames")]
    MissingMainDef,
    #[error("main frames lack the `{0}` field")]
    MissingMainField(&'static str),
    #[error("field definition for frame '{0}' is inconsistent: {1}")]
    Inconsistent(char, String),
}

/// Parsed header block of one log.
#[derive(Debug, Clone)]
pub struct Headers {
    pub raw: RawHeaders,
    /// Frame definitions keyed by frame letter (`I`, `P`, `S`, `G`, `H`).
    pub defs: BTreeMap<u8, FrameDef>,
    pub data_version: u32,
    pub i_interval: u32,
    pub p_interval: PInterval,
    pub minthrottle: i64,
    pub motor_output_low: i64,
    pub vbatref: i64,
    pub firmware: FirmwareInfo,
    /// Byte offset where frame data begins.
    pub data_start: usize,
}

impl Headers {
    /// Parse the header block at the start of `data`.
    pub fn parse(data: &[u8]) -> Result<Headers, HeaderError> {
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
        if raw.is_empty() {
            return Err(HeaderError::NoHeader);
        }
        let raw = RawHeaders(raw);

        let mut defs: BTreeMap<u8, FrameDef> = BTreeMap::new();
        for (name, value) in &raw.0 {
            let Some(rest) = name.strip_prefix("Field ") else {
                continue;
            };
            let mut it = rest.splitn(2, ' ');
            let (Some(ft), Some(kind)) = (it.next(), it.next()) else {
                continue;
            };
            let Some(&ft) = ft.as_bytes().first() else {
                continue;
            };
            let def = defs.entry(ft).or_default();
            match kind {
                "name" => def.names = value.split(',').map(|s| s.trim().to_string()).collect(),
                "signed" => def.signed = parse_ints(value).into_iter().map(|v| v != 0).collect(),
                "predictor" => {
                    def.predictors = parse_ints(value).into_iter().map(|v| v as u8).collect()
                }
                "encoding" => {
                    def.encodings = parse_ints(value).into_iter().map(|v| v as u8).collect()
                }
                _ => {}
            }
        }
        // P frames share names and signedness with I frames.
        let i_def = defs
            .get(&b'I')
            .cloned()
            .ok_or(HeaderError::MissingMainDef)?;
        if let Some(p) = defs.get_mut(&b'P') {
            p.names.clone_from(&i_def.names);
            p.signed.clone_from(&i_def.signed);
        }
        for (ft, def) in &defs {
            def.validate(*ft)?;
        }
        if i_def.index_of("loopIteration").is_none() {
            return Err(HeaderError::MissingMainField("loopIteration"));
        }
        if i_def.index_of("time").is_none() {
            return Err(HeaderError::MissingMainField("time"));
        }

        let i_interval = raw.get_i64("I interval").map_or(32, |v| v.max(1) as u32);
        let p_interval = match raw.get("P interval").map(str::trim) {
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
        };
        let minthrottle = raw.get_i64("minthrottle").unwrap_or(1150);
        let motor_output_low = raw
            .get("motorOutput")
            .and_then(|v| v.split(',').next()?.trim().parse::<i64>().ok())
            .unwrap_or(minthrottle);
        let vbatref = raw.get_i64("vbatref").unwrap_or(4095);
        let mut firmware = FirmwareInfo::parse_revision(raw.get("Firmware revision").unwrap_or(""));
        firmware.build_date = raw.get("Firmware date").map(str::to_string);

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
            data_start: pos,
        })
    }

    pub fn main_def(&self) -> &FrameDef {
        &self.defs[&b'I']
    }

    pub fn def(&self, frame_type: u8) -> Option<&FrameDef> {
        self.defs.get(&frame_type)
    }

    /// Whether the firmware logs a main frame for loop iteration `iteration`.
    pub fn should_have_frame(&self, iteration: u32) -> bool {
        match self.p_interval {
            PInterval::IntraOnly => iteration % self.i_interval == 0,
            PInterval::Fraction { num, denom } => {
                (iteration % self.i_interval + num - 1) % denom < num
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
}
