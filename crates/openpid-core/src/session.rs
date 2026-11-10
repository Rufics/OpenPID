//! The canonical, firmware-independent representation of a decoded log.

use crate::channel::ChannelId;
use crate::series::Series;
use crate::settings::Settings;
use crate::units::Unit;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// Where a [`LogFile`] came from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Path the file was read from, if any.
    pub path: Option<PathBuf>,
    /// File size.
    pub size_bytes: u64,
    /// Identifier of the [`crate::LogFormat`] that produced the file.
    pub format_id: String,
}

/// One file on disk, which may contain several flights.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogFile {
    /// Where the data came from.
    pub source: SourceInfo,
    /// Flights found in the file, in file order.
    pub sessions: Vec<FlightSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Firmware family that wrote the log.
pub enum FirmwareVendor {
    /// Betaflight.
    Betaflight,
    /// `EmuFlight`.
    Emuflight,
    /// Rotorflight.
    Rotorflight,
    /// `INAV`.
    Inav,
    /// Cleanflight.
    Cleanflight,
    /// Baseflight.
    Baseflight,
    /// `ArduPilot`.
    Ardupilot,
    /// Anything else; the string is the name as logged.
    Unknown(String),
}

impl FirmwareVendor {
    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            FirmwareVendor::Betaflight => "Betaflight",
            FirmwareVendor::Emuflight => "EmuFlight",
            FirmwareVendor::Rotorflight => "Rotorflight",
            FirmwareVendor::Inav => "INAV",
            FirmwareVendor::Cleanflight => "Cleanflight",
            FirmwareVendor::Baseflight => "Baseflight",
            FirmwareVendor::Ardupilot => "ArduPilot",
            FirmwareVendor::Unknown(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
/// Semantic version of the firmware (`2025.12.0` for date-based Betaflight releases).
pub struct FirmwareVersion {
    /// Major component (year for date-based versions).
    pub major: u32,
    /// Minor component.
    pub minor: u32,
    /// Patch component.
    pub patch: u32,
}

impl FirmwareVersion {
    /// Build a version from its components.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        FirmwareVersion {
            major,
            minor,
            patch,
        }
    }

