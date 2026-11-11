//! The probes of every round, as data.
//!
//! A probe is one log. Each of its axes carries one experiment: a signal
//! chosen so that one unknown of a tool's pipeline leaves one mark that can
//! be read off a screen. The probes of a round that has been run are never
//! changed afterwards, or the readings taken from them would no longer
//! belong to any log; a new question gets a new probe in a later round
//! (`docs/calibration/ROUNDS.md`).

use crate::signal::{AxisSpec, Filtered, FlightSpec, Plant, Stick, Throttle, Wave};
use serde::Serialize;

/// The part of the tools a probe is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// The spectrum of a channel.
    Spectrum,
}

/// One log of a round.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    /// Short name, also the file name: `s1-levels-2k`.
    pub id: &'static str,
    /// What it is about.
    pub area: Area,
    /// What the log is for, in one line.
    pub title: &'static str,
    /// What each axis settles: roll, pitch, yaw.
    pub purpose: [&'static str; 3],
    /// The flight.
    pub flight: FlightSpec,
}

// The amplitudes are chosen for the reference tool's default axis of
// -50 … 20 dB, on which round 0 showed a 20 deg/s tone near 5 dB and the
// floor of a quiet axis near -40 dB: nothing has to be rescaled by hand.

/// Length of a spectrum probe, seconds: long enough to average over many
/// segments after a tool has dropped the ends of the log.
const SPECTRUM_S: f64 = 30.0;

/// A little white noise under every tone. Whole deg/s is all a log keeps,
/// and a pure sine rounded to whole numbers has spurious lines; with noise
/// of this RMS under it the rounding error is noise as well.
fn dither(seed: u64) -> Wave {
    Wave::Noise { rms: 0.5, seed }
}

/// An axis that carries `tones` (Hz, deg/s) over the dither.
fn tones(seed: u64, tones: &[(f64, f64)]) -> AxisSpec {
    let mut waves: Vec<Wave> = tones.iter().map(|(hz, a)| Wave::tone(*hz, *a)).collect();
    waves.push(dither(seed));
    AxisSpec::of(waves)
}

/// Noise between two frequencies with the given density, (deg/s)²/Hz.
fn band(from_hz: f64, to_hz: f64, density: f64, seed: u64) -> Wave {
    Wave::Band {
        from_hz,
        to_hz,
        rms: (density * (to_hz - from_hz)).sqrt(),
        seed,
    }
}

/// A tone that sounds from `from_s` to `to_s` only.
fn burst(hz: f64, amplitude: f64, from_s: f64, to_s: f64) -> Wave {
    Wave::Tone {
        hz,
        amplitude,
        phase_deg: 0.0,
        from_s: Some(from_s),
        to_s: Some(to_s),
    }
}

/// The level probe at `fs`: the same three axes at every log rate.
fn levels(id: &'static str, fs: f64) -> Probe {
    Probe {
        id,
        area: Area::Spectrum,
        title: "Tones of known amplitude: the dB reference, 10·log against 20·log, the log rate",
        purpose: [
            "a 100 Hz tone of 3 deg/s: where the dB scale puts it",
            "the same tone ten times larger: 20 dB more for decibels of power or of amplitude, \
             10 dB if 10·log is taken of an amplitude, 40 dB if 20·log is taken of a power",
            "three tones of 10 deg/s at 48, 200 and 400 Hz: whether the level depends on the \
             frequency",
        ],
        flight: FlightSpec {
            roll: tones(11, &[(100.0, 3.0)]),
            pitch: tones(12, &[(100.0, 30.0)]),
            yaw: tones(13, &[(48.0, 10.0), (200.0, 10.0), (400.0, 10.0)]),
            ..FlightSpec::quiet(fs, SPECTRUM_S)
        },
    }
}

