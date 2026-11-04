//! Field predictors: the value a field is expected to have, which the
//! firmware subtracts before encoding.

pub const ZERO: u8 = 0;
pub const PREVIOUS: u8 = 1;
pub const STRAIGHT_LINE: u8 = 2;
pub const AVERAGE_2: u8 = 3;
pub const MINTHROTTLE: u8 = 4;
pub const MOTOR_0: u8 = 5;
pub const INC: u8 = 6;
pub const HOME_COORD: u8 = 7;
pub const P1500: u8 = 8;
pub const VBATREF: u8 = 9;
pub const LAST_MAIN_FRAME_TIME: u8 = 10;
pub const MINMOTOR: u8 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PredictError {
    #[error("unsupported predictor {0}")]
    Unsupported(u8),
    #[error("predictor {0} needs a field that the log does not define")]
    MissingReference(u8),
}

/// Everything a predictor may reference besides the frame being decoded.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub minthrottle: i64,
    pub motor_output_low: i64,
    pub vbatref: i64,
    pub previous: Option<&'a [i64]>,
    pub previous2: Option<&'a [i64]>,
    pub motor0_index: Option<usize>,
    /// Last published GPS home frame and the indices of its lat/lon fields.
    pub gps_home: Option<&'a [i64]>,
    pub gps_home_indices: [Option<usize>; 2],
    pub last_main_time: Option<i64>,
}

/// Apply `predictor` to the decoded residual `value` of field `field`.
///
/// `current` is the frame being decoded (fields before `field` are final).
/// `home_pair` selects latitude (0) or longitude (1) for `HOME_COORD`.
pub fn apply(
    predictor: u8,
    field: usize,
    home_pair: u8,
    value: i64,
    ctx: &Context<'_>,
    current: &[i64],
) -> Result<i64, PredictError> {
    Ok(match predictor {
        ZERO => value,
        PREVIOUS => match ctx.previous {
            Some(p) => value + p[field],
            None => value,
        },
        STRAIGHT_LINE => match (ctx.previous, ctx.previous2) {
            (Some(p), Some(p2)) => value + 2 * p[field] - p2[field],
            _ => value,
        },
        AVERAGE_2 => match (ctx.previous, ctx.previous2) {
            // `midpoint` rounds toward zero, exactly like the firmware's `(a + b) / 2`.
            (Some(p), Some(p2)) => value + p[field].midpoint(p2[field]),
            _ => value,
        },
        MINTHROTTLE => value + ctx.minthrottle,
        MOTOR_0 => {
            let i = ctx
                .motor0_index
                .ok_or(PredictError::MissingReference(MOTOR_0))?;
            value + current[i]
        }
        INC => value,
        HOME_COORD => {
            let idx = ctx.gps_home_indices[usize::from(home_pair.min(1))]
                .ok_or(PredictError::MissingReference(HOME_COORD))?;
            value + ctx.gps_home.map_or(0, |h| h[idx])
        }
        P1500 => value + 1500,
        VBATREF => value + ctx.vbatref,
        LAST_MAIN_FRAME_TIME => value + ctx.last_main_time.unwrap_or(0),
        MINMOTOR => value + ctx.motor_output_low,
        other => return Err(PredictError::Unsupported(other)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predictors() {
        let prev = [10i64, 100, 7];
        let prev2 = [8i64, 90, 1];
        let ctx = Context {
            minthrottle: 1070,
            motor_output_low: 158,
            vbatref: 2466,
            previous: Some(&prev),
            previous2: Some(&prev2),
            motor0_index: Some(2),
            gps_home: Some(&[55, 66]),
            gps_home_indices: [Some(0), Some(1)],
            last_main_time: Some(1000),
        };
        let cur = [0i64, 0, 500];
        assert_eq!(apply(ZERO, 0, 0, 5, &ctx, &cur), Ok(5));
        assert_eq!(apply(PREVIOUS, 1, 0, 5, &ctx, &cur), Ok(105));
        assert_eq!(apply(STRAIGHT_LINE, 1, 0, 5, &ctx, &cur), Ok(115));
        assert_eq!(apply(AVERAGE_2, 2, 0, 1, &ctx, &cur), Ok(5)); // (7+1)/2 = 4, +1
        assert_eq!(apply(AVERAGE_2, 2, 0, 0, &ctx, &cur), Ok(4));
        assert_eq!(apply(MINTHROTTLE, 0, 0, 2, &ctx, &cur), Ok(1072));
        assert_eq!(apply(MOTOR_0, 0, 0, 2, &ctx, &cur), Ok(502));
        assert_eq!(apply(HOME_COORD, 0, 0, 1, &ctx, &cur), Ok(56));
        assert_eq!(apply(HOME_COORD, 0, 1, 1, &ctx, &cur), Ok(67));
        assert_eq!(apply(P1500, 0, 0, -3, &ctx, &cur), Ok(1497));
        assert_eq!(apply(VBATREF, 0, 0, -10, &ctx, &cur), Ok(2456));
        assert_eq!(apply(LAST_MAIN_FRAME_TIME, 0, 0, 20, &ctx, &cur), Ok(1020));
        assert_eq!(apply(MINMOTOR, 0, 0, 1, &ctx, &cur), Ok(159));
        assert_eq!(
            apply(99, 0, 0, 1, &ctx, &cur),
            Err(PredictError::Unsupported(99))
        );
        // integer division truncates toward zero, as in the C reference
        let prev = [-7i64];
        let prev2 = [0i64];
        let ctx2 = Context {
            previous: Some(&prev),
            previous2: Some(&prev2),
            ..ctx
        };
        assert_eq!(apply(AVERAGE_2, 0, 0, 0, &ctx2, &[0]), Ok(-3));
    }
}
