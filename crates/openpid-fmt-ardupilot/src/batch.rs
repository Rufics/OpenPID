//! IMU batch sampler (`ISBH`/`ISBD`): bursts of gyro or accelerometer samples
//! at the sensor rate, before the firmware's software filters (and, with
//! `INS_LOG_BAT_OPT` bit 2, alternating with a post-filter copy).
//!
//! `ISBH` opens a batch: sequence number `N`, sensor `type` (0 = accel,
//! 1 = gyro), `instance`, the fixed-point multiplier `mul`, the sample count,
//! the time of the first sample and the sample rate. The following `ISBD`
//! packets carry 32 samples each (`seqno` = position in the batch) as `int16`
//! `x`, `y`, `z` arrays scaled by `mul`. A post-filter batch is logged with
//! `instance` offset by the number of IMUs, which is how the firmware's own
//! tools tell the two apart.

use crate::reader::{RawLog, Value};

/// Samples per `ISBD` packet.
const SAMPLES_PER_PACKET: usize = 32;

/// Which sensor a batch sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BatchKind {
    /// Accelerometer, m/s².
    Accel,
    /// Gyroscope, rad/s.
    Gyro,
}

/// One burst of samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    /// `ISBH.N`.
    pub n: i64,
    /// Sensor sampled.
    pub kind: BatchKind,
    /// `ISBH.instance` as logged (post-filter batches carry `instance + IMU count`).
    pub instance: u8,
    /// Time of the first sample, µs.
    pub t0_us: i64,
    /// Sample rate, Hz.
    pub rate_hz: f64,
    /// Samples in sensor units; `None` where the `ISBD` packet was missing.
    pub samples: Vec<Option<[f64; 3]>>,
}

impl Batch {
    /// Time of sample `i`, µs.
    pub fn time_us(&self, i: usize) -> f64 {
        self.t0_us as f64 + i as f64 * 1.0e6 / self.rate_hz
    }

    /// Time just after the last sample, µs.
    pub fn end_us(&self) -> f64 {
        self.time_us(self.samples.len())
    }

    /// Samples whose `ISBD` packet never arrived.
    pub fn missing(&self) -> usize {
        self.samples.iter().filter(|s| s.is_none()).count()
    }
}

/// Every batch of the log in file order, and the number of `ISBD` packets
/// that belonged to no open batch.
pub fn batches(raw: &RawLog) -> (Vec<Batch>, usize) {
    let (Some((hidx, hdef)), Some((didx, ddef))) = (raw.def("ISBH"), raw.def("ISBD")) else {
        return (Vec::new(), 0);
    };
    let hcol = |name: &str| hdef.column(name);
    let dcol = |name: &str| ddef.column(name);
    let (Some(h_n), Some(h_type), Some(h_inst), Some(h_mul), Some(h_cnt), Some(h_t0), Some(h_rate)) = (
        hcol("N"),
        hcol("type"),
        hcol("instance"),
        hcol("mul"),
        hcol("smp_cnt"),
        hcol("SampleUS"),
        hcol("smp_rate"),
    ) else {
        return (Vec::new(), 0);
    };
    let (Some(d_n), Some(d_seq), Some(d_x), Some(d_y), Some(d_z)) =
        (dcol("N"), dcol("seqno"), dcol("x"), dcol("y"), dcol("z"))
    else {
        return (Vec::new(), 0);
    };
    let mut out: Vec<Batch> = Vec::new();
    let mut orphans = 0usize;
    // (batch index, 1 / mul) of the batch currently being filled
    let mut open: Option<(usize, f64)> = None;
    for m in &raw.messages {
        if m.def == hidx {
            let num = |c: usize| m.values.get(c).and_then(Value::as_f64);
            let (Some(n), Some(kind), Some(inst), Some(mul), Some(cnt), Some(t0), Some(rate)) = (
                num(h_n),
                num(h_type),
                num(h_inst),
                num(h_mul),
                num(h_cnt),
                num(h_t0),
                num(h_rate),
            ) else {
                open = None;
                continue;
            };
            if mul <= 0.0 || rate <= 0.0 || cnt <= 0.0 {
                open = None;
                continue;
            }
            out.push(Batch {
                n: n as i64,
                kind: if kind as i64 == 1 {
                    BatchKind::Gyro
                } else {
                    BatchKind::Accel
                },
                instance: inst as u8,
                t0_us: t0 as i64,
                rate_hz: rate,
                samples: vec![None; cnt as usize],
            });
            open = Some((out.len() - 1, 1.0 / mul));
        } else if m.def == didx {
            let n = m.values.get(d_n).and_then(Value::as_f64).map(|v| v as i64);
            let Some((bi, scale)) = open.filter(|(bi, _)| Some(out[*bi].n) == n) else {
                orphans += 1;
                continue;
            };
            let Some(seq) = m.values.get(d_seq).and_then(Value::as_f64) else {
                continue;
            };
            let arr = |c: usize| match m.values.get(c) {
                Some(Value::Array(a)) => Some(a),
                _ => None,
            };
            let (Some(x), Some(y), Some(z)) = (arr(d_x), arr(d_y), arr(d_z)) else {
                continue;
            };
            let batch = &mut out[bi];
            let start = seq as usize * SAMPLES_PER_PACKET;
            for (k, slot) in batch
                .samples
                .iter_mut()
                .skip(start)
                .take(SAMPLES_PER_PACKET)
                .enumerate()
            {
                if let (Some(&vx), Some(&vy), Some(&vz)) = (x.get(k), y.get(k), z.get(k)) {
                    *slot = Some([
                        f64::from(vx) * scale,
                        f64::from(vy) * scale,
                        f64::from(vz) * scale,
                    ]);
                }
            }
        }
    }
    (out, orphans)
}

