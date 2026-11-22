//! Choosing the reference price that every venue's band is measured from.
//!
//! A venue must not be its own reference. If depth on the SDEX were measured from the SDEX
//! mid, pushing that mid would move the band with it, and a manipulated market would still
//! look deep. Instead each venue reports its mid in USD, and the reference is the **upper
//! median** of the mids of venues that hold at least a minimum amount of exit liquidity.
//!
//! - A thin venue cannot move the reference at all. To count, a venue needs at least
//!   `min_depth_usd` **and** at least [`RELATIVE_MIN_BPS`] of the deepest venue's depth, so
//!   a cheap pool or a few offers placed at a made-up price cannot vote.
//! - With an even number of qualifying venues the higher middle value is used, so a tie
//!   resolves toward a higher reference, which can only lower measured depth.
//! - Pushing one venue's price up makes the other venues' depth shrink, never grow: the
//!   attack can make an asset look thinner (safe for a lender), not deeper.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fixed::Fx;

/// One venue's own price, in USD per unit of the asset, and how deep it is at that price.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueMid {
    pub label: String,
    pub mid_usd: Fx,
    /// Exit at the widest band measured from the venue's own mid, in USD.
    pub depth_usd: Fx,
}

/// The reference chosen for one asset, and which venues were allowed to set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub price_usd: Fx,
    pub qualifying: Vec<String>,
}

/// A venue must hold at least this share of the deepest venue's depth to help set the
/// reference (5%). An absolute floor alone is too cheap to clear.
pub const RELATIVE_MIN_BPS: u32 = 500;

/// Upper median of the mids of venues with at least `min_depth_usd` of depth and at least
/// [`RELATIVE_MIN_BPS`] of the deepest venue's depth.
/// Returns `None` when no venue qualifies: then there is no trustworthy price to measure
/// from, and the caller must count no depth at all.
pub fn consensus_reference(mids: &[VenueMid], min_depth_usd: Fx) -> Option<Reference> {
    let deepest = mids.iter().map(|m| m.depth_usd).max().unwrap_or(Fx::ZERO);
    let relative = deepest.mul_bps_down(RELATIVE_MIN_BPS).unwrap_or(deepest);
    let floor = min_depth_usd.max(relative);
    let mut q: Vec<&VenueMid> = mids
        .iter()
        .filter(|m| !m.mid_usd.is_zero() && m.depth_usd >= floor)
        .collect();
    if q.is_empty() {
        return None;
    }
    q.sort_by_key(|m| m.mid_usd);
    let pick = q[q.len() / 2];
    Some(Reference {
        price_usd: pick.mid_usd,
        qualifying: q.iter().map(|m| m.label.clone()).collect(),
    })
}

/// Signed distance of `mid` from `reference`, in basis points (negative = below).
pub fn deviation_bps(mid: Fx, reference: Fx) -> Result<i64, CoreError> {
    if reference.is_zero() {
        return Err(CoreError::DivByZero);
    }
    let diff = mid.raw() - reference.raw();
    let bps = diff
        .checked_mul(10_000)
        .ok_or(CoreError::Overflow)?
        .checked_div(reference.raw())
        .ok_or(CoreError::DivByZero)?;
    i64::try_from(bps).map_err(|_| CoreError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    fn m(label: &str, mid: &str, depth: &str) -> VenueMid {
        VenueMid {
            label: label.into(),
            mid_usd: fx(mid),
            depth_usd: fx(depth),
        }
    }

    #[test]
    fn thin_venue_cannot_move_the_reference() {
        let mids = [
            m("sdex", "1.08", "270000"),
            m("aquarius", "1.07", "110000"),
            m("dust-pool", "5.00", "40"),
        ];
        let r = consensus_reference(&mids, fx("100")).unwrap();
        assert_eq!(r.price_usd, fx("1.08"));
        assert_eq!(
            r.qualifying,
            vec!["aquarius".to_string(), "sdex".to_string()]
        );
    }

    #[test]
    fn a_cheap_venue_at_a_made_up_price_cannot_vote() {
        // Two real venues, and two cheap ones placed high to drag the upper median up.
        let mids = [
            m("sdex", "1.08", "270000"),
            m("aquarius", "1.07", "107000"),
            m("fake-1", "1.60", "2000"),
            m("fake-2", "1.70", "2000"),
        ];
        let r = consensus_reference(&mids, fx("100")).unwrap();
        assert_eq!(r.price_usd, fx("1.08"));
        assert_eq!(r.qualifying.len(), 2);
    }

    #[test]
    fn even_count_takes_the_higher_middle() {
        let mids = [m("a", "1.00", "1000"), m("b", "1.10", "1000")];
        assert_eq!(
            consensus_reference(&mids, fx("100")).unwrap().price_usd,
            fx("1.10")
        );
    }

    #[test]
    fn odd_count_ignores_one_outlier() {
        let mids = [
            m("a", "1.00", "1000"),
            m("b", "1.01", "1000"),
            m("pushed", "1.50", "1000"),
        ];
        assert_eq!(
            consensus_reference(&mids, fx("100")).unwrap().price_usd,
            fx("1.01")
        );
    }

    #[test]
    fn no_qualifying_venue_means_no_reference() {
        let mids = [m("a", "1.00", "5"), m("b", "0", "100000")];
        assert!(consensus_reference(&mids, fx("100")).is_none());
    }

    #[test]
    fn deviation_is_signed() {
        assert_eq!(deviation_bps(fx("0.97"), fx("1")).unwrap(), -300);
        assert_eq!(deviation_bps(fx("1.0125"), fx("1")).unwrap(), 125);
        assert!(deviation_bps(fx("1"), Fx::ZERO).is_err());
    }
}
