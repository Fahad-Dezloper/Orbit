//! Soroban AMM pools, read on-chain and verified before they are counted.
//!
//! Pools are pinned in `assets.toml`. A pinned address is never trusted on its own: before a
//! pool's reserves are used, its token list must equal the locally derived SAC addresses of
//! the asset and the counter. A pool holding an impostor token with the same code is
//! rejected, and so is any pool whose type is not constant-product.
//!
//! A pool that cannot be read or verified is left out and noted. Leaving a venue out can
//! only understate depth.

use pocky_core::{CpPool, Fx};
use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::registry::Asset;
use crate::soroban::{self, SorobanRpc, Token};

/// Soroswap pairs charge a fixed 0.3% on the input (the pair contract hard-codes 997/1000).
pub const SOROSWAP_FEE_BPS: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// Aquarius standard (constant-product) pool.
    Aquarius,
    /// Soroswap pair (constant-product, Uniswap v2 style).
    Soroswap,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Aquarius => "aquarius",
            Protocol::Soroswap => "soroswap",
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

/// One pinned pool from the registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSpec {
    /// Code of the registered asset this pool sells.
    pub asset: String,
    pub counter: Counter,
    pub protocol: Protocol,
    /// The pool's contract address (`C...`).
    pub contract: String,
}

impl PoolSpec {
    pub fn validate(&self) -> Result<(), IngestError> {
        soroban::parse_contract(&self.contract).map(|_| ())
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
    let want_asset = soroban::sac_address(Token::Credit(asset), rpc.passphrase())?;
    let want_counter = match spec.counter {
        Counter::Usdc => soroban::sac_address(Token::Credit(usdc), rpc.passphrase())?,
        Counter::Xlm => soroban::sac_address(Token::Native, rpc.passphrase())?,
    };
    let c = &spec.contract;
    let (tokens, fee_bps) = match spec.protocol {
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
        Protocol::Soroswap => {
            let t0 = soroban::contract_address(&rpc.read(c, "token_0", vec![])?)?;
            let t1 = soroban::contract_address(&rpc.read(c, "token_1", vec![])?)?;
            (vec![t0, t1], SOROSWAP_FEE_BPS)
        }
    };
    if fee_bps >= 10_000 {
        return Err(reject(spec, format!("fee of {fee_bps} bps")));
    }
    let reserves = soroban::amounts(&rpc.read(c, "get_reserves", vec![])?)?;
    pool_from(
        spec,
        &tokens,
        &reserves,
        fee_bps,
        &want_asset,
        &want_counter,
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    const USTRY: &str = "CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR";
    const USDC: &str = "CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75";
    const XLM: &str = "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA";

    fn spec() -> PoolSpec {
        PoolSpec {
            asset: "USTRY".into(),
            counter: Counter::Usdc,
            protocol: Protocol::Aquarius,
            contract: "CCX2TYR4AQTPPMTZOIMP3YIBHAYLLSTXVP47PAGTL753PCQFSKV32MIA".into(),
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
            let pool = read_pool(&rpc, p, asset, &reg.usdc)
                .unwrap_or_else(|e| panic!("{} {}: {e}", p.asset, p.contract));
            assert!(!pool.reserve_asset.is_zero(), "{}", p.contract);
        }
    }
}
