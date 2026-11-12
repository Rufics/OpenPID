//! Packing of the frames: the inverse of the decoder's field loop, for the
//! main frames, the slow and GPS frames and the events.
//!
//! The firmware writes each frame with code of its own (`writeIntraframe`,
//! `writeInterframe`, `writeSlowFrame`, `writeGPSFrame`, `writeGPSHomeFrame`
//! and `blackboxLogEvent` in `src/main/blackbox/blackbox.c`); in every
//! release that code does exactly what the field definitions of the headers
//! promise, so one packer driven by the definitions writes the same bytes.

use super::version::FieldDef;
use crate::encoding::{self as enc, encode};
use crate::predictor;
use openpid_core::session::{AdjustmentValue, EventKind};

/// Event: the beep at arming, which marks the start of the log.
const EVENT_SYNC_BEEP: u8 = 0;
/// Event: a setting was changed in flight.
const EVENT_INFLIGHT_ADJUSTMENT: u8 = 13;
/// Event: logging resumed after a pause.
const EVENT_LOGGING_RESUME: u8 = 14;
/// Event: the craft was disarmed.
const EVENT_DISARM: u8 = 15;
/// Event: the flight modes changed.
const EVENT_FLIGHT_MODE: u8 = 30;
/// Event: end of the log.
const EVENT_LOG_END: u8 = 255;
/// Added to the function of an adjustment whose value is a float.
const ADJUSTMENT_IS_FLOAT: u8 = 128;

/// What the predictors refer to besides the frames.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct References {
    /// Index of `motor[0]` in the main frame.
    pub motor0: Option<usize>,
    /// Lowest motor output (`motorOutput`).
    pub motor_low: i64,
    /// Reference battery voltage (`vbatref`).
    pub vbatref: i64,
    /// Latitude and longitude of the last home frame written (zero before
    /// the first, as the firmware's own copy starts).
    pub home: [i64; 2],
    /// Time of the last main frame written.
    pub last_main_time: i64,
}