    /// Parse `4.2.11`, `4.3.0-RC1`, `v3.0.1`.
    pub fn parse(s: &str) -> Option<FirmwareVersion> {
        let s = s.trim_start_matches(|c: char| !c.is_ascii_digit());
        let core: String = s
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let mut it = core
            .split('.')
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u32>().ok());
        let major = it.next()??;
        let minor = it.next().flatten().unwrap_or(0);
        let patch = it.next().flatten().unwrap_or(0);
        Some(FirmwareVersion {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Identity of the firmware that wrote a log.
pub struct FirmwareInfo {
    /// Firmware family.
    pub vendor: FirmwareVendor,
    /// Parsed version, if the revision string had one.
    pub version: Option<FirmwareVersion>,
    /// Full revision string as logged (e.g. `Betaflight 4.2.11 (948ba6339) STM32F7X2`).
    pub revision: String,
    /// Build target / board, if present in the revision string.
    pub target: Option<String>,
    /// `Firmware date` header, verbatim.
    pub build_date: Option<String>,
}

impl FirmwareInfo {
    /// Placeholder when no firmware information is available.
    pub fn unknown() -> Self {
        FirmwareInfo {
            vendor: FirmwareVendor::Unknown(String::new()),
            version: None,
            revision: String::new(),
            target: None,
            build_date: None,
        }
    }

    /// Parse a Cleanflight-family `Firmware revision` header value.
    pub fn parse_revision(revision: &str) -> FirmwareInfo {
        let mut words = revision.split_whitespace();
        let name = words.next().unwrap_or("");
        let vendor = match name.to_ascii_lowercase().as_str() {
            "betaflight" => FirmwareVendor::Betaflight,
            "emuflight" => FirmwareVendor::Emuflight,
            "rotorflight" => FirmwareVendor::Rotorflight,
            "inav" => FirmwareVendor::Inav,
            "cleanflight" => FirmwareVendor::Cleanflight,
            "baseflight" => FirmwareVendor::Baseflight,
            "" => FirmwareVendor::Unknown(String::new()),
            _ => FirmwareVendor::Unknown(name.to_string()),
        };
        let version = words.next().and_then(FirmwareVersion::parse);
        let rest: Vec<&str> = words.collect();
        let target = rest
            .iter()
            .rev()
            .find(|w| !w.starts_with('('))
            .map(|s| (*s).to_string());
        FirmwareInfo {
            vendor,
            version,
            revision: revision.to_string(),
            target,
            build_date: None,
        }
    }

    /// Whether the version is known and at least `major.minor`.
    pub fn is_at_least(&self, major: u32, minor: u32) -> bool {
        self.version
            .is_some_and(|v| v >= FirmwareVersion::new(major, minor, 0))
    }
}

/// A gap in the sample timeline (missing/dropped frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// Index of the sample *after* the gap.
    pub index: usize,
    /// Time step across the gap in microseconds.
    pub dt_us: i64,
}

/// Timing information shared by all channels of a session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timebase {
    /// Timestamp of every main-frame sample, microseconds, monotonic.
    pub time_us: Vec<i64>,
    /// Expected step between samples from the firmware configuration, if known.
    pub nominal_dt_us: Option<f64>,
    /// Median of the observed steps.
    pub median_dt_us: f64,
    /// Observed steps considered gaps (larger than 1.5x the median).
    pub gaps: Vec<Gap>,
    /// Number of loop iterations the firmware intentionally did not log
    /// (sub-sampling), as reported by the decoder.
    pub intentionally_absent_iterations: u64,
}

impl Timebase {
    /// Compute statistics from a timestamp vector.
    pub fn from_times(time_us: Vec<i64>, nominal_dt_us: Option<f64>) -> Timebase {
        let mut dts: Vec<i64> = time_us.windows(2).map(|w| w[1] - w[0]).collect();
        let median_dt_us = if dts.is_empty() {
            nominal_dt_us.unwrap_or(0.0)
        } else {
            dts.sort_unstable();
            dts[dts.len() / 2] as f64
        };
        let threshold = (median_dt_us * 1.5).max(1.0);
        let mut gaps = Vec::new();
        for (i, w) in time_us.windows(2).enumerate() {
            let dt = w[1] - w[0];
            if dt as f64 > threshold && gaps.len() < 100_000 {
                gaps.push(Gap {
                    index: i + 1,
                    dt_us: dt,
                });
            }
        }
        Timebase {
            time_us,
            nominal_dt_us,
            median_dt_us,
            gaps,
            intentionally_absent_iterations: 0,
        }
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.time_us.len()
    }

    /// Whether there are no samples.
    pub fn is_empty(&self) -> bool {
        self.time_us.is_empty()
    }

    /// Effective sample rate based on the median step.
    pub fn sample_rate_hz(&self) -> f64 {
        if self.median_dt_us > 0.0 {
            1.0e6 / self.median_dt_us
        } else {
            0.0
        }
    }

    /// Span between the first and last sample in seconds.
    pub fn duration_s(&self) -> f64 {
        match (self.time_us.first(), self.time_us.last()) {
            (Some(a), Some(b)) => (b - a) as f64 / 1.0e6,
            _ => 0.0,
        }
    }

