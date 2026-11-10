//! Models of the filters a flight controller runs on its gyro and D-term,
//! as `f64` state machines that can be stepped sample by sample (`update`)
//! and evaluated analytically (`response`), so that a simulated chain can be
//! compared both with the logged filtered gyro and with closed-form curves.
//!
//! The discrete forms follow the firmware (ADR-0012): PT1 with the
//! `dt / (RC + dt)` gain, PT2/PT3 as cascaded PT1 stages with the cutoff
//! correction that keeps the cascade at −3 dB at the requested cutoff, and
//! biquads from the RBJ cookbook with Betaflight's notch-Q convention. No
//! firmware code is copied; everything is implemented from the formulas.

pub mod filter;
pub mod model;
pub mod signal;

pub use filter::{Biquad, Chain, Filter, Pt1, Ptn, Stage, notch_q, pt_cutoff_correction};
pub use model::{
    ChainKind, ChainModel, ChainPart, Drive, DynLowpass, DynNotchModel, LowpassModel,
    OperatingPoint, PartGroup, RpmNotchModel, track_peaks,
};
pub use openpid_dsp::Complex;
pub use signal::{
    Dropout, MotorNoise, Noise, Plant, SetpointProfile, SignalSpec, ThrottleProfile, Tone,
};
