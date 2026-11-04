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

mod profile;
pub mod reader;
mod session;

pub use profile::{ArdupilotProfile, Vehicle};
pub use reader::{FormatDef, Message, RawLog, Value, read};

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
        let raw = read(data)?;
        let session = session::build(&raw, data.len() as u64, options, self.profile)?;
        let sessions = match &options.sessions {
            Some(wanted) if !wanted.contains(&0) => Vec::new(),
            _ => vec![session],
        };
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
