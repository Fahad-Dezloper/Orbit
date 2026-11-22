# Demo: exit liquidity for Stellar RWAs

A five-minute walkthrough. Every number below is from a live, read-only measurement of
Stellar mainnet on 21 Nov 2026.

## 1. The problem

In February 2026 the YieldBlox pool on Blend lost **$10.97M** against a token whose price
was reported correctly. The market behind that price was so thin that the collateral could
not be sold when it had to be. A price feed says what one token is worth; it does not say
how much of it can be sold.

Auditors prescribed **market-depth validation** afterwards. The checks that exist today
read the SDEX order book only, and lenders on Stellar still size RWA collateral from price
(or NAV) alone.

## 2. What it measures, live

| Asset | Exit @10% | Where it is | Verdict |
| --- | ---: | --- | --- |
| USTRY | $371,837 | SDEX $261K · Aquarius $107K · Soroswap $4.5K | healthy |
| CETES | $361,535 | SDEX $260K · Aquarius $102K | healthy |
| USDY | $103,118 | Aquarius $101K · SDEX only **$2.2K** | healthy |
| TESOURO | $57,729 | SDEX $52K · Aquarius $5.3K | shallow |
| MEX, CETESZ, EUROB, GILTS, KTB, MEXe | **$0** | bids only, no price on any venue | dead |

- **6 of the 10** freely tradable RWA tokens on Stellar cannot be sold at all.
- Measuring the SDEX alone gets USDY wrong by **~50x**: its market is an Aquarius pool.
- A lender elsewhere has **$3.56M** borrowed against deJAAA, which has **$0** of exit on
  Stellar (research figure, 19 Nov).

## 3. The number a lender actually needs

A liquidator paid a bonus `b` only acts if it can sell the seized collateral at most `b`
below the price. So **the exit at band `b` is the most collateral that can be liquidated
on-chain at that bonus.** Pocky computes that curve for every asset, from 0.5% to 50%:

- USTRY at a 5% bonus: **$314,464** can be liquidated right now. Above that, a pool is
  holding bad debt it cannot clear.
- `GET /v1/assets/USTRY/cap?bonus_bps=500&safety_bps=5000` turns the 24h minimum of that
  figure into a recommended collateral cap.

## 4. A lender that checks depth, live

`scripts/blocked-borrow-demo.sh` runs the whole chain on a local network: a live mainnet
measurement, published to the depth oracle, read by a small lending pool
(`contracts/demo-lender`) that refuses collateral the market could not absorb. The pool
takes collateral up to the depth within 10%, less a 50% safety margin. Run of 22 Nov 2026:

```
== what the pool sees (cap = depth within 10% x 50%)
  USTRY  cap $191,233
  KTB    cap $0
  deJAAA cap none   (no market on Stellar; $3.56M lent against it elsewhere)

== deposits
  ACCEPTED  alice: $150,000 of USTRY
  REFUSED   bob:   $100,000 of USTRY (pool would exceed)   DepthCapExceeded: the market cannot absorb it
  ACCEPTED  bob:   $20,000 of USTRY
  REFUSED   alice: $1,000 of KTB (priced, unsellable)      DepthCapExceeded: the market cannot absorb it
  REFUSED   alice: $1,000 of deJAAA                        NoFreshDepth: no depth data, no collateral

== pool totals
  USTRY collateral $170,000 of cap $191,233
  KTB   collateral $0
```

The check is one cross-contract call (`fresh_depth`) and one comparison; a Blend-style pool
could add it to its supply path.

## 5. How fast and how cheap

| | |
| --- | --- |
| Full scan: 10 assets, 29 venues (20 order books, 9 AMM pools) | **4.0 s** (was 17 s) |
| Depth maths for one asset's full 12-band curve | **22 µs** (all ten assets: 0.2 ms) |
| Soroban RPC calls per pool per run, after warm-up | **1** (tokens/type/fee cached 6h) |
| Measurement cadence / published window | every 30 min / minimum over 24h, ≥12 samples |
| On-chain oracle reads | SEP-40 style: `depth`, `depths`, `fresh_depth`, `last_timestamp` |
| Tests | 84 (67 engine/API/publisher, 9 oracle contract, 8 demo lender) |

The scan is network-bound: the maths is four orders of magnitude faster than the I/O.

## 6. Why it is trustworthy

- **Read-only.** The engine never signs. Pools are read by simulation only.
- **Identity from first principles.** Issuers are checked against their own `stellar.toml`
  (SEP-1); pool tokens must equal Stellar Asset Contract ids derived locally from the
  network id. Impostor tokens with the same code are rejected.
- **No venue is its own reference.** Every venue is measured from the median price of the
  venues that hold real depth; a thin venue cannot move it, and pushing one venue can only
  make the others look thinner.
- **Manipulation can only lower the figure.** Published depth is the 24h minimum, so
  liquidity parked for one measurement cannot raise it.
- **Fails closed.** Stale data, thin windows or an unreadable oracle publish nothing.
- **Every rounding understates depth.** Fixed-point, no floats.

## 7. What only this does

| | Price oracles (Reflector, RedStone NAV) | Oracle Shield | Damper | **Pocky** |
| --- | --- | --- | --- | --- |
| Question answered | what is it worth | is this price trustworthy | what does it cost to move the book | **how much can be sold** |
| Depth measured | – | SDEX near the mid (±40 bps), as a score input | SDEX, USDC cost to clear | ✓ SDEX + Aquarius + Soroswap (Curve, Sushi next), 12 bands to 50% |
| Soroban AMMs (incl. concentrated) | – | not yet ("requires a dedicated approach") | – | ✓ verified against on-chain balances |
| Works with no CEX listing | ✓ | needs a CEX reference | ✓ | ✓ reference from Stellar venues |
| Collateral cap for a liquidation bonus | – | – | – | ✓ |
| On-chain | price | circuit breaker | wrapper, not deployed | ✓ depth oracle (testnet) |
| History of depth | – | – | – | ✓ append-only, since 21 Nov 2026 |

USDY is the test case: about 98% of its exit is in an Aquarius pool, so any SDEX-only check
reports a market about 50 times thinner than it is. Oracle Shield and Damper are not rivals
to replace; the AMM-inclusive figure is the input both lack, and either can read it.

Building this in-house means: the order-book unit conventions, constant-product exit maths
against an external reference, pool discovery and on-chain verification, SAC derivation,
reference selection, windowing, history storage, an oracle contract and a publisher, each
tested. The history cannot be rebuilt at all: depth that was not recorded is gone.

## 8. Where it stands

- Live: engine, API, dashboard, liquidation curve and cap, publisher, methodology.
- Proven end to end: a lending pool that refuses collateral the market cannot absorb.
- On-chain: oracle deployed on testnet carrying live mainnet figures
  ([`deployments/testnet.md`](deployments/testnet.md)). Mainnet after an external audit.
- Next: audit of the contract and publisher, then mainnet; stable and concentrated pools;
  hosting around the clock.

## Running the demo

```bash
cargo run --release -p pocky-api            # measures every 30 min, serves :8080
cd web && npm run dev                        # dashboard on :3100
curl 'localhost:8080/v1/assets/USTRY/cap?bonus_bps=500'
```
