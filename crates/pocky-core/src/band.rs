use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fixed::Fx;

/// A slippage band in basis points: the largest acceptable shortfall of the *average*
/// execution price below the reference price. 1_000 = 10%.
///
/// We use average execution price, not the marginal (last-fill) price, because a
/// liquidator's loss is what it receives on average versus what the collateral was
/// valued at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Band(u32);

impl Band {
    pub const TWO_PCT: Band = Band(200);
    pub const TEN_PCT: Band = Band(1_000);
    pub const TWENTY_FIVE_PCT: Band = Band(2_500);
    /// The bands every measurement reports.
    pub const STANDARD: [Band; 3] = [Band::TWO_PCT, Band::TEN_PCT, Band::TWENTY_FIVE_PCT];

    pub fn new(bps: u32) -> Result<Band, CoreError> {
        if bps == 0 || bps >= 10_000 {
            return Err(CoreError::BadBand(bps));
        }
        Ok(Band(bps))
    }

    pub fn bps(self) -> u32 {
        self.0
    }

    /// The lowest acceptable average price for a given reference: `reference * (1 - band)`,
    /// rounded up so the floor is never looser than the band.
    pub fn floor(self, reference: Fx) -> Result<Fx, CoreError> {
        let keep = reference.mul_bps_down(10_000 - self.0)?;
        // mul_bps_down rounds the kept fraction down, which lowers the floor; nudge back up
        // by one unit when the product was inexact so the band is never widened.
        let exact = reference
            .raw()
            .checked_mul(i128::from(10_000 - self.0))
            .ok_or(CoreError::Overflow)?;
        if exact % 10_000 != 0 {
            return Fx::from_raw(keep.raw() + 1);
        }
        Ok(keep)
    }
}

impl TryFrom<u32> for Band {
    type Error = CoreError;
    fn try_from(v: u32) -> Result<Self, Self::Error> {
        Band::new(v)
    }
}

impl From<Band> for u32 {
    fn from(b: Band) -> u32 {
        b.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_range() {
        assert!(Band::new(0).is_err());
        assert!(Band::new(10_000).is_err());
        assert!(Band::new(1).is_ok());
        assert!(Band::new(9_999).is_ok());
    }

    #[test]
    fn floor_is_never_looser() {
        let r: Fx = "1.0000001".parse().unwrap();
        let f = Band::TEN_PCT.floor(r).unwrap();
        // exact value is 0.90000009, so the floor must be >= that
        assert_eq!(f, "0.9000001".parse().unwrap());
        assert_eq!(
            Band::TEN_PCT.floor(Fx::ONE).unwrap(),
            "0.9".parse().unwrap()
        );
    }
}
