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
    /// The step response: how the craft follows the stick.
    StepResponse,
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

/// Length of a step-response probe, seconds.
const STEP_S: f64 = 30.0;

/// The plant most step-response probes fly: a second-order response that
/// overshoots by a quarter, 5 ms late.
const UNDERDAMPED: Plant = Plant::SecondOrder {
    natural_hz: 15.0,
    damping: 0.4,
    delay_ms: 5.0,
    gain: 1.0,
};

/// An axis that follows random stick moves through `plant`: whole levels
/// within `±amplitude`, each held between `hold_ms.0` and `hold_ms.1`, with
/// white noise of `noise` deg/s RMS on the gyro (none when zero).
fn moved(seed: u64, amplitude: f64, hold_ms: (u32, u32), plant: Plant, noise: f64) -> AxisSpec {
    AxisSpec {
        waves: if noise > 0.0 {
            vec![Wave::Noise { rms: noise, seed }]
        } else {
            Vec::new()
        },
        ..AxisSpec::following(
            Stick::Random {
                seed: seed + 1000,
                hold_min_ms: hold_ms.0,
                hold_max_ms: hold_ms.1,
                amplitude,
            },
            plant,
        )
    }
}

/// The plant probe at a log rate and a length: three plants whose step
/// responses are known in closed form.
fn plants(id: &'static str, title: &'static str, fs: f64, duration_s: f64) -> Probe {
    plants_moved(id, title, fs, duration_s, 101)
}

/// [`plants`] with the stick moves of another seed.
fn plants_moved(
    id: &'static str,
    title: &'static str,
    fs: f64,
    duration_s: f64,
    seed: u64,
) -> Probe {
    Probe {
        id,
        area: Area::StepResponse,
        title,
        purpose: [
            "a pure delay of 10 ms: the true response is a step at 10 ms, with no overshoot; \
             what a tool draws shows its own rounding of an edge and its latency rule",
            "a first-order response (time constant 20 ms) after 5 ms: no overshoot, half way \
             at 18.9 ms",
            "a second-order response (15 Hz, damping 0.4) after 5 ms: a peak of 1.254",
        ],
        flight: FlightSpec {
            roll: moved(
                seed,
                200.0,
                (150, 700),
                Plant::Delay { delay_ms: 10.0 },
                0.0,
            ),
            pitch: moved(
                seed + 1,
                200.0,
                (150, 700),
                Plant::FirstOrder {
                    tau_ms: 20.0,
                    delay_ms: 5.0,
                    gain: 1.0,
                },
                0.0,
            ),
            yaw: moved(seed + 2, 200.0, (150, 700), UNDERDAMPED, 0.0),
            ..FlightSpec::quiet(fs, duration_s)
        },
    }
}

/// A stick that is still but for the seconds from `from_s` to `to_s`, where
/// it moves every quarter of a second.
fn busy_between(from_s: f64, to_s: f64, amplitude: f64) -> Stick {
    let quarters = (STEP_S * 4.0) as usize;
    let mut state = 12_345u64;
    let levels = (0..quarters)
        .map(|k| {
            // a fixed sequence of whole levels within the amplitude
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state >> 33) % 2001) as f64 / 1000.0 - 1.0;
            let t = k as f64 / 4.0;
            if t >= from_s && t < to_s {
                (unit * amplitude).round()
            } else {
                0.0
            }
        })
        .collect();
    Stick::Steps {
        start_s: 0.0,
        every_s: 0.25,
        levels,
    }
}

