//! Turn parsed frames into the canonical [`FlightSession`].

use crate::headers::{HeaderError, Headers};
use crate::parser::{EventData, Frame, MainKind, ParseStats, Parser, StreamGap};
use openpid_core::modes::{ModeChange, push_change};
use openpid_core::session::{AuxRow, AuxTable, Diagnostics, Event, EventKind};
use openpid_core::{
    BoundProfile, Channel, ChannelId, ChannelSet, FieldMapping, FlightSession, Origin,
    ProfileRegistry, Series, Timebase, Unit,
};

/// Raw decode result of one log: integer columns exactly as logged.
#[derive(Debug, Clone)]
pub struct DecodedLog {
    /// Parsed header block.
    pub headers: Headers,
    /// One column per main-frame field, only frames that passed validation.
    pub columns: Vec<Vec<i64>>,
    /// `true` where the sample came from an intra (`I`) frame.
    pub intra: Vec<bool>,
    /// Slow (`S`) frames.
    pub slow: AuxTable,
    /// GPS (`G`) frames, if defined.
    pub gps: Option<AuxTable>,
    /// GPS home (`H`) frames, if defined.
    pub gps_home: Option<AuxTable>,
    /// Events in log order.
    pub events: Vec<Event>,
    /// Parser counters.
    pub stats: ParseStats,
}

impl DecodedLog {
    /// Main-frame field names in column order.
    pub fn field_names(&self) -> &[String] {
        &self.headers.main_def().names
    }

    /// Number of accepted main frames.
    pub fn len(&self) -> usize {
        self.columns.first().map_or(0, Vec::len)
    }

    /// Whether no main frame was accepted.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Column of a main-frame field by name.
    pub fn column(&self, name: &str) -> Option<&[i64]> {
        let i = self.headers.main_def().index_of(name)?;
        Some(&self.columns[i])
    }
}

fn aux_table(kind: &str, headers: &Headers, letter: u8) -> Option<AuxTable> {
    headers.def(letter).map(|d| AuxTable {
        kind: kind.to_string(),
        columns: d.names.clone(),
        rows: Vec::new(),
    })
}

fn convert_event(e: EventData) -> EventKind {
    match e {
        EventData::SyncBeep { time_us } => EventKind::SyncBeep { time_us },
        EventData::InflightAdjustment { function, value } => {
            EventKind::InflightAdjustment { function, value }
        }
        EventData::LoggingResume { iteration, time_us } => {
            EventKind::LoggingResume { iteration, time_us }
        }
        EventData::Disarm { reason } => EventKind::Disarm { reason },
        EventData::FlightMode { flags, last_flags } => EventKind::FlightMode { flags, last_flags },
        EventData::LogEnd => EventKind::LogEnd,
        EventData::Other { name, data } => EventKind::Other {
            name: name.to_string(),
            data,
        },
    }
}

/// Decode one log (headers + frames) into integer columns.
pub fn decode(data: &[u8]) -> Result<DecodedLog, HeaderError> {
    let headers = Headers::parse(data)?;
    let n = headers.main_def().len();
    let mut columns: Vec<Vec<i64>> = vec![Vec::new(); n];
    let mut intra = Vec::new();
    let mut slow = aux_table("slow", &headers, b'S').unwrap_or_else(|| AuxTable {
        kind: "slow".into(),
        columns: Vec::new(),
        rows: Vec::new(),
    });
    let mut gps = aux_table("gps", &headers, b'G');
    let mut gps_home = aux_table("gps_home", &headers, b'H');
    let mut events = Vec::new();
    let stats = {
        let mut parser = Parser::new(data, &headers);
        while let Some(frame) = parser.next_frame() {
            let sample_index = columns[0].len();
            match frame {
                Frame::Main { kind, values, .. } => {
                    for (c, v) in columns.iter_mut().zip(values) {
                        c.push(*v);
                    }
                    intra.push(kind == MainKind::Intra);
                }
                Frame::Slow(v) => slow.rows.push(AuxRow {
                    sample_index,
                    values: v.to_vec(),
                }),
                Frame::Gps { values, .. } => {
                    if let Some(t) = gps.as_mut() {
                        t.rows.push(AuxRow {
                            sample_index,
                            values: values.to_vec(),
                        });
                    }
                }
                Frame::GpsHome(v) => {
                    if let Some(t) = gps_home.as_mut() {
                        t.rows.push(AuxRow {
                            sample_index,
                            values: v.to_vec(),
                        });
                    }
                }
                Frame::Event(e) => events.push(Event {
                    sample_index,
                    kind: convert_event(e),
                }),
            }
        }
        parser.stats.clone()
    };
    Ok(DecodedLog {
        headers,
        columns,
        intra,
        slow,
        gps,
        gps_home,
        events,
        stats,
    })
}

