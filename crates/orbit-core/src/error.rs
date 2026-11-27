use thiserror::Error;

/// Errors from the depth math. Every one of them means "do not publish this figure".
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    #[error("value would be negative")]
    Negative,
    #[error("arithmetic overflow")]
    Overflow,
    #[error("division by zero")]
    DivByZero,
    #[error("cannot parse decimal {0:?}")]
    Parse(String),
    #[error("more than 7 decimal places in {0:?}")]
    Precision(String),
    #[error("slippage band must be between 1 and 9999 basis points, got {0}")]
    BadBand(u32),
    #[error("order book is not sorted best-first")]
    UnsortedBook,
    #[error("no reference price: the book has no bids or no asks")]
    NoMid,
    #[error("seller share must be between 1 and 10000 basis points, got {0}")]
    BadShare(u32),
    #[error("not enough fresh samples in the window: have {have}, need {need}")]
    InsufficientSamples { have: usize, need: usize },
    #[error("gap of {gap_secs}s between samples exceeds the allowed {max_gap_secs}s")]
    SampleGap { gap_secs: u64, max_gap_secs: u64 },
}
