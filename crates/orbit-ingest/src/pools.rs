//! Soroban AMM pools, read on-chain and verified before they are counted.
//!
//! Pools are pinned in `assets.toml`. A pinned address is never trusted on its own: before a
//! pool's reserves are used, its token list must equal the locally derived SAC addresses of
//! the asset and the counter. A pool holding an impostor token with the same code is
//! rejected, and so is any pool whose type does not match how it is pinned.
//!
//! Two kinds are measured:
//! - **constant-product** pools (Aquarius standard, Soroswap): reserves are read and the
//!   exit is computed exactly from `x * y = k`;
//! - **quoted** pools (Aquarius concentrated and stable): the pool's own `estimate_swap` is
//!   sampled at increasing sale sizes, so its tick or curve maths is used as-is.
//!
//! A pool's answers about itself (tokens, reserves, quotes) come from the pool contract, and
//! Aquarius pools are upgradeable, so they are never taken on trust alone: what a pool
//! reports is checked against what the token contracts say it actually holds
//! ([`check_held`]). A pool that claims more than it holds is rejected.
//!
//! A pool that cannot be read or verified is left out and noted. Leaving a venue out can
//! only understate depth.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use orbit_core::{CpPool, Fx, QuoteSample, QuotedPool, QUOTE_FRACTIONS_BPS};
use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::registry::Asset;
use crate::soroban::{self, SorobanRpc, Token};

/// Soroswap pairs charge a fixed 0.3% on the input (the pair contract hard-codes 997/1000).
pub const SOROSWAP_FEE_BPS: u32 = 30;

/// How long a pool's verified tokens, type and fee are reused before being read again.
/// Reserves are always read fresh.
pub const METADATA_TTL: Duration = Duration::from_secs(6 * 3600);

type MetaCache = Mutex<HashMap<(String, String), (Instant, Vec<String>, u32)>>;

fn meta_cache() -> &'static MetaCache {
    static CACHE: OnceLock<MetaCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// Aquarius standard (constant-product) pool.
    Aquarius,
    /// Soroswap pair (constant-product, Uniswap v2 style).
    Soroswap,
    /// Sushi v3 pool (concentrated liquidity, Uniswap v3 style). Always quoted.
    Sushi,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Aquarius => "aquarius",
            Protocol::Soroswap => "soroswap",
            Protocol::Sushi => "sushi",
        }
    }
}

/// The asset a pool pairs against, used to convert its output to USD.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Counter {
    #[serde(rename = "USDC")]
    Usdc,
    #[serde(rename = "XLM")]
    Xlm,
}

impl Counter {
    pub fn code(self) -> &'static str {
        match self {
            Counter::Usdc => "USDC",
            Counter::Xlm => "XLM",
        }
    }
}

/// How a pool's exit is computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolKind {
    /// `x * y = k`, computed from reserves.
    #[default]
    ConstantProduct,
    /// Sampled from the pool's own swap quotes (Aquarius concentrated or stable pools).
    Quoted,
}

/// One pinned pool from the registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSpec {
    /// Code of the registered asset this pool sells.
    pub asset: String,
    pub counter: Counter,
    pub protocol: Protocol,
    /// The pool's contract address (`C...`).
    pub contract: String,
    #[serde(default)]
    pub kind: PoolKind,
}

/// What was read from a pool.
#[derive(Clone, Debug)]
pub enum PoolState {
    ConstantProduct(CpPool),
    Quoted(QuotedPool),
}

