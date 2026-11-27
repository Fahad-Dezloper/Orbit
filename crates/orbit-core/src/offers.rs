//! Optional filters on SDEX bids, applied per offer before the exit walk.
//!
//! - **Offer age**: count an offer only once it has rested for `min_age_secs`. Defeats
//!   place-then-pull even when the attacker is lucky with sample timing. It also discards
//!   market makers that reprice often, because repricing an offer resets its age.
//! - **Seller concentration**: count at most `max_seller_share_bps` of the counted depth from
//!   any one account, best prices first. One account cannot set the figure on its own.
//!
//! Both are off by default (`OfferFilter::default()`), so the measurement equals the plain
//! order book until an operator turns them on.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::book::Bid;
use crate::error::CoreError;
use crate::fixed::Fx;

/// One resting buy offer for the base asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BidOffer {
    pub seller: String,
    /// COUNTER per BASE.
    pub price: Fx,
    /// In COUNTER units.
    pub amount_counter: Fx,
    /// Seconds since the offer was created or last changed (a lower bound).
    pub age_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferFilter {
    /// Minimum resting time in seconds; 0 counts every offer.
    pub min_age_secs: u64,
    /// Largest share of the counted depth one account may supply, in basis points; 10000
    /// counts every account in full.
    pub max_seller_share_bps: u32,
}

impl Default for OfferFilter {
    fn default() -> Self {
        OfferFilter {
            min_age_secs: 0,
            max_seller_share_bps: 10_000,
        }
    }
}

impl OfferFilter {
    pub fn is_active(&self) -> bool {
        self.min_age_secs > 0 || self.max_seller_share_bps < 10_000
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.max_seller_share_bps == 0 || self.max_seller_share_bps > 10_000 {
            return Err(CoreError::BadShare(self.max_seller_share_bps));
        }
        Ok(())
    }
}

/// What a filter removed, in COUNTER units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterStats {
    pub offers: usize,
    pub too_young: usize,
    pub dropped_young_counter: Fx,
    pub capped_counter: Fx,
}

/// Bids that pass the filter, best price first, ready for `OrderBook::exit`.
pub fn filter_bids(
    mut offers: Vec<BidOffer>,
    f: &OfferFilter,
) -> Result<(Vec<Bid>, FilterStats), CoreError> {
    f.validate()?;
    let mut stats = FilterStats {
        offers: offers.len(),
        ..FilterStats::default()
    };
    offers.retain(|o| !o.price.is_zero() && !o.amount_counter.is_zero());
    offers.sort_by_key(|o| std::cmp::Reverse(o.price));

    let mut aged = Vec::with_capacity(offers.len());
    for o in offers {
        if o.age_secs < f.min_age_secs {
            stats.too_young += 1;
            stats.dropped_young_counter =
                stats.dropped_young_counter.checked_add(o.amount_counter)?;
        } else {
            aged.push(o);
        }
    }

    let total = aged
        .iter()
        .try_fold(Fx::ZERO, |t, o| t.checked_add(o.amount_counter))?;
    let allowance = total.mul_bps_down(f.max_seller_share_bps)?;
    let mut used: HashMap<&str, Fx> = HashMap::new();
    let mut bids = Vec::with_capacity(aged.len());
    for o in &aged {
        let u = used.entry(o.seller.as_str()).or_insert(Fx::ZERO);
        let room = allowance.checked_sub(*u).unwrap_or(Fx::ZERO);
        let take = o.amount_counter.min(room);
        let cut = o.amount_counter.checked_sub(take)?;
        stats.capped_counter = stats.capped_counter.checked_add(cut)?;
        *u = u.checked_add(take)?;
        if !take.is_zero() {
            bids.push(Bid {
                price: o.price,
                amount_counter: take,
            });
        }
    }
    Ok((bids, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }
    fn offer(seller: &str, price: &str, amount: &str, age: u64) -> BidOffer {
        BidOffer {
            seller: seller.into(),
            price: fx(price),
            amount_counter: fx(amount),
            age_secs: age,
        }
    }

    #[test]
    fn off_by_default_keeps_every_offer_sorted() {
        let f = OfferFilter::default();
        assert!(!f.is_active());
        let (bids, s) = filter_bids(
            vec![offer("A", "0.9", "100", 0), offer("B", "1.0", "50", 0)],
            &f,
        )
        .unwrap();
        assert_eq!(bids.len(), 2);
        assert_eq!(bids[0].price, fx("1.0"));
        assert_eq!(s.capped_counter, Fx::ZERO);
    }

    #[test]
    fn young_offers_do_not_count() {
        let f = OfferFilter {
            min_age_secs: 3600,
            ..OfferFilter::default()
        };
        let (bids, s) = filter_bids(
            vec![
                offer("A", "1.0", "1000000", 60),
                offer("B", "0.99", "500", 7200),
            ],
            &f,
        )
        .unwrap();
        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].amount_counter, fx("500"));
        assert_eq!(s.too_young, 1);
        assert_eq!(s.dropped_young_counter, fx("1000000"));
    }

    #[test]
    fn one_account_cannot_set_the_figure() {
        let f = OfferFilter {
            max_seller_share_bps: 5_000,
            ..OfferFilter::default()
        };
        // A whale bids 900 at the best price; three others bid 100 in total.
        let (bids, s) = filter_bids(
            vec![
                offer("WHALE", "1.00", "900", 0),
                offer("B", "0.99", "40", 0),
                offer("C", "0.98", "30", 0),
                offer("D", "0.97", "30", 0),
            ],
            &f,
        )
        .unwrap();
        // Allowance is 50% of 1000 = 500 per account.
        assert_eq!(bids[0].amount_counter, fx("500"));
        assert_eq!(s.capped_counter, fx("400"));
        let counted = bids
            .iter()
            .fold(Fx::ZERO, |t, b| t.checked_add(b.amount_counter).unwrap());
        assert_eq!(counted, fx("600"));
    }

    #[test]
    fn the_cap_applies_across_an_accounts_offers_best_first() {
        let f = OfferFilter {
            max_seller_share_bps: 5_000,
            ..OfferFilter::default()
        };
        let (bids, _) = filter_bids(
            vec![
                offer("A", "1.00", "300", 0),
                offer("A", "0.90", "300", 0),
                offer("B", "0.95", "400", 0),
            ],
            &f,
        )
        .unwrap();
        // A's allowance (500) is spent on its better offer first.
        assert_eq!(bids[0].amount_counter, fx("300"));
        assert_eq!(bids[1].amount_counter, fx("400"));
        assert_eq!(bids[2].amount_counter, fx("200"));
    }

    #[test]
    fn rejects_a_zero_share() {
        let f = OfferFilter {
            max_seller_share_bps: 0,
            ..OfferFilter::default()
        };
        assert!(filter_bids(vec![], &f).is_err());
    }
}
