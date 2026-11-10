//! Where the pilot's sticks were at a moment of the log.
//!
//! The format crates map what each firmware logs (Betaflight-family
//! `rcCommand`, `ArduPilot` `RCIN` with its channel map and calibration)
//! onto the [`crate::ChannelId::Stick`] channels in one convention;
//! [`crate::FlightSession::sticks_at`] reads them back as a [`Sticks`].

/// The sticks at one moment: roll, pitch and yaw in −1…1 (right, forward,
/// right positive), throttle in 0…1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    /// Roll stick, −1 (full left) … 1 (full right).
    pub roll: f64,
    /// Pitch stick, −1 (full back) … 1 (full forward).
    pub pitch: f64,
    /// Yaw stick, −1 (full left) … 1 (full right).
    pub yaw: f64,
    /// Throttle stick, 0 … 1; `None` when the log does not have it.
    pub throttle: Option<f64>,
}
