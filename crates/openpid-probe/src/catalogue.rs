//! The probes of every round, as data.
//!
//! A probe is one log. Each of its axes carries one experiment: a signal
//! chosen so that one unknown of a tool's pipeline leaves one mark that can
//! be read off a screen. The probes of a round that has been run are never
//! changed afterwards, or the readings taken from them would no longer
//! belong to any log; a new question gets a new probe in a later round
//! (`docs/calibration/ROUNDS.md`).

use crate::signal::{
    AxisSpec, Carried, Carrier, DebugValues, Filtered, FlightSpec, Modes, MotorOutput, PidTerms,
    Plant, Stick, Throttle, Wave,
};
use openpid_core::{Axis, PidTerm};
use serde::Serialize;

/// The part of the tools a probe is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// The spectrum of a channel.
    Spectrum,
    /// The step response: how the craft follows the stick.
    StepResponse,
    /// The map of the spectrum against the throttle.
    Throttle,
    /// The map of the spectrum against the time.
    Time,
    /// The windows of the tool that no round has asked yet: a first look.
    Windows,
    /// The main window: the channels of a log drawn as lines against time.
    LogViewer,
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
            ..FlightSpec::quiet(fs, duration_s)
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

/// A tone whose frequency follows the throttle, over the whole flight.
fn throttle_tone(hz_at_zero: f64, hz_per_percent: f64, amplitude: f64) -> Wave {
    Wave::ThrottleTone {
        hz_at_zero,
        hz_per_percent,
        amplitude,
        band: None,
    }
}

/// A tone of a fixed frequency that sounds only while the throttle is
/// between `low` and `high` percent.
fn tone_between(hz: f64, amplitude: f64, low: f64, high: f64) -> Wave {
    Wave::ThrottleTone {
        hz_at_zero: hz,
        hz_per_percent: 0.0,
        amplitude,
        band: Some((low, high)),
    }
}

/// An axis whose unfiltered gyro carries `unfiltered` and whose filtered
/// gyro carries `filtered`, each over its own dither: two experiments on
/// one axis, since a map of the throttle is drawn for either channel.
fn two_channels(seed: u64, unfiltered: Vec<Wave>, filtered: Vec<Wave>) -> AxisSpec {
    let with_dither = |mut waves: Vec<Wave>, seed: u64| {
        waves.push(dither(seed));
        waves
    };
    AxisSpec {
        filtered: Filtered::Separate {
            waves: with_dither(filtered, seed + 1),
        },
        ..AxisSpec::of(with_dither(unfiltered, seed))
    }
}

/// Stairs of the throttle from 10 to 90 % in steps of 10, `stair_s` each,
/// between two stretches at 50 % that fill the flight up to `duration_s`.
fn stairs(stair_s: f64, duration_s: f64) -> Throttle {
    let ends = (duration_s - 9.0 * stair_s) / 2.0;
    let mut steps = vec![(50.0, ends)];
    steps.extend((1..=9).map(|k| (10.0 * f64::from(k), stair_s)));
    steps.push((50.0, ends));
    Throttle::Stairs { steps }
}

/// The stairs probe: the same six experiments at every log rate and length.
fn throttle_stairs(id: &'static str, fs: f64, stair_s: f64, duration_s: f64) -> Probe {
    Probe {
        id,
        area: Area::Throttle,
        title: "Stairs of the throttle from 10 to 90 %: where a throttle lands on the map, how \
                far it spreads, what a tone and a noise floor read",
        purpose: [
            "unfiltered: a tone of 10 deg/s at 50 Hz + 4 Hz per percent of throttle, so every \
             stair has its own frequency (90 … 410 Hz): the column of each stair, its spread \
             and its value; filtered: the same tone at 2 deg/s: is the map linear",
            "unfiltered: a tone of 10 deg/s at 200 Hz through the whole flight: the same value \
             in every flown column, or one that follows the time spent there; filtered: that \
             tone only on the stairs of 30 and 40 %: the edges of a plateau",
            "unfiltered: white noise of 5 deg/s RMS: the floor, which with the tone gives the \
             length of a segment; filtered: noise of the same density between 100 and 300 Hz \
             only: how far the map is smoothed along the frequency",
        ],
        flight: FlightSpec {
            roll: two_channels(
                801,
                vec![throttle_tone(50.0, 4.0, 10.0)],
                vec![throttle_tone(50.0, 4.0, 2.0)],
            ),
            pitch: two_channels(
                803,
                vec![Wave::tone(200.0, 10.0)],
                vec![tone_between(200.0, 10.0, 25.0, 45.0)],
            ),
            yaw: AxisSpec {
                filtered: Filtered::Separate {
                    // the density of white noise of 5 deg/s RMS at 2 kHz
                    waves: vec![band(100.0, 300.0, 0.025, 806), dither(807)],
                },
                ..AxisSpec::of(vec![Wave::Noise {
                    rms: 5.0,
                    seed: 805,
                }])
            },
            throttle: stairs(stair_s, duration_s),
            ..FlightSpec::quiet(fs, duration_s)
        },
    }
}

/// Round 8: the map of frequency against throttle, identify. Every axis
/// carries two experiments, one on the unfiltered and one on the filtered
/// gyro.
fn round_8() -> Vec<Probe> {
    // ten stairs a fraction of a percent apart, each with its own tone
    let fine: [(f64, f64); 10] = [
        (30.0, 100.0),
        (30.2, 150.0),
        (30.4, 200.0),
        (30.6, 250.0),
        (30.8, 300.0),
        (60.0, 350.0),
        (60.3, 400.0),
        (60.5, 450.0),
        (60.7, 500.0),
        (61.0, 550.0),
    ];
    let fine_tones = |amplitude: f64| -> Vec<Wave> {
        fine.iter()
            .map(|(percent, hz)| tone_between(*hz, amplitude, percent - 0.05, percent + 0.05))
            .collect()
    };
    vec![
        throttle_stairs("z1-stairs-2k", 2000.0, 3.0, 30.0),
        Probe {
            id: "z2-ramp-2k",
            area: Area::Throttle,
            title: "The throttle rises evenly from 0 to 100 %: the throttle axis from end to \
                    end, and columns with little data",
            purpose: [
                "unfiltered: a tone of 10 deg/s at 50 Hz + 4 Hz per percent of throttle: a \
                 straight line from (0 %, 50 Hz) to (100 %, 450 Hz), and where the map puts it; \
                 filtered: a tone of 10 deg/s at 150 Hz between 20 and 40 % only: what is drawn \
                 where there is no tone",
                "unfiltered: a tone of 10 deg/s at 150 Hz through the whole flight: every \
                 column has a third of a second of it; filtered: 150 Hz below 3 % and 300 Hz \
                 above 97 % only: the two ends of the axis",
                "unfiltered: white noise of 5 deg/s RMS; filtered: the rising tone of roll at \
                 2 deg/s",
            ],
            flight: FlightSpec {
                roll: two_channels(
                    811,
                    vec![throttle_tone(50.0, 4.0, 10.0)],
                    vec![tone_between(150.0, 10.0, 20.0, 40.0)],
                ),
                pitch: two_channels(
                    813,
                    vec![Wave::tone(150.0, 10.0)],
                    vec![
                        tone_between(150.0, 10.0, 0.0, 3.0),
                        tone_between(300.0, 10.0, 97.0, 100.0),
                    ],
                ),
                yaw: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![throttle_tone(50.0, 4.0, 2.0), dither(816)],
                    },
                    ..AxisSpec::of(vec![Wave::Noise {
                        rms: 5.0,
                        seed: 815,
                    }])
                },
                throttle: Throttle::Ramp {
                    from: 0.0,
                    to: 100.0,
                },
                ..FlightSpec::quiet(2000.0, 30.0)
            },
        },
        Probe {
            id: "z3-hold-2k",
            area: Area::Throttle,
            title: "The throttle stays at 50 %: the spread of one column at every smoothing \
                    level, how segments are averaged, how close two tones can be",
            purpose: [
                "unfiltered: a tone of 10 deg/s at 100 Hz: one spot, whose shape along the \
                 throttle and along the frequency is the smoothing; filtered: the same tone at \
                 2 deg/s, inside the default colour scale",
                "unfiltered: that tone from 10 to 20 s only; filtered: from 5 to 25 s only: a \
                 mean of amplitudes reads them in proportion to their time, a mean of powers \
                 as its square root",
                "unfiltered: pairs of tones of 10 deg/s 4, 8 and 16 Hz apart (100 and 104, 200 \
                 and 208, 300 and 316 Hz): which pairs the map tells apart; filtered: white \
                 noise of 5 deg/s RMS: the floor of a single column",
            ],
            flight: FlightSpec {
                roll: two_channels(
                    821,
                    vec![Wave::tone(100.0, 10.0)],
                    vec![Wave::tone(100.0, 2.0)],
                ),
                pitch: two_channels(
                    823,
                    vec![burst(100.0, 10.0, 10.0, 20.0)],
                    vec![burst(100.0, 10.0, 5.0, 25.0)],
                ),
                yaw: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![Wave::Noise {
                            rms: 5.0,
                            seed: 826,
                        }],
                    },
                    ..tones(
                        825,
                        &[
                            (100.0, 10.0),
                            (104.0, 10.0),
                            (200.0, 10.0),
                            (208.0, 10.0),
                            (300.0, 10.0),
                            (316.0, 10.0),
                        ],
                    )
                },
                ..FlightSpec::quiet(2000.0, 30.0)
            },
        },
        Probe {
            id: "z4-fine-2k",
            area: Area::Throttle,
            title: "Stairs a few tenths of a percent apart around 30 and 60 %: which column a \
                    throttle value goes to",
            purpose: [
                "unfiltered: ten stairs (30.0, 30.2, 30.4, 30.6, 30.8, 60.0, 60.3, 60.5, 60.7 \
                 and 61.0 %), each with its own tone of 10 deg/s from 100 to 550 Hz in steps of \
                 50: the column of each tone is how a throttle is rounded; filtered: the same \
                 at 2 deg/s",
                "unfiltered: a tone of 10 deg/s at 200 Hz through the whole flight: two groups \
                 of columns with the same tone and different times; filtered: the same",
                "unfiltered: white noise of 5 deg/s RMS; filtered: the same",
            ],
            flight: FlightSpec {
                roll: two_channels(831, fine_tones(10.0), fine_tones(2.0)),
                pitch: tones(833, &[(200.0, 10.0)]),
                yaw: AxisSpec::of(vec![Wave::Noise {
                    rms: 5.0,
                    seed: 835,
                }]),
                throttle: Throttle::Stairs {
                    steps: fine.iter().map(|(percent, _)| (*percent, 3.0)).collect(),
                },
                ..FlightSpec::quiet(2000.0, 30.0)
            },
        },
        throttle_stairs("z5-stairs-1k", 1000.0, 3.0, 30.0),
        throttle_stairs("z6-stairs-4k", 4000.0, 3.0, 30.0),
        throttle_stairs("z7-stairs-8k", 8000.0, 3.0, 30.0),
        throttle_stairs("z8-short-2k", 2000.0, 1.0, 12.0),
    ]
}

/// The stairs probe in a flight of another length: nine stairs of
/// `stair_s` between two stretches of 1.5 s at half throttle.
fn stairs_of(id: &'static str, fs: f64, stair_s: f64) -> Probe {
    stairs_in(id, fs, stair_s, 9.0 * stair_s + 3.0)
}

/// The stairs probe in a flight of `duration_s`: nine stairs of `stair_s`
/// between two stretches at half throttle that fill the flight.
fn stairs_in(id: &'static str, fs: f64, stair_s: f64, duration_s: f64) -> Probe {
    Probe {
        title: "The stairs of the throttle in a flight of another length: how far apart the \
                segments of the map are",
        ..throttle_stairs(id, fs, stair_s, duration_s)
    }
}

/// Round 9: the map of frequency against throttle, what round 8 left open
/// and a flight to hold the rule against.
fn round_9() -> Vec<Probe> {
    // a flight as a craft flies it: the throttle moves between levels of
    // its own choosing and stays on each for a time of its own
    let flown: [(f64, f64); 12] = [
        (35.0, 2.0),
        (62.0, 1.5),
        (48.0, 3.0),
        (80.0, 1.0),
        (22.0, 2.5),
        (55.0, 4.0),
        (70.0, 2.0),
        (30.0, 3.0),
        (90.0, 1.5),
        (45.0, 2.5),
        (65.0, 3.0),
        (40.0, 4.0),
    ];
    let noise = |rms: f64, seed: u64| Wave::Noise { rms, seed };
    let stairs_down = {
        let mut steps = vec![(50.0, 1.5)];
        steps.extend((1..=9).rev().map(|k| (10.0 * f64::from(k), 3.0)));
        steps.push((50.0, 1.5));
        Throttle::Stairs { steps }
    };
    let sources = throttle_stairs("a8-sources-2k", 2000.0, 3.0, 30.0);
    vec![
        stairs_of("a1-len7-2k", 2000.0, 0.5),
        stairs_of("a2-len16-2k", 2000.0, 1.5),
        stairs_of("a3-len21-2k", 2000.0, 2.0),
        stairs_of("a4-len39-2k", 2000.0, 4.0),
        stairs_of("a5-len57-2k", 2000.0, 6.0),
        stairs_of("a6-len84-2k", 2000.0, 9.0),
        stairs_of("a7-len16-4k", 4000.0, 1.5),
        Probe {
            title: "The stairs of round 8 with the motors going the other way: which of the two \
                    the map takes for the throttle",
            flight: FlightSpec {
                motors: Some(stairs_down),
                ..sources.flight
            },
            ..sources
        },
        Probe {
            id: "a9-flight-2k",
            area: Area::Throttle,
            title: "A flight made for no question in particular: the throttle moves as it likes, \
                    motor noise follows it; the rule of round 8 says what the map shows",
            purpose: [
                "unfiltered: a tone of 8 deg/s at 80 Hz + 3 Hz per percent with its second \
                 harmonic at 3 deg/s, over noise of 2 deg/s RMS; filtered: the same at 3 and \
                 1 deg/s over noise of 1 deg/s",
                "unfiltered: a tone of 5 deg/s at 150 Hz over noise of 3 deg/s RMS; filtered: \
                 noise of 1 deg/s RMS",
                "unfiltered: noise between 50 and 400 Hz with a density of 0.02 (deg/s)²/Hz and \
                 a tone of 4 deg/s at 60 Hz + 2 Hz per percent; filtered: that tone at 1 deg/s",
            ],
            flight: FlightSpec {
                roll: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![
                            throttle_tone(80.0, 3.0, 3.0),
                            throttle_tone(160.0, 6.0, 1.0),
                            noise(1.0, 902),
                        ],
                    },
                    ..AxisSpec::of(vec![
                        throttle_tone(80.0, 3.0, 8.0),
                        throttle_tone(160.0, 6.0, 3.0),
                        noise(2.0, 901),
                    ])
                },
                pitch: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![noise(1.0, 904)],
                    },
                    ..AxisSpec::of(vec![Wave::tone(150.0, 5.0), noise(3.0, 903)])
                },
                yaw: two_channels(
                    905,
                    vec![band(50.0, 400.0, 0.02, 907), throttle_tone(60.0, 2.0, 4.0)],
                    vec![throttle_tone(60.0, 2.0, 1.0)],
                ),
                throttle: Throttle::Stairs {
                    steps: flown.to_vec(),
                },
                ..FlightSpec::quiet(2000.0, 30.0)
            },
        },
    ]
}

