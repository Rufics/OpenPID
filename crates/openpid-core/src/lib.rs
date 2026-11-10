//! `OpenPID` core: the firmware-agnostic domain model and the extension points
//! (`LogFormat`, `FirmwareProfile`, `Analysis`) that every other crate builds on.
//!
//! This crate deliberately has no knowledge of any concrete log format or
//! flight-controller firmware. Those live in `openpid-fmt-*` crates and plug in
//! through the traits defined in [`format`](mod@format) and [`profile`](mod@profile).

pub mod analysis;
pub mod channel;
pub mod format;
pub mod modes;
pub mod profile;
pub mod recommend;
pub mod series;
pub mod session;
pub mod settings;
pub mod sliders;
pub mod summary;
pub mod units;

pub use channel::{Axis, BatteryField, ChannelId, GyroStage, PidTerm};
pub use format::{Confidence, FormatError, FormatRegistry, LogFormat, OpenOptions};
pub use modes::{FlightMode, ModeChange};
pub use profile::{FieldMapping, FirmwareProfile, ProfileRegistry, RawHeaders};
pub use recommend::{ExportError, Exporter, HeaderDiff, Proposal, diff_headers, is_setting_header};
pub use series::Series;
pub use session::{
    AuxRow, AuxTable, Channel, ChannelSet, Diagnostics, Event, EventKind, FirmwareInfo,
    FirmwareVendor, FirmwareVersion, FlightSession, FrameStats, Gap, LogFile, Origin, SourceInfo,
    Timebase,
};
pub use settings::Settings;
pub use sliders::{SliderDef, SliderModel, SliderResult, SliderValues};
pub use units::Unit;
