#!/bin/sh
# Starts orbit-publish inside the publisher container.
#
# The key comes from the Docker secret file (default /run/secrets/orbit_publisher_key). It is
# copied into a stellar CLI identity that lives only in this container's home directory,
# checked against ORBIT_PUBLISHER_ADDRESS if set, and never printed or passed as an argument.
set -eu

: "${ORBIT_NETWORK:?set ORBIT_NETWORK (local, testnet or mainnet)}"
: "${ORBIT_ORACLE:?set ORBIT_ORACLE to your depth oracle contract}"
KEY_FILE="${ORBIT_PUBLISHER_KEY_FILE:-/run/secrets/orbit_publisher_key}"
EVERY="${ORBIT_PUBLISH_EVERY_SECS:-1800}"
ID=orbit-publisher

[ -r "$KEY_FILE" ] || { echo "no publisher key at $KEY_FILE (see docs/DEPLOY.md)"; exit 1; }

umask 077
mkdir -p "$HOME/.config/stellar/identity"
key="$(tr -d ' \r\n' < "$KEY_FILE")"
case "$key" in
  S*) ;;
  *) echo "the publisher key file must hold a secret key starting with S"; exit 1 ;;
esac
printf 'secret_key = "%s"\n' "$key" > "$HOME/.config/stellar/identity/$ID.toml"
unset key

got="$(stellar keys address "$ID")"
if [ -n "${ORBIT_PUBLISHER_ADDRESS:-}" ] && [ "$got" != "$ORBIT_PUBLISHER_ADDRESS" ]; then
  echo "publisher key is for $got, expected $ORBIT_PUBLISHER_ADDRESS"
  exit 1
fi
echo "publishing as $got to $ORBIT_ORACLE on $ORBIT_NETWORK every ${EVERY}s"

set -- --network "$ORBIT_NETWORK" --oracle "$ORBIT_ORACLE" --source "$ID" \
  --data-dir /data --registry /etc/orbit/assets.toml --every-secs "$EVERY" "$@"
[ -n "${ORBIT_RPC:-}" ] && set -- "$@" --rpc "$ORBIT_RPC"
[ -n "${ORBIT_MIN_SAMPLES:-}" ] && set -- "$@" --min-samples "$ORBIT_MIN_SAMPLES"
[ "${ORBIT_MAINNET_UNAUDITED:-}" = "yes" ] && set -- "$@" --mainnet-unaudited
exec orbit-publish "$@"
