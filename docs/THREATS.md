# What can break this

Written to be shown up front. A depth oracle is only useful if a lender knows exactly where
it stops protecting them. Research current to 22 Nov 2026; sources in
`../StellarGrant/29-threat-research.md` (kept outside the repository).

## The short answer

There are two ways to "break" Pocky, and they are not equally hard.

| | Make Pocky lie | Change the market |
| --- | --- | --- |
| What the attacker does | Gets Pocky to report depth that does not exist | Creates real depth, lets Pocky measure it honestly, removes it when it matters |
| Kind of security | Layered, like MPC: several independent parts must be compromised at once | Economic: the attacker has to pay for it |
| How hard today | Hard | **Not hard enough on its own.** This is the real risk; the design has to make it expensive |

The MPC comparison fits the first column only. In threshold signing an attacker must break
most key holders at once. To make Pocky *lie*, an attacker has to beat the SEP-1 issuer
check, the locally derived token ids, the on-chain balance check on every pool, the
cross-venue reference, several independent RPC providers, the publisher key **and** the
contract's bounds. The second column is different in kind: nothing is compromised. The market
really was deep for a day, and then it wasn't. Cryptography cannot fix that; only economics
can.

## 1. Making Pocky lie (hard)

| Layer | Stops | Breaking it takes |
| --- | --- | --- |
| SEP-1 issuer check | Impostor tokens with the same code | Control of the issuer's website |
| Locally derived SAC ids | Pools holding look-alike tokens | Breaking SHA-256 |
| On-chain balance check | A pool (upgraded or malicious) claiming reserves or quotes it cannot pay | Making the token contract itself lie |
| Cross-venue reference, 5% relative floor | One venue, or cheap pools at a made-up price, setting the price | Real depth at several venues at once |
| RPC rotation across 3 providers; network check on each | One provider feeding false state or rate-limiting a run into missing venues | Compromising all providers in rotation |
| Incomplete runs are not samples | Missing data dragging the figure down (or being mistaken for real) | n/a |
| Publisher key (OS keychain) | Signing false figures | Stealing the key; contract bounds and admin rotation still limit the damage |
| Admin rotation needs two signatures | Taking over the oracle | Both keys (in production, a multisig) |

Still weak: Horizon (order books) is a single provider and is "nearing end-of-life" with no
RPC replacement for listing offers; the publisher machine trusts its own snapshot store;
nothing is externally audited yet.

## 2. Changing the market (the real risk)

### 2a. Timed spoofing (fixed)
With samples at fixed times, an attacker can place offers or concentrated-liquidity positions
seconds before each sample and cancel after (Stellar ledgers close every ~5 s), paying only
fees. **Fixed:** sample times are randomized ±25% around the interval.

### 2b. Patient capital (open, the main risk)
1. Place real buy-side liquidity and leave it long enough for the 24h minimum to rise.
2. A lender raises its cap; the attacker deposits up to it and borrows.
3. Pull the liquidity in one transaction, in the same ledger as the borrow, and move the price.

**Cost:** mostly the time value of the parked capital. $1M for 48h at 4–5% is about $250–300.
For a treasury token, being filled is not a deterrent: the attacker buys bonds at a discount.
**A 24h window makes this slow, not expensive.** Real attackers are patient: the March 2026
Venus THE exploit was accumulated over about nine months.

### 2c. Liquidity that leaves under stress (open, no attacker needed)
On 10 Oct 2025 market makers pulled bids and BTC top-of-book depth fell over 90% in minutes;
about $19B was liquidated. A trailing minimum describes the past. It is safe for sizing *new*
exposure and blind to the next hour.

### Defences against 2b and 2c

| Defence | Effect | Status |
| --- | --- | --- |
| Safety margin on the cap (50% in the demo) | Attacker must create twice the depth they exploit | Built |
| Fail closed on gaps, stale data, incomplete runs | No cap without continuous, complete history | Built |
| Randomized sampling | No timed spoofing | Built |
| **Slow up, fast down**: rise only on depth sustained for 7 days, fall on the first low sample | Capital parked for a week, and any dip resets it | Planned (5.5) |
| **Owner concentration haircut**: count at most a share of depth per SDEX account or LP position; exclude the borrower's own | One whale cannot set the cap; must be many funded accounts | Planned (5.6), needs per-offer data (Horizon `/offers` or own ingestion) |
| **Offer age filter**: count only offers resting for N hours | Defeats place-then-pull even with luck on timing | Planned (5.7) |
| Rate-limited cap growth, cap on *borrowed* amount | Exposure grows gradually and is bounded by what can be taken | Lender side (Blend v3 draft adds a 5-hour borrow limiter) |

The honest claim after 5.5–5.7: **not "cannot be gamed", but gaming it costs real, spread
out capital, parked in public for a week, where anyone can sell into it.** Until then: Pocky
blocks the YieldBlox class (no depth at all) and timed spoofing, and makes patient attacks
slow but not yet costly.

## 3. Past incidents, and whether this would have helped

