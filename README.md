# Pocky

**Market-depth validation for real-world assets on Stellar.**

A price feed tells a lender what one token is worth. Pocky tells it **how much of that token
could actually be sold right now** before the price collapses, so collateral can be capped
by what the market can absorb.

> February 2026: the YieldBlox pool on Blend lost $10.97M against a token whose price was
> reported correctly, on a market where almost nothing could be sold. Of the three fixes
> auditors prescribed afterwards, market depth validation is the one nobody built.

## What it measures

For each asset, at 2%, 10% and 25% bands: the USD that could be raised by selling across
every Stellar venue (SDEX order books and Soroban AMM pools) before the **average** sale price
falls more than the band below the reference price. The published figure is the **minimum
over a 24-hour window**, so liquidity that is parked and pulled cannot inflate it.

Snapshot from the research phase (19 Nov 2026, 10% band):

| Asset | Exit on Stellar | Verdict |
| --- | ---: | --- |
| USTRY | ~$380K | healthy |
| CETES | ~$379K | healthy |
| USDY | ~$102K | shallow |
| TESOURO | ~$58K | shallow |
| six Etherfuse bond tokens | $1 to $4 | dead |
| deJAAA | $0 (with $3.56M borrowed against it) | dead |

## Layout

| Path | What | Language |
| --- | --- | --- |
| `crates/pocky-core` | Pure depth math: fixed-point, order-book and AMM exit, verdicts, windowed minimum | Rust, no I/O |
| `crates/pocky-ingest` | Horizon and SEP-1 clients (Soroban RPC next) | Rust |
| `crates/pocky-cli` | `pocky measure` | Rust |
| `crates/pocky-api` | HTTP service and history | Rust (phase 4) |
| `contracts/depth-oracle` | On-chain depth oracle with SEP-40-style reads | Rust / Soroban (phase 3) |
| `web/` | Dashboard | TypeScript / Next.js (phase 4) |
| `docs/` | Idea, architecture, plan, security model | |

## Build and test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Status

Phase 1 (depth math) is complete. Phase 2 reads live SDEX order books with SEP-1 issuer
checks:

```bash
cargo run --release -p pocky-cli -- measure          # table
cargo run --release -p pocky-cli -- measure --json   # JSON
```

Soroban AMM pools (needed for USDY and deJAAA) are next. See [`docs/PLAN.md`](docs/PLAN.md) for every phase and
subphase, and [`docs/SECURITY.md`](docs/SECURITY.md) for the threat model.

## Principles

- Read-only: the engine never signs anything; only the publisher signs, and only to our own
  contract.
- Fail closed: missing or stale data publishes nothing.
- Conservative: every rounding understates depth.
- Reproducible: any figure can be recomputed from its stored snapshot.

## License

Apache-2.0.
