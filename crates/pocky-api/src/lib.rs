//! # pocky-api
//!
//! Runs measurements on a schedule, keeps every run in an append-only store, and serves the
//! latest figures, their history and the windowed minimum over HTTP. Read-only: it never
//! signs or submits anything.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod store;
