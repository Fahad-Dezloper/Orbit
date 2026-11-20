//! Exit liquidity in a constant-product (x * y = k) AMM pool, the design used by
//! Aquarius standard pools, Soroswap and Phoenix on Stellar.

use serde::{Deserialize, Serialize};

use crate::band::Band;
use crate::book::ExitQuote;
use crate::error::CoreError;
use crate::fixed::Fx;

/// Reserves of a constant-product pool, from the side of a seller of `asset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpPool {
    /// Reserve of the asset being sold (x).
    pub reserve_asset: Fx,
    /// Reserve of the asset received (y), the COUNTER.
    pub reserve_counter: Fx,
    /// Swap fee in basis points, charged on the input.
    pub fee_bps: u32,
}

impl CpPool {
    /// The pool's own price, COUNTER per asset: `y / x`.
    pub fn mid(&self) -> Result<Fx, CoreError> {
        self.reserve_counter.div_down(self.reserve_asset)
    }

    /// Largest sale whose average execution price stays at or above `mid * (1 - band)`.
    ///
    /// With fee `f`, an input `dx` adds `dx' = dx (1 - f)` to the pool and returns
    /// `y dx' / (x + dx')`. The average price relative to mid is `(1 - f) x / (x + dx')`.
    /// Setting that equal to `1 - s` gives `dx' = x ((1 - f) / (1 - s) - 1)`.
    /// If the fee alone exceeds the band, no sale qualifies.
    pub fn exit(&self, band: Band) -> Result<ExitQuote, CoreError> {
        if self.reserve_asset.is_zero() || self.reserve_counter.is_zero() {
            return Ok(ExitQuote::default());
        }
        if self.fee_bps >= 10_000 {
            return Err(CoreError::BadBand(self.fee_bps));
        }
        if self.fee_bps >= band.bps() {
            return Ok(ExitQuote::default());
        }
        let keep_after_fee = Fx::from_int(i128::from(10_000 - self.fee_bps))?;
        let keep_after_band = Fx::from_int(i128::from(10_000 - band.bps()))?;
        // ratio - 1, rounded down so the admissible input is never overstated
        let ratio = keep_after_fee.div_down(keep_after_band)?;
        let excess = ratio.checked_sub(Fx::ONE)?;
        let dx_eff = self.reserve_asset.mul_down(excess)?;
        let new_x = self.reserve_asset.checked_add(dx_eff)?;
        let out = self.reserve_counter.mul_div_down(dx_eff, new_x)?;
        // gross input actually sold, rounded up (conservative)
        let keep = Fx::from_raw(i128::from(10_000 - self.fee_bps) * 1_000)?; // (1 - f) as Fx
        let dx = dx_eff.div_up(keep)?;
        Ok(ExitQuote {
            base_sold: dx,
            counter_raised: out,
        })
    }

