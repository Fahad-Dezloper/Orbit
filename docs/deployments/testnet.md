# Testnet deployment

Stellar testnet, deployed 21 Nov 2026. Public test network only; no mainnet state.

| | |
| --- | --- |
| Oracle contract | `CCOI2QVXHQIX7ZADUT5LSJPDAQSKPEINUK6HCMCSCZTWLB5BK7W4CBUQ` |
| Explorer | https://stellar.expert/explorer/testnet/contract/CCOI2QVXHQIX7ZADUT5LSJPDAQSKPEINUK6HCMCSCZTWLB5BK7W4CBUQ |
| Wasm SHA-256 | `5471c671be9028d006eda8bc3bfc547c9181cb7f5783759a7d72c5837f28fe3b` |
| Admin | `GDHB4MQVWS7FMWFFDOUETCP47RFMDUKAZLX2PPYV6DIUTV6HS2S6M62R` |
| Publisher | `GCVJPQMM4FRSAB6E6FWAC6BVDDFL6WXQH5WE6AP5674L5EMASNIQ4AZ4` |

The figures it holds are **live mainnet measurements**, published to testnet so integrators
can build against real numbers without touching mainnet. Assets are named by their
**mainnet** SAC address.

## Reading it

```bash
# USTRY at the 10% band. Any account can read; nothing is signed.
stellar contract invoke --id CCOI2QVXHQIX7ZADUT5LSJPDAQSKPEINUK6HCMCSCZTWLB5BK7W4CBUQ \
  --source <any-identity> --network testnet --send=no \
  -- depth --asset '{"Stellar":"CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR"}' --band_bps 1000
```

From a Soroban contract, call `depth(asset, band_bps)` (or `fresh_depth(asset, band_bps,
max_age_secs)`) and compare the result with the collateral you are about to accept.
`exit_usd` has 7 decimals; reject the record if `timestamp` is older than you tolerate or
`samples` is below your minimum.

## First publish (21 Nov 2026)

| Asset | SAC (mainnet) | Exit @10% | Samples |
| --- | --- | ---: | ---: |
| USTRY | `CBLV4ATS…OPNUR` | $370,374.52 | 3 |
| USDY | `CB3YA656…YGAGP` | $103,017.06 | 3 |
| KTB | `CBAECV6U…QTETP` | $0 | 3 |

All ten registered assets at all three bands were published (30 updates, 3 transactions).
The window held only 3 samples at first publish because history had just started; the
publisher ran with `--min-samples 1` on testnet, and each record shows its sample count.
Once the API has measured for 6 hours, publishing uses the normal minimum of 12.
