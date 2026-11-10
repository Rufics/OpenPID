//! Firmware-profile extension point: how to interpret a specific firmware's
//! field names, scales and settings.

use crate::channel::ChannelId;
use crate::modes::FlightMode;
use crate::session::{FirmwareInfo, FlightSession};
use crate::settings::Settings;
use crate::sliders::SliderModel;
use crate::units::Unit;
use std::fmt;

/// Headers as logged, in order; multiple headers may share a name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawHeaders(pub Vec<(String, String)>);

impl RawHeaders {
    /// Value of the first header named `name`.
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

    /// Header parsed as a number.
    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get(name)?.trim().parse().ok()
    }

    /// Header parsed as an integer.
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
    /// Channel the field becomes.
    pub id: ChannelId,
    /// Unit of the scaled values.
    pub unit: Unit,
    /// Multiplier applied after `offset`.
    pub scale: f64,
    /// Added to the raw value before scaling.
    pub offset: f64,
}

impl FieldMapping {
    /// Identity mapping (scale 1, offset 0).
    pub fn new(id: ChannelId, unit: Unit) -> Self {
        FieldMapping {
            id,
            unit,
            scale: 1.0,
            offset: 0.0,
        }
    }

    /// Mapping that multiplies by `scale`.
    pub fn scaled(id: ChannelId, unit: Unit, scale: f64) -> Self {
        FieldMapping {
            id,
            unit,
            scale,
            offset: 0.0,
        }
    }

    /// Scaled value of a raw logged integer.
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

    /// Add derived channels (throttle percent, PID error, the sticks, ...).
    /// Called after all logged channels are in place.
    fn derive(&self, _session: &mut FlightSession) {}

    /// The flight modes the firmware's logged mode word stands for: the
    /// mode-box mask of the Cleanflight family (`flightModeFlags`), the mode
    /// number of `ArduPilot`. Empty when the profile does not know them.
    fn flight_modes(&self, _firmware: &FirmwareInfo, _word: i64) -> Vec<FlightMode> {
        Vec::new()
    }

    /// Slider ("simplified tuning") model for this log, when the firmware has one.
    fn slider_model(
        &self,
        _firmware: &FirmwareInfo,
        _headers: &RawHeaders,
    ) -> Option<Box<dyn SliderModel>> {
        None
    }
}

impl fmt::Debug for dyn FirmwareProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FirmwareProfile({})", self.id())
    }
}

#[derive(Debug, Default)]
/// Registry of firmware profiles with best-match selection.
pub struct ProfileRegistry {
    profiles: Vec<Box<dyn FirmwareProfile>>,
}

impl ProfileRegistry {
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a profile.
    pub fn register(&mut self, profile: Box<dyn FirmwareProfile>) -> &mut Self {
        self.profiles.push(profile);
        self
    }

    /// All registered profiles.
    pub fn iter(&self) -> impl Iterator<Item = &dyn FirmwareProfile> {
        self.profiles.iter().map(AsRef::as_ref)
    }

    /// Profile by its [`FirmwareProfile::id`].
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
