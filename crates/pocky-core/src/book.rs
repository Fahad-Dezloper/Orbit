//! Exit liquidity on a Stellar DEX (SDEX) order book.
//!
//! ## The unit convention that decides correctness
//!
//! For a Horizon `/order_book` request with `selling = BASE` and `buying = COUNTER`:
//! - an **ask** sells BASE, and its `amount` is in **BASE** units;
//! - a **bid** sells COUNTER to buy BASE, and its `amount` is in **COUNTER** units.
//!
//! Prices are always COUNTER per BASE. So selling BASE into a bid of `(price, amount)`
//! raises exactly `amount` COUNTER and absorbs `amount / price` BASE. Multiplying the bid
//! amount by its price is a common bug that overstates USDC pairs by the price and XLM
//! pairs by about 5x.

use serde::{Deserialize, Serialize};

use crate::band::Band;
use crate::error::CoreError;
use crate::fixed::Fx;

/// One bid level as Horizon reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bid {
    /// COUNTER per BASE.
    pub price: Fx,
    /// In COUNTER units (what the bidder is paying).
    pub amount_counter: Fx,
}

/// One ask level as Horizon reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    /// COUNTER per BASE.
    pub price: Fx,
    /// In BASE units (what the asker is selling).
    pub amount_base: Fx,
}

/// A snapshot of one SDEX book. Bids best (highest) first, asks best (lowest) first,
/// exactly as Horizon returns them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderBook {
    pub bids: Vec<Bid>,
    pub asks: Vec<Ask>,
}

/// What a sale into a venue would achieve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitQuote {
    /// BASE sold.
    pub base_sold: Fx,
    /// COUNTER received.
    pub counter_raised: Fx,
}

