//! Read-only Soroban RPC client.
//!
//! Contract state is read by **simulating** a call (`simulateTransaction`). A simulation is
//! never signed and never submitted, so nothing here can move funds or change state. The
//! simulated transaction uses an all-zero source account that cannot sign anything.
//!
//! Token identity is checked from first principles: the contract id of a classic asset's
//! Stellar Asset Contract (SAC) is a hash of the network id and the asset, so we derive it
//! locally and never trust a pool, an API or a registry to tell us which token it holds.

use std::sync::{Condvar, Mutex};
use std::thread::sleep;
use std::time::Duration;

use pocky_core::Fx;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use stellar_xdr::{
    AccountId, AlphaNum12, AlphaNum4, Asset as XdrAsset, AssetCode12, AssetCode4, ContractId,
    ContractIdPreimage, Hash, HashIdPreimage, HashIdPreimageContractId, HostFunction,
    InvokeContractArgs, InvokeHostFunctionOp, Limits, Memo, MuxedAccount, Operation, OperationBody,
    Preconditions, PublicKey, ReadXdr, ScAddress, ScSymbol, ScVal, SequenceNumber, Transaction,
    TransactionEnvelope, TransactionExt, TransactionV1Envelope, Uint256, WriteXdr,
};

use crate::error::IngestError;
use crate::registry::Asset;

pub const MAINNET_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";
pub const DEFAULT_MAINNET_RPC: &str = "https://mainnet.sorobanrpc.com";

const ATTEMPTS: u32 = 5;
/// Most requests in flight to one RPC endpoint at a time. Public endpoints rate-limit
/// bursts; a small cap keeps a parallel run fast without tripping them.
pub const MAX_IN_FLIGHT: usize = 4;
const TIMEOUT: Duration = Duration::from_secs(20);

/// A token as seen by Soroban: native XLM or a classic credit asset, both through their SAC.
#[derive(Clone, Copy, Debug)]
pub enum Token<'a> {
    Native,
    Credit(&'a Asset),
}

/// `sha256(passphrase)`: the network id every contract id and signature is bound to.
pub fn network_id(passphrase: &str) -> [u8; 32] {
    Sha256::digest(passphrase.as_bytes()).into()
}

fn bad(what: &str, reason: impl ToString) -> IngestError {
    IngestError::BadResponse {
        url: what.to_string(),
        reason: reason.to_string(),
    }
}

fn padded<const N: usize>(code: &str) -> Result<[u8; N], IngestError> {
    let b = code.as_bytes();
    if b.is_empty() || b.len() > N {
        return Err(IngestError::Registry(format!(
            "asset code {code:?} does not fit"
        )));
    }
    let mut out = [0u8; N];
    out[..b.len()].copy_from_slice(b);
    Ok(out)
}

/// The contract address (`C...`) of a token's Stellar Asset Contract on `passphrase`'s
/// network, derived locally.
pub fn sac_address(token: Token<'_>, passphrase: &str) -> Result<String, IngestError> {
    let asset = match token {
        Token::Native => XdrAsset::Native,
        Token::Credit(a) => {
            let key = stellar_strkey::ed25519::PublicKey::from_string(&a.issuer)
                .map_err(|e| IngestError::Registry(format!("issuer {}: {e}", a.issuer)))?;
            let issuer = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(key.0)));
            if a.code.len() <= 4 {
                XdrAsset::CreditAlphanum4(AlphaNum4 {
                    asset_code: AssetCode4(padded::<4>(&a.code)?),
                    issuer,
                })
            } else {
                XdrAsset::CreditAlphanum12(AlphaNum12 {
                    asset_code: AssetCode12(padded::<12>(&a.code)?),
                    issuer,
                })
            }
        }
    };
    let preimage = HashIdPreimage::ContractId(HashIdPreimageContractId {
        network_id: Hash(network_id(passphrase)),
        contract_id_preimage: ContractIdPreimage::Asset(asset),
    });
    let bytes = preimage
        .to_xdr(Limits::none())
        .map_err(|e| IngestError::Registry(e.to_string()))?;
    let id: [u8; 32] = Sha256::digest(&bytes).into();
    Ok(stellar_strkey::Contract(id).to_string().as_str().to_owned())
}