    /// Like [`exit`](Self::exit), but the band is measured from an external `reference`
    /// price (COUNTER per asset) instead of the pool's own mid.
    ///
    /// This is what stops a pool from vouching for itself: if its price has been pushed up,
    /// a reference taken from other venues makes its admissible exit shrink, and if the
    /// pool is already below `reference * (1 - band)` it contributes nothing.
    ///
    /// The average price of a sale is `(1 - f) y / (x + dx')`. Requiring it to stay at or
    /// above the floor gives `x + dx' <= (1 - f) y / floor`.
    pub fn exit_against(&self, reference: Fx, band: Band) -> Result<ExitQuote, CoreError> {
        if self.reserve_asset.is_zero() || self.reserve_counter.is_zero() || reference.is_zero() {
            return Ok(ExitQuote::default());
        }
        if self.fee_bps >= 10_000 {
            return Err(CoreError::BadBand(self.fee_bps));
        }
        let floor = band.floor(reference)?; // rounded up: conservative
        let keep = Fx::from_raw(i128::from(10_000 - self.fee_bps) * 1_000)?; // (1 - f)
        let max_x = self.reserve_counter.mul_down(keep)?.div_down(floor)?;
        if max_x <= self.reserve_asset {
            return Ok(ExitQuote::default());
        }
        let dx_eff = max_x.checked_sub(self.reserve_asset)?;
        let out = self.reserve_counter.mul_div_down(dx_eff, max_x)?;
        let dx = dx_eff.div_up(keep)?;
        Ok(ExitQuote {
            base_sold: dx,
            counter_raised: out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    #[test]
    fn no_fee_exit_is_band_times_counter_reserve() {
        // With no fee, the counter raised at band s is exactly s * y.
        let pool = CpPool {
            reserve_asset: fx("1000000"),
            reserve_counter: fx("1000000"),
            fee_bps: 0,
        };
        let q = pool.exit(Band::TEN_PCT).unwrap();
        let expected = fx("100000");
        let diff = expected.raw() - q.counter_raised.raw();
        // Fixed-point rounding may only ever understate depth, by at most $0.10 here.
        assert!((0..=1_000_000).contains(&diff), "got {}", q.counter_raised);
    }

    #[test]
    fn usdy_aquarius_pool_from_research() {
        // Aquarius CP pool CAFHLHGZ..., 19 Nov 2026: 889,850.1654914 USDY / 1,018,012.7008436 USDC,
        // 0.10% fee. Research measured about $100.9K of exit at 10%.
        let pool = CpPool {
            reserve_asset: fx("889850.1654914"),
            reserve_counter: fx("1018012.7008436"),
            fee_bps: 10,
        };
        let q = pool.exit(Band::TEN_PCT).unwrap();
        assert!(
            q.counter_raised > fx("100000") && q.counter_raised < fx("102000"),
            "{}",
            q.counter_raised
        );
        let avg = q.counter_raised.div_down(q.base_sold).unwrap();
        let floor = Band::TEN_PCT.floor(pool.mid().unwrap()).unwrap();
        assert!(avg >= floor.checked_sub(fx("0.0000010")).unwrap());
    }

    #[test]
    fn fee_above_band_gives_nothing() {
        let pool = CpPool {
            reserve_asset: fx("10"),
            reserve_counter: fx("10"),
            fee_bps: 300,
        };
        assert_eq!(pool.exit(Band::TWO_PCT).unwrap(), ExitQuote::default());
    }

    #[test]
    fn exit_against_own_mid_matches_closed_form() {
        let pool = CpPool {
            reserve_asset: fx("889850.1654914"),
            reserve_counter: fx("1018012.7008436"),
            fee_bps: 10,
        };
        let a = pool.exit(Band::TEN_PCT).unwrap().counter_raised;
        let b = pool
            .exit_against(pool.mid().unwrap(), Band::TEN_PCT)
            .unwrap()
            .counter_raised;
        // Both paths round toward less depth; they agree to within $1.
        assert!((a.raw() - b.raw()).abs() <= 10_000_000, "{a} vs {b}");
    }

    #[test]
    fn a_higher_reference_only_shrinks_exit() {
        let pool = CpPool {
            reserve_asset: fx("1000000"),
            reserve_counter: fx("1000000"),
            fee_bps: 30,
        };
        let at_mid = pool.exit_against(fx("1"), Band::TEN_PCT).unwrap();
        let above = pool.exit_against(fx("1.05"), Band::TEN_PCT).unwrap();
        assert!(above.counter_raised < at_mid.counter_raised);
        // Pool already more than 10% below the reference: nothing qualifies.
        let far = pool.exit_against(fx("1.2"), Band::TEN_PCT).unwrap();
        assert_eq!(far, ExitQuote::default());
        // The average price of what is counted never breaks the floor.
        let avg = above.counter_raised.div_down(above.base_sold).unwrap();
        let floor = Band::TEN_PCT.floor(fx("1.05")).unwrap();
        assert!(avg >= floor.checked_sub(fx("0.000001")).unwrap());
    }

    #[test]
    fn empty_pool_gives_nothing() {
        let pool = CpPool {
            reserve_asset: Fx::ZERO,
            reserve_counter: fx("10"),
            fee_bps: 30,
        };
        assert_eq!(pool.exit(Band::TEN_PCT).unwrap(), ExitQuote::default());
    }
}
