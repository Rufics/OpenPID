//! Fallback profile for unknown Cleanflight-family firmwares: well-known
//! field names are mapped with Betaflight 4 conventions.

use super::common::{self, Scales};
use crate::cli::BetaflightCli;
use openpid_core::{
    FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor, FlightSession, RawHeaders,
    Settings,
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

    fn map_field(&self, field: &str, fw: &FirmwareInfo, h: &RawHeaders) -> Option<FieldMapping> {
        common::map_field(field, &Scales::betaflight(fw, h))
    }

    fn settings(&self, fw: &FirmwareInfo, h: &RawHeaders) -> Settings {
        common::settings(fw, h)
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }

    fn flight_modes(&self, _fw: &FirmwareInfo, word: i64) -> Vec<openpid_core::FlightMode> {
        // an unknown firmware's other boxes are not known: the boxes every
        // Cleanflight fork starts with, the other bits stay numbered
        openpid_core::modes::modes_from_mask(word, &common::BOXES_COMMON)
    }

    fn exporter(
        &self,
        fw: &FirmwareInfo,
        h: &RawHeaders,
    ) -> Option<Box<dyn openpid_core::Exporter>> {
        // a log that does not name its firmware (a CSV export, an unknown
        // fork) is taken for Betaflight; a named other firmware has its own
        // setting names, which this profile does not know
        matches!(fw.vendor, FirmwareVendor::Unknown(_))
            .then(|| Box::new(BetaflightCli::for_log(fw, h)) as Box<dyn openpid_core::Exporter>)
    }
}
