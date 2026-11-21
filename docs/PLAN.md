# Build plan

Status key: `[x]` done, `[~]` in progress, `[ ]` not started. Each subphase ends in a commit
with passing checks. Nothing in this plan writes to mainnet.

## Phase 0: Foundation
- [x] 0.1 Repository, workspace, README
- [x] 0.2 Docs: idea, architecture, plan, security model
- [x] 0.3 Toolchain pinned (`rust-toolchain.toml`), lint and format config

## Phase 1: Depth math (`pocky-core`)
- [x] 1.1 Types: assets, order-book levels, AMM pools, slippage bands
- [x] 1.2 Order-book exit walk with the Horizon unit convention
- [x] 1.3 Constant-product AMM exit at a slippage band
- [x] 1.4 Cross-venue aggregation and verdicts (dead / critical / shallow / healthy)
- [x] 1.5 Manipulation resistance: windowed minimum and persistence filter
- [x] 1.6 Unit tests, including the February USTRY book shape

## Phase 2: Ingest (`pocky-ingest`, `pocky-cli`)
- [x] 2.1 Horizon order-book client with timeouts and bounded retries
- [x] 2.2 Asset registry (issuers verified via SEP-1) and USD conversion for XLM pairs
- [x] 2.3 `pocky measure` CLI: table and JSON output
- [x] 2.4 Soroban RPC: read Aquarius and Soroswap constant-product pools by simulation; tokens
  verified against locally derived SAC ids (Phoenix has no pools for these assets)
- [x] 2.5 Live check against mainnet: all venues match the research snapshot
  (`docs/measurements/2026-11-21-all-venues.md`)
- [x] 2.6 Cross-venue reference price: every venue measured from the upper median of venues
  with real depth, never from its own mid
- [ ] 2.7 Aquarius stable and concentrated pools; pools against other counters (sUSD, XLD)
- [x] 2.8 Parallel ingestion: one stellar.toml fetch per issuer, assets and venues read
  concurrently, bounded RPC concurrency with 429 back-off, pool metadata cached (17s to 4s)
- [x] 2.9 Liquidation curve (0.5% to 50%) and collateral cap for a liquidation bonus

## Phase 3: On-chain oracle (`contracts/depth-oracle`)
- [x] 3.1 Storage layout, admin and publisher roles, TTL extension
- [x] 3.2 `publish` (publisher-authorised, monotonic timestamps, bounds checks)
- [x] 3.3 SEP-40-style reads: `depth(asset)`, `depths(asset, n)`, `last_timestamp`
- [x] 3.4 Contract tests: auth, stale data, replay, bounds
- [x] 3.5 Local-network deployment script (`scripts/local-oracle-demo.sh`); never public testnet or mainnet in this phase

## Phase 4: Service and dashboard
- [x] 4.1 `pocky-api`: scheduled measurement loop, append-only snapshot store, JSON endpoints
- [x] 4.2 History and windowed-minimum endpoint per asset (fails closed on thin coverage)
- [~] 4.3 `web/`: asset table with venue breakdown and price-gap flags done; history chart
  and methodology page next
- [~] 4.4 Collateral view: liquidatable at a bonus and recommended cap done (API and
  dashboard); live borrowed amounts per lender next
- [x] 4.5 `pocky-publisher`: windowed minimum to oracle batches; no replays, fails closed,
  mainnet refused; end-to-end local demo (`scripts/local-publish-demo.sh`)
- [x] 4.6 Container image and deployment guide (`Dockerfile`, `docs/DEPLOY.md`)
- [ ] 4.7 Hosted API with a public URL, measuring around the clock

## Phase 5: Hardening and proof
- [ ] 5.1 Security review against `SECURITY.md`; fix findings
- [ ] 5.2 Local-network demo: thin market, price pushed, depth oracle blocks the borrow
- [ ] 5.3 Methodology document a protocol risk team can audit
- [ ] 5.4 Release notes and SCF application material

## Phase 6: Beyond Stellar venues (after first integrations)
- [ ] 6.1 Exit routes for Stellar assets that leave Stellar (NEAR Intents, issuer redemption)
- [x] 6.2a Oracle and publisher on testnet with live mainnet figures
  (`docs/deployments/testnet.md`)
- [ ] 6.2b Mainnet, after an external review