/// Round 1: the spectrum.
fn round_1() -> Vec<Probe> {
    let quiet = |fs: f64| FlightSpec::quiet(fs, SPECTRUM_S);
    vec![
        levels("s1-levels-2k", 2000.0),
        levels("s2-levels-1k", 1000.0),
        levels("s3-levels-4k", 4000.0),
        Probe {
            id: "s4-resolution-2k",
            area: Area::Spectrum,
            title: "Close tones and a lone tone: the segment length and the window",
            purpose: [
                "two tones 4 Hz apart (100 and 104 Hz): one peak or two",
                "two tones 1 Hz apart (100 and 101 Hz): resolved only by segments of a second \
                 or more",
                "a tone on a 4 Hz grid (100 Hz) and one off it (302.3 Hz), both of 30 deg/s: \
                 the width of the peak, the side lobes, and how much lower the second reads",
            ],
            flight: FlightSpec {
                roll: tones(21, &[(100.0, 15.0), (104.0, 15.0)]),
                pitch: tones(22, &[(100.0, 15.0), (101.0, 15.0)]),
                yaw: tones(23, &[(100.0, 30.0), (302.3, 30.0)]),
                ..quiet(2000.0)
            },
        },
        Probe {
            id: "s5-noise-2k",
            area: Area::Spectrum,
            title: "White noise of known power: density against amplitude, and the smoothing",
            purpose: [
                "white noise of 10 deg/s RMS: a flat line whose height says how the spectrum \
                 is normalised",
                "the same noise with a 152 Hz tone of 20 deg/s in it: how far each smoothing \
                 level spreads the line",
                "white noise of 3 deg/s RMS: 10.46 dB below the roll axis on any scale of \
                 power",
            ],
            flight: FlightSpec {
                roll: AxisSpec::of(vec![Wave::Noise {
                    rms: 10.0,
                    seed: 31,
                }]),
                pitch: AxisSpec::of(vec![
                    Wave::Noise {
                        rms: 10.0,
                        seed: 32,
                    },
                    Wave::tone(152.0, 20.0),
                ]),
                yaw: AxisSpec::of(vec![Wave::Noise { rms: 3.0, seed: 33 }]),
                ..quiet(2000.0)
            },
        },
        Probe {
            id: "s6-averaging-2k",
            area: Area::Spectrum,
            title: "A tone that is not always there: how the segments are averaged",
            purpose: [
                "a 100 Hz tone of 20 deg/s in the first half of the log only: 3 dB below the \
                 pitch axis if powers are averaged, far lower if decibels are",
                "the same tone over the whole log, to compare with",
                "the same tone for one second (10 s to 11 s): 14.5 dB below pitch for a mean \
                 of powers over 28 s, equal to it for a maximum",
            ],
            flight: FlightSpec {
                roll: AxisSpec::of(vec![burst(100.0, 20.0, 0.0, 15.0), dither(41)]),
                pitch: tones(42, &[(100.0, 20.0)]),
                yaw: AxisSpec::of(vec![burst(100.0, 20.0, 10.0, 11.0), dither(43)]),
                ..quiet(2000.0)
            },
        },
        Probe {
            id: "s7-low-2k",
            area: Area::Spectrum,
            title: "Offsets, drift and low tones: the lowest bins and the panel below 100 Hz",
            purpose: [
                "a constant 30 deg/s under a 20 Hz tone of 10 deg/s: whether the mean is \
                 removed",
                "a drift from -50 to 50 deg/s under the same tone: whether a trend is removed",
                "tones of 10 deg/s at 5, 20, 60 and 61 Hz: the resolution of the panel below \
                 100 Hz",
            ],
            flight: FlightSpec {
                roll: AxisSpec::of(vec![
                    Wave::Offset { value: 30.0 },
                    Wave::tone(20.0, 10.0),
                    dither(51),
                ]),
                pitch: AxisSpec::of(vec![
                    Wave::Ramp {
                        from: -50.0,
                        to: 50.0,
                    },
                    Wave::tone(20.0, 10.0),
                    dither(52),
                ]),
                yaw: tones(53, &[(5.0, 10.0), (20.0, 10.0), (60.0, 10.0), (61.0, 10.0)]),
                ..quiet(2000.0)
            },
        },
        Probe {
            id: "s8-channels-2k",
            area: Area::Spectrum,
            title: "Different tones before and after the filters: which curve is which channel",
            purpose: [
                "100 Hz on the unfiltered gyro, 248 Hz on the filtered one, both 20 deg/s",
                "the same 100 Hz tone of 20 deg/s on both: the two curves coincide",
                "100 Hz at 20 deg/s unfiltered and at 10 deg/s filtered: the curves are 6.02 \
                 dB apart",
            ],
            flight: FlightSpec {
                roll: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![Wave::tone(248.0, 20.0), dither(61)],
                    },
                    ..tones(61, &[(100.0, 20.0)])
                },
                pitch: tones(62, &[(100.0, 20.0)]),
                yaw: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![Wave::tone(100.0, 10.0), dither(63)],
                    },
                    ..tones(63, &[(100.0, 20.0)])
                },
                ..quiet(2000.0)
            },
        },
        Probe {
            id: "s9-bands-2k",
            area: Area::Spectrum,
            title: "Noise in bands of known density: what a smoothed curve does to width and edges",
            purpose: [
                "a band from 100 to 200 Hz with a density of 1 (deg/s)²/Hz: a plateau whose \
                 height does not depend on how narrow lines are treated",
                "three bands of that same density, 4, 20 and 80 Hz wide (at 100, 200 and 400 \
                 Hz): how wide a feature has to be to keep its height at each smoothing level",
                "a density of 0.1 from 20 to 150 Hz and from 200 to 900 Hz, nothing between: \
                 how each smoothing level blurs an edge",
            ],
            flight: FlightSpec {
                roll: AxisSpec::of(vec![band(100.0, 200.0, 1.0, 71), dither(71)]),
                pitch: AxisSpec::of(vec![
                    band(100.0, 104.0, 1.0, 72),
                    band(200.0, 220.0, 1.0, 73),
                    band(400.0, 480.0, 1.0, 74),
                    dither(72),
                ]),
                yaw: AxisSpec::of(vec![
                    band(20.0, 150.0, 0.1, 75),
                    band(200.0, 900.0, 0.1, 76),
                    dither(73),
                ]),
                ..quiet(2000.0)
            },
        },
    ]
}

