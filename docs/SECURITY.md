# Security model

Orbit's only product is a number other protocols may use to decide how much to lend. A wrong
number can cost someone real money, so correctness is a security property here.

## Principles

1. **Read-only by default.** The engine, CLI and API never sign transactions. Only the
   publisher signs, and only `publish` calls to our own oracle contract.
2. **Fail closed.** If data is missing, stale or inconsistent, we publish nothing (or mark
   the asset unknown). We never fall back to an old value without its age.
3. **Conservative aggregation.** Published depth is the minimum over the window, never the
   mean or the latest. Under uncertainty, round down.
4. **No unverified identities.** An asset is measured only if its issuer is confirmed by the
   issuer's own `stellar.toml` (SEP-1).
5. **Reproducible.** Every published figure can be recomputed from a stored snapshot with
   `orbit-core`, which has no I/O.

## Threats and mitigations

| Threat | Mitigation |
| --- | --- |
| Depth spoofing (orders placed then pulled) | Published figure is the 24h minimum with at least 12 samples and no gap over 2h, so parked liquidity can only lower it. (A separate persistence filter was planned and is not built; the minimum already covers placed-then-pulled depth.) |
| Wash liquidity (self-trading pools) | AMM exit uses reserves, not volume; volume is never an input |
| Impostor assets with the same code | Issuer pinned by SEP-1; asset id = code + issuer |
| Impostor or swapped AMM pools | Pool tokens must equal SAC ids derived locally from the network id and asset; pool type must match how it is pinned |
| A pool lying about itself (pools are upgradeable and self-report reserves and quotes) | Reported reserves and quoted outputs are checked against the balances the token contracts report for the pool; a pool claiming more than it holds is rejected |
| A venue pushing its own price to look deep | One cross-venue reference (upper median of venues with real depth); a thin venue cannot set it; pushing one venue only lowers the others |
| RPC pointed at the wrong network | `getNetwork` passphrase checked before any read |
| Reading pools moves funds | Pools are read by simulation only; the simulated source account is all zeros and nothing is signed or submitted |
| Unit errors (Horizon bid/ask units) | One parsing path in `horizon.rs`; unit tests pin the convention (a bid's amount is in the counter asset), and live figures were checked against the research snapshot |
| Numeric overflow or precision loss | Checked arithmetic in core; contract stores scaled `i128`; bounds checks on publish |
| Stale on-chain data | Every record carries a timestamp; readers get it; contract rejects non-increasing timestamps |
| Publisher key compromise | Publisher role separate from admin; admin can rotate the publisher; per-publish bounds; the asset list is capped (200) so a rogue publisher cannot grow storage until the contract stops loading |
| Admin key loss or compromise | `set_admin` moves the role only with both the current and the new admin's signatures; a production admin should be a multisig account |
| Upstream outage or tampering (Horizon/RPC) | Timeouts, bounded retries with back-off, network passphrase check. **Not yet built:** cross-checking figures against a second Horizon and RPC provider before publishing (plan 5.1b) |
| Contract state archival | Instance and record TTL extended on every publish |
| Secrets in the repo | No keys in code or config; `.env` ignored; publisher key from the environment only |

## Service and dashboard

- `orbit-api` binds to `127.0.0.1` by default, serves GET only, and allows any origin
  because the data is public. It holds no keys.
- `/v1/latest` returns 503 rather than a figure older than the window's 2h gap limit; a
  window with too few samples returns its reason, never a number.
- The snapshot store is append-only; a torn line is skipped and counted, never repaired by
  guessing.
- Dashboard dependencies are pinned to exact patched versions. Known open advisory: PostCSS
  bundled inside Next 15 (build-time CSS processing of our own stylesheet only; the fix
  requires Next 16, tracked for 5.1).

## Publisher

- The only component that signs. Its key is a `stellar` CLI identity; it never appears in
  this repository, a config file or the process arguments. Identity names that could be
  read as flags are rejected.
- Reads the oracle before writing and publishes only newer windows, so it cannot replay a
  timestamp; if the read fails it publishes nothing.
- Mainnet is not a selectable network in the binary until the external review (phase 5).
- Batches of 10 updates per transaction keep well inside Soroban resource limits.

## Rules for contributors

- No mainnet or public-testnet writes outside an explicitly approved release step.
- Any change to the math needs a test that pins the old and new numbers.
- Dependencies: prefer few, well-known crates; `cargo audit` before each release.
