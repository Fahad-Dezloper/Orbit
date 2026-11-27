# Methodology

How Orbit turns public Stellar market data into one depth figure per asset, written so a
protocol risk team can check each step. Every rule here is implemented in `orbit-core`
(pure, no I/O) or `orbit-ingest` (reads and verification), and every figure can be
recomputed from a stored snapshot.

## 1. The question

> How many US dollars could be raised selling this asset on Stellar right now, before the
> **average** sale price falls more than a band `s` below the reference price?

The average price is used, not the last fill, because a liquidator's loss is what it receives
on average against what the collateral was valued at. Bands: 2%, 10% and 25% are published;
the liquidation curve samples 12 bands from 0.5% to 50%.

## 2. Assets

An asset is the pair (code, issuer), never the code alone. It is measured only if the
issuer's own `stellar.toml` (SEP-1, at the registered home domain) lists that exact code and
issuer. Horizon's asset list is never used to discover issuers: it contains impostor tokens.

## 3. Venues

| Venue | Read from | Exit computed by |
| --- | --- | --- |
| SDEX order book vs USDC, vs XLM | Horizon `/order_book`, 200 levels, exact `price_r` | walking bids (§5.1) |
| Constant-product pools (Aquarius standard, Soroswap) | Soroban RPC simulation: reserves, fee | closed form (§5.2) |
| Concentrated and stable pools (Aquarius) | Soroban RPC simulation: the pool's own `estimate_swap` | sampled quotes (§5.3) |

Pools are pinned in `assets.toml` and verified on every read: their two tokens must equal the
Stellar Asset Contract ids derived locally as `sha256(network id, asset)`, their reported
type must match how they are pinned, and what they report (reserves, quoted outputs) must not
exceed what the token contracts themselves say the pool holds. A pool that fails verification, or cannot be read, is
left out and noted. Leaving a venue out can only understate depth.

All reads are simulations or GETs. Nothing is signed.

## 4. The reference price

Each venue reports its own price in USD (book: mid of best bid and best ask; constant-product
pool: `y / x`; quoted pool: average price of a one-unit sale). A venue may help set the
reference only if it holds at least $100 **and** at least 5% of the deepest venue's exit at
the widest band, measured from its own price; an absolute floor alone is too cheap to clear. The reference is the
**upper median** of the remaining venues' prices.

- No venue is its own reference, so pushing one venue's price moves only that venue.
- A thin venue cannot set the price at all.
- An even split resolves to the higher price, which can only lower measured depth.
- If no venue qualifies there is no reference and the asset's depth is **zero**.

Venues whose own price is more than 2% from the reference are reported in the notes.

USD conversion: USDC = $1; XLM at the mid of the XLM/USDC SDEX book. A one-sided XLM/USDC
book fails the whole run.

## 5. Exit on each venue

Let `r` be the reference in the venue's counter units and `floor = r × (1 − s)`, rounded up.

### 5.1 Order book

Horizon convention for selling=BASE, buying=COUNTER: a bid's `amount` is in **COUNTER**
units. Selling into a bid of (price `p`, amount `a`) raises `a` and absorbs `a / p` of BASE
(rounded up). Bids at or above the floor are taken whole; below it, bids are taken only while
the surplus already earned above the floor absorbs them, and the last one partially. Bids
priced below 1e-7 (shown as `0.0000000`) are dust and skipped.

### 5.2 Constant-product pool

With fee `f`, a sale of `dx` adds `dx' = dx (1 − f)` and returns `y dx' / (x + dx')`. Its
average price is `(1 − f) y / (x + dx')`. Requiring that to be at least the floor gives
`x + dx' ≤ (1 − f) y / floor`. A pool whose price is already below the floor contributes
nothing.

### 5.3 Quoted pool

The pool's own `estimate_swap` is simulated for a one-unit probe and twelve sale sizes
(0.05% to 80% of its counter reserve, converted at the probe price). Quotes must be
consistent: sizes rising, output never falling, average price never improving with size;
otherwise the pool is rejected. The exit is the largest **sampled** sale whose average price
is at or above the floor. Nothing is interpolated.

## 6. Combining venues

