# Changelog

## v0.1.0 (unreleased)

First public release: live market depth for Stellar RWAs, and a safe collateral limit built on
it.

### Measurement
- Exit liquidity at 2%, 10% and 25% price drops, and a 12-point curve from 0.5% to 50%, for 46
  tokens: Etherfuse bonds, Ondo USDY, Franklin Templeton (BENJI), WisdomTree funds, Centrifuge
  (deJAAA, deJTRSY and others), and every other token Stellar lenders accept.
- Venues: SDEX order books (USDC and XLM), Aquarius constant-product, concentrated and stable
  pools, Soroswap pairs, Sushi v3 pools.
- Every token confirmed by its issuer's own `stellar.toml`; every pool checked against
  locally derived token addresses and the balances the token contracts report.
- One cross-venue reference price; a thin venue cannot set it.
- Optional SDEX bid filters: minimum offer age and per-account share (off by default; measured
  impact in `docs/METHODOLOGY.md` §7.1).

### Manipulation resistance
- Randomized reading times (±25%).
- Published figure is the lowest reading of the last 7 days, with at least 12 readings and no
  gap over 2 hours in the last 24 hours. A dip lowers it at once; a rise needs a week of
  sustained depth.
- Incomplete readings never count; missing data is a gap, not low depth.

### Lenders
- Holdings of pinned Blend v2 pools, read on-chain and checked against token balances.
- Collateral in Templar's markets (on NEAR), from Templar's API, labelled "reported".

### On-chain
- `depth-oracle` Soroban contract (testnet), readable by any contract (`fresh_depth`).
- `orbit-publish`: publishes complete windows only, never replays, refuses mainnet without an
  explicit flag.

### Running it
- Public instance on GitHub Actions and Pages: docs, dashboard and static API, measured about
  every 30 minutes.
- Self-hosting: `docker compose up` for the API and dashboard; `scripts/deploy-oracle.sh` and
  the `publisher` profile for your own oracle.

### Not yet
- External audit of the contracts and publisher; mainnet oracle.
- sUSD and XLD counter pools; Blend v2.1 pools; Curve, Comet and Stellar Broker have no pools
  for these tokens yet.
- A second data provider to cross-check each run; own ledger ingestion for when Horizon is
  retired.