impl PoolSpec {
    pub fn validate(&self) -> Result<(), IngestError> {
        soroban::parse_contract(&self.contract)?;
        if self.kind == PoolKind::Quoted && self.protocol == Protocol::Soroswap {
            return Err(IngestError::Registry(format!(
                "pool {}: Soroswap pools are constant-product, not quoted",
                self.contract
            )));
        }
        if self.protocol == Protocol::Sushi && self.kind != PoolKind::Quoted {
            return Err(IngestError::Registry(format!(
                "pool {}: Sushi pools must be pinned with kind = \"quoted\"",
                self.contract
            )));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        format!("{}/{}", self.protocol.name(), self.counter.code())
    }
}

/// Reads one pool's reserves from the side of a seller of `asset`, after checking that the
/// pool really holds `asset` and its counter.
pub fn read_pool(
    rpc: &SorobanRpc,
    spec: &PoolSpec,
    asset: &Asset,
    usdc: &Asset,
) -> Result<CpPool, IngestError> {
    let want_asset = asset.token_address(rpc.passphrase())?;
    let want_counter = match spec.counter {
        Counter::Usdc => soroban::sac_address(Token::Credit(usdc), rpc.passphrase())?,
        Counter::Xlm => soroban::sac_address(Token::Native, rpc.passphrase())?,
    };
    let c = &spec.contract;
    let key = (rpc.passphrase().to_string(), c.clone());
    let cached = meta_cache().lock().ok().and_then(|m| {
        m.get(&key)
            .filter(|(at, _, _)| at.elapsed() < METADATA_TTL)
            .map(|(_, t, f)| (t.clone(), *f))
    });
    let (tokens, fee_bps) = match cached {
        Some(hit) => hit,
        None => read_metadata(rpc, spec)?,
    };
    if fee_bps >= 10_000 {
        return Err(reject(spec, format!("fee of {fee_bps} bps")));
    }
    let reserves = soroban::amounts(&rpc.read(c, "get_reserves", vec![])?)?;
    let pool = pool_from(
        spec,
        &tokens,
        &reserves,
        fee_bps,
        &want_asset,
        &want_counter,
    )?;
    let held_asset = held_balance(rpc, &want_asset, c)?;
    let held_counter = held_balance(rpc, &want_counter, c)?;
    let pool = check_held(spec, pool, held_asset, held_counter)?;
    // Only metadata that passed verification is cached.
    if let Ok(mut m) = meta_cache().lock() {
        m.entry(key)
            .or_insert_with(|| (Instant::now(), tokens, fee_bps));
    }
    Ok(pool)
}

/// The static part of a pool: its tokens and fee, after checking its type.
fn read_metadata(rpc: &SorobanRpc, spec: &PoolSpec) -> Result<(Vec<String>, u32), IngestError> {
    let c = &spec.contract;
    Ok(match spec.protocol {
        Protocol::Aquarius => {
            let kind = soroban::as_symbol(&rpc.read(c, "pool_type", vec![])?)?;
            if kind != "constant_product" {
                return Err(reject(
                    spec,
                    format!("pool type is {kind:?}, not constant_product"),
                ));
            }
            let tokens = soroban::contract_addresses(&rpc.read(c, "get_tokens", vec![])?)?;
            let fee = soroban::as_u32(&rpc.read(c, "get_fee_fraction", vec![])?)?;
            (tokens, fee)
        }
        Protocol::Sushi => {
            return Err(reject(spec, "Sushi pools are read from their quotes"));
        }
        Protocol::Soroswap => {
            let t0 = soroban::contract_address(&rpc.read(c, "token_0", vec![])?)?;
            let t1 = soroban::contract_address(&rpc.read(c, "token_1", vec![])?)?;
            (vec![t0, t1], SOROSWAP_FEE_BPS)
        }
    })
}

/// How much of `token` the token contract itself says `holder` owns. The pool cannot
/// influence this answer.
pub(crate) fn held_balance(rpc: &SorobanRpc, token: &str, holder: &str) -> Result<Fx, IngestError> {
    let id = soroban::parse_contract(holder)?;
    let v = rpc.read(
        token,
        "balance",
        vec![stellar_xdr::ScVal::Address(
            stellar_xdr::ScAddress::Contract(stellar_xdr::ContractId(stellar_xdr::Hash(id))),
        )],
    )?;
    let wrapped = stellar_xdr::ScVal::Vec(Some(
        vec![v]
            .try_into()
            .map_err(|_| IngestError::Registry("balance".into()))?,
    ));
    soroban::amounts(&wrapped)?
        .pop()
        .ok_or_else(|| IngestError::Registry("empty balance".into()))
}

/// Largest gap allowed between what a pool reports and what it holds, in basis points.
/// Pools may hold slightly more than their reserves (accrued fees), never more than a hair
/// less.
pub const HELD_TOLERANCE_BPS: u32 = 10;

/// Rejects a pool whose reported reserves exceed what the token contracts say it holds,
/// and otherwise uses the smaller of the two.
pub fn check_held(
    spec: &PoolSpec,
    pool: CpPool,
    held_asset: Fx,
    held_counter: Fx,
) -> Result<CpPool, IngestError> {
    for (reported, held, side) in [
        (pool.reserve_asset, held_asset, "asset"),
        (pool.reserve_counter, held_counter, "counter"),
    ] {
        let allowed = held
            .checked_add(held.mul_bps_down(HELD_TOLERANCE_BPS)?)?
            .checked_add(Fx::from_raw(1)?)?;
        if reported > allowed {
            return Err(reject(
                spec,
                format!("reports {reported} of the {side} but holds {held}"),
            ));
        }
    }
    Ok(CpPool {
        reserve_asset: pool.reserve_asset.min(held_asset),
        reserve_counter: pool.reserve_counter.min(held_counter),
        fee_bps: pool.fee_bps,
    })
}

/// Pure part of [`read_pool`]: matches token order to reserves and checks identity.
pub fn pool_from(
    spec: &PoolSpec,
    tokens: &[String],
    reserves: &[Fx],
    fee_bps: u32,
    want_asset: &str,
    want_counter: &str,
) -> Result<CpPool, IngestError> {
    if tokens.len() != 2 || reserves.len() != 2 {
        return Err(reject(spec, "expected exactly two tokens and two reserves"));
    }
    let (ia, ic) = if tokens[0] == want_asset && tokens[1] == want_counter {
        (0, 1)
    } else if tokens[1] == want_asset && tokens[0] == want_counter {
        (1, 0)
    } else {
        return Err(reject(
            spec,
            format!("holds {tokens:?}, expected {want_asset} and {want_counter}"),
        ));
    };
    Ok(CpPool {
        reserve_asset: reserves[ia],
        reserve_counter: reserves[ic],
        fee_bps,
    })
}

fn reject(spec: &PoolSpec, why: impl std::fmt::Display) -> IngestError {
    IngestError::BadResponse {
        url: format!("{} {}", spec.label(), spec.contract),
        reason: why.to_string(),
    }
}

/// Reads any pinned pool according to its kind.
pub fn read_pool_state(
    rpc: &SorobanRpc,
    spec: &PoolSpec,
    asset: &Asset,
    usdc: &Asset,
) -> Result<PoolState, IngestError> {
    match spec.kind {
        PoolKind::ConstantProduct => {
            read_pool(rpc, spec, asset, usdc).map(PoolState::ConstantProduct)
        }
        PoolKind::Quoted if spec.protocol == Protocol::Sushi => {
            read_sushi_pool(rpc, spec, asset, usdc).map(PoolState::Quoted)
        }
        PoolKind::Quoted => read_quoted_pool(rpc, spec, asset, usdc).map(PoolState::Quoted),
    }
}

fn u128_val(v: i128) -> Result<stellar_xdr::ScVal, IngestError> {
    let u = u128::try_from(v).map_err(|_| IngestError::Registry("negative amount".into()))?;
    Ok(stellar_xdr::ScVal::U128(stellar_xdr::UInt128Parts {
        hi: (u >> 64) as u64,
        lo: u as u64,
    }))
}

/// What the pool would pay, in counter units, for `base_in` of the asset.
fn quote(
    rpc: &SorobanRpc,
    c: &str,
    in_idx: u32,
    out_idx: u32,
    base_in: Fx,
) -> Result<Fx, IngestError> {
    let v = rpc.read(
        c,
        "estimate_swap",
        vec![
            stellar_xdr::ScVal::U32(in_idx),
            stellar_xdr::ScVal::U32(out_idx),
            u128_val(base_in.raw())?,
        ],
    )?;
    let wrapped = stellar_xdr::ScVal::Vec(Some(
        vec![v]
            .try_into()
            .map_err(|_| IngestError::Registry("quote".into()))?,
    ));
    soroban::amounts(&wrapped)?
        .pop()
        .ok_or_else(|| reject(&dummy_spec(c), "empty quote"))
}

fn dummy_spec(c: &str) -> PoolSpec {
    PoolSpec {
        asset: String::new(),
        counter: Counter::Usdc,
        protocol: Protocol::Aquarius,
        contract: c.to_string(),
        kind: PoolKind::Quoted,
    }
}

/// Samples an Aquarius concentrated or stable pool's own swap quotes for selling `asset`,
/// after checking its type and that it holds exactly `asset` and the counter.
pub fn read_quoted_pool(
    rpc: &SorobanRpc,
    spec: &PoolSpec,
    asset: &Asset,
    usdc: &Asset,
) -> Result<QuotedPool, IngestError> {
    let want_asset = asset.token_address(rpc.passphrase())?;
    let want_counter = match spec.counter {
        Counter::Usdc => soroban::sac_address(Token::Credit(usdc), rpc.passphrase())?,
        Counter::Xlm => soroban::sac_address(Token::Native, rpc.passphrase())?,
    };
    let c = &spec.contract;
    let kind = soroban::as_symbol(&rpc.read(c, "pool_type", vec![])?)?;
    if kind != "concentrated" && kind != "stable" {
        return Err(reject(
            spec,
            format!("pool type is {kind:?}, not concentrated or stable"),
        ));
    }
    let tokens = soroban::contract_addresses(&rpc.read(c, "get_tokens", vec![])?)?;
    if tokens.len() != 2 {
        return Err(reject(spec, "expected exactly two tokens"));
    }
    let (in_idx, out_idx) = if tokens[0] == want_asset && tokens[1] == want_counter {
        (0u32, 1u32)
    } else if tokens[1] == want_asset && tokens[0] == want_counter {
        (1, 0)
    } else {
        return Err(reject(
            spec,
            format!("holds {tokens:?}, expected {want_asset} and {want_counter}"),
        ));
    };
    let reserves = soroban::amounts(&rpc.read(c, "get_reserves", vec![])?)?;
    let counter_reserve = *reserves
        .get(out_idx as usize)
        .ok_or_else(|| reject(spec, "missing reserve"))?;
    if counter_reserve.is_zero() {
        return QuotedPool::new(vec![]).map_err(IngestError::from);
    }

    // Probe with one unit of the asset to learn the pool's price, then size the samples as
    // fractions of the counter reserve converted at that price.
    let one = Fx::ONE;
    let probe_out = quote(rpc, c, in_idx, out_idx, one)?;
    if probe_out.is_zero() {
        return QuotedPool::new(vec![]).map_err(IngestError::from);
    }
    let sizes: Vec<Fx> = QUOTE_FRACTIONS_BPS
        .iter()
        .map(|&f| counter_reserve.mul_bps_down(f)?.div_down(probe_out))
        .collect::<Result<_, _>>()?;
    let outs: Vec<Result<Fx, IngestError>> = std::thread::scope(|s| {
        let handles: Vec<_> = sizes
            .iter()
            .map(|&size| s.spawn(move || quote(rpc, c, in_idx, out_idx, size)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(IngestError::Registry("quote panicked".into())))
            })
            .collect()
    });
    let mut samples = vec![QuoteSample {
        base_in: one,
        counter_out: probe_out,
    }];
    for (size, out) in sizes.into_iter().zip(outs) {
        if size > one {
            samples.push(QuoteSample {
                base_in: size,
                counter_out: out?,
            });
        }
    }
    // A pool cannot pay out more of the counter than the counter contract says it holds.
    let held_counter = held_balance(rpc, &want_counter, c)?;
    if let Some(s) = samples.iter().find(|s| s.counter_out > held_counter) {
        return Err(reject(
            spec,
            format!(
                "quotes {} of the counter but holds {held_counter}",
                s.counter_out
            ),
        ));
    }
    QuotedPool::new(samples).map_err(|e| reject(spec, format!("inconsistent quotes: {e}")))
}

