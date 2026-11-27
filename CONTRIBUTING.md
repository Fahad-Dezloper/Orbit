# Contributing to Orbit

Thanks for helping. Orbit publishes a number other protocols may use to decide how much to
lend, so correctness comes before features. Small, well-tested pull requests are easiest to
review.

## Ways to help

- **Add a token or a pool.** The most useful contribution. See "Adding a token" below.
- **Report a wrong figure.** Open an issue with the token, the band, what Orbit says, and how
  you checked it (a quote from the venue, a transaction, a book snapshot).
- **Add a venue.** A new AMM or lender on Stellar. Open an issue first so we can agree how its
  state is verified.
- **Docs.** The docs live in `docs-site/` (Mintlify) and `docs/` (design notes).

## Setting up

```bash
git clone https://github.com/Fahad-Dezloper/Orbit.git && cd Orbit
cargo test --workspace
cargo run -p orbit-cli -- measure --asset USTRY     # read-only against mainnet
```

The toolchain is pinned in `rust-toolchain.toml`. The contracts are separate workspaces:

```bash
(cd contracts/depth-oracle && cargo test)
(cd contracts/demo-lender && cargo test)
```

Dashboard: `cd web && npm ci && npm run dev` (needs an API at `ORBIT_API_URL`, default
`http://127.0.0.1:8080`). Docs: `cd docs-site && npx mint dev`.

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd web && npx tsc --noEmit)
```

CI runs the same checks.

## Rules that keep the figures honest

- **No floats in `orbit-core`.** Use `Fx` (7-decimal fixed point). Round in the direction that
  understates depth.
- **Fail closed.** Missing, stale or inconsistent data means no figure, never an old or guessed
  one.
- **Nothing on trust.** A token is measured only if its issuer's own `stellar.toml` lists it. A
  pool is used only if its tokens match addresses derived locally and its reserves match what the
  token contracts say it holds.
- **Read-only.** Ingestion never signs or submits. Only `orbit-publish` signs, and only `publish`
  calls to its own oracle. Never add code that writes to mainnet.
- **No keys in the repository**, its config, logs or process arguments.
- **Tests for every behaviour change**, including the case that must fail.

## Adding a token

1. Find the token in its issuer's own `stellar.toml` (SEP-1). Never take an issuer from Horizon's
   asset list alone: it lists impostors with the same codes.
2. Add an `[[asset]]` entry to `assets.toml` with `code`, `issuer` (or `contract` for a
   Soroban-only token) and `home_domain`.
3. Find its pools (Aquarius router `get_pools_for_tokens_range`, Soroswap factory `get_pair`,
   Sushi's pool lens) and pin each as a `[[pool]]` entry.
4. Run `cargo run -p orbit-cli -- measure --asset CODE` and check that no venue is "left out".
5. Say in the pull request where you confirmed the issuer and each pool.

## Security issues

Do not open a public issue. See [SECURITY.md](SECURITY.md).

## License

By contributing you agree that your contributions are licensed under the Apache License 2.0.
