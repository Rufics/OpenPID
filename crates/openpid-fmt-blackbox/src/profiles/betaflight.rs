//! Betaflight (and Cleanflight) profile.

use super::common::{self, Scales};
use openpid_core::{
    FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor, FlightSession, RawHeaders,
    Settings,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct Betaflight;

impl FirmwareProfile for Betaflight {
    fn id(&self) -> &'static str {
        "betaflight"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        match fw.vendor {
            FirmwareVendor::Betaflight => Some(100),
            FirmwareVendor::Cleanflight => Some(60),
            _ => None,
        }
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