| Incident | Would a depth cap have helped? |
| --- | --- |
| YieldBlox, Feb 2026 ($10.97M; USTRY book under $1/hour of volume) | **Yes**: depth ~$0, so the cap is ~$0 |
| Mango Markets, Oct 2022 (~$115M) | Yes: thin collateral pumped |
| Curve/Egorov CRV loans, Jul 2023 (~47% of supply as collateral) | Yes: the cap blocks the concentration |
| Tectonic TONIC, Aug 2026 (~$75M borrowed against ~$1.3M of liquidity) | Yes |
| Hyperliquid JELLY, Mar 2025 | Yes, if exposure were capped by depth |
| Aave CRV short, Nov 2022 | **No**: the missing liquidity was on the *debt* side |
| Venus THE, Mar 2026 (supply cap bypassed by donation) | **No**: a cap is only as good as the lender's enforcement |
| 10 Oct 2025 crash | **No**: depth vanished faster than any trailing window |
| LUNA/Venus-Blizz, May 2022 | Partly: new borrowing capped, open positions not |

Trend: one industry count reports 32 price-manipulation exploits on DeFi lenders in the first
eight months of 2026, against 12 in all of 2025 (secondary source, unverified).

## 4. Can anyone actually use it?

The hardest question is not technical.

- **Blend v2 cannot take a fast-moving cap.** Reserve changes go through a 7-day timelock and
  a pool's oracle is fixed at creation. A depth figure reaches existing Blend pools only via
  governance, or via a *new* pool whose oracle is a depth-aware wrapper (returns no price, or
  a clamped one, when depth is below what is posted).
- **Blend v3 is being designed now** (draft 8 Oct 2026: borrow limiter, gated liquidators,
  issuer clawback; "oracle quality is handled at the oracle layer"). This is the window to
  propose depth-aware caps.
- **The market is small today.** About $3.56B of RWAs on Stellar; about $4.2M used as lending
  collateral, mostly deJAAA on Templar; Blend's RWA pools hold about $2M; Blend TVL fell from
  $163M to $106M between October and November.
- **For Etherfuse tokens on-chain depth is the only exit:** Etherfuse fills on-chain purchases
  and redemptions from market liquidity and reports "insufficient liquidity" when the book is
  thin. For fund tokens with issuer redemption and NAV feeds (USDY, deJAAA, BENJI), Stellar
  depth matters less.

## 5. Competitors and substitutes

| Who | Overlap | Difference |
| --- | --- | --- |
| Sun Zu Lab Oracle Shield (SCF #43, $120K) | Readiness score includes **depth**, spread, divergence; on-chain circuit breaker. Their team (Nov 2026, direct correspondence) already runs a Stellar SDEX feed handler showing market depth (±40 bps buckets) | Purpose is to **validate a price**: SDEX mid against a CEX mid ("SDEX cannot be used as a reference"). A health score, not a USD figure of what can be sold. By their own account: AMM liquidity is not yet a source ("one cannot build a regular orderbook from an AMM; extracted liquidity signals will require a dedicated approach"); the USTRY scenario is hypothetical because no CEX lists it; mid-vs-mid is "only a first step" and sources, signals and formulas are still to be extended. **Risk:** they named AMM liquidity as something they "have to consider", so they could build into our area |
| Damper (open source, first mainnet scan 17 Nov 2026) | Manipulation-resistant SEP-40 wrapper for Blend; measures USDC cost to clear the SDEX book | SDEX only (no AMMs), unaudited, not deployed |
| RedStone (Stellar since Mar 2026; serves Blend, Templar, Centrifuge, XOXNO) | Holds the RWA feeds; pushing Settle (KYC'd liquidators) and Credora ratings | NAV ("fundamental") feeds, no depth metric |
| Chainlink (live on Stellar Sep/Oct 2026) | "Liquidity Indicators" (±2% depth, 2023 R&D) and State Pricing (depth-weighted DEX prices) exist elsewhere | Not offered on Stellar today; the largest latent substitute |
| Chaos Labs Edge | Dynamic caps driven by liquidity depth, live on Aave since 2025 | Not on Stellar; left Aave in Apr 2026 |
| Pyth Pro, Reflector | Price feeds on Stellar | No depth |

Our ground: the only measure across **all** Stellar venues (SDEX plus Aquarius, including
concentrated pools, plus Soroswap) from one manipulation-resistant reference, verified against
on-chain balances, turned into a USD liquidation curve and cap, published on-chain, with an
append-only history that cannot be rebuilt later. It needs **no outside market**: the
reference comes from Stellar venues themselves, so it works for RWA tokens that trade on no
CEX (every Etherfuse token), which a CEX-referenced check like Oracle Shield cannot validate
today. The two are complementary: Oracle Shield asks "is this price trustworthy?", Pocky asks
"how much could actually be sold at it?"

## 6. Things that would make it obsolete

1. Liquidations becoming permissioned or off-chain for RWAs (RedStone Settle, Blend v3 gated
   liquidators). Public on-chain depth would stop being the exit that matters.
2. An oracle lenders already read bundling a depth field (Chainlink Liquidity Indicators,
   Sun Zu Lab, RedStone or Credora).
3. Issuers guaranteeing NAV redemption or market making while lenders price at NAV (already
   true for USDY and deJAAA).

Also structural: issuer or validator freezes (CAP-77, auth revocation) remove offers
instantly; Horizon end-of-life removes the easy order-book API; liquidity for some tokens
lives on other chains (Etherfuse also trades on Solana).

## 7. What we will not claim

- That Pocky prevents price manipulation. It limits how much a lender can lose to a market
  that cannot absorb a liquidation.
- That a figure is safe on its own. It is one input to a cap, with a margin, a rate limit and
  a borrow limit.
- That it is audited. It is internally reviewed (`reviews/2026-11-22-internal.md`).
