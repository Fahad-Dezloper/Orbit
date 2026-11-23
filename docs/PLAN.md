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
- [x] 1.5 Manipulation resistance: 24h windowed minimum with coverage rules
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
- [x] 2.7 Aquarius concentrated and stable pools, measured from the pool's own swap quotes
  (USDY's concentrated pool adds ~$8K at 10%)
- [ ] 2.7b Pools against counters other than USDC and XLM (sUSD, XLD)
- [x] 2.8 Parallel ingestion: one stellar.toml fetch per issuer, assets and venues read
  concurrently, bounded RPC concurrency with 429 back-off, pool metadata cached (17s to 4s)
- [x] 2.9 Liquidation curve (0.5% to 50%) and collateral cap for a liquidation bonus
- [ ] 2.10 New venues: Curve (on Stellar since 29 Oct 2026), Comet, Stellar Broker and Sushi pools (since Sep 2026,
  including deJAAA); a check that flags new pools for pinned assets

## Phase 3: On-chain oracle (`contracts/depth-oracle`)
- [x] 3.1 Storage layout, admin and publisher roles, TTL extension
- [x] 3.2 `publish` (publisher-authorised, monotonic timestamps, bounds checks)
- [x] 3.3 SEP-40-style reads: `depth(asset)`, `depths(asset, n)`, `last_timestamp`
- [x] 3.4 Contract tests: auth, stale data, replay, bounds
- [x] 3.5 Local-network deployment script (`scripts/local-oracle-demo.sh`); never public testnet or mainnet in this phase

## Phase 4: Service and dashboard
- [x] 4.1 `pocky-api`: scheduled measurement loop, append-only snapshot store, JSON endpoints
- [x] 4.2 History and windowed-minimum endpoint per asset (fails closed on thin coverage)
- [x] 4.3 `web/`: asset table, venue breakdown, price-gap flags, liquidation curve, 24h
  history chart and methodology page
- [x] 4.4 Collateral view: liquidatable at a bonus and recommended cap; RWA holdings of
  pinned Blend v2 pools (factory-verified) beside the exit (`/v1/lenders`, CLI, dashboard)
- [ ] 4.4b Templar markets (where most Stellar RWA borrowing sits)
- [x] 4.5 `pocky-publisher`: windowed minimum to oracle batches; no replays, fails closed,
  mainnet refused; end-to-end local demo (`scripts/local-publish-demo.sh`)
- [x] 4.6 Container image and deployment guide (`Dockerfile`, `docs/DEPLOY.md`)
- [ ] 4.7 Hosted API with a public URL, measuring around the clock

## Phase 5: Hardening and proof
- [x] 5.1 Internal security review (`docs/reviews/2026-11-22-internal.md`); findings fixed or tracked
- [ ] 5.1b Cross-check each run against a second Horizon and Soroban RPC provider before publishing
- [ ] 5.1c External audit of the contracts and publisher
- [x] 5.2 Local-network demo: a lending pool reads the depth oracle and refuses collateral
  the market cannot absorb (`contracts/demo-lender`, `scripts/blocked-borrow-demo.sh`)
- [x] 5.3 Methodology document a protocol risk team can audit (`docs/METHODOLOGY.md`)
- [ ] 5.4 Release notes and SCF application material
- [ ] 5.5 Slow up, fast down: a published figure may rise only on depth sustained for 7 days,
  and falls on the first low sample (`docs/THREATS.md` §2)
- [ ] 5.6 Concentration haircut: count at most a set share of depth from any single SDEX
  offer account or LP position
- [ ] 5.7 Offer age filter: count only SDEX offers resting for at least N hours
- [x] 5.8 Randomized sample times; relative reference threshold; RPC rotation across three
  providers; incomplete runs excluded from the window
- [ ] 5.9 Depth-aware SEP-40 wrapper oracle for new Blend v2 pools; depth-aware caps proposal
  for the Blend v3 draft
- [ ] 5.10 Own ledger ingestion for order books (Horizon end-of-life; RPC has no offers API)

## Phase 6: Beyond Stellar venues (after first integrations)
- [ ] 6.1 Exit routes for Stellar assets that leave Stellar (NEAR Intents, issuer redemption)
- [x] 6.2a Oracle and publisher on testnet with live mainnet figures
  (`docs/deployments/testnet.md`)
- [ ] 6.2b Mainnet, after an external review

## Later: first integrations
- [ ] 7.1 Nectar Network (SCF #42, Blend liquidation keepers): upcoming testnet integration
  through a direct contact, before the SCF application. Keepers read the curve or `/cap`
  (or `fresh_depth` on the testnet oracle) before bidding on Blend auctions. Needs a hosted
  endpoint (4.7) or the testnet contract. Ask for a merged PR and a short quote
