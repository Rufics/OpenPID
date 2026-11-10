//! Log-format extension point.

use crate::profile::{FirmwareProfile, RawHeaders};
use crate::recommend::Exporter;
use crate::session::{FlightSession, LogFile};
use crate::sliders::SliderModel;
use std::fmt;

/// How confident a format is that it can read a given file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Confidence(pub u8);

impl Confidence {
    /// The format cannot read this file.
    pub const NONE: Confidence = Confidence(0);
    /// Weak hint only (e.g. file extension).
    pub const LOW: Confidence = Confidence(30);
    /// Header structure recognised.
    pub const MEDIUM: Confidence = Confidence(60);
    /// Signature found; this is the format.
    pub const HIGH: Confidence = Confidence(100);
}

/// Options for [`LogFormat::open`].
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// Only decode these session indices (all when `None`).
    pub sessions: Option<Vec<usize>>,
    /// Skip firmware-profile scaling and derived channels; every logged field
    /// becomes a `Custom` channel with `Unit::Raw`.
    pub raw: bool,
}

#[derive(Debug, thiserror::Error)]
/// Why a file could not be opened as a log.
pub enum FormatError {
    #[error("file is not in a recognised log format")]
    /// No registered format recognises the data.
    Unrecognized,
    #[error("malformed log at byte {offset}: {message}")]
    /// The data is recognised but corrupt at `offset`.
    Malformed {
        /// Byte offset of the problem.
        offset: usize,
        /// What was wrong.
        message: String,
    },
    #[error("unsupported: {0}")]
    /// A feature of the file the format does not implement yet.
    Unsupported(String),
    #[error("I/O error: {0}")]
    /// Reading the file failed.
    Io(#[from] std::io::Error),
}

/// A file format that can be turned into a [`LogFile`].
///
/// Implementations live in `openpid-fmt-*` crates. They are stateless and
/// shareable across threads.
pub trait LogFormat: Send + Sync {
    /// Stable identifier, e.g. `blackbox-binary`.
    fn id(&self) -> &'static str;

    /// Typical file extensions, lower-case, without dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Cheap check on the first bytes of a file (and optionally its name).
    fn sniff(&self, head: &[u8], file_name: Option<&str>) -> Confidence;

    /// Decode the whole file.
    fn open(&self, data: &[u8], options: &OpenOptions) -> Result<LogFile, FormatError>;

    /// The firmware profile this format names `id` in
    /// [`FlightSession::profile_id`], when it has one.
    fn profile(&self, _id: &str) -> Option<&dyn FirmwareProfile> {
        None
    }
}

impl fmt::Debug for dyn LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LogFormat({})", self.id())
    }
}

/// Registry of known formats with auto-detection.
#[derive(Debug, Default)]
pub struct FormatRegistry {
    formats: Vec<Box<dyn LogFormat>>,
}

impl FormatRegistry {
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a format; later registrations win ties in [`FormatRegistry::detect`].
    pub fn register(&mut self, format: Box<dyn LogFormat>) -> &mut Self {
        self.formats.push(format);
        self
    }

    /// All registered formats in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn LogFormat> {
        self.formats.iter().map(AsRef::as_ref)
    }

    /// Format by its [`LogFormat::id`].
    pub fn get(&self, id: &str) -> Option<&dyn LogFormat> {
        self.iter().find(|f| f.id() == id)
    }

    /// Pick the format with the highest sniff confidence.
    pub fn detect(&self, data: &[u8], file_name: Option<&str>) -> Option<&dyn LogFormat> {
        let head = &data[..data.len().min(64 * 1024)];
        self.iter()
            .map(|f| (f.sniff(head, file_name), f))
            .filter(|(c, _)| *c > Confidence::NONE)
            .max_by_key(|(c, _)| *c)
            .map(|(_, f)| f)
    }

    /// Detect and open in one step.
    pub fn open(
        &self,
        data: &[u8],
        file_name: Option<&str>,
        options: &OpenOptions,
    ) -> Result<LogFile, FormatError> {
        let format = self
            .detect(data, file_name)
            .ok_or(FormatError::Unrecognized)?;
        format.open(data, options)
    }

    /// The firmware profile that built `session`.
    pub fn profile_for(&self, session: &FlightSession) -> Option<&dyn FirmwareProfile> {
        self.iter().find_map(|f| f.profile(&session.profile_id))
    }

    /// The slider ("simplified tuning") model of `session`, when its firmware
    /// has one.
    pub fn slider_model(&self, session: &FlightSession) -> Option<Box<dyn SliderModel>> {
        let headers = RawHeaders(session.raw_headers.clone());
        self.profile_for(session)?
            .bind(&session.firmware, &headers)
            .slider_model()
    }

    /// What writes proposals and setting diffs for the firmware of `session`,
    /// when its profile has one.
    pub fn exporter(&self, session: &FlightSession) -> Option<Box<dyn Exporter>> {
        let headers = RawHeaders(session.raw_headers.clone());
        self.profile_for(session)?
            .bind(&session.firmware, &headers)
            .exporter()
    }
}
