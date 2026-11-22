//! # Pocky depth oracle
//!
//! Publishes, per asset and slippage band, the USD that could be raised selling the asset
//! on Stellar before the average price drops more than the band. Figures are produced
//! off-chain by `pocky-core` (windowed minimum over 24h) and written here by a single
//! publisher key.
//!
//! The read interface follows SEP-40 conventions (`base`, `decimals`, `assets`,
//! `last_timestamp`, an `Asset` enum, records carrying their timestamp) so a lending
//! protocol can read depth the same way it reads price.
//!
//! ## Safety rules enforced here
//! - Only the publisher can write; only the admin can rotate the publisher, and the admin
//!   role moves only with the signatures of both the current and the new admin.
//! - The asset list is capped, so a compromised publisher cannot grow instance storage
//!   until the contract can no longer load.
//! - Timestamps must strictly increase per (asset, band), may not be in the future
//!   beyond a small skew, and may not be older than `MAX_AGE_SECS` at publish time.
//! - Bands must be 1..=9999 bps; values must be non-negative and below a sanity cap.
//! - A batch is all-or-nothing.
//! - Records carry their timestamp so readers can reject stale data themselves.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Env, Symbol, Vec,
};

/// Fixed-point decimals of `exit_usd` (same as Stellar amounts).
pub const DECIMALS: u32 = 7;
/// Largest accepted figure: $1 trillion, scaled. Anything above is a publisher bug.
pub const MAX_EXIT_USD: i128 = 1_000_000_000_000 * 10_000_000;
/// A publish may claim a measurement time at most this far in the future (clock skew).
pub const MAX_FUTURE_SKEW_SECS: u64 = 60;
/// A publish may not carry a measurement older than this.
pub const MAX_AGE_SECS: u64 = 6 * 3600;
/// Records kept per (asset, band).
pub const HISTORY_LEN: u32 = 48;
/// Most updates accepted in one call, to bound cost.
pub const MAX_BATCH: u32 = 64;
/// Most distinct assets the oracle will track. The list lives in instance storage, which
/// is loaded on every call, so it must stay small.
pub const MAX_ASSETS: u32 = 200;

const DAY_LEDGERS: u32 = 17_280; // ~5s ledgers
const TTL_THRESHOLD: u32 = 7 * DAY_LEDGERS;
const TTL_EXTEND_TO: u32 = 30 * DAY_LEDGERS;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    BadBand = 1,
    BadValue = 2,
    NotMonotonic = 3,
    FutureTimestamp = 4,
    StaleTimestamp = 5,
    BatchTooLarge = 6,
    EmptyBatch = 7,
    NotInitialized = 8,
    TooManyAssets = 9,
}

/// SEP-40 style asset identifier.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Asset {
    /// A Stellar asset contract (SAC) or Soroban token address.
    Stellar(Address),
    /// Anything else, by symbol.
    Other(Symbol),
}

/// One published depth figure.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthRecord {
    /// USD raised at this band, scaled by 10^7.
    pub exit_usd: i128,
    /// Unix seconds of the end of the measurement window.
    pub timestamp: u64,
    /// Number of snapshots the windowed minimum was taken over.
    pub samples: u32,
}

/// One entry in a `publish` batch.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthUpdate {
    pub asset: Asset,
    pub band_bps: u32,
    pub exit_usd: i128,
    pub timestamp: u64,
    pub samples: u32,
}

#[contracttype]
#[derive(Clone)]
enum Key {
    Admin,
    Publisher,
    Assets,
    LastTimestamp,
    History(Asset, u32),
}

#[contractevent(topics = ["depth"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthPublished {
    #[topic]
    pub asset: Asset,
    #[topic]
    pub band_bps: u32,
    pub exit_usd: i128,
    pub timestamp: u64,
}

#[contractevent(topics = ["admin"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminChanged {
    pub admin: Address,
}

#[contractevent(topics = ["publisher"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublisherChanged {
    pub publisher: Address,
}

#[contract]
pub struct DepthOracle;

#[contractimpl]
impl DepthOracle {
    pub fn __constructor(env: Env, admin: Address, publisher: Address) {
        let s = env.storage().instance();
        s.set(&Key::Admin, &admin);
        s.set(&Key::Publisher, &publisher);
        s.set(&Key::Assets, &Vec::<Asset>::new(&env));
        s.set(&Key::LastTimestamp, &0u64);
        bump_instance(&env);
    }

    // ---------------------------------------------------------------- admin

    /// Rotates the publisher key. Admin only.
    pub fn set_publisher(env: Env, publisher: Address) {
        read_admin(&env).require_auth();
        env.storage().instance().set(&Key::Publisher, &publisher);
        bump_instance(&env);
        PublisherChanged { publisher }.publish(&env);
    }

    /// Hands the admin role to `new_admin`. Needs the current admin's signature and the new
    /// admin's, so the role cannot be sent to a mistyped or unowned address.
    pub fn set_admin(env: Env, new_admin: Address) {
        read_admin(&env).require_auth();
        new_admin.require_auth();
        env.storage().instance().set(&Key::Admin, &new_admin);
        bump_instance(&env);
        AdminChanged { admin: new_admin }.publish(&env);
    }

    pub fn admin(env: Env) -> Address {
        read_admin(&env)
    }

    pub fn publisher(env: Env) -> Address {
        read_publisher(&env)
    }

    // -------------------------------------------------------------- publish

