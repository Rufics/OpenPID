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
    let (ok, out, _) = run(&["info", l.to_str().unwrap(), "--json", "--session", "1"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["index"], 1);
    assert!(arr[0]["samples"].as_u64().unwrap() > 20_000);
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
    for (args, key) in [
        (
            vec!["run", "step", f, "--axis", "pitch", "--epoch", "2..12"],
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
    let (ok, _, err) = run(&["run", "step", f, "--epoch", "5"]);
    assert!(!ok && err.contains("A..B"));
    let (ok, _, err) = run(&["run", "step"]);
    assert!(!ok && err.contains("needs a file"));
}

#[test]
fn sliders_command() {
    // the committed 4.2 fixtures predate simplified tuning: a clear error
    let l = log("betaflight-4.2.0.BFL");
    let (ok, _, err) = run(&["sliders", l.to_str().unwrap()]);
    assert!(!ok && err.contains("no slider model"), "{err}");
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
    let (ok, out, err) = run(&["run", "filter-sim", csv.to_str().unwrap()]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(
        v["fit"]["residual_rel"].as_f64().unwrap() < 0.01,
        "{}",
        v["fit"]
    );
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
