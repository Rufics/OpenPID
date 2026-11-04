//! `OpenPID` core: the firmware-agnostic domain model and the extension points
//! (`LogFormat`, `FirmwareProfile`, `Analysis`) that every other crate builds on.
//!
//! This crate deliberately has no knowledge of any concrete log format or
//! flight-controller firmware. Those live in `openpid-fmt-*` crates and plug in
//! through the traits defined in [`format`] and [`profile`].

pub mod analysis;
pub mod channel;
pub mod format;
pub mod profile;
pub mod series;
pub mod session;
pub mod settings;
pub mod summary;
pub mod units;

pub use channel::{Axis, BatteryField, ChannelId, GyroStage, PidTerm};
pub use format::{Confidence, FormatError, FormatRegistry, LogFormat, OpenOptions};
pub use profile::{FieldMapping, FirmwareProfile, ProfileRegistry, RawHeaders};
pub use series::Series;
pub use session::{
    AuxRow, AuxTable, Channel, ChannelSet, Diagnostics, Event, EventKind, FirmwareInfo,
    FirmwareVendor, FirmwareVersion, FlightSession, FrameStats, Gap, LogFile, Origin, SourceInfo,
    Timebase,
};
pub use settings::Settings;
pub use units::Unit;