/// One axis of `batches` (time-sorted, same sensor and instance) on the
/// timebase `times`: every timebase sample takes the mean of the batch
/// samples within ± `half_us` of it (a box anti-alias filter one timebase
/// period wide), or the nearest batch sample when the timebase is faster
/// than the batch, and is invalid where no batch covers it.
pub fn resample_onto(
    times: &[i64],
    batches: &[&Batch],
    axis: usize,
    half_us: f64,
) -> Vec<Option<f64>> {
    let mut cursor = 0usize;
    times
        .iter()
        .map(|&t| {
            let t = t as f64;
            while cursor < batches.len() && batches[cursor].end_us() < t - half_us {
                cursor += 1;
            }
            let b = batches.get(cursor)?;
            if b.t0_us as f64 > t + half_us {
                return None;
            }
            let per_us = b.rate_hz / 1.0e6;
            let last = b.samples.len().checked_sub(1)?;
            let lo = ((t - half_us - b.t0_us as f64) * per_us).ceil().max(0.0) as usize;
            let hi =
                (((t + half_us - b.t0_us as f64) * per_us).floor().max(0.0) as usize).min(last);
            let range = if lo <= hi {
                lo..=hi
            } else {
                let nearest = (((t - b.t0_us as f64) * per_us).round().max(0.0) as usize).min(last);
                nearest..=nearest
            };
            let (sum, count) = b.samples[range]
                .iter()
                .flatten()
                .fold((0.0, 0usize), |(s, c), v| (s + v[axis], c + 1));
            (count > 0).then(|| sum / count as f64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(t0_us: i64, rate_hz: f64, values: &[Option<f64>]) -> Batch {
        Batch {
            n: 0,
            kind: BatchKind::Gyro,
            instance: 0,
            t0_us,
            rate_hz,
            samples: values.iter().map(|v| v.map(|x| [x, 0.0, 0.0])).collect(),
        }
    }

    #[test]
    fn resampling_averages_covers_gaps_and_marks_the_rest_invalid() {
        // 1 kHz batch of 10 samples at 1000 µs, timebase at 250 Hz (4000 µs)
        let b = batch(
            1000,
            1000.0,
            &[
                Some(1.0),
                Some(2.0),
                Some(3.0),
                Some(4.0),
                Some(5.0),
                Some(6.0),
                Some(7.0),
                None,
                Some(9.0),
                Some(10.0),
            ],
        );
        let times = [0i64, 4000, 8000, 12000, 16000];
        let out = resample_onto(&times, &[&b], 0, 2000.0);
        // t = 0: window [-2000, 2000] holds sample 0 (1000 µs) and 1 (2000 µs)
        assert_eq!(out[0], Some(1.5));
        // t = 4000: samples at 2000..=6000 µs → 2, 3, 4, 5, 6
        assert_eq!(out[1], Some(4.0));
        // t = 8000: samples at 6000..=10000 → 6, 7, (missing), 9, 10
        assert_eq!(out[2], Some(8.0));
        // t = 12000: only the last sample (10000 µs) is inside [10000, 14000]
        assert_eq!(out[3], Some(10.0));
        // t = 16000: the batch ended at 11000
        assert_eq!(out[4], None);
        assert_eq!(b.missing(), 1);
        assert!((b.end_us() - 11000.0).abs() < 1e-9);
    }

    #[test]
    fn a_faster_timebase_takes_the_nearest_sample() {
        let b = batch(0, 100.0, &[Some(1.0), Some(2.0), Some(3.0)]);
        let times: Vec<i64> = (0..6).map(|i| i * 4000).collect();
        let out = resample_onto(&times, &[&b], 0, 2000.0);
        assert_eq!(
            out,
            vec![
                Some(1.0),
                Some(1.0),
                Some(2.0),
                Some(2.0),
                Some(3.0),
                Some(3.0)
            ]
        );
        assert_eq!(resample_onto(&times, &[], 0, 2000.0), vec![None; 6]);
    }
}
