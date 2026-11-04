//! Background thread: file loading and analyses never block the UI.

use openpid_analysis::{
    Analysis, AnalysisCtx, Epoch, PsdAnalysis, PsdOutput, PsdParams, StepAnalysis, StepOutput,
    StepParams, StftAnalysis, StftOutput, StftParams, ThrottleHeatmap, ThrottleHeatmapOutput,
    ThrottleHeatmapParams,
};
use openpid_core::{FlightSession, FormatRegistry, LogFile, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::A => "A",
            Slot::B => "B",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Kind {
    Step(StepParams),
    Psd(PsdParams),
    Throttle(ThrottleHeatmapParams),
    Stft(StftParams),
}

impl Kind {
    pub fn id(&self) -> &'static str {
        match self {
            Kind::Step(_) => "step",
            Kind::Psd(_) => "psd",
            Kind::Throttle(_) => "thr",
            Kind::Stft(_) => "stft",
        }
    }
}

#[derive(Debug)]
pub enum Output {
    Step(StepOutput),
    Psd(PsdOutput),
    Throttle(ThrottleHeatmapOutput),
    Stft(StftOutput),
}

/// Identity of one analysis request; equal keys give equal results.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobKey {
    pub slot: Slot,
    pub session: usize,
    pub kind: &'static str,
    pub params_hash: u64,
    pub epoch: Option<(u64, u64)>,
}

impl JobKey {
    pub fn new(slot: Slot, session: usize, kind: &Kind, epoch: Option<Epoch>) -> Self {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match kind {
            Kind::Step(p) => p.hash(&mut h),
            Kind::Psd(p) => p.hash(&mut h),
            Kind::Throttle(p) => p.hash(&mut h),
            Kind::Stft(p) => p.hash(&mut h),
        }
        JobKey {
            slot,
            session,
            kind: kind.id(),
            params_hash: h.finish(),
            epoch: epoch.map(|e| (e.start_s.to_bits(), e.end_s.to_bits())),
        }
    }
}

pub enum Job {
    Load {
        slot: Slot,
        path: PathBuf,
    },
    Analyze {
        key: JobKey,
        session: Arc<FlightSession>,
        kind: Kind,
        epoch: Option<Epoch>,
    },
}

pub enum Done {
    Loaded {
        slot: Slot,
        path: PathBuf,
        result: Result<Box<LogFile>, String>,
    },
    Analyzed {
        key: JobKey,
        result: Result<Box<Output>, String>,
    },
}

pub struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worker")
    }
}

fn registry() -> FormatRegistry {
    let mut r = FormatRegistry::new();
    r.register(Box::new(openpid_fmt_blackbox::BlackboxFormat::new()))
        .register(Box::new(openpid_fmt_csv::CsvFormat::new()))
        .register(Box::new(openpid_fmt_ardupilot::ArdupilotFormat::new()));
    r
}

fn load(path: &PathBuf) -> Result<LogFile, String> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = path.file_name().and_then(std::ffi::OsStr::to_str);
    let mut log = registry()
        .open(&data, name, &OpenOptions::default())
        .map_err(|e| e.to_string())?;
    log.source.path = Some(path.clone());
    Ok(log)
}

fn analyze(session: &FlightSession, kind: &Kind, epoch: Option<Epoch>) -> Result<Output, String> {
    let ctx = AnalysisCtx { session, epoch };
    match kind {
        Kind::Step(p) => StepAnalysis.run(&ctx, p).map(Output::Step),
        Kind::Psd(p) => PsdAnalysis.run(&ctx, p).map(Output::Psd),
        Kind::Throttle(p) => ThrottleHeatmap.run(&ctx, p).map(Output::Throttle),
        Kind::Stft(p) => StftAnalysis.run(&ctx, p).map(Output::Stft),
    }
    .map_err(|e| e.to_string())
}

impl Worker {
    /// Spawn the worker; `wake` is called after every finished job so the UI repaints.
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Worker {
        let (tx, job_rx) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        std::thread::Builder::new()
            .name("openpid-worker".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let done = match job {
                        Job::Load { slot, path } => {
                            let result = load(&path).map(Box::new);
                            Done::Loaded { slot, path, result }
                        }
                        Job::Analyze {
                            key,
                            session,
                            kind,
                            epoch,
                        } => {
                            let result = analyze(&session, &kind, epoch).map(Box::new);
                            Done::Analyzed { key, result }
                        }
                    };
                    if done_tx.send(done).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn worker thread");
        Worker { tx, rx }
    }

    pub fn submit(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    pub fn poll(&self) -> Vec<Done> {
        let mut out = Vec::new();
        while let Ok(d) = self.rx.try_recv() {
            out.push(d);
        }
        out
    }
}
