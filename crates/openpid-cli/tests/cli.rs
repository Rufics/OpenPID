//! End-to-end tests of the `openpid` binary.
#![allow(clippy::unwrap_used, reason = "tests may panic on unexpected data")]

use std::path::PathBuf;
use std::process::Command;

fn openpid() -> Command {
    Command::new(env!("CARGO_BIN_EXE_openpid"))
}

fn log(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/logs")
        .join(name)
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("openpid-cli-{}-{name}", std::process::id()))
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = openpid().args(args).output().expect("run openpid");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn help_and_version() {
    let (ok, out, _) = run(&["--help"]);
    assert!(ok && out.contains("USAGE"));
    let (ok, out, _) = run(&["--version"]);
    assert!(ok && out.starts_with("openpid "));
    let (ok, _, err) = run(&[]);
    assert!(!ok && err.contains("USAGE"));
    let (ok, _, err) = run(&["bogus"]);
    assert!(!ok && err.contains("unknown command"));
    let (ok, _, err) = run(&["info", "--nope"]);
    assert!(!ok && err.contains("unknown option"));
    let (ok, _, err) = run(&["info"]);
    assert!(!ok && err.contains("needs a file"));
    let (ok, _, err) = run(&["info", "/nonexistent/file.bbl"]);
    assert!(!ok && err.contains("cannot read"));
}

