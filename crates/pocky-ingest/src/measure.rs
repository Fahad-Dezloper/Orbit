//! One measurement pass for one asset across its SDEX venues.

use pocky_core::{
    AssetDepth, Band, CoreError, ExitQuote, Fx, OrderBook, Thresholds, Venue, VenueExit,
};
use serde::Serialize;

use crate::error::IngestError;
use crate::horizon::{Horizon, Side};
use crate::registry::Asset;

#[derive(Clone, Debug)]
pub struct MeasureConfig {
    pub bands: Vec<Band>,
    pub thresholds: Thresholds,
}

impl Default for MeasureConfig {
    fn default() -> Self {
        MeasureConfig {
            bands: Band::STANDARD.to_vec(),
            thresholds: Thresholds::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AssetReport {
    pub code: String,
    pub issuer: String,
    pub depths: Vec<AssetDepth>,
    /// Facts a reader needs to interpret the figure, e.g. a one-sided book.
    pub notes: Vec<String>,
}

/// USD per XLM from the XLM/USDC SDEX book. Fails closed if the book is one-sided.
pub fn xlm_usd(h: &Horizon, usdc: &Asset) -> Result<Fx, IngestError> {
    // Book of USDC priced in XLM: price = XLM per USDC.
    let book = h.order_book(usdc, &Side::Native)?;
    let xlm_per_usdc = book.mid()?;
    Ok(Fx::ONE.div_down(xlm_per_usdc)?)
}

fn venue_exits(
    book: &OrderBook,
    venue: Venue,
    usd_per_counter: Fx,
    bands: &[Band],
    notes: &mut Vec<String>,
) -> Result<Vec<VenueExit>, IngestError> {
    let label = match &venue {
        Venue::Sdex { counter } => format!("SDEX/{counter}"),
        Venue::Amm {
            protocol, counter, ..
        } => format!("{protocol}/{counter}"),
    };
    let mut out = Vec::with_capacity(bands.len());
    match book.mid() {
        Ok(mid) => {
            for &b in bands {
                out.push(VenueExit::new(
                    venue.clone(),
                    b,
                    book.exit(mid, b)?,
                    usd_per_counter,
                )?);
            }
        }
        Err(CoreError::NoMid) => {
            // No two-sided market means no trustworthy reference price. Count nothing.
            if !book.bids.is_empty() {
                notes.push(format!(
                    "{label}: bids but no asks; counted as zero (no reference price)"
                ));
            }
            for &b in bands {
                out.push(VenueExit::new(
                    venue.clone(),
                    b,
                    ExitQuote::default(),
                    usd_per_counter,
                )?);
            }
        }
        Err(e) => return Err(e.into()),
    }
    Ok(out)
}

/// Measures one asset on its USDC and XLM SDEX books at every configured band.
pub fn measure_asset(
    h: &Horizon,
    asset: &Asset,
    usdc: &Asset,
    xlm_usd: Fx,
    cfg: &MeasureConfig,
) -> Result<AssetReport, IngestError> {
    let mut notes = Vec::new();
    let usdc_book = h.order_book(asset, &Side::Credit(usdc))?;
    let xlm_book = h.order_book(asset, &Side::Native)?;

    let mut by_venue = venue_exits(
        &usdc_book,
        Venue::Sdex {
            counter: "USDC".into(),
        },
        Fx::ONE,
        &cfg.bands,
        &mut notes,
    )?;
    by_venue.extend(venue_exits(
        &xlm_book,
        Venue::Sdex {
            counter: "XLM".into(),
        },
        xlm_usd,
        &cfg.bands,
        &mut notes,
    )?);

    let mut depths = Vec::with_capacity(cfg.bands.len());
    for &b in &cfg.bands {
        let venues: Vec<VenueExit> = by_venue.iter().filter(|v| v.band == b).cloned().collect();
        depths.push(AssetDepth::combine(
            asset.code.clone(),
            b,
            venues,
            &cfg.thresholds,
        )?);
    }
    Ok(AssetReport {
        code: asset.code.clone(),
        issuer: asset.issuer.clone(),
        depths,
        notes,
    })
}
