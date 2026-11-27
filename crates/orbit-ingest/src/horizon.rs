//! Horizon client: `/order_book`, and `/offers` for per-offer filters.

use std::thread::sleep;
use std::time::Duration;

use orbit_core::{Ask, Bid, BidOffer, Fx, OrderBook};
use serde::Deserialize;

use crate::error::IngestError;
use crate::registry::Asset;

const ATTEMPTS: u32 = 3;
const TIMEOUT: Duration = Duration::from_secs(15);

/// The counter side of a book: native XLM or a credit asset.
#[derive(Clone, Debug)]
pub enum Side<'a> {
    Native,
    Credit(&'a Asset),
}

#[derive(Deserialize)]
struct RawLevel {
    /// Exact rational price. The decimal `price` string is rounded to 7 places and shows
    /// dust offers as "0.0000000", so we always use `price_r`.
    price_r: RawRatio,
    amount: String,
}

#[derive(Deserialize)]
struct RawRatio {
    n: i64,
    d: i64,
}

/// Converts Horizon's `price_r` to [`Fx`]. Bids round down and asks round up, the
/// directions that understate depth.
fn ratio(r: &RawRatio, round_up: bool) -> Result<Fx, orbit_core::CoreError> {
    let n = Fx::from_int(i128::from(r.n))?;
    let d = Fx::from_int(i128::from(r.d))?;
    if round_up {
        n.div_up(d)
    } else {
        n.div_down(d)
    }
}

/// Seconds per ledger used to turn a ledger count into an age. Ledgers close every ~5-6s;
/// using the fast end gives a lower bound on age, so a filter errs towards excluding offers.
const SECS_PER_LEDGER: u64 = 5;
/// Pages of 200 offers read per book. A book with more is cut off and marked truncated.
const OFFER_PAGES: usize = 5;

#[derive(Deserialize)]
struct RawOffer {
    seller: String,
    amount: String,
    price_r: RawRatio,
    last_modified_ledger: u64,
    paging_token: String,
}

#[derive(Deserialize)]
struct RawOffers {
    #[serde(rename = "_embedded")]
    embedded: RawRecords,
}

#[derive(Deserialize)]
struct RawRecords {
    records: Vec<RawOffer>,
}

#[derive(Deserialize)]
struct RawRoot {
    history_latest_ledger: u64,
}

/// Every resting buy offer for an asset in one counter, with its age.
pub struct BidOffers {
    pub offers: Vec<BidOffer>,
    /// True if the book had more offers than were read (the rest are left out).
    pub truncated: bool,
}

#[derive(Deserialize)]
struct RawBook {
    bids: Vec<RawLevel>,
    asks: Vec<RawLevel>,
}

/// A read-only Horizon client with timeouts and bounded retries.
pub struct Horizon {
    base: String,
    agent: ureq::Agent,
}

impl Horizon {
    pub fn new(base: impl Into<String>) -> Horizon {
        let agent = ureq::AgentBuilder::new()
            .timeout(TIMEOUT)
            .user_agent("orbit/0.1")
            .build();
        Horizon {
            base: base.into().trim_end_matches('/').to_string(),
            agent,
        }
    }

    pub fn mainnet() -> Horizon {
        Horizon::new("https://horizon.stellar.org")
    }

    /// The order book for `selling` priced in `buying`, as Horizon reports it.
    pub fn order_book(&self, selling: &Asset, buying: &Side<'_>) -> Result<OrderBook, IngestError> {
        let url = format!("{}/order_book", self.base);
        let mut q: Vec<(&str, String)> = vec![
            ("selling_asset_type", selling.horizon_type().to_string()),
            ("selling_asset_code", selling.code.clone()),
            ("selling_asset_issuer", selling.issuer.clone()),
            ("limit", "200".to_string()),
        ];
        match buying {
            Side::Native => q.push(("buying_asset_type", "native".to_string())),
            Side::Credit(a) => {
                q.push(("buying_asset_type", a.horizon_type().to_string()));
                q.push(("buying_asset_code", a.code.clone()));
                q.push(("buying_asset_issuer", a.issuer.clone()));
            }
        }
        let raw: RawBook = self.get_json(&url, &q)?;
        let parse = |s: &str| {
            s.parse::<Fx>().map_err(|e| IngestError::BadResponse {
                url: url.clone(),
                reason: e.to_string(),
            })
        };
        let mut book = OrderBook::default();
        for l in raw.bids {
            let price = ratio(&l.price_r, false)?;
            // A bid below 1e-7 per unit is dust: worth nothing at any band, and a zero price
            // would make the base amount undefined. Skip it.
            if price.is_zero() {
                continue;
            }
            book.bids.push(Bid {
                price,
                amount_counter: parse(&l.amount)?,
            });
        }
        for l in raw.asks {
            book.asks.push(Ask {
                price: ratio(&l.price_r, true)?,
                amount_base: parse(&l.amount)?,
            });
        }
        book.validate()?;
        Ok(book)
    }

