//! One full measurement run: every registered asset, every venue, one timestamp.
//!
//! Shared by the CLI (one-shot) and the API service (scheduled), so both produce exactly
//! the same figures and the same JSON.
//!
//! Network work is parallel: each issuer's `stellar.toml` is fetched once per run (not once
//! per asset), assets are measured concurrently, and each asset reads its pools
//! concurrently. The maths is unchanged and deterministic; only waiting is overlapped.

use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::horizon::Horizon;
use crate::lenders::{read_lender, LenderReserve};
use crate::measure::{measure_asset, xlm_usd, AssetReport, MeasureConfig};
use crate::registry::{Asset, Registry};
use crate::sep1;
use crate::soroban::SorobanRpc;

/// An asset that could not be measured in this run, and why.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skipped {
    pub code: String,
    pub reason: String,
}

/// How a run went, for latency reporting.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RunStats {
    /// Wall-clock time of the whole run, network included.
    pub duration_ms: u64,
    /// Assets measured successfully.
    pub assets: usize,
    /// Venues read across all assets (order books + verified pools).
    pub venues: usize,
    /// Distinct issuer domains whose stellar.toml was fetched.
    pub sep1_domains: usize,
}

/// The result of one run. This is the snapshot format stored and served by `pocky-api`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    /// Unix seconds at the end of the run.
    pub measured_at: u64,
    pub xlm_usd: String,
    /// Which venues were read, in words.
    pub venues: String,
    pub reports: Vec<AssetReport>,
    pub skipped: Vec<Skipped>,
    /// Absent in runs stored before stats were recorded.
    #[serde(default)]
    pub stats: RunStats,
    /// Measured assets held by pinned lending pools. Absent in older runs.
    #[serde(default)]
    pub lenders: Vec<LenderReserve>,
    /// Lending pools that could not be read or verified, and why.
    #[serde(default)]
    pub lenders_skipped: Vec<Skipped>,
}

/// Options for a run.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    /// Only measure this asset code.
    pub only: Option<String>,
    /// Skip the SEP-1 issuer check (offline testing only).
    pub skip_sep1: bool,
}

/// Measures every registered asset. Per-asset failures are recorded in `skipped`; only a
/// failure to price XLM (needed by every asset) fails the whole run.
pub fn measure_all(
    h: &Horizon,
    rpc: Option<&SorobanRpc>,
    reg: &Registry,
    cfg: &MeasureConfig,
    opts: &RunOptions,
) -> Result<Run, IngestError> {
    let started = Instant::now();
    let xlm = xlm_usd(h, &reg.usdc)?;
    let selected: Vec<&Asset> = reg
        .assets
        .iter()
        .filter(|a| opts.only.as_deref().is_none_or(|c| c == a.code))
        .collect();

    // One stellar.toml fetch per issuer domain, all domains in parallel.
    let domains: BTreeSet<&str> = if opts.skip_sep1 {
        BTreeSet::new()
    } else {
        selected.iter().map(|a| a.home_domain.as_str()).collect()
    };
    let tomls: BTreeMap<&str, Result<String, String>> = thread::scope(|s| {
        let handles: Vec<_> = domains
            .iter()
            .map(|&d| (d, s.spawn(move || sep1::fetch_toml(d))))
            .collect();
        handles
            .into_iter()
            .map(|(d, h)| {
                let r = h
                    .join()
                    .unwrap_or_else(|_| Err("stellar.toml fetch panicked".into()));
                (d, r)
            })
            .collect()
    });

    // Every asset in parallel.
    let outcomes: Vec<(String, Result<AssetReport, String>)> = thread::scope(|s| {
        let handles: Vec<_> = selected
            .iter()
            .map(|&a| {
                let toml = tomls.get(a.home_domain.as_str());
                let skip_sep1 = opts.skip_sep1;
                let handle = s.spawn(move || -> Result<AssetReport, String> {
                    if !skip_sep1 {
                        let fail = |reason: String| {
                            IngestError::Sep1 {
                                code: a.code.clone(),
                                domain: a.home_domain.clone(),
                                reason,
                            }
                            .to_string()
                        };
                        let body = match toml {
                            Some(Ok(b)) => b,
                            Some(Err(e)) => return Err(fail(e.clone())),
                            None => return Err(fail("stellar.toml not fetched".into())),
                        };
                        sep1::check_toml(a, body).map_err(fail)?;
                    }
                    measure_asset(h, rpc, a, &reg.usdc, &reg.pools, xlm, cfg)
                        .map_err(|e| e.to_string())
                });
                (a.code.clone(), handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(code, h)| {
                let r = h
                    .join()
                    .unwrap_or_else(|_| Err("measurement panicked".into()));
                (code, r)
            })
            .collect()
    });

    let mut reports = Vec::new();
    let mut skipped = Vec::new();
    for (code, r) in outcomes {
        match r {
            Ok(report) => reports.push(report),
            Err(reason) => skipped.push(Skipped { code, reason }),
        }
    }
    let (lenders, lenders_skipped) = match (rpc, reg.blend_factory.as_deref()) {
        (Some(rpc), Some(factory)) if !reg.lenders.is_empty() => {
            let assets: Vec<Asset> = selected.iter().map(|&a| a.clone()).collect();
            read_lenders(rpc, factory, reg, &assets)
        }
        _ => (Vec::new(), Vec::new()),
    };
    let measured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let venues = match rpc {
        None => "SDEX order books only".to_string(),
        Some(_) => format!(
            "SDEX order books + {} pinned Soroban AMM pools",
            reg.pools.len()
        ),
    };
    let stats = RunStats {
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        assets: reports.len(),
        venues: reports.iter().map(|r| r.venues_read).sum(),
        sep1_domains: domains.len(),
    };
    Ok(Run {
        measured_at,
        xlm_usd: xlm.to_string(),
        venues,
        reports,
        skipped,
        stats,
        lenders,
        lenders_skipped,
    })
}

/// Reads every pinned lending pool in parallel. A pool that fails is skipped, not fatal.
fn read_lenders(
    rpc: &SorobanRpc,
    factory: &str,
    reg: &Registry,
    assets: &[Asset],
) -> (Vec<LenderReserve>, Vec<Skipped>) {
    let outcomes: Vec<(String, Result<Vec<LenderReserve>, String>)> = thread::scope(|s| {
        let handles: Vec<_> = reg
            .lenders
            .iter()
            .map(|l| {
                let h = s
                    .spawn(move || read_lender(rpc, factory, l, assets).map_err(|e| e.to_string()));
                (l.name.clone(), h)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| {
                let r = h
                    .join()
                    .unwrap_or_else(|_| Err("lender read panicked".into()));
                (name, r)
            })
            .collect()
    });
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for (name, r) in outcomes {
        match r {
            Ok(mut v) => out.append(&mut v),
            Err(reason) => skipped.push(Skipped { code: name, reason }),
        }
    }
    (out, skipped)
}