/// Round 4, the first of the step response: what the reference tool draws
/// for plants whose true response is known, and what its curve, its window
/// count, its peak and its latency depend on.
fn round_4() -> Vec<Probe> {
    let quiet = FlightSpec::quiet(2000.0, STEP_S);
    let hold = (150, 700);
    vec![
        plants(
            "t1-plants-2k",
            "Three plants of known step response: a delay, a first-order and a second-order one",
            2000.0,
            STEP_S,
        ),
        plants("t2-plants-1k", "The same at 1 kHz", 1000.0, STEP_S),
        plants("t3-plants-4k", "The same at 4 kHz", 4000.0, STEP_S),
        Probe {
            id: "t4-amplitude-2k",
            area: Area::StepResponse,
            title: "One plant, stick moves of three sizes: is the response the same for each",
            purpose: [
                "stick moves within 25 deg/s: below what a tool may ask of a window",
                "stick moves within 100 deg/s",
                "stick moves within 400 deg/s",
            ],
            flight: FlightSpec {
                roll: moved(111, 25.0, hold, UNDERDAMPED, 0.0),
                pitch: moved(112, 100.0, hold, UNDERDAMPED, 0.0),
                yaw: moved(113, 400.0, hold, UNDERDAMPED, 0.0),
                ..quiet.clone()
            },
        },
        Probe {
            id: "t5-noise-2k",
            area: Area::StepResponse,
            title: "One plant under gyro noise of three levels: what the noise does to the curve",
            purpose: [
                "gyro noise of 0.5 deg/s RMS",
                "gyro noise of 5 deg/s RMS",
                "gyro noise of 20 deg/s RMS",
            ],
            flight: FlightSpec {
                roll: moved(121, 200.0, hold, UNDERDAMPED, 0.5),
                pitch: moved(122, 200.0, hold, UNDERDAMPED, 5.0),
                yaw: moved(123, 200.0, hold, UNDERDAMPED, 20.0),
                ..quiet.clone()
            },
        },
        Probe {
            id: "t6-holds-2k",
            area: Area::StepResponse,
            title: "One plant, the stick held for short, medium and long times",
            purpose: [
                "a new level every 40 to 120 ms: the craft never settles",
                "a new level every 300 to 900 ms",
                "a new level every 1.5 to 3 s: few moves, long rests",
            ],
            flight: FlightSpec {
                roll: moved(131, 200.0, (40, 120), UNDERDAMPED, 0.0),
                pitch: moved(132, 200.0, (300, 900), UNDERDAMPED, 0.0),
                yaw: moved(133, 200.0, (1500, 3000), UNDERDAMPED, 0.0),
                ..quiet.clone()
            },
        },
        Probe {
            id: "t7-gain-2k",
            area: Area::StepResponse,
            title: "A craft that flies less, as much and more than asked: where the curve settles",
            purpose: [
                "the craft reaches 0.8 of the rate asked for",
                "the craft reaches the rate asked for",
                "the craft reaches 1.25 of the rate asked for",
            ],
            flight: FlightSpec {
                roll: moved(
                    141,
                    200.0,
                    hold,
                    Plant::SecondOrder {
                        natural_hz: 15.0,
                        damping: 0.4,
                        delay_ms: 5.0,
                        gain: 0.8,
                    },
                    0.0,
                ),
                pitch: moved(142, 200.0, hold, UNDERDAMPED, 0.0),
                yaw: moved(
                    143,
                    200.0,
                    hold,
                    Plant::SecondOrder {
                        natural_hz: 15.0,
                        damping: 0.4,
                        delay_ms: 5.0,
                        gain: 1.25,
                    },
                    0.0,
                ),
                ..quiet.clone()
            },
        },
        Probe {
            id: "t8-sparse-2k",
            area: Area::StepResponse,
            title: "Stick moves in parts of the flight only: which parts a tool takes its windows \
                    from, and how many",
            purpose: [
                "moves from 5 to 10 s, a still stick before and after",
                "moves from 20 to 22 s only",
                "a move every 2 s, between 150 and -150 deg/s",
            ],
            flight: FlightSpec {
                roll: AxisSpec::following(busy_between(5.0, 10.0, 200.0), UNDERDAMPED),
                pitch: AxisSpec::following(busy_between(20.0, 22.0, 200.0), UNDERDAMPED),
                yaw: AxisSpec::following(
                    Stick::Steps {
                        start_s: 1.0,
                        every_s: 2.0,
                        levels: vec![150.0, -150.0],
                    },
                    UNDERDAMPED,
                ),
                ..quiet
            },
        },
        plants(
            "t9-short-2k",
            "The three plants in a flight of 12 s",
            2000.0,
            12.0,
        ),
    ]
}

/// A plant that creeps to its level: a first-order response with a time
/// constant of 100 ms. Its curve has no peak before its end, so the "Peak"
/// a tool reads from it tells how far along the curve the tool looks.
const CREEPING: Plant = Plant::FirstOrder {
    tau_ms: 100.0,
    delay_ms: 5.0,
    gain: 1.0,
};

/// A stick that takes a new level every quarter of a second, always
/// `size` deg/s from its centre, to one side or the other.
fn off_centre(seed: u64, size: f64, duration_s: f64) -> Stick {
    let mut state = seed;
    let levels = (0..(duration_s * 4.0) as usize)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            if (state >> 40) & 1 == 0 { size } else { -size }
        })
        .collect();
    Stick::Steps {
        start_s: 0.0,
        every_s: 0.25,
        levels,
    }
}

