//! Fallback profile for unknown Cleanflight-family firmwares: well-known
//! field names are mapped with Betaflight 4 conventions.

use super::common::{self, Log};
use crate::cli::BetaflightCli;
use openpid_core::{
    BoundProfile, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor, FlightSession,
    RawHeaders, Settings,
};

#[derive(Debug, Default, Clone, Copy)]
/// Fallback for unknown Cleanflight-family firmwares.
pub struct Generic;

impl FirmwareProfile for Generic {
    fn id(&self) -> &'static str {
        "generic-cleanflight"
    }

    fn score(&self, _fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        Some(1)
    }

    fn bind<'a>(&self, fw: &'a FirmwareInfo, h: &'a RawHeaders) -> Box<dyn BoundProfile + 'a> {
        // an unknown firmware's other boxes are not known: the boxes every
        // Cleanflight fork starts with, the other bits stay numbered
        Box::new(Bound(Log::new(fw, h, &common::BOXES_COMMON[..])))
    }
}

/// The profile of one log of an unknown firmware of the family.
struct Bound<'a>(Log<'a>);

impl BoundProfile for Bound<'_> {
    fn map_field(&self, field: &str) -> Option<FieldMapping> {
        self.0.map_field(field)
    }

    fn settings(&self) -> Settings {
        self.0.settings()
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }

    fn flight_modes(&self, word: i64) -> Vec<openpid_core::FlightMode> {
        self.0.flight_modes(word)
    }

    fn exporter(&self) -> Option<Box<dyn openpid_core::Exporter>> {
        // a log that does not name its firmware (a CSV export, an unknown
        // fork) is taken for Betaflight; a named other firmware has its own
        // setting names, which this profile does not know
        matches!(self.0.fw.vendor, FirmwareVendor::Unknown(_)).then(|| {
            Box::new(BetaflightCli::for_log(self.0.fw, self.0.h)) as Box<dyn openpid_core::Exporter>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_log_that_names_no_firmware_is_taken_for_betaflight() {
        let h = RawHeaders::default();
        let unknown = FirmwareInfo::unknown();
        let exporter = Generic.bind(&unknown, &h).exporter().expect("an exporter");
        assert_eq!(exporter.label(), "Betaflight CLI");
        let named = FirmwareInfo {
            vendor: FirmwareVendor::Baseflight,
            ..FirmwareInfo::unknown()
        };
        assert!(Generic.bind(&named, &h).exporter().is_none());
        assert!(Generic.bind(&unknown, &h).slider_model().is_none());
    }
}
