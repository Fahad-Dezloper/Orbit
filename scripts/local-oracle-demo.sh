#!/usr/bin/env bash
# Deploys the depth oracle to a LOCAL Stellar network and exercises it end to end.
# Never points at testnet or mainnet: the network is hard-coded to `local`.
#
# Prerequisites:
#   stellar container start local      # Docker quickstart on localhost:8000
#   (cd contracts/depth-oracle && stellar contract build)
set -euo pipefail

NET=local
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASM="$ROOT/contracts/depth-oracle/target/wasm32v1-none/release/orbit_depth_oracle.wasm"

[ -f "$WASM" ] || { echo "build the contract first: cd contracts/depth-oracle && stellar contract build"; exit 1; }

# Throwaway identities, funded by the local friendbot.
for id in orbit-admin orbit-publisher orbit-stranger; do
  stellar keys generate "$id" --network "$NET" --fund --overwrite >/dev/null 2>&1
done
ADMIN=$(stellar keys address orbit-admin)
PUBLISHER=$(stellar keys address orbit-publisher)

echo "== deploy"
ID=$(stellar contract deploy --wasm "$WASM" --source orbit-admin --network "$NET" \
  -- --admin "$ADMIN" --publisher "$PUBLISHER")
echo "contract $ID"

call() { stellar contract invoke --id "$ID" --network "$NET" "$@"; }
NOW=$(date +%s)

update() { # asset exit_usd_scaled timestamp
  printf '[{"asset":{"Other":"%s"},"band_bps":1000,"exit_usd":"%s","timestamp":%s,"samples":24}]' "$1" "$2" "$3"
}

echo "== publish live figures from the 20 Nov run (10%% band, 7 decimals)"
call --source orbit-publisher -- publish --updates "$(update USTRY 2710860000000 $((NOW - 30)))" >/dev/null
call --source orbit-publisher -- publish --updates "$(update KTB 0 $((NOW - 30)))" >/dev/null

echo "== read back"
echo "USTRY: $(call --source orbit-stranger --send=no -- depth --asset '{"Other":"USTRY"}' --band_bps 1000)"
echo "KTB:   $(call --source orbit-stranger --send=no -- depth --asset '{"Other":"KTB"}' --band_bps 1000)"
echo "assets: $(call --source orbit-stranger --send=no -- assets)"

expect_fail() {
  local label="$1"; shift
  if "$@" >/dev/null 2>&1; then echo "FAIL: $label was accepted"; exit 1; else echo "ok: $label rejected"; fi
}

echo "== attacks that must fail"
expect_fail "publish signed by a stranger" \
  call --source orbit-stranger -- publish --updates "$(update USTRY 99990000000000 $((NOW - 20)))"
expect_fail "publish signed by the admin (wrong role)" \
  call --source orbit-admin -- publish --updates "$(update USTRY 99990000000000 $((NOW - 20)))"
expect_fail "replay of an older timestamp" \
  call --source orbit-publisher -- publish --updates "$(update USTRY 99990000000000 $((NOW - 60)))"
expect_fail "future-dated measurement" \
  call --source orbit-publisher -- publish --updates "$(update USTRY 1 $((NOW + 3600)))"
expect_fail "publisher rotating itself" \
  call --source orbit-publisher -- set_publisher --publisher "$PUBLISHER"

echo "USTRY after attacks: $(call --source orbit-stranger --send=no -- depth --asset '{"Other":"USTRY"}' --band_bps 1000)"
echo "done"
