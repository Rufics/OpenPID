//! The command line of `openpid-probe`.

use crate::catalogue;
use crate::compare;
use crate::pack;
use std::io::Write;
use std::path::PathBuf;

/// The help text.
pub const USAGE: &str = "\
openpid-probe - calibration probes: synthetic flights of exactly known content

USAGE:
    openpid-probe list
    openpid-probe pack <round> --out <dir> [--built-from <commit>]
    openpid-probe compare <round> <capture-results-dir> [--ours <ours.json>] [--out <report.md>]

COMMANDS:
    list   The rounds and their probes: one log per line, with what each axis settles.
    pack   Write the pack of a round into <dir>: every probe as a Betaflight Blackbox
           log, manifest.json (what is true about each log), ours.json (what OpenPID
           computes for the same logs), predictions.json where there is a model (what the
           model of the reference tool expects it to show), and the capture script
           with its steps (capture.ps1, steps.json, run.cmd), which walks version
           0.94 of the reference tool through the round on Windows; a round run by
           hand (19 on) has no capture plan. The same round always gives the same
           bytes.
           --built-from records the commit in the manifest.
    compare
           Hold every vector copy (.emf) in <capture-results-dir> that the round's
           capture plan made against OpenPID's own result for the same log and
           setting, and write a Markdown report: one row per copy with its numbers
           and PASS or FAIL against the tolerances, or the decided difference
           (DECISIONS.md) it agrees with once that is taken out, and a summary.
           OpenPID's results are taken from --ours (the pack's ours.json, or a file
           of the schema in docs/calibration/PROTOCOL.md) and computed from the
           round's logs, at the app's default settings, where the file has none.
           Without --out the report is printed. Exits with 1 when a copy fails or
           cannot be read; a copy that is not the plan's, or whose setting OpenPID
           has no counterpart for, is listed as not compared.

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
    11       The map of frequency against time again: what its two smoothing lists do,
             flights of other lengths and rates, and a flight made for no question, to
             hold the rule of round 10 against.
    12       A first look at the windows of the sliders, of the filters and of the setup:
             one flight with stick moves, a picture of each window as it comes up.
    13       Those three windows asked: the slider tool with start gains and its sliders
             moved, the filter simulator's plots, lists, switches and four values typed
             in, the list of the setup with two logs.
    14       The filter simulator's other filter types and loop rates, its notch by the
             cut-off, its impulse response and chirp, a set of filters made for no
             question; the other entries of the slider tool's list.
    15       The main window, which draws the channels of a log as lines: one log whose
             every channel holds a known constant or step, every line of its check-list
             switched on, its four plots copied.
    16       What the rounds before left open in every window: three logs for the main
             window (a gyro that rises evenly beside two throttles, motor outputs that
             are not digital, a gyro that counts its samples), the stairs of the
             throttle in eight more lengths, three logs of rounds 9 to 11 again, and a
             summary of the tables the tool decodes the logs into.
    17       The verification round: every window again on flights made for no question
             (the spectrum, the step response, both maps, the main window, a set of filters
             typed into the filter simulator); every channel of the spectrum window's list
             with a tone of its own; the stick's path into the step response (an RC command
             and a feedforward that are not the setpoint's); the log of round 16 that counts
             its samples at 2 and 4 kHz, and its stairs in flights of 11, 12 and 13 s.
    18       The confirmation of the reference tool's stretch: flights made for no question
             for every window, short stairs of 10.1 to 10.9 s, a log whose clock runs off
             its rate and one that pauses, the step response from windows of 4 and 5 s.
    19       The third round with the web edition 1.0 of the reference tool (W3), run by
             hand: logs and a manifest only, no capture plan. The spectrum's columns
             \"Motor Noise\" (motors at speeds of their own, as eRPM) and \"Filt. Delay\" (a
             filtered gyro of known delay), the factor of \"D term raw\" (a D term that is a
             known factor times the change of the gyro), \"Max start\" of the step response
             (plants that lead the stick or jump at once), a log of 10.4 s, and the stairs
             of the throttle of 63 s at 1 and 8 kHz.
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
        Some("compare") => run_compare(&args[1..], out),
        Some("--help" | "-h" | "help") => print(out, USAGE.to_string()),
        Some(other) => Err(format!("unknown command {other}\n\n{USAGE}")),
        None => Err(USAGE.to_string()),
    }
}

/// `compare <round> <dir> [--ours <file>] [--out <file>]`.
fn run_compare(args: &[String], out: &mut dyn Write) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut ours: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--ours" => ours = Some(rest.next().ok_or("--ours needs a file")?.into()),
            "--out" => report = Some(rest.next().ok_or("--out needs a file")?.into()),
            other if other.starts_with("--") => {
                return Err(format!("unknown option {other}\n\n{USAGE}"));
            }
            other => positional.push(other),
        }
    }
    let [round, dir] = positional[..] else {
        return Err(format!("compare needs a round and a folder\n\n{USAGE}"));
    };
    let round = round
        .parse::<u32>()
        .map_err(|_| format!("the round must be a number, not {round}"))?;
    let result = compare::compare(round, dir.as_ref(), ours.as_deref())?;
    let text = result.markdown();
    match &report {
        Some(path) => {
            std::fs::write(path, &text)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            writeln!(out, "report written to {}", path.display())
        }
        None => write!(out, "{text}"),
    }
    .map_err(|e| format!("cannot print: {e}"))?;
    if result.passed() {
        Ok(())
    } else {
        Err(format!("round {round}: not every copy passed"))
    }
}