/// Validates and decodes a `C...` contract address.
pub fn parse_contract(addr: &str) -> Result<[u8; 32], IngestError> {
    stellar_strkey::Contract::from_string(addr)
        .map(|c| c.0)
        .map_err(|e| IngestError::Registry(format!("contract address {addr:?}: {e}")))
}

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<SimResult>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct SimResult {
    #[serde(default)]
    results: Vec<SimEntry>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SimEntry {
    xdr: String,
}

/// A read-only Soroban RPC client with timeouts and bounded retries.
pub struct SorobanRpc {
    url: String,
    passphrase: String,
    agent: ureq::Agent,
    in_flight: Mutex<usize>,
    slot_free: Condvar,
}

/// A held request slot; released on drop, even if the request panics.
struct Slot<'a>(&'a SorobanRpc);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if let Ok(mut n) = self.0.in_flight.lock() {
            *n = n.saturating_sub(1);
        }
        self.0.slot_free.notify_one();
    }
}

impl SorobanRpc {
    pub fn new(url: impl Into<String>, passphrase: impl Into<String>) -> SorobanRpc {
        let agent = ureq::AgentBuilder::new()
            .timeout(TIMEOUT)
            .user_agent("pocky/0.1")
            .build();
        SorobanRpc {
            url: url.into(),
            passphrase: passphrase.into(),
            agent,
            in_flight: Mutex::new(0),
            slot_free: Condvar::new(),
        }
    }

    pub fn mainnet(url: impl Into<String>) -> SorobanRpc {
        SorobanRpc::new(url, MAINNET_PASSPHRASE)
    }

    pub fn passphrase(&self) -> &str {
        &self.passphrase
    }

    /// Confirms the endpoint serves the network we expect. Run once before reading: a
    /// misconfigured URL must not let testnet state pass as mainnet.
    pub fn check_network(&self) -> Result<(), IngestError> {
        #[derive(Deserialize)]
        struct Net {
            passphrase: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            result: Option<Net>,
        }
        let r: Resp = self.post(&json!({"jsonrpc": "2.0", "id": 1, "method": "getNetwork"}))?;
        match r.result {
            Some(n) if n.passphrase == self.passphrase => Ok(()),
            Some(n) => Err(bad(
                &self.url,
                format!("serves {:?}, expected {:?}", n.passphrase, self.passphrase),
            )),
            None => Err(bad(&self.url, "getNetwork returned no result")),
        }
    }