/// Build the canonical session from a decoded log using the best matching
/// firmware profile (or none when `raw`).
pub fn build_session(
    log: &DecodedLog,
    index: usize,
    offset_bytes: u64,
    profiles: &ProfileRegistry,
    raw: bool,
) -> FlightSession {
    let h = &log.headers;
    let def = h.main_def();
    let profile = if raw {
        None
    } else {
        profiles.select(&h.firmware, &h.raw)
    };
    let profile_id = profile.map_or("raw", |p| p.id()).to_string();
    // the profile of this log: it reads the headers here, once
    let bound = profile.map(|p| p.bind(&h.firmware, &h.raw));
    let settings = bound.as_ref().map(|b| b.settings()).unwrap_or_default();

    let time_idx = def.index_of("time").unwrap_or(1);
    let nominal_dt = settings
        .loop_rates
        .log_hz
        .filter(|hz| *hz > 0.0)
        .map(|hz| 1.0e6 / hz);
    let mut timebase = Timebase::from_times(log.columns[time_idx].clone(), nominal_dt);
    timebase.intentionally_absent_iterations = log.stats.intentionally_absent_iterations;

    let mut channels = ChannelSet::new();
    let mut unmapped = Vec::new();
    for (i, name) in def.names.iter().enumerate() {
        let mapping = bound.as_ref().and_then(|b| b.map_field(name));
        let (id, unit, data) = if let Some(m) = mapping {
            let data = Series::from_vec(log.columns[i].iter().map(|v| m.apply(*v)).collect());
            (m.id, m.unit, data)
        } else {
            if profile.is_some() {
                unmapped.push(name.clone());
            }
            let data = Series::from_vec(log.columns[i].iter().map(|v| *v as f64).collect());
            (ChannelId::Custom(name.clone()), Unit::Raw, data)
        };
        channels.insert(Channel {
            id,
            name: name.clone(),
            unit,
            data,
            origin: Origin::Logged {
                field: name.clone(),
            },
        });
    }

    if let Some(b) = &bound {
        aux_channels(log, b.as_ref(), &mut channels);
    }

    let mut diagnostics = Diagnostics {
        corrupt_frames: log.stats.corrupt_total(),
        desync_frames: log.stats.desync_total(),
        skipped_bytes: log.stats.skipped_bytes,
        total_bytes: log.stats.total_bytes,
        unmapped_fields: unmapped,
        ..Default::default()
    };
    for (letter, st) in &log.stats.frames {
        diagnostics
            .frames
            .insert((*letter as char).to_string(), *st);
    }
    if profile.is_none() && !raw {
        diagnostics
            .notes
            .push("no firmware profile matched; channels are raw".into());
    }
    diagnostics.notes.extend(damage_notes(&log.stats));

    let mut aux = vec![log.slow.clone()];
    aux.extend(log.gps.iter().cloned());
    aux.extend(log.gps_home.iter().cloned());

    let flight_modes = bound
        .as_ref()
        .map_or_else(Vec::new, |b| mode_changes(b.as_ref(), &log.slow));
    let mut session = FlightSession {
        index,
        offset_bytes,
        firmware: h.firmware.clone(),
        profile_id,
        settings,
        raw_headers: h.raw.0.clone(),
        timebase,
        channels,
        events: log.events.clone(),
        flight_modes,
        aux,
        diagnostics,
    };
    if let Some(b) = &bound {
        b.derive(&mut session);
    }
    session
}

