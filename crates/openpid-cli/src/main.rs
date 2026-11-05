//! `openpid` command-line tool: inspect and decode logs headlessly.

use openpid_core::summary::SessionSummary;
use openpid_core::{ChannelId, FormatRegistry, LogFile, OpenOptions};
use openpid_fmt_blackbox::BlackboxFormat;
use openpid_fmt_csv::CsvFormat;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
openpid - blackbox log analysis toolkit

USAGE:
    openpid info <file> [--json] [--raw] [--session N]
    openpid decode <file> [--session N] [--out <path>] [--scaled]
    openpid run <analysis> <file> [--session N] [--epoch A..B] [--axis roll|pitch|yaw]
                [--channel KEY] [--out <path>]
    openpid sliders <file> [--session N] [--set KEY=VALUE,...] [--json]
    openpid --help | --version

COMMANDS:
    info     Summarise every session in a log file (firmware, rates, frames, channels)
    decode   Write one session as CSV. Default: raw integers with the logged field
             names (slow fields forward-filled), compatible with blackbox_decode.
             --scaled writes the canonical channels in physical units instead.

    run      Run an analysis and print its result as JSON. Analyses:
             step            step response (needs --axis, default roll)
             psd             Welch power spectral density of --channel
             thr-heatmap     frequency x throttle heatmap of --channel
             stft            frequency x time heatmap of --channel
             filter-sim      unfiltered gyro through the modelled filter chain (needs
                             gyro.raw.<axis>; --set lpf1_hz=…,lpf1_type=… for what-if)
             Channel keys look like gyro.filtered.roll, gyro.raw.pitch, pid.d.roll,
             motor.0, debug.2 (see `openpid info`). Default: gyro.filtered.<axis>.

    sliders  Betaflight PID sliders (simplified tuning): show the slider values the
             log flew with and the gains they produce; --set moves sliders and prints
             the resulting gains plus the CLI commands to apply them.

OPTIONS:
    --session N   Restrict to session index N (0-based)
    --set K=V,…   Slider values, e.g. --set simplified_master_multiplier=110,simplified_d_gain=90;
                  for filter-sim the what-if filter keys: lpf1_hz, lpf1_type (0 PT1, 1 biquad,
                  2 PT2, 3 PT3), lpf1_dyn_min_hz, lpf1_dyn_max_hz, lpf2_hz, lpf2_type,
                  rpm_harmonics, rpm_q, rpm_min_hz, dyn_count, dyn_q, dyn_min_hz, dyn_max_hz,
                  notch_hz, notch_cutoff_hz (0 switches a stage off)
    --epoch A..B  Analyse only seconds A to B of the session
    --axis AXIS   roll, pitch or yaw
    --json        Machine-readable output
    --raw         Skip firmware-profile scaling and derived channels
    --out <path>  Write to a file instead of stdout
";

#[derive(Debug, Default)]
struct Args {
    command: String,
    analysis: Option<String>,
    file: Option<PathBuf>,
    epoch: Option<(f64, f64)>,
    set: Vec<(String, f64)>,
    axis: Option<String>,
    channel: Option<String>,
    session: Option<usize>,
    json: bool,
    raw: bool,
    scaled: bool,
    out: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    let Some(cmd) = it.next() else {
        return Err(USAGE.to_string());
    };
    args.command = cmd;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => args.json = true,
            "--raw" => args.raw = true,
            "--scaled" => args.scaled = true,
            "--session" => {
                let v = it.next().ok_or("--session needs a value")?;
                args.session = Some(v.parse().map_err(|_| format!("bad session index: {v}"))?);
            }
            "--out" => args.out = Some(PathBuf::from(it.next().ok_or("--out needs a path")?)),
            "--epoch" => {
                let v = it.next().ok_or("--epoch needs A..B")?;
                let (a, b) = v.split_once("..").ok_or("--epoch needs the form A..B")?;
                let a: f64 = a
                    .trim()
                    .parse()
                    .map_err(|_| format!("bad epoch start: {a}"))?;
                let b: f64 = b
                    .trim()
                    .parse()
                    .map_err(|_| format!("bad epoch end: {b}"))?;
                args.epoch = Some((a, b));
            }
            "--axis" => args.axis = Some(it.next().ok_or("--axis needs a value")?),
            "--set" => {
                let v = it.next().ok_or("--set needs KEY=VALUE[,KEY=VALUE]")?;
                for pair in v.split(',').filter(|p| !p.trim().is_empty()) {
                    let (k, val) = pair
                        .split_once('=')
                        .ok_or_else(|| format!("bad --set entry: {pair}"))?;
                    let val: f64 = val
                        .trim()
                        .parse()
                        .map_err(|_| format!("bad slider value: {val}"))?;
                    args.set.push((k.trim().to_string(), val));
                }
            }
            "--channel" => args.channel = Some(it.next().ok_or("--channel needs a key")?),
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n\n{USAGE}")),
            _ => {
                if args.command == "run" && args.analysis.is_none() {
                    args.analysis = Some(a);
                } else if args.file.is_some() {
                    return Err(format!("unexpected argument {a}"));
                } else {
                    args.file = Some(PathBuf::from(a));
                }
            }
        }
    }
    Ok(args)
}

