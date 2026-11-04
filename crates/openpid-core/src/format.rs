//! Log-format extension point.

use crate::session::LogFile;
use std::fmt;

/// How confident a format is that it can read a given file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Confidence(pub u8);

impl Confidence {
    pub const NONE: Confidence = Confidence(0);
    pub const LOW: Confidence = Confidence(30);
    pub const MEDIUM: Confidence = Confidence(60);
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
pub enum FormatError {
    #[error("file is not in a recognised log format")]
    Unrecognized,
    #[error("malformed log at byte {offset}: {message}")]
    Malformed { offset: usize, message: String },
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("I/O error: {0}")]
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, format: Box<dyn LogFormat>) -> &mut Self {
        self.formats.push(format);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn LogFormat> {
        self.formats.iter().map(AsRef::as_ref)
    }

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
}