/// What the frames beside the main ones carry (slow state, GPS fixes, the
/// home point) becomes channels too where the profile knows the field: on
/// the timeline of the main frames, each value held until the next one. The
/// tables themselves stay in the session's `aux`.
fn aux_channels(log: &DecodedLog, profile: &dyn BoundProfile, channels: &mut ChannelSet) {
    let tables = [Some(&log.slow), log.gps.as_ref(), log.gps_home.as_ref()];
    for table in tables.into_iter().flatten() {
        for (column, name) in table.columns.iter().enumerate() {
            let Some(m) = profile.map_aux_field(&table.kind, name) else {
                continue;
            };
            // a main-frame field of the same identity stays
            if channels.contains(&m.id) {
                continue;
            }
            channels.insert(Channel {
                data: held(table, column, log.len(), &m),
                id: m.id,
                name: name.clone(),
                unit: m.unit,
                origin: Origin::Logged {
                    field: name.clone(),
                },
            });
        }
    }
}

/// What the parser read across or gave up on, in words for the session's
/// diagnostics: gaps no event announced, holes in the storage, trailing
/// bytes.
fn damage_notes(stats: &ParseStats) -> Vec<String> {
    let mut notes = Vec::new();
    let (holes, gaps): (Vec<&StreamGap>, Vec<&StreamGap>) =
        stats.resumed_gaps.iter().partition(|g| g.hole_bytes > 0);
    if let Some(longest) = gaps.iter().max_by_key(|g| g.time_us) {
        notes.push(format!(
            "the log goes on after {} gap(s) that no logging-resume event announces (the \
             longest: {:.1} s, {} loop iterations): frames were lost while logging; the frames \
             after each gap are read",
            gaps.len(),
            longest.time_us as f64 / 1.0e6,
            longest.iterations
        ));
    }
    if !holes.is_empty() {
        notes.push(format!(
            "{} stretch(es) of bytes that are not blackbox data lie inside the log ({} bytes in \
             all, {:.1} s of the flight: unwritten or overwritten storage); the frames after \
             each are read",
            holes.len(),
            holes.iter().map(|g| g.hole_bytes).sum::<u64>(),
            holes.iter().map(|g| g.time_us).sum::<i64>() as f64 / 1.0e6
        ));
    }
    if stats.trailing_bytes > 0 {
        notes.push(format!(
            "{} trailing bytes after the last valid frame are not blackbox data (stale card \
             sectors or another file's remains); decoding stopped there",
            stats.trailing_bytes
        ));
    }
    notes
}

/// One column of a table of frames logged now and then as a series over
/// the `len` main frames: sample `i` has the value of the last row logged at
/// or before main frame `i` (a row counts from the main frame that follows
/// it, as the forward-filled slow fields of the raw CSV do), and no value
/// before the first row.
fn held(table: &AuxTable, column: usize, len: usize, mapping: &FieldMapping) -> Series {
    let mut rows = table.rows.iter().peekable();
    let mut current = None;
    Series::from_options((0..len).map(|i| {
        while let Some(row) = rows.next_if(|r| r.sample_index <= i) {
            current = row.values.get(column).map(|v| mapping.apply(*v));
        }
        current
    }))
}

/// The mode changes of the slow frames' `flightModeFlags`, each word turned
/// into modes by the firmware profile; empty when the log has no such field.
fn mode_changes(profile: &dyn BoundProfile, slow: &AuxTable) -> Vec<ModeChange> {
    let Some(col) = slow.columns.iter().position(|c| c == "flightModeFlags") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in &slow.rows {
        if let Some(word) = row.values.get(col) {
            push_change(&mut out, row.sample_index, profile.flight_modes(*word));
        }
    }
    out
}