/// The first probe of the map of frequency against time, the same six
/// experiments at any log rate and length: everything it holds happens in
/// the first 14 s, and a flight shorter than that ends before its last
/// bursts.
fn time_bursts(id: &'static str, fs: f64, duration_s: f64) -> Probe {
    Probe {
        id,
        area: Area::Time,
        title: "A tone that comes and goes, a tone that climbs, close pairs of tones and noise: \
                the columns and the rows of the map of frequency against time, and what a cell \
                reads",
        purpose: [
            "unfiltered: 100 Hz at 10 deg/s from 3 to 3.5 s, 6 to 7 s, 9 to 11 s, 12.5 to 12.6 s \
             and 13.5 to 13.52 s: how long a column is and how far a short tone spreads; \
             filtered: 200 Hz at 2 deg/s all the time: the level of a steady tone",
            "unfiltered: a tone of 10 deg/s that climbs evenly from 50 to 450 Hz over the flight: \
             where a moment of the flight lies on the time axis; filtered: pairs of tones 4, 8 \
             and 16 Hz apart: the rows",
            "unfiltered: white noise of 5 deg/s RMS: the floor; filtered: noise of the same \
             density between 100 and 300 Hz only: smoothing along the frequency",
        ],
        flight: FlightSpec {
            roll: two_channels(
                1001,
                vec![
                    burst(100.0, 10.0, 3.0, 3.5),
                    burst(100.0, 10.0, 6.0, 7.0),
                    burst(100.0, 10.0, 9.0, 11.0),
                    burst(100.0, 10.0, 12.5, 12.6),
                    burst(100.0, 10.0, 13.5, 13.52),
                ],
                vec![Wave::tone(200.0, 2.0)],
            ),
            pitch: two_channels(
                1003,
                vec![Wave::Chirp {
                    from_hz: 50.0,
                    to_hz: 450.0,
                    amplitude: 10.0,
                }],
                [100.0, 104.0, 200.0, 208.0, 300.0, 316.0]
                    .map(|hz| Wave::tone(hz, 10.0))
                    .to_vec(),
            ),
            yaw: AxisSpec {
                filtered: Filtered::Separate {
                    waves: vec![band(100.0, 300.0, 0.025, 1006), dither(1007)],
                },
                ..AxisSpec::of(vec![Wave::Noise {
                    rms: 5.0,
                    seed: 1005,
                }])
            },
            ..FlightSpec::quiet(fs, duration_s)
        },
    }
}

/// Round 10: where the distance between the segments of the throttle map
/// changes, and a first look at the map of frequency against time.
fn round_10() -> Vec<Probe> {
    // a tone of 300 Hz that is `amplitude` deg/s for five seconds
    let loud = |k: u32, amplitude: f64| {
        burst(300.0, amplitude, 5.0 * f64::from(k), 5.0 * f64::from(k + 1))
    };
    vec![
        stairs_of("e1-len23-2k", 2000.0, 2.25),
        stairs_of("e2-len25-2k", 2000.0, 2.5),
        stairs_of("e3-len27-2k", 2000.0, 2.75),
        stairs_of("e4-len59-2k", 2000.0, 6.25),
        stairs_of("e5-len66-2k", 2000.0, 7.0),
        stairs_of("e6-len75-2k", 2000.0, 8.0),
        time_bursts("f1-bursts-2k", 2000.0, 30.0),
        Probe {
            id: "f2-steps-2k",
            area: Area::Time,
            title: "A tone whose frequency jumps every three seconds and a tone that gets louder \
                    every five: the time axis and the scale of the map of frequency against time",
            purpose: [
                "unfiltered: a tone of 10 deg/s whose frequency jumps with the stairs of the \
                 throttle, at 1.5 s and every 3 s from there (250, 90, 130 … 410, 250 Hz): where \
                 a jump lies on the time axis; filtered: 300 Hz at 1, 2, 5, 10, 20 and 40 deg/s, \
                 five seconds each: the decibels of a tone and where the colour scale ends",
                "unfiltered: an offset of 30 deg/s and a tone of 10 deg/s at 20 Hz: the lowest \
                 rows; filtered: a drift from -50 to 50 deg/s and the same tone",
                "unfiltered: white noise of 20 deg/s RMS; filtered: white noise of 1 deg/s RMS: \
                 two floors 26 dB apart",
            ],
            flight: FlightSpec {
                roll: two_channels(
                    1011,
                    vec![throttle_tone(50.0, 4.0, 10.0)],
                    vec![
                        loud(0, 1.0),
                        loud(1, 2.0),
                        loud(2, 5.0),
                        loud(3, 10.0),
                        loud(4, 20.0),
                        loud(5, 40.0),
                    ],
                ),
                pitch: two_channels(
                    1013,
                    vec![Wave::Offset { value: 30.0 }, Wave::tone(20.0, 10.0)],
                    vec![
                        Wave::Ramp {
                            from: -50.0,
                            to: 50.0,
                        },
                        Wave::tone(20.0, 10.0),
                    ],
                ),
                yaw: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![Wave::Noise {
                            rms: 1.0,
                            seed: 1016,
                        }],
                    },
                    ..AxisSpec::of(vec![Wave::Noise {
                        rms: 20.0,
                        seed: 1015,
                    }])
                },
                throttle: stairs(3.0, 30.0),
                ..FlightSpec::quiet(2000.0, 30.0)
            },
        },
        time_bursts("f3-bursts-4k", 4000.0, 30.0),
        time_bursts("f4-short-2k", 2000.0, 16.5),
    ]
}

/// Round 11: the map of frequency against time again. What its two
/// smoothing lists do, its segments in flights of other lengths and rates,
/// and a flight to hold the rule of round 10 against.
fn round_11() -> Vec<Probe> {
    let noise = |rms: f64, seed: u64| Wave::Noise { rms, seed };
    // a flight as a craft flies it: the throttle moves between levels of
    // its own choosing and stays on each for a time of its own
    let flown: [(f64, f64); 10] = [
        (40.0, 1.5),
        (58.0, 2.5),
        (25.0, 2.0),
        (75.0, 3.0),
        (50.0, 1.0),
        (85.0, 2.0),
        (33.0, 3.5),
        (66.0, 2.5),
        (45.0, 3.0),
        (52.0, 3.0),
    ];
    vec![
        time_bursts("g1-levels-2k", 2000.0, 30.0),
        time_bursts("g2-len8-2k", 2000.0, 8.0),
        time_bursts("g3-len45-2k", 2000.0, 45.0),
        time_bursts("g4-len84-2k", 2000.0, 84.0),
        time_bursts("g5-rate-1k", 1000.0, 30.0),
        time_bursts("g6-rate-8k", 8000.0, 30.0),
        Probe {
            id: "g7-flight-2k",
            area: Area::Time,
            title: "A flight made for no question in particular: the throttle moves as it likes, \
                    motor noise follows it; the rule of round 10 says what the map shows",
            purpose: [
                "unfiltered: a tone of 8 deg/s at 70 Hz + 3 Hz per percent with its second \
                 harmonic at 3 deg/s, over noise of 2 deg/s RMS; filtered: the same at 3 and \
                 1 deg/s over noise of 1 deg/s",
                "unfiltered: a tone of 5 deg/s at 180 Hz, 120 Hz at 15 deg/s from 8 to 8.4 s, \
                 over noise of 3 deg/s RMS; filtered: noise of 1 deg/s RMS",
                "unfiltered: noise between 60 and 350 Hz with a density of 0.02 (deg/s)²/Hz and \
                 a tone of 4 deg/s at 50 Hz + 2 Hz per percent; filtered: that tone at 1 deg/s",
            ],
            flight: FlightSpec {
                roll: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![
                            throttle_tone(70.0, 3.0, 3.0),
                            throttle_tone(140.0, 6.0, 1.0),
                            noise(1.0, 1102),
                        ],
                    },
                    ..AxisSpec::of(vec![
                        throttle_tone(70.0, 3.0, 8.0),
                        throttle_tone(140.0, 6.0, 3.0),
                        noise(2.0, 1101),
                    ])
                },
                pitch: AxisSpec {
                    filtered: Filtered::Separate {
                        waves: vec![noise(1.0, 1104)],
                    },
                    ..AxisSpec::of(vec![
                        Wave::tone(180.0, 5.0),
                        burst(120.0, 15.0, 8.0, 8.4),
                        noise(3.0, 1103),
                    ])
                },
                yaw: two_channels(
                    1105,
                    vec![band(60.0, 350.0, 0.02, 1107), throttle_tone(50.0, 2.0, 4.0)],
                    vec![throttle_tone(50.0, 2.0, 1.0)],
                ),
                throttle: Throttle::Stairs {
                    steps: flown.to_vec(),
                },
                ..FlightSpec::quiet(2000.0, 24.0)
            },
        },
    ]
}

/// Round 12: a first look at the windows no round has asked yet (the
/// sliders, the filters, the setup), with the first flight of round 6:
/// stick moves, a plant of its own on every axis and a little gyro noise.
fn round_12() -> Vec<Probe> {
    let flight = round_6().remove(0);
    vec![Probe {
        id: "k1-look-2k",
        area: Area::Windows,
        title: "A flight with stick moves, for a first look at the windows of the sliders, of \
                the filters and of the setup",
        ..flight
    }]
}

/// Round 13: the three windows of round 12 asked for the first time, with
/// the first two flights of round 6 (the list of the setup compares logs).
fn round_13() -> Vec<Probe> {
    let mut flights = round_6();
    let second = flights.remove(1);
    let first = flights.remove(0);
    let title = "A flight with stick moves, for the windows of the sliders, of the filters and \
                 of the setup";
    vec![
        Probe {
            id: "m1-ask-2k",
            area: Area::Windows,
            title,
            ..first
        },
        Probe {
            id: "m2-ask-1k",
            area: Area::Windows,
            title,
            ..second
        },
    ]
}

/// Round 14: the filter simulator and the slider tool again, for what
/// round 13 left: the other filter types and loop rates, a set of filters
/// made for no question, the other entries of the slider tool's list. One
/// flight of round 6; neither window reads it.
fn round_14() -> Vec<Probe> {
    let flight = round_6().remove(0);
    vec![Probe {
        id: "n1-more-2k",
        area: Area::Windows,
        title: "A flight with stick moves, for the rest of the filter simulator and of the \
                slider tool",
        ..flight
    }]
}

/// Length of the flight of round 15, seconds.
const VALUES_S: f64 = 20.0;
/// The debug mode the log of round 15 names. A firmware logs its debug
/// values only while a mode is set, so one has to be: the first after
/// "none". The values of the log do not depend on which; it is not mode 6,
/// which the reader of this repository takes for the unfiltered gyro in
/// logs of older releases.
const VALUES_DEBUG_MODE: u32 = 1;

/// An axis of round 15: the gyro does not follow the stick. The unfiltered
/// gyro holds `unfiltered`, the filtered one `filtered` plus `more`.
fn held(stick: Stick, unfiltered: f64, filtered: f64, more: Vec<Wave>) -> AxisSpec {
    let mut waves = vec![Wave::Offset { value: filtered }];
    waves.extend(more);
    AxisSpec {
        stick,
        plant: Plant::None,
        waves: vec![Wave::Offset { value: unfiltered }],
        filtered: Filtered::Separate { waves },
    }
}

/// A stick that asks for one rate over a whole flight of [`VALUES_S`].
fn steady(rate: f64) -> Stick {
    Stick::Steps {
        start_s: 0.0,
        every_s: VALUES_S,
        levels: vec![rate],
    }
}

/// Round 15: the main window, which draws the channels of a log as lines.
/// One log in which every channel holds a constant or steps of its own, so
/// that each line can be told from every other by its value and its scale
/// and its unit be read off it.
fn round_15() -> Vec<Probe> {
    let stairs = Stick::Steps {
        start_s: 4.0,
        every_s: 4.0,
        levels: vec![100.0, 200.0, 400.0, 0.0],
    };
    let spike = Wave::Spike {
        at_s: 15.0,
        value: 900.0,
    };
    vec![Probe {
        id: "q1-values-2k",
        area: Area::LogViewer,
        title: "Every channel at a constant or a step of its own, for what the lines of the \
                main window read: motors 1 to 4 at 0, 25, 50 and 100 % of the range between \
                the craft's motor outputs, turning at 6000, 12000, 18000 and 24000 rpm; the \
                throttle at 50 %, at 20 % from 10 to 12 s; the debug values 123, -45, 6 and 0 \
                (debug mode 1); armed from 2 s, angle mode from 5 to 9 s",
        purpose: [
            "filtered gyro 100 deg/s, and 1000 deg/s in the one sample at 15 s; unfiltered \
             gyro 120 deg/s; setpoint 0, from 4 s 100, 200 and 400 deg/s for four seconds \
             each, 0 from 16 s; P 100, I 20, D -30, F 5",
            "filtered gyro 200 deg/s, unfiltered gyro 230 deg/s; setpoint 60 deg/s; P -50, \
             I 10, D 15, F 0",
            "filtered gyro -150 deg/s, unfiltered gyro -170 deg/s; setpoint -40 deg/s; P 25, \
             I -5, F 0, and no D term: the craft has no D gain on yaw, so its log has no \
             such field",
        ],
        flight: FlightSpec {
            roll: held(stairs, 120.0, 100.0, vec![spike]),
            pitch: held(steady(60.0), 230.0, 200.0, Vec::new()),
            yaw: held(steady(-40.0), -170.0, -150.0, Vec::new()),
            throttle: Throttle::Stairs {
                steps: vec![(50.0, 10.0), (20.0, 2.0), (50.0, 8.0)],
            },
            motor_levels: Some([0.0, 25.0, 50.0, 100.0]),
            pid_terms: Some(PidTerms {
                p: [100.0, -50.0, 25.0],
                i: [20.0, 10.0, -5.0],
                d: [-30.0, 15.0, 0.0],
                f: [5.0, 0.0, 0.0],
            }),
            motor_rpm: Some([6000.0, 12_000.0, 18_000.0, 24_000.0]),
            debug: Some(DebugValues {
                mode: VALUES_DEBUG_MODE,
                values: vec![123.0, -45.0, 6.0, 0.0],
            }),
            modes: Some(Modes {
                armed_from_s: 2.0,
                angle_s: Some((5.0, 9.0)),
            }),
            ..FlightSpec::quiet(2000.0, VALUES_S)
        },
    }]
}

/// The outputs of the motors of the probe of round 16 whose ESCs are not
/// driven digitally: the range of an analogue protocol above a 7 % idle, as
/// `OneShot125` (protocol 1 of the firmware's list; the probes' craft has
/// `DShot300`, protocol 6).
const ANALOGUE: MotorOutput = MotorOutput {
    low: 1070,
    high: 2000,
    protocol: 1,
};
/// What the four motors of that probe log.
const ANALOGUE_MOTORS: [f64; 4] = [1070.0, 1300.0, 1535.0, 2000.0];
/// The seed of the noise of round 16.
const SAMPLES_SEED: u64 = 1601;

/// An axis whose filtered gyro carries `filtered` and whose unfiltered gyro
/// carries `unfiltered`, with nothing under either: no stick, no plant, no
/// dither. What a line of the main window draws is then the wave itself.
fn bare(unfiltered: Vec<Wave>, filtered: Vec<Wave>) -> AxisSpec {
    AxisSpec {
        filtered: Filtered::Separate { waves: filtered },
        ..AxisSpec::of(unfiltered)
    }
}

