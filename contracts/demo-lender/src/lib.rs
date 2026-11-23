//! # Demo lender: collateral capped by market depth
//!
//! A deliberately small lending pool that shows how a Soroban lender would use the Orbit
//! depth oracle. It only tracks collateral; there is no borrowing or interest. **Demo code
//! for a local network, not a lending protocol.**
//!
//! The one rule it enforces is the one missing from the pool that lost $10.97M in February
//! 2026: before accepting collateral in an asset, check that the total collateral in that
//! asset could actually be sold within the liquidation band.
//!
//! ```text
//! total collateral in asset + new deposit  <=  fresh depth at band  ×  (1 − safety)
//! ```
//!
//! If the oracle has no figure for the asset, or the figure is older than `max_age_secs`, the
//! deposit is refused: no depth data means no collateral.
//!
//! Collateral value is passed in USD to keep the demo focused on the depth check. A real pool
//! would value it with its price oracle.

#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype,
    panic_with_error, Address, Env, Symbol,
};

/// Same encoding as the depth oracle's `Asset`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OracleAsset {
    Stellar(Address),
    Other(Symbol),
}

/// Same encoding as the depth oracle's `DepthRecord`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthRecord {
    pub exit_usd: i128,
    pub timestamp: u64,
    pub samples: u32,
}

/// The part of the depth oracle this pool reads.
#[allow(dead_code)]
#[contractclient(name = "DepthOracleClient")]
pub trait DepthOracle {
    fn fresh_depth(
        env: Env,
        asset: OracleAsset,
        band_bps: u32,
        max_age_secs: u64,
    ) -> Option<DepthRecord>;
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The oracle has no figure for this asset, or it is older than `max_age_secs`.
    NoFreshDepth = 1,
    /// Accepting this deposit would put more collateral in the asset than the market can
    /// absorb within the band.
    DepthCapExceeded = 2,
    BadAmount = 3,
    BadConfig = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub oracle: Address,
    /// Band the oracle is read at, e.g. 1000 = collateral must be sellable within 10%.
    pub band_bps: u32,
    /// Share of the measured depth that is never lent against, e.g. 5000 = 50%.
    pub safety_bps: u32,
    /// Oldest depth figure accepted, in seconds.
    pub max_age_secs: u64,
}

#[contracttype]
#[derive(Clone)]
enum Key {
    Config,
    Total(Address),
    Position(Address, Address),
}

#[contractevent(topics = ["deposit"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposited {
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Address,
    pub value_usd: i128,
    pub total_usd: i128,
    pub cap_usd: i128,
}

#[contract]
pub struct DemoLender;

#[contractimpl]
impl DemoLender {
    pub fn __constructor(
        env: Env,
        oracle: Address,
        band_bps: u32,
        safety_bps: u32,
        max_age_secs: u64,
    ) {
        if band_bps == 0 || band_bps >= 10_000 || safety_bps >= 10_000 || max_age_secs == 0 {
            panic_with_error!(&env, Error::BadConfig);
        }
        env.storage().instance().set(
            &Key::Config,
            &Config {
                oracle,
                band_bps,
                safety_bps,
                max_age_secs,
            },
        );
    }

    pub fn config(env: Env) -> Config {
        read_config(&env)
    }

    /// Most collateral (USD, 7 decimals) the pool will hold in `asset` right now, or `None`
    /// if the oracle has no fresh figure.
    pub fn cap(env: Env, asset: Address) -> Option<i128> {
        let cfg = read_config(&env);
        let oracle = DepthOracleClient::new(&env, &cfg.oracle);
        let rec = oracle.fresh_depth(
            &OracleAsset::Stellar(asset),
            &cfg.band_bps,
            &cfg.max_age_secs,
        )?;
        Some(rec.exit_usd / 10_000 * i128::from(10_000 - cfg.safety_bps))
    }

    /// Total collateral held in `asset` (USD, 7 decimals).
    pub fn collateral(env: Env, asset: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&Key::Total(asset))
            .unwrap_or(0)
    }

    pub fn position(env: Env, user: Address, asset: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&Key::Position(user, asset))
            .unwrap_or(0)
    }

    /// Deposits collateral worth `value_usd` (7 decimals) in `asset`, if the market for that
    /// asset is deep enough to liquidate the pool's whole position within the band.
    pub fn deposit(env: Env, user: Address, asset: Address, value_usd: i128) {
        user.require_auth();
        if value_usd <= 0 {
            panic_with_error!(&env, Error::BadAmount);
        }
        let cap = match Self::cap(env.clone(), asset.clone()) {
            Some(c) => c,
            None => panic_with_error!(&env, Error::NoFreshDepth),
        };
        let total = Self::collateral(env.clone(), asset.clone());
        let new_total = match total.checked_add(value_usd) {
            Some(t) => t,
            None => panic_with_error!(&env, Error::BadAmount),
        };
        if new_total > cap {
            panic_with_error!(&env, Error::DepthCapExceeded);
        }
        let store = env.storage().persistent();
        let pos = Self::position(env.clone(), user.clone(), asset.clone());
        store.set(&Key::Total(asset.clone()), &new_total);
        store.set(
            &Key::Position(user.clone(), asset.clone()),
            &(pos + value_usd),
        );
        Deposited {
            user,
            asset,
            value_usd,
            total_usd: new_total,
            cap_usd: cap,
        }
        .publish(&env);
    }
}

fn read_config(env: &Env) -> Config {
    match env.storage().instance().get(&Key::Config) {
        Some(c) => c,
        None => panic_with_error!(env, Error::BadConfig),
    }
}

#[cfg(test)]
mod test;
