#![cfg(test)]
extern crate std;

use orbit_depth_oracle::{Asset, DepthOracle, DepthOracleClient as OracleClient, DepthUpdate};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{vec, Address, Env};

use crate::{DemoLender, DemoLenderClient, Error};

const NOW: u64 = 1_800_000_000;
const TEN_PCT: u32 = 1_000;

fn usd(n: i128) -> i128 {
    n * 10_000_000
}

struct World {
    env: Env,
    oracle: OracleClient<'static>,
    pool: DemoLenderClient<'static>,
    deep: Address,
    dead: Address,
    unknown: Address,
}

/// An oracle holding live-style figures (USTRY-like deep asset, KTB-like dead one) and a
/// pool reading it at 10% with a 50% safety margin and a 1h freshness limit.
fn world() -> World {
    let env = Env::default();
    env.ledger().with_mut(|l| l.timestamp = NOW);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let publisher = Address::generate(&env);
    let oracle_id = env.register(DepthOracle, (&admin, &publisher));
    let oracle = OracleClient::new(&env, &oracle_id);
    let deep = Address::generate(&env);
    let dead = Address::generate(&env);
    let unknown = Address::generate(&env);
    let upd = |a: &Address, v: i128| DepthUpdate {
        asset: Asset::Stellar(a.clone()),
        band_bps: TEN_PCT,
        exit_usd: v,
        timestamp: NOW - 60,
        samples: 48,
    };
    oracle.publish(&vec![&env, upd(&deep, usd(371_837)), upd(&dead, 0)]);
    let pool_id = env.register(DemoLender, (&oracle_id, TEN_PCT, 5_000u32, 3_600u64));
    let pool = DemoLenderClient::new(&env, &pool_id);
    World {
        env,
        oracle,
        pool,
        deep,
        dead,
        unknown,
    }
}

#[test]
fn accepts_collateral_the_market_can_absorb() {
    let w = world();
    let user = Address::generate(&w.env);
    // Cap = $371,837 x 50% = $185,918.5
    assert_eq!(w.pool.cap(&w.deep), Some(usd(371_837) / 2));
    w.pool.deposit(&user, &w.deep, &usd(150_000));
    assert_eq!(w.pool.collateral(&w.deep), usd(150_000));
    assert_eq!(w.pool.position(&user, &w.deep), usd(150_000));
}

#[test]
fn refuses_collateral_beyond_what_can_be_liquidated() {
    let w = world();
    let user = Address::generate(&w.env);
    w.pool.deposit(&user, &w.deep, &usd(150_000));
    // A second depositor would push the pool past the cap.
    let other = Address::generate(&w.env);
    let r = w.pool.try_deposit(&other, &w.deep, &usd(50_000));
    assert_eq!(r.err().unwrap().unwrap(), Error::DepthCapExceeded.into());
    assert_eq!(w.pool.collateral(&w.deep), usd(150_000));
}

#[test]
fn a_dead_market_takes_no_collateral_at_all() {
    let w = world();
    let user = Address::generate(&w.env);
    // Any price feed may still quote this token; there is nowhere to sell it.
    let r = w.pool.try_deposit(&user, &w.dead, &usd(1));
    assert_eq!(r.err().unwrap().unwrap(), Error::DepthCapExceeded.into());
}

#[test]
fn no_depth_data_means_no_collateral() {
    let w = world();
    let user = Address::generate(&w.env);
    let r = w.pool.try_deposit(&user, &w.unknown, &usd(1));
    assert_eq!(r.err().unwrap().unwrap(), Error::NoFreshDepth.into());
}

#[test]
fn stale_depth_is_refused() {
    let w = world();
    let user = Address::generate(&w.env);
    w.env.ledger().with_mut(|l| l.timestamp = NOW + 2 * 3600);
    assert_eq!(w.pool.cap(&w.deep), None);
    let r = w.pool.try_deposit(&user, &w.deep, &usd(1));
    assert_eq!(r.err().unwrap().unwrap(), Error::NoFreshDepth.into());
}

#[test]
fn a_thinner_market_shrinks_the_cap_for_new_deposits() {
    let w = world();
    let user = Address::generate(&w.env);
    w.pool.deposit(&user, &w.deep, &usd(100_000));
    // Next window: depth fell to $150K, so the cap is now $75K, below what is already held.
    w.oracle.publish(&vec![
        &w.env,
        DepthUpdate {
            asset: Asset::Stellar(w.deep.clone()),
            band_bps: TEN_PCT,
            exit_usd: usd(150_000),
            timestamp: NOW - 30,
            samples: 48,
        },
    ]);
    let r = w.pool.try_deposit(&user, &w.deep, &usd(1));
    assert_eq!(r.err().unwrap().unwrap(), Error::DepthCapExceeded.into());
}

#[test]
fn rejects_bad_amounts() {
    let w = world();
    let user = Address::generate(&w.env);
    assert_eq!(
        w.pool
            .try_deposit(&user, &w.deep, &0)
            .err()
            .unwrap()
            .unwrap(),
        Error::BadAmount.into()
    );
}

#[test]
#[should_panic]
fn rejects_a_zero_band() {
    let env = Env::default();
    let oracle = Address::generate(&env);
    env.register(DemoLender, (&oracle, 0u32, 5_000u32, 3_600u64));
}