    /// Timestamp of the first sample.
    pub fn start_us(&self) -> Option<i64> {
        self.time_us.first().copied()
    }
}

/// How a channel came to exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Decoded straight from the log; `field` is the logged field name.
    /// Decoded straight from the log; `field` is the logged field name.
    Logged {
        /// Logged field name.
        field: String,
    },
    /// Computed from other channels by the named step.
    /// Computed from other channels by the named step.
    Derived {
        /// Inputs of the derivation.
        from: Vec<ChannelId>,
        /// Name of the derivation step.
        by: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// One data series with its identity and unit.
pub struct Channel {
    /// Semantic identity.
    pub id: ChannelId,
    /// Human-readable name (logged field name for logged channels).
    pub name: String,
    /// Unit of `data`.
    pub unit: Unit,
    /// The samples.
    pub data: Series,
    /// Provenance.
    pub origin: Origin,
}

/// Channels of a session, indexed by [`ChannelId`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelSet {
    channels: Vec<Channel>,
    #[serde(skip)]
    index: HashMap<ChannelId, usize>,
}

impl ChannelSet {
    /// Empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a channel.
    pub fn insert(&mut self, channel: Channel) {
        if let Some(&i) = self.index.get(&channel.id) {
            self.channels[i] = channel;
        } else {
            self.index.insert(channel.id.clone(), self.channels.len());
            self.channels.push(channel);
        }
    }

    /// Remove a channel by identity, returning it.
    pub fn remove(&mut self, id: &ChannelId) -> Option<Channel> {
        let i = *self.index.get(id)?;
        let c = self.channels.remove(i);
        self.reindex();
        Some(c)
    }

    /// Channel by identity.
    pub fn get(&self, id: &ChannelId) -> Option<&Channel> {
        self.index.get(id).map(|&i| &self.channels[i])
    }

    /// Whether a channel with this identity exists.
    pub fn contains(&self, id: &ChannelId) -> bool {
        self.index.contains_key(id)
    }

    /// All channels in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Channel> {
        self.channels.iter()
    }

    /// Number of channels.
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// Rebuild the lookup index (needed after deserialization).
    pub fn reindex(&mut self) {
        self.index = self
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.clone(), i))
            .collect();
    }
}

/// Value of an in-flight adjustment event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjustmentValue {
    /// Integer-valued adjustment.
    Int(i32),
    /// Float-valued adjustment.
    Float(f32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Something that happened at a point in time rather than a continuous state.
pub enum EventKind {
    /// Arming beep; `time_us` is the firmware timestamp of the beep.
    SyncBeep {
        /// Firmware timestamp of the beep, microseconds.
        time_us: i64,
    },
    /// The pilot changed a setting in flight.
    InflightAdjustment {
        /// Firmware adjustment function id.
        function: u8,
        /// New value.
        value: AdjustmentValue,
    },
    /// Logging resumed after a pause; counters jump.
    LoggingResume {
        /// Loop iteration at resume.
        iteration: u32,
        /// Timestamp at resume, microseconds.
        time_us: i64,
    },
    /// The craft disarmed.
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
    /// Format-specific event the decoder knows but core does not model.
    Other {
        /// Decoder-defined event name.
        name: String,
        /// Event payload fields.
        data: BTreeMap<String, i64>,
    },
    /// An event code the decoder does not know.
    Unknown {
        /// Raw event code.
        code: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// An event anchored to the main-frame timeline.
pub struct Event {
    /// Index into the main-frame timeline at which the event was seen.
    pub sample_index: usize,
    /// What happened.
    pub kind: EventKind,
}

/// One row of an auxiliary (non-main) table such as slow or GPS frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuxRow {
    /// Index into the main-frame timeline at which the row was logged.
    pub sample_index: usize,
    /// One value per column.
    pub values: Vec<i64>,
}

/// Low-rate data logged alongside the main frames (slow state, GPS, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuxTable {
    /// Decoder-defined kind, e.g. `slow`, `gps`, `gps_home`.
    pub kind: String,
    /// Column names as logged.
    pub columns: Vec<String>,
    /// Rows in log order.
    pub rows: Vec<AuxRow>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Counts for one frame kind.
pub struct FrameStats {
    /// Frames decoded and accepted.
    pub valid: u64,
    /// Frames that failed to decode.
    pub corrupt: u64,
    /// Frames decoded but rejected by validation.
    pub desync: u64,
    /// Bytes of accepted frames.
    pub bytes: u64,
}

/// What the decoder observed while reading the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Per frame kind (decoder-defined names such as `I`, `P`, `S`).
    pub frames: BTreeMap<String, FrameStats>,
    /// Total corrupt frames across kinds.
    pub corrupt_frames: u64,
    /// Total desynchronised frames across kinds.
    pub desync_frames: u64,
    /// Bytes skipped because they were not recognised as a frame.
    pub skipped_bytes: u64,
    /// Bytes of frame data in the session.
    pub total_bytes: u64,
    /// Logged fields the firmware profile could not classify.
    pub unmapped_fields: Vec<String>,
    /// Free-form remarks from the decoder or profile.
    pub notes: Vec<String>,
}

/// One flight (one log inside a file) in canonical form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightSession {
    /// Index of the session within its file.
    pub index: usize,
    /// Byte offset of the session within the file.
    pub offset_bytes: u64,
    /// Firmware that wrote the session.
    pub firmware: FirmwareInfo,
    /// Identifier of the firmware profile used to interpret the log.
    pub profile_id: String,
    /// Typed settings extracted from the headers.
    pub settings: Settings,
    /// Every header as logged, in order.
    pub raw_headers: Vec<(String, String)>,
    /// Timestamps and timing statistics of the main frames.
    pub timebase: Timebase,
    /// All channels.
    pub channels: ChannelSet,
    /// Events in time order.
    pub events: Vec<Event>,
    /// Low-rate tables (slow state, GPS, ...).
    pub aux: Vec<AuxTable>,
    /// Flight-mode changes in sample order, decoded by the format crate
    /// (empty when the log records no modes).
    #[serde(default)]
    pub flight_modes: Vec<crate::modes::ModeChange>,
    /// Decoder observations.
    pub diagnostics: Diagnostics,
}

