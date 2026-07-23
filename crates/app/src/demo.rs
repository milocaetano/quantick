//! Deterministic demo bars, built by the engine, for the standalone renderer.
//!
//! Until the live feed is wired in (#33), the chart renders these. They come out
//! of a real [`TickBarBuilder`] fed a synthetic random walk, so the renderer is
//! exercised against genuine engine output — not hand-made `Bar`s.

use quantick_engine::{Bar, BarBuilder, Side, TickBarBuilder, Trade};
use rust_decimal::Decimal;

/// Closed demo bars plus the forming partial, from a fixed synthetic walk.
#[must_use]
pub fn demo_bars() -> (Vec<Bar>, Option<Bar>) {
    let trades = synthetic_walk(1_000);
    let mut builder = TickBarBuilder::new(20);
    let mut bars = Vec::new();
    for trade in &trades {
        if let Some(bar) = builder.push(trade) {
            bars.push(bar);
        }
    }
    (bars, builder.partial().cloned())
}

/// A deterministic price random walk around 36000 (seeded xorshift, no rng dep).
fn synthetic_walk(n: usize) -> Vec<Trade> {
    let mut price = Decimal::from(36_000);
    let mut rng: u64 = 0x2545_F491_4F6C_DD1D;
    let mut trades = Vec::with_capacity(n);
    for i in 0..n {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let step = (rng % 21) as i64 - 10; // -1.0 .. +1.0 in 0.1 steps
        price += Decimal::new(step, 1);
        if price < Decimal::ONE {
            price = Decimal::ONE;
        }
        let quantity = Decimal::new((rng % 100 + 1) as i64, 3);
        let side = if rng & 1 == 0 { Side::Buy } else { Side::Sell };
        trades.push(Trade {
            agg_id: i as u64,
            timestamp_ms: 1_700_000_000_000 + i as i64 * 100,
            price,
            quantity,
            side,
        });
    }
    trades
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_bars_are_produced_and_deterministic() {
        let (bars_a, partial_a) = demo_bars();
        let (bars_b, partial_b) = demo_bars();
        assert!(!bars_a.is_empty(), "demo produces closed bars");
        assert_eq!(bars_a, bars_b, "demo bars are deterministic");
        assert_eq!(partial_a, partial_b);
    }
}