fn registry() -> FormatRegistry {
    let mut r = FormatRegistry::new();
    r.register(Box::new(BlackboxFormat::new()))
        .register(Box::new(CsvFormat::new()))
        .register(Box::new(openpid_fmt_ardupilot::ArdupilotFormat::new()));
    r
}

fn open(path: &Path, options: &OpenOptions) -> Result<(Vec<u8>, LogFile), String> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = path.file_name().and_then(std::ffi::OsStr::to_str);
    let mut log = registry()
        .open(&data, name, options)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    log.source.path = Some(path.to_path_buf());
    Ok((data, log))
}

fn cmd_info(args: &Args) -> Result<(), String> {
    let path = args.file.as_ref().ok_or("info needs a file")?;
    let options = OpenOptions {
        sessions: args.session.map(|s| vec![s]),
        raw: args.raw,
    };
    let (_, log) = open(path, &options)?;
    let summaries: Vec<SessionSummary> = log.sessions.iter().map(SessionSummary::of).collect();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    println!(
        "{} ({} bytes, format {})",
        path.display(),
        log.source.size_bytes,
        log.source.format_id
    );
    for (s, sum) in log.sessions.iter().zip(&summaries) {
        println!();
        println!("session {} @ byte {}", sum.index, sum.offset_bytes);
        println!(
            "  firmware : {}  [profile {}]",
            sum.firmware_revision, sum.profile_id
        );
        println!(
            "  samples  : {}  ({:.2} s, {:.1} Hz, median dt {} us{})",
            sum.samples,
            sum.duration_s,
            sum.sample_rate_hz,
            sum.median_dt_us,
            sum.nominal_dt_us
                .map_or(String::new(), |n| format!(", nominal {n:.1} us"))
        );
        if sum.gaps > 0 {
            println!("  gaps     : {}", sum.gaps);
        }
        let frames: Vec<String> = sum
            .frames
            .iter()
            .map(|(k, v)| format!("{k}:{}", v.valid))
            .collect();
        println!(
            "  frames   : {}  corrupt {}  desync {}  skipped bytes {}",
            frames.join(" "),
            sum.corrupt_frames,
            sum.desync_frames,
            sum.skipped_bytes
        );
        println!("  events   : {}", sum.events);
        if let Some(pid) = &s.settings.pid {
            println!(
                "  pid      : roll {}/{}/{}  pitch {}/{}/{}  yaw {}/{}/{}",
                pid.roll.p,
                pid.roll.i,
                pid.roll.d,
                pid.pitch.p,
                pid.pitch.i,
                pid.pitch.d,
                pid.yaw.p,
                pid.yaw.i,
                pid.yaw.d
            );
        }
        if let Some(lp) = &s.settings.gyro_filters.lowpass1 {
            println!("  gyro lpf1: {:?} {} Hz", lp.kind, lp.cutoff_hz);
        }
        println!("  channels : {}", sum.channels.len());
        for c in &sum.channels {
            println!(
                "    {:<24} {:<22} {:>6}  [{} .. {}]",
                c.key,
                format!("{} ({})", c.name, c.unit),
                c.valid,
                c.min,
                c.max
            );
        }
        if !sum.unmapped_fields.is_empty() {
            println!("  unmapped : {}", sum.unmapped_fields.join(", "));
        }
        for n in &s.diagnostics.notes {
            println!("  note     : {n}");
        }
    }
    Ok(())
}

