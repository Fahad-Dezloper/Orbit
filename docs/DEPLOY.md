# Deploying

Three pieces run in production. Only the publisher holds a key.

| Piece | Runs where | Holds keys | Talks to |
| --- | --- | --- | --- |
| `orbit-api` | any small server or container host | no | Horizon, Soroban RPC (read-only) |
| `orbit-publish` | a machine with a `stellar` CLI keystore | the publisher key only | the snapshot store, the oracle |
| `web/` | any Node host | no | `orbit-api` |

## 0. The public deployment (GitHub Actions and Pages)

The live instance runs from `.github/workflows/measure.yml`, with no server:

- Every half hour (plus a random delay of up to 7 minutes) a workflow runs
  `orbit-api --once --export-dir site`.
- Each run is appended to the history on the `data` branch, one JSONL file per UTC day,
  append-only.
- The windowed figures go to the **testnet** oracle. The key is the repository secret
  `ORBIT_TESTNET_PUBLISHER_SECRET`: testnet only, written to a temporary keystore file and
  deleted after use. A mainnet key never reaches CI.
- The docs (Mintlify static export) are served at https://fahad-dezloper.github.io/Orbit/, the
  dashboard (built statically from the exported files) at `/Orbit/dashboard/`, and the static
  API at `/Orbit/v1/`.

Static API: `https://fahad-dezloper.github.io/Orbit/v1/latest.json`, `v1/lenders.json`,
`v1/assets/{code}.json`, `v1/assets/{code}/history.json` (10% band, 24h) and
`v1/assets/{code}/cap.json` (5% bonus, 50% margin). `index.json` lists every file with
the status the live API returned. A static file cannot send a status, so a file may hold
`{"error": ...}`. Check for it, and check `measured_at`, before using a figure.

Limits: GitHub may delay or skip scheduled runs under load. A gap of more than 2 hours
invalidates the 24h window and publishing stops until it clears (fail closed).

To set the publisher secret from your local keystore (the value is never printed):

```bash
stellar keys show orbit-testnet-publisher | gh secret set ORBIT_TESTNET_PUBLISHER_SECRET -R Fahad-Dezloper/Orbit
```

## 1. API (measurement + history)

```bash
docker build -t orbit-api .
docker run -d --name orbit-api -p 8080:8080 -v orbit-data:/data orbit-api
```

- Measures every 30 minutes into `/data` and serves `GET /v1/...`. Keep the volume: the 24h
  windowed minimum needs at least 12 samples, so a fresh store takes about 6 hours to fill.
- Put it behind HTTPS and a reverse proxy with rate limits (the host's TLS, Caddy or nginx).
  The API is GET-only and public, and has no rate limiting of its own.
- `docker run ... orbit-api --once` takes a single measurement, useful to check egress.

## 2. Publisher

Runs next to the snapshot store (same machine or a shared volume).

```bash
stellar keys generate orbit-testnet-publisher --network testnet --fund
orbit-publish --network testnet --oracle <ORACLE_ID> --source orbit-testnet-publisher \
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
ORBIT_API_URL=https://<your-api-host> npm start
```

## Rotating the publisher key

```bash
stellar keys generate orbit-testnet-publisher-2 --network testnet --fund
stellar contract invoke --id <ORACLE_ID> --source <admin-identity> --network testnet \
  -- set_publisher --publisher "$(stellar keys address orbit-testnet-publisher-2)"
```

The old key stops working immediately; only the admin can rotate.
