//! # orbit-core
//!
//! Pure exit-liquidity math for Stellar assets. No I/O, no clocks, no floats.
//!
//! Given snapshots of public market data (SDEX order books and AMM pool reserves), this
//! crate answers one question: **how many dollars could be raised selling this asset right
//! now, before the average price falls more than a given band below the reference?**
//!
//! Everything here is deterministic, so any published figure can be recomputed from the
//! stored snapshot it came from.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod amm;
pub mod band;
pub mod book;
pub mod curve;
pub mod depth;
pub mod error;
pub mod fixed;
pub mod quote;
pub mod reference;
pub mod window;

pub use amm::CpPool;
pub use band::Band;
pub use book::{Ask, Bid, ExitQuote, OrderBook};
pub use curve::{curve_bands, liquidatable_at, safe_cap, CurvePoint, CURVE_BPS};
pub use depth::{collateral_at_risk, AssetDepth, Thresholds, Venue, VenueExit, Verdict};
pub use error::CoreError;
pub use fixed::Fx;
pub use quote::{QuoteSample, QuotedPool, QUOTE_FRACTIONS_BPS};
pub use reference::{consensus_reference, deviation_bps, Reference, VenueMid};
pub use window::{windowed_min, Sample, WindowPolicy, WindowedDepth};
