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
    let body = fetch_toml(&asset.home_domain).map_err(|reason| IngestError::Sep1 {
        code: asset.code.clone(),
        domain: asset.home_domain.clone(),
        reason,
    })?;
    check_toml(asset, &body).map_err(|reason| IngestError::Sep1 {
        code: asset.code.clone(),
        domain: asset.home_domain.clone(),
        reason,
    })
}

/// Fetches `https://{domain}/.well-known/stellar.toml`, at most `MAX_BYTES`. Callers that
/// verify several assets from one issuer fetch once and run [`check_toml`] per asset.
pub fn fetch_toml(domain: &str) -> Result<String, String> {
    if domain.is_empty()
        || !domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(format!("invalid home domain {domain:?}"));
    }
    let url = format!("https://{domain}/.well-known/stellar.toml");
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(2)
        .user_agent("orbit/0.1")
        .build();
    let resp = agent.get(&url).call().map_err(|e| e.to_string())?;
    let mut body = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), MAX_BYTES),
        &mut body,
    )
    .map_err(|e| e.to_string())?;
    Ok(body)
}

/// The `[[CURRENCIES]]` entries of a stellar.toml.
///
/// Some issuers publish a file with a syntax error in an unrelated section (WisdomTree's has
/// an unterminated string in `ACCOUNTS`). Then each `[[CURRENCIES]]` table is parsed on its
/// own, so a broken section elsewhere cannot hide the issuer's own currency list. Entries that
/// do not parse are skipped, never guessed at.
fn currencies(body: &str) -> Result<Vec<Currency>, String> {
    match toml::from_str::<Toml>(body) {
        Ok(t) => Ok(t.currencies),
        Err(e) => {
            let mut out = Vec::new();
            let mut section: Option<String> = None;
            let flush = |sec: &mut Option<String>, out: &mut Vec<Currency>| {
                if let Some(text) = sec.take() {
                    if let Ok(t) = toml::from_str::<Toml>(&text) {
                        out.extend(t.currencies);
                    }
                }
            };
            for line in body.lines() {
                let l = line.trim_start();
                if l.starts_with('[') {
                    flush(&mut section, &mut out);
                    if l.starts_with("[[CURRENCIES]]") {
                        section = Some(String::from("[[CURRENCIES]]\n"));
                    }
                } else if let Some(sec) = section.as_mut() {
                    sec.push_str(line);
                    sec.push('\n');
                }
            }
            flush(&mut section, &mut out);
            if out.is_empty() {
                Err(format!("unparseable stellar.toml: {e}"))
            } else {
                Ok(out)
            }
        }
    }
}

pub fn check_toml(asset: &Asset, body: &str) -> Result<(), String> {
    let currencies = currencies(body)?;
    let found = currencies.iter().any(|c| {
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
    fn reads_currencies_past_a_broken_section() {
        // An unterminated string elsewhere in the file (as in WisdomTree's stellar.toml).
        let broken = r#"
            ACCOUNTS = [
            "GAAA",
            "GBBB
            ]
            [[CURRENCIES]]
            code = "WTSY"
            issuer = "GB3ZUC7FGDEEBXY3BDEJWMPNGBFA66YRI4QQT6PBO3ZT6F33S7RL36VF"
            [[CURRENCIES]]
            code = "BAD
            issuer = "GXXX"
        "#;
        let wtsy = asset(
            "WTSY",
            "GB3ZUC7FGDEEBXY3BDEJWMPNGBFA66YRI4QQT6PBO3ZT6F33S7RL36VF",
        );
        assert!(check_toml(&wtsy, broken).is_ok());
        assert!(check_toml(&asset("WTSY", REAL), broken).is_err());
    }

    #[test]
    fn rejects_unlisted_code_and_garbage() {
        assert!(check_toml(&asset("GILTS", REAL), BODY).is_err());
        assert!(check_toml(&asset("USTRY", REAL), "not = [toml").is_err());
    }
}
