//! Writer of Betaflight Blackbox logs: a canonical session becomes the
//! bytes a flight controller of a chosen release would have logged for it.
//!
//! This is the inverse of the decoder and of the Betaflight profile. The
//! caller names the release to write for ([`Target`]); everything that
//! differs between releases (the fields of each frame and the conditions
//! they are logged on, the header lines, the units of the stick commands)
//! comes from the description of that release in `version` and `sysinfo`,
//! which was read from the firmware source at the release's tag.
//!
//! The channels are turned back into the firmware's integers through the
//! decoder's own field mapping, the settings into headers, and the frames
//! are packed with the predictors and encodings of the field definitions,
//! in the order the firmware writes them: an `I` frame every `I interval`
//! loop iterations and `P` frames between, slow frames when the flight
//! modes change (and after a pause, and every 256 `I` intervals), GPS and
//! GPS home frames, and the events.
//!
//! Two kinds of session are written:
//!
//! - one **decoded from a Blackbox log** is written back as that log had
//!   it: its loop counter, its events, its slow and GPS frames, its
//!   headers. For the release that logged it the frames come out byte for
//!   byte (`tests/rewrite.rs`);
//! - any other (a synthetic flight of `openpid gen`, the calibration probes
//!   of `docs/calibration`) is logged as a flight controller would log the
//!   flight: the loop counter follows the loop rates, the arming beep, the
//!   slow frames and the end of the log are added, a sample without a gyro
//!   value pauses the log. Fields the session has no channel for are not
//!   logged, and the headers say so.
//!
//! No firmware code is copied: the layout follows the format description
//! ("Blackbox Internals") and the names and numbers of the firmware's
//! tables.

mod config;
mod fields;
mod frames;
mod sysinfo;
mod version;

pub use version::Target;

use config::{Lines, Source};
use fields::Layout;
use frames::References;
use openpid_core::modes::{FlightMode, modes_at};
use openpid_core::session::{AuxRow, AuxTable, Event, EventKind};
use openpid_core::settings::LoopRates;
use openpid_core::{
    Axis, ChannelId, FirmwareInfo, FirmwareVendor, FlightSession, GyroStage, Origin,
};
use version::FieldDef;

/// `I` intervals between two slow frames when nothing changes
/// (`blackboxSInterval = blackboxIInterval * 256`).
const SLOW_EVERY_INTRA: i64 = 256;
/// Reason of the disarm event the writer adds: the arming switch.
const DISARM_SWITCH: u32 = 4;
/// Flight mode flags of a session that names no modes: armed, nothing else.
const ARMED: i64 = 1;

/// What the caller may set beyond the session itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// Added to every timestamp, µs: a flight controller logs the time since
    /// it was powered, not since the log began.
    pub time_offset_us: i64,
    /// Headers that replace the written ones by name (or are appended), for
    /// what the session does not model: rates, the craft name, a debug mode,
    /// `blackbox_high_resolution`, `fields_disabled_mask`. The frames follow
    /// them as they would on a craft configured so.
    pub headers: Vec<(String, String)>,
    /// Whether the log ends with a disarm event, as after a normal landing
    /// (for a session that was not decoded from a log; a decoded one keeps
    /// the events it has).
    pub disarm: bool,
    /// The Betaflight release to write the log for.
    pub target: Target,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            time_offset_us: 0,
            headers: Vec::new(),
            disarm: true,
            target: Target::default(),
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
         whole multiple of the log rate, of at most one I-frame interval (32 ms)"
    )]
    Rate {
        /// Rate of the logged frames.
        log_hz: f64,
        /// Rate of the PID loop.
        pid_hz: f64,
    },
    /// A timestamp is negative: the firmware's clock starts at zero.
    #[error("the timestamp {0} µs is before the flight controller was powered")]
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

/// The schedule of the frames: which loop iterations are logged.
///
/// The firmware (`blackboxAdvanceIterationTimers`, the same in every
/// release) logs an `I` frame at every multiple of the `I interval` and,
/// counting anew from each of them, a `P` frame every `P interval`
/// iterations; with `P interval:0` it logs the `I` frames only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Schedule {
    /// Loop iterations between two `I` frames.
    i_interval: i64,
    /// Loop iterations between two `P` frames, 0 for none.
    p_interval: i64,
}