    /// The latest closed ledger Horizon has ingested.
    pub fn latest_ledger(&self) -> Result<u64, IngestError> {
        let url = format!("{}/", self.base);
        let root: RawRoot = self.get_json(&url, &[])?;
        Ok(root.history_latest_ledger)
    }

    /// Every offer buying `asset` with `counter` (the bids of `order_book(asset, counter)`),
    /// with its seller and age, so per-offer filters can be applied.
    pub fn bid_offers(
        &self,
        asset: &Asset,
        counter: &Side<'_>,
        latest_ledger: u64,
    ) -> Result<BidOffers, IngestError> {
        let url = format!("{}/offers", self.base);
        let mut base: Vec<(&str, String)> = vec![
            ("buying_asset_type", asset.horizon_type().to_string()),
            ("buying_asset_code", asset.code.clone()),
            ("buying_asset_issuer", asset.issuer.clone()),
            ("limit", "200".to_string()),
            ("order", "asc".to_string()),
        ];
        match counter {
            Side::Native => base.push(("selling_asset_type", "native".to_string())),
            Side::Credit(a) => {
                base.push(("selling_asset_type", a.horizon_type().to_string()));
                base.push(("selling_asset_code", a.code.clone()));
                base.push(("selling_asset_issuer", a.issuer.clone()));
            }
        }
        let bad = |reason: String| IngestError::BadResponse {
            url: url.clone(),
            reason,
        };
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..OFFER_PAGES {
            let mut q = base.clone();
            if let Some(c) = &cursor {
                q.push(("cursor", c.clone()));
            }
            let page: RawOffers = self.get_json(&url, &q)?;
            let n = page.embedded.records.len();
            for o in page.embedded.records {
                cursor = Some(o.paging_token.clone());
                // The offer sells COUNTER for BASE at price_r = BASE per COUNTER; a bid is
                // COUNTER per BASE, the inverse, rounded down.
                let inverse = RawRatio {
                    n: o.price_r.d,
                    d: o.price_r.n,
                };
                if inverse.d == 0 {
                    continue;
                }
                let price = ratio(&inverse, false)?;
                if price.is_zero() {
                    continue;
                }
                out.push(BidOffer {
                    seller: o.seller,
                    price,
                    amount_counter: o.amount.parse::<Fx>().map_err(|e| bad(e.to_string()))?,
                    age_secs: latest_ledger.saturating_sub(o.last_modified_ledger)
                        * SECS_PER_LEDGER,
                });
            }
            if n < 200 {
                return Ok(BidOffers {
                    offers: out,
                    truncated: false,
                });
            }
        }
        Ok(BidOffers {
            offers: out,
            truncated: true,
        })
    }

    fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, String)],
    ) -> Result<T, IngestError> {
        let mut last = String::new();
        for attempt in 1..=ATTEMPTS {
            let mut req = self.agent.get(url);
            for (k, v) in query {
                req = req.query(k, v);
            }
            match req.call() {
                Ok(resp) => {
                    return resp.into_json::<T>().map_err(|e| IngestError::BadResponse {
                        url: url.to_string(),
                        reason: e.to_string(),
                    })
                }
                // 4xx other than rate limiting will not get better on retry.
                Err(ureq::Error::Status(code, _)) if code != 429 && code < 500 => {
                    return Err(IngestError::Http {
                        url: url.to_string(),
                        attempts: attempt,
                        reason: format!("status {code}"),
                    })
                }
                Err(e) => last = e.to_string(),
            }
            if attempt < ATTEMPTS {
                sleep(Duration::from_millis(500 * u64::from(attempt)));
            }
        }
        Err(IngestError::Http {
            url: url.to_string(),
            attempts: ATTEMPTS,
            reason: last,
        })
    }
}
