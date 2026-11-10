//! `ArduPilot` `DataFlash` (`.bin`) logs for `OpenPID`.
//!
//! The format is self-describing: `FMT` packets define every other packet
//! type, `FMTU`/`UNIT`/`MULT` add units and scale factors, `PARM` carries the
//! parameters. [`reader`] decodes the packets without interpreting them;
//! the session builder puts the rate-loop messages (`IMU`, `RATE`, `PID*`, `RCOU`,
//! `RCIN`, `CTUN`, `ATT`, `BAT`/`CURR`, `ESC`) on one timebase and
//! [`ArdupilotProfile`] maps them to `OpenPID` channels and settings.
//!
//! Vehicles: Copter (and Heli) and Plane (including `QuadPlane` in fixed-wing
//! mode) get gyro, setpoint, PID terms, motors/servos and throttle; other
//! vehicles get the gyro and the generic channels. Documentation of the
//! mapping: `docs/ARDUPILOT.md`.

pub mod batch;
mod profile;
pub mod reader;
mod session;

pub use profile::{ArdupilotProfile, Vehicle};
pub use reader::{FormatDef, Message, RawLog, Value, log_starts, read};

use openpid_core::session::SourceInfo;
use openpid_core::{Confidence, FormatError, LogFile, LogFormat, OpenOptions};

/// The `DataFlash` format plugin.
#[derive(Debug, Default, Clone, Copy)]
pub struct ArdupilotFormat {
    profile: ArdupilotProfile,
}

impl ArdupilotFormat {
    /// A format with the built-in `ArduPilot` profile.
    pub fn new() -> Self {
        Self::default()
    }
}

impl LogFormat for ArdupilotFormat {
    fn id(&self) -> &'static str {
        "ardupilot-dataflash"
    }

    fn profile(&self, id: &str) -> Option<&dyn openpid_core::FirmwareProfile> {
        use openpid_core::FirmwareProfile as _;
        (id == self.profile.id()).then_some(&self.profile as &dyn openpid_core::FirmwareProfile)
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["bin"]
    }

    fn sniff(&self, head: &[u8], _file_name: Option<&str>) -> Confidence {
        if reader::looks_like_dataflash(head) {
            Confidence::HIGH
        } else {
            Confidence::NONE
        }
    }

    fn open(&self, data: &[u8], options: &OpenOptions) -> Result<LogFile, FormatError> {
        // one session per log header: a logger restart (or two concatenated
        // files) re-emits the FMT block, starting with FMT's own definition
        let mut starts = log_starts(data);
        if starts.first() != Some(&0) {
            starts.insert(0, 0);
        }
        let mut sessions = Vec::new();
        let mut first_error = None;
        for (index, &start) in starts.iter().enumerate() {
            if options
                .sessions
                .as_ref()
                .is_some_and(|wanted| !wanted.contains(&index))
            {
                continue;
            }
            let end = starts.get(index + 1).copied().unwrap_or(data.len());
            let segment = &data[start..end];
            match read(segment)
                .and_then(|raw| session::build(&raw, segment.len() as u64, options, self.profile))
            {
                Ok(mut session) => {
                    session.index = index;
                    session.offset_bytes = start as u64;
                    sessions.push(session);
                }
                Err(e) => {
                    // a segment without rate-loop data (a header-only restart)
                    // is skipped; the error surfaces only if no segment decodes
                    first_error.get_or_insert(e);
                }
            }
        }
        if sessions.is_empty()
            && let Some(e) = first_error
            && options.sessions.is_none()
        {
            return Err(e);
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
