//! The flight modes active at a moment of the log, for the Log viewer
//! (docs/design/log-sticks). The format crates decode the modes into
//! [`FlightSession::flight_modes`]; this only finds the moment.

use crate::sticks::sample_at;
use openpid_core::{FlightMode, FlightSession};

/// The flight modes active at `t_s` seconds from the start of the session;
/// `None` when the log records no modes.
pub fn flight_modes_at(session: &FlightSession, t_s: f64) -> Option<&[FlightMode]> {
    if session.flight_modes.is_empty() {
        return None;
    }
    let i = sample_at(session, t_s)?;
    Some(openpid_core::modes::modes_at(&session.flight_modes, i))
}

/// Time (s from the start of the session) of every mode change, with the
/// modes from then on, for marks along the plots.
pub fn mode_changes(session: &FlightSession) -> Vec<(f64, &[FlightMode])> {
    let time = &session.timebase.time_us;
    let Some(t0) = time.first().copied() else {
        return Vec::new();
    };
    session
        .flight_modes
        .iter()
        .filter_map(|c| {
            let t = *time.get(c.sample_index)?;
            Some(((t - t0) as f64 / 1.0e6, c.modes.as_slice()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]
    use super::*;
    use openpid_core::{LogFormat, OpenOptions};
    use openpid_fmt_blackbox::BlackboxFormat;

    #[test]
    fn modes_of_a_real_flight() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/logs/betaflight-2025.12.2-btfl_027.bbl");
        let log = BlackboxFormat::new()
            .open(&std::fs::read(path).unwrap(), &OpenOptions::default())
            .unwrap();
        let s = &log.sessions[0];
        // armed in ANGLE, POSHOLD from 12.8 s, disarmed at the end
        assert_eq!(
            flight_modes_at(s, 5.0).unwrap(),
            [FlightMode::Arm, FlightMode::Angle]
        );
        assert_eq!(
            flight_modes_at(s, 20.0).unwrap(),
            [FlightMode::Arm, FlightMode::PosHold]
        );
        let changes = mode_changes(s);
        assert_eq!(changes.len(), 3);
        assert!((changes[1].0 - 12.8).abs() < 0.1, "{}", changes[1].0);
        // a log without modes
        let mut none = s.clone();
        none.flight_modes.clear();
        assert!(flight_modes_at(&none, 5.0).is_none());
    }
}