impl Schedule {
    /// The schedule of the loop rates, with the header lines that state it,
    /// by the arithmetic of the firmware (`gyroInitSensor` and
    /// `blackboxInit`, the same from 4.2 to `master`): the gyro period and
    /// the PID period are whole microseconds, rounded down, and an `I`
    /// frame is due every 32 ms of PID loops, rounded down again.
    fn of_rates(
        target: Target,
        rates: &LoopRates,
        sampled_hz: f64,
        lines: &mut Lines,
    ) -> Result<Self, WriteError> {
        let log_hz = rates.log_hz.unwrap_or(sampled_hz);
        let pid_hz = rates.pid_hz.unwrap_or(log_hz);
        let gyro_hz = rates.gyro_hz.unwrap_or(pid_hz);
        let bad = WriteError::Rate { log_hz, pid_hz };
        if !(log_hz > 0.0 && pid_hz >= log_hz && gyro_hz >= pid_hz) {
            return Err(bad);
        }
        let whole = |ratio: f64| {
            let n = ratio.round();
            ((ratio - n).abs() <= 1e-6).then_some(n as i64)
        };
        let (Some(per_frame), Some(pid_denom)) = (whole(pid_hz / log_hz), whole(gyro_hz / pid_hz))
        else {
            return Err(bad);
        };
        // 1e6 / 3200 is 312.5: the firmware keeps 312
        let microseconds = |period: f64| (period + 1e-9).floor() as i64;
        let looptime = microseconds(1.0e6 / gyro_hz);
        let pid_looptime = microseconds(pid_denom as f64 * 1.0e6 / gyro_hz).max(1);
        let i_interval = (32_000 / pid_looptime).max(1);
        if per_frame > i_interval {
            return Err(bad);
        }
        // every frame an I frame: the firmware says `P interval:0` unless
        // the interval is one of its own five (1, 2, 4, 8, 16; from 4.3)
        let its_own =
            target.has_sample_rates() && per_frame <= 16 && (per_frame as u64).is_power_of_two();
        let p_interval = if per_frame == i_interval && !its_own {
            0
        } else {
            per_frame
        };
        lines.replace("I interval", i_interval.to_string());
        lines.replace("P interval", p_interval.to_string());
        // `master` defines the ratio of a log without P frames as 0; the
        // releases before it divide by zero there
        let ratio = if p_interval == 0 {
            0
        } else {
            i_interval / p_interval
        };
        lines.replace("P ratio", ratio.to_string());
        lines.replace("looptime", looptime.to_string());
        lines.replace("pid_process_denom", pid_denom.to_string());
        Ok(Schedule {
            i_interval,
            p_interval,
        })
    }

    /// The schedule the headers state, when the session's own log or the
    /// caller gave both intervals.
    fn of_headers(lines: &Lines) -> Option<Self> {
        let given = lines.is_given("I interval") && lines.is_given("P interval");
        let number = |key: &str| lines.get(key)?.trim().parse::<i64>().ok();
        let (i_interval, p_interval) = (number("I interval")?, number("P interval")?);
        (given && i_interval >= 1 && p_interval >= 0).then_some(Schedule {
            i_interval,
            p_interval,
        })
    }

    /// Whether the firmware logs a main frame at loop iteration `iteration`.
    fn logs(self, iteration: i64) -> bool {
        let since_intra = iteration % self.i_interval;
        since_intra == 0 || (self.p_interval > 0 && since_intra % self.p_interval == 0)
    }

    /// Whether the frame at `iteration` is an `I` frame.
    fn is_intra(self, iteration: i64) -> bool {
        iteration % self.i_interval == 0
    }

    /// The loop iteration of the `frame`-th logged frame, counting from 0.
    fn iteration_of(self, frame: i64) -> i64 {
        if self.p_interval == 0 {
            return frame * self.i_interval;
        }
        let per_intra = (self.i_interval + self.p_interval - 1) / self.p_interval;
        (frame / per_intra) * self.i_interval + (frame % per_intra) * self.p_interval
    }
}

/// Timestamps of the session with the offset. The firmware's clock is a
/// 32-bit count of microseconds that wraps after 71.6 minutes; the values
/// here are not wrapped yet.
fn timestamps(s: &FlightSession, offset_us: i64) -> Result<Vec<i64>, WriteError> {
    let time = s
        .channel(&ChannelId::Time)
        .ok_or(WriteError::MissingChannel("time"))?;
    time.data
        .values()
        .iter()
        .map(|t| {
            let t = (t.round_ties_even() as i64).saturating_add(offset_us);
            if t >= 0 {
                Ok(t)
            } else {
                Err(WriteError::Time(t))
            }
        })
        .collect()
}

