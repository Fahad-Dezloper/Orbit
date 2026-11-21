//! # pocky-publisher
//!
//! Turns stored measurements into `publish` batches for the depth oracle contract.
//!
//! The planning step is pure and tested: for each asset and band it takes the windowed
//! minimum over the stored runs, and keeps an update only if
//! - the window has enough coverage (fail closed: no window, no update),
//! - its timestamp is newer than what the oracle already holds (the contract would reject a
//!   replay and fail the whole batch), and
//! - it is young enough for the contract's 6h staleness limit.
//!
//! Assets are identified on-chain by their **mainnet** Stellar Asset Contract address, which
//! is what a lending protocol already uses for the same asset.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod chain;

use pocky_core::{windowed_min, Band, WindowPolicy};
use pocky_ingest::run::Run;
use serde::Serialize;

/// Must match `MAX_AGE_SECS` in the contract.
pub const CONTRACT_MAX_AGE_SECS: u64 = 6 * 3600;

/// The asset as the oracle names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleAsset {
    pub code: String,
    /// Mainnet SAC address (`C...`).
    pub sac: String,
}

/// One entry of a `publish` batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub asset: OracleAsset,
    pub band: Band,
    /// USD scaled by 10^7, as the contract stores it.
    pub exit_usd: i128,
    /// End of the measurement window.
    pub timestamp: u64,
    pub samples: u32,
}

/// Why an (asset, band) was not published this round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    pub code: String,
    pub band: Band,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub updates: Vec<Update>,
    pub skipped: Vec<Skip>,
}

/// Builds the updates to publish. `onchain_ts(sac, band)` returns the timestamp of the
/// record the oracle already holds, if any.
pub fn plan(
    runs: &[Run],
    assets: &[OracleAsset],
    now: u64,
    policy: &WindowPolicy,
    onchain_ts: impl Fn(&OracleAsset, Band) -> Option<u64>,
) -> Plan {
    let mut out = Plan::default();
    for asset in assets {
        for &band in &Band::STANDARD {
            let skip = |reason: String| Skip {
                code: asset.code.clone(),
                band,
                reason,
            };
            let points = pocky_api::store::samples(runs, &asset.code, band);
            let w = match windowed_min(&points, now, policy) {
                Ok(w) => w,
                Err(e) => {
                    out.skipped.push(skip(e.to_string()));
                    continue;
                }
            };
            if now.saturating_sub(w.to_ts) > CONTRACT_MAX_AGE_SECS {
                out.skipped
                    .push(skip("latest sample older than the oracle accepts".into()));
                continue;
            }
            if let Some(prev) = onchain_ts(asset, band) {
                if w.to_ts <= prev {
                    out.skipped.push(skip("already published".into()));
                    continue;
                }
            }
            out.updates.push(Update {
                asset: asset.clone(),
                band,
                exit_usd: w.min_exit_usd.raw(),
                timestamp: w.to_ts,
                samples: u32::try_from(w.samples).unwrap_or(u32::MAX),
            });
        }
    }
    out
}

#[derive(Serialize)]
struct JsonUpdate<'a> {
    asset: JsonAsset<'a>,
    band_bps: u32,
    exit_usd: String,
    timestamp: u64,
    samples: u32,
}

#[derive(Serialize)]
struct JsonAsset<'a> {
    #[serde(rename = "Stellar")]
    stellar: &'a str,
}

/// The `--updates` argument for `stellar contract invoke ... -- publish`.
pub fn to_cli_json(updates: &[Update]) -> Result<String, serde_json::Error> {
    let v: Vec<JsonUpdate<'_>> = updates
        .iter()
        .map(|u| JsonUpdate {
            asset: JsonAsset {
                stellar: &u.asset.sac,
            },
            band_bps: u.band.bps(),
            exit_usd: u.exit_usd.to_string(),
            timestamp: u.timestamp,
            samples: u.samples,
        })
        .collect();
    serde_json::to_string(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocky_core::{AssetDepth, Thresholds};
    use pocky_ingest::AssetReport;

    const NOW: u64 = 1_800_000_000;

    fn asset() -> OracleAsset {
        OracleAsset {
            code: "USTRY".into(),
            sac: "CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR".into(),
        }
    }

    fn run(ts: u64, usd: &str) -> Run {
        let t = Thresholds::default();
        let depths = Band::STANDARD
            .iter()
            .map(|&b| AssetDepth {
                total_usd: usd.parse().unwrap(),
                ..AssetDepth::combine("USTRY", b, vec![], &t).unwrap()
            })
            .collect();
        Run {
            measured_at: ts,
            xlm_usd: "0.125".into(),
            venues: "test".into(),
            reports: vec![AssetReport {
                code: "USTRY".into(),
                issuer: "G".into(),
                reference: None,
                venue_mids: vec![],
                depths,
                notes: vec![],
                venues_read: 0,
            }],
            skipped: vec![],
            stats: Default::default(),
        }
    }

    fn hourly(n: u64, end: u64) -> Vec<Run> {
        (0..n)
            .map(|i| run(end - i * 3600, if i == 3 { "50000" } else { "300000" }))
            .collect()
    }

    #[test]
    fn publishes_the_window_minimum_for_every_band() {
        let p = plan(
            &hourly(13, NOW),
            &[asset()],
            NOW,
            &WindowPolicy::default(),
            |_, _| None,
        );
        assert_eq!(p.updates.len(), 3);
        assert!(p.skipped.is_empty());
        let u = &p.updates[1];
        assert_eq!(u.band, Band::TEN_PCT);
        assert_eq!(u.exit_usd, 50_000 * 10_000_000);
        assert_eq!(u.timestamp, NOW);
        assert_eq!(u.samples, 13);
    }

    #[test]
    fn thin_coverage_publishes_nothing() {
        let p = plan(
            &hourly(3, NOW),
            &[asset()],
            NOW,
            &WindowPolicy::default(),
            |_, _| None,
        );
        assert!(p.updates.is_empty());
        assert_eq!(p.skipped.len(), 3);
    }

    #[test]
    fn never_replays_what_the_oracle_already_has() {
        let runs = hourly(13, NOW);
        let p = plan(&runs, &[asset()], NOW, &WindowPolicy::default(), |_, b| {
            (b == Band::TEN_PCT).then_some(NOW)
        });
        assert_eq!(p.updates.len(), 2);
        assert_eq!(p.skipped[0].reason, "already published");
    }

    #[test]
    fn cli_json_matches_the_contract_types() {
        let p = plan(
            &hourly(13, NOW),
            &[asset()],
            NOW,
            &WindowPolicy::default(),
            |_, _| None,
        );
        let j = to_cli_json(&p.updates[..1]).unwrap();
        assert_eq!(
            j,
            r#"[{"asset":{"Stellar":"CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR"},"band_bps":200,"exit_usd":"500000000000","timestamp":1800000000,"samples":13}]"#
        );
    }
}
