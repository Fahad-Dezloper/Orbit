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
- [ ] 2.4 Soroban RPC: read Aquarius / Soroswap / Phoenix pool reserves
- [~] 2.5 Live check against mainnet; SDEX figures match the research snapshot (AMM pending 2.4)

## Phase 3: On-chain oracle (`contracts/depth-oracle`)
- [x] 3.1 Storage layout, admin and publisher roles, TTL extension
- [x] 3.2 `publish` (publisher-authorised, monotonic timestamps, bounds checks)
- [x] 3.3 SEP-40-style reads: `depth(asset)`, `depths(asset, n)`, `last_timestamp`
- [x] 3.4 Contract tests: auth, stale data, replay, bounds
- [ ] 3.5 Local-network deployment script (never public testnet or mainnet in this phase). Needs stellar-cli v25.2+ for `stellar contract build`

## Phase 4: Service and dashboard
- [ ] 4.1 `pocky-api`: scheduled measurement loop, snapshot store, JSON endpoints
- [ ] 4.2 History and windowed-minimum endpoint per asset
- [ ] 4.3 `web/`: asset table, depth history chart, methodology page
- [ ] 4.4 Collateral-at-risk view: borrowed against an asset vs its exit depth

## Phase 5: Hardening and proof
- [ ] 5.1 Security review against `SECURITY.md`; fix findings
- [ ] 5.2 Local-network demo: thin market, price pushed, depth oracle blocks the borrow
- [ ] 5.3 Methodology document a protocol risk team can audit
- [ ] 5.4 Release notes and SCF application material

## Phase 6: Beyond Stellar venues (after first integrations)
- [ ] 6.1 Exit routes for Stellar assets that leave Stellar (NEAR Intents, issuer redemption)
- [ ] 6.2 Publisher on testnet, then mainnet, after an external review
