//! Calibration probes: synthetic flights whose content is known exactly,
//! written as flight-controller logs, so that what any tool shows for them
//! can be checked against the truth (`docs/calibration`, epic #166).
//!
//! - [`signal`]: the building blocks of a probe flight (tones, noise, stick
//!   steps through a plant with a closed-form response) and the flight
//!   itself, as a session the log writer of `openpid-fmt-blackbox` accepts.
//! - [`catalogue`]: the probes of every round, as data: one log per probe,
//!   one experiment per axis.
//! - [`pack`]: a round as files: the logs, `manifest.json` with what is true
//!   about each, `ours.json` with what `OpenPID` computes for the same logs.
//! - [`capture`]: the steps of the script that walks the reference tool
//!   through a round and saves its screens; the script travels in the pack.
//! - [`cli`]: the command line of the `openpid-probe` binary.

pub mod capture;
pub mod catalogue;
pub mod cli;
pub mod pack;
pub mod signal;

pub use catalogue::{Area, Probe};
pub use pack::{Manifest, Ours, Pack, PackError};
pub use signal::{AxisSamples, AxisSpec, Filtered, FlightSpec, Plant, Stick, Throttle, Wave};
