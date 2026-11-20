#![cfg(test)]
extern crate std;

use soroban_sdk::testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::{vec, Address, Env, IntoVal, Symbol, Vec};

use crate::{Asset, DepthOracle, DepthOracleClient, DepthRecord, DepthUpdate, Error, HISTORY_LEN};

const NOW: u64 = 1_800_000_000;
const TEN_PCT: u32 = 1_000;

struct Setup {
    env: Env,
    client: DepthOracleClient<'static>,
    admin: Address,
    publisher: Address,
    asset: Asset,
}

fn setup() -> Setup {
    let env = Env::default();
    env.ledger().with_mut(|l| l.timestamp = NOW);
    let admin = Address::generate(&env);
    let publisher = Address::generate(&env);
    let id = env.register(DepthOracle, (&admin, &publisher));
    let client = DepthOracleClient::new(&env, &id);
    let asset = Asset::Other(Symbol::new(&env, "USTRY"));
    Setup {
        env,
        client,
        admin,
        publisher,
        asset,
    }
}

fn update(asset: &Asset, exit_usd: i128, ts: u64) -> DepthUpdate {
    DepthUpdate {
        asset: asset.clone(),
        band_bps: TEN_PCT,
        exit_usd,
        timestamp: ts,
        samples: 24,
    }
}

fn usd(n: i128) -> i128 {
    n * 10_000_000
}

#[test]
fn publisher_can_publish_and_anyone_can_read() {
    let s = setup();
    s.env.mock_all_auths();
    s.client
        .publish(&vec![&s.env, update(&s.asset, usd(271_086), NOW - 10)]);

    let r = s.client.depth(&s.asset, &TEN_PCT).unwrap();
    assert_eq!(
        r,
        DepthRecord {
            exit_usd: usd(271_086),
            timestamp: NOW - 10,
            samples: 24
        }
    );
    assert_eq!(s.client.assets(), vec![&s.env, s.asset.clone()]);
    assert_eq!(s.client.last_timestamp(), NOW - 10);
    assert_eq!(s.client.decimals(), 7);
    assert_eq!(s.client.base(), Asset::Other(Symbol::new(&s.env, "USD")));
    assert_eq!(s.client.depth(&s.asset, &200), None);
}

#[test]
fn publish_requires_the_publisher_signature() {
    let s = setup();
    let updates = vec![&s.env, update(&s.asset, usd(1), NOW)];
    // Authorize as a random address: must fail.
    let stranger = Address::generate(&s.env);
    let res = s
        .client
        .mock_auths(&[MockAuth {
            address: &stranger,
            invoke: &MockAuthInvoke {
                contract: &s.client.address,
                fn_name: "publish",
                args: (updates.clone(),).into_val(&s.env),
                sub_invokes: &[],
            },
        }])
        .try_publish(&updates);
    assert!(res.is_err());

    // Authorize as the real publisher: succeeds.
    s.client
        .mock_auths(&[MockAuth {
            address: &s.publisher,
            invoke: &MockAuthInvoke {
                contract: &s.client.address,
                fn_name: "publish",
                args: (updates.clone(),).into_val(&s.env),
                sub_invokes: &[],
            },
        }])
        .publish(&updates);
    assert!(s.client.depth(&s.asset, &TEN_PCT).is_some());
}

#[test]
fn only_admin_rotates_publisher() {
    let s = setup();
    let new_pub = Address::generate(&s.env);
    let res = s
        .client
        .mock_auths(&[MockAuth {
            address: &s.publisher,
            invoke: &MockAuthInvoke {
                contract: &s.client.address,
                fn_name: "set_publisher",
                args: (new_pub.clone(),).into_val(&s.env),
                sub_invokes: &[],
            },
        }])
        .try_set_publisher(&new_pub);
    assert!(res.is_err(), "publisher must not be able to rotate itself");

    s.client
        .mock_auths(&[MockAuth {
            address: &s.admin,
            invoke: &MockAuthInvoke {
                contract: &s.client.address,
                fn_name: "set_publisher",
                args: (new_pub.clone(),).into_val(&s.env),
                sub_invokes: &[],
            },
        }])
        .set_publisher(&new_pub);
    assert_eq!(s.client.publisher(), new_pub);
}