Venue exits are converted to USD and summed. A seller can split a sale across independent
venues, keeping each part within the band, so the combined average also stays within it.

Verdict at 10%: dead below $100, critical below $10K, shallow below $100K, healthy above.

## 7. Time: the published figure

A measurement runs on average every 30 minutes, at a randomized time (±25%) so nobody can
place liquidity just before a sample and pull it just after. Soroban reads rotate across
independent RPC providers. A run that could not read every pinned venue is kept but marked
incomplete and is never used as a sample: missing data is a gap, not low depth. The published figure for an asset and band is the
**minimum over the last 7 days**, and only if the last 24 hours hold at least 12 samples with
no gap longer than 2 hours (including the gap from the last sample to now). Otherwise nothing
is published. Liquidity placed just before one measurement and pulled after it can only lower
the figure.

**Slow up, fast down.** A low reading lowers the figure at once. A rise counts only once every
reading for 7 days has been at least that high, so depth has to stay in place, in public, for a
week before any cap built on it can grow.

## 8. Liquidation curve and collateral cap

A liquidator paid a bonus `b` acts only if it can sell the seized collateral at an average
price at most `b` below the reference. So the exit at band `b` is the most collateral that
can be liquidated on Stellar at that bonus. The curve samples that exit at 12 bands.

- Liquidatable at bonus `b`: the curve value at the largest sampled band not above `b`.
- Recommended cap: the windowed minimum (7 days, with 24h coverage rules) of that value ×
  (1 − safety margin), default margin 50%. No recommendation without a full window.

### 8.1 What lenders hold

To show the cap beside real exposure, Orbit reads how much of each measured asset sits in
pinned Blend v2 pools (`[[lender]]` in `assets.toml`):

- A pool is read only if the pinned Blend v2 pool factory confirms it deployed it
  (`is_pool`), so the pool runs Blend's own code.
- A reserve is matched to an asset by its locally derived SAC address, never by code.
- Supplied = `b_supply × b_rate / 10^12`, borrowed = `d_supply × d_rate / 10^12`, both
  rounded down. Supplied counts collateral and plain lending together, so it is an upper
  bound on collateral.
- The reserve's cash (supplied minus borrowed) must not exceed what the token contract says
  the pool holds, within 10 bps; otherwise the pool is skipped and noted.
- Valued at the same run's reference price. Borrowable = supplied × the reserve's collateral
  factor. A reserve whose supplied value exceeds the exit at the bonus is flagged: a full
  liquidation of it would not clear on Stellar.

Only Blend is read. Templar and other lenders are not yet.

## 9. Arithmetic

Fixed-point with 7 decimals (`i128`), the same scale as Stellar amounts. No floating point.
Every rounding is in the direction that lowers depth: amounts and outputs round down, base
absorbed and the floor round up. Overflow is an error, never a wrap.

## 10. On-chain publication

The publisher writes the windowed minimum for each asset and band to the depth oracle, with
the window's end time and sample count. Assets are named by their mainnet SAC address. The
contract rejects non-increasing timestamps, timestamps more than 60 s in the future or more
than 6 h old, negative or absurd values, and any write not signed by the publisher. Readers
should check `timestamp` and `samples` against their own limits (`fresh_depth` does the
first).

## 11. Known limits

- Venues not pinned (new pools, pools against counters other than USDC and XLM, and as of
  Nov 2026 Curve, Comet and Stellar Broker) are not counted. This understates depth.
- Quoted pools are sampled, not solved: exit between samples is not credited.
- Depth on other chains, and redemption with the issuer, are not exit on Stellar and are not
  counted.
- The reference comes from Stellar venues only. If every Stellar venue is moved together the
  reference moves with them; the windowed minimum limits how long that can inflate depth, but it
  cannot detect a market that is wrong everywhere at once.
- The contract and publisher have not had an external audit.
- Tokens whose issuer file cannot be fetched (such as Matrixdock's XAUM) are left out on
  purpose.
- Sushi v3 quotes are sampled up to 80% of the pool's USDC; exit beyond that is not credited.
- Lender holdings cover pinned Blend v2 pools only; Templar, where most Stellar RWA
  borrowing sits, is not read.
