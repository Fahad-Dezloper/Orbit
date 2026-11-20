//! SEP-1: confirm an asset against its issuer's own `stellar.toml`.
//!
//! Horizon's asset list includes impostor tokens with the same codes as real ones. The only
//! authority on which issuer account is the real one is the issuer's domain.

use std::time::Duration;

use serde::Deserialize;

use crate::error::IngestError;
use crate::registry::Asset;

/// Max size of a stellar.toml we are willing to parse (SEP-1 caps it at 100KB).
const MAX_BYTES: u64 = 100 * 1024;

#[derive(Deserialize)]
struct Toml {
    #[serde(rename = "CURRENCIES", default)]
    currencies: Vec<Currency>,
}

#[derive(Deserialize)]
struct Currency {
    code: Option<String>,
    issuer: Option<String>,
}

/// Returns `Ok(())` only if the domain's stellar.toml lists exactly this code and issuer.
pub fn verify(asset: &Asset) -> Result<(), IngestError> {
    asset.validate()?;
    let url = format!("https://{}/.well-known/stellar.toml", asset.home_domain);
    let fail = |reason: String| IngestError::Sep1 {
        code: asset.code.clone(),
        domain: asset.home_domain.clone(),
        reason,
    };
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(2)
        .user_agent("pocky/0.1")
        .build();
    let resp = agent.get(&url).call().map_err(|e| fail(e.to_string()))?;
    let mut body = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), MAX_BYTES),
        &mut body,
    )
    .map_err(|e| fail(e.to_string()))?;
    check_toml(asset, &body).map_err(fail)
}

/// Pure check, separated for testing.
pub fn check_toml(asset: &Asset, body: &str) -> Result<(), String> {
    let t: Toml = toml::from_str(body).map_err(|e| format!("unparseable stellar.toml: {e}"))?;
    let found = t.currencies.iter().any(|c| {
        c.code.as_deref() == Some(asset.code.as_str())
            && c.issuer.as_deref() == Some(asset.issuer.as_str())
    });
    if found {
        Ok(())
    } else {
        Err("code and issuer pair not listed in CURRENCIES".to_string())
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
    const REAL: &str = "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC";
    const FAKE: &str = "GAJMPX5NBOG6TQFPQGRABJEEB2YE7RFRLUKJDZAZGAD5GFX4J7TADAZ6";
    const BODY: &str = r#"
        VERSION = "2.0.0"
        [[CURRENCIES]]
        code = "USTRY"
        issuer = "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC"
        [[CURRENCIES]]
        code = "CETES"
        issuer = "GCRYUGD5NVARGXT56XEZI5CIFCQETYHAPQQTHO2O3IQZTHDH4LATMYWC"
    "#;

    #[test]
    fn accepts_listed_pair() {
        assert!(check_toml(&asset("USTRY", REAL), BODY).is_ok());
    }

    #[test]
    fn rejects_impostor_issuer_with_same_code() {
        assert!(check_toml(&asset("USTRY", FAKE), BODY).is_err());
    }

    #[test]
    fn rejects_unlisted_code_and_garbage() {
        assert!(check_toml(&asset("GILTS", REAL), BODY).is_err());
        assert!(check_toml(&asset("USTRY", REAL), "not = [toml").is_err());
    }
}
