//! Collateral held in Templar lending markets, read from Templar's public API.
//!
//! Templar markets run on NEAR. They hold Stellar tokens (e.g. deJAAA) through NEAR Intents,
//! so the collateral is not visible as a Soroban balance. Orbit reads Templar's own API
//! (`GET /v1/markets`) and matches a market's collateral to a measured asset only through a
//! token id pinned in the registry, never by name.
//!
//! Unlike Blend reserves, these figures are **reported by Templar, not verified on Stellar**:
//! each reserve is labelled `Templar (reported)`. Read-only: HTTP GETs.

use std::time::Duration;

use orbit_core::Fx;
use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::lenders::LenderReserve;

pub const DEFAULT_API: &str = "https://api.templarfi.org";
const PAGES: usize = 10;
const TIMEOUT: Duration = Duration::from_secs(20);

/// A NEAR Intents token id pinned to a measured asset code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplarToken {
    pub asset: String,
    /// The market's `collateralAsset`, e.g. `nep245:v2_1.omni.hot.tg:1100_...`.
    pub token_id: String,
}

#[derive(Deserialize)]
struct Page {
    data: Vec<Market>,
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    #[serde(rename = "hasMore")]
    has_more: bool,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct Market {
    #[serde(rename = "marketId")]
    market_id: String,
    #[serde(rename = "collateralAsset")]
    collateral_asset: String,
    #[serde(rename = "totalCollateral")]
    total_collateral: String,
    status: String,
    configuration: Config,
}

#[derive(Deserialize)]
struct Config {
    price_oracle_configuration: OracleConfig,
    /// Collateral value / debt below which a position is liquidated, e.g. "1.05".
    borrow_mcr_liquidation: String,
}

#[derive(Deserialize)]
struct OracleConfig {
    collateral_asset_decimals: u32,
}

/// `raw / 10^decimals` as a 7-place [`Fx`], rounded down.
fn scale(raw: &str, decimals: u32) -> Result<Fx, String> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("bad amount {raw:?}"));
    }
    if decimals > 38 {
        return Err(format!("bad decimals {decimals}"));
    }
    let d = decimals as usize;
    let padded = format!("{raw:0>width$}", width = d + 1);
    let (int, frac) = padded.split_at(padded.len() - d);
    let frac7: String = frac.chars().chain(std::iter::repeat('0')).take(7).collect();
    format!("{int}.{frac7}")
        .parse::<Fx>()
        .map_err(|e| e.to_string())
}

/// Borrowable share of collateral value in basis points: `10000 / mcr`, rounded down.
fn c_factor_bps(mcr: &str) -> Result<u32, String> {
    let m: f64 = mcr.parse().map_err(|_| format!("bad mcr {mcr:?}"))?;
    if !(m >= 1.0 && m.is_finite()) {
        return Err(format!("bad mcr {mcr:?}"));
    }
    Ok((10_000.0 / m).floor() as u32)
}

fn reserves(markets: Vec<Market>, tokens: &[TemplarToken]) -> Result<Vec<LenderReserve>, String> {
    let mut out = Vec::new();
    for m in markets {
        let Some(t) = tokens.iter().find(|t| t.token_id == m.collateral_asset) else {
            continue;
        };
        let supplied = scale(
            &m.total_collateral,
            m.configuration
                .price_oracle_configuration
                .collateral_asset_decimals,
        )
        .map_err(|e| format!("{}: {e}", m.market_id))?;
        out.push(LenderReserve {
            lender: "Templar (reported)".into(),
            contract: m.market_id.clone(),
            status: m.status,
            code: t.asset.clone(),
            supplied,
            // Collateral is not lent out in Templar; the borrowed asset is USDC.
            borrowed: Fx::ZERO,
            c_factor_bps: c_factor_bps(&m.configuration.borrow_mcr_liquidation)
                .map_err(|e| format!("{}: {e}", m.market_id))?,
            held: supplied,
        });
    }
    Ok(out)
}

/// Every Templar market whose collateral is a pinned token.
pub fn read_templar(api: &str, tokens: &[TemplarToken]) -> Result<Vec<LenderReserve>, IngestError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .user_agent("orbit/0.1")
        .build();
    let url = format!("{}/v1/markets", api.trim_end_matches('/'));
    let bad = |reason: String| IngestError::BadResponse {
        url: url.clone(),
        reason,
    };
    let mut markets = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..PAGES {
        let mut req = agent.get(&url);
        if let Some(c) = &cursor {
            req = req.query("cursor", c);
        }
        let page: Page = req
            .call()
            .map_err(|e| bad(e.to_string()))?
            .into_json()
            .map_err(|e| bad(e.to_string()))?;
        markets.extend(page.data);
        match (page.pagination.has_more, page.pagination.next_cursor) {
            (true, Some(c)) => cursor = Some(c),
            _ => return reserves(markets, tokens).map_err(bad),
        }
    }
    Err(bad(format!("more than {PAGES} pages of markets")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_18_decimal_amounts() {
        // deJAAA collateral in Templar's main market (Nov 2026).
        assert_eq!(
            scale("3597184880039754358236211", 18).unwrap(),
            "3597184.8800397".parse::<Fx>().unwrap()
        );
        assert_eq!(scale("5", 8).unwrap(), "0.0000000".parse::<Fx>().unwrap());
        assert_eq!(scale("0", 18).unwrap(), Fx::ZERO);
        assert!(scale("-1", 7).is_err());
        assert!(scale("1e9", 7).is_err());
    }

    #[test]
    fn c_factor_from_mcr() {
        assert_eq!(
            c_factor_bps("1.05000000000000000000000000000000000001").unwrap(),
            9523
        );
        assert_eq!(c_factor_bps("1.25").unwrap(), 8000);
        assert!(c_factor_bps("0.5").is_err());
    }

    #[test]
    fn matches_markets_only_by_pinned_token_id() {
        let page: Page = serde_json::from_str(
            r#"{"data":[
              {"marketId":"ixlmdejaaa-ixlmusdc-1.v1.tmplr.near","collateralAsset":"nep245:x:DEJAAA",
               "totalCollateral":"2000000000000000000","status":"active",
               "configuration":{"price_oracle_configuration":{"collateral_asset_decimals":18},
                                "borrow_mcr_liquidation":"1.05"}},
              {"marketId":"fake-dejaaa.v1.tmplr.near","collateralAsset":"nep245:x:IMPOSTOR",
               "totalCollateral":"9","status":"active",
               "configuration":{"price_oracle_configuration":{"collateral_asset_decimals":0},
                                "borrow_mcr_liquidation":"1.05"}}],
             "pagination":{"hasMore":false,"nextCursor":null}}"#,
        )
        .unwrap();
        let tokens = vec![TemplarToken {
            asset: "deJAAA".into(),
            token_id: "nep245:x:DEJAAA".into(),
        }];
        let r = reserves(page.data, &tokens).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].code, "deJAAA");
        assert_eq!(r[0].supplied, "2".parse::<Fx>().unwrap());
        assert_eq!(r[0].lender, "Templar (reported)");
    }
}