/// The 32 bits of a time the firmware logs.
fn wrapped(time_us: i64) -> i64 {
    i64::from(time_us as u32)
}

/// Samples that can be logged: the gyro, where the log has it, has a value
/// on every axis.
fn loggable(s: &FlightSession, n: usize) -> Vec<bool> {
    let gyro: Vec<_> = Axis::ALL
        .into_iter()
        .filter_map(|axis| {
            let stage = GyroStage::Filtered;
            s.channel(&ChannelId::Gyro { axis, stage }).map(|c| &c.data)
        })
        .collect();
    (0..n)
        .map(|i| gyro.iter().all(|g| g.get(i).is_some()))
        .collect()
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

/// The loop counter of a session decoded from a log, when it is one the
/// schedule can have produced: then the frames are written at the very
/// iterations the log had them.
fn logged_iterations(s: &FlightSession, schedule: Schedule, n: usize) -> Option<Vec<i64>> {
    let counter = s.channel(&ChannelId::LoopIteration)?;
    if !matches!(counter.origin, Origin::Logged { .. }) || counter.data.len() != n {
        return None;
    }
    let iterations: Vec<i64> = counter.data.values().iter().map(|v| *v as i64).collect();
    let in_range = iterations
        .iter()
        .all(|it| (0..=i64::from(u32::MAX)).contains(it) && schedule.logs(*it));
    let rising = iterations.windows(2).all(|w| w[1] > w[0]);
    (counter.data.all_valid() && in_range && rising).then_some(iterations)
}

/// The rows of one low-rate table of the session, as rows of the frame the
/// target release defines.
#[derive(Debug)]
struct Rows<'a> {
    letter: u8,
    fields: &'a [FieldDef],
    logged: &'a [AuxRow],
    /// For each field of the frame, its column in the session's table.
    columns: Vec<Option<usize>>,
    next: usize,
}

impl<'a> Rows<'a> {
    fn new(letter: u8, fields: &'a [FieldDef], table: Option<&'a AuxTable>) -> Rows<'a> {
        let columns = fields
            .iter()
            .map(|f| table.and_then(|t| t.columns.iter().position(|c| *c == f.name)))
            .collect();
        Rows {
            letter,
            fields,
            logged: table.map_or(&[], |t| t.rows.as_slice()),
            columns,
            next: 0,
        }
    }

    /// The sample the next row is anchored to, when there is one at or
    /// before `sample`.
    fn due(&self, sample: usize) -> Option<usize> {
        self.logged
            .get(self.next)
            .map(|r| r.sample_index)
            .filter(|at| *at <= sample)
    }

    /// The time the next row was logged at, when the session's table has it.
    fn time_of_next(&self) -> Option<i64> {
        let column = self
            .fields
            .iter()
            .zip(&self.columns)
            .find_map(|(f, c)| c.filter(|_| f.name == "time"))?;
        self.logged.get(self.next)?.values.get(column).copied()
    }

    /// The next row as the values of the target's fields. A field the
    /// session's table has no column for is zero; a missing time is the
    /// time of the last main frame.
    fn take(&mut self, time_offset_us: i64, refs: &References) -> Vec<i64> {
        let row = &self.logged[self.next];
        self.next += 1;
        self.fields
            .iter()
            .zip(&self.columns)
            .map(|(f, column)| {
                let value = column.and_then(|c| row.values.get(c).copied());
                match (f.name.as_str(), value) {
                    ("time", Some(v)) => v + time_offset_us,
                    ("time", None) => refs.last_main_time,
                    (_, v) => v.unwrap_or(0),
                }
            })
            .collect()
    }
}

/// Everything the session carries between its main frames, written at the
/// samples it is anchored to.
#[derive(Debug)]
struct Between<'a> {
    home: Rows<'a>,
    gps: Rows<'a>,
    slow: Rows<'a>,
    events: &'a [Event],
    next_event: usize,
    time_offset_us: i64,
    /// For a log of another release: the bit of the target's
    /// `flightModeFlags` for each bit of the source's.
    modes: Option<Vec<Option<u32>>>,
}

/// Where each bit of the `flightModeFlags` of the release that logged the
/// session is in the target's: the mode boxes were renumbered between
/// releases, so a mask is carried over box by box. `None` when the session
/// is of the target release or not Betaflight's (the bits stay).
fn mode_bits(source: &Source, target: Target) -> Option<Vec<Option<u32>>> {
    if !source.betaflight || source.same_release {
        return None;
    }
    let of_target = boxes_of(target);
    let bits = crate::profiles::betaflight::box_names(&source.firmware)
        .into_iter()
        .map(|name| of_target.iter().position(|n| *n == name).map(|b| b as u32))
        .collect();
    Some(bits)
}

