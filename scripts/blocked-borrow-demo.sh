#!/usr/bin/env bash
# The demo in one run, on a LOCAL network, with live mainnet figures:
#   live measurement -> depth oracle -> a lending pool that checks depth before
#   accepting collateral -> deposits accepted and refused.
# Never writes to testnet or mainnet.
#
# Prerequisites:
#   stellar container start local
#   (cd contracts/depth-oracle && stellar contract build)
#   (cd contracts/demo-lender && stellar contract build)
#   cargo build --release -p pocky-api -p pocky-publisher
set -euo pipefail

NET=local
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
ORACLE_WASM=contracts/depth-oracle/target/wasm32v1-none/release/pocky_depth_oracle.wasm
LENDER_WASM=contracts/demo-lender/target/wasm32v1-none/release/pocky_demo_lender.wasm
for w in "$ORACLE_WASM" "$LENDER_WASM"; do [ -f "$w" ] || { echo "missing $w: build the contracts first"; exit 1; }; done

# Mainnet SAC addresses (derived and checked in pocky-ingest's tests).
USTRY=CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR
KTB=CBAECV6UVDS6ZKPMC63MUYY4V7Q3KTHR7NLUFGVVGK7K3AQT475QTETP
DEJAAA=CC64WBDGS6QQP22QTTIACYIXT3WF7BBQEYOQPLTP7GTKYY7PZ74QYGSL

usd() { python3 -I -c "import sys; print(int(round(float(sys.argv[1]) * 10**7)))" "$1"; }
fmt() { python3 -I -c "import sys; v=sys.argv[1].strip('\"'); print('none' if v in ('','null') else '\${:,.0f}'.format(int(v)/1e7))" "$1"; }

echo "== local network and accounts"
exists() { # is the account on the local ledger? (the CLI can report success when it is not)
  python3 -I -c "import sys,urllib.request; urllib.request.urlopen('http://localhost:8000/accounts/'+sys.argv[1], timeout=5)" \
    "$(stellar keys address "$1")" >/dev/null 2>&1
}
fund() { # the local friendbot takes a while after the container starts
  stellar keys address "$1" >/dev/null 2>&1 || stellar keys generate "$1" >/dev/null 2>&1
  for _ in $(seq 1 60); do
    stellar keys fund "$1" --network "$NET" >/dev/null 2>&1 || true
    exists "$1" && return 0
    sleep 3
  done
  echo "could not fund $1 on the local network"; exit 1
}
for id in pocky-admin pocky-publisher pocky-alice pocky-bob; do fund "$id"; done

echo "== deploy the depth oracle and a lending pool that reads it"
ORACLE=$(stellar contract deploy --wasm "$ORACLE_WASM" --source pocky-admin --network "$NET" \
  -- --admin "$(stellar keys address pocky-admin)" --publisher "$(stellar keys address pocky-publisher)" 2>/dev/null)
# Collateral must be sellable within 10%; half of the measured depth is never lent against;
# depth older than 1h is not trusted.
POOL=$(stellar contract deploy --wasm "$LENDER_WASM" --source pocky-admin --network "$NET" \
  -- --oracle "$ORACLE" --band_bps 1000 --safety_bps 5000 --max_age_secs 3600 2>/dev/null)
echo "oracle $ORACLE"
echo "pool   $POOL"

echo "== measure mainnet now (read-only) and publish to the local oracle"
# A fresh store holding just this measurement. The real store's 24h window refuses to
# publish across gaps over 2h (for example a laptop that slept), which is the point of it.
DEMO_DATA="$(mktemp -d)"
trap 'rm -rf "$DEMO_DATA"' EXIT
./target/release/pocky-api --once --data-dir "$DEMO_DATA" >/dev/null
# Local network only: accept a one-sample window; every record carries its sample count.
./target/release/pocky-publish --network "$NET" --oracle "$ORACLE" --source pocky-publisher \
  --data-dir "$DEMO_DATA" --min-samples 1 | grep -c '^published' | xargs -I{} echo "published {} batches"

pool() { stellar contract invoke --id "$POOL" --network "$NET" "$@" 2>&1; }
read_only() { pool --source pocky-bob --send=no -- "$@" | tail -1; }
try_deposit() { # label user asset usd
  local out
  if out=$(pool --source "$2" -- deposit --user "$(stellar keys address "$2")" --asset "$3" --value_usd "$(usd "$4")"); then
    printf "  ACCEPTED  %-46s\n" "$1"
  else
    case "$out" in
      *"#2"*) why="DepthCapExceeded: the market cannot absorb it" ;;
      *"#1"*) why="NoFreshDepth: no depth data, no collateral" ;;
      *) why="$(echo "$out" | tail -1)" ;;
    esac
    printf "  REFUSED   %-46s %s\n" "$1" "$why"
  fi
}

echo
echo "== what the pool sees (cap = depth within 10% x 50%)"
echo "  USTRY  cap $(fmt "$(read_only cap --asset "$USTRY")")"
echo "  KTB    cap $(fmt "$(read_only cap --asset "$KTB")")"
echo "  deJAAA cap $(fmt "$(read_only cap --asset "$DEJAAA")")   (no market on Stellar; \$3.56M lent against it elsewhere)"

echo
echo "== deposits"
try_deposit "alice: \$150,000 of USTRY"                    pocky-alice "$USTRY" 150000
try_deposit "bob:   \$100,000 of USTRY (pool would exceed)" pocky-bob   "$USTRY" 100000
try_deposit "bob:   \$20,000 of USTRY"                     pocky-bob   "$USTRY" 20000
try_deposit "alice: \$1,000 of KTB (priced, unsellable)"   pocky-alice "$KTB"   1000
try_deposit "alice: \$1,000 of deJAAA"                     pocky-alice "$DEJAAA" 1000

echo
echo "== pool totals"
echo "  USTRY collateral $(fmt "$(read_only collateral --asset "$USTRY")") of cap $(fmt "$(read_only cap --asset "$USTRY")")"
echo "  KTB   collateral $(fmt "$(read_only collateral --asset "$KTB")")"
