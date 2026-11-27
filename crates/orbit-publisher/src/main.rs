//! `orbit-publish`: publish windowed depth from the snapshot store to the depth oracle.
//!
//! ```text
//! orbit-publish --network testnet --oracle C... --source orbit-testnet-publisher
//! orbit-publish ... --dry-run            # print the batch, submit nothing
//! orbit-publish ... --every-secs 1800    # keep publishing
//! ```
//!
//! The signing key never passes through this program: `--source` names an identity in the
//! `stellar` CLI's keystore (on mainnet, the OS secure store). Mainnet requires
//! `--mainnet-unaudited` and the full 12-sample window.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use orbit_api::http::now;
use orbit_api::store::Store;
use orbit_core::{Band, WindowPolicy};
use orbit_ingest::soroban::{SorobanRpc, MAINNET_PASSPHRASE};
use orbit_ingest::Registry;
use orbit_publisher::chain::{onchain_ts, submit, Network};
use orbit_publisher::{plan, to_cli_json, OracleAsset};

#[derive(Parser)]
#[command(
    name = "orbit-publish",
    version,
    about = "Publish windowed depth to the oracle"
)]
struct Args {
    /// `local`, `testnet` or `mainnet`.
    #[arg(long)]
    network: String,
    /// Required for mainnet: acknowledges the oracle has not had an external review yet.
    #[arg(long)]
    mainnet_unaudited: bool,
    /// The depth oracle contract address.
    #[arg(long)]
    oracle: String,
    /// `stellar` CLI identity that holds the publisher key.
    #[arg(long)]
    source: String,
    /// Soroban RPC for reading the oracle (defaults per network).
    #[arg(long)]
    rpc: Option<String>,
    #[arg(long, default_value = "data/snapshots")]
    data_dir: PathBuf,
    #[arg(long, default_value = "assets.toml")]
    registry: PathBuf,
    /// Minimum samples in the 24h window. 12 by default; lower only for test networks while
    /// history builds up. Every record carries its sample count, so readers can see it.
    #[arg(long, default_value_t = 12)]
    min_samples: usize,
    /// Updates per transaction, to stay well inside Soroban's per-transaction limits.
    #[arg(long, default_value_t = 10)]
    chunk: usize,
    /// Print what would be published and stop.
    #[arg(long)]
    dry_run: bool,
    /// Repeat every N seconds (minimum 300) instead of publishing once.
    #[arg(long)]
    every_secs: Option<u64>,
}

fn round(args: &Args, network: Network, assets: &[OracleAsset]) -> Result<(), String> {
    let policy = WindowPolicy {
        min_samples: args.min_samples,
        ..WindowPolicy::default()
    };
    let at = now();
    let store = Store::open(&args.data_dir).map_err(|e| e.to_string())?;
    let loaded = store
        .load(at.saturating_sub(policy.lookback_secs()), at)
        .map_err(|e| e.to_string())?;

    // Read what the oracle already holds. If that read fails, publish nothing: a blind
    // publish could replay a timestamp and fail, or worse, hide a real problem.
    let rpc_url = args
        .rpc
        .clone()
        .unwrap_or_else(|| network.default_rpc().into());
    let rpc = SorobanRpc::new(rpc_url.clone(), network.passphrase());
    rpc.check_network().map_err(|e| e.to_string())?;
    let mut held: HashMap<(String, u32), u64> = HashMap::new();
    for a in assets {
        for &b in &Band::STANDARD {
            if let Some(ts) = onchain_ts(&rpc, &args.oracle, a, b).map_err(|e| e.to_string())? {
                held.insert((a.sac.clone(), b.bps()), ts);
            }
        }
    }

    let p = plan(&loaded.runs, assets, at, &policy, |a, b| {
        held.get(&(a.sac.clone(), b.bps())).copied()
    });
    for s in &p.skipped {
        println!("skip {} @{}bps: {}", s.code, s.band.bps(), s.reason);
    }
    if p.updates.is_empty() {
        println!("nothing new to publish");
        return Ok(());
    }
    for chunk in p.updates.chunks(args.chunk.clamp(1, 64)) {
        let json = to_cli_json(chunk).map_err(|e| e.to_string())?;
        if args.dry_run {
            println!("dry run, would publish: {json}");
            continue;
        }
        submit(network, &rpc_url, &args.oracle, &args.source, &json)?;
        let names: Vec<String> = chunk
            .iter()
            .map(|u| format!("{}@{}", u.asset.code, u.band.bps()))
            .collect();
        println!("published {}", names.join(" "));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let network = Network::parse(&args.network, args.mainnet_unaudited)?;
    if network == Network::Mainnet && args.min_samples < WindowPolicy::default().min_samples {
        return Err(format!(
            "mainnet requires the full window: --min-samples must be at least {}",
            WindowPolicy::default().min_samples
        )
        .into());
    }
    let reg = Registry::from_toml(&std::fs::read_to_string(&args.registry)?)?;
    // Assets are named by their mainnet token contract (the SAC, or the token's own contract for
    // contract-only tokens), whatever network the oracle is on.
    let assets = reg
        .assets
        .iter()
        .map(|a| {
            Ok(OracleAsset {
                code: a.code.clone(),
                sac: a.token_address(MAINNET_PASSPHRASE)?,
            })
        })
        .collect::<Result<Vec<_>, orbit_ingest::IngestError>>()?;

    match args.every_secs {
        None => round(&args, network, &assets).map_err(Into::into),
        Some(secs) => {
            if secs < 300 {
                return Err("--every-secs must be at least 300".into());
            }
            loop {
                if let Err(e) = round(&args, network, &assets) {
                    eprintln!("publish round failed: {e}");
                }
                std::thread::sleep(Duration::from_secs(secs));
            }
        }
    }
}
