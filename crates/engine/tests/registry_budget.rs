//! Manual alternating hot-path comparison; both paths return the same builders.
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_engine::{BarBuilder, Side, TickBarBuilder, Trade};
use rust_decimal::Decimal;
use std::{hint::black_box, time::Instant};

#[test]
#[ignore = "manual serialized registry/direct per-trade comparison"]
fn resolved_registry_builder_has_the_direct_builder_ingest_budget() {
    const PRINTS: u64 = 200_000;
    let trades: Vec<_> = (0..PRINTS)
        .map(|id| Trade {
            agg_id: id,
            timestamp_ms: id as i64,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: if id % 2 == 0 { Side::Buy } else { Side::Sell },
        })
        .collect();
    let config = BUILTIN_BARS.parse("tick:50").unwrap();
    let mut direct = Vec::new();
    let mut registered = Vec::new();
    for round in 0..12 {
        for is_registered in [round % 2 == 0, round % 2 != 0] {
            // Resolution/allocation happen before timing; only ingest is measured.
            let mut builder: Box<dyn BarBuilder> = if is_registered {
                config.build()
            } else {
                Box::new(TickBarBuilder::new(50))
            };
            let start = Instant::now();
            let mut closed = 0;
            for trade in &trades {
                closed += usize::from(black_box(builder.push(black_box(trade))).is_some());
            }
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(closed, PRINTS as usize / 50);
            if round > 1 {
                if is_registered {
                    registered.push(elapsed);
                } else {
                    direct.push(elapsed);
                }
            }
        }
    }
    direct.sort();
    registered.sort();
    eprintln!(
        "registry_budget prints={PRINTS} direct_median_ns={} registered_median_ns={} ratio={:.4}",
        direct[direct.len() / 2],
        registered[registered.len() / 2],
        registered[registered.len() / 2] as f64 / direct[direct.len() / 2] as f64
    );
}
