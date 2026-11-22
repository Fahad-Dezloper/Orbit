//! `pocky-api`: measure on a schedule, store every run, serve it over HTTP.
//!
//! ```text
//! pocky-api                          # measure every 30 min, serve on 127.0.0.1:8080
//! pocky-api --once                   # one measurement into the store, then exit
//! pocky-api --serve-only             # serve the existing store, measure nothing
//! ```
//!
//! Read-only against Stellar: Horizon GETs and Soroban simulations only.

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use clap::Parser;
use pocky_api::http::{now, router, AppState, Shared, Status};
use pocky_api::schedule::{jittered, seed};
use pocky_api::store::Store;
use pocky_core::{Thresholds, WindowPolicy};
use pocky_ingest::run::{measure_all, RunOptions};
use pocky_ingest::soroban::{SorobanRpc, DEFAULT_MAINNET_RPC};
use pocky_ingest::{Horizon, MeasureConfig, Registry};

/// Shortest allowed interval. More often would only load public infrastructure.
const MIN_INTERVAL_SECS: u64 = 300;

#[derive(Parser, Clone)]
#[command(
    name = "pocky-api",
    version,
    about = "Scheduled exit-liquidity measurement and HTTP API"
)]
struct Args {
    /// Address to listen on. Defaults to localhost only.
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: SocketAddr,
    /// Directory for snapshot files.
    #[arg(long, default_value = "data/snapshots")]
    data_dir: PathBuf,
    /// Path to the asset registry.
    #[arg(long, default_value = "assets.toml")]
    registry: PathBuf,
    #[arg(long, default_value = "https://horizon.stellar.org")]
    horizon: String,
    #[arg(long, default_value = DEFAULT_MAINNET_RPC)]
    rpc: String,
    /// Measure SDEX order books only.
    #[arg(long)]
    no_amm: bool,
    /// Average seconds between measurements (minimum 300). Each wait is randomized by ±25%
    /// so sample times cannot be predicted. 1800 gives about 48 samples a day, well above the
    /// 12 the 24h window requires, and never a gap near the 2h limit.
    #[arg(long, default_value_t = 1800)]
    interval_secs: u64,
    /// Run one measurement into the store and exit.
    #[arg(long)]
    once: bool,
    /// Serve the existing store without measuring.
    #[arg(long)]
    serve_only: bool,
}

fn measure_once(args: &Args, reg: &Registry) -> Result<pocky_ingest::run::Run, String> {
    let h = Horizon::new(args.horizon.clone());
    let rpc = if args.no_amm {
        None
    } else {
        let r = SorobanRpc::mainnet(args.rpc.clone());
        r.check_network().map_err(|e| e.to_string())?;
        Some(r)
    };
    measure_all(
        &h,
        rpc.as_ref(),
        reg,
        &MeasureConfig::default(),
        &RunOptions::default(),
    )
    .map_err(|e| e.to_string())
}

fn record(state: &AppState, result: Result<pocky_ingest::run::Run, String>) {
    let outcome = result.and_then(|run| {
        state.store.append(&run).map_err(|e| e.to_string())?;
        Ok(run.measured_at)
    });
    if let Ok(mut st) = state.status.write() {
        match outcome {
            Ok(ts) => {
                st.last_run_at = Some(ts);
                st.last_error = None;
                st.runs_ok += 1;
                println!("measured at {ts}");
            }
            Err(e) => {
                st.last_error = Some(e.clone());
                st.runs_failed += 1;
                eprintln!("measurement failed: {e}");
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.interval_secs < MIN_INTERVAL_SECS {
        return Err(format!("--interval-secs must be at least {MIN_INTERVAL_SECS}").into());
    }
    let reg = Arc::new(Registry::from_toml(&std::fs::read_to_string(
        &args.registry,
    )?)?);
    let policy = WindowPolicy::default();
    let state: Shared = Arc::new(AppState {
        store: Store::open(&args.data_dir)?,
        policy,
        thresholds: Thresholds::default(),
        // Serve the latest run only while it is fresh enough to be in a valid window.
        max_staleness_secs: policy.max_gap_secs,
        status: RwLock::new(Status::default()),
    });

    if args.once {
        let (a, r) = (args.clone(), reg.clone());
        let result = tokio::task::spawn_blocking(move || measure_once(&a, &r)).await?;
        let failed = result.is_err();
        record(&state, result);
        return if failed {
            Err("measurement failed".into())
        } else {
            Ok(())
        };
    }

    if !args.serve_only {
        let (st, a, r) = (state.clone(), args.clone(), reg.clone());
        tokio::spawn(async move {
            // Measure now, then at unpredictable times around the interval.
            loop {
                let (a2, r2) = (a.clone(), r.clone());
                match tokio::task::spawn_blocking(move || measure_once(&a2, &r2)).await {
                    Ok(result) => record(&st, result),
                    Err(e) => record(&st, Err(format!("measurement task panicked: {e}"))),
                }
                let wait = jittered(Duration::from_secs(a.interval_secs), seed());
                tokio::time::sleep(wait).await;
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    println!(
        "pocky-api listening on http://{} (store {}, started {})",
        args.bind,
        args.data_dir.display(),
        now()
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
