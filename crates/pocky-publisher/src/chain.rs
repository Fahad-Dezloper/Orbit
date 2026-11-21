//! Talking to the oracle: reading what it holds (simulation, no signature) and submitting a
//! batch (signed by a `stellar` CLI identity, so no secret key ever passes through this
//! program, its arguments or its config).
//!
//! Mainnet must be asked for explicitly (`allow_mainnet`), because the contract has not yet
//! had an external review. On mainnet the 24h window must have its full sample count.

use std::process::Command;

use pocky_core::Band;
use pocky_ingest::soroban::{parse_contract, SorobanRpc};
use pocky_ingest::IngestError;
use stellar_xdr::{ContractId, Hash, ScAddress, ScSymbol, ScVal};

use crate::OracleAsset;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    /// `stellar container start local` on this machine.
    Local,
    /// Stellar's public test network.
    Testnet,
    /// Stellar mainnet. Only with explicit acknowledgement that the oracle is unaudited.
    Mainnet,
}

impl Network {
    pub fn parse(s: &str, allow_mainnet: bool) -> Result<Network, String> {
        match s {
            "local" => Ok(Network::Local),
            "testnet" => Ok(Network::Testnet),
            "mainnet" if allow_mainnet => Ok(Network::Mainnet),
            "mainnet" => Err(
                "the oracle has not had an external review; pass --mainnet-unaudited to publish \
                 to mainnet anyway"
                    .into(),
            ),
            other => Err(format!(
                "unknown network {other:?}; use local, testnet or mainnet"
            )),
        }
    }

    /// Name of the network in the `stellar` CLI config.
    pub fn cli_name(self) -> &'static str {
        match self {
            Network::Local => "local",
            Network::Testnet => "testnet",
            Network::Mainnet => "mainnet",
        }
    }

    pub fn passphrase(self) -> &'static str {
        match self {
            Network::Local => "Standalone Network ; February 2017",
            Network::Testnet => "Test SDF Network ; September 2015",
            Network::Mainnet => pocky_ingest::soroban::MAINNET_PASSPHRASE,
        }
    }

    pub fn default_rpc(self) -> &'static str {
        match self {
            Network::Local => "http://localhost:8000/rpc",
            Network::Testnet => "https://soroban-testnet.stellar.org",
            Network::Mainnet => pocky_ingest::soroban::DEFAULT_MAINNET_RPC,
        }
    }
}

fn asset_val(a: &OracleAsset) -> Result<ScVal, IngestError> {
    let id = parse_contract(&a.sac)?;
    let tag = ScSymbol(
        "Stellar"
            .try_into()
            .map_err(|_| IngestError::Registry("tag".into()))?,
    );
    let items = vec![
        ScVal::Symbol(tag),
        ScVal::Address(ScAddress::Contract(ContractId(Hash(id)))),
    ];
    Ok(ScVal::Vec(Some(items.try_into().map_err(|_| {
        IngestError::Registry("asset value".into())
    })?)))
}

/// Timestamp of the record the oracle holds for (asset, band), if any. Read by simulation.
pub fn onchain_ts(
    rpc: &SorobanRpc,
    oracle: &str,
    asset: &OracleAsset,
    band: Band,
) -> Result<Option<u64>, IngestError> {
    let v = rpc.read(
        oracle,
        "depth",
        vec![asset_val(asset)?, ScVal::U32(band.bps())],
    )?;
    match v {
        ScVal::Void => Ok(None),
        ScVal::Map(Some(m)) => {
            for e in m.iter() {
                if let (ScVal::Symbol(k), ScVal::U64(ts)) = (&e.key, &e.val) {
                    if k.to_string() == "timestamp" {
                        return Ok(Some(*ts));
                    }
                }
            }
            Err(IngestError::BadResponse {
                url: oracle.into(),
                reason: "depth record has no timestamp".into(),
            })
        }
        other => Err(IngestError::BadResponse {
            url: oracle.into(),
            reason: format!("unexpected depth value {other:?}"),
        }),
    }
}

/// Submits one `publish` batch, signed by the `stellar` CLI identity `source`.
/// Returns the CLI's output (the transaction result) on success.
pub fn submit(
    network: Network,
    rpc_url: &str,
    oracle: &str,
    source: &str,
    updates_json: &str,
) -> Result<String, String> {
    parse_contract(oracle).map_err(|e| e.to_string())?;
    if source.is_empty()
        || source.starts_with('-')
        || !source
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(format!("invalid identity name {source:?}"));
    }
    let out = Command::new("stellar")
        .args([
            "contract",
            "invoke",
            "--id",
            oracle,
            "--source-account",
            source,
            "--rpc-url",
            rpc_url,
            "--network-passphrase",
            network.passphrase(),
            "--send=yes",
            "--",
            "publish",
            "--updates",
            updates_json,
        ])
        .output()
        .map_err(|e| format!("cannot run the stellar CLI: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_needs_explicit_acknowledgement() {
        assert!(Network::parse("mainnet", false)
            .unwrap_err()
            .contains("--mainnet-unaudited"));
        assert_eq!(Network::parse("mainnet", true).unwrap(), Network::Mainnet);
        assert_eq!(Network::parse("testnet", false).unwrap(), Network::Testnet);
        assert!(Network::parse("futurenet", true).is_err());
    }

    #[test]
    fn rejects_identity_names_that_could_be_flags() {
        let r = submit(
            Network::Local,
            "http://localhost:8000/rpc",
            "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA",
            "--help",
            "[]",
        );
        assert!(r.unwrap_err().contains("invalid identity"));
        let r = submit(Network::Local, "http://x", "not-a-contract", "pub", "[]");
        assert!(r.is_err());
    }
}
