# Security policy

Orbit's figures may be used to size lending limits, so a bug that makes a figure too high is a
security issue, not just a correctness bug.

## Reporting a vulnerability

Please report privately through GitHub: **Security → Report a vulnerability** on
[github.com/Fahad-Dezloper/Orbit](https://github.com/Fahad-Dezloper/Orbit/security). Do not open
a public issue or pull request for it.

Include what you found, how to reproduce it (a local network or a unit test, never mainnet or
public testnet), and the impact you expect. We aim to acknowledge reports within 3 days.

## In scope

- Anything that makes Orbit publish a figure higher than the market could absorb, or publish
  when it should fail closed.
- Ways to make Orbit measure an impostor token or pool.
- The depth oracle contract (`contracts/depth-oracle`) and the publisher (`crates/orbit-publisher`).
- Key handling in the publisher, the Docker images and the GitHub workflow.

## Known limits

Read [`docs/THREATS.md`](docs/THREATS.md) first: it lists the attacks Orbit does not yet stop
(for example patient capital parked for a week). Reports that extend or quantify those are
welcome.

## Status

The contracts have had an internal review (`docs/reviews/`) but **no external audit yet**. The
oracle runs on testnet only; the publisher refuses mainnet without an explicit flag. The design
and threat model are in [`docs/SECURITY.md`](docs/SECURITY.md).
