//! Automatic analysis windows (docs/design/analysis-window, R4): trim the
//! take-off and landing by throttle and motor rpm, pick the most active
//! stretch for the step response, or cut one second off each end, as the
//! common desktop analysis tools do (decision D-012 of the calibration).

use openpid_core::analysis::Epoch;
use openpid_core::{Axis, ChannelId, FlightSession};

/// Throttle at or above this is "in the air" (percent).
const AIR_THROTTLE_PCT: f64 = 5.0;
/// When motor rpm is logged, the fastest motor must spin at least this fast.
const AIR_RPM: f64 = 1000.0;
/// An airborne stretch must last this long to count (seconds).
const AIR_MIN_RUN_S: f64 = 1.0;
/// Margin cut inside the first and last airborne moment (seconds).
const AIR_MARGIN_S: f64 = 1.0;
/// A trimmed window shorter than this is not worth it.
const MIN_WINDOW_S: f64 = 5.0;
/// Length of the "most active" window (seconds).
pub const ACTIVE_WINDOW_S: f64 = 20.0;

/// Seconds of sample `i` from the first sample.
fn seconds(s: &FlightSession, i: usize) -> f64 {
    let t = &s.timebase.time_us;
    (t[i] - t[0]) as f64 * 1e-6
}

/// Whether sample `i` is airborne: throttle at or above 5 % and, when rpm is
/// logged, the fastest motor at or above 1000 rpm.
fn airborne(throttle: &[f64], rpm: &[&[f64]], i: usize) -> bool {
    if !throttle.get(i).is_some_and(|t| *t >= AIR_THROTTLE_PCT) {
        return false;
    }
    if rpm.is_empty() {
        return true;
    }
    rpm.iter().filter_map(|r| r.get(i)).any(|r| *r >= AIR_RPM)
}

/// The window from the first to the last airborne moment, one second inside
/// each; `None` when the log has no throttle, never flies for a second, or
/// the trimmed window would be shorter than five seconds.
pub fn auto(s: &FlightSession) -> Option<Epoch> {
    let throttle = s.channel(&ChannelId::Throttle)?.data.values();
    let rpm: Vec<&[f64]> = s
        .channels
        .iter()
        .filter(|c| matches!(c.id, ChannelId::MotorRpm { .. }))
        .map(|c| c.data.values())
        .collect();
    let n = s.timebase.len().min(throttle.len());
    if n == 0 {
        return None;
    }
    // first and last sample of every airborne run that lasts long enough
    let mut first: Option<usize> = None;
    let mut last: Option<usize> = None;
    let mut run_start: Option<usize> = None;
    for i in 0..=n {
        let air = i < n && airborne(throttle, &rpm, i);
        match (air, run_start) {
            (true, None) => run_start = Some(i),
            (false, Some(a)) => {
                let b = i - 1;
                if seconds(s, b) - seconds(s, a) >= AIR_MIN_RUN_S {
                    first.get_or_insert(a);
                    last = Some(b);
                }
                run_start = None;
            }
            _ => {}
        }
    }
    let (a, b) = (first?, last?);
    let start = seconds(s, a) + AIR_MARGIN_S;
    let end = seconds(s, b) - AIR_MARGIN_S;
    (end - start >= MIN_WINDOW_S).then_some(Epoch {
        start_s: start,
        end_s: end,
    })
}

/// The `window_s` long window with the most |setpoint| energy on roll and
/// pitch (where the sticks moved most, so the step response has the most
/// windows); `None` without a setpoint or when the log is shorter than the window.
pub fn most_active(s: &FlightSession, window_s: f64) -> Option<Epoch> {
    let sp: Vec<&[f64]> = [Axis::Roll, Axis::Pitch]
        .iter()
        .filter_map(|axis| s.channel(&ChannelId::Setpoint { axis: *axis }))
        .map(|c| c.data.values())
        .collect();
    if sp.is_empty() {
        return None;
    }
    let n = s.timebase.len();
    if n == 0 || s.timebase.duration_s() < window_s {
        return None;
    }
    // prefix sums of the activity, so every window is one subtraction
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0);
    for i in 0..n {
        let a: f64 = sp
            .iter()
            .filter_map(|v| v.get(i))
            .filter(|v| v.is_finite())
            .map(|v| v.abs())
            .sum();
        prefix.push(prefix[i] + a);
    }
    let times = &s.timebase.time_us;
    let t0 = times[0];
    let mut best: Option<(f64, usize)> = None;
    let mut j = 0;
    for i in 0..n {
        let end_us = times[i] + (window_s * 1e6) as i64;
        if end_us > times[n - 1] + 1 {
            break;
        }
        while j < n && times[j] < end_us {
            j += 1;
        }
        let energy = prefix[j] - prefix[i];
        if best.is_none_or(|(e, _)| energy > e) {
            best = Some((energy, i));
        }
    }
    let (_, i) = best?;
    let start = (times[i] - t0) as f64 * 1e-6;
    Some(Epoch {
        start_s: start,
        end_s: start + window_s,
    })
}

