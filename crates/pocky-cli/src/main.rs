//! `pocky` command-line interface.
//!
//! ```text
//! pocky measure                # exit liquidity for every registered asset, all venues
//! pocky measure --json         # machine-readable output
//! pocky measure --asset USTRY  # one asset
//! pocky measure --no-amm       # SDEX order books only
//! ```
//!
//! Read-only. Never signs or submits anything.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use pocky_core::{Band, Fx};
use pocky_ingest::run::{measure_all, Run, RunOptions};
use pocky_ingest::soroban::{SorobanRpc, DEFAULT_MAINNET_RPC};
use pocky_ingest::{Horizon, MeasureConfig, Registry};

#[derive(Parser)]
#[command(
    name = "pocky",
    version,
    about = "Exit-liquidity measurement for Stellar real-world assets"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Measure exit liquidity on mainnet SDEX books and Soroban AMM pools (read-only).
    Measure {
        /// Path to the asset registry.
        #[arg(long, default_value = "assets.toml")]
        registry: PathBuf,
        /// Horizon base URL.
        #[arg(long, default_value = "https://horizon.stellar.org")]
        horizon: String,
        /// Soroban RPC URL (mainnet). Only simulations are sent; nothing is submitted.
        #[arg(long, default_value = DEFAULT_MAINNET_RPC)]
        rpc: String,
        /// Skip Soroban AMM pools and measure SDEX order books only.
        #[arg(long)]
        no_amm: bool,
        /// Only measure this asset code.
        #[arg(long)]
        asset: Option<String>,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Skip the SEP-1 issuer check (offline testing only).
        #[arg(long)]
        skip_sep1: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Cmd::Measure {
        registry,
        horizon,
        rpc,
        no_amm,
        asset,
        json,
        skip_sep1,
    } = cli.cmd;
    let reg = Registry::from_toml(&std::fs::read_to_string(&registry)?)?;
    let h = Horizon::new(horizon);
    let soroban = if no_amm {
        None
    } else {
        let s = SorobanRpc::mainnet(rpc);
        s.check_network()?;
        Some(s)
    };
    let opts = RunOptions {
        only: asset,
        skip_sep1,
    };
    let out = measure_all(&h, soroban.as_ref(), &reg, &MeasureConfig::default(), &opts)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print_table(&out);
    }
    Ok(())
}

fn usd(v: Fx) -> String {
    let whole = v.raw() / 10_000_000;
    let s = whole.to_string();
    let mut grouped = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("${grouped}")
}

/// A price to 4 decimal places, truncated.
fn price(v: Fx) -> String {
    let r = v.raw();
    format!("${}.{:04}", r / 10_000_000, (r % 10_000_000) / 1_000)
}

fn print_table(out: &Run) {
    println!("XLM/USD {}   ({})\n", out.xlm_usd, out.venues);
    println!("ASSET      REFERENCE     EXIT @2%    EXIT @10%    EXIT @25%  VERDICT @10%");
    for r in &out.reports {
        let at = |b: Band| r.depths.iter().find(|d| d.band == b);
        let cell = |b: Band| {
            at(b)
                .map(|d| usd(d.total_usd))
                .unwrap_or_else(|| "-".into())
        };
        let verdict = at(Band::TEN_PCT)
            .map(|d| format!("{:?}", d.verdict).to_lowercase())
            .unwrap_or_default();
        let reference = r
            .reference
            .as_ref()
            .map(|x| price(x.price_usd))
            .unwrap_or_else(|| "none".into());
        println!(
            "{:<9} {:>10} {:>12} {:>12} {:>12}  {}",
            r.code,
            reference,
            cell(Band::TWO_PCT),
            cell(Band::TEN_PCT),
            cell(Band::TWENTY_FIVE_PCT),
            verdict
        );
        if let Some(d) = at(Band::TEN_PCT) {
            for v in d.venues.iter().filter(|v| !v.usd.is_zero()) {
                let name = match &v.venue {
                    pocky_core::Venue::Sdex { counter } => format!("SDEX/{counter}"),
                    pocky_core::Venue::Amm {
                        protocol,
                        pool,
                        counter,
                    } => format!("{protocol}/{counter} {}", pool.get(..8).unwrap_or(pool)),
                };
                println!("          {:<28} {:>12} @10%", name, usd(v.usd));
            }
        }
        for n in &r.notes {
            println!("          note: {n}");
        }
    }
    for s in &out.skipped {
        println!("{:<9} skipped: {}", s.code, s.reason);
    }
}