/// The three logs of round 16 for the main window.
fn viewer_open_ends() -> Vec<Probe> {
    let triangle = |size: f64, period_s: f64| Wave::Triangle {
        from: -size,
        to: size,
        period_s,
    };
    vec![
        Probe {
            id: "v1-deriv-2k",
            area: Area::LogViewer,
            title: "A gyro that rises and falls evenly, for the line the main window computes \
                    from a gyro (\"D-term(pf)\"), and the throttle of the stick at 50 % \
                    (rcCommand[3] 1500) beside the throttle of the mixer at 30 % (setpoint[3] \
                    300), for which of the two a window reads; motors 1 to 4 at 10, 20, 60 and \
                    70 % of the range between the craft's motor outputs, turning at 12000, \
                    18000, 24000 and 30000 rpm (200, 300, 400 and 500 revolutions per second)",
            purpose: [
                "filtered gyro: a triangle between -200 and 200 deg/s with a period of 2 s, \
                 from -200 at the first sample: 400 deg/s per second up and down (0.2 deg/s a \
                 sample); unfiltered gyro: a triangle between -300 and 300 deg/s with a period \
                 of 1.2 s: 1000 deg/s per second (0.5 deg/s a sample); setpoint 0; P 0, I 0, \
                 D 40, F 0",
                "filtered gyro: a tone of 100 deg/s at 5 Hz (at most 3142 deg/s per second); \
                 unfiltered gyro: a tone of 100 deg/s at 2 Hz (at most 1257 deg/s per second); \
                 setpoint 0; P 0, I 0, D 40, F 0",
                "filtered gyro 0; unfiltered gyro: a tone of 10 deg/s at 150 Hz, for the map of \
                 frequency against throttle: the column it lies in is the throttle that window \
                 reads; setpoint 0; no D term, the craft has no D gain on yaw",
            ],
            flight: FlightSpec {
                roll: bare(vec![triangle(300.0, 1.2)], vec![triangle(200.0, 2.0)]),
                pitch: bare(vec![Wave::tone(2.0, 100.0)], vec![Wave::tone(5.0, 100.0)]),
                yaw: bare(vec![Wave::tone(150.0, 10.0)], Vec::new()),
                throttle: Throttle::Constant { percent: 30.0 },
                stick_throttle: Some(50.0),
                motor_levels: Some([10.0, 20.0, 60.0, 70.0]),
                pid_terms: Some(PidTerms {
                    p: [0.0; 3],
                    i: [0.0; 3],
                    d: [40.0, 40.0, 0.0],
                    f: [0.0; 3],
                }),
                motor_rpm: Some([12_000.0, 18_000.0, 24_000.0, 30_000.0]),
                ..FlightSpec::quiet(2000.0, VALUES_S)
            },
        },
        Probe {
            id: "v2-out-2k",
            area: Area::LogViewer,
            title: "Motor outputs that are not digital, for what the motor lines of the main \
                    window read then: motors 1 to 4 at 1070, 1300, 1535 and 2000 with the header \
                    motorOutput:1070,2000 (0, 24.7, 50 and 100 % of the range between the two); \
                    the header that names the protocol, motor_pwm_protocol, is 1 (OneShot125) \
                    in place of the craft's 6 (DShot300); the throttle at 50 %",
            purpose: [
                "filtered gyro 20 deg/s, unfiltered gyro 30 deg/s, setpoint 10 deg/s",
                "filtered gyro -25 deg/s, unfiltered gyro -35 deg/s, setpoint -15 deg/s",
                "filtered gyro 40 deg/s, unfiltered gyro 45 deg/s, setpoint 5 deg/s",
            ],
            flight: FlightSpec {
                roll: held(steady(10.0), 30.0, 20.0, Vec::new()),
                pitch: held(steady(-15.0), -35.0, -25.0, Vec::new()),
                yaw: held(steady(5.0), 45.0, 40.0, Vec::new()),
                motor_levels: Some(ANALOGUE_MOTORS.map(|output| ANALOGUE.percent(output))),
                motor_output: Some(ANALOGUE),
                ..FlightSpec::quiet(2000.0, VALUES_S)
            },
        },
        Probe {
            id: "v3-samp-1k",
            area: Area::LogViewer,
            title: "A gyro that counts its samples, pulses of one, five and fifty samples, and \
                    a step under noise: which sample the read-out at the cursor shows, what the \
                    line smoothing does, how many points a drawn line keeps; 7 s at 1 kHz, 7000 \
                    samples",
            purpose: [
                "filtered gyro 0, and 500 deg/s in one sample at 2.000 s (sample 2000), in five \
                 samples from 4.000 s (4000 to 4004) and in fifty from 5.000 s (5000 to 5049); \
                 unfiltered gyro 0",
                "filtered gyro: the number of the sample in deg/s, 0 at the first and one more \
                 at every sample up to 6999 at the last; unfiltered gyro 0",
                "filtered gyro: white noise of 20 deg/s RMS (seed 1601), and 200 deg/s more \
                 from 3.500 s (sample 3500) to the end; unfiltered gyro 0",
            ],
            flight: counting(1000.0),
        },
    ]
}

/// The pulse at `at_s` of `samples` samples of `value` deg/s.
fn pulse(at_s: f64, samples: u32, value: f64) -> Wave {
    Wave::Pulse {
        at_s,
        samples,
        value,
    }
}

/// The flight of 7 s at `fs` of the log of round 16 that counts its
/// samples at 1 kHz: pulses of 1, 5 and 50 samples on the filtered gyro of
/// roll at 2, 4 and 5 s; a ramp from 0 to 7000 deg/s on pitch; noise and a
/// step at 3.5 s that lasts to the end on yaw.
fn counting(fs: f64) -> FlightSpec {
    let to_the_end = (3.5 * fs).round() as u32;
    FlightSpec {
        roll: bare(
            Vec::new(),
            vec![
                pulse(2.0, 1, 500.0),
                pulse(4.0, 5, 500.0),
                pulse(5.0, 50, 500.0),
            ],
        ),
        pitch: bare(
            Vec::new(),
            vec![Wave::Ramp {
                from: 0.0,
                to: 7000.0,
            }],
        ),
        yaw: bare(
            Vec::new(),
            vec![
                Wave::Noise {
                    rms: 20.0,
                    seed: SAMPLES_SEED,
                },
                // a step: a pulse that lasts to the end of the flight
                pulse(3.5, to_the_end, 200.0),
            ],
        ),
        ..FlightSpec::quiet(fs, 7.0)
    }
}

/// Round 16: the open ends the rounds before it left in every window.
///
/// Three logs for the main window; the stairs of round 8 in flights of
/// eight more lengths, for where the map of frequency against throttle
/// changes the distance between its segments (rounds 8 to 10 left the
/// thresholds inside (19, 21.25] and (57.25, 64] s of analysed stretch) and
/// what it and the map against time do with a short log; and three logs of
/// rounds 9 to 11 as they were, which the tool only has to decode: it read
/// two of them one sample later than the third.
///
/// The logs are in the order of their names, which is the order they are
/// opened in: the plan of the round takes a log by its row in the tool's
/// lists.
fn round_16() -> Vec<Probe> {
    let again = |probes: Vec<Probe>, id: &str| probes.into_iter().find(|p| p.id == id);
    let mut probes: Vec<Probe> = [
        again(round_9(), "a2-len16-2k"),
        again(round_10(), "f4-short-2k"),
        again(round_11(), "g7-flight-2k"),
    ]
    .into_iter()
    .flatten()
    .collect();
    probes.extend(viewer_open_ends());
    // the time on a stair and the length of the flight, seconds: the nine
    // stairs take the same time in the flights that are compared, and the
    // stretches at half throttle before and after them take the rest (1.25
    // to 2.25 s each: more than the second a window leaves out)
    probes.extend(
        [
            ("w1-len21-2k", 2.0, 21.5),
            ("w2-len22-2k", 2.0, 22.0),
            ("w3-len22h-2k", 2.0, 22.5),
            ("w4-len61-2k", 6.5, 61.0),
            ("w5-len62-2k", 6.5, 62.0),
            ("w6-len63-2k", 6.5, 63.0),
            ("w7-len10-2k", 0.75, 10.0),
            ("w8-len14-2k", 1.25, 14.0),
        ]
        .map(|(id, stair_s, duration_s)| stairs_in(id, 2000.0, stair_s, duration_s)),
    );
    probes
}

/// Round 17 asks every window again, on logs made for no question: the
/// verification round. The seeds of its noise and sticks start here.
const VERIFY_SEED: u64 = 1701;

/// A throttle that wanders between `low` and `high` percent: straight
/// lines between points `every_s` apart, each at a level of a fixed
/// sequence of `seed` (a tenth of a percent at a time), in the upper and
/// the lower half of the range by turns, so that it sweeps the range; the
/// first at the middle, the second at the top, the third at the bottom.
fn wandering(seed: u64, (low, high): (f64, f64), every_s: f64, duration_s: f64) -> Throttle {
    let mut state = seed;
    let count = (duration_s / every_s).ceil() as usize;
    let points = (0..=count)
        .map(|k| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state >> 33) % 1001) as f64 / 1000.0;
            let half = if k % 2 == 1 { 0.5 } else { 0.0 };
            let level = match k {
                0 => 0.5,
                1 => 1.0,
                2 => 0.0,
                _ => half + unit / 2.0,
            };
            let t = (k as f64 * every_s).min(duration_s);
            (t, (10.0 * (low + (high - low) * level)).round() / 10.0)
        })
        .collect();
    Throttle::Path { points }
}

/// The noise of a flown axis of round 17: motor noise whose fundamental
/// is `motor.0` Hz at zero throttle and `motor.1` Hz more per percent,
/// with `size` deg/s, its second harmonic at two fifths of that, a
/// resonance of the frame at `resonance_hz` with a third of it, and white
/// noise of `floor` deg/s RMS.
#[derive(Clone, Copy)]
struct Noisy {
    motor: (f64, f64),
    size: f64,
    resonance_hz: f64,
    floor: f64,
}

impl Noisy {
    /// The components on the unfiltered gyro (`share` 1) or a share of
    /// them on the filtered one.
    fn waves(self, share: f64, seed: u64) -> Vec<Wave> {
        let (at_zero, per_percent) = self.motor;
        vec![
            throttle_tone(at_zero, per_percent, self.size * share),
            throttle_tone(2.0 * at_zero, 2.0 * per_percent, 0.4 * self.size * share),
            Wave::tone(self.resonance_hz, self.size * share / 3.0),
            Wave::Noise {
                rms: self.floor * share,
                seed,
            },
        ]
    }
}

/// What the filtered gyro of round 17 keeps of the noise of the
/// unfiltered one.
const FILTERED_SHARE: f64 = 0.35;

/// An axis of a flight of round 17: random stick moves within `stick`
/// deg/s, held 150 to 850 ms, through `plant`, under the noise `noisy`.
fn verified(seed: u64, stick: f64, plant: Plant, noisy: Noisy) -> AxisSpec {
    AxisSpec {
        stick: Stick::Random {
            seed: seed + 100,
            hold_min_ms: 150,
            hold_max_ms: 850,
            amplitude: stick,
        },
        plant,
        waves: noisy.waves(1.0, seed),
        filtered: Filtered::Separate {
            waves: noisy.waves(FILTERED_SHARE, seed),
        },
    }
}

/// A second-order plant of gain 1.
fn second(natural_hz: f64, damping: f64, delay_ms: f64) -> Plant {
    Plant::SecondOrder {
        natural_hz,
        damping,
        delay_ms,
        gain: 1.0,
    }
}

/// A first-order plant of gain 1.
fn first(tau_ms: f64, delay_ms: f64) -> Plant {
    Plant::FirstOrder {
        tau_ms,
        delay_ms,
        gain: 1.0,
    }
}

/// One axis of a flight of round 17 as data: the stick's reach, the plant,
/// the noise.
type Flown17 = (f64, Plant, Noisy);