/// A log at most this long is taken whole by [`cut_ends`] (seconds). The
/// calibration placed the switch between 10 and 11 s (rounds 16 and 17);
/// the middle of that interval until a round narrows it.
pub const CUT_ENDS_WHOLE_UP_TO_S: f64 = 10.5;

/// The stretch the common desktop analysis tools take: the log without its
/// first and last second, from sample `fs + 2` to sample `n − fs` (counting
/// from 0; `fs·(T − 2) − 1` samples of a log of `n = fs·T`); a log of at
/// most [`CUT_ENDS_WHOLE_UP_TO_S`] from its second sample to its end (the
/// first sample is never taken). Bounds are the timestamps of those samples,
/// so the stretch is exact on a log with jitter. `None` for a log of fewer
/// than three samples. Decision D-012 of the calibration (docs/calibration).
pub fn cut_ends(s: &FlightSession) -> Option<Epoch> {
    let n = s.timebase.len();
    if n < 3 {
        return None;
    }
    let fs = (1.0e6 / crate::prepare::nominal_dt_us(s)).round() as usize;
    let (a, b) = if n as f64 / fs.max(1) as f64 <= CUT_ENDS_WHOLE_UP_TO_S {
        (1, n - 1)
    } else {
        (fs + 2, n - fs)
    };
    (a < b).then(|| Epoch {
        start_s: seconds(s, a),
        end_s: seconds(s, b),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::series::Series;
    use openpid_core::session::Channel;
    use openpid_sim::signal::{SetpointProfile, SignalSpec, ThrottleProfile};

    /// A 40 s flight: motors off for the first `idle_s` and the last `idle_s`.
    fn flight(duration_s: f64, idle_s: f64, with_rpm: bool) -> FlightSession {
        let mut s = SignalSpec {
            duration_s,
            throttle: ThrottleProfile::Constant { percent: 40.0 },
            motors: if with_rpm { 4 } else { 0 },
            setpoint: SetpointProfile::Silence,
            ..SignalSpec::default()
        }
        .build();
        let fs = 1000.0;
        let idle = (idle_s * fs) as usize;
        let n = s.timebase.len();
        let ground = |i: usize| i < idle || i >= n.saturating_sub(idle);
        let ids: Vec<ChannelId> = s
            .channels
            .iter()
            .filter(|c| matches!(c.id, ChannelId::Throttle | ChannelId::MotorRpm { .. }))
            .map(|c| c.id.clone())
            .collect();
        for id in ids {
            let c = s.channels.remove(&id).unwrap();
            let values: Vec<f64> = c
                .data
                .values()
                .iter()
                .enumerate()
                .map(|(i, v)| if ground(i) { 0.0 } else { *v })
                .collect();
            s.channels.insert(Channel {
                data: Series::from_vec(values),
                ..c
            });
        }
        s
    }

    #[test]
    fn auto_trims_the_ground_time_one_second_inside() {
        let s = flight(40.0, 3.0, true);
        let e = auto(&s).expect("a 34 s flight");
        assert!((e.start_s - 4.0).abs() < 0.01, "{e:?}");
        assert!((e.end_s - 36.0).abs() < 0.01, "{e:?}");
        // without rpm the throttle alone decides
        let e = auto(&flight(40.0, 3.0, false)).unwrap();
        assert!((e.start_s - 4.0).abs() < 0.01 && (e.end_s - 36.0).abs() < 0.01);
    }

    #[test]
    fn auto_ignores_a_blip_and_refuses_a_short_flight() {
        let mut s = flight(40.0, 3.0, false);
        // a half-second throttle blip on the ground does not start the flight
        let c = s.channels.remove(&ChannelId::Throttle).unwrap();
        let mut v = c.data.values().to_vec();
        for x in &mut v[500..900] {
            *x = 30.0;
        }
        s.channels.insert(Channel {
            data: Series::from_vec(v),
            ..c
        });
        let e = auto(&s).unwrap();
        assert!((e.start_s - 4.0).abs() < 0.01, "{e:?}");
        // 6 s of flight minus the margins is 4 s: too short
        assert_eq!(auto(&flight(12.0, 3.0, true)), None);
        // never airborne, or no throttle at all
        assert_eq!(auto(&flight(20.0, 10.0, true)), None);
        let mut s = flight(20.0, 1.0, false);
        s.channels.remove(&ChannelId::Throttle);
        assert_eq!(auto(&s), None);
    }

    #[test]
    fn rpm_vetoes_the_throttle() {
        let mut s = flight(40.0, 3.0, true);
        // throttle up for the whole log, motors still off at both ends
        let c = s.channels.remove(&ChannelId::Throttle).unwrap();
        let v = vec![40.0; c.data.len()];
        s.channels.insert(Channel {
            data: Series::from_vec(v),
            ..c
        });
        let e = auto(&s).unwrap();
        assert!(
            (e.start_s - 4.0).abs() < 0.01 && (e.end_s - 36.0).abs() < 0.01,
            "{e:?}"
        );
    }

    #[test]
    fn most_active_finds_the_stick_moves() {
        let mut s = SignalSpec {
            duration_s: 60.0,
            setpoint: SetpointProfile::Silence,
            ..SignalSpec::default()
        }
        .build();
        // a burst of roll setpoint between 30 and 40 s
        let id = ChannelId::Setpoint { axis: Axis::Roll };
        let c = s.channels.remove(&id).unwrap();
        let mut v = vec![0.0; c.data.len()];
        for x in &mut v[30_000..40_000] {
            *x = 200.0;
        }
        s.channels.insert(Channel {
            data: Series::from_vec(v),
            ..c
        });
        let e = most_active(&s, ACTIVE_WINDOW_S).unwrap();
        assert!((e.end_s - e.start_s - 20.0).abs() < 1e-6);
        assert!(
            e.start_s <= 30.0 && e.end_s >= 40.0,
            "{e:?} holds the burst"
        );
        assert_eq!(most_active(&s, 100.0), None, "longer than the log");
        s.channels.remove(&id);
        s.channels
            .remove(&ChannelId::Setpoint { axis: Axis::Pitch });
        assert_eq!(most_active(&s, 20.0), None, "no setpoint");
    }

    /// Samples `[a, b)` that the epoch covers.
    fn covered(s: &FlightSession, e: Epoch) -> (usize, usize) {
        crate::prepare::epoch_range(s, Some(e))
    }

    #[test]
    fn cut_ends_leaves_out_one_second_at_each_end() {
        // 20 s at 1 kHz: samples 1002 to 19 000, 17 999 of them
        let s = flight(20.0, 0.0, false);
        let n = s.timebase.len();
        assert_eq!(n, 20_000);
        let e = cut_ends(&s).unwrap();
        assert_eq!(covered(&s, e), (1002, 19_001));
        assert_eq!(19_001 - 1002, 1000 * (20 - 2) - 1);
        // the ground time does not matter: it is not the automatic trim
        assert_eq!(cut_ends(&flight(20.0, 3.0, true)), Some(e));
    }

    #[test]
    fn cut_ends_takes_a_short_log_whole_but_its_first_sample() {
        let s = flight(10.0, 0.0, false);
        let e = cut_ends(&s).unwrap();
        assert_eq!(covered(&s, e), (1, 10_000));
        // just above the switch: trimmed
        let s = flight(10.6, 0.0, false);
        let e = cut_ends(&s).unwrap();
        assert_eq!(covered(&s, e), (1002, s.timebase.len() - 1000 + 1));
    }

    #[test]
    fn cut_ends_needs_three_samples() {
        let mut s = flight(1.0, 0.0, false);
        s.timebase.time_us.truncate(2);
        assert_eq!(cut_ends(&s), None);
    }
}
