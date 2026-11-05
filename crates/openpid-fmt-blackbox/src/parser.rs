//! Frame-level parser with the same validation and resynchronisation rules as
//! the reference decoders: a frame is accepted only when it decodes without
//! error, is not absurdly long, is immediately followed by another frame
//! letter (or EOF), and its iteration/time counters move forward sanely.

use crate::encoding::{self as enc, DecodeError};
use crate::headers::{FrameDef, Headers};
use crate::predictor;
use crate::reader::Reader;
use openpid_core::FrameStats;
use openpid_core::session::AdjustmentValue;
use std::collections::BTreeMap;

/// Longest frame body (without the letter) the firmware can write.
pub const MAX_FRAME_LEN: usize = 256;
/// Bytes without a single accepted frame after which the rest of the file is
/// treated as not being blackbox data at all (stale sectors on the card, another
/// file's remains) instead of one failed frame attempt per byte.
pub const GIVE_UP_BYTES: usize = 16 * 1024;
const MAX_ITERATION_JUMP: u32 = 500 * 10;
const MAX_TIME_JUMP_US: i64 = 10 * 1_000_000;

const EVENT_SYNC_BEEP: u8 = 0;
const EVENT_INFLIGHT_ADJUSTMENT: u8 = 13;
const EVENT_LOGGING_RESUME: u8 = 14;
const EVENT_DISARM: u8 = 15;
const EVENT_FLIGHT_MODE: u8 = 30;
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

/// Streaming parser over the frame data of one log.
#[derive(Debug)]
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
    rollover_us: i64,
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
            rollover_us: 0,
            last_skipped: 0,
            slow_buf: vec![0; headers.def(b'S').map_or(0, FrameDef::len)],
            gps_buf: vec![0; gps_def.map_or(0, FrameDef::len)],
            home_buf: vec![0; home_def.map_or(0, FrameDef::len)],
            home_published: vec![0; home_def.map_or(0, FrameDef::len)],
            home_valid: false,
            stats,
            done: false,
            last_accept_pos: 0,
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
            if self.done {
                return None;
            }
            let start = self.reader.pos();
            if start - self.last_accept_pos > GIVE_UP_BYTES {
                // nothing decodable for a long stretch: the log ended here
                self.stats.trailing_bytes = (self.reader.len() - start) as u64;
                self.done = true;
                return None;
            }
            let letter = self.reader.read_u8()?;
            if !is_frame_letter(letter) {
                self.stats.skipped_bytes += 1;
                self.stream_valid = false;
                continue;
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
                continue;
            }
            let body = body.unwrap_or(Body::Event(None));
            match self.complete(letter, body) {
                Some(acc) => {
                    let st = self.stats.frame(letter);
                    st.valid += 1;
                    st.bytes += size as u64 + 1;
                    // only time-validated main frames prove the stream is still alive
                    if matches!(acc, Accepted::Main { .. }) {
                        self.last_accept_pos = self.reader.pos();
                    }
                    break acc;
                }
                None => {
                    self.stats.frame(letter).desync += 1;
                }
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
                let t = i64::from(enc::read_uvb(r)?) + self.rollover_us;
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
                let t = i64::from(enc::read_uvb(r)?) + self.rollover_us;
                Some(EventData::LoggingResume {
                    iteration,
                    time_us: t,
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

    /// 32-bit microsecond counter rollover recovery.
    fn apply_rollover(&mut self, t: i64) -> i64 {
        if let Some(last) = self.last_time {
            let t32 = t as u32;
            let last32 = last as u32;
            if t32 < last32 && i64::from(t32.wrapping_sub(last32)) < MAX_TIME_JUMP_US {
                self.rollover_us += 1i64 << 32;
            }
        }
        i64::from(t as u32) + self.rollover_us
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

    fn free_slot(&self) -> usize {
        (0..3)
            .find(|i| Some(*i) != self.prev && Some(*i) != self.prev2)
            .expect("three slots")
    }

    fn complete(&mut self, letter: u8, body: Body) -> Option<Accepted> {
        match body {
            Body::Main(kind) => {
                let raw_t = self.hist[self.cur][self.time_idx];
                let t = self.apply_rollover(raw_t);
                self.hist[self.cur][self.time_idx] = t;
                let iteration = self.hist[self.cur][self.iter_idx] as u32;
                let valid = self.validate_main(iteration, t);
                match kind {
                    MainKind::Intra => {
                        if !valid {
                            self.stream_valid = false;
                            self.prev = None;
                            self.prev2 = None;
                            return None;
                        }
                        self.stream_valid = true;
                        self.stats.intentionally_absent_iterations +=
                            u64::from(self.count_skipped_to(iteration));
                        self.last_iteration = Some(iteration);
                        self.last_time = Some(t);
                        let buf = self.cur;
                        self.prev = Some(buf);
                        self.prev2 = Some(buf);
                        self.cur = self.free_slot();
                        Some(Accepted::Main {
                            buf,
                            kind,
                            iteration,
                            time_us: t,
                        })
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
                if let Some(ti) = self.gps_time_idx {
                    let t = self.gps_buf[ti];
                    self.gps_buf[ti] = self.apply_rollover(t);
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
                while count < 8 && i + count < n && def.encodings[i + count] == enc::TAG8_8SVB {
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
