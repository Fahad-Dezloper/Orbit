# Live run: 21 Nov 2026 (SDEX order books + Soroban AMM pools)

Command: `orbit measure` against `https://horizon.stellar.org` and the Soroban RPC at
`https://mainnet.sorobanrpc.com`. Read-only: pools are read by simulation, nothing is signed
or submitted. All issuers passed SEP-1; all nine pinned pools passed on-chain token
verification. Full JSON: `2026-11-21-all-venues.json`.

```
XLM/USD 0.1256269   (SDEX order books + 9 pinned Soroban AMM pools)

ASSET      REFERENCE     EXIT @2%    EXIT @10%    EXIT @25%  VERDICT @10%
USTRY        $1.0791     $290,023     $381,836     $553,977  healthy
          SDEX/USDC                        $270,806 @10%
          aquarius/USDC CCX2TYR4           $106,505 @10%
          aquarius/XLM CACX5646                  $4 @10%
          soroswap/USDC CDIXSYDR             $4,518 @10%
          note: SDEX/XLM: own price 86.70% above the reference
CETES        $0.0642           $0     $352,896     $527,779  healthy
          SDEX/USDC                        $267,453 @10%
          aquarius/USDC CCKGQSQG            $85,441 @10%
          aquarius/USDC CBLPZUFE                 $0 @10%
          soroswap/USDC CAQJUHO6                 $0 @10%
          note: SDEX/XLM: own price 18.08% below the reference
          note: aquarius/USDC CCKGQSQG: own price 2.57% below the reference
TESOURO      $0.2158      $55,869      $60,335      $68,656  shallow
          SDEX/USDC                         $54,991 @10%
          aquarius/USDC CDPF7GMA             $5,344 @10%
          note: SDEX/XLM: own price 222.78% above the reference
MEX             none           $0           $0           $0  dead
          note: SDEX/USDC: bids but no asks, so no price of its own
          note: no venue has $100 of depth at its own price; no reference, counted as zero
CETESZ          none           $0           $0           $0  dead
          note: SDEX/USDC: bids but no asks, so no price of its own
          note: SDEX/XLM: bids but no asks, so no price of its own
          note: no venue has $100 of depth at its own price; no reference, counted as zero
EUROB           none           $0           $0           $0  dead
          note: SDEX/USDC: bids but no asks, so no price of its own
          note: SDEX/XLM: bids but no asks, so no price of its own
          note: no venue has $100 of depth at its own price; no reference, counted as zero
GILTS           none           $0           $0           $0  dead
          note: SDEX/USDC: bids but no asks, so no price of its own
          note: SDEX/XLM: bids but no asks, so no price of its own
          note: no venue has $100 of depth at its own price; no reference, counted as zero
KTB             none           $0           $0           $0  dead
          note: SDEX/USDC: bids but no asks, so no price of its own
          note: SDEX/XLM: bids but no asks, so no price of its own
          note: no venue has $100 of depth at its own price; no reference, counted as zero
MEXe            none           $0           $0           $0  dead
          note: SDEX/USDC: bids but no asks, so no price of its own
          note: SDEX/XLM: bids but no asks, so no price of its own
          note: no venue has $100 of depth at its own price; no reference, counted as zero
USDY         $1.1380      $28,322     $109,377     $261,382  healthy
          SDEX/USDC                          $2,177 @10%
          aquarius/USDC CAFHLHGZ           $107,199 @10%
          soroswap/XLM CA53URIS                  $0 @10%
          note: SDEX/XLM: own price 16.46% below the reference
```

## Reading it

- **Matches the research snapshot (19 Nov), now from Orbit's own code.** At 10%: USTRY
  $382K (research ~$380K), USDY $109K (~$102K), TESOURO $60K (~$58K). USDY moves from
  "critical" on the SDEX alone ($2.2K) to "healthy": its liquidity is in an Aquarius pool,
  which is why SDEX-only measurement is not enough.
- **CETES is lower than the research figure ($353K vs ~$379K)** because of the cross-venue
  reference. Its largest Aquarius pool sits 2.6% below the reference set by the venues
  together, so less of that pool counts inside each band. Under the old method each venue was
  measured from its own price and the pool vouched for itself.
- **The reference is set only by venues with real depth.** USTRY's SDEX/XLM book shows a
  price 87% above everyone else; it holds under $100, so it is reported but has no say in the
  reference. This is the signal a manipulation scorer looks for, produced as a by-product.
- **The six other Etherfuse tokens are still dead.** Their books have bids and no asks, and
  no venue anywhere has $100 of depth, so there is no trustworthy price and nothing counts.
- **Not yet measured:** Aquarius stable and concentrated pools (USDY has a concentrated pool
  holding about $280K of USDC; USTRY and CETES have small ones against sUSD), and pools
  against counters other than USDC and XLM. Leaving them out can only understate depth.
