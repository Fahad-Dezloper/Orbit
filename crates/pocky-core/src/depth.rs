//! Combining venues into one depth figure per asset, and classifying it.

use serde::{Deserialize, Serialize};

use crate::band::Band;
use crate::book::ExitQuote;
use crate::error::CoreError;
use crate::fixed::Fx;

/// Where liquidity was measured.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Venue {
    /// SDEX order book against a counter asset, e.g. `"USDC"` or `"XLM"`.
    Sdex { counter: String },
    /// A Soroban AMM pool, e.g. protocol `"aquarius"` and its contract id.
    Amm {
        protocol: String,
        pool: String,
        counter: String,
    },
}

/// Exit on one venue at one band, converted to USD.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueExit {
    pub venue: Venue,
    pub band: Band,
    pub quote: ExitQuote,
    /// USD value of one unit of the venue's counter asset.
    pub usd_per_counter: Fx,
    pub usd: Fx,
}

impl VenueExit {
    pub fn new(
        venue: Venue,
        band: Band,
        quote: ExitQuote,
        usd_per_counter: Fx,
    ) -> Result<Self, CoreError> {
        let usd = quote.counter_raised.mul_down(usd_per_counter)?;
        Ok(VenueExit {
            venue,
            band,
            quote,
            usd_per_counter,
            usd,
        })
    }
}

/// How healthy an asset's exit liquidity is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Effectively unsellable.
    Dead,
    /// A single small position could not be exited.
    Critical,
    /// Exit exists but is thin relative to institutional sizes.
    Shallow,
    /// Meaningful exit liquidity right now.
    Healthy,
}

/// USD thresholds separating the verdicts. Exit below `dead_below` is Dead, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thresholds {
    pub dead_below: Fx,
    pub critical_below: Fx,
    pub shallow_below: Fx,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            dead_below: Fx::whole(100),
            critical_below: Fx::whole(10_000),
            shallow_below: Fx::whole(100_000),
        }
    }
}

impl Thresholds {
    pub fn classify(&self, usd: Fx) -> Verdict {
        if usd < self.dead_below {
            Verdict::Dead
        } else if usd < self.critical_below {
            Verdict::Critical
        } else if usd < self.shallow_below {
            Verdict::Shallow
        } else {
            Verdict::Healthy
        }
    }
}

/// Total exit for one asset at one band across all venues.
///
/// Venues are summed: a seller can split a sale across independent venues, keeping each
/// part within the band, so the combined average also stays within the band.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetDepth {
    pub asset: String,
    pub band: Band,
    pub venues: Vec<VenueExit>,
    pub total_usd: Fx,
    pub verdict: Verdict,
}

impl AssetDepth {
    pub fn combine(
        asset: impl Into<String>,
        band: Band,
        venues: Vec<VenueExit>,
        t: &Thresholds,
    ) -> Result<Self, CoreError> {
        let mut total = Fx::ZERO;
        for v in &venues {
            if v.band != band {
                return Err(CoreError::BadBand(v.band.bps()));
            }
            total = total.checked_add(v.usd)?;
        }
        Ok(AssetDepth {
            asset: asset.into(),
            band,
            venues,
            total_usd: total,
            verdict: t.classify(total),
        })
    }
}

/// Collateral at risk: how many times over the borrowed amount exceeds what the market can
/// absorb. `None` means unbounded (no exit at all). Above 1.0 means a full liquidation
/// cannot be completed on-chain within the band.
pub fn collateral_at_risk(borrowed_usd: Fx, exit_usd: Fx) -> Result<Option<Fx>, CoreError> {
    if exit_usd.is_zero() {
        return Ok(if borrowed_usd.is_zero() {
            Some(Fx::ZERO)
        } else {
            None
        });
    }
    borrowed_usd.div_up(exit_usd).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    #[test]
    fn verdicts_follow_research_figures() {
        let t = Thresholds::default();
        assert_eq!(t.classify(fx("4")), Verdict::Dead); // KTB
        assert_eq!(t.classify(fx("1261")), Verdict::Critical); // USDY, SDEX only
        assert_eq!(t.classify(fx("58033")), Verdict::Shallow); // TESOURO
        assert_eq!(t.classify(fx("379567")), Verdict::Healthy); // USTRY
    }

    #[test]
    fn combine_sums_venues_and_rejects_mixed_bands() {
        let t = Thresholds::default();
        let q = ExitQuote {
            base_sold: fx("1"),
            counter_raised: fx("100"),
        };
        let usdc = VenueExit::new(
            Venue::Sdex {
                counter: "USDC".into(),
            },
            Band::TEN_PCT,
            q,
            Fx::ONE,
        )
        .unwrap();
        let xlm = VenueExit::new(
            Venue::Sdex {
                counter: "XLM".into(),
            },
            Band::TEN_PCT,
            q,
            fx("0.14"),
        )
        .unwrap();
        let d = AssetDepth::combine("USTRY", Band::TEN_PCT, vec![usdc.clone(), xlm], &t).unwrap();
        assert_eq!(d.total_usd, fx("114"));
        assert_eq!(d.verdict, Verdict::Critical);

        let other = VenueExit::new(
            Venue::Sdex {
                counter: "USDC".into(),
            },
            Band::TWO_PCT,
            q,
            Fx::ONE,
        )
        .unwrap();
        assert!(AssetDepth::combine("X", Band::TEN_PCT, vec![usdc, other], &t).is_err());
    }

    #[test]
    fn collateral_at_risk_cases() {
        // deJAAA: $3.2M borrowed, $0 exit -> unbounded
        assert_eq!(collateral_at_risk(fx("3199681"), Fx::ZERO).unwrap(), None);
        assert_eq!(
            collateral_at_risk(Fx::ZERO, Fx::ZERO).unwrap(),
            Some(Fx::ZERO)
        );
        // Templar USTRY capacity reading: 99,000 / 379,567 ~= 0.26
        let r = collateral_at_risk(fx("99000"), fx("379567"))
            .unwrap()
            .unwrap();
        assert!(r > fx("0.26") && r < fx("0.27"));
    }
}
