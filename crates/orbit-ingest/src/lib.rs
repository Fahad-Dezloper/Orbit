//! # orbit-ingest
//!
//! Read-only network access for Orbit: Horizon order books and issuers' SEP-1
//! `stellar.toml` files. Nothing in this crate signs or submits anything.
//!
//! All parsing of external data happens here, and only typed, validated values are passed
//! to `orbit-core`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod horizon;
pub mod lenders;
pub mod measure;
pub mod pools;
pub mod registry;
pub mod run;
pub mod sep1;
pub mod soroban;
pub mod templar;

pub use error::IngestError;
pub use horizon::{Horizon, Side};
pub use measure::{measure_asset, AssetReport, MeasureConfig};
pub use registry::{Asset, Registry};