    /// Simulates `contract.func(args)` and returns its return value. Never submits.
    pub fn read(&self, contract: &str, func: &str, args: Vec<ScVal>) -> Result<ScVal, IngestError> {
        let id = parse_contract(contract)?;
        let invoke = InvokeContractArgs {
            contract_address: ScAddress::Contract(ContractId(Hash(id))),
            function_name: ScSymbol(func.try_into().map_err(|_| bad(func, "bad symbol"))?),
            args: args.try_into().map_err(|_| bad(func, "too many args"))?,
        };
        let op = Operation {
            source_account: None,
            body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: HostFunction::InvokeContract(invoke),
                auth: Default::default(),
            }),
        };
        let tx = Transaction {
            source_account: MuxedAccount::Ed25519(Uint256([0u8; 32])),
            fee: 100,
            seq_num: SequenceNumber(0),
            cond: Preconditions::None,
            memo: Memo::None,
            operations: vec![op].try_into().map_err(|_| bad(func, "ops"))?,
            ext: TransactionExt::V0,
        };
        let env = TransactionEnvelope::Tx(TransactionV1Envelope {
            tx,
            signatures: Default::default(),
        });
        let b64 = env
            .to_xdr_base64(Limits::none())
            .map_err(|e| bad(func, e))?;
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "simulateTransaction",
            "params": {"transaction": b64}
        });
        let what = format!("{contract}.{func}");
        let resp: RpcResponse = self.post(&req)?;
        if let Some(e) = resp.error {
            return Err(bad(&what, e));
        }
        let sim = resp.result.ok_or_else(|| bad(&what, "no result"))?;
        if let Some(e) = sim.error {
            return Err(bad(&what, e));
        }
        let first = sim
            .results
            .into_iter()
            .next()
            .ok_or_else(|| bad(&what, "no return value"))?;
        ScVal::from_xdr_base64(&first.xdr, Limits::len(64 * 1024)).map_err(|e| bad(&what, e))
    }

    fn slot(&self) -> Slot<'_> {
        if let Ok(mut n) = self.in_flight.lock() {
            while *n >= MAX_IN_FLIGHT {
                n = match self.slot_free.wait(n) {
                    Ok(g) => g,
                    Err(_) => return Slot(self),
                };
            }
            *n += 1;
        }
        Slot(self)
    }

    fn post<T: serde::de::DeserializeOwned>(
        &self,
        body: &serde_json::Value,
    ) -> Result<T, IngestError> {
        let mut last = String::new();
        for attempt in 1..=ATTEMPTS {
            let sent = {
                let _slot = self.slot();
                self.agent.post(&self.url).send_json(body.clone())
            };
            let mut backoff_ms = 300 * u64::from(attempt);
            match sent {
                Ok(resp) => {
                    return resp
                        .into_json::<T>()
                        .map_err(|e| bad(&self.url, e.to_string()))
                }
                Err(ureq::Error::Status(code, _)) if code != 429 && code < 500 => {
                    return Err(IngestError::Http {
                        url: self.url.clone(),
                        attempts: attempt,
                        reason: format!("status {code}"),
                    })
                }
                Err(ureq::Error::Status(429, _)) => {
                    // Rate limited: back off exponentially (0.6s, 1.2s, 2.4s, 4.8s).
                    last = "status 429 (rate limited)".into();
                    backoff_ms = 300 << attempt;
                }
                Err(e) => last = e.to_string(),
            }
            if attempt < ATTEMPTS {
                sleep(Duration::from_millis(backoff_ms));
            }
        }
        Err(IngestError::Http {
            url: self.url.clone(),
            attempts: ATTEMPTS,
            reason: last,
        })
    }
}

// ------------------------------------------------------------------ ScVal decoding

/// A `Vec<Address>` of contract addresses, as `C...` strings.
pub fn contract_addresses(v: &ScVal) -> Result<Vec<String>, IngestError> {
    let ScVal::Vec(Some(items)) = v else {
        return Err(bad("ScVal", "expected a vector of addresses"));
    };
    items.iter().map(contract_address).collect()
}

/// A single contract address.
pub fn contract_address(v: &ScVal) -> Result<String, IngestError> {
    match v {
        ScVal::Address(ScAddress::Contract(ContractId(Hash(id)))) => {
            Ok(stellar_strkey::Contract(*id)
                .to_string()
                .as_str()
                .to_owned())
        }
        _ => Err(bad("ScVal", "expected a contract address")),
    }
}

/// A vector (or tuple) of token amounts. SAC amounts for classic assets carry 7 decimals,
/// the same scale as [`Fx`], so the raw integer maps directly.
pub fn amounts(v: &ScVal) -> Result<Vec<Fx>, IngestError> {
    let ScVal::Vec(Some(items)) = v else {
        return Err(bad("ScVal", "expected a vector of amounts"));
    };
    items
        .iter()
        .map(|x| {
            let raw: i128 = match x {
                ScVal::I128(p) => (i128::from(p.hi) << 64) | i128::from(p.lo),
                ScVal::U128(p) => {
                    let u = (u128::from(p.hi) << 64) | u128::from(p.lo);
                    i128::try_from(u).map_err(|_| bad("ScVal", "amount overflows i128"))?
                }
                _ => return Err(bad("ScVal", "expected an i128 or u128 amount")),
            };
            Fx::from_raw(raw).map_err(IngestError::from)
        })
        .collect()
}

