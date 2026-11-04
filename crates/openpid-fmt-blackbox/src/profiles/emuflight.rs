//! `EmuFlight` profile: Betaflight 3.5 lineage, so battery voltage is logged in
//! decivolts (confirmed on `EmuFlight` 0.4.1 logs where `vbatref` is ~250 for 6S).

use super::common::{self, Scales};
use openpid_core::{
    FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor, FlightSession, RawHeaders,
    Settings,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct Emuflight;

fn scales(fw: &FirmwareInfo, h: &RawHeaders) -> Scales {
    Scales {
        vbat: 0.1,
        ..Scales::betaflight(fw, h)
    }
}

impl FirmwareProfile for Emuflight {
    fn id(&self) -> &'static str {
        "emuflight"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        (fw.vendor == FirmwareVendor::Emuflight).then_some(100)
    }

    fn map_field(&self, field: &str, fw: &FirmwareInfo, h: &RawHeaders) -> Option<FieldMapping> {
        common::map_field(field, &scales(fw, h))
    }

    fn settings(&self, _fw: &FirmwareInfo, h: &RawHeaders) -> Settings {
        let mut s = common::settings(h);
        // EmuFlight has per-axis gyro lowpass cutoffs; expose the roll value as the chain cutoff.
        if s.gyro_filters.lowpass1.is_none()
            && let Some(hz) = h.get_f64("gyro_lowpass_hz_roll").filter(|v| *v > 0.0)
        {
            s.gyro_filters.lowpass1 = Some(openpid_core::settings::Lowpass {
                kind: None,
                cutoff_hz: hz,
                dyn_min_hz: None,
                dyn_max_hz: None,
            });
        }
        s
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }
}
