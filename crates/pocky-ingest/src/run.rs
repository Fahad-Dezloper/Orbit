//! One full measurement run: every registered asset, every venue, one timestamp.
//!
//! Shared by the CLI (one-shot) and the API service (scheduled), so both produce exactly
//! the same figures and the same JSON.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::horizon::Horizon;
use crate::measure::{measure_asset, xlm_usd, AssetReport, MeasureConfig};
use crate::registry::Registry;
use crate::sep1;
use crate::soroban::SorobanRpc;

/// An asset that could not be measured in this run, and why.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skipped {
    pub code: String,
    pub reason: String,
}

/// The result of one run. This is the snapshot format stored and served by `pocky-api`.
#[derive(Clone, Debug, Serialize)]
pub struct Run {
    /// Unix seconds at the end of the run.
    pub measured_at: u64,
    pub xlm_usd: String,
    /// Which venues were read, in words.
    pub venues: String,
    pub reports: Vec<AssetReport>,
    pub skipped: Vec<Skipped>,
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
    let xlm = xlm_usd(h, &reg.usdc)?;
    let mut reports = Vec::new();
    let mut skipped = Vec::new();
    for a in reg
        .assets
        .iter()
        .filter(|a| opts.only.as_deref().is_none_or(|c| c == a.code))
    {
        if !opts.skip_sep1 {
            if let Err(e) = sep1::verify(a) {
                skipped.push(Skipped {
                    code: a.code.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        }
        match measure_asset(h, rpc, a, &reg.usdc, &reg.pools, xlm, cfg) {
            Ok(r) => reports.push(r),
            Err(e) => skipped.push(Skipped {
                code: a.code.clone(),
                reason: e.to_string(),
            }),
        }
    }
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
    Ok(Run {
        measured_at,
        xlm_usd: xlm.to_string(),
        venues,
        reports,
        skipped,
    })
}
