//! Speed of the depth maths, excluding network. Run with:
//! `cargo test --release -p orbit-core --test bench -- --ignored --nocapture`

use std::hint::black_box;
use std::time::Instant;

use orbit_core::{curve_bands, Ask, Bid, CpPool, Fx, OrderBook};

fn fx(s: &str) -> Fx {
    s.parse().unwrap()
}

#[test]
#[ignore]
fn depth_maths_throughput() {
    // A full Horizon page: 200 bid levels stepping down 0.05% each.
    let mut bids = Vec::new();
    let mut p = fx("1.0800000");
    for _ in 0..200 {
        bids.push(Bid {
            price: p,
            amount_counter: fx("1500"),
        });
        p = p.mul_bps_down(9_995).unwrap();
    }
    let book = OrderBook {
        bids,
        asks: vec![Ask {
            price: fx("1.0810000"),
            amount_base: fx("1000"),
        }],
    };
    let pool = CpPool {
        reserve_asset: fx("1017159.2424775"),
        reserve_counter: fx("1097386.1845172"),
        fee_bps: 30,
    };
    let bands = curve_bands().unwrap();
    let reference = fx("1.0791000");

    // One asset's full curve: 12 bands x (2 books + 3 pools), as in a live run.
    let n = 20_000u32;
    let start = Instant::now();
    for _ in 0..n {
        for &b in &bands {
            for _ in 0..2 {
                black_box(book.exit(black_box(reference), b).unwrap());
            }
            for _ in 0..3 {
                black_box(pool.exit_against(black_box(reference), b).unwrap());
            }
        }
    }
    let per_curve = start.elapsed() / n;
    println!("one asset's 12-band curve over 5 venues (200-level books): {per_curve:?}");
    println!("ten assets: {:?}", per_curve * 10);
}