// ------------------------------------------------------------------ Sushi v3

/// Lowest and highest sqrt prices a Uniswap-v3-style swap accepts (MIN + 1, MAX - 1).
const MIN_SQRT_RATIO_PLUS_1: u128 = 4_295_128_740;
const MAX_SQRT_RATIO_MINUS_1: &str = "1461446703485210103287273052203988822378723970341";

fn u256_val(n: &str) -> Result<stellar_xdr::ScVal, IngestError> {
    // Parse a decimal string into four big-endian u64 limbs.
    let mut limbs = [0u64; 4];
    for ch in n.bytes() {
        let d = u64::from(
            ch.checked_sub(b'0')
                .filter(|d| *d <= 9)
                .ok_or_else(|| IngestError::Registry(format!("bad u256 {n}")))?,
        );
        let mut carry = d as u128;
        for limb in limbs.iter_mut().rev() {
            let v = (*limb as u128) * 10 + carry;
            *limb = v as u64;
            carry = v >> 64;
        }
        if carry != 0 {
            return Err(IngestError::Registry(format!("u256 overflow {n}")));
        }
    }
    Ok(stellar_xdr::ScVal::U256(stellar_xdr::UInt256Parts {
        hi_hi: limbs[0],
        hi_lo: limbs[1],
        lo_hi: limbs[2],
        lo_lo: limbs[3],
    }))
}

