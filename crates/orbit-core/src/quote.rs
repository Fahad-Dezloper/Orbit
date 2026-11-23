//! Exit for pools whose maths we do not re-implement: concentrated-liquidity and
//! stable-swap pools, measured from the pool's **own** on-chain swap quote.
//!
//! The ingest layer asks the pool what it would pay for a series of increasing sale sizes
//! (`estimate_swap`, a simulation). Those quotes include the pool's fee and its tick or
//! curve maths exactly, because they are computed by the pool contract itself.
//!
//! Exit at a band is the largest **sampled** size whose average price stays at or above
//! `reference * (1 - band)`. Between samples nothing is interpolated, so the figure can only
//! be understated.

use serde::{Deserialize, Serialize};

use crate::band::Band;
use crate::book::ExitQuote;
use crate::error::CoreError;
use crate::fixed::Fx;

/// Pool quote for selling `base_in` of the asset: `counter_out` of the counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteSample {
    pub base_in: Fx,
    pub counter_out: Fx,
}

/// Fractions of the pool's counter-side reserve to sample, in basis points. Dense at the
/// small end where tight bands are decided.
pub const QUOTE_FRACTIONS_BPS: [u32; 12] = [
    5, 25, 50, 100, 200, 400, 700, 1_000, 2_000, 3_500, 5_500, 8_000,
];

/// A pool described only by its own quotes, smallest sale first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotedPool {
    pub samples: Vec<QuoteSample>,
}

impl QuotedPool {
    /// Builds and checks the samples: sizes strictly increasing, output never decreasing,
    /// and average price never increasing with size. A pool whose quotes violate this is
    /// not trusted at all.
    pub fn new(samples: Vec<QuoteSample>) -> Result<QuotedPool, CoreError> {
        for w in samples.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b.base_in <= a.base_in || b.counter_out < a.counter_out {
                return Err(CoreError::UnsortedBook);
            }
            // a.out / a.in >= b.out / b.in  <=>  a.out * b.in >= b.out * a.in
            let lhs = a.counter_out.raw().checked_mul(b.base_in.raw());
            let rhs = b.counter_out.raw().checked_mul(a.base_in.raw());
            match (lhs, rhs) {
                (Some(l), Some(r)) if l >= r => {}
                (Some(_), Some(_)) => return Err(CoreError::UnsortedBook),
                _ => return Err(CoreError::Overflow),
            }
        }
        Ok(QuotedPool { samples })
    }

    /// The pool's own price: the average price of its smallest sampled sale (fee included,
    /// so slightly below the true marginal price).
    pub fn mid(&self) -> Result<Fx, CoreError> {
        let s = self
            .samples
            .iter()
            .find(|s| !s.base_in.is_zero() && !s.counter_out.is_zero())
            .ok_or(CoreError::NoMid)?;
        s.counter_out.div_down(s.base_in)
    }

    /// Largest sampled sale whose average price is at or above `reference * (1 - band)`.
    pub fn exit_against(&self, reference: Fx, band: Band) -> Result<ExitQuote, CoreError> {
        let floor = band.floor(reference)?;
        let mut best = ExitQuote::default();
        for s in &self.samples {
            if s.base_in.is_zero() {
                continue;
            }
            // out / in >= floor  <=>  out >= floor * in (floor * in rounded up)
            let needed = floor.mul_down(s.base_in)?.checked_add(Fx::from_raw(1)?)?;
            if s.counter_out >= needed {
                best = ExitQuote {
                    base_sold: s.base_in,
                    counter_raised: s.counter_out,
                };
            } else {
                break; // average price only falls from here
            }
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amm::CpPool;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    /// Samples a constant-product pool's exact swap output, as a quoting pool would.
    fn sampled(pool: &CpPool, sizes: &[&str]) -> QuotedPool {
        let keep = i128::from(10_000 - pool.fee_bps);
        let samples = sizes
            .iter()
            .map(|s| {
                let dx = fx(s);
                let dx_eff = Fx::from_raw(dx.raw() * keep / 10_000).unwrap();
                let new_x = pool.reserve_asset.checked_add(dx_eff).unwrap();
                QuoteSample {
                    base_in: dx,
                    counter_out: pool.reserve_counter.mul_div_down(dx_eff, new_x).unwrap(),
                }
            })
            .collect();
        QuotedPool::new(samples).unwrap()
    }

    #[test]
    fn never_exceeds_the_exact_constant_product_answer() {
        let pool = CpPool {
            reserve_asset: fx("1000000"),
            reserve_counter: fx("1000000"),
            fee_bps: 30,
        };
        let q = sampled(
            &pool,
            &["1000", "10000", "50000", "100000", "110000", "200000"],
        );
        let exact = pool.exit_against(fx("1"), Band::TEN_PCT).unwrap();
        let got = q.exit_against(fx("1"), Band::TEN_PCT).unwrap();
        assert!(got.counter_raised <= exact.counter_raised);
        // The 100k sample is inside the band; 110k is not (exact limit is ~107.8k).
        assert_eq!(got.base_sold, fx("100000"));
    }

    #[test]
    fn mid_is_the_smallest_sale_price() {
        let q = QuotedPool::new(vec![
            QuoteSample {
                base_in: fx("10"),
                counter_out: fx("11.44"),
            },
            QuoteSample {
                base_in: fx("100"),
                counter_out: fx("114"),
            },
        ])
        .unwrap();
        assert_eq!(q.mid().unwrap(), fx("1.144"));
    }

    #[test]
    fn rejects_quotes_that_improve_with_size() {
        // A larger sale getting a better average price is impossible for an honest pool.
        let bad = QuotedPool::new(vec![
            QuoteSample {
                base_in: fx("10"),
                counter_out: fx("10"),
            },
            QuoteSample {
                base_in: fx("20"),
                counter_out: fx("25"),
            },
        ]);
        assert!(bad.is_err());
        let shrinking = QuotedPool::new(vec![
            QuoteSample {
                base_in: fx("10"),
                counter_out: fx("10"),
            },
            QuoteSample {
                base_in: fx("20"),
                counter_out: fx("9"),
            },
        ]);
        assert!(shrinking.is_err());
    }

    #[test]
    fn a_pool_far_below_the_reference_gives_nothing() {
        let q = QuotedPool::new(vec![QuoteSample {
            base_in: fx("10"),
            counter_out: fx("5"),
        }])
        .unwrap();
        assert_eq!(
            q.exit_against(fx("1"), Band::TEN_PCT).unwrap(),
            ExitQuote::default()
        );
    }
}
