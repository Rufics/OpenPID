//! Flight modes as one set of values for every firmware (docs/design/log-sticks).
//!
//! Each format crate turns what its log records (Betaflight-family
//! `flightModeFlags` bits, `ArduPilot` `MODE` and `EV` messages, ...) into
//! [`ModeChange`]s on the session, through its firmware profile
//! ([`crate::BoundProfile::flight_modes`]); everything after the decoder
//! reads them with [`crate::FlightSession::flight_modes_at`] and
//! [`crate::FlightSession::mode_changes`], never the firmware's numbering.

use serde::{Deserialize, Serialize};

/// A flight mode or mode switch. The common ones are variants of their own;
/// anything else keeps the firmware's name in [`FlightMode::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightMode {
    /// Motors armed.
    Arm,
    /// Armed, waiting for the arm switch to be confirmed.
    Prearm,
    /// Rate mode without self-levelling (`ArduPilot` ACRO).
    Acro,
    /// Self-levelling (Betaflight/INAV ANGLE, `ArduPilot` STABILIZE and FBWA).
    Angle,
    /// Self-levelling near centre stick, rate at full deflection.
    Horizon,
    /// PID authority kept at zero throttle.
    AirMode,
    /// Servos / motors straight from the sticks (INAV MANUAL, `ArduPilot` MANUAL, PASSTHRU).
    Manual,
    /// Altitude held (ALTHOLD, NAV ALTHOLD, `ArduPilot` `ALT_HOLD`).
    AltHold,
    /// Position held (POSHOLD, NAV POSHOLD, `ArduPilot` POSHOLD).
    PosHold,
    /// Circling or holding a point (`ArduPilot` LOITER).
    Loiter,
    /// Heading kept by the flight controller.
    HeadingHold,
    /// Controls relative to the take-off heading.
    Headfree,
    /// Return to home (NAV RTH, `ArduPilot` RTL, `SMART_RTL`, QRTL).
    ReturnHome,
    /// Betaflight's GPS rescue.
    GpsRescue,
    /// Rotorflight's rescue.
    Rescue,
    /// Failsafe active.
    Failsafe,
    /// Landing.
    Land,
    /// Following a mission or waypoints (`ArduPilot` AUTO, INAV NAV WP).
    Auto,
    /// Guided by a ground station or a companion computer.
    Guided,
    /// Assisted launch or take-off.
    Launch,
    /// Flip over after a crash (TURTLE, FLIPOVERAFTERCRASH).
    Turtle,
    /// Tuning in flight (AUTOTUNE, AUTO TUNE).
    Autotune,
    /// Any other mode or switch, by the firmware's name.
    Other(String),
}

impl FlightMode {
    /// The name shown to the user.
    pub fn name(&self) -> &str {
        match self {
            FlightMode::Arm => "ARM",
            FlightMode::Prearm => "PREARM",
            FlightMode::Acro => "ACRO",
            FlightMode::Angle => "ANGLE",
            FlightMode::Horizon => "HORIZON",
            FlightMode::AirMode => "AIRMODE",
            FlightMode::Manual => "MANUAL",
            FlightMode::AltHold => "ALTHOLD",
            FlightMode::PosHold => "POSHOLD",
            FlightMode::Loiter => "LOITER",
            FlightMode::HeadingHold => "HEADING HOLD",
            FlightMode::Headfree => "HEADFREE",
            FlightMode::ReturnHome => "RTH",
            FlightMode::GpsRescue => "GPS RESCUE",
            FlightMode::Rescue => "RESCUE",
            FlightMode::Failsafe => "FAILSAFE",
            FlightMode::Land => "LAND",
            FlightMode::Auto => "AUTO",
            FlightMode::Guided => "GUIDED",
            FlightMode::Launch => "LAUNCH",
            FlightMode::Turtle => "TURTLE",
            FlightMode::Autotune => "AUTOTUNE",
            FlightMode::Other(name) => name,
        }
    }

