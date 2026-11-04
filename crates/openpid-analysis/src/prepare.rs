//! Common preparation: cut the epoch and resample a channel onto a uniform grid.

use openpid_core::analysis::{AnalysisCtx, AnalysisError, Epoch};
use openpid_core::{Channel, ChannelId, FlightSession, Unit};
use openpid_dsp::resample::{Uniform, to_uniform};

/// A channel resampled onto the session's nominal uniform grid inside the epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub id: ChannelId,
    pub unit: Unit,
    pub fs: f64,
    pub data: Uniform,
    /// Seconds from the start of the session to the first prepared sample.
    pub t0_s: f64,
}

impl Prepared {
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Valid samples only, with invalid ones replaced by linear interpolation of
    /// their neighbours (used where a gap-free array is required).
    pub fn filled(&self) -> Vec<f64> {
        let v = &self.data.values;
        let ok = &self.data.valid;
        let mut out = v.clone();
        let mut i = 0;
        while i < out.len() {
            if ok[i] {
                i += 1;
                continue;
            }
            let start = i;
            while i < out.len() && !ok[i] {
                i += 1;
            }
            let left = if start > 0 { Some(v[start - 1]) } else { None };
            let right = if i < out.len() { Some(v[i]) } else { None };
            let n = (i - start) as f64 + 1.0;
            for (k, slot) in out[start..i].iter_mut().enumerate() {
                *slot = match (left, right) {
                    (Some(l), Some(r)) => l + (r - l) * (k as f64 + 1.0) / n,
                    (Some(l), None) => l,
                    (None, Some(r)) => r,
                    (None, None) => 0.0,
                };
            }
        }
        out
    }
}

/// Sample index range of the epoch within the session (half-open).
pub fn epoch_range(session: &FlightSession, epoch: Option<Epoch>) -> (usize, usize) {
    let t = &session.timebase.time_us;
    let Some(e) = epoch else { return (0, t.len()) };
    let Some(&t0) = t.first() else { return (0, 0) };
    let lo = t0 + (e.start_s.max(0.0) * 1.0e6) as i64;
    let hi = t0 + (e.end_s.max(0.0) * 1.0e6) as i64;
    let a = t.partition_point(|&x| x < lo);
    let b = t.partition_point(|&x| x <= hi);
    (a, b.max(a))
}

/// Nominal uniform step in microseconds: the configured log rate when known,
/// otherwise the observed median step.
pub fn nominal_dt_us(session: &FlightSession) -> f64 {
    session
        .timebase
        .nominal_dt_us
        .filter(|d| *d > 0.0)
        .unwrap_or(session.timebase.median_dt_us)
        .max(1.0)
}

fn channel<'a>(ctx: &AnalysisCtx<'a>, id: &ChannelId) -> Result<&'a Channel, AnalysisError> {
    ctx.session
        .channel(id)
        .ok_or_else(|| AnalysisError::MissingChannel(id.clone()))
}

/// Resample one channel within the epoch. `expect_unit` refuses a channel with
/// a different unit (no silent conversions).
pub fn prepare(
    ctx: &AnalysisCtx<'_>,
    id: &ChannelId,
    expect_unit: Option<&Unit>,
) -> Result<Prepared, AnalysisError> {
    let c = channel(ctx, id)?;
    if let Some(u) = expect_unit
        && &c.unit != u
    {
        return Err(AnalysisError::WrongUnit(
            id.clone(),
            c.unit.to_string(),
            u.to_string(),
        ));
    }
    let (a, b) = epoch_range(ctx.session, ctx.epoch);
    if b - a < 2 {
        return Err(AnalysisError::NotEnoughData(format!(
            "epoch holds {} samples",
            b - a
        )));
    }
    let dt = nominal_dt_us(ctx.session);
    let t = &ctx.session.timebase.time_us[a..b];
    let x = &c.data.values()[a..b];
    let valid: Vec<bool> = (a..b).map(|i| c.data.is_valid(i)).collect();
    let data = to_uniform(t, x, Some(&valid), dt, 1.5 * dt);
    let t0_s = (t[0] - ctx.session.timebase.time_us[0]) as f64 / 1.0e6;
    Ok(Prepared {
        id: id.clone(),
        unit: c.unit.clone(),
        fs: 1.0e6 / dt,
        data,
        t0_s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpid_dsp::resample::Uniform;

    #[test]
    fn fills_gaps() {
        let p = Prepared {
            id: ChannelId::Throttle,
            unit: Unit::Percent,
            fs: 1000.0,
            t0_s: 0.0,
            data: Uniform {
                start_us: 0,
                dt_us: 1000.0,
                values: vec![1.0, 0.0, 0.0, 4.0, 0.0],
                valid: vec![true, false, false, true, false],
            },
        };
        assert_eq!(p.filled(), vec![1.0, 2.0, 3.0, 4.0, 4.0]);
    }
}
