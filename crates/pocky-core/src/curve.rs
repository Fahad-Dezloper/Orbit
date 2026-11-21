//! The liquidation curve, and the collateral cap it implies.
//!
//! A lender pays liquidators a bonus `b`: the liquidator receives collateral worth the debt
//! plus `b`. Liquidating is worth doing only while the liquidator can sell that collateral
//! at an average price at most `b` below the reference. So **the exit at band `b` is the most
//! collateral that can be liquidated on-chain at that bonus, right now.** Anything above it
//! cannot be cleared without the lender absorbing the loss, which is exactly what happened
//! to the YieldBlox pool in February 2026.
//!
//! The curve samples that exit at fixed bands from 0.5% to 50%, across every venue, from
//! one reference price. The cap for a bonus is read from the curve conservatively (the
//! largest sampled band not above the bonus) and then reduced by a safety margin.

use serde::{Deserialize, Serialize};

use crate::band::Band;
use crate::error::CoreError;
use crate::fixed::Fx;

/// Bands the curve is sampled at, in basis points.
pub const CURVE_BPS: [u32; 12] = [
    50, 100, 200, 300, 500, 750, 1000, 1500, 2000, 2500, 3500, 5000,
];

/// Exit across all venues at one band.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub band: Band,
    pub exit_usd: Fx,
}

/// The bands of [`CURVE_BPS`] as [`Band`]s.
pub fn curve_bands() -> Result<Vec<Band>, CoreError> {
    CURVE_BPS.iter().map(|&b| Band::new(b)).collect()
}

/// Exit liquidatable at a given liquidation bonus: the curve value at the largest sampled
/// band that does not exceed the bonus. Never interpolates upward. `None` if the bonus is
/// below the smallest sampled band.
pub fn liquidatable_at(curve: &[CurvePoint], bonus_bps: u32) -> Option<Fx> {
    curve
        .iter()
        .filter(|p| p.band.bps() <= bonus_bps)
        .max_by_key(|p| p.band)
        .map(|p| p.exit_usd)
}

/// A recommended collateral cap: `liquidatable * (1 - safety)`, rounded down.
pub fn safe_cap(liquidatable: Fx, safety_bps: u32) -> Result<Fx, CoreError> {
    if safety_bps >= 10_000 {
        return Err(CoreError::BadBand(safety_bps));
    }
    liquidatable.mul_bps_down(10_000 - safety_bps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    fn curve() -> Vec<CurvePoint> {
        [(200, "100000"), (500, "250000"), (1000, "370000")]
            .iter()
            .map(|&(b, v)| CurvePoint {
                band: Band::new(b).unwrap(),
                exit_usd: fx(v),
            })
            .collect()
    }

    #[test]
    fn bonus_reads_the_curve_conservatively() {
        let c = curve();
        // Exactly on a sample.
        assert_eq!(liquidatable_at(&c, 500), Some(fx("250000")));
        // Between samples: never rounds up to the next band.
        assert_eq!(liquidatable_at(&c, 750), Some(fx("250000")));
        // Below the first sample: nothing can be claimed.
        assert_eq!(liquidatable_at(&c, 100), None);
    }

    #[test]
    fn cap_applies_the_safety_margin() {
        assert_eq!(safe_cap(fx("250000"), 5_000).unwrap(), fx("125000"));
        assert_eq!(safe_cap(fx("250000"), 0).unwrap(), fx("250000"));
        assert!(safe_cap(fx("1"), 10_000).is_err());
    }

    #[test]
    fn sampled_bands_are_valid_and_increasing() {
        let b = curve_bands().unwrap();
        assert!(b.windows(2).all(|w| w[0] < w[1]));
        assert!(b.contains(&Band::TEN_PCT));
    }
}
