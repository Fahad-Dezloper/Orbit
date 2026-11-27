//! How much of each measured asset sits in lending pools, read on-chain.
//!
//! This is the other half of a cap: depth says how much could be sold, this says how much a
//! lender holds that might have to be. Blend v2 pools are read here, on-chain; Templar
//! markets (on NEAR) are read from Templar's API in `templar.rs`.
//!
//! As with AMM pools, nothing is taken on trust. A pinned Blend pool is used only if the
//! pinned Blend pool factory confirms it deployed it (`is_pool`), so the pool runs Blend's
//! own code. Each reserve is matched to a measured asset by its locally derived SAC
//! address, never by code. A reserve's reported cash (supplied minus borrowed) is checked
//! against what the token contract says the pool holds.
//!
//! Read-only: simulations only.

use orbit_core::{liquidatable_at, Fx};
use serde::{Deserialize, Serialize};
use stellar_xdr::{ContractId, Hash, ScAddress, ScVal};

use crate::error::IngestError;
use crate::measure::AssetReport;
use crate::pools::held_balance;
use crate::registry::Asset;
use crate::soroban::{self, SorobanRpc};

/// Blend v2 keeps interest rates with 12 decimals.
const RATE_SCALE: i128 = 1_000_000_000_000;

/// Largest shortfall allowed between a reserve's cash and the pool's token balance.
pub const CASH_TOLERANCE_BPS: u32 = 10;

/// One pinned lending pool from the registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LenderSpec {
    /// A label for people; the contract address is the identity.
    pub name: String,
    pub contract: String,
}

/// One measured asset's reserve in one lending pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LenderReserve {
    pub lender: String,
    pub contract: String,
    /// The pool's status as Blend names it (e.g. "active", "on_ice", "frozen").
    pub status: String,
    pub code: String,
    /// Tokens supplied to the reserve, collateral and plain lending together.
    pub supplied: Fx,
    /// Tokens borrowed from the reserve.
    pub borrowed: Fx,
    /// Share of the collateral's value the pool lets be borrowed against it.
    pub c_factor_bps: u32,
    /// What the token contract says the pool holds.
    pub held: Fx,
}

/// A reserve valued against the same run's depth.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exposure {
    pub lender: String,
    pub contract: String,
    pub status: String,
    pub code: String,
    pub supplied_usd: Fx,
    /// `supplied_usd * c_factor`: the most that can be borrowed against it.
    pub borrowable_usd: Fx,
    /// Collateral that can be sold before the bonus is gone, across all venues.
    pub exit_usd_at_bonus: Option<Fx>,
    /// True when the pool holds more of the asset than the market could absorb at the
    /// bonus: a liquidation of all of it would not clear.
    pub exceeds_exit: bool,
}

/// Blend v2 pool status codes.
pub fn status_name(code: u32) -> &'static str {
    match code {
        0 => "admin_active",
        1 => "active",
        2 => "admin_on_ice",
        3 => "on_ice",
        4 => "admin_frozen",
        5 => "frozen",
        6 => "setup",
        _ => "unknown",
    }
}

fn bad(what: &str, why: impl std::fmt::Display) -> IngestError {
    IngestError::BadResponse {
        url: what.to_string(),
        reason: why.to_string(),
    }
}

fn address_arg(contract: &str) -> Result<ScVal, IngestError> {
    let id = soroban::parse_contract(contract)?;
    Ok(ScVal::Address(ScAddress::Contract(ContractId(Hash(id)))))
}

fn field<'a>(v: &'a ScVal, key: &str) -> Result<&'a ScVal, IngestError> {
    let ScVal::Map(Some(m)) = v else {
        return Err(bad(key, "expected a struct"));
    };
    m.iter()
        .find(|e| matches!(&e.key, ScVal::Symbol(s) if s.to_string() == key))
        .map(|e| &e.val)
        .ok_or_else(|| bad(key, "missing field"))
}

