use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("http request to {url} failed after {attempts} attempts: {reason}")]
    Http {
        url: String,
        attempts: u32,
        reason: String,
    },
    #[error("unexpected response from {url}: {reason}")]
    BadResponse { url: String, reason: String },
    #[error("asset registry: {0}")]
    Registry(String),
    #[error("SEP-1 check failed for {code} at {domain}: {reason}")]
    Sep1 {
        code: String,
        domain: String,
        reason: String,
    },
    #[error(transparent)]
    Core(#[from] pocky_core::CoreError),
}
