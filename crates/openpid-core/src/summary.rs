//! Compact, serialisable description of a session, used by the CLI `info`
//! command and by golden tests.

use crate::session::{FlightSession, FrameStats};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelSummary {
    pub key: String,
    pub name: String,
    pub unit: String,
    pub valid: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub index: usize,
    pub offset_bytes: u64,
    pub firmware_vendor: String,
    pub firmware_version: Option<String>,
    pub firmware_revision: String,
    pub profile_id: String,
    pub samples: usize,
    pub duration_s: f64,
    pub sample_rate_hz: f64,
    pub median_dt_us: f64,
    pub nominal_dt_us: Option<f64>,
    pub gaps: usize,
    pub first_time_us: Option<i64>,
    pub last_time_us: Option<i64>,
    pub frames: BTreeMap<String, FrameStats>,
    pub corrupt_frames: u64,
    pub desync_frames: u64,
    pub skipped_bytes: u64,
    pub events: usize,
    pub aux_rows: BTreeMap<String, usize>,
    pub unmapped_fields: Vec<String>,
    pub channels: Vec<ChannelSummary>,
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

impl SessionSummary {
    pub fn of(s: &FlightSession) -> SessionSummary {
        let mut channels: Vec<ChannelSummary> = s
            .channels
            .iter()
            .map(|c| {
                let (mut min, mut max, mut sum, mut n) =
                    (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0usize);
                for v in c.data.iter().flatten() {
                    min = min.min(v);
                    max = max.max(v);
                    sum += v;
                    n += 1;
                }
                let (min, max, mean) = if n == 0 {
                    (0.0, 0.0, 0.0)
                } else {
                    (min, max, sum / n as f64)
                };
                ChannelSummary {
                    key: c.id.key(),
                    name: c.name.clone(),
                    unit: c.unit.to_string(),
                    valid: n,
                    min: round6(min),
                    max: round6(max),
                    mean: round6(mean),
                }
            })
            .collect();
        channels.sort_by(|a, b| a.key.cmp(&b.key));
        let mut aux_rows = BTreeMap::new();
        for t in &s.aux {
            *aux_rows.entry(t.kind.clone()).or_insert(0) += t.rows.len();
        }
        SessionSummary {
            index: s.index,
            offset_bytes: s.offset_bytes,
            firmware_vendor: s.firmware.vendor.name().to_string(),
            firmware_version: s.firmware.version.map(|v| v.to_string()),
            firmware_revision: s.firmware.revision.clone(),
            profile_id: s.profile_id.clone(),
            samples: s.len(),
            duration_s: round6(s.timebase.duration_s()),
            sample_rate_hz: round6(s.timebase.sample_rate_hz()),
            median_dt_us: s.timebase.median_dt_us,
            nominal_dt_us: s.timebase.nominal_dt_us.map(round6),
            gaps: s.timebase.gaps.len(),
            first_time_us: s.timebase.time_us.first().copied(),
            last_time_us: s.timebase.time_us.last().copied(),
            frames: s.diagnostics.frames.clone(),
            corrupt_frames: s.diagnostics.corrupt_frames,
            desync_frames: s.diagnostics.desync_frames,
            skipped_bytes: s.diagnostics.skipped_bytes,
            events: s.events.len(),
            aux_rows,
            unmapped_fields: s.diagnostics.unmapped_fields.clone(),
            channels,
        }
    }
}
