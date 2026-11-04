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
    pub path: Option<PathBuf>,
    pub size_bytes: u64,
    /// Identifier of the [`crate::LogFormat`] that produced the file.
    pub format_id: String,
}

/// One file on disk, which may contain several flights.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogFile {
    pub source: SourceInfo,
    pub sessions: Vec<FlightSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareVendor {
    Betaflight,
    Emuflight,
    Rotorflight,
    Inav,
    Cleanflight,
    Baseflight,
    Ardupilot,
    Unknown(String),
}

impl FirmwareVendor {
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
pub struct FirmwareVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl FirmwareVersion {
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
pub struct FirmwareInfo {
    pub vendor: FirmwareVendor,
    pub version: Option<FirmwareVersion>,
    /// Full revision string as logged (e.g. `Betaflight 4.2.11 (948ba6339) STM32F7X2`).
    pub revision: String,
    pub target: Option<String>,
    pub build_date: Option<String>,
}

impl FirmwareInfo {
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

    pub fn len(&self) -> usize {
        self.time_us.len()
    }

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

    pub fn duration_s(&self) -> f64 {
        match (self.time_us.first(), self.time_us.last()) {
            (Some(a), Some(b)) => (b - a) as f64 / 1.0e6,
            _ => 0.0,
        }
    }

    pub fn start_us(&self) -> Option<i64> {
        self.time_us.first().copied()
    }
}

/// How a channel came to exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Decoded straight from the log; `field` is the logged field name.
    Logged { field: String },
    /// Computed from other channels by the named step.
    Derived { from: Vec<ChannelId>, by: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    /// Human-readable name (logged field name for logged channels).
    pub name: String,
    pub unit: Unit,
    pub data: Series,
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

    pub fn get(&self, id: &ChannelId) -> Option<&Channel> {
        self.index.get(id).map(|&i| &self.channels[i])
    }

    pub fn contains(&self, id: &ChannelId) -> bool {
        self.index.contains_key(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Channel> {
        self.channels.iter()
    }

    pub fn len(&self) -> usize {
        self.channels.len()
    }

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
    Int(i32),
    Float(f32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Arming beep; `time_us` is the firmware timestamp of the beep.
    SyncBeep {
        time_us: i64,
    },
    InflightAdjustment {
        function: u8,
        value: AdjustmentValue,
    },
    LoggingResume {
        iteration: u32,
        time_us: i64,
    },
    Disarm {
        reason: u32,
    },
    FlightMode {
        flags: u32,
        last_flags: u32,
    },
    LogEnd,
    /// Format-specific event the decoder knows but core does not model.
    Other {
        name: String,
        data: BTreeMap<String, i64>,
    },
    Unknown {
        code: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Index into the main-frame timeline at which the event was seen.
    pub sample_index: usize,
    pub kind: EventKind,
}

/// One row of an auxiliary (non-main) table such as slow or GPS frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuxRow {
    /// Index into the main-frame timeline at which the row was logged.
    pub sample_index: usize,
    pub values: Vec<i64>,
}

/// Low-rate data logged alongside the main frames (slow state, GPS, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuxTable {
    /// Decoder-defined kind, e.g. `slow`, `gps`, `gps_home`.
    pub kind: String,
    pub columns: Vec<String>,
    pub rows: Vec<AuxRow>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameStats {
    pub valid: u64,
    pub corrupt: u64,
    pub desync: u64,
    pub bytes: u64,
}

/// What the decoder observed while reading the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Per frame kind (decoder-defined names such as `I`, `P`, `S`).
    pub frames: BTreeMap<String, FrameStats>,
    pub corrupt_frames: u64,
    pub desync_frames: u64,
    /// Bytes skipped because they were not recognised as a frame.
    pub skipped_bytes: u64,
    pub total_bytes: u64,
    /// Logged fields the firmware profile could not classify.
    pub unmapped_fields: Vec<String>,
    pub notes: Vec<String>,
}

/// One flight (one log inside a file) in canonical form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightSession {
    /// Index of the session within its file.
    pub index: usize,
    /// Byte offset of the session within the file.
    pub offset_bytes: u64,
    pub firmware: FirmwareInfo,
    /// Identifier of the firmware profile used to interpret the log.
    pub profile_id: String,
    pub settings: Settings,
    /// Every header as logged, in order.
    pub raw_headers: Vec<(String, String)>,
    pub timebase: Timebase,
    pub channels: ChannelSet,
    pub events: Vec<Event>,
    pub aux: Vec<AuxTable>,
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

    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        self.channels.get(id)
    }

    /// Number of main-frame samples.
    pub fn len(&self) -> usize {
        self.timebase.len()
    }

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