pub fn as_u32(v: &ScVal) -> Result<u32, IngestError> {
    match v {
        ScVal::U32(n) => Ok(*n),
        _ => Err(bad("ScVal", "expected u32")),
    }
}

pub fn as_symbol(v: &ScVal) -> Result<String, IngestError> {
    match v {
        ScVal::Symbol(s) => Ok(s.to_string()),
        _ => Err(bad("ScVal", "expected a symbol")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(code: &str, issuer: &str) -> Asset {
        Asset {
            code: code.into(),
            issuer: issuer.into(),
            home_domain: "example.com".into(),
            name: String::new(),
        }
    }

    #[test]
    fn derives_known_mainnet_sac_addresses() {
        // Published SAC ids on mainnet, as reported by `stellar contract id asset`.
        assert_eq!(
            sac_address(Token::Native, MAINNET_PASSPHRASE).unwrap(),
            "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA"
        );
        let usdc = asset(
            "USDC",
            "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN",
        );
        assert_eq!(
            sac_address(Token::Credit(&usdc), MAINNET_PASSPHRASE).unwrap(),
            "CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75"
        );
        // 5-12 character codes use AlphaNum12.
        let ustry = asset(
            "USTRY",
            "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC",
        );
        assert_eq!(
            sac_address(Token::Credit(&ustry), MAINNET_PASSPHRASE).unwrap(),
            "CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR"
        );
    }

    #[test]
    fn sac_address_depends_on_the_network() {
        let testnet = "Test SDF Network ; September 2015";
        assert_ne!(
            sac_address(Token::Native, MAINNET_PASSPHRASE).unwrap(),
            sac_address(Token::Native, testnet).unwrap()
        );
    }

    #[test]
    fn rejects_malformed_contract_addresses() {
        assert!(parse_contract("CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA").is_ok());
        assert!(
            parse_contract("GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN").is_err()
        );
        assert!(
            parse_contract("CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMB").is_err()
        );
    }

    #[test]
    fn decodes_amounts_and_rejects_other_shapes() {
        use stellar_xdr::{Int128Parts, UInt128Parts};
        let v = ScVal::Vec(Some(
            vec![
                ScVal::U128(UInt128Parts {
                    hi: 0,
                    lo: 10_171_592_424_775,
                }),
                ScVal::I128(Int128Parts { hi: 0, lo: 5 }),
            ]
            .try_into()
            .unwrap(),
        ));
        let a = amounts(&v).unwrap();
        assert_eq!(a[0], "1017159.2424775".parse().unwrap());
        assert_eq!(a[1].raw(), 5);
        assert!(amounts(&ScVal::U32(1)).is_err());
        let neg = ScVal::Vec(Some(
            vec![ScVal::I128(Int128Parts {
                hi: -1,
                lo: u64::MAX,
            })]
            .try_into()
            .unwrap(),
        ));
        assert!(amounts(&neg).is_err(), "negative reserves must be rejected");
    }

    /// Live, read-only check against mainnet. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_reads_an_aquarius_pool() {
        let rpc = SorobanRpc::mainnet(DEFAULT_MAINNET_RPC);
        rpc.check_network().unwrap();
        let pool = "CCX2TYR4AQTPPMTZOIMP3YIBHAYLLSTXVP47PAGTL753PCQFSKV32MIA";
        let tokens = contract_addresses(&rpc.read(pool, "get_tokens", vec![]).unwrap()).unwrap();
        assert!(tokens
            .contains(&"CBLV4ATSIWU67CFSQU2NVRKINQIKUZ2ODSZBUJTJ43VJVRSBTZYOPNUR".to_string()));
        let r = amounts(&rpc.read(pool, "get_reserves", vec![]).unwrap()).unwrap();
        assert_eq!(r.len(), 2);
        assert!(!r[0].is_zero());
        let wrong = SorobanRpc::new(DEFAULT_MAINNET_RPC, "Test SDF Network ; September 2015");
        assert!(wrong.check_network().is_err());
    }
}
