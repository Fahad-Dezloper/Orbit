//! The list of assets Pocky measures, loaded from `assets.toml`.

use serde::{Deserialize, Serialize};

use crate::error::IngestError;

/// A Stellar classic asset. Identity is the pair (code, issuer), never the code alone.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Asset {
    pub code: String,
    pub issuer: String,
    pub home_domain: String,
    #[serde(default)]
    pub name: String,
}

impl Asset {
    /// Validates the shape of the code and issuer so malformed entries never reach a URL.
    pub fn validate(&self) -> Result<(), IngestError> {
        let code_ok = (1..=12).contains(&self.code.len())
            && self.code.bytes().all(|b| b.is_ascii_alphanumeric());
        let issuer_ok = self.issuer.len() == 56
            && self.issuer.starts_with('G')
            && self
                .issuer
                .bytes()
                .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b));
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
}

impl Registry {
    pub fn from_toml(s: &str) -> Result<Registry, IngestError> {
        let r: Registry = toml::from_str(s).map_err(|e| IngestError::Registry(e.to_string()))?;
        r.usdc.validate()?;
        for a in &r.assets {
            a.validate()?;
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
    fn rejects_malformed_entries() {
        let bad = Asset {
            code: "US TRY".into(),
            issuer: "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC".into(),
            home_domain: "etherfuse.com".into(),
            name: String::new(),
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
