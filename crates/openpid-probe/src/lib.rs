//! Calibration probes: synthetic flights whose content is known exactly,
//! written as flight-controller logs, so that what any tool shows for them
//! can be checked against the truth (`docs/calibration`, epic #166).
//!
//! - [`signal`]: the building blocks of a probe flight (tones, noise, stick
//!   steps through a plant with a closed-form response) and the flight
//!   itself, as a session the log writer of `openpid-fmt-blackbox` accepts.
//! - [`catalogue`]: the probes of every round, as data: one log per probe,
//!   one experiment per axis. Rounds 1 to 3 are about the spectrum, 4 to 6
//!   about the step response; the last of each (3 and 6) is a hold-out,
//!   flights the model of the reference tool was not fitted to.
//! - [`pack`]: a round as files: the logs, `manifest.json` with what is true
//!   about each, `ours.json` with what `OpenPID` computes for the same logs,
//!   `predictions.json` with what the model expects the reference tool to
//!   show.
//! - [`capture`]: the steps of the script that walks the reference tool
//!   through a round and saves its screens; the script travels in the pack.
//! - [`model`]: the spectrum curve of the reference tool as the rounds have
//!   identified it, for the predictions of the next round. It was held to
//!   the tool's own copies of its plots during the rounds, a one-off
//!   comparison whose material is not in the repository.
//! - [`step_model`]: the step response of the reference tool, likewise.
//! - [`cli`]: the command line of the `openpid-probe` binary.

pub mod capture;
pub mod catalogue;
pub mod cli;
pub mod model;
pub mod pack;
pub mod signal;
pub mod step_model;

pub use catalogue::{Area, Probe};
pub use pack::{Manifest, Ours, Pack, PackError};
pub use signal::{AxisSamples, AxisSpec, Filtered, FlightSpec, Plant, Stick, Throttle, Wave};