fn i128_of(v: &stellar_xdr::ScVal) -> Option<i128> {
    match v {
        stellar_xdr::ScVal::I128(p) => Some((i128::from(p.hi) << 64) | i128::from(p.lo)),
        _ => None,
    }
}

fn map_field<'a>(v: &'a stellar_xdr::ScVal, key: &str) -> Option<&'a stellar_xdr::ScVal> {
    let stellar_xdr::ScVal::Map(Some(m)) = v else {
        return None;
    };
    m.iter()
        .find(|e| matches!(&e.key, stellar_xdr::ScVal::Symbol(s) if s.to_string() == key))
        .map(|e| &e.val)
}

/// `10^(decimals - 7)`: how many of the token's smallest units make one [`Fx`] raw unit.
/// Tokens with fewer than 7 decimals are refused rather than rounded.
pub fn unit_scale(decimals: u32) -> Result<i128, IngestError> {
    if !(7..=30).contains(&decimals) {
        return Err(IngestError::Registry(format!(
            "unsupported token decimals {decimals}"
        )));
    }
    Ok(10i128.pow(decimals - 7))
}

/// The counter amount the pool pays out for selling `amount_in` (token units) of the asset.
/// Uniswap-v3 convention: the amount leaving the pool is negative.
pub fn sushi_out(result: &stellar_xdr::ScVal, zero_for_one: bool) -> Result<i128, IngestError> {
    let key = if zero_for_one { "amount1" } else { "amount0" };
    let out = map_field(result, key)
        .and_then(i128_of)
        .ok_or_else(|| IngestError::Registry(format!("quote has no {key}")))?;
    if out > 0 {
        return Err(IngestError::Registry(
            "quote pays a positive amount in".into(),
        ));
    }
    Ok(-out)
}

