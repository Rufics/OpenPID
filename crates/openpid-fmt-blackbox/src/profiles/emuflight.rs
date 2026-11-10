//! `EmuFlight` profile: Betaflight 3.5 lineage, so battery voltage is logged in
//! decivolts (confirmed on `EmuFlight` 0.4.1 logs where `vbatref` is ~250 for 6S).

use super::common::{self, Log};
use openpid_core::{
    BoundProfile, FieldMapping, FirmwareInfo, FirmwareProfile, FirmwareVendor, FlightSession,
    RawHeaders, Settings,
};

#[derive(Debug, Default, Clone, Copy)]
/// `EmuFlight`.
pub struct Emuflight;

impl FirmwareProfile for Emuflight {
    fn id(&self) -> &'static str {
        "emuflight"
    }

    fn score(&self, fw: &FirmwareInfo, _h: &RawHeaders) -> Option<u8> {
        (fw.vendor == FirmwareVendor::Emuflight).then_some(100)
    }

    fn bind<'a>(&self, fw: &'a FirmwareInfo, h: &'a RawHeaders) -> Box<dyn BoundProfile + 'a> {
        // `EmuFlight`'s own Explorer names none of its boxes, and its list
        // is not Betaflight's: the boxes every Cleanflight fork starts with,
        // the other bits stay numbered
        let mut log = Log::new(fw, h, &common::BOXES_COMMON[..]);
        log.scales.vbat = 0.1;
        Box::new(Bound(log))
    }
}

/// The profile of one `EmuFlight` log.
struct Bound<'a>(Log<'a>);

impl BoundProfile for Bound<'_> {
    fn map_field(&self, field: &str) -> Option<FieldMapping> {
        self.0.map_field(field)
    }

    fn settings(&self) -> Settings {
        let mut s = self.0.settings();
        // EmuFlight has per-axis gyro lowpass cutoffs; expose the roll value as the chain cutoff.
        if s.gyro_filters.lowpass1.is_none()
            && let Some(hz) = self
                .0
                .h
                .get_f64("gyro_lowpass_hz_roll")
                .filter(|v| *v > 0.0)
        {
            s.gyro_filters.lowpass1 = Some(openpid_core::settings::Lowpass {
                kind: None,
                cutoff_hz: hz,
                dyn_min_hz: None,
                dyn_max_hz: None,
                dyn_expo: 0.0,
            });
        }
        s
    }

    fn derive(&self, session: &mut FlightSession) {
        common::derive(session);
    }

    fn flight_modes(&self, word: i64) -> Vec<openpid_core::FlightMode> {
        self.0.flight_modes(word)
    }
}
