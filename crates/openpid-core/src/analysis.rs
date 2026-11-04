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
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Row-major: `z[iy * x.len() + ix]`.
    pub z: Vec<f64>,
    pub x_label: String,
    pub y_label: String,
    pub z_label: String,
}

impl Grid2D {
    pub fn get(&self, ix: usize, iy: usize) -> f64 {
        self.z[iy * self.x.len() + ix]
    }
}

/// Time window of a session selected for analysis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Epoch {
    pub start_s: f64,
    pub end_s: f64,
}

/// What an analysis needs from the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    Channel(ChannelId),
    AnyOf(Vec<ChannelId>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisError {
    #[error("missing channel: {0}")]
    MissingChannel(ChannelId),
    #[error("channel {0} has unit {1}, expected {2}")]
    WrongUnit(ChannelId, String, String),
    #[error("not enough data: {0}")]
    NotEnoughData(String),
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
}

#[derive(Debug, Clone, Copy)]
pub struct AnalysisCtx<'a> {
    pub session: &'a FlightSession,
    /// `None` means the whole session.
    pub epoch: Option<Epoch>,
}

/// A pure computation over a session. Outputs are plain data that any
/// front-end can render.
pub trait Analysis: Send + Sync {
    type Params: Clone + Hash + Default + Serialize;
    type Output: Serialize;

    fn id(&self) -> &'static str;
    fn requires(&self) -> Vec<Requirement>;
    fn run(
        &self,
        ctx: &AnalysisCtx<'_>,
        params: &Self::Params,
    ) -> Result<Self::Output, AnalysisError>;
}