fn write_raw_csv(data: &[u8], session: usize, out: &mut dyn Write) -> Result<(), String> {
    let segments = openpid_fmt_blackbox::split_logs(data);
    let (_, bytes) = segments
        .get(session)
        .ok_or_else(|| format!("no session {session}"))?;
    let log = openpid_fmt_blackbox::decode(bytes).map_err(|e| e.to_string())?;
    let mut header: Vec<&str> = log.field_names().iter().map(String::as_str).collect();
    header.extend(log.slow.columns.iter().map(String::as_str));
    writeln!(out, "{}", header.join(",")).map_err(|e| e.to_string())?;
    let mut slow_iter = log.slow.rows.iter().peekable();
    let mut slow_current: Vec<i64> = vec![0; log.slow.columns.len()];
    let mut line = String::new();
    for i in 0..log.len() {
        while let Some(row) = slow_iter.peek() {
            if row.sample_index <= i {
                slow_current.clone_from(&row.values);
                slow_iter.next();
            } else {
                break;
            }
        }
        line.clear();
        for (j, col) in log.columns.iter().enumerate() {
            if j > 0 {
                line.push(',');
            }
            line.push_str(&col[i].to_string());
        }
        for v in &slow_current {
            line.push(',');
            line.push_str(&v.to_string());
        }
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_scaled_csv(log: &LogFile, session: usize, out: &mut dyn Write) -> Result<(), String> {
    let s = log
        .sessions
        .iter()
        .find(|s| s.index == session)
        .ok_or_else(|| format!("no session {session}"))?;
    let mut channels: Vec<_> = s.channels.iter().collect();
    channels.sort_by_key(|c| {
        (
            c.id != ChannelId::LoopIteration,
            c.id != ChannelId::Time,
            c.id.key(),
        )
    });
    let header: Vec<String> = channels
        .iter()
        .map(|c| format!("{} ({})", c.id.key(), c.unit))
        .collect();
    writeln!(out, "{}", header.join(",")).map_err(|e| e.to_string())?;
    let mut line = String::new();
    for i in 0..s.len() {
        line.clear();
        for (j, c) in channels.iter().enumerate() {
            if j > 0 {
                line.push(',');
            }
            if let Some(v) = c.data.get(i) {
                use std::fmt::Write as _;
                let _ = write!(line, "{v}");
            }
        }
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cmd_decode(args: &Args) -> Result<(), String> {
    let path = args.file.as_ref().ok_or("decode needs a file")?;
    let session = args.session.unwrap_or(0);
    let options = OpenOptions {
        sessions: Some(vec![session]),
        raw: args.raw,
    };
    let (data, log) = open(path, &options)?;
    let mut sink: Box<dyn Write> = match &args.out {
        Some(p) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(p).map_err(|e| format!("cannot create {}: {e}", p.display()))?,
        )),
        None => Box::new(std::io::BufWriter::new(std::io::stdout().lock())),
    };
    if args.scaled || log.source.format_id != "blackbox-binary" {
        write_scaled_csv(&log, session, &mut sink)?;
    } else {
        write_raw_csv(&data, session, &mut sink)?;
    }
    sink.flush().map_err(|e| e.to_string())
}

fn parse_axis(s: Option<&str>) -> Result<openpid_core::Axis, String> {
    match s.unwrap_or("roll") {
        "roll" => Ok(openpid_core::Axis::Roll),
        "pitch" => Ok(openpid_core::Axis::Pitch),
        "yaw" => Ok(openpid_core::Axis::Yaw),
        other => Err(format!("unknown axis {other}")),
    }
}

fn cmd_run(args: &Args) -> Result<(), String> {
    use openpid_analysis::{Analysis, AnalysisCtx, Epoch};
    let analysis = args
        .analysis
        .as_deref()
        .ok_or("run needs an analysis name")?;
    let path = args.file.as_ref().ok_or("run needs a file")?;
    let session = args.session.unwrap_or(0);
    let (_, log) = open(
        path,
        &OpenOptions {
            sessions: Some(vec![session]),
            raw: false,
        },
    )?;
    let s = log
        .sessions
        .iter()
        .find(|s| s.index == session)
        .ok_or_else(|| format!("no session {session}"))?;
    let epoch = args.epoch.map(|(a, b)| Epoch {
        start_s: a,
        end_s: b,
    });
    let ctx = AnalysisCtx { session: s, epoch };
    let axis = parse_axis(args.axis.as_deref())?;
    let channel = match &args.channel {
        Some(k) => ChannelId::parse(k).ok_or_else(|| format!("unknown channel key {k}"))?,
        None => ChannelId::Gyro {
            axis,
            stage: openpid_core::GyroStage::Filtered,
        },
    };
    let json = match analysis {
        "step" => {
            let p = openpid_analysis::StepParams {
                axis,
                ..Default::default()
            };
            serde_json::to_string_pretty(
                &openpid_analysis::StepAnalysis
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        "psd" => {
            let p = openpid_analysis::PsdParams {
                channel,
                ..Default::default()
            };
            serde_json::to_string_pretty(
                &openpid_analysis::PsdAnalysis
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        "thr-heatmap" => {
            let p = openpid_analysis::ThrottleHeatmapParams {
                channel,
                ..Default::default()
            };
            serde_json::to_string_pretty(
                &openpid_analysis::ThrottleHeatmap
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        "stft" => {
            let p = openpid_analysis::StftParams {
                channel,
                ..Default::default()
            };
            serde_json::to_string_pretty(
                &openpid_analysis::StftAnalysis
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        "filter-sim" => {
            let mut p = openpid_analysis::FilterSimParams {
                axis,
                ..Default::default()
            };
            for (k, v) in &args.set {
                p.overrides.set(k, *v)?;
            }
            serde_json::to_string_pretty(
                &openpid_analysis::FilterSimAnalysis
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        other => return Err(format!("unknown analysis {other}\n\n{USAGE}")),
    }
    .map_err(|e| e.to_string())?;
    if let Some(p) = &args.out {
        std::fs::write(p, json).map_err(|e| format!("cannot write {}: {e}", p.display()))
    } else {
        println!("{json}");
        Ok(())
    }
}

fn cmd_sliders(args: &Args) -> Result<(), String> {
    let path = args.file.as_ref().ok_or("sliders needs a file")?;
    let session = args.session.unwrap_or(0);
    let (_, log) = open(
        path,
        &OpenOptions {
            sessions: Some(vec![session]),
            raw: false,
        },
    )?;
    let s = log
        .sessions
        .iter()
        .find(|s| s.index == session)
        .ok_or_else(|| format!("no session {session}"))?;
    let model = openpid_fmt_blackbox::slider_model_for(s).ok_or_else(|| {
        format!(
            "{} has no slider model (firmware {} without simplified tuning headers)",
            path.display(),
            s.firmware.revision
        )
    })?;
    let mut values = openpid_core::SliderValues::new();
    let known: Vec<String> = model.sliders().iter().map(|d| d.key.clone()).collect();
    for (k, v) in &args.set {
        if !known.contains(k) {
            return Err(format!("unknown slider {k}; known: {}", known.join(", ")));
        }
        values.insert(k.clone(), *v);
    }
    let result = model.compute(&values);
    if args.json {
        let out = serde_json::json!({ "model": model.id(), "sliders": model.sliders(), "set": values, "result": result });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    println!("{} ({})", model.id(), s.firmware.revision);
    for d in model.sliders() {
        let now = values.get(&d.key).copied().unwrap_or(d.value);
        let mark = if (now - d.value).abs() > f64::EPSILON {
            " *"
        } else {
            ""
        };
        println!(
            "  {:<36} {:>6} (log {}, default {}){mark}",
            d.key, now, d.value, d.default
        );
    }
    let fmt = |g: &openpid_core::settings::PidGains| {
        format!(
            "P {:>3} I {:>3} D {:>3} F {:>4}{}",
            g.p,
            g.i,
            g.d,
            g.f.map_or("-".into(), |v| v.to_string()),
            g.d_min
                .map(|v| format!(" Dmin {v}"))
                .or_else(|| g.d_max.map(|v| format!(" Dmax {v}")))
                .unwrap_or_default()
        )
    };
    println!("computed gains:");
    println!("  roll   {}", fmt(&result.pid.roll));
    println!("  pitch  {}", fmt(&result.pid.pitch));
    println!("  yaw    {}", fmt(&result.pid.yaw));
    if let Some(l) = model.logged() {
        println!("logged gains:");
        println!("  roll   {}", fmt(&l.roll));
        println!("  pitch  {}", fmt(&l.pitch));
        println!("  yaw    {}", fmt(&l.yaw));
    }
    if let Some((lo, hi)) = result.gyro_lpf1_hz {
        println!("gyro lpf1  {lo}..{hi} Hz");
    }
    if let Some(v) = result.gyro_lpf2_hz {
        println!("gyro lpf2  {v} Hz");
    }
    if let Some((lo, hi)) = result.dterm_lpf1_hz {
        println!("dterm lpf1 {lo}..{hi} Hz");
    }
    if let Some(v) = result.dterm_lpf2_hz {
        println!("dterm lpf2 {v} Hz");
    }
    println!("CLI:");
    for c in &result.cli {
        println!("  {c}");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let result = match args.command.as_str() {
        "info" => cmd_info(&args),
        "decode" => cmd_decode(&args),
        "run" => cmd_run(&args),
        "sliders" => cmd_sliders(&args),
        "--help" | "-h" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        "--version" | "-V" => {
            println!("openpid {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(format!("unknown command {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // `openpid decode ... | head` closes the pipe early; that is not an error.
        Err(e) if e.contains("Broken pipe") => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