/// The probe of round 2: one flight at several log rates and lengths.
fn mixed(id: &'static str, title: &'static str, fs: f64, duration_s: f64) -> Probe {
    Probe {
        id,
        area: Area::Spectrum,
        title,
        purpose: [
            "tones of 2, 8 and 32 deg/s at 30, 70 and 110 Hz: whether the width of a tone's \
             bump depends on its height",
            "a band of density 0.3 from 20 to 60 Hz next to an 80 Hz tone of 10 deg/s: the \
             level of the band and the shape of its edges",
            "white noise of 4 deg/s RMS on an offset of 20 deg/s: the floor, and the rise \
             towards 0 Hz",
        ],
        flight: FlightSpec {
            roll: tones(81, &[(30.0, 2.0), (70.0, 8.0), (110.0, 32.0)]),
            pitch: AxisSpec::of(vec![
                band(20.0, 60.0, 0.3, 82),
                Wave::tone(80.0, 10.0),
                dither(82),
            ]),
            yaw: AxisSpec::of(vec![
                Wave::Offset { value: 20.0 },
                Wave::Noise { rms: 4.0, seed: 83 },
            ]),
            ..FlightSpec::quiet(fs, duration_s)
        },
    }
}

/// Round 2: the spectrum again, to confirm the model of round 1
/// (`crate::model`) on a flight it was not fitted to, and to settle what
/// logs of one length and three rates could not: whether the smoothing
/// spans are counted in points or in hertz, and where the analysed span of
/// a log starts.
fn round_2() -> Vec<Probe> {
    vec![
        mixed(
            "c1-mixed-2k",
            "The flight of round 2 as round 1 had its logs: 2 kHz, 30 s",
            2000.0,
            SPECTRUM_S,
        ),
        mixed(
            "c2-mixed-1k",
            "The same at 1 kHz: the spans and the start of the analysed span against the rate",
            1000.0,
            SPECTRUM_S,
        ),
        mixed("c3-mixed-4k", "The same at 4 kHz", 4000.0, SPECTRUM_S),
        mixed(
            "c4-short-2k",
            "12 s at 2 kHz: points of 0.1 Hz, so a span in points and a span in hertz differ",
            2000.0,
            12.0,
        ),
        mixed(
            "c5-long-2k",
            "47 s at 2 kHz: points of 0.022 Hz",
            2000.0,
            47.0,
        ),
        mixed(
            "c6-short-8k",
            "12 s at 8 kHz: the highest log rate, and a rate and a length changed together",
            8000.0,
            12.0,
        ),
    ]
}

