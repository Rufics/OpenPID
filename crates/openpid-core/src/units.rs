//! Physical units carried by every channel.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unit of measurement of a [`crate::Channel`].
///
/// Analyses check the unit of their inputs and refuse to run on unexpected
/// units instead of silently converting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Raw integer value exactly as logged, meaning defined by the firmware.
    Raw,
    /// Dimensionless number (ratios, counters).
    Unitless,
    /// Time in microseconds.
    Microseconds,
    /// Time in seconds.
    Seconds,
    /// Angular rate.
    DegreesPerSecond,
    /// Angle.
    Degrees,
    /// Fraction 0..100.
    Percent,
    /// Revolutions per minute.
    Rpm,
    /// Electric potential.
    Volts,
    /// Electric current.
    Amperes,
    /// Charge.
    MilliampHours,
    /// Length in metres.
    Meters,
    /// Length in centimetres.
    Centimeters,
    /// Speed.
    MetersPerSecond,
    /// Acceleration in multiples of standard gravity.
    Gs,
    /// Bit flags, interpret through the firmware profile.
    Flags,
    /// Unit the firmware profile could not classify; the string is free-form.
    Custom(String),
}

impl Unit {
    /// Short symbol suitable for axis labels.
    pub fn symbol(&self) -> &str {
        match self {
            Unit::Raw => "raw",
            Unit::Unitless => "",
            Unit::Microseconds => "us",
            Unit::Seconds => "s",
            Unit::DegreesPerSecond => "deg/s",
            Unit::Degrees => "deg",
            Unit::Percent => "%",
            Unit::Rpm => "rpm",
            Unit::Volts => "V",
            Unit::Amperes => "A",
            Unit::MilliampHours => "mAh",
            Unit::Meters => "m",
            Unit::Centimeters => "cm",
            Unit::MetersPerSecond => "m/s",
            Unit::Gs => "g",
            Unit::Flags => "flags",
            Unit::Custom(s) => s.as_str(),
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}
