//! Analysis extension point and shared result types.
//!
//! Concrete analyses (step response, spectra, ...) live in `openpid-analysis`.

use crate::channel::ChannelId;
use crate::session::FlightSession;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// Regular 2-D grid of values, e.g. a frequency × throttle heatmap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grid2D {
    /// Column coordinates.
    pub x: Vec<f64>,
    /// Row coordinates.
    pub y: Vec<f64>,
    /// Row-major: `z[iy * x.len() + ix]`.
    pub z: Vec<f64>,
    /// Axis label for `x`.
    pub x_label: String,
    /// Axis label for `y`.
    pub y_label: String,
    /// Label (with unit) for `z`.
    pub z_label: String,
}

impl Grid2D {
    /// Value at column `ix`, row `iy`.
    ///
    /// # Panics
    /// Panics if the indices are out of range.
    pub fn get(&self, ix: usize, iy: usize) -> f64 {
        self.z[iy * self.x.len() + ix]
    }
}

/// Time window of a session selected for analysis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Epoch {
    /// Start, seconds from the first sample of the session.
    pub start_s: f64,
    /// End, seconds from the first sample of the session.
    pub end_s: f64,
}

/// What an analysis needs from the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    /// A specific channel must exist.
    Channel(ChannelId),
    /// At least one of these channels must exist.
    AnyOf(Vec<ChannelId>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
/// Why an analysis could not run.
pub enum AnalysisError {
    #[error("missing channel: {0}")]
    /// A required channel is not in the session.
    MissingChannel(ChannelId),
    #[error("channel {0} has unit {1}, expected {2}")]
    /// A channel exists but carries another unit (channel, actual, expected).
    WrongUnit(ChannelId, String, String),
    #[error("not enough data: {0}")]
    /// The epoch or the log is too short for the requested parameters.
    NotEnoughData(String),
    #[error("invalid parameters: {0}")]
    /// Parameters are inconsistent or out of range.
    InvalidParams(String),
}

#[derive(Debug, Clone, Copy)]
/// What an analysis runs on.
pub struct AnalysisCtx<'a> {
    /// The session being analysed.
    pub session: &'a FlightSession,
    /// `None` means the whole session.
    pub epoch: Option<Epoch>,
}

/// A pure computation over a session. Outputs are plain data that any
/// front-end can render.
pub trait Analysis: Send + Sync {
    /// Tunable parameters; hashed to key the result cache.
    type Params: Clone + Hash + Default + Serialize;
    /// Plain-data result, serialisable for the CLI and project files.
    type Output: Serialize;

    /// Stable identifier, e.g. `step-response`.
    fn id(&self) -> &'static str;
    /// Channels the analysis needs from the session.
    fn requires(&self) -> Vec<Requirement>;
    /// Run the analysis over the context's epoch.
    fn run(
        &self,
        ctx: &AnalysisCtx<'_>,
        params: &Self::Params,
    ) -> Result<Self::Output, AnalysisError>;
}
