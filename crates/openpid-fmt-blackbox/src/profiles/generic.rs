//! Fallback profile for unknown Cleanflight-family firmwares: well-known
//! field names are mapped with Betaflight 4 conventions.

use super::common::{self, Scales};
use openpid_core::{
    FieldMapping, FirmwareInfo, FirmwareProfile, FlightSession, RawHeaders, Settings,
};

#[derive(Debug, Default, Clone, Copy)]
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

    fn settings(&self, _fw: &FirmwareInfo, h: &RawHeaders) -> Settings {
        common::settings(h)
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }
}
