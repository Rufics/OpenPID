//! INAV profile. Phase 0 only maps the fields shared with Betaflight plus the
//! INAV-specific names needed for PID work; the rest stays raw until the
//! INAV phase.

use super::common::{self, Scales};
use openpid_core::{
    Axis, BatteryField, ChannelId, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor,
    FlightSession, RawHeaders, Settings, Unit,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct Inav;

impl FirmwareProfile for Inav {
    fn id(&self) -> &'static str {
        "inav"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        (fw.vendor == FirmwareVendor::Inav).then_some(100)
    }

    fn map_field(&self, field: &str, fw: &FirmwareInfo, h: &RawHeaders) -> Option<FieldMapping> {
        let (base, idx) = common::split_indexed(field);
        match (base, idx.and_then(Axis::from_index)) {
            ("axisRate", Some(axis)) => Some(FieldMapping::new(
                ChannelId::Setpoint { axis },
                Unit::DegreesPerSecond,
            )),
            ("vbat", _) => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Voltage),
                Unit::Volts,
                0.01,
            )),
            ("amperage", _) => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Current),
                Unit::Amperes,
                0.01,
            )),
            _ => common::map_field(field, &Scales::betaflight(fw, h)),
        }
    }

    fn settings(&self, _fw: &FirmwareInfo, h: &RawHeaders) -> Settings {
        common::settings(h)
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }
}
