//! One measurement pass for one asset across every venue: its SDEX books and its pinned
//! Soroban AMM pools.
//!
//! 1. Read every venue. A pool that fails on-chain verification is left out and noted.
//! 2. Take each venue's own price and its depth at that price.
//! 3. Choose one reference price for the asset from venues with real depth
//!    ([`consensus_reference`]). No venue is its own reference.
//! 4. Measure every venue's exit from that reference, at every band, and sum them.

use pocky_core::{
    consensus_reference, curve_bands, deviation_bps, AssetDepth, Band, CpPool, CurvePoint,
    ExitQuote, Fx, OrderBook, QuotedPool, Reference, Thresholds, Venue, VenueExit, VenueMid,
};
use serde::{Deserialize, Serialize};

use crate::error::IngestError;
use crate::horizon::{Horizon, Side};
use crate::pools::{read_pool_state, Counter, PoolSpec, PoolState};
use crate::registry::Asset;
use crate::soroban::SorobanRpc;

/// A venue whose mid sits further than this from the reference is called out in the notes.
pub const DEVIATION_NOTE_BPS: i64 = 200;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetReport {
    pub code: String,
    pub issuer: String,
    /// The price every band was measured from, or `None` if no venue had enough depth to
    /// set one (then every figure is zero).
    pub reference: Option<Reference>,
    /// Each venue's own price and depth, before the reference was applied.
    pub venue_mids: Vec<VenueMid>,
    pub depths: Vec<AssetDepth>,
    /// Facts a reader needs to interpret the figure, e.g. a one-sided book.
    pub notes: Vec<String>,
    /// Venues actually read (two SDEX books plus every pool that passed verification).
    #[serde(default)]
    pub venues_read: usize,
    /// Total exit across all venues at each band of `pocky_core::CURVE_BPS`, from the same
    /// reference. Empty when there is no reference.
    #[serde(default)]
    pub curve: Vec<CurvePoint>,
}

/// USD per XLM from the XLM/USDC SDEX book. Fails closed if the book is one-sided.
pub fn xlm_usd(h: &Horizon, usdc: &Asset) -> Result<Fx, IngestError> {
    // Book of USDC priced in XLM: price = XLM per USDC.
    let book = h.order_book(usdc, &Side::Native)?;
    let xlm_per_usdc = book.mid()?;
    Ok(Fx::ONE.div_down(xlm_per_usdc)?)
}

enum Source {
    Book(OrderBook),
    Pool(CpPool),
    Quoted(QuotedPool),
}

struct Leg {
    venue: Venue,
    label: String,
    usd_per_counter: Fx,
    source: Source,
}

impl Leg {
    /// The venue's own price in COUNTER units, if it has one.
    fn own_mid(&self) -> Option<Fx> {
        match &self.source {
            Source::Book(b) => b.mid().ok(),
            Source::Pool(p) if p.reserve_asset.is_zero() || p.reserve_counter.is_zero() => None,
            Source::Pool(p) => p.mid().ok(),
            Source::Quoted(q) => q.mid().ok().filter(|m| !m.is_zero()),
        }
    }

    fn exit_against(&self, reference: Fx, band: Band) -> Result<ExitQuote, IngestError> {
        Ok(match &self.source {
            Source::Book(b) => b.exit(reference, band)?,
            Source::Pool(p) => p.exit_against(reference, band)?,
            Source::Quoted(q) => q.exit_against(reference, band)?,
        })
    }
}

