# Architecture

```
            public Stellar data (read-only)
   ┌───────────────────────┬──────────────────────────┐
   │ Horizon /order_book   │ Soroban RPC              │
   │ (SDEX bids and asks)  │ (AMM pool reserves)      │
   └──────────┬────────────┴────────────┬─────────────┘
              ▼                         ▼
        crates/pocky-ingest   (I/O only: fetch, parse, verify issuers via SEP-1)
              │  typed snapshots
              ▼
        crates/pocky-core     (pure math, no I/O: walk books, AMM exit, aggregate,
              │                time-window minimum, verdicts)
              ▼
   ┌──────────┼──────────────────────┬───────────────────────────┐
   ▼          ▼                      ▼                           ▼
 pocky-cli  pocky-api (HTTP+JSON)  publisher → contracts/      web/ dashboard
 (one-shot) (history, per asset)   depth-oracle (Soroban,      (Next.js, reads API)
                                    SEP-40-style reads)
```

## Why this split

| Layer | Language | Why |
| --- | --- | --- |
| `pocky-core` | Rust, zero I/O | The number is the product. Keeping the math pure means every figure can be unit-tested and reproduced from a saved snapshot. Integer/fixed-point where it matters. |
| `pocky-ingest` | Rust | Network code isolated from math. Retries, timeouts, unit conventions live here and only here. |
| `pocky-cli` | Rust | Anyone can reproduce a measurement with one command. |
| `pocky-api` | Rust (axum) | Same binary family as the engine; no second runtime to secure. |
| `contracts/depth-oracle` | Rust, Soroban SDK, `wasm32v1-none` | The on-chain read surface for Stellar protocols. Lowest-level piece: storage layout, TTL, auth. |
| `web/` | TypeScript, Next.js | Human-readable dashboard for issuers, protocols and reviewers. |

## Data conventions (decided once, here)

- **Horizon `/order_book` units**: for a pair selling=BASE, buying=COUNTER, `ask.amount` is in
  BASE units and `bid.amount` is in COUNTER units. USD raised selling into bids is therefore
  `sum(bid.amount) * usd_per_counter`, **not** `amount * price`. Getting this wrong overstates
  XLM pairs about 5x.
- **Slippage bands**: 2%, 10%, 25%. A band bounds the shortfall of the **average** execution
  price below the reference price (a liquidator's loss is what it receives on average), not
  the marginal last-fill price.
- **Order-book exit**: walk bids best-first; levels at or above `reference * (1 - band)` are
  taken whole, then lower levels only while the surplus above the floor absorbs them.
- **Constant-product AMM exit** with fee `f` at band `s`: the admissible effective input is
  `dx' = x * ((1 - f)/(1 - s) - 1)`, raising `y * dx' / (x + dx')`. With no fee this is
  exactly `s * y`.
- **Reference price**: every venue is measured from one reference per asset, the upper median
  of the own-prices of venues holding at least $100 of depth. A venue is never its own
  reference, a thin venue cannot move it, and pushing one venue can only shrink the others.
- **AMM exit against a reference** `r`: the average sale price `(1 - f) y / (x + dx')` must stay
  at or above `r (1 - s)`, so `x + dx' <= (1 - f) y / (r (1 - s))`. A pool already below the
  floor contributes nothing.
- **Pool identity**: a pinned pool counts only if its two tokens equal the SAC ids derived
  locally from `sha256(network id, asset)`, and (Aquarius) its type is constant-product.
- **Rounding** always understates depth: amounts round down, base absorbed rounds up, the
  price floor rounds up. No floating point anywhere in `pocky-core`.
- **Published depth** = minimum of per-snapshot exit over the window (default 24h), so depth
  that is placed and pulled cannot inflate the figure.
- **Issuer identity** is resolved from each issuer's own `stellar.toml` (SEP-1), never from
  Horizon's asset list, which contains impostor tokens.

## Repository layout

```
crates/pocky-core     pure math + types
crates/pocky-ingest   Horizon / Soroban RPC / SEP-1 clients
crates/pocky-cli      `pocky measure`
crates/pocky-api      HTTP service (phase 4)
contracts/depth-oracle Soroban contract (own workspace, wasm target)
web/                  dashboard (phase 4)
docs/                 idea, architecture, plan, security, methodology
```
