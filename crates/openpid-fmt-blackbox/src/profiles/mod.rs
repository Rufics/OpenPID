//! Firmware profiles for the Cleanflight family.

pub mod betaflight;
pub mod common;
pub mod emuflight;
pub mod generic;
pub mod inav;
pub mod rotorflight;

use openpid_core::ProfileRegistry;

/// Registry with every built-in profile.
pub fn builtin() -> ProfileRegistry {
    let mut r = ProfileRegistry::new();
    r.register(Box::new(betaflight::Betaflight))
        .register(Box::new(emuflight::Emuflight))
        .register(Box::new(rotorflight::Rotorflight))
        .register(Box::new(inav::Inav))
        .register(Box::new(generic::Generic));
    r
}
