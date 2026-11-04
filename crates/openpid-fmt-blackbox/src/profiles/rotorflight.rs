//! Rotorflight (helicopter) profile. Shares the Betaflight layout but logs
//! `gyroRAW`, `Vbat`/`Ibat`, `headspeed`, and uses `setpoint[3]` for collective.

use super::common::{self, Scales};
use openpid_core::{
    BatteryField, ChannelId, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor,
    FlightSession, RawHeaders, Settings, Unit,
};

#[derive(Debug, Default, Clone, Copy)]
/// Rotorflight.
pub struct Rotorflight;

impl FirmwareProfile for Rotorflight {
    fn id(&self) -> &'static str {
        "rotorflight"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        (fw.vendor == FirmwareVendor::Rotorflight).then_some(100)
    }

    fn map_field(&self, field: &str, fw: &FirmwareInfo, h: &RawHeaders) -> Option<FieldMapping> {
        let (base, idx) = common::split_indexed(field);
        match (base, idx) {
            ("Vbat", _) => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Voltage),
                Unit::Volts,
                0.01,
            )),
            ("Ibat", _) => Some(FieldMapping::scaled(
                ChannelId::Battery(BatteryField::Current),
                Unit::Amperes,
                0.01,
            )),
            ("headspeed", _) => Some(FieldMapping::new(
                ChannelId::Custom(field.to_string()),
                Unit::Rpm,
            )),
            ("setpoint", Some(3)) => Some(FieldMapping::new(
                ChannelId::Custom(field.to_string()),
                Unit::Raw,
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