#[test]
fn info_text_and_json() {
    let l = log("betaflight-4.2.11-two-logs.bbl");
    let (ok, out, _) = run(&["info", l.to_str().unwrap()]);
    assert!(ok, "{out}");
    assert!(out.contains("session 0") && out.contains("session 1"));
    assert!(out.contains("Betaflight 4.2.11") && out.contains("gyro.filtered.roll"));
    assert!(
        out.contains("fitness  : [") && out.contains("[!]  rate"),
        "{out}"
    );
    let (ok, out, _) = run(&["info", l.to_str().unwrap(), "--json", "--session", "1"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["index"], 1);
    assert!(arr[0]["samples"].as_u64().unwrap() > 20_000);
    let fit = &arr[0]["fitness"];
    assert!(fit["overall"].is_string(), "{fit}");
    assert!(
        fit["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "raw_gyro")
    );
    let (ok, out, _) = run(&["info", l.to_str().unwrap(), "--raw"]);
    assert!(ok && out.contains("[profile raw]"));
    let (ok, _, err) = run(&["info", l.to_str().unwrap(), "--session", "x"]);
    assert!(!ok && err.contains("bad session"));
}

#[test]
fn decode_raw_and_scaled() {
    let l = log("betaflight-4.2.0.BFL");
    let (ok, out, _) = run(&["decode", l.to_str().unwrap()]);
    assert!(ok);
    let mut lines = out.lines();
    let header = lines.next().unwrap();
    assert!(header.starts_with("loopIteration,time,"));
    assert!(
        header.ends_with(",rxFlightChannelsValid"),
        "slow fields appended"
    );
    let first = lines.next().unwrap();
    assert_eq!(first.split(',').count(), header.split(',').count());
    assert_eq!(out.lines().count(), 16775);
    let tmp = temp("scaled.csv");
    let (ok, _, _) = run(&[
        "decode",
        l.to_str().unwrap(),
        "--scaled",
        "--out",
        tmp.to_str().unwrap(),
    ]);
    assert!(ok);
    let scaled = std::fs::read_to_string(&tmp).unwrap();
    std::fs::remove_file(&tmp).ok();
    assert!(
        scaled
            .lines()
            .next()
            .unwrap()
            .starts_with("loop_iteration (),time (us),")
    );
    assert!(scaled.contains("gyro.filtered.roll (deg/s)"));
    let (ok, _, err) = run(&["decode", l.to_str().unwrap(), "--session", "7"]);
    assert!(!ok && err.contains("no session 7"));
}

#[test]
fn decode_csv_input() {
    let l = log("betaflight-4.2.0.BFL");
    let tmp = temp("roundtrip.csv");
    let (ok, _, _) = run(&[
        "decode",
        l.to_str().unwrap(),
        "--out",
        tmp.to_str().unwrap(),
    ]);
    assert!(ok);
    // the CSV we wrote is itself a valid input (blackbox_decode compatible)
    let (ok, out, err) = run(&["info", tmp.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("format blackbox-csv"));
    let (ok, out, err) = run(&["decode", tmp.to_str().unwrap()]);
    std::fs::remove_file(&tmp).ok();
    let header = out.lines().next().unwrap_or("").to_string();
    assert!(ok, "decode of our own CSV failed: {err}");
    assert!(
        header.contains("gyro.filtered.roll"),
        "unexpected header: {}",
        &header[..header.len().min(300)]
    );
}

#[test]
fn run_analyses() {
    let l = log("betaflight-4.2.0.BFL");
    let f = l.to_str().unwrap();
    // 4.2.0 has only gentle stick inputs, below the default step threshold
    let sharp = log("betaflight-2025.12.2-btfl_027.bbl");
    let sharp = sharp.to_str().unwrap();
    for (args, key) in [
        (
            vec!["run", "step", sharp, "--axis", "roll", "--epoch", "5..20"],
            "metrics",
        ),
        (vec!["run", "psd", f, "--channel", "pid.d.roll"], "power_db"),
        (vec!["run", "thr-heatmap", f], "counts"),
        (vec!["run", "stft", f, "--channel", "motor.0"], "grid"),
    ] {
        let (ok, out, err) = run(&args);
        assert!(ok, "{args:?}: {err}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert!(v.get(key).is_some(), "{args:?}: missing {key}");
    }
    let tmp = temp("psd.json");
    let (ok, _, _) = run(&["run", "psd", f, "--out", tmp.to_str().unwrap()]);
    assert!(ok && std::fs::metadata(&tmp).unwrap().len() > 100);
    std::fs::remove_file(&tmp).ok();
    let (ok, _, err) = run(&["run", "nope", f]);
    assert!(!ok && err.contains("unknown analysis"));
    let (ok, _, err) = run(&["run", "step", f, "--axis", "diagonal"]);
    assert!(!ok && err.contains("unknown axis"));
    let (ok, _, err) = run(&["run", "psd", f, "--channel", "nope.nope"]);
    assert!(!ok && err.contains("unknown channel key"));
    let (ok, _, err) = run(&["run", "psd", f, "--channel", "debug.7"]);
    assert!(!ok && err.contains("missing channel"));
    // automatic windows: the trim and the most active 20 s
    let (ok, out, err) = run(&[
        "run", "step", sharp, "--epoch", "auto", "--axis", "yaw", "--json",
    ]);
    assert!(ok, "{err}");
    assert!(err.contains("epoch 1.43..38.60 s"), "{err}");
    assert!(out.contains("windows_used") && out.contains("windows_rejected"));
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let verdicts = v["verdicts"].as_array().expect("verdicts in the step JSON");
    assert_eq!(verdicts.len(), 7);
    assert!(
        verdicts
            .iter()
            .any(|x| x["metric"] == "overshoot" && x["level"] == "Warn"),
        "{verdicts:?}"
    );
    // gentle stick inputs: not enough data at the default threshold
    let (ok, _, err) = run(&["run", "step", f, "--epoch", "auto"]);
    assert!(!ok && err.contains("above 80 deg/s"), "{err}");
    let (ok, _, err) = run(&["run", "step", f, "--epoch", "active"]);
    assert!(!ok && err.contains("shorter than 20 s"), "{err}");
    let short = log("betaflight-4.5.1-two-sessions.bbl");
    let (ok, _, err) = run(&[
        "run",
        "step",
        short.to_str().unwrap(),
        "--session",
        "0",
        "--epoch",
        "auto",
    ]);
    assert!(!ok && err.contains("less than 5 s in the air"), "{err}");
    let (ok, _, err) = run(&["run", "step", f, "--epoch", "5"]);
    assert!(!ok && err.contains("A..B"));
    let (ok, _, err) = run(&["run", "step"]);
    assert!(!ok && err.contains("needs a file"));
}

#[test]
fn sliders_command() {
    // the 4.2 fixtures predate simplified tuning: ratio sliders over the logged gains
    let l = log("betaflight-4.2.0.BFL");
    let (ok, out, err) = run(&["sliders", l.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.starts_with("ratio-sliders (Betaflight 4.2.0"), "{out}");
    assert!(out.contains("roll   P  77 I 119 D  83") && out.contains("no commands"));
    let (ok, out, _) = run(&[
        "sliders",
        l.to_str().unwrap(),
        "--set",
        "master_multiplier=110,pd_ratio=120",
    ]);
    // P 77 × 1.1, I 119 × 1.1, D 83 × 1.2 × 1.1, in Betaflight's whole numbers
    assert!(ok && out.contains("roll   P  85 I 131 D 110"), "{out}");
    // an ArduPilot log: the same sliders, gains at its 0.0001 step
    let a = log("ardupilot-copter-3.3-dronekit.bin");
    let (ok, out, _) = run(&[
        "sliders",
        a.to_str().unwrap(),
        "--set",
        "master_multiplier=120",
    ]);
    assert!(
        ok && out.contains("roll   P 0.1546 I 0.1546 D 0.0086"),
        "{out}"
    );
    let (ok, _, err) = run(&["sliders"]);
    assert!(!ok && err.contains("needs a file"));
    let (ok, _, err) = run(&["sliders", l.to_str().unwrap(), "--set", "nonsense"]);
    assert!(!ok && err.contains("bad --set entry"));
    // a 4.5 log with simplified tuning: every slider at 100 gives the firmware defaults
    let f = log("betaflight-4.5.0-gps-rescue.bbl");
    let f = f.to_str().unwrap();
    let (ok, out, err) = run(&["sliders", f]);
    assert!(ok, "{err}");
    assert!(
        out.contains("simplified_master_multiplier")
            && out.contains("computed gains:")
            && out.contains("logged gains:")
    );
    let all_100 = "simplified_master_multiplier=100,simplified_pi_gain=100,simplified_i_gain=100,\
                   simplified_d_gain=100,simplified_feedforward_gain=100,simplified_pitch_d_gain=100,\
                   simplified_pitch_pi_gain=100,simplified_d_max_gain=100";
    let (ok, out, _) = run(&["sliders", f, "--set", all_100, "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["result"]["pid"]["roll"]["p"], 45.0);
    assert_eq!(v["result"]["pid"]["roll"]["d"], 40.0);
    assert_eq!(v["result"]["pid"]["roll"]["d_min"], 30.0);
    assert_eq!(v["result"]["pid"]["pitch"]["f"], 125.0);
    let (ok, _, err) = run(&["sliders", f, "--set", "bogus=1"]);
    assert!(!ok && err.contains("unknown slider"));
}

#[test]
fn filter_sim_command() {
    let l = log("betaflight-2025.12.2-btfl_027.bbl");
    let f = l.to_str().unwrap();
    let (ok, out, err) = run(&["run", "filter-sim", f, "--axis", "roll"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let fit = &v["fit"];
    assert!(fit["residual_rel"].as_f64().unwrap() < 0.25, "{fit}");
    assert!(v["description"].as_array().unwrap().len() >= 3);
    assert_eq!(
        v["model"], v["whatif"],
        "no overrides: the what-if chain is the log's"
    );
    let (ok, out, err) = run(&[
        "run",
        "filter-sim",
        f,
        "--set",
        "lpf1_hz=100,lpf1_type=1,dyn_count=0",
    ]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let desc: Vec<String> = v["description"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap().to_string())
        .collect();
    assert!(
        desc.iter().any(|d| d == "lowpass 1: biquad 100 Hz"),
        "{desc:?}"
    );
    assert!(
        !desc.iter().any(|d| d.starts_with("dynamic notch")),
        "{desc:?}"
    );
    assert_ne!(v["model"], v["whatif"]);
    let (ok, _, err) = run(&["run", "filter-sim", f, "--set", "nope=1"]);
    assert!(!ok && err.contains("unknown filter key"), "{err}");
    let plain = log("betaflight-4.2.11-two-logs.bbl");
    let (ok, _, err) = run(&["run", "filter-sim", plain.to_str().unwrap()]);
    assert!(!ok && err.contains("missing channel"), "{err}");
}

#[test]
fn gen_command_writes_a_session_openpid_can_analyse() {
    let (ok, out, err) = run(&["gen", "--example"]);
    assert!(ok, "{err}");
    let spec: serde_json::Value = serde_json::from_str(&out).expect("json spec");
    assert_eq!(spec["fs"], 1000.0);
    assert_eq!(spec["setpoint"]["kind"], "steps");
    let csv = temp("synthetic.csv");
    let (ok, _, err) = run(&["gen", "--out", csv.to_str().unwrap()]);
    assert!(ok, "{err}");
    let (ok, out, err) = run(&["info", csv.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(
        out.contains("gyro.filtered.roll") && out.contains("motor_rpm.3"),
        "{out}"
    );
    let (ok, out, err) = run(&["run", "step", csv.to_str().unwrap()]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(v["metrics"]["delay_ms"].is_number());
    // a CSV carries no settings, so Filter Sim finds no chain: the fit equals
    // the unfiltered residual and the description says so
    let (ok, out, err) = run(&["run", "filter-sim", csv.to_str().unwrap()]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["description"], serde_json::json!(["no filters"]));
    assert_eq!(v["fit"]["residual_rel"], v["fit"]["unfiltered_rel"]);
    // an edited spec
    let spec_path = temp("spec.json");
    std::fs::write(
        &spec_path,
        r#"{"duration_s": 2, "motors": 2, "setpoint": {"kind": "sine", "hz": 3, "amplitude": 50}}"#,
    )
    .unwrap();
    let (ok, out, err) = run(&["gen", spec_path.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert_eq!(out.lines().count(), 2001, "header + 2 s at 1 kHz");
    assert!(!out.lines().next().unwrap().contains("motor_rpm.2"));
    std::fs::write(&spec_path, "{\"duration_s\": 0}").unwrap();
    let (ok, _, err) = run(&["gen", spec_path.to_str().unwrap()]);
    assert!(!ok && err.contains("no samples"), "{err}");
    std::fs::write(&spec_path, "{nope").unwrap();
    let (ok, _, err) = run(&["gen", spec_path.to_str().unwrap()]);
    assert!(!ok && err.contains("bad spec"), "{err}");
    std::fs::remove_file(&csv).ok();
    std::fs::remove_file(&spec_path).ok();
}

#[test]
fn png_of_a_heatmap() {
    let l = log("betaflight-4.2.0.BFL");
    let f = l.to_str().unwrap();
    let png = temp("thr.png");
    let (ok, out, err) = run(&["run", "thr-heatmap", f, "--png", png.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("\"grid\""), "JSON still printed");
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    assert!(
        bytes.len() > 4 * 101 * 100,
        "101 throttle bins × frequency rows"
    );
    let (ok, _, err) = run(&["run", "stft", f, "--png", png.to_str().unwrap()]);
    assert!(ok, "{err}");
    let (ok, _, err) = run(&["run", "psd", f, "--png", png.to_str().unwrap()]);
    assert!(!ok && err.contains("--png is for"), "{err}");
    std::fs::remove_file(&png).ok();
}

#[test]
fn the_time_map_takes_its_two_levels() {
    // betaflight-4.2.0.BFL is logged at 1 kHz: a segment of 0.3 s is 300
    // samples, the rows are the bins 0 … 150
    let l = log("betaflight-4.2.0.BFL");
    let f = l.to_str().unwrap();
    let map = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec!["run", "stft", f];
        args.extend_from_slice(extra);
        let (ok, out, err) = run(&args);
        assert!(ok, "{args:?}: {err}");
        serde_json::from_str(&out).expect("json")
    };
    let columns = |v: &serde_json::Value| v["grid"]["x"].as_array().unwrap().len();
    // by default both lists at "low": half a segment between the columns
    let low = map(&[]);
    assert_eq!(low["smooth_freq"], "low");
    assert_eq!(low["smooth_time"], "low");
    assert_eq!(
        (low["nperseg"].as_u64(), low["hop"].as_u64()),
        (Some(300), Some(150))
    );
    assert_eq!(low["grid"]["y"].as_array().unwrap().len(), 151);
    assert_eq!(low["grid"]["z_label"], "power density (dB, (deg/s)²/Hz)");
    // the other levels, "med" as the reference tool's list writes it
    let other = map(&["--smooth-freq", "off", "--smooth-time", "high"]);
    assert_eq!(other["smooth_freq"], "off");
    assert_eq!(other["smooth_time"], "high");
    assert_eq!(other["hop"], 30);
    assert!(columns(&other) > 4 * columns(&low));
    let medium = map(&["--smooth-freq", "med", "--smooth-time", "medium"]);
    assert_eq!(medium["smooth_freq"], "medium");
    assert_eq!(medium["hop"], 60);
    let off = map(&["--smooth-time", "off"]);
    assert_eq!(off["hop"], 300);
    assert!(columns(&off) < columns(&low));
    // the smoothing along the frequency changes the cells, not the grid
    assert_eq!(columns(&map(&["--smooth-freq", "high"])), columns(&low));
    assert_ne!(
        map(&["--smooth-freq", "high"])["grid"]["z"],
        low["grid"]["z"]
    );
    // a level that is not in the list is refused, and says what the list is
    for option in ["--smooth-freq", "--smooth-time"] {
        let (ok, _, err) = run(&["run", "stft", f, option, "loud"]);
        assert!(!ok, "{option}");
        assert!(
            err.contains(&format!("bad {option} loud: off, low, med or high")),
            "{err}"
        );
        let (ok, _, err) = run(&["run", "stft", f, option]);
        assert!(
            !ok && err.contains(&format!("{option} needs a level")),
            "{err}"
        );
    }
    // a project gives its levels (one saved before the map followed the
    // reference tool has none: both "low"), and the options win over them
    let project = temp("time.openpid.json");
    let saved = |stft: &str| {
        std::fs::write(
            &project,
            format!(
                r#"{{"version":2,"slots":[{{"path":"{}","session":0}}],"stft":{stft}}}"#,
                l.to_str().unwrap().replace('\\', "/")
            ),
        )
        .unwrap();
    };
    saved(r#"{"smooth_freq":"high","smooth_time":"off","max_hz":200.0}"#);
    let p = project.to_str().unwrap();
    let from_project = map(&["--project", p]);
    assert_eq!(from_project["smooth_freq"], "high");
    assert_eq!(from_project["hop"], 300);
    assert_eq!(
        from_project["grid"]["y"].as_array().unwrap().len(),
        61,
        "0 … 200 Hz"
    );
    let overridden = map(&["--project", p, "--smooth-time", "low"]);
    assert_eq!(overridden["smooth_freq"], "high");
    assert_eq!(overridden["hop"], 150);
    saved(r#"{"segment_s":0.2,"hop":0.25,"window":"hann","max_hz":1000.0}"#);
    let old = map(&["--project", p]);
    assert_eq!(old["smooth_freq"], "low");
    assert_eq!(
        (old["nperseg"].as_u64(), old["hop"].as_u64()),
        (Some(300), Some(150))
    );
    std::fs::remove_file(&project).ok();
    // the picture: a pixel per cell, the decibels on −30 … 10 dB, nothing
    // transparent (the first pixel is the top left cell)
    let png = temp("time.png");
    let (ok, _, err) = run(&["run", "stft", f, "--png", png.to_str().unwrap()]);
    assert!(ok, "{err}");
    let bytes = std::fs::read(&png).unwrap();
    let be =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    assert_eq!((be(16) as usize, be(20) as usize), (columns(&low), 151));
    // signature 8, IHDR 25, IDAT length and name 8, zlib 2, block 5, filter 1
    assert_eq!(bytes[49 + 3], 255, "the first pixel is opaque");
    std::fs::remove_file(&png).ok();
    let (ok, help, _) = run(&["--help"]);
    assert!(ok && help.contains("--smooth-freq") && help.contains("--smooth-time"));
}

#[test]
fn run_takes_the_log_and_parameters_from_a_project() {
    let project = temp("flight.openpid.json");
    let log = log("betaflight-4.2.0.BFL");
    std::fs::write(
        &project,
        format!(
            r#"{{"version":1,"slots":[{{"path":"{}","session":0,"epoch":{{"start_s":2.0,"end_s":8.0}}}}],"axis":"pitch","step":{{"cutoff_hz":40.0,"window_s":1.5,"min_input":20.0}}}}"#,
            log.to_str().unwrap().replace('\\', "/")
        ),
    )
    .unwrap();
    let (ok, out, err) = run(&["run", "step", "--project", project.to_str().unwrap()]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["axis"], "pitch", "axis from the project");
    assert_eq!(v["time_ms"].as_array().unwrap().len(), 500);
    // the epoch came from the project: fewer windows than the whole log
    let whole_project = temp("whole.openpid.json");
    std::fs::write(
        &whole_project,
        format!(
            r#"{{"version":1,"slots":[{{"path":"{}","session":0}}],"axis":"pitch","step":{{"min_input":20.0}}}}"#,
            log.to_str().unwrap().replace('\\', "/")
        ),
    )
    .unwrap();
    let (ok, whole, err) = run(&["run", "step", "--project", whole_project.to_str().unwrap()]);
    assert!(ok, "{err}");
    let w: serde_json::Value = serde_json::from_str(&whole).unwrap();
    assert!(v["windows_total"].as_u64().unwrap() < w["windows_total"].as_u64().unwrap());
    // flags override the project
    let (ok, out, _) = run(&[
        "run",
        "step",
        "--project",
        project.to_str().unwrap(),
        "--axis",
        "roll",
    ]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["axis"], "roll");
    std::fs::write(&project, r#"{"version":1,"slots":[]}"#).unwrap();
    let (ok, _, err) = run(&["run", "step", "--project", project.to_str().unwrap()]);
    assert!(!ok && err.contains("needs a file"), "{err}");
    std::fs::write(&project, r#"{"version":7}"#).unwrap();
    let (ok, _, err) = run(&["run", "step", "--project", project.to_str().unwrap()]);
    assert!(!ok && err.contains("newer"), "{err}");
    std::fs::remove_file(&project).ok();
}

#[test]
fn rpm_command() {
    let l = log("betaflight-4.5.1-two-sessions.bbl");
    let f = l.to_str().unwrap();
    let (ok, out, err) = run(&["run", "rpm", f, "--session", "1"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(
        v["median_error"].as_f64().unwrap() < 0.05,
        "{}",
        v["median_error"]
    );
    assert_eq!(
        v["rpm"].as_array().unwrap().len(),
        v["frames"].as_u64().unwrap() as usize
    );
    let (ok, _, err) = run(&["run", "rpm", f, "--channel", "gyro.filtered.roll"]);
    assert!(!ok && err.contains("gyro.raw"), "{err}");
}

#[test]
fn motor_noise_command() {
    let l = log("betaflight-4.5.1-two-sessions.bbl");
    let f = l.to_str().unwrap();
    let (ok, out, err) = run(&["run", "motor-noise", f, "--session", "1", "--axis", "pitch"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["rpm_source"], "logged");
    assert_eq!(v["motors"].as_array().unwrap().len(), 4);
    assert!(v["loudest"].is_number());
    assert_eq!(v["channel"]["gyro"]["axis"], "pitch");
}

#[test]
fn diff_command_lists_changed_headers_and_cli_commands() {
    let a = log("betaflight-4.5.1-two-sessions.bbl");
    let b = log("betaflight-2025.12.2-btfl_027.bbl");
    let (ok, out, err) = run(&["diff", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("A: ") && out.contains("Betaflight 4.5.1"));
    assert!(out.contains("B: ") && out.contains("Betaflight 2025.12.2"));
    assert!(out.contains("headers differ"));
    assert!(out.contains("d_min") && out.contains("d_max"), "{out}");
    assert!(!out.contains("Log start datetime"));
    // the same file against itself: nothing
    let (ok, out, _) = run(&["diff", a.to_str().unwrap(), a.to_str().unwrap()]);
    assert!(ok && out.contains("no header differs"), "{out}");
    // two sessions of one file
    let (ok, out, _) = run(&[
        "diff",
        a.to_str().unwrap(),
        a.to_str().unwrap(),
        "--session",
        "0",
        "--session-b",
        "1",
    ]);
    assert!(
        ok && out.contains("session 0") && out.contains("session 1"),
        "{out}"
    );
    // CLI commands turn A into B
    let (ok, out, _) = run(&["diff", a.to_str().unwrap(), b.to_str().unwrap(), "--cli"]);
    assert!(ok);
    assert!(out.contains("set d_max_roll = 40\n"), "{out}");
    assert!(
        out.contains("set p_roll = 45\n") || !out.contains("rollPID"),
        "{out}"
    );
    assert!(out.contains("# Firmware revision:") && out.contains("(not a CLI setting)"));
    assert!(out.trim_end().ends_with("save"), "{out}");
    // JSON
    let (ok, out, _) = run(&["diff", a.to_str().unwrap(), b.to_str().unwrap(), "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.as_array().unwrap().iter().any(|d| d["key"] == "d_max"));
    // errors
    let (ok, _, err) = run(&["diff", a.to_str().unwrap()]);
    assert!(!ok && err.contains("two files"));
    let (ok, _, err) = run(&[
        "diff",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--session",
        "9",
    ]);
    assert!(!ok && err.contains("no session 9"));
    // the commands are those of the log's firmware: none is made up for a
    // firmware whose profile has no exporter
    let ardu = log("ardupilot-copter-3.3-dronekit.bin");
    let (ok, out, err) = run(&[
        "diff",
        ardu.to_str().unwrap(),
        ardu.to_str().unwrap(),
        "--cli",
    ]);
    assert!(!ok && out.is_empty(), "{out}");
    assert!(err.contains("no command export for APM:Copter"), "{err}");
    let inav = log("inav-9.0.1-truncated.TXT");
    let (ok, _, err) = run(&["run", "recommend", inav.to_str().unwrap(), "--cli"]);
    assert!(!ok && err.contains("no command export for INAV"), "{err}");
}

#[test]
fn recommend_command_reports_evidence_and_cli_commands() {
    let a = log("betaflight-4.2.0.BFL");
    let (ok, out, err) = run(&["run", "recommend", a.to_str().unwrap()]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let findings = v["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f["rule"] == "overshoot" && f["axis"] == "roll")
    );
    assert!(findings.iter().any(|f| f["rule"] == "gyro-lpf"));
    let proposals = v["proposals"].as_array().unwrap();
    assert!(
        proposals
            .iter()
            .all(|p| p["confidence"].as_f64().unwrap() >= 0.3)
    );
    assert!(
        !proposals.iter().any(|p| p["path"] == "pid.yaw.d"),
        "no D on yaw"
    );
    let (ok, out, _) = run(&["run", "recommend", a.to_str().unwrap(), "--cli"]);
    assert!(ok);
    assert!(out.starts_with("# evidence\n# [overshoot roll]"), "{out}");
    assert!(out.contains("# OpenPID proposals for Betaflight 4.2.0"));
    if out.contains("set d_roll = ") {
        assert!(out.trim_end().ends_with("save"), "{out}");
    } else {
        assert!(out.contains("nothing to change"));
    }
    // an epoch with no stick input: findings explain, nothing is proposed blindly
    let (ok, out, _) = run(&[
        "run",
        "recommend",
        a.to_str().unwrap(),
        "--epoch",
        "0..1.5",
        "--cli",
    ]);
    assert!(ok, "{out}");
    assert!(!out.contains("set p_"), "{out}");
}
