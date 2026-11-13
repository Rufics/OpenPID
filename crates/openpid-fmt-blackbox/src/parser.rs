//! Frame-level parser with the same validation and resynchronisation rules as
//! the reference decoders: a frame is accepted only when it decodes without
//! error, is not absurdly long, is immediately followed by another frame
//! letter (or EOF), and its iteration/time counters move forward sanely.
//!
//! # Going on after a gap
//!
//! "Sanely" means within [`MAX_ITERATION_JUMP`] iterations and
//! [`MAX_TIME_JUMP_US`] of the last accepted main frame: bytes that are not
//! frames decode to arbitrary counters, and that window is what keeps them
//! out. A log can also really jump further than that with no "logging
//! resume" event to announce it: the firmware drops frames while the storage
//! is busy, and its counters run on. Every frame after such a gap is outside
//! the window of the last frame before it, for ever. So an intra frame
//! outside the window is not thrown away at once: it becomes the new
//! reference when there is evidence that it is the same log going on
//! (`Parser::anchor_time`, `Parser::corroborated`):
//!
//! 1. its iteration is one the firmware writes an intra frame on (a multiple
//!    of `I interval`) and lies after the last accepted one;
//! 2. iteration and time moved together across the gap, at the rate of the
//!    log so far (within [`RATE_TOLERANCE`]), which is what one flight
//!    controller's two counters do and what the remains of another log on
//!    the same card do not;
//! 3. the frames that follow it read on without one corrupt frame, rejected
//!    frame or stray byte up to the next intra frame (or the "end of log"
//!    marker), and that frame agrees with it in iteration and time at the
//!    same rate.
//!
//! The same rules find the log again after a hole in the storage: more
//! than [`GIVE_UP_BYTES`] that are no frames (unwritten or overwritten
//! blocks). The parser used to take everything after such a stretch as not
//! being the log; it now looks on for an intra frame that passes the three
//! rules, wherever its counters lie, and only when there is none are the
//! remaining bytes trailing ones.
//!
//! Where the log so far has no rate (it has only intra frames, or the gap is
//! at its start) the rate inside the frames after the gap stands in for it
//! in rule 2. Junk has to pass every one of these at once to be read as a
//! frame, and nothing that passed before is rejected now.

use crate::encoding::{self as enc, DecodeError};
use crate::headers::{FrameDef, Headers};
use crate::predictor;
use crate::reader::Reader;
use openpid_core::FrameStats;
use openpid_core::session::AdjustmentValue;
use std::collections::BTreeMap;

/// Longest frame body (without the letter) the firmware can write.
pub const MAX_FRAME_LEN: usize = 256;
/// Bytes without a single accepted main frame after which the parser stops
/// making one frame attempt per byte: what follows is a hole in the storage
/// that the log goes on after, or not blackbox data at all (stale sectors on
/// the card, another file's remains). It looks for the frame the log goes on
/// with and takes the rest of the file as trailing bytes when there is none.
pub const GIVE_UP_BYTES: usize = 16 * 1024;
/// Most loop iterations between two main frames of an unbroken stream.
pub const MAX_ITERATION_JUMP: u32 = 500 * 10;
/// Most time between two main frames of an unbroken stream.
pub const MAX_TIME_JUMP_US: i64 = 10 * 1_000_000;
/// How far two measurements of the microseconds per loop iteration of one
/// log may be apart, as a ratio either way, to count as the same rate: the
/// rate across a gap against the rate before it and after it. The loop of a
/// flight controller runs off a crystal, so the real spread is a few
/// percent; the allowance is for the rate being learnt from single frames.
pub const RATE_TOLERANCE: f64 = 1.25;

const EVENT_SYNC_BEEP: u8 = 0;
const EVENT_INFLIGHT_ADJUSTMENT: u8 = 13;
const EVENT_LOGGING_RESUME: u8 = 14;
const EVENT_DISARM: u8 = 15;
const EVENT_FLIGHT_MODE: u8 = 30;
/// Rotorflight: main-motor governor state (one byte).
const EVENT_GOV_STATE: u8 = 50;
/// Rotorflight: rescue state (one byte).
const EVENT_RESCUE_STATE: u8 = 51;
/// Rotorflight: airborne state (one byte).
const EVENT_AIRBORNE_STATE: u8 = 52;
/// Rotorflight: custom data (length byte + bytes).
const EVENT_CUSTOM_DATA: u8 = 100;
/// Rotorflight: custom string (zero-terminated).
const EVENT_CUSTOM_STRING: u8 = 101;
const EVENT_LOG_END: u8 = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Which kind of main frame was decoded.
pub enum MainKind {
    /// Intra frame: decodable on its own.
    Intra,
    /// Inter frame: predicted from previous frames.
    Inter,
}

#[derive(Debug, Clone, PartialEq)]
/// Decoded payload of an `E` frame.
pub enum EventData {
    /// Arming beep.
    SyncBeep {
        /// Firmware timestamp of the beep, microseconds.
        time_us: i64,
    },
    /// In-flight adjustment.
    InflightAdjustment {
        /// Firmware adjustment function id.
        function: u8,
        /// New value.
        value: AdjustmentValue,
    },
    /// Logging resumed after a pause.
    LoggingResume {
        /// Loop iteration at resume.
        iteration: u32,
        /// Timestamp at resume, microseconds.
        time_us: i64,
    },
    /// Disarm.
    Disarm {
        /// Firmware disarm reason code.
        reason: u32,
    },
    /// Flight-mode flags changed.
    FlightMode {
        /// New flags.
        flags: u32,
        /// Previous flags.
        last_flags: u32,
    },
    /// End-of-log marker.
    LogEnd,
    /// A firmware-specific event with a known layout (Rotorflight governor,
    /// rescue and airborne states, custom data).
    Other {
        /// Event name, e.g. `gov_state`.
        name: &'static str,
        /// Decoded payload fields.
        data: BTreeMap<String, i64>,
    },
}

/// One accepted frame.
#[derive(Debug, PartialEq)]
pub enum Frame<'p> {
    /// An `I` or `P` frame.
    Main {
        /// Intra or inter.
        kind: MainKind,
        /// Loop iteration.
        iteration: u32,
        /// Timestamp in microseconds (32-bit rollover already unwrapped).
        time_us: i64,
        /// Decoded field values in definition order.
        values: &'p [i64],
    },
    /// Slow (`S`) frame values.
    Slow(&'p [i64]),
    /// GPS (`G`) frame values.
    Gps {
        /// Decoded field values.
        values: &'p [i64],
        /// Whether a home frame was seen before this fix.
        home_valid: bool,
    },
    /// GPS home (`H`) frame values.
    GpsHome(&'p [i64]),
    /// Event (`E`) frame.
    Event(EventData),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Counters gathered while parsing.
pub struct ParseStats {
    /// Per frame letter.
    pub frames: BTreeMap<u8, FrameStats>,
    /// Bytes that were not the start of any known frame.
    pub skipped_bytes: u64,
    /// Loop iterations not logged because of the configured sub-sampling.
    pub intentionally_absent_iterations: u64,
    /// Bytes of frame data in the log.
    pub total_bytes: u64,
    /// Bytes left unread once the parser gave up (see [`GIVE_UP_BYTES`]);
    /// not included in `skipped_bytes`.
    pub trailing_bytes: u64,
    /// Places where the log went on after a gap that no "logging resume"
    /// event announced, in log order.
    pub resumed_gaps: Vec<StreamGap>,
}

/// A gap in the main frames the parser read across (see the module
/// documentation): what lies between the last frame before it and the intra
/// frame after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamGap {
    /// Loop iterations between the two frames.
    pub iterations: u32,
    /// Time between the two frames, microseconds.
    pub time_us: i64,
    /// Bytes between the two frames that are no frames, when the parser
    /// had given up on them (more than [`GIVE_UP_BYTES`]: a hole in the
    /// storage); 0 when the frames lie next to each other.
    pub hole_bytes: u64,
}

impl ParseStats {
    fn frame(&mut self, letter: u8) -> &mut FrameStats {
        self.frames.entry(letter).or_default()
    }

    /// Corrupt frames across all kinds.
    pub fn corrupt_total(&self) -> u64 {
        self.frames.values().map(|f| f.corrupt).sum()
    }

    /// Desynchronised frames across all kinds.
    pub fn desync_total(&self) -> u64 {
        self.frames.values().map(|f| f.desync).sum()
    }
}

enum Body {
    Main(MainKind),
    Slow,
    Gps,
    GpsHome,
    Event(Option<EventData>),
}

enum Accepted {
    Main {
        buf: usize,
        kind: MainKind,
        iteration: u32,
        time_us: i64,
    },
    Slow,
    Gps {
        home_valid: bool,
    },
    GpsHome,
    Event(EventData),
}

/// What one attempt to read a frame came to.
enum Step {
    /// A frame was accepted.
    Frame(Accepted),
    /// A stray byte, a corrupt frame or a frame that failed validation.
    Rejected,
    /// Nothing more to read.
    End,
}

/// Streaming parser over the frame data of one log.
#[derive(Debug, Clone)]
pub struct Parser<'a> {
    reader: Reader<'a>,
    headers: &'a Headers,
    main_def: &'a FrameDef,
    inter_def: Option<&'a FrameDef>,
    slow_def: Option<&'a FrameDef>,
    gps_def: Option<&'a FrameDef>,
    home_def: Option<&'a FrameDef>,
    iter_idx: usize,
    time_idx: usize,
    motor0_idx: Option<usize>,
    gps_time_idx: Option<usize>,
    gps_home_pair: Vec<u8>,
    home_indices: [Option<usize>; 2],

    hist: [Vec<i64>; 3],
    cur: usize,
    prev: Option<usize>,
    prev2: Option<usize>,
    stream_valid: bool,
    last_iteration: Option<u32>,
    last_time: Option<i64>,
    /// Observed microseconds per loop iteration, learnt from accepted frames;
    /// used to reject junk that happens to parse as an intra frame after a sync loss.
    us_per_iteration: Option<f64>,
    last_skipped: u32,

    slow_buf: Vec<i64>,
    gps_buf: Vec<i64>,
    home_buf: Vec<i64>,
    home_published: Vec<i64>,
    home_valid: bool,

    /// Counters so far.
    pub stats: ParseStats,
    done: bool,
    /// Reader position just after the last accepted frame.
    last_accept_pos: usize,
    /// This parser is a copy reading ahead to corroborate a new reference
    /// frame: it takes no new reference itself.
    probing: bool,
}

