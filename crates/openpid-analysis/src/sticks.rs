//! Where the pilot's sticks were at a moment of the log, for the Log
//! viewer's stick display (docs/design/log-sticks): roll, pitch and yaw from
//! the RC commands, throttle from the throttle channel.

use openpid_core::{ChannelId, FlightSession};

/// Full deflection of an RC command (Betaflight-family `rcCommand` on roll,
/// pitch and yaw spans ±500).
const RC_FULL: f64 = 500.0;

/// The sticks at one moment: roll, pitch and yaw in −1…1 (right, forward,
/// right positive), throttle in 0…1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    /// Roll command, −1 (full left) … 1 (full right).
    pub roll: f64,
    /// Pitch command, −1 (full back) … 1 (full forward).
    pub pitch: f64,
    /// Yaw command, −1 (full left) … 1 (full right).
    pub yaw: f64,
    /// Throttle, 0 … 1; `None` when the log has no throttle.
    pub throttle: Option<f64>,
}

/// The sticks at `t_s` seconds from the start of the session (the sample at
/// or just after it); `None` when the log has no RC commands.
pub fn sticks_at(session: &FlightSession, t_s: f64) -> Option<Sticks> {
    let time = &session.timebase.time_us;
    let t0 = *time.first()?;
    let target = t0 + (t_s.max(0.0) * 1.0e6) as i64;
    let i = time.partition_point(|t| *t < target).min(time.len() - 1);
    let read = |id: &ChannelId| -> Option<f64> {
        let c = session.channel(id)?;
        let v = c.data.values();
        // the nearest valid sample at or before `i`
        (0..=i.min(v.len().checked_sub(1)?))
            .rev()
            .find(|k| c.data.is_valid(*k))
            .map(|k| v[k])
    };
    let rc = |ch: u8| read(&ChannelId::RcCommand { ch }).map(|v| (v / RC_FULL).clamp(-1.0, 1.0));
    Some(Sticks {
        roll: rc(0)?,
        pitch: rc(1)?,
        yaw: rc(2)?,
        throttle: read(&ChannelId::Throttle).map(|v| (v / 100.0).clamp(0.0, 1.0)),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]
    use super::*;
    use openpid_core::{LogFormat, OpenOptions};
    use openpid_fmt_blackbox::BlackboxFormat;

    #[test]
    fn sticks_follow_the_rc_commands() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/logs/betaflight-2025.12.2-btfl_027.bbl");
        let log = BlackboxFormat::new()
            .open(&std::fs::read(path).unwrap(), &OpenOptions::default())
            .unwrap();
        let s = &log.sessions[0];
        let duration = s.timebase.duration_s();
        let mut throttle_seen = 0.0_f64;
        let mut roll_seen = 0.0_f64;
        for k in 0..200 {
            let st = sticks_at(s, duration * f64::from(k) / 200.0).unwrap();
            for v in [st.roll, st.pitch, st.yaw] {
                assert!((-1.0..=1.0).contains(&v));
            }
            let t = st.throttle.unwrap();
            assert!((0.0..=1.0).contains(&t));
            throttle_seen = throttle_seen.max(t);
            roll_seen = roll_seen.max(st.roll.abs());
        }
        // the flight: 10–37 % throttle, roll inputs well off centre
        assert!(
            throttle_seen > 0.2 && throttle_seen < 0.45,
            "{throttle_seen}"
        );
        assert!(roll_seen > 0.3, "{roll_seen}");
        // the value at a moment is the logged command scaled to the stick
        let rc0 = s.channel(&ChannelId::RcCommand { ch: 0 }).unwrap();
        let i = 5000;
        let t = (s.timebase.time_us[i] - s.timebase.time_us[0]) as f64 / 1e6;
        let st = sticks_at(s, t).unwrap();
        assert!((st.roll - rc0.data.values()[i] / 500.0).abs() < 1e-9);
        // past the end: the last sample; before the start: the first
        assert!(sticks_at(s, duration + 10.0).is_some());
        assert!(sticks_at(s, -1.0).is_some());
    }
}
