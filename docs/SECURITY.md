# Security model

Pocky's only product is a number other protocols may use to decide how much to lend. A wrong
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
   `pocky-core`, which has no I/O.

## Threats and mitigations

| Threat | Mitigation |
| --- | --- |
| Depth spoofing (orders placed then pulled) | Windowed minimum; persistence filter counts only liquidity seen in consecutive snapshots |
| Wash liquidity (self-trading pools) | AMM exit uses reserves, not volume; volume is never an input |
| Impostor assets with the same code | Issuer pinned by SEP-1; asset id = code + issuer |
| Unit errors (Horizon bid/ask units) | Single conversion function, unit-tested against a known `/offers` reconciliation |
| Numeric overflow or precision loss | Checked arithmetic in core; contract stores scaled `i128`; bounds checks on publish |
| Stale on-chain data | Every record carries a timestamp; readers get it; contract rejects non-increasing timestamps |
| Publisher key compromise | Publisher role separate from admin; admin can rotate the publisher; per-publish bounds |
| Upstream outage or tampering (Horizon/RPC) | Timeouts, bounded retries, cross-check against a second endpoint before publishing |
| Contract state archival | Instance and record TTL extended on every publish |
| Secrets in the repo | No keys in code or config; `.env` ignored; publisher key from the environment only |

## Rules for contributors

- No mainnet or public-testnet writes outside an explicitly approved release step.
- Any change to the math needs a test that pins the old and new numbers.
- Dependencies: prefer few, well-known crates; `cargo audit` before each release.