/// One axis of the flight of round 3: the craft follows random stick
/// moves, and its gyro carries motor noise that moves with the throttle, a
/// frame resonance and sensor noise. The filtered gyro has less of each, as
/// after the firmware's filters.
fn flown(seed: u64, stick_deg_s: f64, resonance_hz: f64) -> AxisSpec {
    let noise = |amplitude: f64, floor: f64| {
        vec![
            Wave::ThrottleTone {
                hz_at_zero: 70.0,
                hz_per_percent: 2.5,
                amplitude: 12.0 * amplitude,
                band: None,
            },
            Wave::ThrottleTone {
                hz_at_zero: 140.0,
                hz_per_percent: 5.0,
                amplitude: 5.0 * amplitude,
                band: None,
            },
            Wave::tone(resonance_hz, 4.0 * amplitude),
            Wave::Noise { rms: floor, seed },
        ]
    };
    AxisSpec {
        stick: Stick::Random {
            seed: seed + 100,
            hold_min_ms: 150,
            hold_max_ms: 900,
            amplitude: stick_deg_s,
        },
        plant: Plant::SecondOrder {
            natural_hz: 14.0,
            damping: 0.6,
            delay_ms: 4.0,
            gain: 1.0,
        },
        waves: noise(1.0, 2.0),
        filtered: Filtered::Separate {
            waves: noise(0.4, 1.0),
        },
    }
}

/// The flight of round 3 at a log rate and a length.
fn flight(id: &'static str, title: &'static str, fs: f64, duration_s: f64) -> Probe {
    Probe {
        id,
        area: Area::Spectrum,
        title,
        purpose: [
            "stick moves of up to 300 deg/s, motor noise from 120 to 270 Hz and its second \
             harmonic, a resonance at 410 Hz",
            "the same with stick moves of up to 250 deg/s and the resonance at 380 Hz",
            "the same with stick moves of up to 120 deg/s and the resonance at 330 Hz",
        ],
        flight: FlightSpec {
            fs,
            duration_s,
            roll: flown(91, 300.0, 410.0),
            pitch: flown(92, 250.0, 380.0),
            yaw: flown(93, 120.0, 330.0),
            throttle: Throttle::Stairs {
                steps: vec![
                    (20.0, 2.0),
                    (45.0, 3.0),
                    (70.0, 2.5),
                    (35.0, 1.5),
                    (80.0, 2.0),
                    (55.0, 3.0),
                ],
            },
        },
    }
}

/// Round 3: the hold-out. A flight as a craft flies it, which the model
/// (`crate::model`) was not fitted to, at four log rates and four lengths
/// none of which the earlier rounds had; and one probe of round 1 again,
/// to see whether the start of the analysed span follows the log or the
/// company it is loaded in.
fn round_3() -> Vec<Probe> {
    let again = round_1()
        .into_iter()
        .find(|p| p.id == "s9-bands-2k")
        .map(|p| Probe {
            id: "h5-bands-2k",
            title: "s9-bands-2k of round 1 again, loaded with other logs this time",
            ..p
        });
    let mut probes = vec![
        flight(
            "h1-flight-2k",
            "A flight with stick moves, motor noise and a resonance: 2 kHz, 25 s",
            2000.0,
            25.0,
        ),
        flight("h2-flight-1k", "The same at 1 kHz, 18 s", 1000.0, 18.0),
        flight("h3-flight-4k", "The same at 4 kHz, 36 s", 4000.0, 36.0),
        flight("h4-flight-8k", "The same at 8 kHz, 21 s", 8000.0, 21.0),
    ];
    probes.extend(again);
    probes
}

/// The probes of `round`, or `None` for a round that does not exist.
pub fn round(round: u32) -> Option<Vec<Probe>> {
    match round {
        1 => Some(round_1()),
        2 => Some(round_2()),
        3 => Some(round_3()),
        _ => None,
    }
}

