#!/usr/bin/env bash
# End to end on a LOCAL network: live mainnet measurement -> snapshot store -> publisher ->
# depth oracle -> read back by an unrelated account. Never touches testnet or mainnet state.
#
# Prerequisites:
#   stellar container start local
#   (cd contracts/depth-oracle && stellar contract build)
#   cargo build --release -p pocky-api -p pocky-publisher
set -euo pipefail

NET=local
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
WASM=contracts/depth-oracle/target/wasm32v1-none/release/pocky_depth_oracle.wasm
[ -f "$WASM" ] || { echo "build the contract first"; exit 1; }

echo "== waiting for the local network"
for _ in $(seq 1 60); do
  if stellar keys generate pocky-admin --network "$NET" --fund --overwrite >/dev/null 2>&1; then break; fi
  sleep 3
done
for id in pocky-publisher pocky-stranger; do
  stellar keys generate "$id" --network "$NET" --fund --overwrite >/dev/null
done

echo "== deploy"
ORACLE=$(stellar contract deploy --wasm "$WASM" --source pocky-admin --network "$NET" \
  -- --admin "$(stellar keys address pocky-admin)" --publisher "$(stellar keys address pocky-publisher)")
echo "oracle $ORACLE"

echo "== measure mainnet now (read-only) into the store"
./target/release/pocky-api --once

# The 24h window normally needs 12 samples. On a test network we accept fewer while
# history builds; each record carries its sample count so a reader can see that.
PUB=(./target/release/pocky-publish --network "$NET" --oracle "$ORACLE" --source pocky-publisher --min-samples 1)

echo "== publish"
"${PUB[@]}"

echo "== read back as an unrelated account"
# Mainnet SAC addresses (derived and checked in pocky-ingest's soroban tests).
USTRY_SAC=CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR
KTB_SAC=CBAECV6UVDS6ZKPMC63MUYY4V7Q3KTHR7NLUFGVVGK7K3AQT475QTETP
read_depth() {
  stellar contract invoke --id "$ORACLE" --source pocky-stranger --network "$NET" --send=no \
    -- depth --asset "{\"Stellar\":\"$1\"}" --band_bps 1000
}
echo "USTRY @10%: $(read_depth "$USTRY_SAC")"
echo "KTB   @10%: $(read_depth "$KTB_SAC")"

echo "== publishing again with no new measurement must be a no-op"
"${PUB[@]}" | grep -q "nothing new to publish" && echo "ok: no replay"

echo "== the publisher refuses mainnet without the explicit flag"
if ./target/release/pocky-publish --network mainnet --oracle "$ORACLE" --source pocky-publisher 2>/dev/null; then
  echo "FAIL: mainnet accepted"; exit 1
else
  echo "ok: mainnet refused"
fi
echo "done"
