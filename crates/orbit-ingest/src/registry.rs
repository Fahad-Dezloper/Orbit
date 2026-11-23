//! The list of assets Orbit measures, loaded from `assets.toml`.

use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::lenders::LenderSpec;
use crate::pools::PoolSpec;

/// A token Orbit measures. A classic asset is identified by (code, issuer); a token that
/// exists only as a Soroban contract (SEP-41, e.g. Centrifuge's deJAAA) by (code, contract).
/// Never by the code alone.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Asset {
    pub code: String,
    /// Issuer account (`G...`) of a classic asset. Empty for a contract-only token.
    #[serde(default)]
    pub issuer: String,
    pub home_domain: String,
    #[serde(default)]
    pub name: String,
    /// Contract address (`C...`) of a token that exists only on Soroban. Such a token has no
    /// SDEX order book; it is measured in its pinned pools only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
}

impl Asset {
    /// Validates the shape of the code and issuer so malformed entries never reach a URL.
    pub fn validate(&self) -> Result<(), IngestError> {
        let code_ok = (1..=12).contains(&self.code.len())
            && self.code.bytes().all(|b| b.is_ascii_alphanumeric());
        let issuer_ok = match &self.contract {
            Some(c) => crate::soroban::parse_contract(c).is_ok() && self.issuer.is_empty(),
            None => self.issuer_shape_ok(),
        };
        let domain_ok = !self.home_domain.is_empty()
            && self
                .home_domain
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        if code_ok && issuer_ok && domain_ok {
            Ok(())
        } else {
            Err(IngestError::Registry(format!(
                "invalid asset entry {}:{}",
                self.code, self.issuer
            )))
        }
    }

    fn issuer_shape_ok(&self) -> bool {
        self.issuer.len() == 56
            && self.issuer.starts_with('G')
            && self
                .issuer
                .bytes()
                .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
    }

    /// True for a token that exists only as a Soroban contract.
    pub fn is_contract(&self) -> bool {
        self.contract.is_some()
    }

    /// The token's contract address on `passphrase`'s network: its own contract, or the
    /// Stellar Asset Contract of a classic asset, derived locally.
    pub fn token_address(&self, passphrase: &str) -> Result<String, IngestError> {
        match &self.contract {
            Some(c) => Ok(c.clone()),
            None => crate::soroban::sac_address(crate::soroban::Token::Credit(self), passphrase),
        }
    }

    /// Who vouches for the token: the issuer account, or the contract.
    pub fn identity(&self) -> &str {
        self.contract.as_deref().unwrap_or(&self.issuer)
    }

    /// Horizon's asset type for this code length.
    pub fn horizon_type(&self) -> &'static str {
        if self.code.len() <= 4 {
            "credit_alphanum4"
        } else {
            "credit_alphanum12"
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Registry {
    #[serde(rename = "asset")]
    pub assets: Vec<Asset>,
    pub usdc: Asset,
    /// Soroban AMM pools, pinned and verified on-chain before use.
    #[serde(default, rename = "pool")]
    pub pools: Vec<PoolSpec>,
    /// Blend pools whose holdings of measured assets are read, each confirmed by
    /// `blend_factory` before use.
    #[serde(default, rename = "lender")]
    pub lenders: Vec<LenderSpec>,
    /// The Blend v2 pool factory. Required when lenders are listed.
    #[serde(default)]
    pub blend_factory: Option<String>,
}

impl Registry {
    pub fn from_toml(s: &str) -> Result<Registry, IngestError> {
        let r: Registry = toml::from_str(s).map_err(|e| IngestError::Registry(e.to_string()))?;
        r.usdc.validate()?;
        for a in &r.assets {
            a.validate()?;
        }
        for p in &r.pools {
            p.validate()?;
            if !r.assets.iter().any(|a| a.code == p.asset) {
                return Err(IngestError::Registry(format!(
                    "pool {} sells unregistered asset {}",
                    p.contract, p.asset
                )));
            }
        }
        if !r.lenders.is_empty() {
            let f = r.blend_factory.as_deref().ok_or_else(|| {
                IngestError::Registry("lenders are listed but blend_factory is not".into())
            })?;
            crate::soroban::parse_contract(f)?;
        }
        for l in &r.lenders {
            crate::soroban::parse_contract(&l.contract)?;
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_the_shipped_registry() {
        let r = Registry::from_toml(include_str!("../../../assets.toml")).unwrap();
        assert!(r.assets.iter().any(|a| a.code == "USTRY"));
        assert_eq!(r.usdc.code, "USDC");
    }

    #[test]
    fn contract_tokens_need_a_contract_and_no_issuer() {
        let ok = Asset {
            code: "deJAAA".into(),
            issuer: String::new(),
            home_domain: "centrifuge.io".into(),
            name: String::new(),
            contract: Some("CC64WBDGS6QQP22QTTIACYIXT3WF7BBQEYOQPLTP7GTKYY7PZ74QYGSL".into()),
        };
        assert!(ok.validate().is_ok());
        assert!(ok.is_contract());
        assert_eq!(
            ok.identity(),
            "CC64WBDGS6QQP22QTTIACYIXT3WF7BBQEYOQPLTP7GTKYY7PZ74QYGSL"
        );
        let bad_contract = Asset {
            contract: Some("Cnotacontract".into()),
            ..ok.clone()
        };
        assert!(bad_contract.validate().is_err());
        let both = Asset {
            issuer: "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC".into(),
            ..ok
        };
        assert!(both.validate().is_err());
    }

    #[test]
    fn rejects_malformed_entries() {
        let bad = Asset {
            code: "US TRY".into(),
            issuer: "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC".into(),
            home_domain: "etherfuse.com".into(),
            name: String::new(),
            contract: None,
        };
        assert!(bad.validate().is_err());
        let bad_issuer = Asset {
            code: "USTRY".into(),
            issuer: "Gshort".into(),
            ..bad.clone()
        };
        assert!(bad_issuer.validate().is_err());
        let bad_domain = Asset {
            code: "USTRY".into(),
            home_domain: "evil.com/x?y".into(),
            ..bad
        };
        assert!(bad_domain.validate().is_err());
    }
}