fn is_frame_letter(b: u8) -> bool {
    matches!(b, b'I' | b'P' | b'S' | b'G' | b'H' | b'E')
}

impl<'a> Parser<'a> {
    /// Create a parser over the frame data of a log whose headers are already parsed.
    /// `data` is the whole log (headers included); parsing starts at `headers.data_start`.
    pub fn new(data: &'a [u8], headers: &'a Headers) -> Parser<'a> {
        let main_def = headers.main_def();
        let n = main_def.len();
        let gps_def = headers.def(b'G');
        let home_def = headers.def(b'H');
        let mut gps_home_pair = Vec::new();
        if let Some(g) = gps_def {
            let mut run = 0u8;
            for p in &g.predictors {
                if *p == predictor::HOME_COORD {
                    gps_home_pair.push(run);
                    run = run.saturating_add(1);
                } else {
                    gps_home_pair.push(0);
                    run = 0;
                }
            }
        }
        let home_indices = match home_def {
            Some(h) => [h.index_of("GPS_home[0]"), h.index_of("GPS_home[1]")],
            None => [None, None],
        };
        let mut reader = Reader::new(data);
        reader.set_pos(headers.data_start);
        let stats = ParseStats {
            total_bytes: (data.len() - headers.data_start) as u64,
            ..Default::default()
        };
        Parser {
            reader,
            headers,
            main_def,
            inter_def: headers.def(b'P'),
            slow_def: headers.def(b'S'),
            gps_def,
            home_def,
            iter_idx: main_def.index_of("loopIteration").unwrap_or(0),
            time_idx: main_def.index_of("time").unwrap_or(1),
            motor0_idx: main_def.index_of("motor[0]"),
            gps_time_idx: gps_def.and_then(|g| g.index_of("time")),
            gps_home_pair,
            home_indices,
            hist: [vec![0; n], vec![0; n], vec![0; n]],
            cur: 0,
            prev: None,
            prev2: None,
            stream_valid: false,
            last_iteration: None,
            last_time: None,
            us_per_iteration: None,
            last_skipped: 0,
            slow_buf: vec![0; headers.def(b'S').map_or(0, FrameDef::len)],
            gps_buf: vec![0; gps_def.map_or(0, FrameDef::len)],
            home_buf: vec![0; home_def.map_or(0, FrameDef::len)],
            home_published: vec![0; home_def.map_or(0, FrameDef::len)],
            home_valid: false,
            stats,
            done: false,
            last_accept_pos: headers.data_start,
            probing: false,
        }
    }

    /// Headers the parser was created with.
    pub fn headers(&self) -> &'a Headers {
        self.headers
    }

    /// Current byte offset within the log.
    pub fn position(&self) -> usize {
        self.reader.pos()
    }

    /// Next accepted frame, or `None` at the end of the log.
    pub fn next_frame(&mut self) -> Option<Frame<'_>> {
        let accepted = loop {
            match self.step() {
                Step::Frame(acc) => break acc,
                Step::Rejected => {}
                Step::End => return None,
            }
        };
        Some(match accepted {
            Accepted::Main {
                buf,
                kind,
                iteration,
                time_us,
            } => Frame::Main {
                kind,
                iteration,
                time_us,
                values: &self.hist[buf],
            },
            Accepted::Slow => Frame::Slow(&self.slow_buf),
            Accepted::Gps { home_valid } => Frame::Gps {
                values: &self.gps_buf,
                home_valid,
            },
            Accepted::GpsHome => Frame::GpsHome(&self.home_published),
            Accepted::Event(e) => Frame::Event(e),
        })
    }

    /// One attempt to read a frame at the reader's position.
    fn step(&mut self) -> Step {
        if self.done {
            return Step::End;
        }
        let start = self.reader.pos();
        if start - self.last_accept_pos > GIVE_UP_BYTES {
            // nothing decodable for a long stretch: the log goes on after a
            // hole, or it ended here
            return self.search_past_hole(start);
        }
        let Some(letter) = self.reader.read_u8() else {
            return Step::End;
        };
        if !is_frame_letter(letter) {
            self.stats.skipped_bytes += 1;
            self.stream_valid = false;
            return Step::Rejected;
        }
        let body = self.parse_body(letter);
        self.reader.align();
        let size = self.reader.pos() - start - 1;
        // "End of log" terminates the stream: nothing after it has to look like a frame.
        let is_log_end = matches!(body, Ok(Body::Event(Some(EventData::LogEnd))));
        let next_ok = is_log_end || self.reader.peek_u8().is_none_or(is_frame_letter);
        let complete = body.is_ok() && size <= MAX_FRAME_LEN && next_ok;
        if !complete {
            let st = self.stats.frame(letter);
            st.corrupt += 1;
            self.stream_valid = false;
            self.prev = None;
            self.prev2 = None;
            self.reader.set_pos(start + 1);
            return Step::Rejected;
        }
        let body = body.unwrap_or(Body::Event(None));
        let Some(acc) = self.complete(letter, body) else {
            self.stats.frame(letter).desync += 1;
            return Step::Rejected;
        };
        let st = self.stats.frame(letter);
        st.valid += 1;
        st.bytes += size as u64 + 1;
        // only time-validated main frames prove the stream is still alive
        if matches!(acc, Accepted::Main { .. }) {
            self.last_accept_pos = self.reader.pos();
        }
        Step::Frame(acc)
    }

    /// The log from `from` on, after [`GIVE_UP_BYTES`] without a main
    /// frame: no longer one attempt per byte and per frame kind, but a
    /// search for the intra frame the log goes on with, which has to pass
    /// the rules of going on after a gap (see the module documentation)
    /// wherever its counters lie. When there is none, the rest of the file
    /// is not this log.
    fn search_past_hole(&mut self, from: usize) -> Step {
        let data = self.reader.data();
        let mut at = from;
        while !self.probing
            && let Some(offset) = data[at..].iter().position(|b| *b == b'I')
        {
            let start = at + offset;
            if let Some(acc) = self.intra_past_hole(start) {
                self.stats.skipped_bytes += (start - from) as u64;
                if let Some(gap) = self.stats.resumed_gaps.last_mut() {
                    gap.hole_bytes = (start - self.last_accept_pos) as u64;
                }
                self.last_accept_pos = self.reader.pos();
                return Step::Frame(acc);
            }
            at = start + 1;
        }
        self.stats.trailing_bytes = (data.len() - from) as u64;
        self.done = true;
        Step::End
    }

    /// The intra frame at `start`, when it is one and the log goes on with
    /// it; it is the reference then.
    fn intra_past_hole(&mut self, start: usize) -> Option<Accepted> {
        self.reader.set_pos(start + 1);
        self.parse_main(MainKind::Intra).ok()?;
        self.reader.align();
        let size = self.reader.pos() - start - 1;
        if size > MAX_FRAME_LEN || !self.reader.peek_u8().is_none_or(is_frame_letter) {
            return None;
        }
        let iteration = self.hist[self.cur][self.iter_idx] as u32;
        let t32 = self.hist[self.cur][self.time_idx] as u32;
        let t = self.anchor_time(iteration, t32)?;
        if !self.corroborated(iteration, t) {
            return None;
        }
        let st = self.stats.frame(b'I');
        st.valid += 1;
        st.bytes += size as u64 + 1;
        Some(self.take_anchor(iteration, t))
    }

    fn parse_body(&mut self, letter: u8) -> Result<Body, DecodeError> {
        match letter {
            b'I' => {
                self.parse_main(MainKind::Intra)?;
                Ok(Body::Main(MainKind::Intra))
            }
            b'P' => {
                self.parse_main(MainKind::Inter)?;
                Ok(Body::Main(MainKind::Inter))
            }
            b'S' | b'G' | b'H' => self.parse_aux(letter),
            b'E' => self.parse_event().map(Body::Event),
            _ => unreachable!("filtered by is_frame_letter"),
        }
    }

    /// Slow, GPS and GPS-home frames: no inter-frame history, but GPS frames
    /// may reference the last home frame and the last main-frame time.
    fn parse_aux(&mut self, letter: u8) -> Result<Body, DecodeError> {
        let Self {
            reader,
            headers,
            slow_def,
            gps_def,
            home_def,
            gps_home_pair,
            home_indices,
            slow_buf,
            gps_buf,
            home_buf,
            home_published,
            home_valid,
            last_time,
            ..
        } = self;
        let ctx = predictor::Context {
            minthrottle: headers.minthrottle,
            motor_output_low: headers.motor_output_low,
            vbatref: headers.vbatref,
            previous: None,
            previous2: None,
            motor0_index: None,
            gps_home: if *home_valid {
                Some(home_published.as_slice())
            } else {
                None
            },
            gps_home_indices: *home_indices,
            last_main_time: *last_time,
        };
        let dv = headers.data_version;
        match letter {
            b'S' => {
                let def = slow_def.ok_or(DecodeError::BadGroup)?;
                parse_fields(reader, def, dv, &ctx, &[], 0, slow_buf).map(|()| Body::Slow)
            }
            b'G' => {
                let def = gps_def.ok_or(DecodeError::BadGroup)?;
                parse_fields(reader, def, dv, &ctx, gps_home_pair, 0, gps_buf).map(|()| Body::Gps)
            }
            _ => {
                let def = home_def.ok_or(DecodeError::BadGroup)?;
                parse_fields(reader, def, dv, &ctx, &[], 0, home_buf).map(|()| Body::GpsHome)
            }
        }
    }

    fn parse_main(&mut self, kind: MainKind) -> Result<(), DecodeError> {
        let def = match kind {
            MainKind::Intra => self.main_def,
            MainKind::Inter => self.inter_def.ok_or(DecodeError::BadGroup)?,
        };
        let skipped = if kind == MainKind::Inter {
            self.count_skipped()
        } else {
            0
        };
        self.last_skipped = skipped;
        let Self {
            reader,
            hist,
            cur,
            prev,
            prev2,
            stream_valid,
            headers,
            motor0_idx,
            home_indices,
            ..
        } = self;
        let mut buf = std::mem::take(&mut hist[*cur]);
        let (previous, previous2) = if kind == MainKind::Inter && *stream_valid {
            (
                prev.map(|i| hist[i].as_slice()),
                prev2.map(|i| hist[i].as_slice()),
            )
        } else {
            (None, None)
        };
        let ctx = predictor::Context {
            minthrottle: headers.minthrottle,
            motor_output_low: headers.motor_output_low,
            vbatref: headers.vbatref,
            previous,
            previous2,
            motor0_index: *motor0_idx,
            gps_home: None,
            gps_home_indices: *home_indices,
            last_main_time: None,
        };
        let r = parse_fields(
            reader,
            def,
            headers.data_version,
            &ctx,
            &[],
            skipped,
            &mut buf,
        );
        hist[*cur] = buf;
        r
    }

    fn parse_event(&mut self) -> Result<Option<EventData>, DecodeError> {
        let r = &mut self.reader;
        let code = r.read_u8().ok_or(DecodeError::Eof)?;
        Ok(match code {
            EVENT_SYNC_BEEP => {
                let t32 = enc::read_uvb(r)?;
                // the beep sounds within the frames around it
                let t = self
                    .last_time
                    .map_or(i64::from(t32), |last| unwrap_near(last, t32));
                Some(EventData::SyncBeep { time_us: t })
            }
            EVENT_INFLIGHT_ADJUSTMENT => {
                let function = r.read_u8().ok_or(DecodeError::Eof)?;
                if function > 127 {
                    let bytes = r.read_bytes(4).ok_or(DecodeError::Eof)?;
                    let f = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    Some(EventData::InflightAdjustment {
                        function: function - 128,
                        value: AdjustmentValue::Float(f),
                    })
                } else {
                    let v = enc::read_svb(r)?;
                    Some(EventData::InflightAdjustment {
                        function,
                        value: AdjustmentValue::Int(v),
                    })
                }
            }
            EVENT_LOGGING_RESUME => {
                let iteration = enc::read_uvb(r)?;
                let t32 = enc::read_uvb(r)?;
                Some(EventData::LoggingResume {
                    iteration,
                    time_us: self.resume_time(iteration, t32),
                })
            }
            EVENT_DISARM => Some(EventData::Disarm {
                reason: enc::read_uvb(r)?,
            }),
            EVENT_FLIGHT_MODE => {
                let flags = enc::read_uvb(r)?;
                let last_flags = enc::read_uvb(r)?;
                Some(EventData::FlightMode { flags, last_flags })
            }
            EVENT_GOV_STATE | EVENT_RESCUE_STATE | EVENT_AIRBORNE_STATE => {
                let state = r.read_u8().ok_or(DecodeError::Eof)?;
                let name = match code {
                    EVENT_GOV_STATE => "gov_state",
                    EVENT_RESCUE_STATE => "rescue_state",
                    _ => "airborne_state",
                };
                Some(EventData::Other {
                    name,
                    data: [("state".to_string(), i64::from(state))].into(),
                })
            }
            EVENT_CUSTOM_DATA => {
                let len = r.read_u8().ok_or(DecodeError::Eof)?;
                let bytes = r.read_bytes(usize::from(len)).ok_or(DecodeError::Eof)?;
                let data = bytes
                    .iter()
                    .enumerate()
                    .map(|(i, b)| (format!("byte{i}"), i64::from(*b)))
                    .collect();
                Some(EventData::Other {
                    name: "custom_data",
                    data,
                })
            }
            EVENT_CUSTOM_STRING => {
                let mut len = 0usize;
                loop {
                    let b = r.read_u8().ok_or(DecodeError::Eof)?;
                    if b == 0 {
                        break;
                    }
                    len += 1;
                    if len > MAX_FRAME_LEN {
                        return Err(DecodeError::Eof);
                    }
                }
                Some(EventData::Other {
                    name: "custom_string",
                    data: [("length".to_string(), len as i64)].into(),
                })
            }
            EVENT_LOG_END => {
                let bytes = r.read_bytes(11).ok_or(DecodeError::Eof)?;
                if bytes == b"End of log\0" {
                    Some(EventData::LogEnd)
                } else {
                    None
                }
            }
            _ => None,
        })
    }

    fn count_skipped(&self) -> u32 {
        let Some(last) = self.last_iteration else {
            return 0;
        };
        let mut n = 0u32;
        let mut idx = last.wrapping_add(1);
        while !self.headers.should_have_frame(idx) {
            n += 1;
            idx = idx.wrapping_add(1);
            if n > MAX_ITERATION_JUMP {
                break;
            }
        }
        n
    }

    fn count_skipped_to(&self, target: u32) -> u32 {
        let Some(last) = self.last_iteration else {
            return 0;
        };
        let mut n = 0u32;
        let mut idx = last.wrapping_add(1);
        while idx < target {
            if !self.headers.should_have_frame(idx) {
                n += 1;
            }
            idx = idx.wrapping_add(1);
            if n > MAX_ITERATION_JUMP {
                break;
            }
        }
        n
    }

    /// The time of a main frame from the 32-bit microsecond counter it
    /// logs. The counter wraps every 71.6 minutes; the time goes on from
    /// the last accepted main frame (or "logging resume" event): a value
    /// below that one's, but less than [`MAX_TIME_JUMP_US`] on from it
    /// through the wrap, is in the next round of the counter.
    ///
    /// Nothing is kept besides the time of the last accepted frame: a frame
    /// that is rejected, and a GPS frame or an event with a time of its
    /// own, leave the times of the main frames after them alone.
    fn main_time(&self, t32: u32) -> i64 {
        let Some(last) = self.last_time else {
            return i64::from(t32);
        };
        let last32 = last as u32;
        let round = last - i64::from(last32);
        if t32 < last32 && i64::from(t32.wrapping_sub(last32)) < MAX_TIME_JUMP_US {
            round + (1i64 << 32) + i64::from(t32)
        } else {
            round + i64::from(t32)
        }
    }

    /// The time of a "logging resume" event from the 32-bit counter value
    /// it carries. Logging was paused for any length of time, so the
    /// counter may have wrapped, more than once: the iteration count, which
    /// the firmware keeps running while logging is paused, says how long the
    /// pause was when the rate of the log is known. Without a rate it is
    /// the first such time after the last frame.
    fn resume_time(&self, iteration: u32, t32: u32) -> i64 {
        let Some(last) = self.last_time else {
            return i64::from(t32);
        };
        match (self.last_iteration, self.us_per_iteration) {
            (Some(li), Some(upi)) if iteration > li => {
                let expected = last + (f64::from(iteration - li) * upi) as i64;
                Some(unwrap_near(expected, t32)).filter(|t| *t >= last)
            }
            _ => None,
        }
        .unwrap_or_else(|| unwrap_forward(last, t32))
    }

    fn validate_main(&self, iteration: u32, time_us: i64) -> bool {
        let (Some(li), Some(lt)) = (self.last_iteration, self.last_time) else {
            return true;
        };
        let window = iteration >= li
            && iteration < li.saturating_add(MAX_ITERATION_JUMP)
            && time_us >= lt
            && time_us < lt + MAX_TIME_JUMP_US;
        if self.stream_valid || !window {
            return window;
        }
        // Resynchronising after a loss: iteration and time must have advanced
        // together, as they do in the firmware, within a factor of four of the
        // rate seen so far. Junk that parses as a frame rarely satisfies this.
        let Some(upi) = self.us_per_iteration else {
            return true;
        };
        let d_iter = f64::from(iteration - li);
        let d_t = (time_us - lt) as f64;
        if d_iter < 1.0 {
            return false;
        }
        let ratio = d_t / (d_iter * upi);
        (0.25..=4.0).contains(&ratio)
    }

    /// Learn the loop period from two accepted frames of the same valid stream.
    fn learn_rate(&mut self, iteration: u32, time_us: i64) {
        if let (Some(li), Some(lt)) = (self.last_iteration, self.last_time)
            && iteration > li
            && time_us > lt
        {
            let upi = (time_us - lt) as f64 / f64::from(iteration - li);
            self.us_per_iteration = Some(match self.us_per_iteration {
                Some(prev) => 0.9 * prev + 0.1 * upi,
                None => upi,
            });
        }
    }

    /// The intra frame just decoded becomes the reference of the stream.
    fn set_reference(&mut self, iteration: u32, time_us: i64) -> Accepted {
        self.stream_valid = true;
        self.last_iteration = Some(iteration);
        self.last_time = Some(time_us);
        let buf = self.cur;
        self.prev = Some(buf);
        self.prev2 = Some(buf);
        self.cur = self.free_slot();
        Accepted::Main {
            buf,
            kind: MainKind::Intra,
            iteration,
            time_us,
        }
    }

    /// The time of an intra frame outside the window of the last accepted
    /// frame, when the frame is the same log going on after a gap (see the
    /// module documentation); `None` when the evidence is not there.
    fn anchor_after_gap(&self, iteration: u32, t32: u32) -> Option<i64> {
        if self.probing {
            return None;
        }
        let t = self.anchor_time(iteration, t32)?;
        self.corroborated(iteration, t).then_some(t)
    }

    /// Rules 1 and 2 of going on after a gap: the time an intra frame with
    /// these counters has when it can be a later frame of this log, the
    /// 32-bit time counter unwrapped to where the iteration count puts it.
    fn anchor_time(&self, iteration: u32, t32: u32) -> Option<i64> {
        if !iteration.is_multiple_of(self.headers.i_interval) {
            return None;
        }
        let (Some(li), Some(lt)) = (self.last_iteration, self.last_time) else {
            // no frame before it to agree with
            return Some(i64::from(t32));
        };
        if iteration <= li {
            return None;
        }
        let d_iter = f64::from(iteration - li);
        let t = match self.us_per_iteration {
            Some(upi) => {
                let t = unwrap_near(lt + (d_iter * upi) as i64, t32);
                if !same_rate((t - lt) as f64 / d_iter, upi) {
                    return None;
                }
                t
            }
            // no rate yet: the first time after the last frame; the frames
            // that follow have to show the same rate (`corroborated`)
            None => unwrap_forward(lt, t32),
        };
        (t > lt).then_some(t)
    }

    /// Rule 3 of going on after a gap: whether the frames after an intra
    /// frame with these counters, the one just decoded, read on cleanly to
    /// the next intra frame or the end of the log and agree with it.
    fn corroborated(&self, iteration: u32, time_us: i64) -> bool {
        let gap_rate = match (self.last_iteration, self.last_time) {
            (Some(li), Some(lt)) => Some((time_us - lt) as f64 / f64::from(iteration - li)),
            _ => None,
        };
        // the frames after it agree with it when the rate from it to them
        // is the rate of the log so far and the rate across the gap
        let agrees = |i: u32, t: i64| {
            if i <= iteration || t <= time_us {
                return false;
            }
            let rate = (t - time_us) as f64 / f64::from(i - iteration);
            self.us_per_iteration.is_none_or(|upi| same_rate(rate, upi))
                && gap_rate.is_none_or(|gap| same_rate(gap, rate))
        };
        let mut probe = self.clone();
        probe.probing = true;
        let _ = probe.set_reference(iteration, time_us);
        probe.hist[self.cur][self.time_idx] = time_us;
        probe.last_accept_pos = probe.reader.pos();
        let mut last = (iteration, time_us);
        // an intra frame comes at least every `I interval` iterations
        for _ in 0..=u64::from(self.headers.i_interval) {
            loop {
                match probe.step() {
                    Step::Frame(Accepted::Main {
                        kind,
                        iteration: i,
                        time_us: t,
                        ..
                    }) => {
                        last = (i, t);
                        if kind == MainKind::Intra {
                            return i.is_multiple_of(self.headers.i_interval) && agrees(i, t);
                        }
                        break;
                    }
                    Step::Frame(Accepted::Event(EventData::LogEnd)) => {
                        return agrees(last.0, last.1);
                    }
                    Step::Frame(_) => {}
                    Step::Rejected | Step::End => return false,
                }
            }
        }
        false
    }

    /// The intra frame just decoded is the log going on after a gap: it
    /// becomes the reference, and the gap is put on record.
    fn take_anchor(&mut self, iteration: u32, time_us: i64) -> Accepted {
        // a log whose first frames are lost has no frame before the gap
        let (li, lt) = (
            self.last_iteration.unwrap_or(iteration),
            self.last_time.unwrap_or(time_us),
        );
        self.stats.resumed_gaps.push(StreamGap {
            iterations: iteration - li,
            time_us: time_us - lt,
            hole_bytes: 0,
        });
        self.hist[self.cur][self.time_idx] = time_us;
        self.set_reference(iteration, time_us)
    }

    fn free_slot(&self) -> usize {
        (0..3)
            .find(|i| Some(*i) != self.prev && Some(*i) != self.prev2)
            .expect("three slots")
    }

    fn complete(&mut self, letter: u8, body: Body) -> Option<Accepted> {
        match body {
            Body::Main(kind) => {
                let raw_t = self.hist[self.cur][self.time_idx];
                let t = self.main_time(raw_t as u32);
                self.hist[self.cur][self.time_idx] = t;
                let iteration = self.hist[self.cur][self.iter_idx] as u32;
                let valid = self.validate_main(iteration, t);
                match kind {
                    MainKind::Intra => {
                        if !valid {
                            // outside the window: the log going on after a
                            // gap, when there is evidence for that
                            if let Some(t) = self.anchor_after_gap(iteration, raw_t as u32) {
                                return Some(self.take_anchor(iteration, t));
                            }
                            self.stream_valid = false;
                            self.prev = None;
                            self.prev2 = None;
                            return None;
                        }
                        self.stats.intentionally_absent_iterations +=
                            u64::from(self.count_skipped_to(iteration));
                        Some(self.set_reference(iteration, t))
                    }
                    MainKind::Inter => {
                        if self.stream_valid && !valid {
                            self.stream_valid = false;
                            self.prev = None;
                            self.prev2 = None;
                        }
                        if !self.stream_valid {
                            return None;
                        }
                        self.learn_rate(iteration, t);
                        self.last_iteration = Some(iteration);
                        self.last_time = Some(t);
                        self.stats.intentionally_absent_iterations += u64::from(self.last_skipped);
                        let buf = self.cur;
                        self.prev2 = self.prev;
                        self.prev = Some(buf);
                        self.cur = self.free_slot();
                        Some(Accepted::Main {
                            buf,
                            kind,
                            iteration,
                            time_us: t,
                        })
                    }
                }
            }
            Body::Slow => Some(Accepted::Slow),
            Body::Gps => {
                // The time of a GPS frame is logged against the main frame
                // before it and is its own: the counter may wrap between
                // the two, and the main frames after it go on from the last
                // main frame, not from here.
                if let (Some(ti), Some(last)) = (self.gps_time_idx, self.last_time) {
                    self.gps_buf[ti] = unwrap_near(last, self.gps_buf[ti] as u32);
                }
                Some(Accepted::Gps {
                    home_valid: self.home_valid,
                })
            }
            Body::GpsHome => {
                self.home_published.clone_from(&self.home_buf);
                self.home_valid = true;
                Some(Accepted::GpsHome)
            }
            Body::Event(None) => {
                debug_assert_eq!(letter, b'E');
                None
            }
            Body::Event(Some(e)) => {
                match &e {
                    EventData::LoggingResume { iteration, time_us } => {
                        self.last_iteration = Some(*iteration);
                        self.last_time = Some(*time_us);
                    }
                    EventData::LogEnd => self.done = true,
                    _ => {}
                }
                Some(Accepted::Event(e))
            }
        }
    }
}

