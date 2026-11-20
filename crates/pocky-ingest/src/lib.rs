//! # pocky-ingest
//!
//! Read-only network access for Pocky: Horizon order books and issuers' SEP-1
//! `stellar.toml` files. Nothing in this crate signs or submits anything.
//!
//! All parsing of external data happens here, and only typed, validated values are passed
//! to `pocky-core`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod horizon;
pub mod measure;
pub mod registry;
pub mod sep1;

pub use error::IngestError;
pub use horizon::{Horizon, Side};
pub use measure::{measure_asset, AssetReport, MeasureConfig};
pub use registry::{Asset, Registry};