/// A flight of round 17 made for no question: on every axis its own stick
/// moves, plant and noise (`axes`, seeds from `seed`), the throttle
/// wandering within `throttle` percent.
fn made_for_none(
    (id, area, title): (&'static str, Area, &'static str),
    purpose: [&'static str; 3],
    (fs, duration_s): (f64, f64),
    (seed, throttle): (u64, (f64, f64)),
    axes: &[Flown17; 3],
) -> Probe {
    let [roll, pitch, yaw] = [0, 1, 2].map(|k: usize| {
        let (stick, plant, noisy) = axes[k];
        verified(seed + k as u64, stick, plant, noisy)
    });
    Probe {
        id,
        area,
        title,
        purpose,
        flight: FlightSpec {
            roll,
            pitch,
            yaw,
            throttle: wandering(seed + 10, throttle, 1.6, duration_s),
            ..FlightSpec::quiet(fs, duration_s)
        },
    }
}

/// The noise of an axis of round 17: `motor`, `size` deg/s, the resonance
/// and the floor.
fn noisy(motor: (f64, f64), size: f64, resonance_hz: f64, floor: f64) -> Noisy {
    Noisy {
        motor,
        size,
        resonance_hz,
        floor,
    }
}

/// The six flights of round 17 made for no question, one or two for each
/// window: the spectrum, the step response, both maps, the main window.
fn verification_flights() -> Vec<Probe> {
    vec![
        made_for_none(
            (
                "e1-spec-1k",
                Area::Spectrum,
                "A flight made for no question, for the spectrum window: stick moves, motor \
                 noise that follows a wandering throttle (25 to 65 %), a resonance, gyro noise; \
                 1 kHz, 26 s",
            ),
            [
                "stick moves within 280 deg/s through a second-order response of 17 Hz, damping \
                 0.38, after 3.25 ms; motor noise of 11 deg/s at 68 Hz + 2.7 Hz per percent of \
                 throttle and its second harmonic; a resonance at 345 Hz; white noise of 2.5 \
                 deg/s RMS; the filtered gyro keeps 0.35 of the noise",
                "stick moves within 230 deg/s, 13 Hz, damping 0.62, after 4.25 ms; motor noise \
                 of 9 deg/s; a resonance at 395 Hz; white noise of 2 deg/s RMS",
                "stick moves within 140 deg/s, a first-order response of 35 ms after 5.5 ms; \
                 motor noise of 6 deg/s; a resonance at 290 Hz; white noise of 1.5 deg/s RMS",
            ],
            (1000.0, 26.0),
            (VERIFY_SEED, (25.0, 65.0)),
            &[
                (
                    280.0,
                    second(17.0, 0.38, 3.25),
                    noisy((68.0, 2.7), 11.0, 345.0, 2.5),
                ),
                (
                    230.0,
                    second(13.0, 0.62, 4.25),
                    noisy((68.0, 2.7), 9.0, 395.0, 2.0),
                ),
                (140.0, first(35.0, 5.5), noisy((68.0, 2.7), 6.0, 290.0, 1.5)),
            ],
        ),
        made_for_none(
            (
                "e2-spec-4k",
                Area::Spectrum,
                "Another flight for the spectrum window, with other noise: the throttle \
                 wandering from 30 to 75 %; 4 kHz, 34 s",
            ),
            [
                "stick moves within 320 deg/s, 21 Hz, damping 0.47, after 2.75 ms; motor noise \
                 of 13 deg/s at 75 Hz + 3 Hz per percent and its second harmonic; a resonance at \
                 430 Hz; white noise of 3 deg/s RMS",
                "stick moves within 260 deg/s, 12 Hz, damping 0.85, after 5.25 ms; motor noise \
                 of 10 deg/s; a resonance at 470 Hz; white noise of 2.5 deg/s RMS",
                "stick moves within 150 deg/s, a first-order response of 28 ms after 4.75 ms; \
                 motor noise of 7 deg/s; a resonance at 375 Hz; white noise of 2 deg/s RMS",
            ],
            (4000.0, 34.0),
            (VERIFY_SEED + 20, (30.0, 75.0)),
            &[
                (
                    320.0,
                    second(21.0, 0.47, 2.75),
                    noisy((75.0, 3.0), 13.0, 430.0, 3.0),
                ),
                (
                    260.0,
                    second(12.0, 0.85, 5.25),
                    noisy((75.0, 3.0), 10.0, 470.0, 2.5),
                ),
                (
                    150.0,
                    first(28.0, 4.75),
                    noisy((75.0, 3.0), 7.0, 375.0, 2.0),
                ),
            ],
        ),
        made_for_none(
            (
                "e3-step-2k",
                Area::StepResponse,
                "A flight made for no question, for the step response window: stick moves \
                 through plants of dampings and delays no earlier round flew, with motor noise, \
                 a resonance and gyro noise; 2 kHz, 28 s",
            ),
            [
                "stick moves within 300 deg/s; a second-order response of 19 Hz, damping 0.44, \
                 after 2.75 ms; motor noise of 8 deg/s at 62 Hz + 3.1 Hz per percent; a \
                 resonance at 365 Hz; white noise of 2.5 deg/s RMS",
                "stick moves within 240 deg/s; a second-order response of 11 Hz, damping 0.88, \
                 after 5.75 ms; motor noise of 7 deg/s; a resonance at 415 Hz; white noise of 2 \
                 deg/s RMS",
                "stick moves within 160 deg/s; a first-order response of 26 ms after 9 ms; motor \
                 noise of 5 deg/s; a resonance at 305 Hz; white noise of 1.5 deg/s RMS",
            ],
            (2000.0, 28.0),
            (VERIFY_SEED + 40, (30.0, 70.0)),
            &[
                (
                    300.0,
                    second(19.0, 0.44, 2.75),
                    noisy((62.0, 3.1), 8.0, 365.0, 2.5),
                ),
                (
                    240.0,
                    second(11.0, 0.88, 5.75),
                    noisy((62.0, 3.1), 7.0, 415.0, 2.0),
                ),
                (160.0, first(26.0, 9.0), noisy((62.0, 3.1), 5.0, 305.0, 1.5)),
            ],
        ),
        made_for_none(
            (
                "e4-step-8k",
                Area::StepResponse,
                "Another flight for the step response window, with other plants: 8 kHz, 22 s",
            ),
            [
                "stick moves within 340 deg/s; a second-order response of 24 Hz, damping 0.95, \
                 after 3.25 ms; motor noise of 9 deg/s at 70 Hz + 2.9 Hz per percent; a \
                 resonance at 520 Hz; white noise of 3 deg/s RMS",
                "stick moves within 250 deg/s; a second-order response of 15 Hz, damping 0.33, \
                 after 4.25 ms; motor noise of 8 deg/s; a resonance at 610 Hz; white noise of \
                 2.5 deg/s RMS",
                "stick moves within 130 deg/s; a second-order response of 9 Hz, damping 0.67, \
                 after 5.5 ms; motor noise of 5 deg/s; a resonance at 450 Hz; white noise of 2 \
                 deg/s RMS",
            ],
            (8000.0, 22.0),
            (VERIFY_SEED + 60, (25.0, 75.0)),
            &[
                (
                    340.0,
                    second(24.0, 0.95, 3.25),
                    noisy((70.0, 2.9), 9.0, 520.0, 3.0),
                ),
                (
                    250.0,
                    second(15.0, 0.33, 4.25),
                    noisy((70.0, 2.9), 8.0, 610.0, 2.5),
                ),
                (
                    130.0,
                    second(9.0, 0.67, 5.5),
                    noisy((70.0, 2.9), 5.0, 450.0, 2.0),
                ),
            ],
        ),
        made_for_none(
            (
                "e5-maps-2k",
                Area::Throttle,
                "A flight made for no question, for the maps of frequency against throttle and \
                 against time: the throttle wandering from 20 to 80 %, motor tones that follow \
                 it, a resonance, gyro noise; 2 kHz, 47 s",
            ),
            [
                "stick moves within 260 deg/s, 18 Hz, damping 0.57, after 3.75 ms; motor noise \
                 of 12 deg/s at 55 Hz + 3.5 Hz per percent of throttle (125 to 335 Hz) and its \
                 second harmonic (250 to 670 Hz); a resonance at 455 Hz; white noise of 2.5 \
                 deg/s RMS; the filtered gyro keeps 0.35 of the noise",
                "stick moves within 220 deg/s, 14 Hz, damping 0.73, after 4.75 ms; motor noise \
                 of 10 deg/s; a resonance at 505 Hz; white noise of 2 deg/s RMS",
                "stick moves within 120 deg/s, a first-order response of 32 ms after 6.25 ms; \
                 motor noise of 7 deg/s; a resonance at 380 Hz; white noise of 1.5 deg/s RMS",
            ],
            (2000.0, 47.0),
            (VERIFY_SEED + 80, (20.0, 80.0)),
            &[
                (
                    260.0,
                    second(18.0, 0.57, 3.75),
                    noisy((55.0, 3.5), 12.0, 455.0, 2.5),
                ),
                (
                    220.0,
                    second(14.0, 0.73, 4.75),
                    noisy((55.0, 3.5), 10.0, 505.0, 2.0),
                ),
                (
                    120.0,
                    first(32.0, 6.25),
                    noisy((55.0, 3.5), 7.0, 380.0, 1.5),
                ),
            ],
        ),
        made_for_none(
            (
                "e6-view-2k",
                Area::LogViewer,
                "A flight made for no question, for the main window: stick moves, motor noise, \
                 a resonance, gyro noise, the throttle wandering from 30 to 70 %; 2 kHz, 24 s",
            ),
            [
                "stick moves within 290 deg/s, 16 Hz, damping 0.49, after 3.25 ms; motor noise \
                 of 10 deg/s at 66 Hz + 2.8 Hz per percent; a resonance at 385 Hz; white noise \
                 of 2.5 deg/s RMS",
                "stick moves within 210 deg/s, 12.5 Hz, damping 0.79, after 4.25 ms; motor noise \
                 of 8 deg/s; a resonance at 440 Hz; white noise of 2 deg/s RMS",
                "stick moves within 130 deg/s, a first-order response of 38 ms after 5.75 ms; \
                 motor noise of 6 deg/s; a resonance at 335 Hz; white noise of 1.5 deg/s RMS",
            ],
            (2000.0, 24.0),
            (VERIFY_SEED + 100, (30.0, 70.0)),
            &[
                (
                    290.0,
                    second(16.0, 0.49, 3.25),
                    noisy((66.0, 2.8), 10.0, 385.0, 2.5),
                ),
                (
                    210.0,
                    second(12.5, 0.79, 4.25),
                    noisy((66.0, 2.8), 8.0, 440.0, 2.0),
                ),
                (
                    130.0,
                    first(38.0, 5.75),
                    noisy((66.0, 2.8), 6.0, 335.0, 1.5),
                ),
            ],
        ),
    ]
}

/// What each channel of the spectrum window's list carries on one axis of
/// the probe of the other channels, as `(Hz, size)`: the setpoint (deg/s,
/// over the stick), the filtered and the unfiltered gyro (deg/s, over the
/// response to the stick), and the P, D and F terms (the firmware's
/// units); no D term on an axis without one.
struct ListedTones {
    setpoint: (f64, f64),
    gyro: (f64, f64),
    prefilt: (f64, f64),
    p: (f64, f64),
    d: Option<(f64, f64)>,
    f: (f64, f64),
}

/// The tones of the probe of the other channels: roll, pitch and yaw.
const LISTED: [ListedTones; 3] = [
    ListedTones {
        setpoint: (30.0, 6.0),
        gyro: (240.0, 2.5),
        prefilt: (300.0, 7.0),
        p: (120.0, 4.0),
        d: Some((180.0, 3.0)),
        f: (60.0, 5.0),
    },
    ListedTones {
        setpoint: (35.0, 6.5),
        gyro: (250.0, 3.0),
        prefilt: (310.0, 7.5),
        p: (130.0, 4.5),
        d: Some((190.0, 3.5)),
        f: (70.0, 5.5),
    },
    ListedTones {
        setpoint: (40.0, 7.0),
        gyro: (260.0, 3.5),
        prefilt: (320.0, 8.0),
        p: (140.0, 5.0),
        d: None,
        f: (80.0, 6.0),
    },
];
/// The tone of each motor of that probe, `(Hz, percent)`.
const MOTOR_TONES: [(f64, f64); 4] = [(85.0, 1.5), (105.0, 2.0), (135.0, 2.5), (165.0, 3.0)];
/// The tone of the debug values 0 to 2 of that probe, `(Hz, units)`.
const DEBUG_TONES: [(f64, f64); 3] = [(330.0, 8.0), (345.0, 9.0), (360.0, 10.0)];
/// White noise under each motor tone of that probe, percent RMS: about
/// half a unit of a log, which keeps the rounding of the log from making
/// lines of its own.
const MOTOR_DITHER: f64 = 0.03;

/// The probe of the other channels the spectrum window lists (S8, the
/// owner, 2026-11-17): every channel of the list carries a tone of its own
/// on every axis, so that a curve names the channel it was drawn from.
fn listed_channels() -> Probe {
    let seed = VERIFY_SEED + 200;
    let toned = |(hz, size): (f64, f64), seed: u64| vec![Wave::tone(hz, size), dither(seed)];
    let mut carried = Vec::new();
    let mut axes = Vec::new();
    for (k, (axis, tones)) in Axis::ALL.into_iter().zip(LISTED).enumerate() {
        let seed = seed + 10 * k as u64;
        let mut term = |term: PidTerm, tone: (f64, f64), seed: u64| {
            carried.push(Carried {
                channel: Carrier::Term { axis, term },
                waves: toned(tone, seed),
            });
        };
        term(PidTerm::P, tones.p, seed + 3);
        if let Some(d) = tones.d {
            term(PidTerm::D, d, seed + 4);
        }
        term(PidTerm::F, tones.f, seed + 5);
        carried.push(Carried {
            channel: Carrier::Setpoint { axis },
            waves: toned(tones.setpoint, seed + 6),
        });
        // a slow stick: a new rate every 2.5 s
        let levels = [80.0, -50.0, 120.0, 0.0, -90.0, 40.0].map(|l| l * (1.0 - 0.2 * k as f64));
        axes.push(AxisSpec {
            stick: Stick::Steps {
                start_s: 1.0,
                every_s: 2.5,
                levels: levels.to_vec(),
            },
            plant: second(12.0 - k as f64, 0.7, 4.0),
            waves: toned(tones.prefilt, seed + 1),
            filtered: Filtered::Separate {
                waves: toned(tones.gyro, seed + 2),
            },
        });
    }
    for (idx, (hz, size)) in MOTOR_TONES.into_iter().enumerate() {
        carried.push(Carried {
            channel: Carrier::Motor { idx: idx as u8 },
            waves: vec![
                Wave::tone(hz, size),
                Wave::Noise {
                    rms: MOTOR_DITHER,
                    seed: seed + 40 + idx as u64,
                },
            ],
        });
    }
    for (idx, tone) in DEBUG_TONES.into_iter().enumerate() {
        carried.push(Carried {
            channel: Carrier::Debug { idx: idx as u8 },
            waves: toned(tone, seed + 50 + idx as u64),
        });
    }
    let [roll, pitch, yaw]: [AxisSpec; 3] = axes.try_into().unwrap_or_else(|_| unreachable!());
    Probe {
        id: "i1-chan-2k",
        area: Area::Spectrum,
        title: "Every channel of the spectrum window's list with a tone of its own, for which \
                channel each entry of the list draws: motors 1 to 4 a tone of 1.5, 2, 2.5 and \
                3 % at 85, 105, 135 and 165 Hz over 50 % throttle; debug 0, 1 and 2 (debug mode \
                1) a tone of 8, 9 and 10 at 330, 345 and 360 Hz; PID error and PID sum follow \
                from the rest; every tone over white noise of 0.5 RMS (0.03 % on the motors); \
                2 kHz, 24 s",
        purpose: [
            "setpoint: a tone of 6 deg/s at 30 Hz over a slow stick (a new rate every 2.5 s \
             within 120 deg/s, which a second-order response of 12 Hz, damping 0.7, after 4 ms \
             follows); filtered gyro a tone of 2.5 deg/s at 240 Hz, unfiltered gyro 7 deg/s \
             at 300 Hz, both over that response; P a tone of 4 at 120 Hz, D 3 at 180 Hz, F 5 \
             at 60 Hz, I 0",
            "setpoint 6.5 deg/s at 35 Hz over a slow stick within 96 deg/s (11 Hz, damping \
             0.7, after 4 ms); filtered gyro 3 deg/s at 250 Hz, unfiltered 7.5 deg/s at 310 \
             Hz; P 4.5 at 130 Hz, D 3.5 at 190 Hz, F 5.5 at 70 Hz",
            "setpoint 7 deg/s at 40 Hz over a slow stick within 72 deg/s (10 Hz, damping 0.7, \
             after 4 ms); filtered gyro 3.5 deg/s at 260 Hz, unfiltered 8 deg/s at 320 Hz; P \
             5 at 140 Hz, F 6 at 80 Hz, and no D term: the craft has no D gain on yaw",
        ],
        flight: FlightSpec {
            roll,
            pitch,
            yaw,
            debug: Some(DebugValues {
                mode: VALUES_DEBUG_MODE,
                values: Vec::new(),
            }),
            carried,
            ..FlightSpec::quiet(2000.0, 24.0)
        },
    }
}

/// How much later the RC command of the probe of the stick's path holds
/// the stick, ms, and how much of it.
pub const ECHO: (f64, f64) = (40.0, 0.5);
/// How long the feedforward of that probe kicks at every move of the
/// stick, ms, and how much of the move.
pub const KICK: (f64, f64) = (10.0, 1.0);

/// The probe of the stick's path (#172, the owner): what the step response
/// window takes for the input of the craft, when the RC command is not the
/// setpoint's image and the feedforward carries kicks at every move.
fn stick_path() -> Probe {
    let seed = VERIFY_SEED + 300;
    let echo = |axis: Axis| Carried {
        channel: Carrier::RcCommand { axis },
        waves: vec![Wave::Echo {
            delay_ms: ECHO.0,
            scale: ECHO.1,
        }],
    };
    let kicks = |axis: Axis| Carried {
        channel: Carrier::Term {
            axis,
            term: PidTerm::F,
        },
        waves: vec![Wave::Kicks {
            ms: KICK.0,
            gain: KICK.1,
        }],
    };
    let axis = |k: u64, amplitude: f64, plant: Plant, noise: f64| AxisSpec {
        waves: vec![Wave::Noise {
            rms: noise,
            seed: seed + k,
        }],
        ..AxisSpec::following(
            Stick::Random {
                seed: seed + 10 + k,
                hold_min_ms: 180,
                hold_max_ms: 650,
                amplitude,
            },
            plant,
        )
    };
    Probe {
        id: "j1-path-2k",
        area: Area::StepResponse,
        title: "Random stick moves through known plants, with an RC command that is not the \
                setpoint's image and a feedforward that kicks at every move: what the step \
                response window takes for the input, the setpoint, the stick or the setpoint \
                with the feedforward; the manifest holds the three answers; 2 kHz, 28 s",
        purpose: [
            "stick moves within 240 deg/s through a second-order response of 16 Hz, damping \
             0.52, after 3.75 ms, gyro noise of 1.5 deg/s RMS; the RC command holds the same \
             moves 40 ms later and half as large (through the craft's rates, 1000 deg/s at \
             full stick); the feedforward holds the size of every move of the setpoint for \
             10 ms after it",
            "stick moves within 200 deg/s through a first-order response of 22 ms after 4.75 \
             ms, gyro noise of 1.5 deg/s RMS; the RC command is the setpoint's image; the \
             feedforward kicks as on roll",
            "stick moves within 150 deg/s through a second-order response of 10 Hz, damping \
             0.78, after 6.25 ms, gyro noise of 1 deg/s RMS; the RC command 40 ms later and \
             half as large, as on roll; no feedforward",
        ],
        flight: FlightSpec {
            roll: axis(0, 240.0, second(16.0, 0.52, 3.75), 1.5),
            pitch: axis(1, 200.0, first(22.0, 4.75), 1.5),
            yaw: axis(2, 150.0, second(10.0, 0.78, 6.25), 1.0),
            carried: vec![
                echo(Axis::Roll),
                kicks(Axis::Roll),
                kicks(Axis::Pitch),
                echo(Axis::Yaw),
            ],
            ..FlightSpec::quiet(2000.0, 28.0)
        },
    }
}

/// Round 17, the verification round: every window asked again, now that
/// the rules of the reference tool are known and `OpenPID`'s decisions are
/// taken, on logs that answer no question of their own. In the order the
/// plan values them:
///
/// - six flights made for no question (rates, lengths, plants and noise of
///   their own; stick moves, motor noise that follows a wandering
///   throttle, a resonance, gyro noise): two for the spectrum window, two
///   for the step response window, one for both maps, one for the main
///   window; and a set of filters typed into the filter simulator
///   (`capture::VERIFY_FILTERS`);
/// - the other channels of the spectrum window's list, each with a tone of
///   its own;
/// - the stick's path into the step response;
/// - two leftovers of round 16: its log that counts its samples at 2 and
///   4 kHz, for whether the line smoothing of the main window counts
///   samples or milliseconds, and its stairs in flights of 11, 12 and
///   13 s, for where the maps stop trimming the ends of a short log.
///
/// The logs are in the order of their names, which is the order they are
/// opened in.
fn round_17() -> Vec<Probe> {
    let mut probes = verification_flights();
    probes.push(listed_channels());
    probes.push(stick_path());
    let counted = |id: &'static str, fs: f64, rate: &'static str| Probe {
        id,
        area: Area::LogViewer,
        title: rate,
        purpose: [
            "filtered gyro 0, and 500 deg/s in one sample at 2.000 s, in five samples from \
             4.000 s and in fifty from 5.000 s: the same seconds and the same samples as \
             v3-samp-1k of round 16, at another rate; unfiltered gyro 0",
            "filtered gyro: a ramp from 0 to 7000 deg/s over the 7 s; unfiltered gyro 0",
            "filtered gyro: white noise of 20 deg/s RMS (seed 1601), and 200 deg/s more from \
             3.500 s to the end; unfiltered gyro 0",
        ],
        flight: counting(fs),
    };
    probes.push(counted(
        "x1-samp-2k",
        2000.0,
        "The log of round 16 that counts its samples (v3-samp-1k) at 2 kHz: pulses of one, \
         five and fifty samples at the same seconds, for whether the line smoothing of the main \
         window spans samples or milliseconds; 7 s, 14000 samples",
    ));
    probes.push(counted(
        "x2-samp-4k",
        4000.0,
        "The same at 4 kHz; 7 s, 28000 samples",
    ));
    for (id, duration_s) in [
        ("x3-len11-2k", 11.0),
        ("x4-len12-2k", 12.0),
        ("x5-len13-2k", 13.0),
    ] {
        probes.push(Probe {
            title: "The stairs of the throttle of round 16 (a second a stair) in a short \
                    flight: where between 10 and 14 s the maps stop taking a log whole and \
                    trim its ends",
            ..stairs_in(id, 2000.0, 1.0, duration_s)
        });
    }
    probes
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
        8 => Some(round_8()),
        9 => Some(round_9()),
        10 => Some(round_10()),
        11 => Some(round_11()),
        12 => Some(round_12()),
        13 => Some(round_13()),
        14 => Some(round_14()),
        15 => Some(round_15()),
        16 => Some(round_16()),
        17 => Some(round_17()),
        _ => None,
    }
}