    /// The common mode a firmware's name stands for, else [`FlightMode::Other`]
    /// with the name as given. Case, spaces, `_` and a `_MODE` suffix are
    /// ignored ("NAV POSHOLD", "POSHOLD", `POS_HOLD` are all [`FlightMode::PosHold`]).
    pub fn from_name(name: &str) -> FlightMode {
        let key: String = name
            .trim()
            .trim_end_matches("_MODE")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect();
        match key.as_str() {
            "ARM" | "ARMED" => FlightMode::Arm,
            "PREARM" => FlightMode::Prearm,
            "ACRO" => FlightMode::Acro,
            "ANGLE" | "STABILIZE" | "FBWA" | "QSTABILIZE" => FlightMode::Angle,
            "HORIZON" => FlightMode::Horizon,
            "AIRMODE" => FlightMode::AirMode,
            "MANUAL" | "PASSTHRU" => FlightMode::Manual,
            "ALTHOLD" | "NAVALTHOLD" | "BARO" | "QHOVER" => FlightMode::AltHold,
            "POSHOLD" | "NAVPOSHOLD" | "GPSHOLD" => FlightMode::PosHold,
            "LOITER" | "QLOITER" => FlightMode::Loiter,
            "HEADINGHOLD" | "HEADING" | "MAG" => FlightMode::HeadingHold,
            "HEADFREE" => FlightMode::Headfree,
            "NAVRTH" | "RTL" | "SMARTRTL" | "QRTL" | "GPSHOME" | "AUTORTL" => {
                FlightMode::ReturnHome
            }
            "GPSRESCUE" => FlightMode::GpsRescue,
            "RESCUE" => FlightMode::Rescue,
            "FAILSAFE" => FlightMode::Failsafe,
            "LAND" | "QLAND" => FlightMode::Land,
            "AUTO" | "NAVWP" => FlightMode::Auto,
            "GUIDED" => FlightMode::Guided,
            "NAVLAUNCH" | "LAUNCH" | "TAKEOFF" => FlightMode::Launch,
            "TURTLE" | "FLIPOVERAFTERCRASH" => FlightMode::Turtle,
            "AUTOTUNE" | "QAUTOTUNE" => FlightMode::Autotune,
            _ => FlightMode::Other(name.trim().to_string()),
        }
    }
}

/// The modes active from one main-frame sample on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeChange {
    /// Index into the main-frame timeline from which `modes` hold.
    pub sample_index: usize,
    /// Every mode active from then on, in the firmware's order.
    pub modes: Vec<FlightMode>,
}

/// The modes active at sample `i` of `changes` (sorted by sample): the last
/// change at or before it, else the first one; empty without changes.
pub fn modes_at(changes: &[ModeChange], i: usize) -> &[FlightMode] {
    let k = changes.partition_point(|c| c.sample_index <= i);
    changes
        .get(k.saturating_sub(1))
        .map_or(&[], |c| c.modes.as_slice())
}

/// Appends `modes` from `sample_index` on, unless they equal the last change.
pub fn push_change(changes: &mut Vec<ModeChange>, sample_index: usize, modes: Vec<FlightMode>) {
    if changes.last().is_some_and(|c| c.modes == modes) {
        return;
    }
    changes.push(ModeChange {
        sample_index,
        modes,
    });
}

/// The modes of the bits set in a mode-box mask, by the firmware's box
/// names in bit order; a bit past the list reads `bit n`.
pub fn modes_from_mask(mask: i64, names: &[&str]) -> Vec<FlightMode> {
    (0..64)
        .filter(|b| (mask >> b) & 1 == 1)
        .map(|b| match names.get(b as usize) {
            Some(n) => FlightMode::from_name(n),
            None => FlightMode::Other(format!("bit {b}")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_the_common_modes() {
        assert_eq!(FlightMode::from_name("NAV POSHOLD"), FlightMode::PosHold);
        assert_eq!(FlightMode::from_name("ANGLE_MODE"), FlightMode::Angle);
        assert_eq!(
            FlightMode::from_name("FLIPOVERAFTERCRASH"),
            FlightMode::Turtle
        );
        assert_eq!(FlightMode::from_name("QRTL"), FlightMode::ReturnHome);
        assert_eq!(
            FlightMode::from_name("BLACKBOX"),
            FlightMode::Other("BLACKBOX".into())
        );
        assert_eq!(FlightMode::GpsRescue.name(), "GPS RESCUE");
        assert_eq!(FlightMode::Other("OSD".into()).name(), "OSD");
    }

    #[test]
    fn changes_are_looked_up_by_sample() {
        let mut c = Vec::new();
        push_change(&mut c, 0, vec![FlightMode::Arm]);
        push_change(&mut c, 5, vec![FlightMode::Arm]); // no change: dropped
        push_change(&mut c, 10, vec![FlightMode::Arm, FlightMode::Angle]);
        assert_eq!(c.len(), 2);
        assert_eq!(modes_at(&c, 3), [FlightMode::Arm]);
        assert_eq!(modes_at(&c, 10), [FlightMode::Arm, FlightMode::Angle]);
        assert_eq!(modes_at(&c, 99).len(), 2);
        assert!(modes_at(&[], 3).is_empty());
        assert_eq!(
            modes_from_mask(0b101 | 1 << 40, &["ARM", "ANGLE", "NAV POSHOLD"]),
            [
                FlightMode::Arm,
                FlightMode::PosHold,
                FlightMode::Other("bit 40".into())
            ]
        );
    }
}