    /// Writes a batch of depth figures. Publisher only. The whole batch is rejected if any
    /// entry is invalid.
    pub fn publish(env: Env, updates: Vec<DepthUpdate>) {
        read_publisher(&env).require_auth();
        if updates.is_empty() {
            panic_with_error!(&env, Error::EmptyBatch);
        }
        if updates.len() > MAX_BATCH {
            panic_with_error!(&env, Error::BatchTooLarge);
        }
        let now = env.ledger().timestamp();
        let mut assets: Vec<Asset> = env
            .storage()
            .instance()
            .get(&Key::Assets)
            .unwrap_or(Vec::new(&env));
        let mut latest: u64 = env
            .storage()
            .instance()
            .get(&Key::LastTimestamp)
            .unwrap_or(0);

        for u in updates.iter() {
            validate(&env, &u, now);
            let key = Key::History(u.asset.clone(), u.band_bps);
            let store = env.storage().persistent();
            let mut hist: Vec<DepthRecord> = store.get(&key).unwrap_or(Vec::new(&env));
            if let Some(last) = hist.last() {
                if u.timestamp <= last.timestamp {
                    panic_with_error!(&env, Error::NotMonotonic);
                }
            }
            hist.push_back(DepthRecord {
                exit_usd: u.exit_usd,
                timestamp: u.timestamp,
                samples: u.samples,
            });
            while hist.len() > HISTORY_LEN {
                hist.pop_front();
            }
            store.set(&key, &hist);
            store.extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);

            if !assets.contains(&u.asset) {
                if assets.len() >= MAX_ASSETS {
                    panic_with_error!(&env, Error::TooManyAssets);
                }
                assets.push_back(u.asset.clone());
            }
            if u.timestamp > latest {
                latest = u.timestamp;
            }
            DepthPublished {
                asset: u.asset,
                band_bps: u.band_bps,
                exit_usd: u.exit_usd,
                timestamp: u.timestamp,
            }
            .publish(&env);
        }
        env.storage().instance().set(&Key::Assets, &assets);
        env.storage().instance().set(&Key::LastTimestamp, &latest);
        bump_instance(&env);
    }

    // ---------------------------------------------------------------- reads

    /// Base currency of every figure (SEP-40 `base`).
    pub fn base(env: Env) -> Asset {
        Asset::Other(Symbol::new(&env, "USD"))
    }

    /// Decimals of `exit_usd` (SEP-40 `decimals`).
    pub fn decimals(_env: Env) -> u32 {
        DECIMALS
    }

    /// Every asset that has ever been published (SEP-40 `assets`).
    pub fn assets(env: Env) -> Vec<Asset> {
        env.storage()
            .instance()
            .get(&Key::Assets)
            .unwrap_or(Vec::new(&env))
    }

    /// Timestamp of the most recent figure across all assets (SEP-40 `last_timestamp`).
    pub fn last_timestamp(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&Key::LastTimestamp)
            .unwrap_or(0)
    }

    /// Most recent depth for an asset at a band, if any (the depth analogue of `lastprice`).
    /// Readers must check `timestamp` against their own freshness requirement.
    pub fn depth(env: Env, asset: Asset, band_bps: u32) -> Option<DepthRecord> {
        let hist: Vec<DepthRecord> = env
            .storage()
            .persistent()
            .get(&Key::History(asset, band_bps))?;
        hist.last()
    }

    /// Up to `records` most recent figures, newest first (the analogue of SEP-40 `prices`).
    pub fn depths(env: Env, asset: Asset, band_bps: u32, records: u32) -> Option<Vec<DepthRecord>> {
        let hist: Vec<DepthRecord> = env
            .storage()
            .persistent()
            .get(&Key::History(asset, band_bps))?;
        let mut out = Vec::new(&env);
        let n = hist.len();
        let take = if records < n { records } else { n };
        for i in 0..take {
            if let Some(r) = hist.get(n - 1 - i) {
                out.push_back(r);
            }
        }
        Some(out)
    }

    /// Convenience for lenders: the depth if it is no older than `max_age_secs`, else None.
    pub fn fresh_depth(
        env: Env,
        asset: Asset,
        band_bps: u32,
        max_age_secs: u64,
    ) -> Option<DepthRecord> {
        let r = Self::depth(env.clone(), asset, band_bps)?;
        let now = env.ledger().timestamp();
        if now.saturating_sub(r.timestamp) <= max_age_secs {
            Some(r)
        } else {
            None
        }
    }
}

fn read_admin(env: &Env) -> Address {
    match env.storage().instance().get(&Key::Admin) {
        Some(a) => a,
        None => panic_with_error!(env, Error::NotInitialized),
    }
}

fn read_publisher(env: &Env) -> Address {
    match env.storage().instance().get(&Key::Publisher) {
        Some(a) => a,
        None => panic_with_error!(env, Error::NotInitialized),
    }
}

fn validate(env: &Env, u: &DepthUpdate, now: u64) {
    if u.band_bps == 0 || u.band_bps >= 10_000 {
        panic_with_error!(env, Error::BadBand);
    }
    if u.exit_usd < 0 || u.exit_usd > MAX_EXIT_USD {
        panic_with_error!(env, Error::BadValue);
    }
    if u.timestamp > now.saturating_add(MAX_FUTURE_SKEW_SECS) {
        panic_with_error!(env, Error::FutureTimestamp);
    }
    if now.saturating_sub(u.timestamp) > MAX_AGE_SECS {
        panic_with_error!(env, Error::StaleTimestamp);
    }
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
}

#[cfg(test)]
mod test;