/// The probe that asks what follows the log rate: a pure delay for the
/// smoothing levels, the creeping plant for the peak, and a noisy axis
/// whose windows lie near the limits a window has to keep.
fn rates(id: &'static str, title: &'static str, fs: f64, seed: u64) -> Probe {
    let hold = (150, 700);
    Probe {
        id,
        area: Area::StepResponse,
        title,
        purpose: [
            "a pure delay of 10 ms: how wide each smoothing level blurs the step at this rate",
            "a first-order response with a time constant of 100 ms: the curve rises to its \
             end, so the peak a tool reads tells where it stops looking",
            "the second-order plant under gyro noise of 20 deg/s RMS: windows near the limits \
             a window has to keep, to tell a limit in milliseconds from one in samples",
        ],
        flight: FlightSpec {
            roll: moved(seed, 200.0, hold, Plant::Delay { delay_ms: 10.0 }, 0.0),
            pitch: moved(seed + 1, 200.0, hold, CREEPING, 0.0),
            yaw: moved(seed + 2, 200.0, hold, UNDERDAMPED, 20.0),
            ..FlightSpec::quiet(fs, STEP_S)
        },
    }
}

/// Round 5, the second of the step response: the model of round 4
/// (`step_model`) predicts every plot of these logs before they are run.
/// New stick moves throughout, and probes for what round 4 left open: what
/// follows the log rate, how far the peak is looked for, the stick a
/// window needs, and where the first window starts.
fn round_5() -> Vec<Probe> {
    let quiet = FlightSpec::quiet(2000.0, STEP_S);
    let slow = |natural_hz: f64| Plant::SecondOrder {
        natural_hz,
        damping: 0.5,
        delay_ms: 5.0,
        gain: 1.0,
    };
    vec![
        plants_moved(
            "u1-plants-2k",
            "The three plants again, with other stick moves, in 26 s",
            2000.0,
            26.0,
            201,
        ),
        rates(
            "u2-rates-1k",
            "At 1 kHz: a delay, a creeping plant and a noisy axis",
            1000.0,
            211,
        ),
        rates("u3-rates-4k", "The same at 4 kHz", 4000.0, 221),
        Probe {
            id: "u4-slow-2k",
            area: Area::StepResponse,
            title: "Slow plants: how far along the curve the peak is looked for",
            purpose: [
                "a first-order response with a time constant of 100 ms: no peak before the end",
                "a second-order response of 2.5 Hz, damping 0.5: a peak of 1.163 at 236 ms",
                "a second-order response of 4 Hz, damping 0.5: a peak of 1.163 at 149 ms",
            ],
            flight: FlightSpec {
                roll: moved(231, 200.0, (150, 700), CREEPING, 0.0),
                pitch: moved(232, 200.0, (150, 700), slow(2.5), 0.0),
                yaw: moved(233, 200.0, (150, 700), slow(4.0), 0.0),
                ..quiet.clone()
            },
        },
        Probe {
            id: "u5-stick-2k",
            area: Area::StepResponse,
            title: "A stick always the same distance from its centre: what a window needs",
            purpose: [
                "always 19 deg/s from the centre: the mean of every window is 19",
                "always 21 deg/s from the centre",
                "always 20 deg/s from the centre: exactly the value round 4 points to",
            ],
            flight: FlightSpec {
                roll: AxisSpec::following(off_centre(241, 19.0, STEP_S), UNDERDAMPED),
                pitch: AxisSpec::following(off_centre(242, 21.0, STEP_S), UNDERDAMPED),
                yaw: AxisSpec::following(off_centre(243, 20.0, STEP_S), UNDERDAMPED),
                ..quiet
            },
        },
        plants_moved(
            "u6-short-2k",
            "The three plants in 12 s: where the first window starts",
            2000.0,
            12.0,
            251,
        ),
        plants_moved("u7-mid-2k", "The three plants in 18 s", 2000.0, 18.0, 261),
    ]
}

/// One axis of a hold-out flight of the step response: the largest rate
/// the stick asks for, deg/s; the shortest and the longest time it holds a
/// level, ms; the plant; the gyro noise, deg/s RMS.
type Flown = (f64, (u32, u32), Plant, f64);

