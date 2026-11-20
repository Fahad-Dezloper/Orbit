//! `pocky` command-line interface.
//!
//! ```text
//! pocky measure                # table of exit liquidity for every registered asset
//! pocky measure --json         # machine-readable output
//! pocky measure --asset USTRY  # one asset
//! ```
//!
//! Read-only. Never signs or submits anything.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use pocky_core::Band;
use pocky_ingest::{measure::xlm_usd, measure_asset, sep1, Horizon, MeasureConfig, Registry};
use serde::Serialize;

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
    /// Measure exit liquidity on mainnet SDEX books (read-only).
    Measure {
        /// Path to the asset registry.
        #[arg(long, default_value = "assets.toml")]
        registry: PathBuf,
        /// Horizon base URL.
        #[arg(long, default_value = "https://horizon.stellar.org")]
        horizon: String,
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

#[derive(Serialize)]
struct Output {
    measured_at: u64,
    xlm_usd: String,
    reports: Vec<pocky_ingest::AssetReport>,
    skipped: Vec<Skipped>,
}

#[derive(Serialize)]
struct Skipped {
    code: String,
    reason: String,
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
        asset,
        json,
        skip_sep1,
    } = cli.cmd;
    let reg = Registry::from_toml(&std::fs::read_to_string(&registry)?)?;
    let h = Horizon::new(horizon);
    let cfg = MeasureConfig::default();
    let xlm = xlm_usd(&h, &reg.usdc)?;

    let mut reports = Vec::new();
    let mut skipped = Vec::new();
    for a in reg
        .assets
        .iter()
        .filter(|a| asset.as_deref().is_none_or(|c| c == a.code))
    {
        if !skip_sep1 {
            if let Err(e) = sep1::verify(a) {
                skipped.push(Skipped {
                    code: a.code.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        }
        match measure_asset(&h, a, &reg.usdc, xlm, &cfg) {
            Ok(r) => reports.push(r),
            Err(e) => skipped.push(Skipped {
                code: a.code.clone(),
                reason: e.to_string(),
            }),
        }
    }

    let measured_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let out = Output {
        measured_at,
        xlm_usd: xlm.to_string(),
        reports,
        skipped,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print_table(&out);
    }
    Ok(())
}

fn usd(v: pocky_core::Fx) -> String {
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

fn print_table(out: &Output) {
    println!(
        "XLM/USD {}   (SDEX order books only; Soroban AMM pools not yet included)\n",
        out.xlm_usd
    );
    println!("ASSET         EXIT @2%    EXIT @10%    EXIT @25%  VERDICT @10%");
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
        println!(
            "{:<9} {:>12} {:>12} {:>12}  {}",
            r.code,
            cell(Band::TWO_PCT),
            cell(Band::TEN_PCT),
            cell(Band::TWENTY_FIVE_PCT),
            verdict
        );
        for n in &r.notes {
            println!("          note: {n}");
        }
    }
    for s in &out.skipped {
        println!("{:<9} skipped: {}", s.code, s.reason);
    }
}
