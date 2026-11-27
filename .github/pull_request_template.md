## What this changes

## How it was checked

- [ ] `cargo fmt`, `cargo clippy -D warnings` and `cargo test --workspace` pass
- [ ] Tests cover the new behaviour, including the case that must fail
- [ ] No floats in `orbit-core`; rounding understates depth
- [ ] No keys, no mainnet writes
- [ ] For a new token or pool: where the issuer and each pool were confirmed