/// A flight of the hold-out of the step response: on every axis random
/// stick moves through a plant of its own, under a little gyro noise. The
/// axes take the seeds `seed`, `seed + 1` and `seed + 2`.
fn held_out(
    id: &'static str,
    title: &'static str,
    purpose: [&'static str; 3],
    (fs, duration_s): (f64, f64),
    seed: u64,
    axes: [Flown; 3],
) -> Probe {
    let [roll, pitch, yaw] = [0, 1, 2].map(|k: usize| {
        let (amplitude, hold_ms, plant, noise) = axes[k];
        moved(seed + k as u64, amplitude, hold_ms, plant, noise)
    });
    Probe {
        id,
        area: Area::StepResponse,
        title,
        purpose,
        flight: FlightSpec {
            roll,
            pitch,
            yaw,
            ..FlightSpec::quiet(fs, duration_s)
        },
    }
}

/// Round 6, the hold-out of the step response: flights made for no
/// question in particular, as a craft flies them, which the model
/// (`step_model`) was not fitted to. Fifteen plants none of which an
/// earlier round flew, each behind a delay; stick moves of new seeds; a
/// little gyro noise on every axis; log lengths no earlier round had; and
/// 8 kHz, a log rate no step-response round had.
fn round_6() -> Vec<Probe> {
    let second = |natural_hz: f64, damping: f64, delay_ms: f64| Plant::SecondOrder {
        natural_hz,
        damping,
        delay_ms,
        gain: 1.0,
    };
    let first = |tau_ms: f64, delay_ms: f64| Plant::FirstOrder {
        tau_ms,
        delay_ms,
        gain: 1.0,
    };
    vec![
        held_out(
            "w1-flight-2k",
            "A flight with stick moves, a plant of its own on every axis and a little gyro \
             noise: 2 kHz, 34 s",
            [
                "stick moves within 300 deg/s; a second-order response of 22 Hz, damping 0.7, \
                 after 3 ms; gyro noise of 3 deg/s RMS",
                "stick moves within 250 deg/s; a second-order response of 18 Hz, damping 0.55, \
                 after 4 ms; gyro noise of 3 deg/s RMS",
                "stick moves within 120 deg/s; a first-order response with a time constant of \
                 30 ms after 6 ms; gyro noise of 2 deg/s RMS",
            ],
            (2000.0, 34.0),
            301,
            [
                (300.0, (150, 900), second(22.0, 0.7, 3.0), 3.0),
                (250.0, (150, 900), second(18.0, 0.55, 4.0), 3.0),
                (120.0, (150, 900), first(30.0, 6.0), 2.0),
            ],
        ),
        held_out(
            "w2-flight-1k",
            "Another flight, with other plants: 1 kHz, 22 s",
            [
                "stick moves within 280 deg/s; a second-order response of 12 Hz, damping 0.45, \
                 after 6 ms; gyro noise of 4 deg/s RMS",
                "stick moves within 220 deg/s; a second-order response of 9 Hz, damping 0.8, \
                 after 7 ms; gyro noise of 3 deg/s RMS",
                "stick moves within 150 deg/s; a second-order response of 16 Hz, damping 0.35, \
                 after 5 ms; gyro noise of 2.5 deg/s RMS",
            ],
            (1000.0, 22.0),
            311,
            [
                (280.0, (200, 800), second(12.0, 0.45, 6.0), 4.0),
                (220.0, (200, 800), second(9.0, 0.8, 7.0), 3.0),
                (150.0, (200, 800), second(16.0, 0.35, 5.0), 2.5),
            ],
        ),
        held_out(
            "w3-flight-4k",
            "Another flight, with other plants: 4 kHz, 16 s",
            [
                "stick moves within 350 deg/s; a second-order response of 25 Hz, damping 0.6, \
                 after 2.5 ms; gyro noise of 5 deg/s RMS",
                "stick moves within 260 deg/s; a first-order response with a time constant of \
                 15 ms after 4 ms; gyro noise of 4 deg/s RMS",
                "stick moves within 100 deg/s; a second-order response of 10 Hz, damping 0.5, \
                 after 8 ms; gyro noise of 2 deg/s RMS",
            ],
            (4000.0, 16.0),
            321,
            [
                (350.0, (120, 600), second(25.0, 0.6, 2.5), 5.0),
                (260.0, (120, 600), first(15.0, 4.0), 4.0),
                (100.0, (120, 600), second(10.0, 0.5, 8.0), 2.0),
            ],
        ),
        held_out(
            "w4-flight-8k",
            "Another flight, with other plants: 8 kHz, 14 s",
            [
                "stick moves within 320 deg/s; a second-order response of 20 Hz, damping 0.3, \
                 after 3.5 ms; gyro noise of 5 deg/s RMS",
                "stick moves within 240 deg/s; a second-order response of 13 Hz, damping 0.65, \
                 after 4.5 ms; gyro noise of 3 deg/s RMS",
                "stick moves within 140 deg/s; a second-order response of 8 Hz, damping 0.75, \
                 after 6.5 ms; gyro noise of 2 deg/s RMS",
            ],
            (8000.0, 14.0),
            331,
            [
                (320.0, (150, 900), second(20.0, 0.3, 3.5), 5.0),
                (240.0, (150, 900), second(13.0, 0.65, 4.5), 3.0),
                (140.0, (150, 900), second(8.0, 0.75, 6.5), 2.0),
            ],
        ),
        held_out(
            "w5-mixed-2k",
            "A flight of mixed flying, quick moves, long holds and small moves: 2 kHz, 41 s",
            [
                "quick stick moves within 400 deg/s, a new one every 80 to 400 ms; a \
                 second-order response of 24 Hz, damping 0.4, after 2 ms; gyro noise of 5 deg/s \
                 RMS",
                "stick moves within 200 deg/s, held for 300 to 1200 ms; a first-order response \
                 with a time constant of 45 ms after 5 ms; gyro noise of 3 deg/s RMS",
                "small stick moves within 90 deg/s, held for 150 to 2000 ms; a second-order \
                 response of 11 Hz, damping 0.7, after 7.5 ms; gyro noise of 4 deg/s RMS",
            ],
            (2000.0, 41.0),
            341,
            [
                (400.0, (80, 400), second(24.0, 0.4, 2.0), 5.0),
                (200.0, (300, 1200), first(45.0, 5.0), 3.0),
                (90.0, (150, 2000), second(11.0, 0.7, 7.5), 4.0),
            ],
        ),
    ]
}

/// A probe of round 7: at 200 deg/s of random stick moves, each axis
/// follows a plant of its own (roll, pitch, yaw) under gyro noise of the
/// given RMS, deg/s. The axes take the seeds `seed`, `seed + 1` and
/// `seed + 2`.
fn answered(
    id: &'static str,
    title: &'static str,
    purpose: [&'static str; 3],
    fs: f64,
    seed: u64,
    axes: [(Plant, f64); 3],
) -> Probe {
    let [roll, pitch, yaw] = [0, 1, 2].map(|k: usize| {
        let (plant, noise) = axes[k];
        moved(seed + k as u64, 200.0, (150, 700), plant, noise)
    });
    Probe {
        id,
        area: Area::StepResponse,
        title,
        purpose,
        flight: FlightSpec {
            roll,
            pitch,
            yaw,
            ..FlightSpec::quiet(fs, STEP_S)
        },
    }
}

/// The probe of round 7 with one natural frequency at three dampings, at a
/// log rate: the same plants and the same stick moves at every rate.
fn dampings(id: &'static str, title: &'static str, fs: f64) -> Probe {
    let plant = |damping: f64| Plant::SecondOrder {
        natural_hz: 20.0,
        damping,
        delay_ms: 4.0,
        gain: 1.0,
    };
    answered(
        id,
        title,
        [
            "a second-order response of 20 Hz, damping 0.3, after 4 ms: it overshoots by 37 %",
            "the same with damping 0.5: it overshoots by 16 %",
            "the same with damping 0.9: it all but does not overshoot",
        ],
        fs,
        431,
        [(plant(0.3), 0.0), (plant(0.5), 0.0), (plant(0.9), 0.0)],
    )
}

/// Round 7, the first about the numbers version 0.94 of the reference tool
/// reads off a step response besides its peak and its latency: rise time,
/// bandwidth, the peak of the sensitivity and its frequency, phase margin
/// and crossover. The plants are those whose answers are known in closed
/// form (`crate::truth`; the manifest carries them), flown without gyro
/// noise so that the mean curve is as clean as a log allows. The round
/// identifies: the pack predicts the step response as before and nothing
/// of the new numbers.
fn round_7() -> Vec<Probe> {
    let delay = |delay_ms: f64| (Plant::Delay { delay_ms }, 0.0);
    let first = |tau_ms: f64| {
        let plant = Plant::FirstOrder {
            tau_ms,
            delay_ms: 0.0,
            gain: 1.0,
        };
        (plant, 0.0)
    };
    let second = |natural_hz: f64, delay_ms: f64, gain: f64, noise: f64| {
        let plant = Plant::SecondOrder {
            natural_hz,
            damping: 0.5,
            delay_ms,
            gain,
        };
        (plant, noise)
    };
    vec![
        answered(
            "y1-delays-2k",
            "Pure delays: the sensitivity peak, the crossover and the phase margin in closed form",
            [
                "a pure delay of 10 ms: the sensitivity peaks at 6.02 dB at 50 Hz, the open loop \
                 crosses 1 at 16.67 Hz with a phase margin of 60 degrees, the gain never falls",
                "a pure delay of 20 ms: the same peak at 25 Hz, the crossover at 8.33 Hz, the \
                 same phase margin",
                "a pure delay of 5 ms: the same peak at 100 Hz, the crossover at 33.33 Hz, the \
                 same phase margin",
            ],
            2000.0,
            401,
            [delay(10.0), delay(20.0), delay(5.0)],
        ),
        answered(
            "y2-first-2k",
            "First-order plants without a delay: the bandwidth in closed form",
            [
                "a first-order response with a time constant of 10 ms: a bandwidth of 15.92 Hz",
                "a first-order response with a time constant of 30 ms: a bandwidth of 5.31 Hz",
                "a first-order response with a time constant of 80 ms: a bandwidth of 1.99 Hz",
            ],
            2000.0,
            411,
            [first(10.0), first(30.0), first(80.0)],
        ),
        answered(
            "y3-gain-2k",
            "One plant at three gains: whether -3 dB is counted from the gain at 0 Hz or from 1",
            [
                "a second-order response of 15 Hz, damping 0.5, after 5 ms, that reaches 0.8 of \
                 the rate asked for",
                "the same plant reaching the rate asked for",
                "the same plant reaching 1.2 of the rate asked for",
            ],
            2000.0,
            421,
            [
                second(15.0, 5.0, 0.8, 0.0),
                second(15.0, 5.0, 1.0, 0.0),
                second(15.0, 5.0, 1.2, 0.0),
            ],
        ),
        dampings(
            "y4-damping-2k",
            "One natural frequency at three dampings; also from windows of 1 s and 3 s, and \
             smoothed",
            2000.0,
        ),
        dampings(
            "y5-rate-1k",
            "The plants and stick moves of y4-damping-2k at 1 kHz",
            1000.0,
        ),
        dampings("y6-rate-4k", "The same at 4 kHz", 4000.0),
        dampings("y7-rate-8k", "The same at 8 kHz", 8000.0),
        answered(
            "y8-noise-2k",
            "One plant under gyro noise of three levels: what the noise does to the numbers",
            [
                "a second-order response of 20 Hz, damping 0.5, after 4 ms, without gyro noise",
                "the same plant under gyro noise of 5 deg/s RMS",
                "the same plant under gyro noise of 20 deg/s RMS",
            ],
            2000.0,
            441,
            [
                second(20.0, 4.0, 1.0, 0.0),
                second(20.0, 4.0, 1.0, 5.0),
                second(20.0, 4.0, 1.0, 20.0),
            ],
        ),
    ]
}

/// The probes of `round`, or `None` for a round that does not exist.
pub fn round(round: u32) -> Option<Vec<Probe>> {
    match round {
        1 => Some(round_1()),
        2 => Some(round_2()),
        3 => Some(round_3()),
        4 => Some(round_4()),
        5 => Some(round_5()),
        6 => Some(round_6()),
        7 => Some(round_7()),
        _ => None,
    }
}

/// The rounds that exist.
pub const ROUNDS: &[u32] = &[1, 2, 3, 4, 5, 6, 7];

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

    #[test]
    fn round_4_flies_plants_of_known_step_response() {
        let probes = round(4).unwrap();
        let ids: Vec<&str> = probes.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            [
                "t1-plants-2k",
                "t2-plants-1k",
                "t3-plants-4k",
                "t4-amplitude-2k",
                "t5-noise-2k",
                "t6-holds-2k",
                "t7-gain-2k",
                "t8-sparse-2k",
                "t9-short-2k",
            ]
        );
        for p in &probes {
            assert_eq!(p.area, Area::StepResponse);
            for axis in Axis::ALL {
                let spec = p.flight.axis(axis);
                assert_ne!(spec.stick, Stick::Silence, "{} {axis:?}", p.id);
                assert_ne!(spec.plant, Plant::None, "{} {axis:?}", p.id);
                let s = p.flight.samples(axis);
                let peak = s.unfiltered.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(peak > 20.0 && peak < 2000.0, "{} {axis:?}: {peak}", p.id);
                // the setpoint is whole deg/s, as a log keeps it
                assert!(s.setpoint.iter().all(|v| v.fract() == 0.0));
            }
        }
        // what the titles promise about the plants
        let yaw = probes[0].flight.yaw.plant;
        assert!(
            (yaw.overshoot() - 0.254).abs() < 0.001,
            "{}",
            yaw.overshoot()
        );
        let pitch = probes[0].flight.pitch.plant;
        assert!((pitch.step_response(0.005 + 0.02 * 2.0f64.ln()) - 0.5).abs() < 1.0e-9);
        let roll = probes[0].flight.roll.plant;
        assert_eq!(roll.step_response(0.0099), 0.0);
        assert_eq!(roll.step_response(0.0101), 1.0);
        // the sparse probe is still outside its busy seconds
        let sparse = probes[7].flight.samples(Axis::Roll).setpoint;
        assert!(sparse[..9_990].iter().all(|v| *v == 0.0));
        assert!(sparse[10_000..20_000].iter().any(|v| *v != 0.0));
        assert!(sparse[20_010..].iter().all(|v| *v == 0.0));
        let late = probes[7].flight.samples(Axis::Pitch).setpoint;
        assert!(
            late[..39_990].iter().all(|v| *v == 0.0) && late[44_010..].iter().all(|v| *v == 0.0)
        );
        assert!(late[40_000..44_000].iter().any(|v| *v != 0.0));
    }

    /// The plants and the seeds (of stick moves and of noise) of `probes`.
    fn flown_in(probes: &[Probe]) -> (Vec<Plant>, BTreeSet<u64>) {
        let mut plants = Vec::new();
        let mut seeds = BTreeSet::new();
        for spec in probes
            .iter()
            .flat_map(|p| Axis::ALL.map(|axis| p.flight.axis(axis)))
        {
            plants.push(spec.plant);
            if let Stick::Random { seed, .. } = spec.stick {
                seeds.insert(seed);
            }
            let filtered = match &spec.filtered {
                Filtered::Same => &[][..],
                Filtered::Separate { waves } => waves,
            };
            for wave in spec.waves.iter().chain(filtered) {
                if let Wave::Noise { seed, .. } | Wave::Band { seed, .. } = wave {
                    seeds.insert(*seed);
                }
            }
        }
        (plants, seeds)
    }

    /// What every axis of the hold-out is held to: random stick moves, a
    /// first- or second-order plant behind a delay, a little gyro noise.
    fn assert_flown_as_a_flight(spec: &AxisSpec, at: &str) {
        assert!(matches!(spec.stick, Stick::Random { .. }), "{at}");
        match spec.plant {
            Plant::FirstOrder {
                tau_ms,
                delay_ms,
                gain,
            } => assert!(tau_ms > 0.0 && delay_ms > 0.0 && gain == 1.0, "{at}"),
            Plant::SecondOrder {
                natural_hz,
                damping,
                delay_ms,
                gain,
            } => {
                assert!((8.0..=25.0).contains(&natural_hz), "{at}");
                assert!((0.3..=0.8).contains(&damping), "{at}");
                assert!(delay_ms > 0.0 && gain == 1.0, "{at}");
            }
            other => panic!("{at}: {other:?}"),
        }
        match spec.waves[..] {
            [Wave::Noise { rms, .. }] => assert!((2.0..=5.0).contains(&rms), "{at}"),
            _ => panic!("{at}: {:?}", spec.waves),
        }
        assert_eq!(spec.filtered, Filtered::Same, "{at}");
    }

    #[test]
    fn round_6_is_a_hold_out_of_the_step_response() {
        let probes = round(6).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("w1-flight-2k", 2000.0, 34.0),
                ("w2-flight-1k", 1000.0, 22.0),
                ("w3-flight-4k", 4000.0, 16.0),
                ("w4-flight-8k", 8000.0, 14.0),
                ("w5-mixed-2k", 2000.0, 41.0),
            ]
        );
        let earlier: Vec<Probe> = (1..=5).flat_map(|r| round(r).unwrap()).collect();
        // no length of an earlier round, in seconds or in samples; and 8 kHz
        // is new to the step response
        for p in &probes {
            assert_eq!(p.area, Area::StepResponse);
            for e in &earlier {
                assert_ne!(p.flight.duration_s, e.flight.duration_s, "{}", p.id);
                assert_ne!(p.flight.len(), e.flight.len(), "{} {}", p.id, e.id);
            }
        }
        assert!(
            earlier
                .iter()
                .all(|e| e.area != Area::StepResponse || e.flight.fs < 8000.0)
        );
        // fifteen plants and thirty seeds, none of them flown before
        let (old_plants, old_seeds) = flown_in(&earlier);
        let (plants, seeds) = flown_in(&probes);
        assert_eq!((plants.len(), seeds.len()), (15, 30));
        assert!(seeds.is_disjoint(&old_seeds));
        for (k, plant) in plants.iter().enumerate() {
            assert!(!old_plants.contains(plant), "{plant:?}");
            assert!(!plants[..k].contains(plant), "{plant:?} flies twice");
        }
        for p in &probes {
            for axis in Axis::ALL {
                let at = format!("{} {axis:?}", p.id);
                assert_flown_as_a_flight(p.flight.axis(axis), &at);
                let s = p.flight.samples(axis);
                let peak = s.unfiltered.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(peak > 80.0 && peak < 2000.0, "{at}: {peak}");
                // the setpoint is whole deg/s, as a log keeps it
                assert!(s.setpoint.iter().all(|v| v.fract() == 0.0), "{at}");
            }
        }
    }

    #[test]
    fn round_7_flies_plants_whose_answers_are_known() {
        let probes = round(7).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("y1-delays-2k", 2000.0, 30.0),
                ("y2-first-2k", 2000.0, 30.0),
                ("y3-gain-2k", 2000.0, 30.0),
                ("y4-damping-2k", 2000.0, 30.0),
                ("y5-rate-1k", 1000.0, 30.0),
                ("y6-rate-4k", 4000.0, 30.0),
                ("y7-rate-8k", 8000.0, 30.0),
                ("y8-noise-2k", 2000.0, 30.0),
            ]
        );
        // stick moves no earlier round had
        let earlier: Vec<Probe> = (1..=6).flat_map(|r| round(r).unwrap()).collect();
        let (_, old_seeds) = flown_in(&earlier);
        let (plants, seeds) = flown_in(&probes);
        assert!(seeds.is_disjoint(&old_seeds));
        // every axis follows random stick moves through a plant, and only
        // the last probe has gyro noise
        for p in &probes {
            assert_eq!(p.area, Area::StepResponse);
            assert_eq!(p.flight.throttle, Throttle::Constant { percent: 50.0 });
            for axis in Axis::ALL {
                let spec = p.flight.axis(axis);
                let at = format!("{} {axis:?}", p.id);
                assert!(matches!(spec.stick, Stick::Random { .. }), "{at}");
                assert_ne!(spec.plant, Plant::None, "{at}");
                assert_eq!(spec.filtered, Filtered::Same, "{at}");
                let noisy = p.id == "y8-noise-2k" && axis != Axis::Roll;
                assert_eq!(spec.waves.is_empty(), !noisy, "{at}");
                let s = p.flight.samples(axis);
                let peak = s.unfiltered.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(peak > 150.0 && peak < 2000.0, "{at}: {peak}");
                assert!(s.setpoint.iter().all(|v| v.fract() == 0.0), "{at}");
            }
        }
        // what the titles promise about the plants
        let delays: Vec<Plant> = [10.0, 20.0, 5.0]
            .map(|delay_ms| Plant::Delay { delay_ms })
            .to_vec();
        assert_eq!(plants[..3], delays);
        let second = |natural_hz: f64, damping: f64, delay_ms: f64, gain: f64| Plant::SecondOrder {
            natural_hz,
            damping,
            delay_ms,
            gain,
        };
        for (plant, tau) in plants[3..6].iter().zip([10.0, 30.0, 80.0]) {
            let first = Plant::FirstOrder {
                tau_ms: tau,
                delay_ms: 0.0,
                gain: 1.0,
            };
            assert_eq!(*plant, first);
        }
        for (plant, gain) in plants[6..9].iter().zip([0.8, 1.0, 1.2]) {
            assert_eq!(*plant, second(15.0, 0.5, 5.0, gain));
        }
        for (plant, damping) in plants[9..12].iter().zip([0.3, 0.5, 0.9]) {
            assert_eq!(*plant, second(20.0, damping, 4.0, 1.0));
        }
        assert!(
            plants[21..]
                .iter()
                .all(|p| *p == second(20.0, 0.5, 4.0, 1.0))
        );
        let noise: Vec<f64> = [&probes[7].flight.pitch, &probes[7].flight.yaw]
            .iter()
            .map(|spec| match spec.waves[..] {
                [Wave::Noise { rms, .. }] => rms,
                _ => 0.0,
            })
            .collect();
        assert_eq!(noise, [5.0, 20.0]);
        // the three probes of the log rate are the fourth probe at another
        // rate: the same plants, and the stick moves at the same moments
        let base = &probes[3].flight;
        let moves = base.samples(Axis::Roll).setpoint;
        for p in &probes[4..7] {
            let same = FlightSpec {
                fs: base.fs,
                ..p.flight.clone()
            };
            assert_eq!(&same, base, "{}", p.id);
            let setpoint = p.flight.samples(Axis::Roll).setpoint;
            let at = |seconds: f64, x: &[f64], fs: f64| x[(seconds * fs) as usize];
            for tenth in 0..290 {
                let t = f64::from(tenth) / 10.0 + 0.0125;
                let (here, there) = (at(t, &setpoint, p.flight.fs), at(t, &moves, base.fs));
                assert_eq!(here, there, "{} at {t} s", p.id);
            }
        }
    }
}