/// The mode boxes of the target release, in the bit order of
/// `flightModeFlags`.
fn boxes_of(target: Target) -> Vec<&'static str> {
    let firmware = FirmwareInfo {
        vendor: FirmwareVendor::Betaflight,
        version: Some(target.version()),
        ..FirmwareInfo::unknown()
    };
    crate::profiles::betaflight::box_names(&firmware)
}

impl Between<'_> {
    /// A mask of mode boxes in the bit order of the target release.
    fn flags(&self, flags: i64) -> i64 {
        let Some(bits) = &self.modes else {
            return flags;
        };
        bits.iter()
            .enumerate()
            .filter(|(from, _)| flags >> from & 1 == 1)
            .filter_map(|(_, to)| *to)
            .fold(0, |mask, to| mask | (1 << to))
    }

    /// Writes what is anchored at or before `sample`.
    ///
    /// The session keeps the events, the GPS frames and the slow frames
    /// apart, each anchored to the main frame that followed; the order among
    /// them is the one the firmware's loop gives them (`blackboxLogIteration`):
    /// on every iteration the events first, then the slow frame straight
    /// before the main frame, then the GPS frames (a home frame always with
    /// the fix after it). So between two main frames the GPS frames come
    /// first, as the frames of the earlier iterations, then the events, then
    /// the slow frame. One event is placed more exactly: the arming beep
    /// carries its time, and it is logged before a fix that is not older
    /// (at arming the beep and the home point arrive within one iteration).
    fn write(&mut self, buf: &mut Vec<u8>, sample: usize, refs: &mut References) {
        loop {
            let (home, fix) = (self.home.due(sample), self.gps.due(sample));
            if home.is_none() && fix.is_none() {
                break;
            }
            let fix_time = self.gps.time_of_next();
            while let Some(e) = self.events.get(self.next_event) {
                match e.kind {
                    EventKind::SyncBeep { time_us }
                        if e.sample_index <= sample && fix_time.is_none_or(|t| time_us <= t) =>
                    {
                        self.event(buf);
                    }
                    _ => break,
                }
            }
            match (home, fix) {
                (Some(h), g) if g.is_none_or(|g| h <= g) => {
                    let row = self.home.take(self.time_offset_us, refs);
                    frames::aux_frame(buf, self.home.letter, self.home.fields, &row, refs);
                    refs.home = [
                        row.first().copied().unwrap_or(0),
                        row.get(1).copied().unwrap_or(0),
                    ];
                }
                _ => {
                    let row = self.gps.take(self.time_offset_us, refs);
                    frames::aux_frame(buf, self.gps.letter, self.gps.fields, &row, refs);
                }
            }
        }
        while self
            .events
            .get(self.next_event)
            .is_some_and(|e| e.sample_index <= sample)
        {
            self.event(buf);
        }
        while self.slow.due(sample).is_some() {
            let mut row = self.slow.take(self.time_offset_us, refs);
            row[0] = self.flags(row[0]);
            frames::aux_frame(buf, self.slow.letter, self.slow.fields, &row, refs);
        }
    }

    /// Writes the next event of the session, its times on the clock of the
    /// log written.
    fn event(&mut self, buf: &mut Vec<u8>) {
        let e = &self.events[self.next_event];
        self.next_event += 1;
        let offset = self.time_offset_us;
        let kind = match &e.kind {
            EventKind::SyncBeep { time_us } => EventKind::SyncBeep {
                time_us: time_us + offset,
            },
            EventKind::LoggingResume { iteration, time_us } => EventKind::LoggingResume {
                iteration: *iteration,
                time_us: time_us + offset,
            },
            EventKind::FlightMode { flags, last_flags } => EventKind::FlightMode {
                flags: self.flags(i64::from(*flags)) as u32,
                last_flags: self.flags(i64::from(*last_flags)) as u32,
            },
            other => other.clone(),
        };
        frames::event(buf, &kind);
    }
}

