//! Firmware-profile extension point: how to interpret a specific firmware's
//! field names, scales and settings.

use crate::channel::ChannelId;
use crate::session::{FirmwareInfo, FlightSession};
use crate::settings::Settings;
use crate::units::Unit;
use std::fmt;

/// Headers as logged, in order; multiple headers may share a name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawHeaders(pub Vec<(String, String)>);

impl RawHeaders {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// First header among `names` that exists.
    pub fn first_of(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|n| self.get(n))
    }

    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get(name)?.trim().parse().ok()
    }

    pub fn get_i64(&self, name: &str) -> Option<i64> {
        self.get(name)?.trim().parse().ok()
    }

    /// Comma-separated numeric list.
    pub fn get_list(&self, name: &str) -> Option<Vec<f64>> {
        self.get(name)?
            .split(',')
            .map(|p| p.trim().parse::<f64>().ok())
            .collect()
    }
}

/// How one logged field maps onto a channel: `value = (raw + offset) * scale`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldMapping {
    pub id: ChannelId,
    pub unit: Unit,
    pub scale: f64,
    pub offset: f64,
}

impl FieldMapping {
    pub fn new(id: ChannelId, unit: Unit) -> Self {
        FieldMapping {
            id,
            unit,
            scale: 1.0,
            offset: 0.0,
        }
    }

    pub fn scaled(id: ChannelId, unit: Unit, scale: f64) -> Self {
        FieldMapping {
            id,
            unit,
            scale,
            offset: 0.0,
        }
    }

    pub fn apply(&self, raw: i64) -> f64 {
        (raw as f64 + self.offset) * self.scale
    }
}

/// Knowledge about one firmware family (and its versions).
pub trait FirmwareProfile: Send + Sync {
    /// Stable identifier, e.g. `betaflight`.
    fn id(&self) -> &'static str;

    /// How well this profile fits the log; `None` when it does not apply.
    fn score(&self, firmware: &FirmwareInfo, headers: &RawHeaders) -> Option<u8>;

    /// Map a logged field name onto a channel. `None` leaves it as a raw
    /// `Custom` channel.
    fn map_field(
        &self,
        field: &str,
        firmware: &FirmwareInfo,
        headers: &RawHeaders,
    ) -> Option<FieldMapping>;

    /// Extract typed settings from the headers.
    fn settings(&self, firmware: &FirmwareInfo, headers: &RawHeaders) -> Settings;

    /// Add derived channels (throttle percent, PID error, ...). Called after
    /// all logged channels are in place.
    fn derive(&self, _session: &mut FlightSession) {}
}

impl fmt::Debug for dyn FirmwareProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FirmwareProfile({})", self.id())
    }
}

#[derive(Debug, Default)]
pub struct ProfileRegistry {
    profiles: Vec<Box<dyn FirmwareProfile>>,
}

impl ProfileRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, profile: Box<dyn FirmwareProfile>) -> &mut Self {
        self.profiles.push(profile);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn FirmwareProfile> {
        self.profiles.iter().map(AsRef::as_ref)
    }

    pub fn get(&self, id: &str) -> Option<&dyn FirmwareProfile> {
        self.iter().find(|p| p.id() == id)
    }

    /// Best-scoring profile for a log.
    pub fn select(
        &self,
        firmware: &FirmwareInfo,
        headers: &RawHeaders,
    ) -> Option<&dyn FirmwareProfile> {
        self.iter()
            .filter_map(|p| p.score(firmware, headers).map(|s| (s, p)))
            .max_by_key(|(s, _)| *s)
            .map(|(_, p)| p)
    }
}