/// The time congruent to the 32-bit counter value `t32` that is nearest to
/// `reference`.
fn unwrap_near(reference: i64, t32: u32) -> i64 {
    reference + i64::from(t32.wrapping_sub(reference as u32) as i32)
}

/// The first time at or after `reference` congruent to the 32-bit counter
/// value `t32`.
fn unwrap_forward(reference: i64, t32: u32) -> i64 {
    reference + i64::from(t32.wrapping_sub(reference as u32))
}

/// Whether two rates (microseconds per loop iteration) are the same within
/// [`RATE_TOLERANCE`].
fn same_rate(a: f64, b: f64) -> bool {
    a > 0.0 && b > 0.0 && a <= b * RATE_TOLERANCE && b <= a * RATE_TOLERANCE
}

/// Decode every field of `def` from `r` into `out`.
fn parse_fields(
    r: &mut Reader<'_>,
    def: &FrameDef,
    data_version: u32,
    ctx: &predictor::Context<'_>,
    home_pair: &[u8],
    skipped: u32,
    out: &mut Vec<i64>,
) -> Result<(), DecodeError> {
    let n = def.len();
    out.resize(n, 0);
    let pair = |i: usize| home_pair.get(i).copied().unwrap_or(0);
    let predict = |i: usize, v: i64, out: &[i64]| -> Result<i64, DecodeError> {
        predictor::apply(def.predictors[i], i, pair(i), v, ctx, out)
            .map_err(|_| DecodeError::BadGroup)
    };
    let mut i = 0;
    while i < n {
        if def.predictors[i] == predictor::INC {
            let base = ctx.previous.map_or(0, |p| p[i]);
            out[i] = base + i64::from(skipped) + 1;
            i += 1;
            continue;
        }
        match def.encodings[i] {
            enc::TAG8_8SVB => {
                r.align();
                let mut count = 1;
                while count < 8
                    && i + count < n
                    && def.encodings[i + count] == enc::TAG8_8SVB
                    && !def.group_starts.contains(&(i + count))
                {
                    count += 1;
                }
                let vals = enc::read_tag8_8svb(r, count)?;
                for (j, v) in vals.iter().take(count).enumerate() {
                    out[i + j] = predict(i + j, i64::from(*v), out)?;
                }
                i += count;
            }
            enc::TAG2_3S32 => {
                r.align();
                let vals = enc::read_tag2_3s32(r)?;
                for (j, v) in vals.iter().enumerate() {
                    out[i + j] = predict(i + j, i64::from(*v), out)?;
                }
                i += 3;
            }
            enc::TAG2_3SVARIABLE => {
                r.align();
                let vals = enc::read_tag2_3svariable(r)?;
                for (j, v) in vals.iter().enumerate() {
                    out[i + j] = predict(i + j, i64::from(*v), out)?;
                }
                i += 3;
            }
            enc::TAG8_4S16 => {
                r.align();
                let vals = enc::read_tag8_4s16(r, data_version)?;
                for (j, v) in vals.iter().enumerate() {
                    out[i + j] = predict(i + j, i64::from(*v), out)?;
                }
                i += 4;
            }
            e => {
                let raw: i64 = match e {
                    enc::SIGNED_VB => {
                        r.align();
                        i64::from(enc::read_svb(r)?)
                    }
                    enc::UNSIGNED_VB => {
                        r.align();
                        i64::from(enc::read_uvb(r)?)
                    }
                    enc::NEG_14BIT => {
                        r.align();
                        i64::from(enc::read_neg14(r)?)
                    }
                    enc::ELIAS_DELTA_U32 => i64::from(enc::read_elias_delta_u32(r)?),
                    enc::ELIAS_DELTA_S32 => i64::from(enc::read_elias_delta_s32(r)?),
                    enc::NULL => 0,
                    other => return Err(DecodeError::Unsupported(other)),
                };
                let v = predict(i, raw, out)?;
                // Non-grouped fields are 32-bit quantities in the firmware.
                out[i] = if def.signed[i] {
                    i64::from(v as i32)
                } else {
                    i64::from(v as u32)
                };
                i += 1;
            }
        }
    }
    r.align();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::encode;

    const HDR: &str = "H Product:Blackbox flight data recorder by Nicholas Sherlock\n\
H Data version:2\n\
H I interval:4\n\
H P interval:2\n\
H Field I name:loopIteration,time,gyroADC[0],gyroADC[1],gyroADC[2],motor[0],motor[1]\n\
H Field I signed:0,0,1,1,1,0,0\n\
H Field I predictor:0,0,0,0,0,11,5\n\
H Field I encoding:1,1,0,0,0,1,0\n\
H Field P predictor:6,2,3,3,3,3,3\n\
H Field P encoding:9,0,7,7,7,0,0\n\
H Field S name:flightModeFlags,stateFlags,failsafePhase,rxSignalReceived,rxFlightChannelsValid\n\
H Field S signed:0,0,0,0,0\n\
H Field S predictor:0,0,0,0,0\n\
H Field S encoding:1,1,7,7,7\n\
H minthrottle:1070\n\
H motorOutput:158,2047\n";

    fn intra(buf: &mut Vec<u8>, it: u32, t: u32, g: [i32; 3], m: [i32; 2]) {
        buf.push(b'I');
        encode::write_uvb(buf, it);
        encode::write_uvb(buf, t);
        for v in g {
            encode::write_svb(buf, v);
        }
        encode::write_uvb(buf, (m[0] - 158) as u32);
        encode::write_svb(buf, m[1] - m[0]);
    }

    /// Inter frame given the two previous decoded frames.
    fn inter(buf: &mut Vec<u8>, t: i64, g: [i64; 3], m: [i64; 2], p: &[i64], p2: &[i64]) {
        buf.push(b'P');
        encode::write_svb(buf, (t - (2 * p[1] - p2[1])) as i32);
        let d = [
            (g[0] - p[2].midpoint(p2[2])) as i32,
            (g[1] - p[3].midpoint(p2[3])) as i32,
            (g[2] - p[4].midpoint(p2[4])) as i32,
        ];
        encode::write_tag2_3s32(buf, d);
        encode::write_svb(buf, (m[0] - p[5].midpoint(p2[5])) as i32);
        encode::write_svb(buf, (m[1] - p[6].midpoint(p2[6])) as i32);
    }

    fn collect(data: &[u8]) -> (Vec<Vec<i64>>, Vec<EventData>, ParseStats) {
        let h = Headers::parse(data).unwrap();
        let mut p = Parser::new(data, &h);
        let mut frames = Vec::new();
        let mut events = Vec::new();
        while let Some(f) = p.next_frame() {
            match f {
                Frame::Main { values, .. } => frames.push(values.to_vec()),
                Frame::Event(e) => events.push(e),
                _ => {}
            }
        }
        (frames, events, p.stats.clone())
    }

    #[test]
    fn intra_and_inter_frames() {
        let mut data = HDR.as_bytes().to_vec();
        // iteration 0: I frame; iterations 2: P; 4: I; 6: P (P interval 2, I interval 4)
        intra(&mut data, 0, 1000, [10, -20, 30], [1200, 1300]);
        let f0 = [0i64, 1000, 10, -20, 30, 1200, 1300];
        inter(&mut data, 1250, [12, -25, 33], [1210, 1290], &f0, &f0);
        let f1 = [2i64, 1250, 12, -25, 33, 1210, 1290];
        intra(&mut data, 4, 1500, [15, -30, 36], [1220, 1280]);
        let f2 = [4i64, 1500, 15, -30, 36, 1220, 1280];
        inter(&mut data, 1750, [100, -200, 300], [1000, 2000], &f2, &f2);
        let f3 = [6i64, 1750, 100, -200, 300, 1000, 2000];
        // slow frame + events
        data.extend_from_slice(b"S");
        encode::write_uvb(&mut data, 3);
        encode::write_uvb(&mut data, 1);
        encode::write_tag2_3s32(&mut data, [0, 1, 1]);
        data.extend_from_slice(b"E\x0f");
        encode::write_uvb(&mut data, 7);
        data.extend_from_slice(b"E\xffEnd of log\0");
        data.extend_from_slice(b"garbage after end");

        let (frames, events, stats) = collect(&data);
        assert_eq!(
            frames,
            vec![f0.to_vec(), f1.to_vec(), f2.to_vec(), f3.to_vec()]
        );
        assert_eq!(
            events,
            vec![EventData::Disarm { reason: 7 }, EventData::LogEnd]
        );
        assert_eq!(stats.frames[&b'I'].valid, 2);
        assert_eq!(stats.frames[&b'P'].valid, 2);
        assert_eq!(stats.frames[&b'S'].valid, 1);
        assert_eq!(stats.frames[&b'E'].valid, 2);
        assert_eq!(stats.corrupt_total(), 0);
        assert_eq!(stats.intentionally_absent_iterations, 3);
    }

    #[test]
    fn resyncs_after_corruption() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, 1000, [1, 2, 3], [1200, 1300]);
        let f0 = [0i64, 1000, 1, 2, 3, 1200, 1300];
        // corrupt: a P frame cut short, followed by junk
        data.extend_from_slice(b"P\x80\x80\x80");
        data.extend_from_slice(b"\x00\x01\x02zz");
        // a valid I frame resynchronises
        intra(&mut data, 8, 2000, [4, 5, 6], [1210, 1310]);
        let f1 = [8i64, 2000, 4, 5, 6, 1210, 1310];
        inter(&mut data, 2250, [7, 8, 9], [1220, 1320], &f1, &f1);
        let f2 = [10i64, 2250, 7, 8, 9, 1220, 1320];
        let (frames, _, stats) = collect(&data);
        assert_eq!(frames, vec![f0.to_vec(), f1.to_vec(), f2.to_vec()]);
        assert!(stats.corrupt_total() >= 1);
        assert!(stats.skipped_bytes >= 1);
    }

    #[test]
    fn gives_up_on_trailing_garbage() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, 1000, [1, 2, 3], [1200, 1300]);
        let f0 = [0i64, 1000, 1, 2, 3, 1200, 1300];
        inter(&mut data, 1250, [4, 5, 6], [1210, 1310], &f0, &f0);
        let f1 = [2i64, 1250, 4, 5, 6, 1210, 1310];
        inter(&mut data, 1500, [4, 5, 6], [1210, 1310], &f1, &f0);
        let f2 = [4i64, 1500, 4, 5, 6, 1210, 1310];
        inter(&mut data, 1750, [4, 5, 6], [1210, 1310], &f2, &f1);
        let good_len = data.len();
        // 40 KiB of pseudo-random bytes with a frame letter every ~12 bytes, like stale
        // data from another file (the last good frame is lost too: no frame follows it)
        let mut x: u32 = 0x1234_5678;
        for _ in 0..40 * 1024 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            data.push(if x.is_multiple_of(12) {
                b"IPSGHE"[(x as usize / 12) % 6]
            } else {
                (x >> 8) as u8
            });
        }
        let (frames, _, stats) = collect(&data);
        assert_eq!(
            &frames[..3],
            &[f0.to_vec(), f1.to_vec(), f2.to_vec()],
            "the good frames survive"
        );
        assert!(
            frames.len() <= 5,
            "junk rarely passes as a frame: {}",
            frames.len()
        );
        assert!(stats.trailing_bytes > 20 * 1024, "{}", stats.trailing_bytes);
        assert!(
            stats.trailing_bytes as usize + stats.skipped_bytes as usize + good_len
                <= data.len() + 1,
            "trailing and skipped bytes do not overlap"
        );
        assert!(
            stats.corrupt_total() < 6 * 1024,
            "attempts stop once nothing decodes: {}",
            stats.corrupt_total()
        );
    }

    /// The frames of a steady flight over `iterations` (which start on a
    /// multiple of 4): an I frame every fourth iteration, a P frame between,
    /// `rate` microseconds per iteration from `t0` at iteration 0. Returns
    /// the rows a decoder has to produce.
    fn steady(
        data: &mut Vec<u8>,
        iterations: std::ops::Range<u32>,
        t0: i64,
        rate: i64,
    ) -> Vec<Vec<i64>> {
        let mut rows: Vec<Vec<i64>> = Vec::new();
        for it in iterations.step_by(2) {
            let i = i64::from(it);
            let (t, g) = (t0 + i * rate, i % 50);
            let row = vec![i, t, g, -g, 7, 1200 + i % 7, 1300];
            if it.is_multiple_of(4) {
                intra(
                    data,
                    it,
                    t as u32,
                    [g as i32, -g as i32, 7],
                    [row[5] as i32, 1300],
                );
            } else {
                let p = rows.last().expect("a P frame follows an I frame");
                inter(data, t, [g, -g, 7], [row[5], 1300], p, p);
            }
            rows.push(row);
        }
        rows
    }

    /// Compare decoded rows with the expected ones without printing a whole
    /// log when they differ.
    fn assert_same(frames: &[Vec<i64>], rows: &[Vec<i64>], what: &str) {
        assert_eq!(frames.len(), rows.len(), "{what}: number of frames");
        if let Some(i) = (0..rows.len()).find(|i| frames[*i] != rows[*i]) {
            panic!("{what}: frame {i} is {:?}, not {:?}", frames[i], rows[i]);
        }
    }

    /// The same flight logged with I frames only, so that the parser never
    /// learns its rate.
    fn steady_intra_only(
        data: &mut Vec<u8>,
        iterations: std::ops::Range<u32>,
        t0: i64,
        rate: i64,
    ) -> Vec<Vec<i64>> {
        let mut rows = Vec::new();
        for it in iterations.step_by(4) {
            let i = i64::from(it);
            let t = t0 + i * rate;
            intra(data, it, t as u32, [1, 2, 3], [1200, 1300]);
            rows.push(vec![i, t, 1, 2, 3, 1200, 1300]);
        }
        rows
    }

    /// Issue #183 B: frames lost while logging leave a gap of more than
    /// [`MAX_ITERATION_JUMP`] iterations with no "logging resume" event.
    /// The I frame after it is outside the window of the last frame before
    /// it, and so was every frame after that: the rest of the log was lost.
    #[test]
    fn goes_on_after_a_gap_without_a_resume_event() {
        // 39 600 iterations (4.95 s) lost, then 20 s, then 30 s more
        for (second, third) in [(40_000u32, 40_400u32), (160_000, 400_000)] {
            let mut data = HDR.as_bytes().to_vec();
            let mut rows = steady(&mut data, 0..400, 1_000_000, 125);
            rows.extend(steady(&mut data, second..second + 400, 1_000_000, 125));
            rows.extend(steady(&mut data, third..third + 400, 1_000_000, 125));
            let (frames, _, stats) = collect(&data);
            assert_same(&frames, &rows, "two gaps");
            assert_eq!((stats.corrupt_total(), stats.desync_total()), (0, 0));
            let gap = |from: u32, to: u32| StreamGap {
                iterations: to - from,
                time_us: i64::from(to - from) * 125,
                hole_bytes: 0,
            };
            let expected: Vec<StreamGap> = [(398, second), (second + 398, third)]
                .into_iter()
                .filter(|(from, to)| to - from >= MAX_ITERATION_JUMP)
                .map(|(from, to)| gap(from, to))
                .collect();
            assert_eq!(stats.resumed_gaps, expected);
        }
        // a log of I frames only has no rate to hold the gap against: the
        // frames after the gap carry it
        let mut data = HDR.as_bytes().to_vec();
        let mut rows = steady_intra_only(&mut data, 0..400, 1_000_000, 125);
        rows.extend(steady_intra_only(&mut data, 80_000..80_400, 1_000_000, 125));
        let (frames, _, stats) = collect(&data);
        assert_same(&frames, &rows, "I frames only");
        assert_eq!(stats.resumed_gaps.len(), 1);
        // the time counter may wrap inside the gap: 2^32 us is 34 359 738
        // iterations of 125 us
        let mut data = HDR.as_bytes().to_vec();
        let mut rows = steady(&mut data, 0..400, 1_000_000, 125);
        rows.extend(steady(&mut data, 34_400_000..34_400_400, 1_000_000, 125));
        let (frames, _, stats) = collect(&data);
        assert_same(&frames, &rows, "time runs on past 2^32 us");
        assert!(frames[200][1] > 1 << 32);
        assert_eq!(stats.resumed_gaps.len(), 1);
        // the log may end soon after the gap: the frames up to the "end of
        // log" marker are evidence too
        let mut data = HDR.as_bytes().to_vec();
        let mut rows = steady(&mut data, 0..400, 1_000_000, 125);
        rows.extend(steady(&mut data, 40_000..40_004, 1_000_000, 125));
        data.extend_from_slice(b"E\xffEnd of log\0");
        let (frames, _, _) = collect(&data);
        assert_same(&frames, &rows, "the end of the log after a gap");
    }

    /// The other direction of #183 B: what is not this log going on is
    /// still not read as frames, however much it looks like frames.
    #[test]
    fn what_follows_a_gap_needs_evidence() {
        let good = |data: &mut Vec<u8>| steady(data, 0..400, 1_000_000, 125);
        // one frame with the right counters and nothing readable after it
        let mut data = HDR.as_bytes().to_vec();
        let mut rows = good(&mut data);
        intra(&mut data, 40_000, 6_000_000, [1, 2, 3], [1200, 1300]);
        data.extend_from_slice(b"I\x80\x80\x00\x01\x02zz");
        rows.extend(steady(&mut data, 400..800, 1_000_000, 125));
        let (frames, _, stats) = collect(&data);
        assert_same(&frames, &rows, "the lone frame is not taken, the log is");
        assert!(stats.resumed_gaps.is_empty());
        assert!(stats.desync_total() >= 1);
        // the remains of another flight: frames that agree with each other,
        // but whose time is not where this log's iteration count puts it
        for (iterations, t0, rate) in [
            (40_000..40_400, 300_000_000, 125), // another time base
            (40_000..40_400, 1_000_000, 500),   // another rate
            (40_000..40_400, -2_000_000, 200),  // the right time at the first frame only
            (0..300, 1_000_000, 125),           // iterations this log already has
            (40_000..40_400, -4_500_000, 125),  // time runs backwards
        ] {
            let mut data = HDR.as_bytes().to_vec();
            let rows = good(&mut data);
            let _ = steady(&mut data, iterations.clone(), t0, rate);
            let (frames, _, stats) = collect(&data);
            assert_same(
                &frames,
                &rows,
                &format!("{iterations:?} from {t0} at {rate}"),
            );
            assert!(stats.resumed_gaps.is_empty());
        }
        // with I frames only there is no rate so far: the gap still has to
        // have the rate of the frames after it
        let mut data = HDR.as_bytes().to_vec();
        let rows = steady_intra_only(&mut data, 0..400, 1_000_000, 125);
        let _ = steady_intra_only(&mut data, 80_000..80_400, 300_000_000, 125);
        let (frames, _, stats) = collect(&data);
        assert_same(&frames, &rows, "I frames only, another time base");
        assert!(stats.resumed_gaps.is_empty());
        // an I frame on an iteration the firmware writes none on
        let mut data = HDR.as_bytes().to_vec();
        let rows = good(&mut data);
        for it in (40_002u32..40_400).step_by(4) {
            let t = 1_000_000 + it * 125;
            intra(&mut data, it, t, [1, 2, 3], [1200, 1300]);
        }
        let (frames, _, _) = collect(&data);
        assert_same(&frames, &rows, "I frames off the grid");
    }

    /// Pseudo-random bytes with a frame letter every twelfth.
    fn junk(len: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if x.is_multiple_of(12) {
                    b"IPSGHE"[(x as usize / 12) % 6]
                } else {
                    (x >> 8) as u8
                }
            })
            .collect()
    }

    /// Issue #183 C: more than [`GIVE_UP_BYTES`] of undecodable bytes in
    /// the middle of a log (a block of the storage that was never written,
    /// or written over) ended it: everything after the hole was reported as
    /// trailing bytes.
    #[test]
    fn goes_on_after_a_hole_in_the_storage() {
        let holes: [(&str, Vec<u8>); 3] = [
            ("zeros", vec![0; 20 * 1024]),
            ("erased flash", vec![0xFF; 20 * 1024]),
            ("another file's bytes", junk(64 * 1024, 0x1234_5678)),
        ];
        for (what, hole) in &holes {
            // the frames on both sides of the hole belong together; then
            // frames were lost with the hole; then the log has I frames only
            for (lost, intra_only) in [(0u32, false), (20_000, false), (0, true), (20_000, true)] {
                let write = if intra_only {
                    steady_intra_only
                } else {
                    steady
                };
                let mut data = HDR.as_bytes().to_vec();
                let mut rows = write(&mut data, 0..400, 1_000_000, 125);
                // the frame before the hole goes with it: no frame follows it
                rows.pop();
                data.extend_from_slice(hole);
                rows.extend(write(&mut data, 400 + lost..800 + lost, 1_000_000, 125));
                data.extend_from_slice(b"E\xffEnd of log\0");
                let (frames, events, stats) = collect(&data);
                let what = format!("{what}, {lost} iterations lost, I only: {intra_only}");
                assert_same(&frames, &rows, &what);
                assert_eq!(events, [EventData::LogEnd], "{what}");
                assert_eq!(stats.trailing_bytes, 0, "{what}");
                assert_eq!(stats.resumed_gaps.len(), 1, "{what}");
                let gap = stats.resumed_gaps[0];
                assert!(gap.hole_bytes >= hole.len() as u64, "{what}: {gap:?}");
                assert!(gap.iterations >= lost, "{what}: {gap:?}");
                assert!(
                    stats.skipped_bytes as usize + stats.corrupt_total() as usize >= hole.len() / 2
                        && stats.skipped_bytes as usize <= hole.len() + 64,
                    "{what}: the hole is counted once: {stats:?}"
                );
            }
        }
        // a hole right after the headers, before any frame
        let mut data = HDR.as_bytes().to_vec();
        data.extend_from_slice(&holes[0].1);
        let rows = steady(&mut data, 400..800, 1_000_000, 125);
        let (frames, _, stats) = collect(&data);
        assert_same(&frames, &rows, "a hole before the first frame");
        assert_eq!(stats.resumed_gaps[0].iterations, 0);
    }

    /// The other direction of #183 C: after the hole nothing is taken that
    /// is not this log going on, and a file that ends in junk still ends
    /// where its log does.
    #[test]
    fn what_follows_a_hole_needs_evidence() {
        let hole = vec![0u8; 20 * 1024];
        for (what, iterations, t0, rate) in [
            ("another time base", 40_000..40_400, 300_000_000, 125),
            ("another rate", 400..800, 1_000_000, 500),
            ("an earlier part", 0..300, 1_000_000, 125),
        ] {
            for write in [steady, steady_intra_only] {
                let mut data = HDR.as_bytes().to_vec();
                let mut rows = write(&mut data, 0..400, 1_000_000, 125);
                rows.pop();
                data.extend_from_slice(&hole);
                let tail = data.len();
                let _ = write(&mut data, iterations.clone(), t0, rate);
                let (frames, _, stats) = collect(&data);
                assert_same(&frames, &rows, what);
                assert!(stats.resumed_gaps.is_empty(), "{what}");
                assert!(
                    stats.trailing_bytes as usize >= data.len() - tail,
                    "{what}: the rest is trailing bytes"
                );
            }
        }
        // 2 MiB of junk after a log: all of it is looked through, none taken
        for seed in [0x1234_5678, 0x0BAD_F00D, 0x9E37_79B9] {
            let mut data = HDR.as_bytes().to_vec();
            let rows = steady(&mut data, 0..400, 1_000_000, 125);
            let good = data.len();
            data.extend_from_slice(&junk(2 * 1024 * 1024, seed));
            let (frames, _, stats) = collect(&data);
            assert_eq!(&frames[..rows.len() - 1], &rows[..rows.len() - 1]);
            assert!(frames.len() <= rows.len() + 5, "{}", frames.len());
            assert!(stats.resumed_gaps.is_empty());
            assert!(stats.trailing_bytes as usize > data.len() - good - 2 * GIVE_UP_BYTES);
        }
    }

    /// Bytes that are not a log do not become one: pseudo-random bytes of
    /// several kinds after good frames, many times over.
    #[test]
    fn junk_is_not_taken_for_the_log_going_on() {
        let mut x: u32 = 0x9E37_79B9;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let mut accepted_junk = 0;
        for round in 0..150 {
            let mut data = HDR.as_bytes().to_vec();
            let rows = steady(&mut data, 0..400, 1_000_000, 125);
            // below the give-up limit, so that every byte is tried
            for _ in 0..12 * 1024 {
                let r = next();
                data.push(match round % 3 {
                    // any byte, a frame letter every twelfth
                    0 if r.is_multiple_of(12) => b"IPSGHE"[(r as usize / 12) % 6],
                    0 => (r >> 8) as u8,
                    // short numbers and many I frames: most attempts decode
                    1 if r.is_multiple_of(5) => b'I',
                    1 => (r >> 8) as u8 & 0x7F,
                    // numbers of every length
                    _ if r.is_multiple_of(7) => b"IP"[(r as usize / 7) % 2],
                    _ if r.is_multiple_of(3) => (r >> 8) as u8 | 0x80,
                    _ => (r >> 8) as u8 & 0x7F,
                });
            }
            let (frames, _, stats) = collect(&data);
            // the last good frame may go: no frame letter follows it
            assert!(frames.len() >= rows.len() - 1, "round {round}");
            assert_eq!(&frames[..rows.len() - 1], &rows[..rows.len() - 1]);
            assert!(stats.resumed_gaps.is_empty(), "round {round}");
            accepted_junk += frames.len().saturating_sub(rows.len());
        }
        // inside the window junk passes as rarely as it did
        assert!(accepted_junk <= 150, "{accepted_junk}");
    }

    #[test]
    fn rotorflight_state_events_keep_the_stream() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, 1000, [1, 2, 3], [1200, 1300]);
        let f0 = [0i64, 1000, 1, 2, 3, 1200, 1300];
        data.extend_from_slice(b"E\x32\x02"); // governor state 2
        inter(&mut data, 1250, [4, 5, 6], [1210, 1310], &f0, &f0);
        let f1 = [2i64, 1250, 4, 5, 6, 1210, 1310];
        data.extend_from_slice(b"E\x34\x01"); // airborne
        data.extend_from_slice(b"E\x64\x03abc"); // custom data, 3 bytes
        data.extend_from_slice(b"E\x65hello\x00"); // custom string
        inter(&mut data, 1500, [7, 8, 9], [1220, 1320], &f1, &f0);
        let f2 = [4i64, 1500, 7, 8, 9, 1220, 1320];
        let (frames, events, stats) = collect(&data);
        assert_eq!(frames, vec![f0.to_vec(), f1.to_vec(), f2.to_vec()]);
        assert_eq!(stats.corrupt_total(), 0, "{stats:?}");
        assert_eq!(stats.desync_total(), 0);
        let names: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                EventData::Other { name, .. } => Some(*name),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            [
                "gov_state",
                "airborne_state",
                "custom_data",
                "custom_string"
            ]
        );
        assert!(matches!(&events[0], EventData::Other { data, .. } if data["state"] == 2));
    }

    #[test]
    fn rejects_time_travel() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, 1000, [1, 2, 3], [1200, 1300]);
        intra(&mut data, 4, 500, [1, 2, 3], [1200, 1300]); // time moves backwards
        intra(&mut data, 8, 1500, [1, 2, 3], [1200, 1300]);
        let (frames, _, stats) = collect(&data);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1][0], 8);
        assert_eq!(stats.frames[&b'I'].desync, 1);
    }

    #[test]
    fn time_rollover() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, u32::MAX - 100, [0, 0, 0], [1200, 1300]);
        intra(&mut data, 4, 100, [0, 0, 0], [1200, 1300]);
        let (frames, _, _) = collect(&data);
        assert_eq!(frames[1][1], i64::from(u32::MAX) + 1 + 100);
    }

    /// Issue #183 D: the time of a GPS frame is logged against the main
    /// frame before it. When the 32-bit counter wrapped between the two,
    /// the GPS frame moved the times of all later main frames a round on,
    /// once more with every frame, and all of them were rejected.
    #[test]
    fn a_gps_frame_after_the_time_wrap_leaves_the_main_frames_alone() {
        let header = format!(
            "{HDR}H Field G name:time,GPS_numSat,GPS_speed\n\
             H Field G signed:0,0,0\n\
             H Field G predictor:10,0,0\n\
             H Field G encoding:1,1,1\n"
        );
        // the 20th I frame is 300 us before the wrap; a GPS frame follows
        // it by `after` us
        for after in [200u32, 300, 400] {
            let t0 = (1i64 << 32) - 19 * 32_000 - 300;
            let mut data = header.as_bytes().to_vec();
            let (mut rows, mut gps_times) = (Vec::new(), Vec::new());
            for k in 0..200i64 {
                let t = t0 + k * 32_000;
                intra(&mut data, k as u32 * 4, t as u32, [1, 2, 3], [1200, 1300]);
                rows.push(vec![k * 4, t, 1, 2, 3, 1200, 1300]);
                if k % 3 == 0 || k == 19 {
                    let dt = if k == 19 { after } else { 250 };
                    data.push(b'G');
                    encode::write_uvb(&mut data, dt);
                    encode::write_uvb(&mut data, 14);
                    encode::write_uvb(&mut data, 1200);
                    gps_times.push(t + i64::from(dt));
                }
            }
            let h = Headers::parse(&data).unwrap();
            let mut p = Parser::new(&data, &h);
            let (mut frames, mut gps) = (Vec::new(), Vec::new());
            while let Some(f) = p.next_frame() {
                match f {
                    Frame::Main { values, .. } => frames.push(values.to_vec()),
                    Frame::Gps { values, .. } => gps.push(values[0]),
                    _ => {}
                }
            }
            assert_same(&frames, &rows, &format!("a GPS frame {after} us on"));
            assert_eq!(gps, gps_times, "{after}");
            assert_eq!(p.stats.desync_total(), 0);
        }
    }

    /// A frame that is rejected leaves the times of the frames after it
    /// alone too (it used to move them a round on when its own time looked
    /// like one after the wrap).
    #[test]
    fn a_rejected_frame_near_the_time_wrap_changes_nothing() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 8, u32::MAX - 1000, [0, 0, 0], [1200, 1300]);
        intra(&mut data, 4, 50, [0, 0, 0], [1200, 1300]); // the iteration runs backwards
        intra(&mut data, 12, u32::MAX - 500, [0, 0, 0], [1200, 1300]);
        intra(&mut data, 16, 500, [0, 0, 0], [1200, 1300]);
        let (frames, _, stats) = collect(&data);
        let times: Vec<i64> = frames.iter().map(|f| f[1]).collect();
        let max = i64::from(u32::MAX);
        assert_eq!(times, [max - 1000, max - 500, max + 1 + 500]);
        assert_eq!(stats.frames[&b'I'].desync, 1);
    }

    /// Issue #183 E: the time of a "logging resume" event was put in the
    /// round of the counter of the frames before the pause: after a pause
    /// that spans the wrap the time ran back by 2^32 us.
    #[test]
    fn a_pause_may_span_the_time_wrap() {
        // I frames only (no rate known): the first such time after the pause
        let t0 = (1i64 << 32) - 5_000_000;
        let mut data = HDR.as_bytes().to_vec();
        let mut rows = Vec::new();
        let mut frame = |data: &mut Vec<u8>, k: i64| {
            let t = t0 + k * 32_000;
            intra(data, k as u32 * 4, t as u32, [1, 2, 3], [1200, 1300]);
            rows.push(vec![k * 4, t, 1, 2, 3, 1200, 1300]);
        };
        for k in 0..50 {
            frame(&mut data, k);
        }
        let resume = t0 + 1050 * 32_000; // 32 s later, past the wrap
        data.extend_from_slice(b"E\x0e");
        encode::write_uvb(&mut data, 1050 * 4);
        encode::write_uvb(&mut data, resume as u32);
        for k in 1050..1100 {
            frame(&mut data, k);
        }
        let (frames, events, stats) = collect(&data);
        assert_same(&frames, &rows, "a pause over the wrap");
        assert_eq!(
            events,
            [EventData::LoggingResume {
                iteration: 4200,
                time_us: resume
            }]
        );
        assert!(resume > 1 << 32);
        assert_eq!(stats.desync_total(), 0);
        assert!(stats.resumed_gaps.is_empty(), "the event announces the gap");
        // with a rate the pause may be longer than a round of the counter:
        // the iteration count says how long it was. 40 000 000 iterations of
        // 125 us are 5000 s; a round is 4295 s
        let mut data = HDR.as_bytes().to_vec();
        let mut rows = steady(&mut data, 0..400, 1_000_000, 125);
        let resume = 1_000_000 + 40_000_000 * 125;
        data.extend_from_slice(b"E\x0e");
        encode::write_uvb(&mut data, 40_000_000);
        encode::write_uvb(&mut data, resume as u32);
        rows.extend(steady(&mut data, 40_000_000..40_000_400, 1_000_000, 125));
        let (frames, events, stats) = collect(&data);
        assert_same(&frames, &rows, "a pause of more than a round");
        assert_eq!(
            events,
            [EventData::LoggingResume {
                iteration: 40_000_000,
                time_us: resume
            }]
        );
        assert!(resume > 1 << 32 && (resume as u32) > 1_000_000);
        assert_eq!(stats.desync_total(), 0);
        // the arming beep is timed against the frames around it
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, u32::MAX - 100, [0, 0, 0], [1200, 1300]);
        data.extend_from_slice(b"E\x00");
        encode::write_uvb(&mut data, 20);
        intra(&mut data, 4, 100, [0, 0, 0], [1200, 1300]);
        data.extend_from_slice(b"E\x00");
        encode::write_uvb(&mut data, u32::MAX - 50);
        intra(&mut data, 8, 600, [0, 0, 0], [1200, 1300]);
        let (frames, events, _) = collect(&data);
        assert_eq!(frames.len(), 3);
        assert_eq!(
            events,
            [
                EventData::SyncBeep {
                    time_us: (1 << 32) + 20
                },
                EventData::SyncBeep {
                    time_us: i64::from(u32::MAX) - 50
                }
            ]
        );
    }

    /// Issue #183 F: Betaflight (2025.12 on) writes the optional fields
    /// `vbatLatest` … `rssi` of a P frame as one group of the 8-value
    /// encoding and the servos as a second one. With gyro, accelerometer,
    /// debug and motor logging switched off the two stand next to each
    /// other, and were read as one group of eight and one of two: the
    /// servo values landed on the wrong servos, with no error.
    #[test]
    fn the_servos_are_a_group_of_their_own() {
        let header = |firmware: &str| {
            format!(
                "H Product:Blackbox flight data recorder by Nicholas Sherlock\n\
                 H Data version:2\n\
                 H I interval:32\n\
                 H P interval:4\n\
                 H Field I name:loopIteration,time,vbatLatest,rssi,servo[0],servo[1],servo[2],\
                 servo[3],servo[4],servo[5],servo[6],servo[7]\n\
                 H Field I signed:0,0,0,0,0,0,0,0,0,0,0,0\n\
                 H Field I predictor:0,0,9,0,8,8,8,8,8,8,8,8\n\
                 H Field I encoding:1,1,3,1,6,6,6,6,6,6,6,6\n\
                 H Field P predictor:6,2,1,1,1,1,1,1,1,1,1,1\n\
                 H Field P encoding:9,0,6,6,6,6,6,6,6,6,6,6\n\
                 H Firmware revision:{firmware}\n\
                 H vbatref:1680\n"
            )
        };
        let mut frames = Vec::new();
        let mut rows: Vec<Vec<i64>> = Vec::new();
        let (mut vbat, rssi) = (1680i64, 800i64);
        let mut servo = [1500i64, 1510, 1490, 1500, 1500, 1500, 1500, 1500];
        for k in 0..64i64 {
            let (it, t) = (k * 4, 5_000_000 + k * 4000);
            if it % 32 == 0 {
                frames.push(b'I');
                encode::write_uvb(&mut frames, it as u32);
                encode::write_uvb(&mut frames, t as u32);
                encode::write_neg14(&mut frames, (vbat - 1680) as i32);
                encode::write_uvb(&mut frames, rssi as u32);
                let centred: Vec<i32> = servo.iter().map(|s| (*s - 1500) as i32).collect();
                encode::write_tag8_8svb(&mut frames, &centred);
            } else {
                let (p, p2) = (&rows[rows.len() - 1], &rows[rows.len().saturating_sub(2)]);
                let p2 = if (it - 4) % 32 == 0 { p } else { p2 };
                frames.push(b'P');
                encode::write_svb(&mut frames, (t - (2 * p[1] - p2[1])) as i32);
                // the firmware: one group for the optional fields ...
                encode::write_tag8_8svb(&mut frames, &[-1, 0]);
                vbat -= 1;
                // ... and a second one for the servos
                encode::write_tag8_8svb(&mut frames, &[3, -2, 0, 0, 0, 0, 0, 1]);
                servo[0] += 3;
                servo[1] -= 2;
                servo[7] += 1;
            }
            let mut row = vec![it, t, vbat, rssi];
            row.extend_from_slice(&servo);
            rows.push(row);
        }
        let mut data = header("Betaflight 2025.12.2 (c6b3f0a) STM32F7X2").into_bytes();
        data.extend_from_slice(&frames);
        let h = Headers::parse(&data).unwrap();
        assert!(h.servo_group);
        assert_eq!(h.def(b'P').unwrap().group_starts, [4]);
        let (decoded, _, stats) = collect(&data);
        assert_same(&decoded, &rows, "two groups");
        assert_eq!((stats.corrupt_total(), stats.desync_total()), (0, 0));
        // a firmware that is not known to write the servos apart is read as
        // before: neighbouring fields in groups of eight
        let mut data = header("INAV 7.0.0 (abcdef0) MATEKF405").into_bytes();
        data.extend_from_slice(&frames);
        let h = Headers::parse(&data).unwrap();
        assert!(!h.servo_group && h.def(b'P').unwrap().group_starts.is_empty());
        let (decoded, _, _) = collect(&data);
        assert!(decoded != rows, "read as one group of eight and one of two");
    }

    #[test]
    fn logging_resume_event_reanchors() {
        let mut data = HDR.as_bytes().to_vec();
        intra(&mut data, 0, 1000, [0, 0, 0], [1200, 1300]);
        data.extend_from_slice(b"E\x0e");
        encode::write_uvb(&mut data, 100_000);
        encode::write_uvb(&mut data, 60_000_000);
        intra(&mut data, 100_000, 60_000_000, [1, 1, 1], [1200, 1300]);
        let (frames, events, stats) = collect(&data);
        assert_eq!(frames.len(), 2);
        assert_eq!(
            events,
            vec![EventData::LoggingResume {
                iteration: 100_000,
                time_us: 60_000_000
            }]
        );
        assert_eq!(stats.desync_total(), 0);
    }
}