/// The slow frames and flight-mode events of a session that was not decoded
/// from a log, by the rules of the firmware (`writeSlowFrameIfNeeded`,
/// `blackboxCheckAndLogFlightMode`, the same in every release).
#[derive(Debug)]
struct SlowLog<'a> {
    session: &'a FlightSession,
    /// The mode of each bit of `flightModeFlags` in the target release.
    boxes: Vec<FlightMode>,
    /// The session has slow frames of its own: none are made up.
    own_frames: bool,
    /// The session has flight-mode events of its own.
    own_events: bool,
    /// Loop iterations after which a slow frame is repeated.
    interval: i64,
    /// The state last logged and the iteration it was logged at; `None`
    /// forces a frame (at the start and after a pause).
    logged: Option<([i64; 5], i64)>,
    /// The flags the last flight-mode event (or the start of the log) had.
    flags: i64,
}

impl<'a> SlowLog<'a> {
    fn new(session: &'a FlightSession, target: Target, schedule: Schedule, first: usize) -> Self {
        let boxes = boxes_of(target)
            .into_iter()
            .map(FlightMode::from_name)
            .collect();
        let has_event =
            |wanted: fn(&EventKind) -> bool| session.events.iter().any(|e| wanted(&e.kind));
        let mut log = SlowLog {
            session,
            boxes,
            own_frames: fields::table(session, fields::SLOW_TABLE)
                .is_some_and(|t| !t.rows.is_empty()),
            own_events: has_event(|k| matches!(k, EventKind::FlightMode { .. })),
            interval: schedule.i_interval * SLOW_EVERY_INTRA,
            logged: None,
            flags: 0,
        };
        log.flags = log.flags_at(first);
        log
    }

    /// `flightModeFlags` at `sample`: the bits of the modes the session
    /// names there, in the bit order of the target release.
    fn flags_at(&self, sample: usize) -> i64 {
        if self.session.flight_modes.is_empty() {
            return ARMED;
        }
        let active = modes_at(&self.session.flight_modes, sample);
        self.boxes
            .iter()
            .enumerate()
            .filter(|(_, mode)| active.contains(mode))
            .fold(0, |flags, (bit, _)| flags | (1 << bit))
    }

    /// The event the firmware logs on a loop iteration without an `I` frame
    /// when the modes are not the ones it last logged.
    fn mode_event(&mut self, buf: &mut Vec<u8>, sample: usize) {
        let flags = self.flags_at(sample);
        if self.own_events || self.own_frames || flags == self.flags {
            return;
        }
        let kind = EventKind::FlightMode {
            flags: flags as u32,
            last_flags: self.flags as u32,
        };
        frames::event(buf, &kind);
        self.flags = flags;
    }

    /// The slow frame the firmware logs before a main frame when the state
    /// changed, or was last logged [`SlowLog::interval`] iterations ago, or
    /// the log has just started or resumed. A healthy radio link, no
    /// failsafe: the session says nothing else.
    fn frame(&mut self, buf: &mut Vec<u8>, layout: &Layout, sample: usize, iteration: i64) {
        if self.own_frames {
            return;
        }
        let state = [self.flags_at(sample), 0, 0, 1, 1];
        let due = self
            .logged
            .is_none_or(|(last, at)| last != state || iteration - at >= self.interval);
        if due {
            frames::aux_frame(buf, b'S', &layout.slow, &state, &References::default());
            self.logged = Some((state, iteration));
        }
    }

    /// The log paused: the firmware logs a slow frame when it resumes.
    fn force(&mut self) {
        self.logged = None;
    }
}

/// The header block: the field definitions of every frame, then the
/// system information.
fn header_block(layout: &Layout, lines: Lines) -> Vec<u8> {
    let join = |fields: &[FieldDef], f: &dyn Fn(&FieldDef) -> String| {
        fields.iter().map(f).collect::<Vec<_>>().join(",")
    };
    let mut out: Vec<(String, String)> = vec![
        (
            "Product".into(),
            "Blackbox flight data recorder by Nicholas Sherlock".into(),
        ),
        ("Data version".into(), "2".into()),
    ];
    let mut define = |letter: char, fields: &[FieldDef], deltas: bool| {
        let name = |f: &FieldDef| f.name.clone();
        let signed = |f: &FieldDef| u8::from(f.signed).to_string();
        out.push((format!("Field {letter} name"), join(fields, &name)));
        out.push((format!("Field {letter} signed"), join(fields, &signed)));
        let predictor = join(fields, &|f| f.intra.0.to_string());
        out.push((format!("Field {letter} predictor"), predictor));
        let encoding = join(fields, &|f| f.intra.1.to_string());
        out.push((format!("Field {letter} encoding"), encoding));
        if deltas {
            let predictor = join(fields, &|f| f.inter.0.to_string());
            out.push(("Field P predictor".into(), predictor));
            let encoding = join(fields, &|f| f.inter.1.to_string());
            out.push(("Field P encoding".into(), encoding));
        }
    };
    define('I', &layout.main, true);
    if let Some((home, fix)) = &layout.gps {
        define('H', home, false);
        define('G', fix, false);
    }
    define('S', &layout.slow, false);
    out.extend(lines.into_vec());
    let mut buf = Vec::new();
    for (key, value) in out {
        buf.extend_from_slice(format!("H {key}:{value}\n").as_bytes());
    }
    buf
}