/// Writes `count` residuals with the encodings `encoding(i)`, grouping the
/// fields the way the grouped encodings pack them.
fn pack(
    buf: &mut Vec<u8>,
    count: usize,
    encoding: impl Fn(usize) -> u8,
    residual: impl Fn(usize) -> i32,
) {
    let mut i = 0;
    while i < count {
        let e = encoding(i);
        let run = (i..count).take_while(|j| encoding(*j) == e).count();
        match e {
            enc::TAG8_8SVB => {
                let group = run.min(8);
                let values: Vec<i32> = (i..i + group).map(&residual).collect();
                encode::write_tag8_8svb(buf, &values);
                i += group;
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

/// Packs one main frame. With no `history` (the two frames written before,
/// newest first) the frame is an `I` frame.
pub(super) fn main_frame(
    buf: &mut Vec<u8>,
    fields: &[FieldDef],
    row: &[i64],
    history: Option<(&[i64], &[i64])>,
    refs: &References,
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
    // the firmware computes in 32 bits and lets them wrap; so does the
    // conversion of the difference
    let residual = |i: usize| -> i32 {
        let predicted = match how(i).0 {
            predictor::PREVIOUS => previous[i],
            predictor::STRAIGHT_LINE => 2 * previous[i] - previous2[i],
            // towards zero, as the firmware's `(a + b) / 2`
            predictor::AVERAGE_2 => previous[i].midpoint(previous2[i]),
            predictor::MOTOR_0 => refs.motor0.map_or(0, |m| row[m]),
            predictor::MINMOTOR => refs.motor_low,
            predictor::VBATREF => refs.vbatref,
            predictor::P1500 => 1500,
            _ => 0,
        };
        (row[i] - predicted) as i32
    };
    pack(buf, fields.len(), |i| how(i).1, residual);
}

/// Packs a slow (`S`), GPS (`G`) or GPS home (`H`) frame: these have no
/// deltas, only the predictors of their definitions.
pub(super) fn aux_frame(
    buf: &mut Vec<u8>,
    letter: u8,
    fields: &[FieldDef],
    row: &[i64],
    refs: &References,
) {
    buf.push(letter);
    // the first field predicted from home is the latitude, the next the longitude
    let coordinate = |i: usize| {
        fields[..i]
            .iter()
            .filter(|f| f.intra.0 == predictor::HOME_COORD)
            .count()
            .min(1)
    };
    let residual = |i: usize| -> i32 {
        let predicted = match fields[i].intra.0 {
            predictor::HOME_COORD => refs.home[coordinate(i)],
            predictor::LAST_MAIN_FRAME_TIME => refs.last_main_time,
            _ => 0,
        };
        (row[i] - predicted) as i32
    };
    pack(buf, fields.len(), |i| fields[i].intra.1, residual);
}

/// Writes an event; `false` (and nothing written) for an event Betaflight
/// does not have.
pub(super) fn event(buf: &mut Vec<u8>, kind: &EventKind) -> bool {
    let mut out = vec![b'E'];
    match kind {
        EventKind::SyncBeep { time_us } => {
            out.push(EVENT_SYNC_BEEP);
            encode::write_uvb(&mut out, *time_us as u32);
        }
        EventKind::InflightAdjustment { function, value } => {
            out.push(EVENT_INFLIGHT_ADJUSTMENT);
            match value {
                AdjustmentValue::Int(v) => {
                    out.push(function & !ADJUSTMENT_IS_FLOAT);
                    encode::write_svb(&mut out, *v);
                }
                AdjustmentValue::Float(v) => {
                    out.push(function | ADJUSTMENT_IS_FLOAT);
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        EventKind::LoggingResume { iteration, time_us } => {
            out.push(EVENT_LOGGING_RESUME);
            encode::write_uvb(&mut out, *iteration);
            encode::write_uvb(&mut out, *time_us as u32);
        }
        EventKind::Disarm { reason } => {
            out.push(EVENT_DISARM);
            encode::write_uvb(&mut out, *reason);
        }
        EventKind::FlightMode { flags, last_flags } => {
            out.push(EVENT_FLIGHT_MODE);
            encode::write_uvb(&mut out, *flags);
            encode::write_uvb(&mut out, *last_flags);
        }
        EventKind::LogEnd => {
            out.push(EVENT_LOG_END);
            out.extend_from_slice(b"End of log\0");
        }
        EventKind::Other { .. } | EventKind::Unknown { .. } => return false,
    }
    buf.extend_from_slice(&out);
    true
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::super::version::{gps_fields, home_fields, slow_fields};
    use super::*;

    /// Bytes worked out by hand from `blackboxLogEvent` (`master` 4b1520e).
    #[test]
    fn events_as_the_firmware_writes_them() {
        let bytes = |kind: EventKind| {
            let mut buf = Vec::new();
            assert!(event(&mut buf, &kind));
            buf
        };
        assert_eq!(
            bytes(EventKind::SyncBeep { time_us: 300 }),
            [b'E', 0, 0xac, 0x02]
        );
        // the time is the firmware's 32-bit counter: it wraps
        assert_eq!(
            bytes(EventKind::SyncBeep {
                time_us: (1 << 32) + 5
            }),
            [b'E', 0, 5]
        );
        assert_eq!(
            bytes(EventKind::FlightMode {
                flags: 0x201,
                last_flags: 1
            }),
            [b'E', 30, 0x81, 0x04, 1]
        );
        assert_eq!(
            bytes(EventKind::InflightAdjustment {
                function: 5,
                value: AdjustmentValue::Int(-3)
            }),
            [b'E', 13, 5, 5]
        );
        assert_eq!(
            bytes(EventKind::InflightAdjustment {
                function: 5,
                value: AdjustmentValue::Float(1.5)
            }),
            [b'E', 13, 133, 0x00, 0x00, 0xc0, 0x3f]
        );
        assert_eq!(
            bytes(EventKind::LoggingResume {
                iteration: 128,
                time_us: 1
            }),
            [b'E', 14, 0x80, 0x01, 1]
        );
        assert_eq!(bytes(EventKind::Disarm { reason: 4 }), [b'E', 15, 4]);
        assert_eq!(bytes(EventKind::LogEnd), b"E\xffEnd of log\0");
        let mut buf = Vec::new();
        assert!(!event(&mut buf, &EventKind::Unknown { code: 99 }));
        assert!(buf.is_empty());
    }

    /// `writeSlowFrame`, `writeGPSHomeFrame` and `writeGPSFrame`.
    #[test]
    fn slow_and_gps_frames_as_the_firmware_writes_them() {
        let refs = References {
            home: [100, -200],
            last_main_time: 5000,
            ..References::default()
        };
        let mut buf = Vec::new();
        aux_frame(&mut buf, b'S', &slow_fields(), &[0x201, 3, 0, 1, 1], &refs);
        // two variable bytes, one, and the three small values in one byte
        assert_eq!(buf, [b'S', 0x81, 0x04, 3, 0b0000_0101]);

        let mut buf = Vec::new();
        let home = home_fields(Target::V2026_6);
        aux_frame(&mut buf, b'H', &home, &[100, -200, 15, 300], &refs);
        assert_eq!(buf, [b'H', 0xc8, 0x01, 0x8f, 0x03, 30, 0xac, 0x02]);

        // 4.4: the time since the last main frame, then the fix relative to home
        let mut buf = Vec::new();
        let fix = gps_fields(Target::V4_4);
        aux_frame(
            &mut buf,
            b'G',
            &fix,
            &[5250, 9, 103, -202, 40, 7, 1800],
            &refs,
        );
        assert_eq!(buf, [b'G', 0xfa, 0x01, 9, 6, 3, 40, 7, 0x88, 0x0e]);
        // master: velocities and the time of the fix after them
        let mut buf = Vec::new();
        let fix = gps_fields(Target::Master);
        let row = [5000, 9, 100, -200, -1, 0, 0, 1, -1, 0, 2];
        aux_frame(&mut buf, b'G', &fix, &row, &refs);
        assert_eq!(buf, [b'G', 0, 9, 0, 0, 1, 0, 0, 2, 1, 0, 2]);
    }
}
