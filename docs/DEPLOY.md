# Deploying

Three pieces run in production. Only the publisher holds a key.

| Piece | Runs where | Holds keys | Talks to |
| --- | --- | --- | --- |
| `pocky-api` | any small server or container host | no | Horizon, Soroban RPC (read-only) |
| `pocky-publish` | a machine with a `stellar` CLI keystore | the publisher key only | the snapshot store, the oracle |
| `web/` | any Node host | no | `pocky-api` |

## 1. API (measurement + history)

```bash
docker build -t pocky-api .
docker run -d --name pocky-api -p 8080:8080 -v pocky-data:/data pocky-api
```

- Measures every 30 minutes into `/data` and serves `GET /v1/...`. Keep the volume: the 24h
  windowed minimum needs at least 12 samples, so a fresh store takes about 6 hours to fill.
- Put it behind HTTPS and a reverse proxy with rate limits (the host's TLS, Caddy or nginx).
  The API is GET-only and public, and has no rate limiting of its own.
- `docker run ... pocky-api --once` takes a single measurement, useful to check egress.

## 2. Publisher

Runs next to the snapshot store (same machine or a shared volume).

```bash
stellar keys generate pocky-testnet-publisher --network testnet --fund
pocky-publish --network testnet --oracle <ORACLE_ID> --source pocky-testnet-publisher \
  --data-dir /data --every-secs 1800
```

- The key lives in the `stellar` CLI keystore and never in this repository, its config or
  the process arguments.
- Run the publisher as its own OS user with read-only access to the snapshot store: anyone
  who can write the store can make it sign false figures.
- Publishes only windows with enough samples, never replays a timestamp the oracle already
  holds, and publishes nothing if it cannot read the oracle first.
- `--min-samples` below 12 is for test networks while history builds; every record carries
  its sample count.
- Mainnet requires `--mainnet-unaudited` until the external audit (`PLAN.md` 5.1c), and is
  never published with fewer than 12 samples.

## 3. Dashboard

```bash
cd web && npm ci && npm run build
POCKY_API_URL=https://<your-api-host> npm start
```

## Rotating the publisher key

```bash
stellar keys generate pocky-testnet-publisher-2 --network testnet --fund
stellar contract invoke --id <ORACLE_ID> --source <admin-identity> --network testnet \
  -- set_publisher --publisher "$(stellar keys address pocky-testnet-publisher-2)"
```

The old key stops working immediately; only the admin can rotate.
