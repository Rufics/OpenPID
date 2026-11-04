//! Decoder for the binary Blackbox format written by Cleanflight-descended
//! flight controllers (Betaflight, `EmuFlight`, Rotorflight, INAV).
//!
//! The format is documented in Betaflight's "Blackbox Internals". This crate
//! implements it from that description; the official `blackbox_decode` and the
//! `blackbox-log` crate are used only as test oracles.
//!
//! Layers, lowest first:
//! - [`reader`] / [`encoding`]: bytes and bits into integers;
//! - [`headers`] / [`predictor`] / [`parser`]: frames into validated integer rows;
//! - [`session`]: rows into the canonical [`openpid_core::FlightSession`] via a
//!   firmware [`profiles`] profile;
//! - [`BlackboxFormat`]: the [`openpid_core::LogFormat`] entry point.

pub mod encoding;
pub mod headers;
pub mod parser;
pub mod predictor;
pub mod profiles;
pub mod reader;
pub mod session;

pub use headers::{Headers, LOG_START_MARKER};
pub use session::{DecodedLog, build_session, decode};

use openpid_core::session::Diagnostics;
use openpid_core::{
    ChannelSet, Confidence, FlightSession, FormatError, LogFile, LogFormat, OpenOptions,
    ProfileRegistry, SourceInfo, Timebase,
};

/// Byte offsets at which logs start inside a file (an SD-card dump may hold many).
pub fn find_log_starts(data: &[u8]) -> Vec<usize> {
    let m = LOG_START_MARKER;
    let mut starts: Vec<usize> = Vec::new();
    if data.len() >= m.len() {
        let mut i = 0;
        while i + m.len() <= data.len() {
            if data[i] == b'H' && &data[i..i + m.len()] == m {
                starts.push(i);
                i += m.len();
            } else {
                i += 1;
            }
        }
    }
    if starts.is_empty() && data.starts_with(b"H ") {
        starts.push(0);
    }
    starts
}

/// Split a file into `(offset, bytes)` segments, one per log.
pub fn split_logs(data: &[u8]) -> Vec<(usize, &[u8])> {
    let starts = find_log_starts(data);
    starts
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let end = starts.get(i + 1).copied().unwrap_or(data.len());
            (s, &data[s..end])
        })
        .collect()
}

/// [`LogFormat`] implementation for binary Blackbox files.
#[derive(Debug)]
pub struct BlackboxFormat {
    profiles: ProfileRegistry,
}

impl Default for BlackboxFormat {
    fn default() -> Self {
        BlackboxFormat {
            profiles: profiles::builtin(),
        }
    }
}

impl BlackboxFormat {
    /// Format with the built-in firmware profiles.
    pub fn new() -> Self {
        Self::default()
    }

    /// Format with a custom profile registry.
    pub fn with_profiles(profiles: ProfileRegistry) -> Self {
        BlackboxFormat { profiles }
    }

    /// The profile registry in use.
    pub fn profiles(&self) -> &ProfileRegistry {
        &self.profiles
    }
}

impl LogFormat for BlackboxFormat {
    fn id(&self) -> &'static str {
        "blackbox-binary"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["bbl", "bfl", "txt"]
    }

    fn sniff(&self, head: &[u8], _file_name: Option<&str>) -> Confidence {
        if !find_log_starts(head).is_empty()
            && head
                .windows(LOG_START_MARKER.len())
                .any(|w| w == LOG_START_MARKER)
        {
            Confidence::HIGH
        } else if head.starts_with(b"H Data version") || head.starts_with(b"H Field I name") {
            Confidence::MEDIUM
        } else {
            Confidence::NONE
        }
    }

    fn open(&self, data: &[u8], options: &OpenOptions) -> Result<LogFile, FormatError> {
        let segments = split_logs(data);
        if segments.is_empty() {
            return Err(FormatError::Unrecognized);
        }
        let mut sessions = Vec::new();
        let mut errors = Vec::new();
        for (index, (offset, bytes)) in segments.into_iter().enumerate() {
            if let Some(wanted) = &options.sessions
                && !wanted.contains(&index)
            {
                continue;
            }
            match decode(bytes) {
                Ok(log) => sessions.push(build_session(
                    &log,
                    index,
                    offset as u64,
                    &self.profiles,
                    options.raw,
                )),
                Err(e) => {
                    errors.push((offset, e.to_string()));
                    sessions.push(FlightSession {
                        index,
                        offset_bytes: offset as u64,
                        firmware: openpid_core::FirmwareInfo::unknown(),
                        profile_id: "none".into(),
                        settings: openpid_core::Settings::default(),
                        raw_headers: Vec::new(),
                        timebase: Timebase::default(),
                        channels: ChannelSet::new(),
                        events: Vec::new(),
                        aux: Vec::new(),
                        diagnostics: Diagnostics {
                            total_bytes: bytes.len() as u64,
                            notes: vec![format!("undecodable log: {e}")],
                            ..Default::default()
                        },
                    });
                }
            }
        }
        if sessions.iter().all(FlightSession::is_empty) && !errors.is_empty() {
            let (offset, message) = errors.remove(0);
            return Err(FormatError::Malformed { offset, message });
        }
        Ok(LogFile {
            source: SourceInfo {
                path: None,
                size_bytes: data.len() as u64,
                format_id: self.id().to_string(),
            },
            sessions,
        })
    }
}

/// Slider ("simplified tuning") model for a decoded session, if its firmware
/// profile provides one.
pub fn slider_model_for(session: &FlightSession) -> Option<Box<dyn openpid_core::SliderModel>> {
    let profiles = profiles::builtin();
    let profile = profiles.get(&session.profile_id)?;
    let headers = openpid_core::RawHeaders(session.raw_headers.clone());
    profile.slider_model(&session.firmware, &headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_marker() {
        let mut data = Vec::new();
        data.extend_from_slice(b"junk");
        data.extend_from_slice(LOG_START_MARKER);
        data.extend_from_slice(b"\nH Data version:2\nI...");
        data.extend_from_slice(LOG_START_MARKER);
        data.extend_from_slice(b"\nH Data version:2\n");
        let segs = split_logs(&data);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].0, 4);
        assert!(segs[0].1.starts_with(LOG_START_MARKER));
        assert_eq!(segs[1].0 + segs[1].1.len(), data.len());
        assert_eq!(split_logs(b"nothing here").len(), 0);
        assert_eq!(split_logs(b"H Data version:2\n").len(), 1);
    }
}
