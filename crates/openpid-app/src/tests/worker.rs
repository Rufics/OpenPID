//! The worker thread and its job keys, without the UI.

use super::LOG_4_2;
use crate::app::Tab;
use crate::testkit::GENTLE_STEP_INPUT;
use crate::worker::{Done, Job, JobKey, Kind, Output, Slot, Worker};
use openpid_analysis::{Epoch, StepParams};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn wait_for(w: &Worker) -> Done {
    let t0 = Instant::now();
    loop {
        if let Some(d) = w.poll().pop() {
            return d;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "worker timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn worker_loads_and_analyses_off_thread() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = {
        let woken = woken.clone();
        Worker::spawn(move || {
            woken.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
    };
    w.submit(Job::Load {
        slot: Slot::A,
        path: LOG_4_2.into(),
    });
    let Done::Loaded {
        slot, path, result, ..
    } = wait_for(&w)
    else {
        panic!("expected a load result");
    };
    assert_eq!(slot, Slot::A);
    assert!(path.ends_with("betaflight-4.2.0.BFL"));
    let file = result.expect("decodes");
    assert_eq!(file.source.path.as_deref(), Some(Path::new(LOG_4_2)));
    let session = Arc::new(file.sessions[0].clone());
    let kind = Kind::Step(StepParams {
        min_input: GENTLE_STEP_INPUT,
        ..StepParams::default()
    });
    let key = JobKey::new(Slot::A, 0, &kind, None);
    w.submit(Job::Analyze {
        key: key.clone(),
        session: session.clone(),
        kind: kind.clone(),
        epoch: None,
    });
    let Done::Analyzed { key: k, result } = wait_for(&w) else {
        panic!("expected an analysis result");
    };
    assert_eq!(k, key);
    assert!(matches!(*result.expect("step runs"), Output::Step(_)));
    w.submit(Job::Analyze {
        key: key.clone(),
        session,
        kind: Kind::Step(StepParams {
            window_s: 0.0,
            ..StepParams::default()
        }),
        epoch: None,
    });
    let Done::Analyzed { result, .. } = wait_for(&w) else {
        panic!("expected an analysis result");
    };
    assert!(
        result.is_err(),
        "invalid params come back as an error string"
    );
    w.submit(Job::Load {
        slot: Slot::B,
        path: "/nowhere/missing.bbl".into(),
    });
    let Done::Loaded { result, .. } = wait_for(&w) else {
        panic!("expected a load result");
    };
    assert!(result.unwrap_err().contains("cannot read"));
    assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[test]
fn job_keys_identify_params_and_epoch() {
    let a = JobKey::new(Slot::A, 0, &Kind::Step(StepParams::default()), None);
    let b = JobKey::new(Slot::A, 0, &Kind::Step(StepParams::default()), None);
    assert_eq!(a, b);
    assert_eq!(a.kind, "step");
    let epoch = Some(Epoch {
        start_s: 0.0,
        end_s: 1.0,
    });
    assert_ne!(
        a,
        JobKey::new(Slot::A, 0, &Kind::Step(StepParams::default()), epoch)
    );
    assert_ne!(
        a,
        JobKey::new(Slot::B, 0, &Kind::Step(StepParams::default()), None)
    );
    assert_ne!(
        a,
        JobKey::new(Slot::A, 1, &Kind::Step(StepParams::default()), None)
    );
    assert_ne!(
        a,
        JobKey::new(
            Slot::A,
            0,
            &Kind::Psd(openpid_analysis::PsdParams::default()),
            None
        )
    );
    for (kind, id) in [
        (Kind::Psd(openpid_analysis::PsdParams::default()), "psd"),
        (
            Kind::Throttle(openpid_analysis::ThrottleHeatmapParams::default()),
            "thr",
        ),
        (Kind::Stft(openpid_analysis::StftParams::default()), "stft"),
    ] {
        assert_eq!(kind.id(), id);
    }
    assert_eq!(Slot::A.name(), "A");
    assert_eq!(Slot::B.name(), "B");
}

#[test]
fn tab_names_and_titles() {
    for (name, tab) in [
        ("log", Tab::LogViewer),
        ("step", Tab::Step),
        ("spectrum", Tab::Spectrum),
        ("throttle", Tab::ThrottleHeatmap),
        ("time", Tab::TimeHeatmap),
        ("surface", Tab::Surface),
        ("filter", Tab::FilterSim),
        ("advice", Tab::Advice),
        ("sliders", Tab::Sliders),
        ("diagnostics", Tab::Diagnostics),
        ("settings", Tab::Settings),
    ] {
        assert_eq!(Tab::from_name(name), Some(tab));
    }
    assert_eq!(Tab::from_name("nope"), None);
    // a project saved on the removed Motors tab opens on the default tab
    assert_eq!(Tab::from_name("motors"), None);
    let titles: std::collections::BTreeSet<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
    assert_eq!(titles.len(), Tab::ALL.len(), "titles are unique");
}

#[test]
fn command_line_gives_files_and_tab() {
    let args = |v: &[&str]| {
        v.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    };
    let (files, tab) = crate::parse_args(args(&["a.bbl", "--tab", "step", "b.bbl"]));
    assert_eq!(files, vec![PathBuf::from("a.bbl"), PathBuf::from("b.bbl")]);
    assert_eq!(tab, Some(Tab::Step));
    let (files, tab) = crate::parse_args(args(&["--tab", "bogus"]));
    assert!(files.is_empty());
    assert_eq!(tab, None);
    let (files, tab) = crate::parse_args(args(&["--tab"]));
    assert!(files.is_empty() && tab.is_none());
}