fn int(v: &ScVal, what: &str) -> Result<i128, IngestError> {
    match v {
        ScVal::I128(p) => Ok((i128::from(p.hi) << 64) | i128::from(p.lo)),
        ScVal::U32(n) => Ok(i128::from(*n)),
        ScVal::I64(n) => Ok(i128::from(*n)),
        ScVal::U64(n) => Ok(i128::from(*n)),
        _ => Err(bad(what, "expected an integer")),
    }
}

/// `b_supply * rate / 1e12`, rounded down.
fn underlying(supply: i128, rate: i128, what: &str) -> Result<Fx, IngestError> {
    if supply < 0 || rate < 0 {
        return Err(bad(what, "negative supply or rate"));
    }
    let raw = supply
        .checked_mul(rate)
        .ok_or_else(|| bad(what, "overflow"))?
        / RATE_SCALE;
    Fx::from_raw(raw).map_err(IngestError::from)
}

/// The figures of one reserve, decoded from `get_reserve`. Pure, for testing.
pub fn reserve_from(v: &ScVal) -> Result<(Fx, Fx, u32, u32), IngestError> {
    let config = field(v, "config")?;
    let data = field(v, "data")?;
    let decimals = int(field(config, "decimals")?, "decimals")?;
    let c_factor = int(field(config, "c_factor")?, "c_factor")?;
    let supplied = underlying(
        int(field(data, "b_supply")?, "b_supply")?,
        int(field(data, "b_rate")?, "b_rate")?,
        "b_supply",
    )?;
    let borrowed = underlying(
        int(field(data, "d_supply")?, "d_supply")?,
        int(field(data, "d_rate")?, "d_rate")?,
        "d_supply",
    )?;
    // c_factor has 7 decimals: 9_500_000 is 95%.
    let c_factor_bps = u32::try_from((c_factor / 1_000).clamp(0, 10_000))
        .map_err(|_| bad("c_factor", "out of range"))?;
    let decimals = u32::try_from(decimals).map_err(|_| bad("decimals", "out of range"))?;
    Ok((supplied, borrowed, c_factor_bps, decimals))
}

/// Rejects a reserve whose cash (supplied minus borrowed) exceeds what the pool holds.
pub fn check_cash(supplied: Fx, borrowed: Fx, held: Fx) -> Result<(), String> {
    let cash = supplied.checked_sub(borrowed).unwrap_or(Fx::ZERO);
    let allowed = held
        .checked_add(held.mul_bps_down(CASH_TOLERANCE_BPS).unwrap_or(Fx::ZERO))
        .and_then(|x| x.checked_add(Fx::ONE))
        .map_err(|e| e.to_string())?;
    if cash > allowed {
        return Err(format!("reports {cash} of cash but holds {held}"));
    }
    Ok(())
}

/// Reads every measured asset's reserve in one pinned pool. `assets` are the measured
/// assets whose SAC addresses identify reserves.
pub fn read_lender(
    rpc: &SorobanRpc,
    factory: &str,
    spec: &LenderSpec,
    assets: &[Asset],
) -> Result<Vec<LenderReserve>, IngestError> {
    let c = &spec.contract;
    let is_pool = rpc.read(factory, "is_pool", vec![address_arg(c)?])?;
    if !matches!(is_pool, ScVal::Bool(true)) {
        return Err(bad(c, "not deployed by the pinned Blend pool factory"));
    }
    let status = int(
        field(&rpc.read(c, "get_config", vec![])?, "status")?,
        "status",
    )?;
    let status = status_name(u32::try_from(status).unwrap_or(u32::MAX)).to_string();
    let listed = soroban::contract_addresses(&rpc.read(c, "get_reserve_list", vec![])?)?;
    let mut out = Vec::new();
    for a in assets {
        let sac = a.token_address(rpc.passphrase())?;
        if !listed.contains(&sac) {
            continue;
        }
        let v = rpc.read(c, "get_reserve", vec![address_arg(&sac)?])?;
        let (supplied, borrowed, c_factor_bps, decimals) = reserve_from(&v)?;
        if decimals != 7 {
            return Err(bad(
                c,
                format!("{} reserve has {decimals} decimals", a.code),
            ));
        }
        let held = held_balance(rpc, &sac, c)?;
        check_cash(supplied, borrowed, held).map_err(|why| bad(c, format!("{}: {why}", a.code)))?;
        out.push(LenderReserve {
            lender: spec.name.clone(),
            contract: c.clone(),
            status: status.clone(),
            code: a.code.clone(),
            supplied,
            borrowed,
            c_factor_bps,
            held,
        });
    }
    Ok(out)
}

