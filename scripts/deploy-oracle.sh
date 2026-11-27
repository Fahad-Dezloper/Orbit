#!/usr/bin/env bash
# Deploy your own depth oracle.
#
#   scripts/deploy-oracle.sh --network testnet
#   scripts/deploy-oracle.sh --network local
#   scripts/deploy-oracle.sh --network testnet --admin my-admin --publisher my-publisher
#
# Uses identities from your stellar CLI keystore; creates (and on test networks funds) any that
# do not exist. Keys never leave the keystore. Prints the oracle address and the lines to put
# in .env. Mainnet is refused: the contract has not had an external review yet.
set -euo pipefail

NET=""
ADMIN=orbit-admin
PUBLISHER=orbit-publisher
while [ $# -gt 0 ]; do
  case "$1" in
    --network) NET="$2"; shift 2 ;;
    --admin) ADMIN="$2"; shift 2 ;;
    --publisher) PUBLISHER="$2"; shift 2 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown argument $1"; exit 1 ;;
  esac
done
[ -n "$NET" ] || { echo "usage: $0 --network local|testnet|<network name>"; exit 1; }
case "$NET" in
  mainnet|pubnet) echo "refusing mainnet: the oracle has not had an external review (docs/PLAN.md 5.1c)"; exit 1 ;;
esac
command -v stellar >/dev/null || { echo "install the stellar CLI first"; exit 1; }

# What the network is, from its passphrase rather than its name.
INFO="$(stellar network ls --long 2>/dev/null | awk -v n="$NET" '$0=="Name: "n{f=1;next} /^Name: /{f=0} f')"
PASSPHRASE="$(printf '%s\n' "$INFO" | sed -n 's/^Network passphrase: //p')"
RPC_URL="$(printf '%s\n' "$INFO" | sed -n 's/^RPC url: //p')"
case "$PASSPHRASE" in
  "Standalone Network ; February 2017") KIND=local ;;
  "Test SDF Network ; September 2015") KIND=testnet ;;
  "Public Global Stellar Network ; September 2015")
    echo "refusing mainnet: the oracle has not had an external review (docs/PLAN.md 5.1c)"; exit 1 ;;
  "") echo "unknown network $NET (see: stellar network ls)"; exit 1 ;;
  *) echo "unsupported network passphrase: $PASSPHRASE"; exit 1 ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASM="$ROOT/contracts/depth-oracle/target/wasm32v1-none/release/orbit_depth_oracle.wasm"

echo "== build the contract"
(cd "$ROOT/contracts/depth-oracle" && stellar contract build >/dev/null)

ensure_identity() {
  if ! stellar keys address "$1" >/dev/null 2>&1; then
    echo "creating identity $1"
    stellar keys generate "$1" --network "$NET" --fund >/dev/null
  else
    stellar keys fund "$1" --network "$NET" >/dev/null 2>&1 || true
  fi
}
echo "== identities"
ensure_identity "$ADMIN"
ensure_identity "$PUBLISHER"
ADMIN_ADDR="$(stellar keys address "$ADMIN")"
PUB_ADDR="$(stellar keys address "$PUBLISHER")"
echo "admin     $ADMIN ($ADMIN_ADDR)"
echo "publisher $PUBLISHER ($PUB_ADDR)"
[ "$ADMIN_ADDR" != "$PUB_ADDR" ] || { echo "use different keys for admin and publisher"; exit 1; }

echo "== deploy to $NET"
ORACLE="$(stellar contract deploy --wasm "$WASM" --source "$ADMIN" --network "$NET" \
  -- --admin "$ADMIN_ADDR" --publisher "$PUB_ADDR")"
echo "oracle $ORACLE"

echo "== check"
got="$(stellar contract invoke --id "$ORACLE" --source "$PUBLISHER" --network "$NET" --send=no -- publisher)"
[ "$got" = "\"$PUB_ADDR\"" ] || { echo "unexpected publisher $got"; exit 1; }
echo "ok: publisher is $PUB_ADDR"

cat <<EOF

Put these in .env:
  ORBIT_NETWORK=$KIND
  ORBIT_ORACLE=$ORACLE
  ORBIT_PUBLISHER_ADDRESS=$PUB_ADDR
  ORBIT_RPC=$RPC_URL
EOF
[ "$KIND" = local ] && cat <<EOF
  ORBIT_MIN_SAMPLES=1
  (inside Docker, reach a network on this machine at host.docker.internal, not localhost)
EOF
cat <<EOF

Then hand the publisher key to Docker as a secret file (keep it out of git):
  mkdir -p secrets && stellar keys show $PUBLISHER > secrets/publisher.key && chmod 600 secrets/publisher.key
  docker compose --profile publisher up -d
EOF
