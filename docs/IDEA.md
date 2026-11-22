# Pocky: the idea

> Read this first if you are new to the project (human or agent). It explains what we are
> building, why, and what we already proved. The research behind it lives in
> `~/Desktop/StellarGrant/` (files 23 to 27).

## One sentence

Pocky publishes, for every real-world-asset (RWA) token on Stellar, **how much of it can
actually be sold right now**, so lending protocols can cap collateral by what the market can
absorb instead of trusting a price alone.

## The problem

A price feed answers "what is one token worth?". It does not answer "if a lender had to sell
the collateral, could it?". Those are different questions, and on thin markets the gap
between them is where money is lost.

- **22 Feb 2026, YieldBlox pool on Blend V2: $10.97M lost.** The attacker pushed the price
  of USTRY on a near-empty order book, the oracle reported that price accurately, and the
  pool lent against it. Nothing was hacked. Reflector reported what the market showed, and
  Blend's contracts had no bug. The missing input was **depth**.
- After the exploit, auditors prescribed three fixes: minimum liquidity thresholds, circuit
  breakers, and market depth validation. Thresholds exist in-house at some protocols. A
  circuit breaker (Oracle Shield, SCF #43) is funded and reads SDEX depth near the mid as a
  score input; Damper (open source, not deployed) measures the cost to clear the SDEX book.
  **Neither counts the Soroban AMM pools, and neither produces a sellable-dollar cap.** That
  is the gap Pocky fills.

## What we measured (live, read-only, 18 to 19 Nov 2026)

Real exit = USD raised selling into the Stellar venues we read (SDEX order books plus Soroban AMM
pools) before losing more than 10%.

| Token | Real exit on Stellar | Note |
| --- | ---: | --- |
| USTRY (US Treasuries) | ~$380K | healthy, for now |
| CETES (Mexican bills) | ~$379K | healthy, for now |
| USDY (Ondo) | ~$102K | thin for 3,321 holders |
| TESOURO (Brazil bonds) | ~$58K | shallow |
| 6 more Etherfuse bond tokens | $1 to $4 each | dead; no lender accepts them |
| **deJAAA (Centrifuge)** | **$0** | **$3.56M of USDC borrowed against it on Templar** |

deJAAA is the sharpest case: about $3.56M of Stellar USDC is lent against it in Templar's
markets (contracts on NEAR, assets bridged from Stellar), priced by a single NAV feed, and
there is no deJAAA pool or order book anywhere on Stellar. That is a cannot-liquidate risk.

## What Pocky is

1. **A measurement engine** (Rust) that reads public Stellar data (Horizon order books,
   Soroban AMM pool reserves) and computes exit liquidity per asset at several slippage
   bands, continuously.
2. **Manipulation resistance**: depth that appears and disappears is discounted. We publish
   the minimum over a time window, never a single snapshot.
3. **An on-chain oracle** (Soroban contract) that publishes the depth figure per asset with a
   SEP-40-style read interface, so any Stellar protocol can read it by config.
4. **A public API and dashboard** with the full history, so risk teams and reviewers can
   audit every number.

## Who uses it

- **Lending protocols** (Blend pool creators, Templar, Oracle Shield operators): cap
  collateral at a fraction of measured depth. Example rule: "accept at most 50% of the
  lowest 7-day exit depth."
- **Circuit breakers** (Oracle Shield): depth is the missing liquidity input their own threat
  model asks for (Tamper.1, "minimum liquidity rules").
- **Issuers** (Etherfuse, Ondo, Centrifuge): proof their token is liquid enough to be
  collateral.
- **Risk teams, auditors, SCF reviewers**: the first historical record of Stellar RWA depth.

## The standing constraint

"I don't want to depend on any other service to exist. I want to be the one others integrate
with." Pocky reads only public chain data. If every protocol ignored it, it would still
publish.

## What is explicitly out of scope

- We do not build a lending protocol, a liquidation engine, or a price oracle. Reflector does
  price; we do depth, next to it.
- We never trade, place orders, or touch mainnet funds. Measurement is read-only. Any
  "manipulation" demos run only on a local network.
