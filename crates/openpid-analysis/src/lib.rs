//! Analyses over a [`openpid_core::FlightSession`]: step response, power
//! spectra, frequency heatmaps and the filter simulation. Each analysis is a pure function of the
//! session, an optional epoch and typed parameters, and returns plain data
//! that any front-end can render.

pub mod epoch;
pub mod filter_sim;
pub mod fitness;
pub mod motor_noise;
pub mod prepare;
pub mod project;
pub mod psd;
pub mod recommend;
pub mod render;
pub mod rpm;
pub mod step;
pub mod step_performance;
pub mod step_windows;
pub mod stft;
pub mod targets;
pub mod throttle;

pub use filter_sim::{
    ChainResponse, FilterFit, FilterOverrides, FilterSimAnalysis, FilterSimOutput, FilterSimParams,
    PartResponse,
};
pub use fitness::{Check, Fitness, Level as FitnessLevel, check_session};
pub use motor_noise::{
    HarmonicNoise, MotorNoise, MotorNoiseAnalysis, MotorNoiseOutput, MotorNoiseParams,
};
pub use openpid_core::analysis::{
    Analysis, AnalysisCtx, AnalysisError, Epoch, Grid2D, Requirement,
};
pub use prepare::{Prepared, prepare};
pub use project::{Project, ProjectSlot};
pub use psd::{PsdAnalysis, PsdOutput, PsdParams};
pub use recommend::{Finding, FindingStatus, Recommend, RecommendOutput, RecommendParams};
pub use rpm::{RpmEstimateOutput, RpmEstimateParams, RpmEstimator};
pub use step::{StepAnalysis, StepMethod, StepMetrics, StepOutput, StepParams, StepSpread};
pub use step_performance::StepPerformance;
pub use stft::{StftAnalysis, StftOutput, StftParams};
pub use targets::{StepTargets, TargetPreset, Verdict, VerdictLevel, verdicts};
pub use throttle::{RpmOverlay, ThrottleHeatmap, ThrottleHeatmapOutput, ThrottleHeatmapParams};