#[test]
fn rejects_replayed_or_older_timestamps() {
    let s = setup();
    s.env.mock_all_auths();
    s.client
        .publish(&vec![&s.env, update(&s.asset, usd(5), NOW)]);
    let same = s
        .client
        .try_publish(&vec![&s.env, update(&s.asset, usd(9_999_999), NOW)]);
    assert_eq!(same.err().unwrap().unwrap(), Error::NotMonotonic.into());
    let older = s
        .client
        .try_publish(&vec![&s.env, update(&s.asset, usd(9_999_999), NOW - 1)]);
    assert_eq!(older.err().unwrap().unwrap(), Error::NotMonotonic.into());
    // The stored figure is unchanged.
    assert_eq!(s.client.depth(&s.asset, &TEN_PCT).unwrap().exit_usd, usd(5));
}

#[test]
fn rejects_future_and_stale_measurements() {
    let s = setup();
    s.env.mock_all_auths();
    let fut = s
        .client
        .try_publish(&vec![&s.env, update(&s.asset, usd(1), NOW + 3600)]);
    assert_eq!(fut.err().unwrap().unwrap(), Error::FutureTimestamp.into());
    let stale = s
        .client
        .try_publish(&vec![&s.env, update(&s.asset, usd(1), NOW - 7 * 3600)]);
    assert_eq!(stale.err().unwrap().unwrap(), Error::StaleTimestamp.into());
}

#[test]
fn rejects_bad_bands_values_and_batches() {
    let s = setup();
    s.env.mock_all_auths();
    let mut bad = update(&s.asset, usd(1), NOW);
    bad.band_bps = 0;
    assert_eq!(
        s.client
            .try_publish(&vec![&s.env, bad.clone()])
            .err()
            .unwrap()
            .unwrap(),
        Error::BadBand.into()
    );
    bad.band_bps = 10_000;
    assert_eq!(
        s.client
            .try_publish(&vec![&s.env, bad])
            .err()
            .unwrap()
            .unwrap(),
        Error::BadBand.into()
    );
    let neg = update(&s.asset, -1, NOW);
    assert_eq!(
        s.client
            .try_publish(&vec![&s.env, neg])
            .err()
            .unwrap()
            .unwrap(),
        Error::BadValue.into()
    );
    let huge = update(&s.asset, i128::MAX, NOW);
    assert_eq!(
        s.client
            .try_publish(&vec![&s.env, huge])
            .err()
            .unwrap()
            .unwrap(),
        Error::BadValue.into()
    );
    let empty: Vec<DepthUpdate> = Vec::new(&s.env);
    assert_eq!(
        s.client.try_publish(&empty).err().unwrap().unwrap(),
        Error::EmptyBatch.into()
    );
}

#[test]
fn one_bad_entry_rejects_the_whole_batch() {
    let s = setup();
    s.env.mock_all_auths();
    let other = Asset::Other(Symbol::new(&s.env, "CETES"));
    let batch = vec![
        &s.env,
        update(&other, usd(10), NOW),
        update(&s.asset, -5, NOW),
    ];
    assert!(s.client.try_publish(&batch).is_err());
    assert_eq!(s.client.depth(&other, &TEN_PCT), None);
    assert_eq!(s.client.assets().len(), 0);
}

#[test]
fn history_is_bounded_and_newest_first() {
    let s = setup();
    s.env.mock_all_auths();
    let total = HISTORY_LEN + 5;
    for i in 0..total {
        let ts = NOW - 3600 + u64::from(i);
        s.client
            .publish(&vec![&s.env, update(&s.asset, usd(i128::from(i)), ts)]);
    }
    let all = s.client.depths(&s.asset, &TEN_PCT, &1000).unwrap();
    assert_eq!(all.len(), HISTORY_LEN);
    assert_eq!(all.get(0).unwrap().exit_usd, usd(i128::from(total - 1)));
    let two = s.client.depths(&s.asset, &TEN_PCT, &2).unwrap();
    assert_eq!(two.len(), 2);
}

#[test]
fn fresh_depth_respects_reader_max_age() {
    let s = setup();
    s.env.mock_all_auths();
    s.client
        .publish(&vec![&s.env, update(&s.asset, usd(100), NOW - 1800)]);
    assert!(s.client.fresh_depth(&s.asset, &TEN_PCT, &3600).is_some());
    assert!(s.client.fresh_depth(&s.asset, &TEN_PCT, &600).is_none());
}