/// Measures one asset across its SDEX books and, when `rpc` is given, its pinned pools.
pub fn measure_asset(
    h: &Horizon,
    rpc: Option<&SorobanRpc>,
    asset: &Asset,
    usdc: &Asset,
    pools: &[PoolSpec],
    xlm_usd: Fx,
    cfg: &MeasureConfig,
) -> Result<AssetReport, IngestError> {
    let mut notes = Vec::new();
    let mine: Vec<&PoolSpec> = match rpc {
        Some(_) => pools.iter().filter(|p| p.asset == asset.code).collect(),
        None => Vec::new(),
    };
    // Both books and every pool are fetched at the same time.
    let (usdc_book, xlm_book, pool_reads) = std::thread::scope(|s| {
        let usdc_book = s.spawn(|| h.order_book(asset, &Side::Credit(usdc)));
        let xlm_book = s.spawn(|| h.order_book(asset, &Side::Native));
        let pool_handles: Vec<_> = mine
            .iter()
            .map(|&spec| {
                s.spawn(move || match rpc {
                    Some(rpc) => read_pool_state(rpc, spec, asset, usdc),
                    None => Err(IngestError::Registry("no RPC".into())),
                })
            })
            .collect();
        let panicked = || IngestError::Registry("venue read panicked".into());
        (
            usdc_book.join().unwrap_or_else(|_| Err(panicked())),
            xlm_book.join().unwrap_or_else(|_| Err(panicked())),
            pool_handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err(panicked())))
                .collect::<Vec<_>>(),
        )
    });
    let mut legs = vec![
        Leg {
            venue: Venue::Sdex {
                counter: "USDC".into(),
            },
            label: "SDEX/USDC".into(),
            usd_per_counter: Fx::ONE,
            source: Source::Book(usdc_book?),
        },
        Leg {
            venue: Venue::Sdex {
                counter: "XLM".into(),
            },
            label: "SDEX/XLM".into(),
            usd_per_counter: xlm_usd,
            source: Source::Book(xlm_book?),
        },
    ];
    {
        for (spec, read) in mine.iter().zip(pool_reads) {
            let short = spec.contract.get(..8).unwrap_or(&spec.contract);
            let label = format!("{} {short}", spec.label());
            match read {
                Ok(pool) => legs.push(Leg {
                    venue: Venue::Amm {
                        protocol: spec.protocol.name().into(),
                        pool: spec.contract.clone(),
                        counter: spec.counter.code().into(),
                    },
                    label,
                    usd_per_counter: match spec.counter {
                        Counter::Usdc => Fx::ONE,
                        Counter::Xlm => xlm_usd,
                    },
                    source: match pool {
                        PoolState::ConstantProduct(p) => Source::Pool(p),
                        PoolState::Quoted(q) => Source::Quoted(q),
                    },
                }),
                Err(e) => notes.push(format!("{label}: left out ({e})")),
            }
        }
    }

    // Each venue's own price and its depth at that price, in USD.
    let widest = cfg
        .bands
        .iter()
        .copied()
        .max()
        .unwrap_or(Band::TWENTY_FIVE_PCT);
    let mut mids = Vec::new();
    for leg in &legs {
        match leg.own_mid() {
            Some(mid) => {
                let depth = leg.exit_against(mid, widest)?.counter_raised;
                mids.push(VenueMid {
                    label: leg.label.clone(),
                    mid_usd: mid.mul_down(leg.usd_per_counter)?,
                    depth_usd: depth.mul_down(leg.usd_per_counter)?,
                });
            }
            None => {
                if let Source::Book(b) = &leg.source {
                    if !b.bids.is_empty() {
                        notes.push(format!(
                            "{}: bids but no asks, so no price of its own",
                            leg.label
                        ));
                    }
                }
            }
        }
    }

    let reference = consensus_reference(&mids, cfg.thresholds.dead_below);
    let mut by_venue = Vec::new();
    let mut curve = Vec::new();
    match &reference {
        None => {
            notes.push(format!(
                "no venue has ${} of depth at its own price; no reference, counted as zero",
                cfg.thresholds.dead_below.raw() / 10_000_000
            ));
            for leg in &legs {
                for &b in &cfg.bands {
                    by_venue.push(VenueExit::new(
                        leg.venue.clone(),
                        b,
                        ExitQuote::default(),
                        leg.usd_per_counter,
                    )?);
                }
            }
        }
        Some(r) => {
            for band in curve_bands()? {
                let mut total = Fx::ZERO;
                for leg in &legs {
                    let reference_counter = r.price_usd.div_up(leg.usd_per_counter)?;
                    let q = leg.exit_against(reference_counter, band)?;
                    total = total.checked_add(q.counter_raised.mul_down(leg.usd_per_counter)?)?;
                }
                curve.push(CurvePoint {
                    band,
                    exit_usd: total,
                });
            }
            for m in &mids {
                let d = deviation_bps(m.mid_usd, r.price_usd)?;
                if d.abs() > DEVIATION_NOTE_BPS {
                    let side = if d < 0 { "below" } else { "above" };
                    let thin = if r.qualifying.contains(&m.label) {
                        ""
                    } else {
                        " (too thin to set the reference)"
                    };
                    notes.push(format!(
                        "{}: own price {}.{:02}% {side} the reference{thin}",
                        m.label,
                        d.abs() / 100,
                        d.abs() % 100
                    ));
                }
            }
            for leg in &legs {
                // Reference in this venue's counter units, rounded up: a higher reference
                // can only lower measured depth.
                let reference_counter = r.price_usd.div_up(leg.usd_per_counter)?;
                for &b in &cfg.bands {
                    by_venue.push(VenueExit::new(
                        leg.venue.clone(),
                        b,
                        leg.exit_against(reference_counter, b)?,
                        leg.usd_per_counter,
                    )?);
                }
            }
        }
    }

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
        reference,
        venue_mids: mids,
        depths,
        notes,
        venues_read: legs.len(),
        curve,
    })
}
