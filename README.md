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

Every venue is measured from **one reference price** per asset, the median of the venues
that hold real depth, so no venue can vouch for its own price.

Live run, 21 Nov 2026, 10% band, SDEX order books plus nine Soroban AMM pools
([full output](docs/measurements/2026-11-21-all-venues.md)):

| Asset | Exit on Stellar | Where it is | Verdict |
| --- | ---: | --- | --- |
| USTRY | $381,836 | SDEX $271K, Aquarius $107K, Soroswap $4.5K | healthy |
| CETES | $352,896 | SDEX $267K, Aquarius $85K | healthy |
| USDY | $109,377 | Aquarius $107K, SDEX $2.2K | healthy |
| TESOURO | $60,335 | SDEX $55K, Aquarius $5.3K | shallow |
| six Etherfuse bond tokens | $0 | bids only, no price anywhere | dead |
| deJAAA | $0, with $3.56M borrowed against it (research, 19 Nov) | no Stellar venue | dead |

## Layout

| Path | What | Language |
| --- | --- | --- |
| `crates/pocky-core` | Pure depth math: fixed-point, order-book and AMM exit, verdicts, windowed minimum | Rust, no I/O |
| `crates/pocky-ingest` | Horizon, Soroban RPC and SEP-1 clients; pool verification | Rust |
| `crates/pocky-cli` | `pocky measure` | Rust |
| `crates/pocky-api` | HTTP service and history | Rust (phase 4) |
| `contracts/depth-oracle` | On-chain depth oracle with SEP-40-style reads | Rust / Soroban |
| `web/` | Dashboard | TypeScript / Next.js (phase 4) |
| `docs/` | Idea, architecture, plan, security model | |

## Build and test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# contract (own workspace)
cd contracts/depth-oracle && cargo test
stellar contract build   # needs stellar-cli v25.2+
```

### Run the oracle on a local network

```bash
stellar container start local      # Docker quickstart, localhost only
./scripts/local-oracle-demo.sh
```

Deploys the contract, publishes live figures, reads them back, and checks that a stranger,
the admin, a replayed timestamp, a future-dated figure and a self-rotating publisher are all
rejected.

## Status

Depth math, live ingestion (SDEX books and Soroban AMM pools, with SEP-1 issuer checks and
on-chain pool verification) and the on-chain oracle are complete:

```bash
cargo run --release -p pocky-cli -- measure          # table
cargo run --release -p pocky-cli -- measure --json   # JSON
cargo run --release -p pocky-cli -- measure --no-amm # SDEX order books only
```

Pools are read from Soroban RPC by simulation only. Next: the API service and dashboard. See [`docs/PLAN.md`](docs/PLAN.md) for every phase and
subphase, and [`docs/SECURITY.md`](docs/SECURITY.md) for the threat model.

## Principles

- Read-only: the engine never signs anything; only the publisher signs, and only to our own
  contract.
- Fail closed: missing or stale data publishes nothing.
- Conservative: every rounding understates depth.
- Reproducible: any figure can be recomputed from its stored snapshot.

## License

Apache-2.0.