/// The rounds that exist.
pub const ROUNDS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_core::modes::modes_at;
    use openpid_core::{
        Axis, ChannelId, FlightMode, FlightSession, GyroStage, LogFormat, OpenOptions, PidTerm,
    };
    use openpid_fmt_blackbox::BlackboxFormat;
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

    #[test]
    fn round_8_moves_the_throttle_and_keeps_the_sticks_still() {
        let probes = round(8).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("z1-stairs-2k", 2000.0, 30.0),
                ("z2-ramp-2k", 2000.0, 30.0),
                ("z3-hold-2k", 2000.0, 30.0),
                ("z4-fine-2k", 2000.0, 30.0),
                ("z5-stairs-1k", 1000.0, 30.0),
                ("z6-stairs-4k", 4000.0, 30.0),
                ("z7-stairs-8k", 8000.0, 30.0),
                ("z8-short-2k", 2000.0, 12.0),
            ]
        );
        for p in &probes {
            assert_eq!(p.area, Area::Throttle);
            let throttle = p.flight.throttle_samples();
            assert!(
                throttle.iter().all(|t| (0.0..=100.0).contains(t)),
                "{}",
                p.id
            );
            for axis in Axis::ALL {
                let spec = p.flight.axis(axis);
                let at = format!("{} {axis:?}", p.id);
                assert_eq!(spec.stick, Stick::Silence, "{at}");
                assert_eq!(spec.plant, Plant::None, "{at}");
                // every tone is below the Nyquist frequency of its log, at
                // any throttle
                let filtered: &[Wave] = match &spec.filtered {
                    Filtered::Same => &[],
                    Filtered::Separate { waves } => waves,
                };
                for wave in spec.waves.iter().chain(filtered) {
                    let top = match wave {
                        Wave::Tone { hz, .. } => *hz,
                        Wave::ThrottleTone {
                            hz_at_zero,
                            hz_per_percent,
                            ..
                        } => hz_at_zero + 100.0 * hz_per_percent,
                        Wave::Band { to_hz, .. } => *to_hz,
                        _ => 0.0,
                    };
                    assert!(top < p.flight.fs / 2.0, "{at}: {top} Hz");
                }
                let s = p.flight.samples(axis);
                let peak = s
                    .unfiltered
                    .iter()
                    .chain(&s.filtered)
                    .fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(peak > 2.0 && peak < 100.0, "{at}: {peak}");
            }
        }
        // the stairs: nine levels for the same time each, between two
        // stretches at half throttle, and the same flight at every rate
        let stairs = &probes[0].flight;
        let throttle = stairs.throttle_samples();
        assert_eq!(throttle.len(), 60_000);
        assert_eq!((throttle[0], throttle[2999]), (50.0, 50.0));
        for k in 1..=9 {
            let at = 3000 + (k - 1) * 6000;
            let level = 10.0 * k as f64;
            assert_eq!((throttle[at], throttle[at + 5999]), (level, level), "{k}");
        }
        assert_eq!((throttle[57_000], throttle[59_999]), (50.0, 50.0));
        for (k, fs) in [(4, 1000.0), (5, 4000.0), (6, 8000.0)] {
            assert_eq!(probes[k].flight.fs, fs);
            let same = FlightSpec {
                fs: 2000.0,
                ..probes[k].flight.clone()
            };
            assert_eq!(&same, stairs, "{}", probes[k].id);
        }
        let short = probes[7].flight.throttle_samples();
        assert_eq!(
            (short[2999], short[3000], short[4999], short[5000]),
            (50.0, 10.0, 10.0, 20.0)
        );
        // a tone on a stair has the frequency its title promises
        match &stairs.roll.waves[0] {
            Wave::ThrottleTone {
                hz_at_zero,
                hz_per_percent,
                amplitude,
                band: None,
            } => assert_eq!(
                (*hz_at_zero, *hz_per_percent, *amplitude),
                (50.0, 4.0, 10.0)
            ),
            other => panic!("{other:?}"),
        }
        // the ramp reaches both ends, the hold stays at half
        let ramp = probes[1].flight.throttle_samples();
        assert_eq!(ramp[0], 0.0);
        assert!(ramp[59_999] > 99.99 && ramp[59_999] < 100.0);
        assert_eq!(
            probes[2].flight.throttle,
            Throttle::Constant { percent: 50.0 }
        );
        // the fine stairs are tenths of a percent, which a log keeps
        let fine = probes[3].flight.throttle_samples();
        let levels: BTreeSet<i64> = fine.iter().map(|t| (t * 10.0).round() as i64).collect();
        assert_eq!(
            levels.into_iter().collect::<Vec<_>>(),
            [300, 302, 304, 306, 308, 600, 603, 605, 607, 610]
        );
        assert!(
            fine.iter()
                .all(|t| ((t * 10.0).round() - t * 10.0).abs() < 1.0e-9)
        );
    }

    #[test]
    fn round_9_changes_the_length_of_the_flight_and_parts_the_motors() {
        let probes = round(9).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("a1-len7-2k", 2000.0, 7.5),
                ("a2-len16-2k", 2000.0, 16.5),
                ("a3-len21-2k", 2000.0, 21.0),
                ("a4-len39-2k", 2000.0, 39.0),
                ("a5-len57-2k", 2000.0, 57.0),
                ("a6-len84-2k", 2000.0, 84.0),
                ("a7-len16-4k", 4000.0, 16.5),
                ("a8-sources-2k", 2000.0, 30.0),
                ("a9-flight-2k", 2000.0, 30.0),
            ]
        );
        let stairs = round(8).unwrap().remove(0).flight;
        for p in &probes {
            assert_eq!(p.area, Area::Throttle);
            for axis in Axis::ALL {
                assert_eq!(p.flight.axis(axis).stick, Stick::Silence, "{}", p.id);
            }
        }
        // the flights of other lengths are the stairs of round 8 but for
        // the time on a stair; the motors follow the throttle
        for p in &probes[..7] {
            let same = FlightSpec {
                fs: stairs.fs,
                duration_s: stairs.duration_s,
                throttle: stairs.throttle.clone(),
                ..p.flight.clone()
            };
            assert_eq!(same, stairs, "{}", p.id);
            let throttle = p.flight.throttle_samples();
            assert_eq!(p.flight.motor_samples(), throttle, "{}", p.id);
            let levels: BTreeSet<i64> = throttle.iter().map(|t| t.round() as i64).collect();
            assert_eq!(levels.len(), 9, "{}", p.id);
            // a stretch at half throttle of a second and a half at either end
            let end = (1.5 * p.flight.fs) as usize;
            assert!(throttle[..end].iter().all(|t| *t == 50.0), "{}", p.id);
            assert_eq!(throttle[end], 10.0, "{}", p.id);
            assert!(throttle[throttle.len() - end..].iter().all(|t| *t == 50.0));
        }
        // the stairs again with the motors going down while the throttle
        // goes up
        let sources = &probes[7].flight;
        assert_eq!(
            FlightSpec {
                motors: None,
                ..sources.clone()
            },
            stairs
        );
        let (throttle, motors) = (sources.throttle_samples(), sources.motor_samples());
        assert_eq!((throttle[4000], motors[4000]), (10.0, 90.0));
        assert_eq!((throttle[30_000], motors[30_000]), (50.0, 50.0));
        assert_eq!((throttle[56_000], motors[56_000]), (90.0, 10.0));
        // the flight: twelve levels of the throttle over all of its 30 s,
        // with tones below the Nyquist frequency at any throttle
        let flight = &probes[8].flight;
        let throttle = flight.throttle_samples();
        let levels: BTreeSet<i64> = throttle.iter().map(|t| t.round() as i64).collect();
        assert_eq!(levels.len(), 12);
        assert_eq!((throttle[0], throttle[59_999]), (35.0, 40.0));
        for axis in Axis::ALL {
            let spec = flight.axis(axis);
            let filtered: &[Wave] = match &spec.filtered {
                Filtered::Same => &[],
                Filtered::Separate { waves } => waves,
            };
            for wave in spec.waves.iter().chain(filtered) {
                if let Wave::ThrottleTone {
                    hz_at_zero,
                    hz_per_percent,
                    ..
                } = wave
                {
                    assert!(hz_at_zero + 100.0 * hz_per_percent < 1000.0);
                }
            }
        }
        // a flight written before the motors could go their own way says
        // nothing of them
        let text = serde_json::to_string(&stairs).unwrap();
        assert!(!text.contains("motors"));
        assert!(serde_json::to_string(sources).unwrap().contains("motors"));
    }

    /// Where a channel changes its value: the first sample and every
    /// sample that differs from the one before it, with the new value.
    fn steps(values: &[f64]) -> Vec<(usize, f64)> {
        let mut out: Vec<(usize, f64)> = Vec::new();
        for (i, value) in values.iter().enumerate() {
            if out.last().is_none_or(|last| last.1 != *value) {
                out.push((i, *value));
            }
        }
        out
    }

    #[test]
    fn round_15_is_one_flight_whose_every_channel_is_known() {
        let probes = round(15).unwrap();
        assert_eq!(probes.len(), 1);
        let probe = &probes[0];
        assert_eq!((probe.id, probe.area), ("q1-values-2k", Area::LogViewer));
        let flight = &probe.flight;
        assert_eq!((flight.fs, flight.duration_s), (2000.0, 20.0));
        assert_eq!(flight.len(), 40_000);
        // the gyro: constant on both channels, and on the filtered one of
        // roll one sample of 1000 deg/s at 15 s
        let gyro = |axis: Axis| {
            let s = flight.samples(axis);
            (steps(&s.unfiltered), steps(&s.filtered))
        };
        let spike = vec![(0, 100.0), (30_000, 1000.0), (30_001, 100.0)];
        assert_eq!(gyro(Axis::Roll), (vec![(0, 120.0)], spike));
        assert_eq!(gyro(Axis::Pitch), (vec![(0, 230.0)], vec![(0, 200.0)]));
        assert_eq!(gyro(Axis::Yaw), (vec![(0, -170.0)], vec![(0, -150.0)]));
        // the setpoint: stairs on roll, constant on pitch and yaw
        let setpoint = |axis: Axis| steps(&flight.samples(axis).setpoint);
        assert_eq!(
            setpoint(Axis::Roll),
            [
                (0, 0.0),
                (8000, 100.0),
                (16_000, 200.0),
                (24_000, 400.0),
                (32_000, 0.0)
            ]
        );
        assert_eq!(setpoint(Axis::Pitch), [(0, 60.0)]);
        assert_eq!(setpoint(Axis::Yaw), [(0, -40.0)]);
        // the throttle dips for two seconds; each motor keeps its own level
        assert_eq!(
            steps(&flight.throttle_samples()),
            [(0, 50.0), (20_000, 20.0), (24_000, 50.0)]
        );
        for (idx, level) in [0.0, 25.0, 50.0, 100.0].into_iter().enumerate() {
            assert_eq!(steps(&flight.motor_samples_of(idx)), [(0, level)], "{idx}");
        }
        // the terms of the controller, the speeds, the debug values
        let pid = flight.pid_terms.unwrap();
        assert_eq!(pid.p, [100.0, -50.0, 25.0]);
        assert_eq!(pid.i, [20.0, 10.0, -5.0]);
        assert_eq!(pid.d, [-30.0, 15.0, 0.0]);
        assert_eq!(pid.f, [5.0, 0.0, 0.0]);
        assert_eq!(
            flight.motor_rpm,
            Some([6000.0, 12_000.0, 18_000.0, 24_000.0])
        );
        let debug = flight.debug.clone().unwrap();
        assert_eq!(
            (debug.mode, debug.values),
            (1, vec![123.0, -45.0, 6.0, 0.0])
        );
        // no mode at first, armed from 2 s, angle mode from 5 to 9 s
        let modes: Vec<(usize, Vec<FlightMode>)> = flight
            .mode_changes()
            .into_iter()
            .map(|change| (change.sample_index, change.modes))
            .collect();
        assert_eq!(
            modes,
            [
                (0, vec![]),
                (4000, vec![FlightMode::Arm]),
                (10_000, vec![FlightMode::Arm, FlightMode::Angle]),
                (18_000, vec![FlightMode::Arm]),
            ]
        );
        // the texts say what the log holds
        assert!(probe.title.contains("0, 25, 50 and 100 %"));
        assert!(probe.title.contains("6000, 12000, 18000 and 24000 rpm"));
        assert!(probe.title.contains("at 20 % from 10 to 12 s"));
        assert!(probe.title.contains("123, -45, 6 and 0"));
        assert!(
            probe
                .title
                .contains("armed from 2 s, angle mode from 5 to 9 s")
        );
        assert!(probe.purpose[0].contains("1000 deg/s in the one sample at 15 s"));
        assert!(probe.purpose[0].contains("P 100, I 20, D -30, F 5"));
        assert!(probe.purpose[1].contains("P -50, I 10, D 15, F 0"));
        assert!(probe.purpose[2].contains("P 25, I -5, F 0, and no D term"));
    }

    /// The log of round 15 as the pack carries it, read back.
    fn values_log() -> (String, FlightSession) {
        let pack = crate::pack::build(15, "test").unwrap();
        let (name, bytes) = &pack.logs[0];
        assert_eq!(name, "q1-values-2k.bbl");
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(40_000)]).into_owned();
        let mut log = BlackboxFormat::new()
            .open(bytes, &OpenOptions::default())
            .unwrap();
        assert_eq!(log.sessions.len(), 1);
        (head, log.sessions.remove(0))
    }

    /// Where the channel `id` of `session` changes its value.
    fn logged(session: &FlightSession, id: &ChannelId) -> Vec<(usize, f64)> {
        let channel = session.channel(id);
        let channel = channel.unwrap_or_else(|| panic!("the log has no {}", id.key()));
        assert!(channel.data.all_valid(), "{}", id.key());
        steps(channel.data.values())
    }

    #[test]
    fn the_log_of_round_15_holds_the_gyro_the_setpoint_and_the_throttle() {
        let (_, session) = values_log();
        assert_eq!(session.len(), 40_000);
        assert_eq!(session.diagnostics.corrupt_frames, 0);
        let gyro = |axis: Axis, stage: GyroStage| {
            let id = ChannelId::Gyro { axis, stage };
            logged(&session, &id)
        };
        assert_eq!(gyro(Axis::Roll, GyroStage::Raw), [(0, 120.0)]);
        assert_eq!(gyro(Axis::Pitch, GyroStage::Raw), [(0, 230.0)]);
        assert_eq!(gyro(Axis::Yaw, GyroStage::Raw), [(0, -170.0)]);
        assert_eq!(gyro(Axis::Pitch, GyroStage::Filtered), [(0, 200.0)]);
        assert_eq!(gyro(Axis::Yaw, GyroStage::Filtered), [(0, -150.0)]);
        // the one sample of 1000 deg/s is the frame 15.000 s after the first
        assert_eq!(
            gyro(Axis::Roll, GyroStage::Filtered),
            [(0, 100.0), (30_000, 1000.0), (30_001, 100.0)]
        );
        let time = &session.timebase.time_us;
        let since_start = |sample: usize| time[sample] - time[0];
        assert_eq!(since_start(30_000), 15_000_000);
        assert_eq!(since_start(39_999), 19_999_500);
        let setpoint = |axis: Axis| logged(&session, &ChannelId::Setpoint { axis });
        assert_eq!(
            setpoint(Axis::Roll),
            [
                (0, 0.0),
                (8000, 100.0),
                (16_000, 200.0),
                (24_000, 400.0),
                (32_000, 0.0)
            ]
        );
        assert_eq!(setpoint(Axis::Pitch), [(0, 60.0)]);
        assert_eq!(setpoint(Axis::Yaw), [(0, -40.0)]);
        assert_eq!(
            (since_start(8000), since_start(32_000)),
            (4_000_000, 16_000_000)
        );
        // the throttle, as the log holds it twice: the mixer's (`setpoint[3]`,
        // tenths of a percent) and the stick's (`rcCommand[3]`, 1000 … 2000)
        let dip = |high: f64, low: f64| vec![(0, high), (20_000, low), (24_000, high)];
        assert_eq!(logged(&session, &ChannelId::Throttle), dip(50.0, 20.0));
        let mixer = ChannelId::Custom("setpoint[3]".into());
        assert_eq!(logged(&session, &mixer), dip(50.0, 20.0));
        let stick = ChannelId::RcCommand { ch: 3 };
        assert_eq!(logged(&session, &stick), dip(1500.0, 1200.0));
        assert_eq!(
            (since_start(20_000), since_start(24_000)),
            (10_000_000, 12_000_000)
        );
        // the sticks the writer makes of the setpoints: full stick is 500
        // units and asks for 1000 deg/s
        let stick = |ch: u8| logged(&session, &ChannelId::RcCommand { ch });
        assert_eq!(stick(1), [(0, 30.0)]);
        assert_eq!(stick(2), [(0, -20.0)]);
        assert_eq!(
            stick(0)[1..4],
            [(8000, 50.0), (16_000, 100.0), (24_000, 200.0)]
        );
    }

    #[test]
    fn the_log_of_round_15_holds_the_terms_the_motors_and_the_debug_values() {
        let (head, session) = values_log();
        let line = |key: &str| head.lines().find_map(|l| l.strip_prefix(key));
        let term = |term: PidTerm, axis: Axis| {
            let id = ChannelId::PidTerm { axis, term };
            logged(&session, &id)
        };
        for (which, values) in [
            (PidTerm::P, [100.0, -50.0, 25.0]),
            (PidTerm::I, [20.0, 10.0, -5.0]),
            (PidTerm::F, [5.0, 0.0, 0.0]),
        ] {
            for (axis, value) in Axis::ALL.into_iter().zip(values) {
                assert_eq!(term(which, axis), [(0, value)], "{which:?} {axis:?}");
            }
        }
        assert_eq!(term(PidTerm::D, Axis::Roll), [(0, -30.0)]);
        assert_eq!(term(PidTerm::D, Axis::Pitch), [(0, 15.0)]);
        // the craft has no D gain on yaw (`yawPID:45,80,0`), and a firmware
        // logs no D term for such an axis
        let yaw_d = ChannelId::PidTerm {
            axis: Axis::Yaw,
            term: PidTerm::D,
        };
        assert!(session.channel(&yaw_d).is_none());
        let fields = line("H Field I name:").unwrap();
        assert!(fields.contains("axisD[0],axisD[1],axisF[0]"));
        // the motors: a share of the range of the header, in whole units
        assert_eq!(line("H motorOutput:"), Some("158,2047"));
        for (idx, percent, raw) in [
            (0u8, 0.0, 158.0),
            (1, 25.0, 630.0),
            (2, 50.0, 1102.0),
            (3, 100.0, 2047.0),
        ] {
            let read = logged(&session, &ChannelId::Motor { idx });
            assert_eq!(read.len(), 1, "motor {idx}");
            // 25 and 50 % of 1889 units are no whole number of them
            assert!((read[0].1 - percent).abs() < 0.03, "motor {idx}");
            let units = 158.0 + read[0].1 / 100.0 * (2047.0 - 158.0);
            assert!((units - raw).abs() < 1e-9, "motor {idx}: {units}");
        }
        // the speeds: the electrical speed in hundreds, with 14 poles
        assert_eq!(line("H dshot_bidir:"), Some("1"));
        assert_eq!(line("H motor_poles:"), Some("14"));
        for (idx, erpm) in [(0u8, 420.0), (1, 840.0), (2, 1260.0), (3, 1680.0)] {
            assert_eq!(logged(&session, &ChannelId::Erpm { idx }), [(0, erpm)]);
            let rpm = 6000.0 * f64::from(idx + 1);
            assert_eq!(logged(&session, &ChannelId::MotorRpm { idx }), [(0, rpm)]);
        }
        // the debug values: the first four of the eight slots of the log
        assert_eq!(line("H debug_mode:"), Some("1"));
        let slots = [123.0, -45.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for (idx, value) in slots.into_iter().enumerate() {
            let id = ChannelId::Debug { idx: idx as u8 };
            assert_eq!(logged(&session, &id), [(0, value)], "debug {idx}");
        }
        assert!(fields.contains("debug[7],motor[0],motor[1],motor[2],motor[3],eRPM[0]"));
        assert!(fields.ends_with("eRPM[3]"));
    }

    #[test]
    fn the_log_of_round_15_holds_the_flight_modes_at_their_times() {
        let (_, session) = values_log();
        let time = &session.timebase.time_us;
        // the firmware logs its modes before the first frame that is not a
        // key frame, so the modes of the start are read at the second frame
        let modes: Vec<(i64, Vec<FlightMode>)> = session
            .flight_modes
            .iter()
            .map(|c| (time[c.sample_index] - time[0], c.modes.clone()))
            .collect();
        assert_eq!(
            modes,
            [
                (500, vec![]),
                (2_000_000, vec![FlightMode::Arm]),
                (5_000_000, vec![FlightMode::Arm, FlightMode::Angle]),
                (9_000_000, vec![FlightMode::Arm]),
            ]
        );
        let at = |sample: usize| modes_at(&session.flight_modes, sample);
        assert!(at(0).is_empty() && at(3999).is_empty());
        assert_eq!(at(4000), [FlightMode::Arm]);
        assert_eq!(at(9999), [FlightMode::Arm]);
        assert_eq!(at(10_000), [FlightMode::Arm, FlightMode::Angle]);
        assert_eq!(at(17_999), [FlightMode::Arm, FlightMode::Angle]);
        assert_eq!(at(18_000), [FlightMode::Arm]);
        assert_eq!(at(39_999), [FlightMode::Arm]);
    }

    #[test]
    fn the_flights_before_round_15_say_nothing_of_its_parts() {
        // their manifests and their logs stay what they were: a flight
        // without the parts round 15 added is written down without them,
        // and is given the header lines of the craft unchanged
        let parts = [
            "motor_levels",
            "pid_terms",
            "motor_rpm",
            "debug",
            "modes",
            "spike",
        ];
        for r in ROUNDS.iter().filter(|r| **r < 15) {
            for probe in round(*r).unwrap() {
                let text = serde_json::to_string(&probe.flight).unwrap();
                for part in parts {
                    assert!(!text.contains(part), "{}: {part}", probe.id);
                }
                assert!(probe.flight.mode_changes().is_empty(), "{}", probe.id);
                assert_eq!(
                    crate::pack::probe_headers(&probe),
                    crate::pack::craft_headers(probe.id),
                    "{}",
                    probe.id
                );
            }
        }
        let values = round(15).unwrap().remove(0);
        let text = serde_json::to_string(&values.flight).unwrap();
        for part in parts {
            assert!(text.contains(part), "{part}");
        }
        let changed: Vec<(String, String)> = crate::pack::probe_headers(&values)
            .into_iter()
            .zip(crate::pack::craft_headers(values.id))
            .filter(|(now, craft)| now != craft)
            .map(|(now, _)| now)
            .collect();
        assert_eq!(changed, [("debug_mode".to_string(), "1".to_string())]);
    }

    /// The probe `id` of round 16.
    fn open_end(id: &str) -> Probe {
        let found = round(16).unwrap().into_iter().find(|p| p.id == id);
        found.unwrap_or_else(|| panic!("round 16 has no {id}"))
    }

    /// The log of `probe` as a pack carries it, read back, and its header.
    fn log_of(probe: &Probe) -> (String, FlightSession) {
        let bytes = crate::pack::log_of(probe).unwrap();
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(40_000)]).into_owned();
        let mut log = BlackboxFormat::new()
            .open(&bytes, &OpenOptions::default())
            .unwrap();
        assert_eq!(log.sessions.len(), 1);
        let session = log.sessions.remove(0);
        assert_eq!(session.len(), probe.flight.len(), "{}", probe.id);
        assert_eq!(session.diagnostics.corrupt_frames, 0, "{}", probe.id);
        (head, session)
    }

    /// The samples of a gyro channel of a log.
    fn gyro_of(session: &FlightSession, axis: Axis, stage: GyroStage) -> Vec<f64> {
        let channel = session.channel(&ChannelId::Gyro { axis, stage }).unwrap();
        assert!(channel.data.all_valid());
        channel.data.values().to_vec()
    }

    #[test]
    fn round_16_is_fourteen_logs_in_the_order_of_their_names() {
        let probes = round(16).unwrap();
        let shape: Vec<(&str, Area, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.area, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("a2-len16-2k", Area::Throttle, 2000.0, 16.5),
                ("f4-short-2k", Area::Time, 2000.0, 16.5),
                ("g7-flight-2k", Area::Time, 2000.0, 24.0),
                ("v1-deriv-2k", Area::LogViewer, 2000.0, 20.0),
                ("v2-out-2k", Area::LogViewer, 2000.0, 20.0),
                ("v3-samp-1k", Area::LogViewer, 1000.0, 7.0),
                ("w1-len21-2k", Area::Throttle, 2000.0, 21.5),
                ("w2-len22-2k", Area::Throttle, 2000.0, 22.0),
                ("w3-len22h-2k", Area::Throttle, 2000.0, 22.5),
                ("w4-len61-2k", Area::Throttle, 2000.0, 61.0),
                ("w5-len62-2k", Area::Throttle, 2000.0, 62.0),
                ("w6-len63-2k", Area::Throttle, 2000.0, 63.0),
                ("w7-len10-2k", Area::Throttle, 2000.0, 10.0),
                ("w8-len14-2k", Area::Throttle, 2000.0, 14.0),
            ]
        );
        // the plan takes a log by its row in the tool's lists: the logs are
        // opened in the order of their names
        let mut sorted: Vec<&str> = probes.iter().map(|p| p.id).collect();
        sorted.sort_unstable();
        assert_eq!(sorted, probes.iter().map(|p| p.id).collect::<Vec<_>>());
        // three logs of earlier rounds, each the probe its own round has
        assert_eq!(probes[0], round(9).unwrap().remove(1));
        assert_eq!(probes[1], round(10).unwrap().remove(9));
        assert_eq!(probes[2], round(11).unwrap().remove(6));
        // the stairs of round 8 in eight more lengths
        let stairs = round(8).unwrap().remove(0).flight;
        let lengths = [
            (2.0, 1.75),
            (2.0, 2.0),
            (2.0, 2.25),
            (6.5, 1.25),
            (6.5, 1.75),
            (6.5, 2.25),
            (0.75, 1.625),
            (1.25, 1.375),
        ];
        for (p, (stair_s, ends_s)) in probes[6..].iter().zip(lengths) {
            let same = FlightSpec {
                duration_s: stairs.duration_s,
                throttle: stairs.throttle.clone(),
                ..p.flight.clone()
            };
            assert_eq!(same, stairs, "{}", p.id);
            assert_eq!(p.title, probes[0].title);
            assert_eq!(
                p.flight.duration_s,
                9.0 * stair_s + 2.0 * ends_s,
                "{}",
                p.id
            );
            // half throttle at either end for longer than the second a
            // window leaves out, and nine stairs of one length between
            let throttle = p.flight.throttle_samples();
            assert_eq!(p.flight.motor_samples(), throttle, "{}", p.id);
            let (end, stair) = ((ends_s * 2000.0) as usize, (stair_s * 2000.0) as usize);
            let mut want = vec![(0, 50.0)];
            want.extend((0..9).map(|k| (end + k * stair, 10.0 * (k + 1) as f64)));
            want.push((end + 9 * stair, 50.0));
            assert_eq!(steps(&throttle), want, "{}", p.id);
            assert_eq!(throttle.len(), 2 * end + 9 * stair, "{}", p.id);
            assert!(ends_s > 1.0);
        }
        // the stretches a window analyses: three around 20 s and three
        // around 60 s, inside the brackets rounds 8 to 10 left, and two
        // short ones
        let stretch: Vec<f64> = probes[6..]
            .iter()
            .map(|p| p.flight.duration_s - 2.0)
            .collect();
        assert_eq!(stretch, [19.5, 20.0, 20.5, 59.0, 60.0, 61.0, 8.0, 12.0]);
        assert!(stretch[..3].iter().all(|s| *s > 19.0 && *s <= 21.25));
        assert!(stretch[3..6].iter().all(|s| *s > 57.25 && *s <= 64.0));
    }

    #[test]
    fn the_first_log_of_round_16_rises_evenly_and_parts_the_two_throttles() {
        let probe = open_end("v1-deriv-2k");
        let flight = &probe.flight;
        assert_eq!(flight.len(), 40_000);
        // roll: 0.2 deg/s a sample on the filtered gyro, a period of 4000
        // samples; 0.5 deg/s a sample on the unfiltered one, 2400 samples
        let roll = flight.samples(Axis::Roll);
        let f = &roll.filtered;
        assert_eq!(
            (f[0], f[1000], f[2000], f[4000]),
            (-200.0, 0.0, 200.0, -200.0)
        );
        assert!((f[1] + 199.8).abs() < 1e-9 && (f[2001] - 199.8).abs() < 1e-9);
        assert!(
            f.windows(2)
                .all(|w| ((w[1] - w[0]).abs() - 0.2).abs() < 1e-9)
        );
        assert_eq!(f[..4000], f[36_000..]);
        let u = &roll.unfiltered;
        assert_eq!(
            (u[0], u[600], u[1200], u[2400]),
            (-300.0, 0.0, 300.0, -300.0)
        );
        assert!(u.windows(2).all(|w| (w[1] - w[0]).abs() == 0.5));
        assert_eq!(u[..2400], u[2400..4800]);
        // per second: 400 and 1000 deg/s
        assert!((f[2000] - f[0] - 400.0).abs() < 1e-9 && u[1200] - u[0] == 600.0);
        // pitch: 5 Hz and 2 Hz at 100 deg/s; yaw: 150 Hz on the unfiltered
        // gyro only
        let pitch = flight.samples(Axis::Pitch);
        let tone =
            |hz: f64, i: usize| 100.0 * (std::f64::consts::TAU * hz * i as f64 / 2000.0).sin();
        for i in [0, 1, 99, 100, 250, 12_345, 39_999] {
            assert!((pitch.filtered[i] - tone(5.0, i)).abs() < 1e-9, "{i}");
            assert!((pitch.unfiltered[i] - tone(2.0, i)).abs() < 1e-9, "{i}");
        }
        assert!((pitch.filtered[100] - 100.0).abs() < 1e-9);
        assert!((pitch.unfiltered[250] - 100.0).abs() < 1e-9);
        let yaw = flight.samples(Axis::Yaw);
        assert!(yaw.filtered.iter().all(|v| *v == 0.0));
        let power = yaw.unfiltered.iter().map(|v| v * v).sum::<f64>() / 40_000.0;
        assert!((power - 50.0).abs() < 1e-6, "{power}");
        let hundred_fifty =
            |i: usize| 10.0 * (std::f64::consts::TAU * 150.0 * i as f64 / 2000.0).sin();
        assert!((yaw.unfiltered[777] - hundred_fifty(777)).abs() < 1e-9);
        for axis in Axis::ALL {
            assert!(flight.samples(axis).setpoint.iter().all(|v| *v == 0.0));
        }
        // the two throttles, the motors apart from both, the terms, the speeds
        assert_eq!(steps(&flight.throttle_samples()), [(0, 30.0)]);
        assert_eq!(flight.stick_throttle, Some(50.0));
        assert_eq!(flight.motor_levels, Some([10.0, 20.0, 60.0, 70.0]));
        let pid = flight.pid_terms.unwrap();
        assert_eq!((pid.p, pid.i, pid.f), ([0.0; 3], [0.0; 3], [0.0; 3]));
        assert_eq!(pid.d, [40.0, 40.0, 0.0]);
        let rpm = flight.motor_rpm.unwrap();
        assert_eq!(rpm, [12_000.0, 18_000.0, 24_000.0, 30_000.0]);
        // every motor turns faster than 150 revolutions per second
        assert_eq!(rpm.map(|r| r / 60.0), [200.0, 300.0, 400.0, 500.0]);
        assert!(flight.debug.is_none() && flight.modes.is_none() && flight.motor_output.is_none());
        assert!(
            probe.title.contains("rcCommand[3] 1500")
                && probe.title.contains(
                    "setpoint[3] \
             300"
                )
        );
        assert!(probe.purpose[0].contains("400 deg/s per second"));
        assert!(probe.purpose[0].contains("1000 deg/s per second"));
    }

    #[test]
    fn the_log_of_the_first_log_of_round_16_holds_both_throttles() {
        let probe = open_end("v1-deriv-2k");
        let (head, session) = log_of(&probe);
        let line = |key: &str| head.lines().find_map(|l| l.strip_prefix(key));
        // the stick at 1500 (`rcCommand[3]`), the mixer at 30 % (`setpoint[3]`
        // 300, which the reader gives in percent)
        let stick = ChannelId::RcCommand { ch: 3 };
        assert_eq!(logged(&session, &stick), [(0, 1500.0)]);
        let mixer = ChannelId::Custom("setpoint[3]".into());
        assert_eq!(logged(&session, &mixer), [(0, 30.0)]);
        for ch in 0..3 {
            assert_eq!(logged(&session, &ChannelId::RcCommand { ch }), [(0, 0.0)]);
        }
        // OpenPID's own throttle is the mixer's
        assert_eq!(logged(&session, &ChannelId::Throttle), [(0, 30.0)]);
        // the gyro is the flight in whole deg/s: a tie goes to the even number
        for axis in Axis::ALL {
            let exact = probe.flight.samples(axis);
            for (stage, want) in [
                (GyroStage::Raw, &exact.unfiltered),
                (GyroStage::Filtered, &exact.filtered),
            ] {
                let got = gyro_of(&session, axis, stage);
                let equal = got.iter().zip(want).all(|(g, w)| *g == w.round_ties_even());
                assert!(equal && got.len() == want.len(), "{axis:?} {stage:?}");
            }
        }
        let unfiltered = gyro_of(&session, Axis::Roll, GyroStage::Raw);
        assert_eq!(
            unfiltered[..9],
            [
                -300.0, -300.0, -299.0, -298.0, -298.0, -298.0, -297.0, -296.0, -296.0
            ]
        );
        let filtered = gyro_of(&session, Axis::Roll, GyroStage::Filtered);
        assert_eq!(
            filtered[..6],
            [-200.0, -200.0, -200.0, -199.0, -199.0, -199.0]
        );
        assert_eq!((filtered[2000], filtered[4000]), (200.0, -200.0));
        // a second of the log is 400 and 1000 deg/s on those lines
        assert_eq!(filtered[1500] - filtered[500], 200.0);
        assert_eq!(unfiltered[700] - unfiltered[200], 250.0);
        // the D term on roll and pitch, none on yaw; the other terms at zero
        let term = |term: PidTerm, axis: Axis| ChannelId::PidTerm { axis, term };
        assert_eq!(logged(&session, &term(PidTerm::D, Axis::Roll)), [(0, 40.0)]);
        assert_eq!(
            logged(&session, &term(PidTerm::D, Axis::Pitch)),
            [(0, 40.0)]
        );
        assert!(session.channel(&term(PidTerm::D, Axis::Yaw)).is_none());
        assert_eq!(logged(&session, &term(PidTerm::P, Axis::Roll)), [(0, 0.0)]);
        // the motors in the craft's digital range, and their speeds
        assert_eq!(line("H motorOutput:"), Some("158,2047"));
        assert_eq!(line("H motor_pwm_protocol:"), Some("6"));
        for (idx, raw) in [(0u8, 347.0), (1, 536.0), (2, 1291.0), (3, 1480.0)] {
            let read = logged(&session, &ChannelId::Motor { idx });
            assert_eq!(read.len(), 1, "motor {idx}");
            let units = 158.0 + read[0].1 / 100.0 * (2047.0 - 158.0);
            assert!((units - raw).abs() < 1e-9, "motor {idx}: {units}");
        }
        for (idx, erpm) in [(0u8, 840.0), (1, 1260.0), (2, 1680.0), (3, 2100.0)] {
            assert_eq!(logged(&session, &ChannelId::Erpm { idx }), [(0, erpm)]);
        }
        assert_eq!(line("H dshot_bidir:"), Some("1"));
        assert_eq!(session.len(), 40_000);
    }

    #[test]
    fn the_second_log_of_round_16_has_outputs_that_are_not_digital() {
        let probe = open_end("v2-out-2k");
        let flight = &probe.flight;
        let output = flight.motor_output.unwrap();
        assert_eq!((output.low, output.high, output.protocol), (1070, 2000, 1));
        let levels = flight.motor_levels.unwrap();
        assert_eq!((levels[0], levels[2], levels[3]), (0.0, 50.0, 100.0));
        assert!((levels[1] - 100.0 * 230.0 / 930.0).abs() < 1e-12);
        assert_eq!(steps(&flight.throttle_samples()), [(0, 50.0)]);
        assert!(flight.stick_throttle.is_none() && flight.motor_rpm.is_none());
        // the header follows the probe: the range, and the protocol in place
        // of the craft's
        let changed: Vec<(String, String)> = crate::pack::probe_headers(&probe)
            .into_iter()
            .zip(crate::pack::craft_headers(probe.id))
            .filter(|(now, craft)| now != craft)
            .map(|(now, _)| now)
            .collect();
        assert_eq!(
            changed,
            [("motor_pwm_protocol".to_string(), "1".to_string())]
        );
        let craft = crate::pack::craft_headers(probe.id);
        let protocol = craft.iter().find(|(key, _)| key == "motor_pwm_protocol");
        assert_eq!(protocol.map(|(_, value)| value.as_str()), Some("6"));
        let (head, session) = log_of(&probe);
        let line = |key: &str| head.lines().find_map(|l| l.strip_prefix(key));
        assert_eq!(line("H motorOutput:"), Some("1070,2000"));
        assert_eq!(line("H motor_pwm_protocol:"), Some("1"));
        assert_eq!(line("H dshot_bidir:"), Some("0"));
        assert!(!line("H Field I name:").unwrap().contains("eRPM"));
        // the four motors as logged: the numbers the title names
        for (idx, raw) in [(0u8, 1070.0), (1, 1300.0), (2, 1535.0), (3, 2000.0)] {
            let read = logged(&session, &ChannelId::Motor { idx });
            assert_eq!(read.len(), 1, "motor {idx}");
            let units = 1070.0 + read[0].1 / 100.0 * 930.0;
            assert!((units - raw).abs() < 1e-9, "motor {idx}: {units}");
        }
        assert!(probe.title.contains("1070, 1300, 1535 and 2000"));
        assert!(probe.title.contains("motorOutput:1070,2000"));
        assert!(
            probe
                .title
                .contains("motor_pwm_protocol, is 1 (OneShot125)")
        );
        // the gyro and the setpoint at small constants of their own
        let constants = [
            (Axis::Roll, 30.0, 20.0, 10.0),
            (Axis::Pitch, -35.0, -25.0, -15.0),
            (Axis::Yaw, 45.0, 40.0, 5.0),
        ];
        for (axis, unfiltered, filtered, setpoint) in constants {
            let raw = ChannelId::Gyro {
                axis,
                stage: GyroStage::Raw,
            };
            let gyro = ChannelId::Gyro {
                axis,
                stage: GyroStage::Filtered,
            };
            assert_eq!(logged(&session, &raw), [(0, unfiltered)], "{axis:?}");
            assert_eq!(logged(&session, &gyro), [(0, filtered)], "{axis:?}");
            let asked = logged(&session, &ChannelId::Setpoint { axis });
            assert_eq!(asked, [(0, setpoint)], "{axis:?}");
        }
        assert_eq!(logged(&session, &ChannelId::Throttle), [(0, 50.0)]);
    }

    #[test]
    fn the_third_log_of_round_16_counts_its_samples() {
        let probe = open_end("v3-samp-1k");
        let flight = &probe.flight;
        assert_eq!((flight.fs, flight.len()), (1000.0, 7000));
        let (_, session) = log_of(&probe);
        let time = &session.timebase.time_us;
        assert_eq!(
            (time[2000] - time[0], time[6999] - time[0]),
            (2_000_000, 6_999_000)
        );
        // pitch: the number of the sample, which the writer and the reader
        // leave as it is
        let pitch = gyro_of(&session, Axis::Pitch, GyroStage::Filtered);
        assert_eq!(pitch.len(), 7000);
        assert!(pitch.iter().enumerate().all(|(i, v)| *v == i as f64));
        let exact = flight.samples(Axis::Pitch).filtered;
        assert!(
            exact
                .iter()
                .enumerate()
                .all(|(i, v)| (v - i as f64).abs() < 1e-9)
        );
        // roll: 500 deg/s in one sample, in five and in fifty
        let roll = gyro_of(&session, Axis::Roll, GyroStage::Filtered);
        assert_eq!(
            steps(&roll),
            [
                (0, 0.0),
                (2000, 500.0),
                (2001, 0.0),
                (4000, 500.0),
                (4005, 0.0),
                (5000, 500.0),
                (5050, 0.0)
            ]
        );
        assert_eq!(roll, flight.samples(Axis::Roll).filtered);
        // yaw: noise of 20 deg/s RMS, and 200 deg/s more from sample 3500 on
        let yaw = gyro_of(&session, Axis::Yaw, GyroStage::Filtered);
        let exact = flight.samples(Axis::Yaw).filtered;
        assert!(
            yaw.iter()
                .zip(&exact)
                .all(|(g, w)| *g == w.round_ties_even())
        );
        let noise = Wave::Noise {
            rms: 20.0,
            seed: 1601,
        };
        let alone = FlightSpec {
            yaw: AxisSpec::of(vec![noise.clone()]),
            ..FlightSpec::quiet(1000.0, 7.0)
        };
        let alone = alone.samples(Axis::Yaw).unfiltered;
        for (i, (with_step, noise)) in exact.iter().zip(&alone).enumerate() {
            let step = if i >= 3500 { 200.0 } else { 0.0 };
            assert_eq!(*with_step, noise + step, "sample {i}");
        }
        let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len() as f64;
        let rms = (alone.iter().map(|v| v * v).sum::<f64>() / 7000.0).sqrt();
        assert!((rms - 20.0).abs() < 0.5, "{rms}");
        assert!(mean(&yaw[..3500]).abs() < 1.0 && (mean(&yaw[3500..]) - 200.0).abs() < 1.0);
        let Filtered::Separate { waves } = &flight.yaw.filtered else {
            panic!("the yaw of v3 has a filtered gyro of its own");
        };
        assert_eq!(waves[0], noise);
        // the unfiltered gyro, the setpoint and the throttle stay as a quiet
        // flight has them
        for axis in Axis::ALL {
            let raw = gyro_of(&session, axis, GyroStage::Raw);
            assert!(raw.iter().all(|v| *v == 0.0), "{axis:?}");
            let asked = logged(&session, &ChannelId::Setpoint { axis });
            assert_eq!(asked, [(0, 0.0)], "{axis:?}");
        }
        assert_eq!(logged(&session, &ChannelId::Throttle), [(0, 50.0)]);
        assert!(probe.purpose[0].contains("(4000 to 4004)"));
        assert!(probe.purpose[0].contains("(5000 to 5049)"));
        assert!(probe.purpose[2].contains("seed 1601") && probe.purpose[2].contains("sample 3500"));
    }

    #[test]
    fn the_flights_before_round_16_say_nothing_of_its_parts() {
        // their manifests and their logs stay what they were: a flight
        // without the parts round 16 added is written down without them
        let parts = ["stick_throttle", "motor_output", "triangle", "pulse"];
        let earlier = ROUNDS.iter().filter(|r| **r < 16);
        let again = round(16).unwrap().into_iter().take(3);
        for probe in earlier.flat_map(|r| round(*r).unwrap()).chain(again) {
            let text = serde_json::to_string(&probe.flight).unwrap();
            for part in parts {
                assert!(!text.contains(part), "{}: {part}", probe.id);
            }
            if probe.flight.debug.is_none() {
                assert_eq!(
                    crate::pack::probe_headers(&probe),
                    crate::pack::craft_headers(probe.id),
                    "{}",
                    probe.id
                );
            }
        }
        let new: Vec<String> = round(16).unwrap()[3..6]
            .iter()
            .map(|p| serde_json::to_string(&p.flight).unwrap())
            .collect();
        assert!(new[0].contains("\"stick_throttle\":50.0") && new[0].contains("\"triangle\""));
        assert!(new[1].contains("\"motor_output\":{\"low\":1070,\"high\":2000,\"protocol\":1}"));
        assert!(new[2].contains("\"pulse\"") && !new[2].contains("stick_throttle"));
        // the stairs of round 16 are written down as those of round 8 are
        let stairs = serde_json::to_string(&round(16).unwrap()[6].flight).unwrap();
        assert!(parts.iter().all(|part| !stairs.contains(part)));
    }

    #[test]
    fn round_14_takes_one_flight_of_round_6_to_two_windows() {
        let probes = round(14).unwrap();
        assert_eq!(probes.len(), 1);
        assert_eq!(
            (probes[0].id, probes[0].area),
            ("n1-more-2k", Area::Windows)
        );
        assert_eq!(probes[0].flight, round(6).unwrap().remove(0).flight);
    }

    #[test]
    fn round_13_takes_two_flights_of_round_6_to_the_other_windows() {
        let probes = round(13).unwrap();
        let flights = round(6).unwrap();
        assert_eq!(probes.len(), 2);
        for (probe, flight) in probes.iter().zip(&flights) {
            assert_eq!(probe.area, Area::Windows);
            assert_eq!(probe.flight, flight.flight);
        }
        assert_eq!((probes[0].id, probes[1].id), ("m1-ask-2k", "m2-ask-1k"));
        // at the rates their names say
        assert_eq!((probes[0].flight.fs, probes[1].flight.fs), (2000.0, 1000.0));
    }

    #[test]
    fn round_12_takes_a_flight_of_round_6_to_the_other_windows() {
        let probes = round(12).unwrap();
        assert_eq!(probes.len(), 1);
        let look = &probes[0];
        assert_eq!((look.id, look.area), ("k1-look-2k", Area::Windows));
        let flight = round(6).unwrap().remove(0);
        assert_eq!(look.flight, flight.flight);
        assert_eq!(look.purpose, flight.purpose);
        assert_ne!(look.title, flight.title);
    }

    #[test]
    fn round_11_asks_the_map_against_time_about_lengths_rates_and_a_flight() {
        let probes = round(11).unwrap();
        let shape: Vec<(&str, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("g1-levels-2k", 2000.0, 30.0),
                ("g2-len8-2k", 2000.0, 8.0),
                ("g3-len45-2k", 2000.0, 45.0),
                ("g4-len84-2k", 2000.0, 84.0),
                ("g5-rate-1k", 1000.0, 30.0),
                ("g6-rate-8k", 8000.0, 30.0),
                ("g7-flight-2k", 2000.0, 24.0),
            ]
        );
        // the names are in the order of the list the tool shows them in
        let mut sorted: Vec<&str> = probes.iter().map(|p| p.id).collect();
        sorted.sort_unstable();
        assert_eq!(sorted, probes.iter().map(|p| p.id).collect::<Vec<_>>());
        // the first six are the first probe of round 10 but for the rate
        // and the length
        let bursts = round(10).unwrap().remove(6).flight;
        for p in &probes[..6] {
            assert_eq!(p.area, Area::Time);
            let same = FlightSpec {
                fs: bursts.fs,
                duration_s: bursts.duration_s,
                ..p.flight.clone()
            };
            assert_eq!(same, bursts, "{}", p.id);
        }
        // the stretches lie on either side of the lengths at which the
        // throttle window changes the distance between its segments
        let stretch = |k: usize| probes[k].flight.duration_s - 2.0;
        assert!(stretch(1) < 10.0 && stretch(0) > 20.0 && stretch(0) < 60.0);
        assert!(stretch(2) > 20.0 && stretch(2) < 60.0 && stretch(3) > 60.0);
        // the flight: ten levels of the throttle over its 24 s, the sticks
        // still, every tone below the Nyquist frequency at any throttle
        let flight = &probes[6].flight;
        assert_eq!(probes[6].area, Area::Time);
        let throttle = flight.throttle_samples();
        let levels: BTreeSet<i64> = throttle.iter().map(|t| t.round() as i64).collect();
        assert_eq!(levels.len(), 10);
        assert_eq!((throttle[0], throttle[47_999]), (40.0, 52.0));
        for axis in Axis::ALL {
            let spec = flight.axis(axis);
            assert_eq!(spec.stick, Stick::Silence);
            let filtered: &[Wave] = match &spec.filtered {
                Filtered::Same => &[],
                Filtered::Separate { waves } => waves,
            };
            for wave in spec.waves.iter().chain(filtered) {
                match wave {
                    Wave::ThrottleTone {
                        hz_at_zero,
                        hz_per_percent,
                        ..
                    } => assert!(hz_at_zero + 100.0 * hz_per_percent < 1000.0),
                    Wave::Tone { hz, .. } => assert!(*hz < 1000.0),
                    Wave::Band { to_hz, .. } => assert!(*to_hz < 1000.0),
                    _ => {}
                }
            }
            let s = flight.samples(axis);
            assert!(s.unfiltered.iter().all(|v| v.abs() < 100.0), "{axis:?}");
        }
    }

    #[test]
    fn round_10_has_six_lengths_and_four_flights_for_the_map_against_time() {
        let probes = round(10).unwrap();
        let shape: Vec<(&str, Area, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.area, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("e1-len23-2k", Area::Throttle, 2000.0, 23.25),
                ("e2-len25-2k", Area::Throttle, 2000.0, 25.5),
                ("e3-len27-2k", Area::Throttle, 2000.0, 27.75),
                ("e4-len59-2k", Area::Throttle, 2000.0, 59.25),
                ("e5-len66-2k", Area::Throttle, 2000.0, 66.0),
                ("e6-len75-2k", Area::Throttle, 2000.0, 75.0),
                ("f1-bursts-2k", Area::Time, 2000.0, 30.0),
                ("f2-steps-2k", Area::Time, 2000.0, 30.0),
                ("f3-bursts-4k", Area::Time, 4000.0, 30.0),
                ("f4-short-2k", Area::Time, 2000.0, 16.5),
            ]
        );
        // the names are in the order of the list the tool shows them in
        let mut sorted: Vec<&str> = probes.iter().map(|p| p.id).collect();
        sorted.sort_unstable();
        assert_eq!(sorted, probes.iter().map(|p| p.id).collect::<Vec<_>>());
        // the lengths lie between those round 9 found a change between
        let stretch = |p: &Probe| p.flight.duration_s - 2.0;
        assert!(
            probes[..3]
                .iter()
                .all(|p| stretch(p) > 19.0 && stretch(p) < 28.0)
        );
        assert!(
            probes[3..6]
                .iter()
                .all(|p| stretch(p) > 55.0 && stretch(p) < 82.0)
        );
        let stairs = round(8).unwrap().remove(0).flight;
        for p in &probes[..6] {
            let same = FlightSpec {
                duration_s: stairs.duration_s,
                throttle: stairs.throttle.clone(),
                ..p.flight.clone()
            };
            assert_eq!(same, stairs, "{}", p.id);
        }
        // the flights for the map against time keep the sticks still and
        // their tones below the Nyquist frequency; the bursts end before
        // the shortest flight does
        for p in &probes[6..] {
            for axis in Axis::ALL {
                let spec = p.flight.axis(axis);
                assert_eq!(spec.stick, Stick::Silence, "{}", p.id);
                let filtered: &[Wave] = match &spec.filtered {
                    Filtered::Same => &[],
                    Filtered::Separate { waves } => waves,
                };
                for wave in spec.waves.iter().chain(filtered) {
                    match wave {
                        Wave::Tone { hz, to_s, .. } => {
                            assert!(*hz < 500.0, "{} {hz}", p.id);
                            assert!(to_s.is_none_or(|t| t <= p.flight.duration_s), "{}", p.id);
                        }
                        Wave::Chirp { to_hz, .. } | Wave::Band { to_hz, .. } => {
                            assert!(*to_hz < 500.0, "{}", p.id);
                        }
                        _ => {}
                    }
                }
                let s = p.flight.samples(axis);
                let peak = s
                    .unfiltered
                    .iter()
                    .chain(&s.filtered)
                    .fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(peak > 2.0 && peak < 200.0, "{} {axis:?}: {peak}", p.id);
            }
        }
        let bursts = &probes[6].flight;
        for (k, fs, duration_s) in [(8, 4000.0, 30.0), (9, 2000.0, 16.5)] {
            let same = FlightSpec {
                fs: bursts.fs,
                duration_s: bursts.duration_s,
                ..probes[k].flight.clone()
            };
            assert_eq!(&same, bursts, "{}", probes[k].id);
            assert_eq!(
                (probes[k].flight.fs, probes[k].flight.duration_s),
                (fs, duration_s)
            );
        }
        // a burst is there when its title says so
        let roll = bursts.samples(Axis::Roll).unfiltered;
        let loud =
            |from: usize, to: usize| roll[from..to].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(loud(6000, 7000) > 9.0 && loud(7100, 11_900) < 3.0);
        assert!(loud(25_000, 25_200) > 9.0 && loud(27_000, 27_040) > 5.0);
    }

    /// The probe `id` of round 17.
    fn verifying(id: &str) -> Probe {
        let found = round(17).unwrap().into_iter().find(|p| p.id == id);
        found.unwrap_or_else(|| panic!("round 17 has no {id}"))
    }

    #[test]
    fn round_17_is_thirteen_logs_in_the_order_of_their_names() {
        let probes = round(17).unwrap();
        let shape: Vec<(&str, Area, f64, f64)> = probes
            .iter()
            .map(|p| (p.id, p.area, p.flight.fs, p.flight.duration_s))
            .collect();
        assert_eq!(
            shape,
            [
                ("e1-spec-1k", Area::Spectrum, 1000.0, 26.0),
                ("e2-spec-4k", Area::Spectrum, 4000.0, 34.0),
                ("e3-step-2k", Area::StepResponse, 2000.0, 28.0),
                ("e4-step-8k", Area::StepResponse, 8000.0, 22.0),
                ("e5-maps-2k", Area::Throttle, 2000.0, 47.0),
                ("e6-view-2k", Area::LogViewer, 2000.0, 24.0),
                ("i1-chan-2k", Area::Spectrum, 2000.0, 24.0),
                ("j1-path-2k", Area::StepResponse, 2000.0, 28.0),
                ("x1-samp-2k", Area::LogViewer, 2000.0, 7.0),
                ("x2-samp-4k", Area::LogViewer, 4000.0, 7.0),
                ("x3-len11-2k", Area::Throttle, 2000.0, 11.0),
                ("x4-len12-2k", Area::Throttle, 2000.0, 12.0),
                ("x5-len13-2k", Area::Throttle, 2000.0, 13.0),
            ]
        );
        let mut names: Vec<&str> = probes.iter().map(|p| p.id).collect();
        names.sort_unstable();
        assert!(names.iter().zip(&probes).all(|(n, p)| *n == p.id));
        // the log that counts its samples is that of round 16 at other rates
        let v3 = open_end("v3-samp-1k").flight;
        for id in ["x1-samp-2k", "x2-samp-4k"] {
            let x = verifying(id).flight;
            assert_eq!((&x.roll, &x.pitch), (&v3.roll, &v3.pitch), "{id}");
            let Filtered::Separate { waves } = &x.yaw.filtered else {
                panic!("{id}: no filtered gyro of its own");
            };
            let Wave::Pulse { samples, .. } = waves[1] else {
                panic!("{id}: no step");
            };
            assert_eq!(f64::from(samples), 3.5 * x.fs);
        }
        // the short stairs are those of round 16, a second a stair
        for (id, duration_s) in [("x3-len11-2k", 11.0), ("x5-len13-2k", 13.0)] {
            let stairs = verifying(id).flight;
            assert_eq!(stairs.roll, open_end("w7-len10-2k").flight.roll);
            assert_eq!(stairs.throttle, super::stairs(1.0, duration_s));
        }
    }

    #[test]
    fn the_flights_of_round_17_are_made_for_no_question() {
        let earlier: Vec<Probe> = ROUNDS
            .iter()
            .filter(|r| **r < 17)
            .flat_map(|r| round(*r).unwrap())
            .collect();
        let plants: Vec<Plant> = earlier
            .iter()
            .flat_map(|p| Axis::ALL.map(|axis| p.flight.axis(axis).plant))
            .collect();
        for probe in &round(17).unwrap()[..6] {
            let f = &probe.flight;
            // a rate and a length no earlier log of its window had together
            let same = |p: &Probe| {
                (p.area, p.flight.fs, p.flight.duration_s) == (probe.area, f.fs, f.duration_s)
            };
            assert!(!earlier.iter().any(same), "{}", probe.id);
            let Throttle::Path { points } = &f.throttle else {
                panic!("{}: the throttle does not wander", probe.id);
            };
            assert!(points.len() > 10 && points.last().unwrap().0 >= f.duration_s);
            for axis in Axis::ALL {
                let spec = f.axis(axis);
                assert!(matches!(spec.stick, Stick::Random { .. }), "{}", probe.id);
                assert!(!plants.contains(&spec.plant), "{} {axis:?}", probe.id);
                let s = f.samples(axis);
                let peak = s.unfiltered.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(
                    peak > 100.0 && peak < 2000.0,
                    "{} {axis:?}: {peak}",
                    probe.id
                );
                // the noise of the filtered gyro is the smaller
                let noise = |x: &[f64]| {
                    let d: Vec<f64> = x.iter().zip(&s.response).map(|(a, b)| a - b).collect();
                    d.iter().map(|v| v * v).sum::<f64>()
                };
                assert!(noise(&s.filtered) < 0.2 * noise(&s.unfiltered));
                // every component is below the Nyquist frequency
                let top = f.throttle_samples().iter().fold(0.0f64, |m, v| m.max(*v));
                for wave in &spec.waves {
                    let hz = match wave {
                        Wave::Tone { hz, .. } => *hz,
                        Wave::ThrottleTone {
                            hz_at_zero,
                            hz_per_percent,
                            ..
                        } => hz_at_zero + hz_per_percent * top,
                        _ => 0.0,
                    };
                    assert!(hz < f.fs / 2.0, "{} {axis:?}: {hz}", probe.id);
                }
            }
        }
        // the maps flight wanders over 20 to 80 % of throttle
        let maps = verifying("e5-maps-2k").flight.throttle_samples();
        let (low, high) = maps
            .iter()
            .fold((100.0f64, 0.0f64), |(l, h), v| (l.min(*v), h.max(*v)));
        assert_eq!((low, high), (20.0, 80.0));
    }

    #[test]
    fn the_log_of_the_other_channels_carries_a_tone_on_each() {
        let probe = verifying("i1-chan-2k");
        let (head, session) = log_of(&probe);
        assert!(head.contains("H debug_mode:1"));
        let fs = probe.flight.fs;
        let tone_of = |id: ChannelId, hz: f64| {
            let c = session.channel(&id).unwrap();
            let values = c.data.values();
            let (mut s, mut co) = (0.0, 0.0);
            for (i, v) in values.iter().enumerate() {
                let phase = std::f64::consts::TAU * hz * i as f64 / fs;
                s += v * phase.sin();
                co += v * phase.cos();
            }
            2.0 * (s * s + co * co).sqrt() / values.len() as f64
        };
        let roll = Axis::Roll;
        let term = |term: PidTerm| ChannelId::PidTerm { axis: roll, term };
        let near = |got: f64, want: f64| (got - want).abs() < 0.05 * want;
        let tones = &LISTED[0];
        assert!(near(tone_of(term(PidTerm::P), tones.p.0), tones.p.1));
        assert!(near(tone_of(term(PidTerm::D), 180.0), 3.0));
        assert!(near(tone_of(term(PidTerm::F), 60.0), 5.0));
        assert!(tone_of(term(PidTerm::I), 120.0) < 0.01);
        assert!(near(tone_of(term(PidTerm::Sum), 120.0), 4.0));
        assert!(near(tone_of(ChannelId::Setpoint { axis: roll }, 30.0), 6.0));
        assert!(near(
            tone_of(ChannelId::PidError { axis: roll }, 240.0),
            2.5
        ));
        let gyro = |stage: GyroStage| ChannelId::Gyro { axis: roll, stage };
        assert!(near(tone_of(gyro(GyroStage::Filtered), 240.0), 2.5));
        assert!(near(tone_of(gyro(GyroStage::Raw), 300.0), 7.0));
        assert!(near(tone_of(ChannelId::Motor { idx: 0 }, 85.0), 1.5));
        assert!(near(tone_of(ChannelId::Motor { idx: 3 }, 165.0), 3.0));
        assert!(near(tone_of(ChannelId::Debug { idx: 0 }, 330.0), 8.0));
        // no D term on yaw: the craft has no D gain there
        let yaw_d = ChannelId::PidTerm {
            axis: Axis::Yaw,
            term: PidTerm::D,
        };
        assert!(session.channel(&yaw_d).is_none());
    }

    #[test]
    fn the_log_of_the_stick_s_path_parts_the_rc_command_and_the_feedforward() {
        let probe = verifying("j1-path-2k");
        let (_, session) = log_of(&probe);
        let values = |id: ChannelId| session.channel(&id).unwrap().data.values().to_vec();
        let setpoint = values(ChannelId::Setpoint { axis: Axis::Roll });
        let rc = values(ChannelId::RcCommand { ch: 0 });
        // 40 ms later (80 samples) and half as large, in the units of a log:
        // a quarter of the setpoint's deg/s, rounded
        for i in (80..setpoint.len()).step_by(97) {
            assert!((rc[i] - setpoint[i - 80] / 4.0).abs() <= 0.5, "{i}");
        }
        // pitch keeps the setpoint's image
        let pitch = values(ChannelId::Setpoint { axis: Axis::Pitch });
        let image = values(ChannelId::RcCommand { ch: 1 });
        assert!(
            pitch
                .iter()
                .zip(&image)
                .all(|(s, r)| (r - s / 2.0).abs() <= 0.5)
        );
        // the feedforward: every move of the setpoint for 10 ms (20 samples)
        let f = values(ChannelId::PidTerm {
            axis: Axis::Roll,
            term: PidTerm::F,
        });
        let moves: Vec<usize> = (1..setpoint.len())
            .filter(|i| setpoint[*i] != setpoint[i - 1])
            .collect();
        assert!(moves.len() > 30);
        for i in &moves {
            assert_eq!(f[*i], setpoint[*i] - setpoint[i - 1], "{i}");
            assert_eq!(f[i + 19], f[*i]);
        }
        let first = usize::from(setpoint[0] != 0.0);
        assert_eq!(
            f.iter().filter(|v| **v != 0.0).count(),
            20 * (moves.len() + first)
        );
        let yaw_f = values(ChannelId::PidTerm {
            axis: Axis::Yaw,
            term: PidTerm::F,
        });
        assert!(yaw_f.iter().all(|v| *v == 0.0));
    }
}