/// Writes `session` as one Betaflight Blackbox log of the release
/// `options.target`.
///
/// The session needs a time channel and the filtered gyro (unless the
/// headers switch the gyro off); everything else is optional. A field the
/// session has no channel for is not logged and `fields_disabled_mask`, or
/// the absence of the sensor's header lines, says so; the PID terms alone
/// are logged as zero when the session has none, which is what the firmware
/// logs while the controller is idle.
///
/// A session decoded from a Blackbox log is written as that log was: every
/// frame at the loop iteration it had, the events and the slow, GPS and home
/// frames where they were, nothing added. (Where the log lost frames to
/// damage, the `P` frames after the gap are predicted from the frames that
/// are left, and a decoder counts their loop iterations on from there.)
///
/// In any other session a sample without a gyro value is not logged: the
/// log pauses there and resumes, with the firmware's "logging resumed"
/// event, at the next `I` frame, as a flight controller does when its
/// storage falls behind.
///
/// # Errors
/// [`WriteError`] when there is nothing to log, a needed channel is absent,
/// or the loop rates of the settings cannot be scheduled.
pub fn betaflight(session: &FlightSession, options: &WriteOptions) -> Result<Vec<u8>, WriteError> {
    let target = options.target;
    let offset = options.time_offset_us;
    let source = Source::of(session, target);
    let time = timestamps(session, offset)?;
    let n = time.len();
    let carried = fields::carried(session, target);
    let mut lines = config::lines(session, &source, target, carried, options);
    let schedule = if let Some(schedule) = Schedule::of_headers(&lines) {
        schedule
    } else {
        let rates = &session.settings.loop_rates;
        Schedule::of_rates(target, rates, sampled_hz(&time), &mut lines)?
    };
    let layout = fields::layout(session, &source, target, schedule, &mut lines)?;
    let gyro_logged = layout.main.iter().any(|f| f.name == "gyroADC[0]");
    let valid = if gyro_logged {
        loggable(session, n)
    } else {
        vec![true; n]
    };
    let first = match valid.iter().position(|v| *v) {
        Some(first) => first,
        // a log that ended before its first frame is written as it was
        None if source.decoded => n,
        None => return Err(WriteError::Empty),
    };

    let mut columns = fields::columns(session, &source, &layout, &lines, n);
    let replayed = logged_iterations(session, schedule, n).filter(|_| source.decoded);
    columns[0] = replayed.unwrap_or_else(|| {
        (0..n)
            .map(|k| schedule.iteration_of(k.saturating_sub(first) as i64))
            .collect()
    });
    columns[1] = time.iter().map(|t| wrapped(*t)).collect();
    let field = |name: &str| layout.main.iter().position(|f| f.name == name);
    if let Some(voltage) = field("vbatLatest").and_then(|v| columns[v].get(first))
        && !lines.is_given("vbatref")
    {
        // the voltage when the log began
        lines.replace("vbatref", voltage.to_string());
    }
    if target >= Target::Master && !lines.is_given("debug_mode_name") {
        let name = match (&session.settings.debug_mode, lines.int("debug_mode")) {
            (_, 0) => "NONE",
            (Some(mode), _) => mode.name.as_deref().unwrap_or("UNKNOWN"),
            _ => "UNKNOWN",
        };
        lines.replace("debug_mode_name", name);
    }
    let mut refs = References {
        motor0: field("motor[0]"),
        motor_low: lines.number("motorOutput", 0),
        vbatref: lines.int("vbatref"),
        ..References::default()
    };

    let no_fields: (Vec<FieldDef>, Vec<FieldDef>) = (Vec::new(), Vec::new());
    let (home_fields, gps_fields) = layout.gps.as_ref().unwrap_or(&no_fields);
    let gps_logged = layout.gps.is_some();
    let table = |kind: &str, logged: bool| fields::table(session, kind).filter(|_| logged);
    let mut between = Between {
        home: Rows::new(b'H', home_fields, table(fields::HOME_TABLE, gps_logged)),
        gps: Rows::new(b'G', gps_fields, table(fields::GPS_TABLE, gps_logged)),
        slow: Rows::new(b'S', &layout.slow, table(fields::SLOW_TABLE, true)),
        events: &session.events,
        next_event: 0,
        time_offset_us: offset,
        modes: mode_bits(&source, target),
    };
    // a decoded log is written as it was; a flight is logged by the rules
    let logging = !source.decoded;
    let mut slow = SlowLog::new(session, target, schedule, first);
    // `P interval:0` logs the slow frames with the I frames; a P interval
    // as long as the I interval logs no P frame either, and no slow frame
    // at all: the firmware writes one only before a P frame
    let intra_only = schedule.p_interval == 0;
    let no_inter = intra_only || schedule.p_interval >= schedule.i_interval;
    let has_event = |wanted: fn(&EventKind) -> bool| session.events.iter().any(|e| wanted(&e.kind));

    let mut buf = header_block(&layout, lines);
    // the two frames before the current one; `None` while the log is paused
    let mut history: Option<(Vec<i64>, Vec<i64>)> = None;
    let mut paused = false;
    for k in first..n {
        if !valid[k] {
            paused = true;
            history = None;
            continue;
        }
        let row: Vec<i64> = columns.iter().map(|c| c[k]).collect();
        let (iteration, time_us) = (row[0], row[1]);
        let intra = schedule.is_intra(iteration);
        if paused && !intra {
            // the firmware resumes a paused log on an I frame only
            continue;
        }
        between.write(&mut buf, k, &mut refs);
        if paused {
            let resume = EventKind::LoggingResume {
                iteration: iteration as u32,
                time_us,
            };
            frames::event(&mut buf, &resume);
            slow.force();
            paused = false;
        }
        if logging && !intra {
            slow.mode_event(&mut buf, k);
        }
        if logging && (!intra || intra_only) {
            slow.frame(&mut buf, &layout, k, iteration);
        }
        let references = history
            .as_ref()
            .filter(|_| !intra)
            .map(|(p, p2)| (p.as_slice(), p2.as_slice()));
        frames::main_frame(&mut buf, &layout.main, &row, references, &refs);
        refs.last_main_time = time_us;
        if logging && k == first && !has_event(|k| matches!(k, EventKind::SyncBeep { .. })) {
            frames::event(&mut buf, &EventKind::SyncBeep { time_us });
        }
        if logging && no_inter {
            // checked on the iteration after the frame, which logs nothing
            slow.mode_event(&mut buf, k);
        }
        history = Some(match history.take() {
            Some((previous, _)) if !intra => (row, previous),
            _ => (row.clone(), row),
        });
    }
    between.write(&mut buf, usize::MAX, &mut refs);
    if logging && !has_event(|k| matches!(k, EventKind::LogEnd)) {
        if options.disarm && !has_event(|k| matches!(k, EventKind::Disarm { .. })) {
            let reason = DISARM_SWITCH;
            frames::event(&mut buf, &EventKind::Disarm { reason });
        }
        frames::event(&mut buf, &EventKind::LogEnd);
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates(gyro_hz: f64, pid_hz: f64, log_hz: f64) -> LoopRates {
        LoopRates {
            gyro_hz: Some(gyro_hz),
            pid_hz: Some(pid_hz),
            log_hz: Some(log_hz),
        }
    }

    /// The lines of the default release, to see what a schedule writes.
    fn lines() -> Lines {
        let session = FlightSession {
            index: 0,
            offset_bytes: 0,
            firmware: FirmwareInfo::unknown(),
            profile_id: "synthetic".into(),
            settings: openpid_core::Settings::default(),
            raw_headers: Vec::new(),
            timebase: openpid_core::Timebase::default(),
            channels: openpid_core::ChannelSet::new(),
            events: Vec::new(),
            aux: Vec::new(),
            flight_modes: Vec::new(),
            diagnostics: openpid_core::session::Diagnostics::default(),
        };
        let source = Source::of(&session, Target::default());
        let carried = config::Carried::default();
        config::lines(
            &session,
            &source,
            Target::default(),
            carried,
            &WriteOptions::default(),
        )
    }

    /// `I interval`, `P interval`, `P ratio`, `looptime` and
    /// `pid_process_denom` for a gyro, PID and log rate.
    fn headers(gyro_hz: f64, pid_hz: f64, log_hz: f64) -> [i64; 5] {
        let mut lines = lines();
        let schedule = Schedule::of_rates(
            Target::default(),
            &rates(gyro_hz, pid_hz, log_hz),
            0.0,
            &mut lines,
        )
        .unwrap();
        assert_eq!(schedule.i_interval, lines.int("I interval"));
        assert_eq!(schedule.p_interval, lines.int("P interval"));
        [
            "I interval",
            "P interval",
            "P ratio",
            "looptime",
            "pid_process_denom",
        ]
        .map(|key| lines.int(key))
    }

    /// The firmware's arithmetic: whole microseconds, rounded down, and
    /// 32 ms of them, rounded down (`gyro_init.c`, `blackboxInit`).
    #[test]
    fn rates_are_rounded_as_the_firmware_rounds_them() {
        assert_eq!(headers(8000.0, 4000.0, 1000.0), [128, 4, 32, 125, 2]);
        assert_eq!(headers(8000.0, 8000.0, 8000.0), [256, 1, 256, 125, 1]);
        // 3.2 kHz: 312.5 µs is logged as 312, and 32000 / 312 is 102
        assert_eq!(headers(3200.0, 3200.0, 800.0), [102, 4, 25, 312, 1]);
        assert_eq!(headers(3200.0, 3200.0, 1600.0), [102, 2, 51, 312, 1]);
        // the PID period of 3.2 kHz / 2 is 625 µs exactly
        assert_eq!(headers(3200.0, 1600.0, 1600.0), [51, 1, 51, 312, 2]);
        // 6.4 kHz: 156.25 µs is 156, and 32000 / 156 is 205
        assert_eq!(headers(6400.0, 6400.0, 3200.0), [205, 2, 102, 156, 1]);
        // every frame an I frame: an interval of the firmware's own, or none
        assert_eq!(headers(8000.0, 2000.0, 125.0), [64, 16, 4, 125, 4]);
        assert_eq!(headers(500.0, 500.0, 31.25), [16, 16, 1, 2000, 1]);
        assert_eq!(headers(8000.0, 4000.0, 31.25), [128, 0, 0, 125, 2]);
        // 4.2 has no interval of its own: any P ratio, and 0 for no P frames
        let mut lines = lines();
        let slow = rates(500.0, 500.0, 31.25);
        let schedule = Schedule::of_rates(Target::V4_2, &slow, 0.0, &mut lines).unwrap();
        assert_eq!((schedule.i_interval, schedule.p_interval), (16, 0));
    }

    #[test]
    fn the_schedule_is_the_firmware_s() {
        // I 102, P 4: the last P frame of an interval is two iterations
        // before the next I frame
        let s = Schedule {
            i_interval: 102,
            p_interval: 4,
        };
        let logged: Vec<i64> = (0..210).filter(|it| s.logs(*it)).collect();
        assert_eq!(&logged[..4], [0, 4, 8, 12]);
        assert_eq!(&logged[24..29], [96, 100, 102, 106, 110]);
        let by_frame: Vec<i64> = (0..logged.len() as i64)
            .map(|f| s.iteration_of(f))
            .collect();
        assert_eq!(by_frame, logged);
        assert!(s.is_intra(204) && !s.is_intra(100));
        // I frames only
        let s = Schedule {
            i_interval: 32,
            p_interval: 0,
        };
        assert!(s.logs(64) && !s.logs(16));
        assert_eq!(s.iteration_of(3), 96);
    }

    #[test]
    fn rates_the_firmware_cannot_log() {
        let refused = |gyro_hz, pid_hz, log_hz| {
            let rates = rates(gyro_hz, pid_hz, log_hz);
            Schedule::of_rates(Target::default(), &rates, 0.0, &mut lines()).is_err()
        };
        assert!(refused(8000.0, 1500.0, 1000.0), "not a whole multiple");
        assert!(refused(6000.0, 4000.0, 1000.0), "gyro and PID");
        assert!(refused(8000.0, 500.0, 1000.0), "a log faster than its loop");
        assert!(
            refused(8000.0, 4000.0, 25.0),
            "less than an I frame each time"
        );
        assert!(refused(8000.0, 4000.0, 0.0));
    }

    #[test]
    fn the_clock_wraps_at_32_bits() {
        assert_eq!(wrapped(5), 5);
        assert_eq!(wrapped(i64::from(u32::MAX)), i64::from(u32::MAX));
        assert_eq!(wrapped(i64::from(u32::MAX) + 3), 2);
    }
}
