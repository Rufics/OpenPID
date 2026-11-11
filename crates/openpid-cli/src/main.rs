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
    openpid run <analysis> [<file>] [--project <p.openpid.json>] [--session N] [--epoch A..B]
                [--axis roll|pitch|yaw] [--channel KEY] [--out <path>] [--png <path>]
                [--method curve|density] [--smoothing LEVEL]
    openpid sliders <file> [--session N] [--set KEY=VALUE,...] [--json]
    openpid diff <fileA> <fileB> [--session N] [--session-b M] [--json] [--cli]
    openpid gen [<spec.json>] [--out synthetic.csv | synthetic.bbl] | gen --example
    openpid --help | --version

COMMANDS:
    info     Summarise every session in a log file (firmware, rates, frames, channels)
    decode   Write one session as CSV. Default: raw integers with the logged field
             names (slow fields forward-filled), compatible with blackbox_decode.
             --scaled writes the canonical channels in physical units instead.

    run      Run an analysis and print its result as JSON. Analyses:
             step            step response (needs --axis, default roll)
             psd             spectrum of --channel: the smoothed curve of the whole log
                             (--method curve, the default) or Welch's power spectral
                             density (--method density)
             thr-heatmap     frequency x throttle heatmap of --channel
             stft            frequency x time heatmap of --channel
             motor-noise     per-motor harmonic amplitudes on the gyro (needs gyro.raw.<axis>;
                             motor speeds from eRPM or the rpm estimate)
             rpm             motor speed estimated from the gyro's noise peaks (needs
                             gyro.raw.<axis>; validated against motor_rpm when logged)
             filter-sim      unfiltered gyro through the modelled filter chain (needs
                             gyro.raw.<axis>; --set lpf1_hz=…,lpf1_type=… for what-if)
             recommend       rule-based tuning proposals with their evidence (overshoot →
                             more D, an uncovered spectrum peak → a notch, headroom above
                             the gyro LPF → a higher cutoff); --cli prints them as
                             commands of the log's firmware (Betaflight CLI `set …`)
                             instead of JSON
             Channel keys look like gyro.filtered.roll, gyro.raw.pitch, pid.d.roll,
             motor.0, debug.2 (see `openpid info`). Default: gyro.filtered.<axis>.

    sliders  PID sliders: the slider values the log flew with and the gains they
             produce; --set moves sliders and prints the resulting gains. A Betaflight
             4.3+ log has the firmware's simplified tuning and gets the CLI commands to
             apply it; any other log with PID gains has ratio sliders over those gains
             (master_multiplier, pd_ratio, pi_ratio, roll_pitch_ratio, yaw_ratio).

    diff     Headers (settings) that differ between two logs, A → B. --cli prints the
             commands of B's firmware (Betaflight CLI `set …`) that turn A's settings
             into B's (per-axis and min/max headers expanded, structural headers as
             comments), then `save`.

    gen      Write a synthetic session with known ground truth (stick steps or a
             chirp, a first-order plant with delay, motor harmonics tied to the
             throttle, white/pink noise, a PT1 'firmware' filter, jitter, dropouts)
             as a scaled CSV that `openpid` opens like any log. Without a spec the
             defaults are used; --example prints the default spec as JSON to edit.
             With --out <name>.bbl (or .bfl) it is written as a Betaflight Blackbox
             log instead, the way a flight controller logs it (whole deg/s, the
             firmware's frames and headers), for any tool that reads such logs.

OPTIONS:
    --session N   Restrict to session index N (0-based)
    --set K=V,…   Slider values, e.g. --set simplified_master_multiplier=110,simplified_d_gain=90;
                  for filter-sim the what-if filter keys: lpf1_hz, lpf1_type (0 PT1, 1 biquad,
                  2 PT2, 3 PT3), lpf1_dyn_min_hz, lpf1_dyn_max_hz, lpf2_hz, lpf2_type,
                  rpm_harmonics, rpm_q, rpm_min_hz, dyn_count, dyn_q, dyn_min_hz, dyn_max_hz,
                  notch_hz, notch_cutoff_hz (0 switches a stage off)
    --epoch A..B  Analyse only seconds A to B of the session; `auto` trims the
                  take-off and landing (throttle / motor rpm), `active` takes
                  the most active 20 s (stick input)
    --axis AXIS   roll, pitch or yaw
    --method M    psd: `curve` is one transform of the whole analysis window with its
                  decibels smoothed, as the reference tool draws it; `density` is
                  Welch's averaged power density
    --smoothing L psd, curve: low, low-med, medium (default), med-high or high
    --json        Machine-readable output
    --session-b M diff: session index in the second file (default: --session)
    --cli         diff / run recommend: the firmware's commands instead of the table / JSON
    --example     gen: print the default spec instead of generating
    --raw         Skip firmware-profile scaling and derived channels
    --out <path>  Write to a file instead of stdout
    --png <path>  thr-heatmap / stft: also write the grid as a PNG image (viridis)
    --project <p> run: take the log (slot A), session, epoch and tool parameters from
                  a project file saved by the GUI; --axis / --channel / --epoch override
";

#[derive(Debug, Default)]
struct Args {
    command: String,
    analysis: Option<String>,
    file: Option<PathBuf>,
    epoch: Option<(f64, f64)>,
    /// `auto` or `active` from `--epoch`.
    epoch_mode: Option<String>,
    set: Vec<(String, f64)>,
    axis: Option<String>,
    method: Option<String>,
    smoothing: Option<String>,
    channel: Option<String>,
    session: Option<usize>,
    json: bool,
    example: bool,
    raw: bool,
    scaled: bool,
    out: Option<PathBuf>,
    png: Option<PathBuf>,
    project: Option<PathBuf>,
    file_b: Option<PathBuf>,
    session_b: Option<usize>,
    cli: bool,
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
            "--example" => args.example = true,
            "--raw" => args.raw = true,
            "--scaled" => args.scaled = true,
            "--cli" => args.cli = true,
            "--session-b" => {
                let v = it.next().ok_or("--session-b needs a value")?;
                args.session_b = Some(v.parse().map_err(|_| format!("bad session index: {v}"))?);
            }
            "--session" => {
                let v = it.next().ok_or("--session needs a value")?;
                args.session = Some(v.parse().map_err(|_| format!("bad session index: {v}"))?);
            }
            "--out" => args.out = Some(PathBuf::from(it.next().ok_or("--out needs a path")?)),
            "--png" => args.png = Some(PathBuf::from(it.next().ok_or("--png needs a path")?)),
            "--project" => {
                args.project = Some(PathBuf::from(it.next().ok_or("--project needs a path")?));
            }
            "--epoch" => {
                let v = it.next().ok_or("--epoch needs A..B, auto or active")?;
                if v == "auto" || v == "active" {
                    args.epoch_mode = Some(v);
                    continue;
                }
                let (a, b) = v
                    .split_once("..")
                    .ok_or("--epoch needs the form A..B, auto or active")?;
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
            "--method" => args.method = Some(it.next().ok_or("--method needs a value")?),
            "--smoothing" => {
                args.smoothing = Some(it.next().ok_or("--smoothing needs a level")?);
            }
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
                } else if args.file.is_some() && args.command == "diff" && args.file_b.is_none() {
                    args.file_b = Some(PathBuf::from(a));
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
    let fitness: Vec<openpid_analysis::Fitness> = log
        .sessions
        .iter()
        .map(openpid_analysis::check_session)
        .collect();
    if args.json {
        let mut v = serde_json::to_value(&summaries).map_err(|e| e.to_string())?;
        if let serde_json::Value::Array(items) = &mut v {
            for (item, f) in items.iter_mut().zip(&fitness) {
                item["fitness"] = serde_json::to_value(f).map_err(|e| e.to_string())?;
            }
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
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
            let axis = |g: &openpid_core::settings::PidGains| {
                [g.p, g.i, g.d].map(|v| s.settings.gain_text(v)).join("/")
            };
            println!(
                "  pid      : roll {}  pitch {}  yaw {}",
                axis(&pid.roll),
                axis(&pid.pitch),
                axis(&pid.yaw)
            );
        }
        if let Some(lp) = &s.settings.gyro_filters.lowpass1 {
            println!(
                "  gyro lpf1: {} {} Hz",
                lp.kind
                    .as_ref()
                    .map_or("?".to_string(), |k| format!("{k:?}")),
                lp.cutoff_hz
            );
        }
        if let Some(f) = fitness.get(sum.index) {
            println!("  fitness  : {} overall", f.overall.marker());
            for c in &f.checks {
                let fix = c
                    .fix
                    .as_deref()
                    .map_or(String::new(), |x| format!("  -> {x}"));
                println!(
                    "    {:<4} {:<9}{}{fix}",
                    c.level.marker(),
                    c.title,
                    c.detail
                );
            }
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

fn cmd_gen(args: &Args) -> Result<(), String> {
    use openpid_sim::SignalSpec;
    if args.example {
        println!(
            "{}",
            serde_json::to_string_pretty(&SignalSpec::default()).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let spec: SignalSpec = match &args.file {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("{}: bad spec: {e}", p.display()))?
        }
        None => SignalSpec::default(),
    };
    if spec.is_empty() {
        return Err("the spec produces no samples (fs × duration_s < 1)".into());
    }
    let mut session = spec.build();
    if let Some(path) = args.out.as_ref().filter(|p| is_blackbox_name(p)) {
        // as a flight controller logs it: armed half a minute after power-up
        session.settings.loop_rates = openpid_fmt_blackbox::write::typical_rates(spec.fs);
        let options = openpid_fmt_blackbox::write::WriteOptions {
            time_offset_us: 30_000_000,
            ..Default::default()
        };
        let bytes = openpid_fmt_blackbox::write::betaflight(&session, &options)
            .map_err(|e| e.to_string())?;
        return std::fs::write(path, bytes)
            .map_err(|e| format!("cannot create {}: {e}", path.display()));
    }
    let log = LogFile {
        source: openpid_core::session::SourceInfo {
            path: None,
            size_bytes: 0,
            format_id: "synthetic".into(),
        },
        sessions: vec![session],
    };
    let mut sink: Box<dyn Write> = match &args.out {
        Some(p) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(p).map_err(|e| format!("cannot create {}: {e}", p.display()))?,
        )),
        None => Box::new(std::io::BufWriter::new(std::io::stdout().lock())),
    };
    write_scaled_csv(&log, 0, &mut sink)?;
    sink.flush().map_err(|e| e.to_string())
}

/// Whether `path` names a Blackbox log (`.bbl`, `.bfl`).
fn is_blackbox_name(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|e| e.eq_ignore_ascii_case("bbl") || e.eq_ignore_ascii_case("bfl"))
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
    // a project supplies the log, session, epoch and parameters of slot A
    let project = match &args.project {
        Some(p) => Some((openpid_analysis::Project::load(p)?, p.clone())),
        None => None,
    };
    let project_slot = project.as_ref().and_then(|(p, path)| {
        p.slots.first().map(|s| {
            (
                openpid_analysis::Project::resolve(path, &s.path),
                s.session,
                s.epoch,
            )
        })
    });
    let resolved_file = args
        .file
        .clone()
        .or_else(|| project_slot.as_ref().map(|(p, _, _)| p.clone()));
    let path = resolved_file
        .as_ref()
        .ok_or("run needs a file (or --project with a log)")?;
    let session = args
        .session
        .or_else(|| project_slot.as_ref().map(|(_, s, _)| *s))
        .unwrap_or(0);
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
    let epoch = match args.epoch_mode.as_deref() {
        Some("auto") => Some(
            openpid_analysis::epoch::auto(s)
                .ok_or("--epoch auto: no throttle, or less than 5 s in the air")?,
        ),
        Some("active") => Some(
            openpid_analysis::epoch::most_active(s, openpid_analysis::epoch::ACTIVE_WINDOW_S)
                .ok_or("--epoch active: no setpoint, or the log is shorter than 20 s")?,
        ),
        _ => args
            .epoch
            .map(|(a, b)| Epoch {
                start_s: a,
                end_s: b,
            })
            .or_else(|| project_slot.as_ref().and_then(|(_, _, e)| *e)),
    };
    if args.epoch_mode.is_some()
        && let Some(e) = epoch
    {
        eprintln!("epoch {:.2}..{:.2} s", e.start_s, e.end_s);
    }
    let ctx = AnalysisCtx { session: s, epoch };
    let base = project.as_ref().map(|(p, _)| p.clone()).unwrap_or_default();
    let axis = match (&args.axis, &project) {
        (None, Some(_)) => base.axis,
        (a, _) => parse_axis(a.as_deref())?,
    };
    let channel = match (&args.channel, &project) {
        (Some(k), _) => ChannelId::parse(k).ok_or_else(|| format!("unknown channel key {k}"))?,
        (None, Some(_)) if analysis == "psd" => base.spectrum_channel.clone(),
        (None, Some(_)) if analysis == "thr-heatmap" || analysis == "stft" => {
            base.heatmap_channel.clone()
        }
        (None, _) => ChannelId::Gyro {
            axis,
            stage: openpid_core::GyroStage::Filtered,
        },
    };
    let mut grid: Option<openpid_core::analysis::Grid2D> = None;
    let json = match analysis {
        "step" => {
            let p = openpid_analysis::StepParams { axis, ..base.step };
            let out = openpid_analysis::StepAnalysis
                .run(&ctx, &p)
                .map_err(|e| e.to_string())?;
            // the verdicts against the project's targets (or the defaults)
            let verdicts =
                openpid_analysis::verdicts(&out.metrics, out.windows_used, &base.recommend.targets);
            let mut v = serde_json::to_value(&out).map_err(|e| e.to_string())?;
            v["verdicts"] = serde_json::to_value(&verdicts).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&v)
        }
        "psd" => {
            let method = match args.method.as_deref() {
                None => base.psd.method,
                Some("curve") => openpid_analysis::PsdMethod::Curve,
                Some("density") => openpid_analysis::PsdMethod::Density,
                Some(other) => {
                    return Err(format!("unknown method {other}: curve or density"));
                }
            };
            let smoothing = match args.smoothing.as_deref() {
                None => base.psd.smoothing,
                Some(name) => openpid_analysis::CURVE_LEVELS
                    .iter()
                    .position(|level| *level == name)
                    .ok_or_else(|| {
                        format!(
                            "unknown smoothing level {name}: {}",
                            openpid_analysis::CURVE_LEVELS.join(", ")
                        )
                    })?,
            };
            let p = openpid_analysis::PsdParams {
                channel,
                method,
                smoothing,
                ..base.psd
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
                ..base.throttle
            };
            let out = openpid_analysis::ThrottleHeatmap
                .run(&ctx, &p)
                .map_err(|e| e.to_string())?;
            grid = Some(out.grid.clone());
            serde_json::to_string_pretty(&out)
        }
        "stft" => {
            let p = openpid_analysis::StftParams {
                channel,
                ..base.stft
            };
            let out = openpid_analysis::StftAnalysis
                .run(&ctx, &p)
                .map_err(|e| e.to_string())?;
            grid = Some(out.grid.clone());
            serde_json::to_string_pretty(&out)
        }
        "motor-noise" => {
            let p = openpid_analysis::MotorNoiseParams {
                channel: match &args.channel {
                    Some(_) => channel,
                    None => ChannelId::Gyro {
                        axis,
                        stage: openpid_core::GyroStage::Raw,
                    },
                },
                ..Default::default()
            };
            serde_json::to_string_pretty(
                &openpid_analysis::MotorNoiseAnalysis
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        "recommend" => {
            let out = openpid_analysis::Recommend
                .run(&ctx, &openpid_analysis::RecommendParams::default())
                .map_err(|e| e.to_string())?;
            if args.cli {
                use std::fmt::Write as _;
                let mut text = String::from("# evidence\n");
                for f in &out.findings {
                    let _ = writeln!(
                        text,
                        "# [{}{}] {}",
                        f.rule,
                        f.axis.map_or(String::new(), |a| format!(" {}", a.name())),
                        f.text
                    );
                }
                if out.proposals.is_empty() {
                    text.push_str("# nothing to change: every rule is satisfied\n");
                }
                let exporter = registry()
                    .exporter(s)
                    .ok_or_else(|| format!("no command export for {}", s.firmware.revision))?;
                text.push_str(&exporter.export(&out.proposals).map_err(|e| e.to_string())?);
                Ok(text)
            } else {
                serde_json::to_string_pretty(&out)
            }
        }
        "rpm" => {
            let p = openpid_analysis::RpmEstimateParams {
                channel: match &args.channel {
                    Some(_) => channel,
                    None => ChannelId::Gyro {
                        axis,
                        stage: openpid_core::GyroStage::Raw,
                    },
                },
                ..Default::default()
            };
            serde_json::to_string_pretty(
                &openpid_analysis::RpmEstimator
                    .run(&ctx, &p)
                    .map_err(|e| e.to_string())?,
            )
        }
        "filter-sim" => {
            let mut p = openpid_analysis::FilterSimParams {
                axis,
                ..base.filter_sim
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
    if let Some(png) = &args.png {
        let g = grid
            .as_ref()
            .ok_or_else(|| format!("--png is for thr-heatmap and stft, not {analysis}"))?;
        let (w, h, rgba) = openpid_analysis::render::grid_rgba(g, 0.0, 20.0);
        std::fs::write(png, openpid_analysis::render::png_rgba(w, h, &rgba))
            .map_err(|e| format!("cannot write {}: {e}", png.display()))?;
    }
    if let Some(p) = &args.out {
        std::fs::write(p, json).map_err(|e| format!("cannot write {}: {e}", p.display()))
    } else {
        println!("{json}");
        Ok(())
    }
}

/// One session of a file, decoded without raw mode.
fn session_of(path: &Path, index: usize) -> Result<openpid_core::FlightSession, String> {
    let (_, log) = open(
        path,
        &OpenOptions {
            sessions: Some(vec![index]),
            raw: false,
        },
    )?;
    log.sessions
        .into_iter()
        .find(|s| s.index == index)
        .ok_or_else(|| format!("{}: no session {index}", path.display()))
}

fn cmd_diff(args: &Args) -> Result<(), String> {
    let a = args.file.as_ref().ok_or("diff needs two files")?;
    let b = args.file_b.as_ref().ok_or("diff needs two files")?;
    let sa = args.session.unwrap_or(0);
    let sb = args.session_b.or(args.session).unwrap_or(0);
    let (sess_a, sess_b) = (session_of(a, sa)?, session_of(b, sb)?);
    let diff = openpid_core::diff_headers(&sess_a, &sess_b);
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&diff).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.cli {
        // the commands are for the craft of log B: its values are the target
        let exporter = registry()
            .exporter(&sess_b)
            .ok_or_else(|| format!("no command export for {}", sess_b.firmware.revision))?;
        print!("{}", exporter.diff(&diff));
        return Ok(());
    }
    println!(
        "A: {} session {sa} ({})",
        a.display(),
        sess_a.firmware.revision
    );
    println!(
        "B: {} session {sb} ({})",
        b.display(),
        sess_b.firmware.revision
    );
    if diff.is_empty() {
        println!("no header differs");
        return Ok(());
    }
    let settings = diff
        .iter()
        .filter(|d| openpid_core::is_setting_header(&d.key))
        .count();
    println!(
        "{} headers differ ({settings} settings, {} other)",
        diff.len(),
        diff.len() - settings
    );
    let width = diff.iter().map(|d| d.key.len()).max().unwrap_or(0);
    for d in &diff {
        println!(
            "  {:width$}  {}  →  {}",
            d.key,
            d.a.as_deref().unwrap_or("(absent)"),
            d.b.as_deref().unwrap_or("(absent)")
        );
    }
    Ok(())
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
    let model = registry().slider_model(s).ok_or_else(|| {
        format!(
            "{} has no slider model ({} logged no PID gains)",
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
    // gains as the firmware writes them (integers on Betaflight, fractions on ArduPilot)
    let text = |v: f64| s.settings.gain_text(v);
    let fmt = |g: &openpid_core::settings::PidGains| {
        format!(
            "P {:>3} I {:>3} D {:>3} F {:>4}{}",
            text(g.p),
            text(g.i),
            text(g.d),
            g.f.map_or("-".into(), text),
            g.d_min
                .map(|v| format!(" Dmin {}", text(v)))
                .or_else(|| g.d_max.map(|v| format!(" Dmax {}", text(v))))
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
    if result.cli.is_empty() {
        println!(
            "no commands: these sliders scale the logged gains; type the result into the \
             firmware's configurator"
        );
    } else {
        println!("CLI:");
        for c in &result.cli {
            println!("  {c}");
        }
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
        "diff" => cmd_diff(&args),
        "gen" => cmd_gen(&args),
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