impl OrderBook {
    /// Checks the book is sorted best-first on both sides and has no zero prices.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.bids.iter().any(|b| b.price.is_zero())
            || self.asks.iter().any(|a| a.price.is_zero())
        {
            return Err(CoreError::UnsortedBook);
        }
        let bids_sorted = self.bids.windows(2).all(|w| w[0].price >= w[1].price);
        let asks_sorted = self.asks.windows(2).all(|w| w[0].price <= w[1].price);
        if bids_sorted && asks_sorted {
            Ok(())
        } else {
            Err(CoreError::UnsortedBook)
        }
    }

    /// Mid price, `(best_bid + best_ask) / 2`, rounded down. Fails when either side is
    /// empty: a one-sided book has no market reference, and the caller must supply one.
    pub fn mid(&self) -> Result<Fx, CoreError> {
        let bid = self.bids.first().ok_or(CoreError::NoMid)?.price;
        let ask = self.asks.first().ok_or(CoreError::NoMid)?.price;
        bid.checked_add(ask)?.div_down(Fx::from_int(2)?)
    }

    /// Largest sale of BASE into the bids whose **average** execution price stays at or
    /// above `reference * (1 - band)`.
    ///
    /// Walks bids best-first. Levels priced at or above the floor are taken whole. Once
    /// prices fall below the floor, levels are taken only while the accumulated surplus
    /// above the floor can absorb them, and the last level is taken partially.
    pub fn exit(&self, reference: Fx, band: Band) -> Result<ExitQuote, CoreError> {
        self.validate()?;
        let floor = band.floor(reference)?;
        let mut q = ExitQuote::default();

        for bid in &self.bids {
            if bid.amount_counter.is_zero() {
                continue;
            }
            // Round the base absorbed UP: assuming we sold slightly more base for the same
            // counter lowers the average price, which is the conservative direction.
            let level_base = bid.amount_counter.div_up(bid.price)?;

            if bid.price >= floor {
                q.base_sold = q.base_sold.checked_add(level_base)?;
                q.counter_raised = q.counter_raised.checked_add(bid.amount_counter)?;
                continue;
            }

            // surplus = counter_raised - floor * base_sold (>= 0 by construction)
            let floor_value = floor.mul_down(q.base_sold)?;
            let surplus = match q.counter_raised.checked_sub(floor_value) {
                Ok(s) => s,
                Err(_) => break,
            };
            if surplus.is_zero() {
                break;
            }
            let gap = floor.checked_sub(bid.price)?;
            let max_base = surplus.div_down(gap)?;
            if max_base >= level_base {
                q.base_sold = q.base_sold.checked_add(level_base)?;
                q.counter_raised = q.counter_raised.checked_add(bid.amount_counter)?;
            } else {
                q.base_sold = q.base_sold.checked_add(max_base)?;
                q.counter_raised = q
                    .counter_raised
                    .checked_add(max_base.mul_down(bid.price)?)?;
                break;
            }
        }
        Ok(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }
    fn bid(p: &str, a: &str) -> Bid {
        Bid {
            price: fx(p),
            amount_counter: fx(a),
        }
    }
    fn ask(p: &str, a: &str) -> Ask {
        Ask {
            price: fx(p),
            amount_base: fx(a),
        }
    }

    #[test]
    fn bid_amount_is_counter_not_price_times_amount() {
        // One bid paying 100 USDC at 2 USDC/token. Selling into it raises 100 USDC, not 200.
        let book = OrderBook {
            bids: vec![bid("2", "100")],
            asks: vec![ask("2", "1")],
        };
        let q = book.exit(fx("2"), Band::TEN_PCT).unwrap();
        assert_eq!(q.counter_raised, fx("100"));
        assert_eq!(q.base_sold, fx("50"));
    }

    #[test]
    fn levels_inside_the_band_are_taken_whole() {
        let book = OrderBook {
            bids: vec![bid("0.99", "99"), bid("0.95", "95"), bid("0.5", "1000")],
            asks: vec![ask("1.01", "10")],
        };
        let q = book.exit(fx("1"), Band::TEN_PCT).unwrap();
        // first two levels are above the 0.9 floor; the 0.5 level is partially usable
        assert!(q.counter_raised >= fx("194"));
        // average price must stay >= floor
        let avg = q.counter_raised.div_down(q.base_sold).unwrap();
        assert!(avg >= fx("0.9"), "average {avg} fell below the floor");
    }

    #[test]
    fn partial_level_lands_exactly_on_the_floor() {
        // 100 counter at 1.0 (100 base), then a deep level at 0.8.
        // Floor 0.9: c + 0.8 t = 0.9 (100 + t) -> t = 100, raising 80 more.
        let book = OrderBook {
            bids: vec![bid("1", "100"), bid("0.8", "8000")],
            asks: vec![],
        };
        let q = book.exit(fx("1"), Band::TEN_PCT).unwrap();
        assert_eq!(q.base_sold, fx("200"));
        assert_eq!(q.counter_raised, fx("180"));
    }

    #[test]
    fn nothing_above_floor_means_zero_exit() {
        let book = OrderBook {
            bids: vec![bid("0.5", "1000")],
            asks: vec![],
        };
        assert_eq!(
            book.exit(fx("1"), Band::TEN_PCT).unwrap(),
            ExitQuote::default()
        );
    }

    #[test]
    fn dead_asset_with_empty_book() {
        let book = OrderBook::default();
        assert_eq!(
            book.exit(fx("1"), Band::TEN_PCT).unwrap(),
            ExitQuote::default()
        );
        assert_eq!(book.mid(), Err(CoreError::NoMid));
    }

    #[test]
    fn rejects_unsorted_books() {
        let book = OrderBook {
            bids: vec![bid("0.9", "1"), bid("1", "1")],
            asks: vec![],
        };
        assert_eq!(
            book.exit(fx("1"), Band::TEN_PCT),
            Err(CoreError::UnsortedBook)
        );
    }

    #[test]
    fn february_shape_spoofed_top_of_book() {
        // Shape of the USTRY book before the 22 Feb 2026 exploit: a tiny bid near the
        // inflated price and nothing else. Whatever the oracle says, the exit is tiny.
        let book = OrderBook {
            bids: vec![bid("106.74", "5"), bid("1.06", "3")],
            asks: vec![ask("106.80", "1")],
        };
        let mid = book.mid().unwrap();
        let q = book.exit(mid, Band::TEN_PCT).unwrap();
        assert!(
            q.counter_raised < fx("6"),
            "exit {} should be tiny",
            q.counter_raised
        );
    }

    #[test]
    fn mid_rounds_down() {
        let book = OrderBook {
            bids: vec![bid("1", "1")],
            asks: vec![ask("1.0000001", "1")],
        };
        assert_eq!(book.mid().unwrap(), fx("1"));
    }
}
