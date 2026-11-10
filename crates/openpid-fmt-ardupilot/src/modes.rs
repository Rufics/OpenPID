//! Flight modes from the `MODE` messages (the vehicle's mode numbers) and
//! arming from the `EV` messages (docs/design/log-sticks).

use crate::profile::Vehicle;
use openpid_core::modes::{FlightMode, ModeChange, push_change};
use openpid_core::session::{Event, EventKind};

/// `EV` ids for arming and disarming.
const EV_ARMED: i64 = 10;
const EV_DISARMED: i64 = 11;

/// The name of Copter mode `n` (`ArduCopter` `Mode::Number`).
fn copter(n: i64) -> Option<&'static str> {
    Some(match n {
        0 => "STABILIZE",
        1 => "ACRO",
        2 => "ALT_HOLD",
        3 => "AUTO",
        4 => "GUIDED",
        5 => "LOITER",
        6 => "RTL",
        7 => "CIRCLE",
        9 => "LAND",
        11 => "DRIFT",
        13 => "SPORT",
        14 => "FLIP",
        15 => "AUTOTUNE",
        16 => "POSHOLD",
        17 => "BRAKE",
        18 => "THROW",
        19 => "AVOID_ADSB",
        20 => "GUIDED_NOGPS",
        21 => "SMART_RTL",
        22 => "FLOWHOLD",
        23 => "FOLLOW",
        24 => "ZIGZAG",
        25 => "SYSTEMID",
        26 => "AUTOROTATE",
        27 => "AUTO_RTL",
        28 => "TURTLE",
        _ => return None,
    })
}

/// The name of Plane mode `n` (`ArduPlane` `Mode::Number`, `QuadPlane` modes included).
fn plane(n: i64) -> Option<&'static str> {
    Some(match n {
        0 => "MANUAL",
        1 => "CIRCLE",
        2 => "STABILIZE",
        3 => "TRAINING",
        4 => "ACRO",
        5 => "FBWA",
        6 => "FBWB",
        7 => "CRUISE",
        8 => "AUTOTUNE",
        10 => "AUTO",
        11 => "RTL",
        12 => "LOITER",
        13 => "TAKEOFF",
        14 => "AVOID_ADSB",
        15 => "GUIDED",
        17 => "QSTABILIZE",
        18 => "QHOVER",
        19 => "QLOITER",
        20 => "QLAND",
        21 => "QRTL",
        22 => "QAUTOTUNE",
        23 => "QACRO",
        24 => "THERMAL",
        25 => "LOITER_ALT_QLAND",
        _ => return None,
    })
}

/// Mode `n` of `vehicle`; an unknown number keeps it as `mode n`.
pub fn mode(vehicle: Vehicle, n: i64) -> FlightMode {
    let name = match vehicle {
        Vehicle::Copter => copter(n),
        Vehicle::Plane => plane(n),
        Vehicle::Other => None,
    };
    name.map_or_else(
        || FlightMode::Other(format!("mode {n}")),
        FlightMode::from_name,
    )
}

/// The mode changes of a log's `MODE` and `EV` events (in sample order):
/// the mode, with [`FlightMode::Arm`] first while armed. Before the first
/// `EV` the arming state is unknown and not shown.
pub fn changes(vehicle: Vehicle, events: &[Event]) -> Vec<ModeChange> {
    let mut out = Vec::new();
    let mut armed: Option<bool> = None;
    let mut current: Option<FlightMode> = None;
    for e in events {
        let EventKind::Other { name, data } = &e.kind else {
            continue;
        };
        match name.as_str() {
            "MODE" => {
                let Some(n) = data.get("ModeNum").or_else(|| data.get("Mode")) else {
                    continue;
                };
                current = Some(mode(vehicle, *n));
            }
            "EV" => match data.get("Id") {
                Some(&EV_ARMED) => armed = Some(true),
                Some(&EV_DISARMED) => armed = Some(false),
                _ => continue,
            },
            _ => continue,
        }
        let mut modes = Vec::new();
        if armed == Some(true) {
            modes.push(FlightMode::Arm);
        }
        modes.extend(current.clone());
        push_change(&mut out, e.sample_index, modes);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ev(i: usize, name: &str, kv: &[(&str, i64)]) -> Event {
        Event {
            sample_index: i,
            kind: EventKind::Other {
                name: name.into(),
                data: kv
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), *v))
                    .collect::<BTreeMap<_, _>>(),
            },
        }
    }

    #[test]
    fn modes_follow_the_vehicle_and_the_arming() {
        assert_eq!(mode(Vehicle::Copter, 5), FlightMode::Loiter);
        assert_eq!(mode(Vehicle::Copter, 0), FlightMode::Angle, "STABILIZE");
        assert_eq!(mode(Vehicle::Plane, 21), FlightMode::ReturnHome, "QRTL");
        assert_eq!(mode(Vehicle::Plane, 7), FlightMode::Other("CRUISE".into()));
        assert_eq!(mode(Vehicle::Other, 3), FlightMode::Other("mode 3".into()));
        let events = [
            ev(0, "MODE", &[("Mode", 0), ("ModeNum", 0)]),
            ev(5, "EV", &[("Id", EV_ARMED)]),
            ev(8, "EV", &[("Id", 15)]), // not an arming event
            ev(9, "MODE", &[("Mode", 2), ("ModeNum", 2), ("Rsn", 1)]),
            ev(20, "EV", &[("Id", EV_DISARMED)]),
        ];
        let c = changes(Vehicle::Copter, &events);
        let seq: Vec<(usize, Vec<FlightMode>)> =
            c.into_iter().map(|c| (c.sample_index, c.modes)).collect();
        assert_eq!(
            seq,
            [
                (0, vec![FlightMode::Angle]),
                (5, vec![FlightMode::Arm, FlightMode::Angle]),
                (9, vec![FlightMode::Arm, FlightMode::AltHold]),
                (20, vec![FlightMode::AltHold]),
            ]
        );
    }
}