/// The rounds that exist.
pub const ROUNDS: &[u32] = &[1, 2, 3];

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::Axis;
    use std::collections::BTreeSet;

    #[test]
    fn every_round_has_probes_with_unique_file_names() {
        assert!(round(0).is_none());
        assert!(round(99).is_none());
        for r in ROUNDS {
            let probes = round(*r).unwrap();
            assert!(!probes.is_empty());
            let ids: BTreeSet<&str> = probes.iter().map(|p| p.id).collect();
            assert_eq!(ids.len(), probes.len(), "round {r}: ids repeat");
            for p in &probes {
                let name_ok =
                    p.id.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                assert!(name_ok, "{}", p.id);
                assert!(!p.title.is_empty() && p.purpose.iter().all(|s| !s.is_empty()));
                assert!(!p.flight.is_empty(), "{}", p.id);
            }
        }
    }

    #[test]
    fn spectrum_probes_keep_the_sticks_still_and_stay_inside_the_gyro_range() {
        for p in [1, 2].iter().flat_map(|r| round(*r).unwrap()) {
            assert_eq!(p.area, Area::Spectrum);
            for axis in Axis::ALL {
                assert_eq!(p.flight.axis(axis).stick, Stick::Silence, "{}", p.id);
                let s = p.flight.samples(axis);
                let peak = s
                    .unfiltered
                    .iter()
                    .chain(&s.filtered)
                    .fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(peak < 2000.0, "{} {axis:?}: {peak} deg/s", p.id);
                // every tone is below the Nyquist frequency of its log
                for wave in &p.flight.axis(axis).waves {
                    if let Wave::Tone { hz, .. } = wave {
                        assert!(*hz < p.flight.fs / 2.0, "{} {axis:?}: {hz} Hz", p.id);
                    }
                }
            }
        }
    }

    #[test]
    fn the_level_probe_is_the_same_flight_at_every_rate() {
        let probes = round(1).unwrap();
        let at = |id: &str| probes.iter().find(|p| p.id == id).unwrap().flight.clone();
        let base = at("s1-levels-2k");
        for (id, fs) in [("s2-levels-1k", 1000.0), ("s3-levels-4k", 4000.0)] {
            let other = at(id);
            assert_eq!(other.fs, fs);
            assert_eq!(
                FlightSpec {
                    fs: 2000.0,
                    ..other
                },
                base,
                "{id}"
            );
        }
    }

    #[test]
    fn round_2_is_one_flight_at_several_rates_and_lengths() {
        let probes = round(2).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("c1-mixed-2k", 2000.0, 30.0),
                ("c2-mixed-1k", 1000.0, 30.0),
                ("c3-mixed-4k", 4000.0, 30.0),
                ("c4-short-2k", 2000.0, 12.0),
                ("c5-long-2k", 2000.0, 47.0),
                ("c6-short-8k", 8000.0, 12.0),
            ]
        );
        let base = probes[0].flight.clone();
        for p in &probes[1..] {
            let same = FlightSpec {
                fs: base.fs,
                duration_s: base.duration_s,
                ..p.flight.clone()
            };
            assert_eq!(same, base, "{}", p.id);
        }
        // every tone and the band are below the lowest Nyquist frequency
        for wave in base.roll.waves.iter().chain(&base.pitch.waves) {
            match wave {
                Wave::Tone { hz, .. } => assert!(*hz < 500.0),
                Wave::Band { to_hz, .. } => assert!(*to_hz < 500.0),
                _ => {}
            }
        }
    }

    #[test]
    fn round_3_is_a_flight_with_sticks_and_a_probe_of_round_1_again() {
        let probes = round(3).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("h1-flight-2k", 2000.0, 25.0),
                ("h2-flight-1k", 1000.0, 18.0),
                ("h3-flight-4k", 4000.0, 36.0),
                ("h4-flight-8k", 8000.0, 21.0),
                ("h5-bands-2k", 2000.0, 30.0),
            ]
        );
        // no length of the earlier rounds
        for p in &probes[..4] {
            for earlier in [1, 2].iter().flat_map(|r| round(*r).unwrap()) {
                assert_ne!(p.flight.len(), earlier.flight.len(), "{}", p.id);
            }
        }
        let s9 = round(1).unwrap().pop().unwrap();
        assert_eq!((s9.id, &s9.flight), ("s9-bands-2k", &probes[4].flight));

        // the craft moves, and the gyro stays inside what a log holds
        let h1 = &probes[0].flight;
        for axis in Axis::ALL {
            let spec = h1.axis(axis);
            assert!(matches!(spec.stick, Stick::Random { .. }));
            let s = h1.samples(axis);
            let peak = s.unfiltered.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            assert!(peak > 100.0 && peak < 2000.0, "{axis:?}: {peak}");
            assert!(s.setpoint.iter().any(|v| *v != 0.0));
            // the filtered gyro is the quieter of the two
            let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
            let noise = |x: &[f64]| {
                let d: Vec<f64> = x.iter().zip(&s.response).map(|(a, b)| a - b).collect();
                rms(&d)
            };
            assert!(noise(&s.filtered) < 0.6 * noise(&s.unfiltered), "{axis:?}");
            // every tone below the lowest Nyquist frequency of the round
            for wave in &spec.waves {
                match wave {
                    Wave::Tone { hz, .. } => assert!(*hz < 500.0),
                    Wave::ThrottleTone {
                        hz_at_zero,
                        hz_per_percent,
                        ..
                    } => assert!(hz_at_zero + 100.0 * hz_per_percent < 4000.0),
                    _ => {}
                }
            }
        }
    }
}