/// Values a reserve at its asset's reference price and compares it with the exit at
/// `bonus_bps`. `None` when the asset has no report or no reference price in the run.
pub fn exposure(r: &LenderReserve, report: &AssetReport, bonus_bps: u32) -> Option<Exposure> {
    let price = report.reference.as_ref()?.price_usd;
    let supplied_usd = r.supplied.mul_down(price).ok()?;
    let borrowable_usd = supplied_usd.mul_bps_down(r.c_factor_bps).ok()?;
    let exit = liquidatable_at(&report.curve, bonus_bps);
    Some(Exposure {
        lender: r.lender.clone(),
        contract: r.contract.clone(),
        status: r.status.clone(),
        code: r.code.clone(),
        supplied_usd,
        borrowable_usd,
        exit_usd_at_bonus: exit,
        exceeds_exit: exit.is_none_or(|e| supplied_usd > e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::{Int128Parts, ScMap, ScMapEntry, ScSymbol};

    fn i(n: i128) -> ScVal {
        ScVal::I128(Int128Parts {
            hi: (n >> 64) as i64,
            lo: n as u64,
        })
    }

    fn map(entries: Vec<(&str, ScVal)>) -> ScVal {
        let es: Vec<ScMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ScMapEntry {
                key: ScVal::Symbol(ScSymbol(k.try_into().unwrap())),
                val: v,
            })
            .collect();
        ScVal::Map(Some(ScMap(es.try_into().unwrap())))
    }

    /// The EURC reserve of Blend's Fixed pool as read on mainnet (Nov 2026).
    fn eurc_reserve() -> ScVal {
        map(vec![
            (
                "config",
                map(vec![
                    ("c_factor", ScVal::U32(9_500_000)),
                    ("decimals", ScVal::U32(7)),
                ]),
            ),
            (
                "data",
                map(vec![
                    ("b_rate", i(1_142_481_042_376)),
                    ("b_supply", i(646_490_334_474)),
                    ("d_rate", i(1_229_852_584_101)),
                    ("d_supply", i(332_019_479_244)),
                ]),
            ),
        ])
    }

    #[test]
    fn decodes_a_mainnet_reserve() {
        let (supplied, borrowed, cf, dec) = reserve_from(&eurc_reserve()).unwrap();
        // 64,649.0334474 b-tokens at 1.142481042376 = 73,860.2951215 EURC, rounded down.
        assert_eq!(supplied.to_string(), "73860.2951215");
        assert!(borrowed.to_string().starts_with("40833."));
        assert_eq!(cf, 9_500);
        assert_eq!(dec, 7);
    }

    #[test]
    fn missing_fields_are_rejected() {
        let v = map(vec![("config", map(vec![]))]);
        assert!(reserve_from(&v).is_err());
        assert!(reserve_from(&ScVal::Void).is_err());
    }

    #[test]
    fn cash_must_be_held() {
        let f = |s: &str| s.parse::<Fx>().unwrap();
        assert!(check_cash(f("1000"), f("400"), f("600")).is_ok());
        // Extra tokens (donations, backstop credit) are fine.
        assert!(check_cash(f("1000"), f("400"), f("900")).is_ok());
        // Claiming cash the pool does not hold is not.
        assert!(check_cash(f("1000"), f("400"), f("500")).is_err());
    }

    #[test]
    fn status_names() {
        assert_eq!(status_name(1), "active");
        assert_eq!(status_name(3), "on_ice");
        assert_eq!(status_name(99), "unknown");
    }
}