impl FlightSession {
    /// First raw header with the given name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.raw_headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Channel by identity.
    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        self.channels.get(id)
    }

    /// Number of main-frame samples.
    pub fn len(&self) -> usize {
        self.timebase.len()
    }

    /// Whether the session has no main-frame samples.
    pub fn is_empty(&self) -> bool {
        self.timebase.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_revision() {
        let fw = FirmwareInfo::parse_revision("Betaflight 4.2.11 (948ba6339) STM32F7X2");
        assert_eq!(fw.vendor, FirmwareVendor::Betaflight);
        assert_eq!(fw.version, Some(FirmwareVersion::new(4, 2, 11)));
        assert_eq!(fw.target.as_deref(), Some("STM32F7X2"));
        let fw = FirmwareInfo::parse_revision("Rotorflight 4.3.0-RC1 (19d3380) STM32G47X");
        assert_eq!(fw.vendor, FirmwareVendor::Rotorflight);
        assert_eq!(fw.version, Some(FirmwareVersion::new(4, 3, 0)));
        let fw = FirmwareInfo::parse_revision("EmuFlight 0.4.1 (7bac6ddd0) HELIOSPRING");
        assert_eq!(fw.vendor, FirmwareVendor::Emuflight);
        assert!(fw.is_at_least(0, 4));
        assert!(!fw.is_at_least(1, 0));
        let fw = FirmwareInfo::parse_revision("INAV 3.0.1 (753c4479) MATEKF405SE");
        assert_eq!(fw.vendor, FirmwareVendor::Inav);
        let fw = FirmwareInfo::parse_revision("");
        assert_eq!(fw.vendor, FirmwareVendor::Unknown(String::new()));
    }

    #[test]
    fn timebase_gaps() {
        let t = Timebase::from_times(vec![0, 1000, 2000, 3000, 6000, 7000], Some(1000.0));
        assert_eq!(t.median_dt_us, 1000.0);
        assert_eq!(
            t.gaps,
            vec![Gap {
                index: 4,
                dt_us: 3000
            }]
        );
        assert!((t.sample_rate_hz() - 1000.0).abs() < 1e-9);
        assert!((t.duration_s() - 0.007).abs() < 1e-12);
    }
}