/// Samples a Sushi v3 pool's own `quote_exact_input` for selling `asset`, after checking it
/// holds exactly `asset` and the counter. Token amounts are converted from the token's own
/// decimals. A sale too large to quote (the simulation runs out of budget) ends the samples
/// there, which can only understate depth.
pub fn read_sushi_pool(
    rpc: &SorobanRpc,
    spec: &PoolSpec,
    asset: &Asset,
    usdc: &Asset,
) -> Result<QuotedPool, IngestError> {
    let want_asset = asset.token_address(rpc.passphrase())?;
    let want_counter = match spec.counter {
        Counter::Usdc => soroban::sac_address(Token::Credit(usdc), rpc.passphrase())?,
        Counter::Xlm => soroban::sac_address(Token::Native, rpc.passphrase())?,
    };
    let c = &spec.contract;
    let t0 = soroban::contract_address(&rpc.read(c, "token0", vec![])?)?;
    let t1 = soroban::contract_address(&rpc.read(c, "token1", vec![])?)?;
    let zero_for_one = if t0 == want_asset && t1 == want_counter {
        true
    } else if t1 == want_asset && t0 == want_counter {
        false
    } else {
        return Err(reject(
            spec,
            format!("holds [{t0}, {t1}], expected {want_asset} and {want_counter}"),
        ));
    };
    let decimals = soroban::as_u32(&rpc.read(&want_asset, "decimals", vec![])?)?;
    let scale = unit_scale(decimals)?;
    let limit = if zero_for_one {
        u256_val(&MIN_SQRT_RATIO_PLUS_1.to_string())?
    } else {
        u256_val(MAX_SQRT_RATIO_MINUS_1)?
    };
    let quote_at = |base_in: Fx| -> Result<Fx, IngestError> {
        let amount_in = base_in
            .raw()
            .checked_mul(scale)
            .ok_or_else(|| reject(spec, "sale size overflows"))?;
        let v = rpc.read(
            c,
            "quote_exact_input",
            vec![
                stellar_xdr::ScVal::Bool(zero_for_one),
                stellar_xdr::ScVal::I128(stellar_xdr::Int128Parts {
                    hi: (amount_in >> 64) as i64,
                    lo: amount_in as u64,
                }),
                limit.clone(),
            ],
        )?;
        Fx::from_raw(sushi_out(&v, zero_for_one)?).map_err(IngestError::from)
    };

    let held_counter = held_balance(rpc, &want_counter, c)?;
    if held_counter.is_zero() {
        return QuotedPool::new(vec![]).map_err(IngestError::from);
    }
    let one = Fx::ONE;
    let probe_out = quote_at(one)?;
    if probe_out.is_zero() {
        return QuotedPool::new(vec![]).map_err(IngestError::from);
    }
    let sizes: Vec<Fx> = QUOTE_FRACTIONS_BPS
        .iter()
        .map(|&f| held_counter.mul_bps_down(f)?.div_down(probe_out))
        .collect::<Result<_, _>>()?;
    let outs: Vec<Result<Fx, IngestError>> = std::thread::scope(|s| {
        let handles: Vec<_> = sizes
            .iter()
            .map(|&size| s.spawn(move || quote_at(size)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(IngestError::Registry("quote panicked".into())))
            })
            .collect()
    });
    let mut samples = vec![QuoteSample {
        base_in: one,
        counter_out: probe_out,
    }];
    for (size, out) in sizes.into_iter().zip(outs) {
        if size <= one {
            continue;
        }
        match out {
            Ok(out) => samples.push(QuoteSample {
                base_in: size,
                counter_out: out,
            }),
            // Too large to quote: stop here rather than guess.
            Err(_) => break,
        }
    }
    if let Some(s) = samples.iter().find(|s| s.counter_out > held_counter) {
        return Err(reject(
            spec,
            format!(
                "quotes {} of the counter but holds {held_counter}",
                s.counter_out
            ),
        ));
    }
    QuotedPool::new(samples).map_err(|e| reject(spec, format!("inconsistent quotes: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sushi_amounts_and_decimals() {
        use stellar_xdr::{Int128Parts, ScMap, ScMapEntry, ScSymbol, ScVal};
        let i = |n: i128| {
            ScVal::I128(Int128Parts {
                hi: (n >> 64) as i64,
                lo: n as u64,
            })
        };
        let entry = |k: &str, v: ScVal| ScMapEntry {
            key: ScVal::Symbol(ScSymbol(k.try_into().unwrap())),
            val: v,
        };
        // Selling 1 deJAAA (token1, 18 decimals) for 1.0494 USDC (token0, 7 decimals).
        let r = ScVal::Map(Some(ScMap(
            vec![
                entry("amount0", i(-10_494_000)),
                entry("amount1", i(10i128.pow(18))),
            ]
            .try_into()
            .unwrap(),
        )));
        assert_eq!(sushi_out(&r, false).unwrap(), 10_494_000);
        assert!(sushi_out(&r, true).is_err(), "amount1 is paid in, not out");
        assert_eq!(unit_scale(18).unwrap(), 10i128.pow(11));
        assert_eq!(unit_scale(7).unwrap(), 1);
        assert!(unit_scale(6).is_err());
        assert!(u256_val(MAX_SQRT_RATIO_MINUS_1).is_ok());
        assert!(u256_val("1").is_ok());
    }

    const USTRY: &str = "CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR";
    const USDC: &str = "CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75";
    const XLM: &str = "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA";

    fn spec() -> PoolSpec {
        PoolSpec {
            asset: "USTRY".into(),
            counter: Counter::Usdc,
            protocol: Protocol::Aquarius,
            contract: "CCX2TYR4AQTPPMTZOIMP3YIBHAYLLSTXVP47PAGTL753PCQFSKV32MIA".into(),
            kind: PoolKind::ConstantProduct,
        }
    }

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    #[test]
    fn maps_reserves_in_either_token_order() {
        let r = [fx("100"), fx("250")];
        let a = pool_from(&spec(), &[USTRY.into(), USDC.into()], &r, 30, USTRY, USDC).unwrap();
        assert_eq!((a.reserve_asset, a.reserve_counter), (fx("100"), fx("250")));
        let b = pool_from(&spec(), &[USDC.into(), USTRY.into()], &r, 30, USTRY, USDC).unwrap();
        assert_eq!((b.reserve_asset, b.reserve_counter), (fx("250"), fx("100")));
    }

    #[test]
    fn a_pool_cannot_claim_more_than_it_holds() {
        let pool = CpPool {
            reserve_asset: fx("1000"),
            reserve_counter: fx("1000"),
            fee_bps: 30,
        };
        // Holds exactly what it reports, or a little more (fees): accepted, reserves kept.
        let ok = check_held(&spec(), pool, fx("1000"), fx("1003")).unwrap();
        assert_eq!(ok.reserve_counter, fx("1000"));
        // Reports a counter reserve it does not hold: rejected outright.
        assert!(check_held(&spec(), pool, fx("1000"), fx("500")).is_err());
        // Within the 0.1% tolerance: accepted but capped at what it holds.
        let capped = check_held(&spec(), pool, fx("999.5"), fx("1000")).unwrap();
        assert_eq!(capped.reserve_asset, fx("999.5"));
    }

    #[test]
    fn rejects_a_pool_holding_the_wrong_tokens() {
        let r = [fx("1"), fx("1")];
        // Right asset, wrong counter (XLM where USDC was pinned).
        assert!(pool_from(&spec(), &[USTRY.into(), XLM.into()], &r, 30, USTRY, USDC).is_err());
        // An impostor token in place of the asset.
        let impostor = "CAL6ER2TI6CTRAY6BFXWNWA7WTYXUXTQCHUBCIBU5O6KM3HJFG6Z6VXV";
        assert!(pool_from(
            &spec(),
            &[impostor.into(), USDC.into()],
            &r,
            30,
            USTRY,
            USDC
        )
        .is_err());
        // Three-token pools are not constant-product pairs.
        let three = [USTRY.into(), USDC.into(), XLM.into()];
        assert!(pool_from(&spec(), &three, &r, 30, USTRY, USDC).is_err());
    }

    #[test]
    fn the_shipped_registry_pins_valid_pools() {
        let reg =
            crate::registry::Registry::from_toml(include_str!("../../../assets.toml")).unwrap();
        assert!(!reg.pools.is_empty());
        for p in &reg.pools {
            p.validate().unwrap();
            assert!(reg.assets.iter().any(|a| a.code == p.asset), "{}", p.asset);
        }
    }

    /// Live, read-only: every pinned pool passes on-chain verification. `--ignored` to run.
    #[test]
    #[ignore]
    fn live_every_pinned_pool_verifies() {
        let reg =
            crate::registry::Registry::from_toml(include_str!("../../../assets.toml")).unwrap();
        let rpc = SorobanRpc::mainnet(soroban::DEFAULT_MAINNET_RPC);
        rpc.check_network().unwrap();
        for p in &reg.pools {
            let asset = reg.assets.iter().find(|a| a.code == p.asset).unwrap();
            match read_pool_state(&rpc, p, asset, &reg.usdc)
                .unwrap_or_else(|e| panic!("{} {}: {e}", p.asset, p.contract))
            {
                PoolState::ConstantProduct(pool) => {
                    assert!(!pool.reserve_asset.is_zero(), "{}", p.contract)
                }
                PoolState::Quoted(q) => assert!(q.samples.len() > 1, "{}", p.contract),
            }
        }
    }
}
