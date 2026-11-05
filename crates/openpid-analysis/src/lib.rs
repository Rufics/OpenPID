//! Analyses over a [`openpid_core::FlightSession`]: step response, power
//! spectra, frequency heatmaps and the filter simulation. Each analysis is a pure function of the
//! session, an optional epoch and typed parameters, and returns plain data
//! that any front-end can render.

pub mod filter_sim;
pub mod prepare;
pub mod psd;
pub mod step;
pub mod stft;
pub mod throttle;

pub use filter_sim::{
    FilterFit, FilterOverrides, FilterSimAnalysis, FilterSimOutput, FilterSimParams,
};
pub use openpid_core::analysis::{
    Analysis, AnalysisCtx, AnalysisError, Epoch, Grid2D, Requirement,
};
pub use prepare::{Prepared, prepare};
pub use psd::{PsdAnalysis, PsdOutput, PsdParams};
pub use step::{StepAnalysis, StepMetrics, StepOutput, StepParams};
pub use stft::{StftAnalysis, StftOutput, StftParams};
pub use throttle::{ThrottleHeatmap, ThrottleHeatmapOutput, ThrottleHeatmapParams};
