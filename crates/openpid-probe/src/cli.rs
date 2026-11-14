//! The command line of `openpid-probe`.

use crate::catalogue;
use crate::pack;
use std::io::Write;
use std::path::PathBuf;

/// The help text.
pub const USAGE: &str = "\
openpid-probe - calibration probes: synthetic flights of exactly known content

USAGE:
    openpid-probe list
    openpid-probe pack <round> --out <dir> [--built-from <commit>]

COMMANDS:
    list   The rounds and their probes: one log per line, with what each axis settles.
    pack   Write the pack of a round into <dir>: every probe as a Betaflight Blackbox
           log, manifest.json (what is true about each log), ours.json (what OpenPID
           computes for the same logs), predictions.json where there is a model (what the
           model of the reference tool expects it to show), and the capture script
           with its steps (capture.ps1, steps.json, run.cmd), which walks version
           0.94 of the reference tool through the round on Windows. The same round
           always gives the same bytes.
           --built-from records the commit in the manifest.

ROUNDS:
    1 to 3   The spectrum. Round 3 is the hold-out: a flight as a craft flies it, which
             the model of the reference tool was not fitted to.
    4 to 6   The step response. Round 6 is the hold-out: five flights with a plant of
             its own on every axis, one of them at 8 kHz.
    7        The numbers version 0.94 of the reference tool reads off a step response
             (rise time, bandwidth, sensitivity peak, phase margin, crossover), asked
             with plants whose answers are known; the manifest carries the answers.
    8        The map of frequency against throttle, identify: stairs, a ramp and a hold
             of the throttle with tones and noise of known size, an experiment on each
             gyro channel of every axis.
    9        The same map: flights of six other lengths (how far apart its segments
             are), the motors going against the throttle (which of the two it reads),
             and a flight made for no question, to hold the rule of round 8 against.
    10       Six more lengths for the throttle map (where the distance between its
             segments changes) and a first look at the map of frequency against time:
             tones that come and go, a tone that climbs, steps of frequency and of
             loudness, noise.
";

/// Runs the tool with `args` (without the program name), printing to `out`.
///
/// # Errors
/// The message for the user when the arguments or the work fail.
pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), String> {
    let print = |out: &mut dyn Write, text: String| {
        writeln!(out, "{text}").map_err(|e| format!("cannot print: {e}"))
    };
    match args.first().map(String::as_str) {
        Some("list") => {
            for round in catalogue::ROUNDS {
                let probes = catalogue::round(*round).unwrap_or_default();
                print(out, format!("round {round}: {} logs", probes.len()))?;
                for probe in probes {
                    let f = &probe.flight;
                    print(
                        out,
                        format!(
                            "  {}.bbl  {} Hz, {} s  {}",
                            probe.id, f.fs, f.duration_s, probe.title
                        ),
                    )?;
                    for (axis, purpose) in ["roll", "pitch", "yaw"].iter().zip(probe.purpose) {
                        print(out, format!("      {axis:<5} {purpose}"))?;
                    }
                }
            }
            Ok(())
        }
        Some("pack") => {
            let mut round = None;
            let mut dir: Option<PathBuf> = None;
            let mut built_from = String::from("unknown");
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--out" => dir = Some(rest.next().ok_or("--out needs a directory")?.into()),
                    "--built-from" => {
                        built_from.clone_from(rest.next().ok_or("--built-from needs a commit")?);
                    }
                    other if other.starts_with("--") => {
                        return Err(format!("unknown option {other}\n\n{USAGE}"));
                    }
                    number => {
                        round =
                            Some(number.parse::<u32>().map_err(|_| {
                                format!("the round must be a number, not {number}")
                            })?);
                    }
                }
            }
            let round = round.ok_or("pack needs a round")?;
            let dir = dir.ok_or("pack needs --out <dir>")?;
            let pack = pack::build(round, &built_from).map_err(|e| e.to_string())?;
            pack.write_to(&dir).map_err(|e| e.to_string())?;
            for (name, bytes) in pack.files() {
                print(
                    out,
                    format!("{:>10}  {}", bytes.len(), dir.join(name).display()),
                )?;
            }
            Ok(())
        }
        Some("--help" | "-h" | "help") => print(out, USAGE.to_string()),
        Some(other) => Err(format!("unknown command {other}\n\n{USAGE}")),
        None => Err(USAGE.to_string()),
    }
}
