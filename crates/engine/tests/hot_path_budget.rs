//! Every bar builder's per-trade work, counted and held to a budget.
//!
//! Chart, backtest and bot all feed trades through
//! [`BarBuilder::push_into`]; this binary pushes one deterministic burst (the
//! shape `benches/hot_path.rs` times) through each builder and counts the heap
//! work of the pushes alone with [`work_meter`]: allocations, bytes allocated
//! and bytes a reallocation may copy, per trade, after a warm-up. Counts do
//! not move with the host's load, so the budgets hold in the ordinary
//! `cargo test`; a push that starts cloning a buffer, or rebuilding one per
//! trade, crosses them. A slowdown that allocates nothing is invisible here:
//! that stays with the bench.
//!
//! The budgets are declared first, each with the calibration it was set
//! from, and never read back from a measurement.

use quantick_engine::work_meter::{self, Counting, Tally};
use quantick_engine::{
    Bar, BarBuilder, DealBarBuilder, DealSample, DollarBarBuilder, ImbalanceBarBuilder,
    ImbalanceUnit, RenkoBarBuilder, Side, TickBarBuilder, TimeBarBuilder, Trade, VolumeBarBuilder,
};
use rust_decimal::Decimal;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// ---------------------------------------------------------------------------
// Budgets: heap work per trade pushed. Declared at no more than twice the
// calibration: every builder measured zero, so every budget is zero — a
// steady-state push allocates nothing, and the first one that does fails.
// ---------------------------------------------------------------------------

/// Per-trade ceilings of one builder's push.
#[derive(Clone, Copy, Debug)]
struct Budget {
    path: &'static str,
    /// Allocations per trade.
    allocs: f64,
    /// Bytes allocated per trade.
    bytes: f64,
    /// Bytes reallocation may copy per trade.
    copy_bytes: f64,
}

/// tick:100. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (200 bars closed in the window).
const TICK: Budget = Budget {
    path: "engine.push.tick",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// volume:50. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (199 bars closed in the window).
const VOLUME: Budget = Budget {
    path: "engine.push.volume",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// dollar:1e7. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (36 bars closed in the window).
const DOLLAR: Budget = Budget {
    path: "engine.push.dollar",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// time:1000ms. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (20 bars closed in the window).
const TIME: Budget = Budget {
    path: "engine.push.time",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// imbalance:100. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (67 bars closed in the window).
const IMBALANCE_TRADES: Budget = Budget {
    path: "engine.push.imbalance.trades",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// imbalance:100 in volume. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (67 bars closed in the window).
const IMBALANCE_VOLUME: Budget = Budget {
    path: "engine.push.imbalance.volume",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// imbalance:100 in dollars. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (67 bars closed in the window).
const IMBALANCE_DOLLAR: Budget = Budget {
    path: "engine.push.imbalance.dollar",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// renko:4, step read off the prints. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (12,400 bars closed in the window).
const RENKO: Budget = Budget {
    path: "engine.push.renko",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};
/// deals:500, a reading a second. Calibrated at `ff300983`: 0 allocations, 0 bytes, 0 copy bytes per
/// trade (120 bars closed in the window).
const DEALS: Budget = Budget {
    path: "engine.push.deals",
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};

// ---------------------------------------------------------------------------
// Measurement.
// ---------------------------------------------------------------------------

/// Trades pushed before counting starts, so a builder's first buffers are
/// not charged to the window.
const WARMUP_TRADES: usize = 5_000;
/// Trades counted.
const MEASURED_TRADES: usize = 20_000;
/// Trades between two venue deal-counter readings for the deal builder: one
/// second of this tape.
const TRADES_PER_READING: usize = 1_000;

/// A deterministic burst of `n` trades around a 36000 base price: the
/// generator of `benches/hot_path.rs` (a bench cannot be imported).
fn make_trades(n: usize) -> Vec<Trade> {
    let base = Decimal::from(36_000);
    let mut trades = Vec::with_capacity(n);
    for i in 0..n {
        // Price wiggles in a deterministic ±5.00 band in 0.10 steps.
        let tick = (i % 100) as i64 - 50;
        let price = base + Decimal::new(tick, 1);
        // Quantity cycles 0.001 .. 1.000.
        let quantity = Decimal::new(((i % 1000) + 1) as i64, 3);
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        trades.push(Trade {
            agg_id: i as u64,
            timestamp_ms: 1_700_000_000_000 + i as i64,
            price,
            quantity,
            side,
        });
    }
    trades
}

fn add(a: Tally, b: Tally) -> Tally {
    Tally {
        allocs: a.allocs + b.allocs,
        alloc_bytes: a.alloc_bytes + b.alloc_bytes,
        reallocs: a.reallocs + b.reallocs,
        realloc_copy_bytes: a.realloc_copy_bytes + b.realloc_copy_bytes,
        largest_realloc_copy: a.largest_realloc_copy.max(b.largest_realloc_copy),
    }
}

/// Push the burst through `builder` as chart, backtest and bot do, feeding
/// a deal-counter reading every [`TRADES_PER_READING`] trades to a builder
/// that takes them, and hold the counted pushes to `budget`.
fn holds_to_budget(budget: Budget, mut builder: Box<dyn BarBuilder>) {
    let trades = make_trades(WARMUP_TRADES + MEASURED_TRADES);
    let mut closed: Vec<Bar> = Vec::with_capacity(4);
    let mut bars = 0_usize;
    let mut work = Tally::default();
    for (index, trade) in trades.iter().enumerate() {
        if index % TRADES_PER_READING == 0
            && let Some(input) = builder.deal_counter_input()
        {
            input.observe(DealSample {
                time_ms: trade.timestamp_ms,
                session_deals: 3 * index as u64,
            });
        }
        let before = work_meter::tally();
        builder.push_into(std::hint::black_box(trade), &mut closed);
        let spent = work_meter::tally().since(before);
        if index >= WARMUP_TRADES {
            work = add(work, spent);
            bars += closed.len();
        }
        closed.clear();
    }
    assert!(bars > 0, "{}: the burst closes bars", budget.path);
    let per = |total: u64| total as f64 / MEASURED_TRADES as f64;
    let measured = (
        per(work.allocs),
        per(work.alloc_bytes),
        per(work.realloc_copy_bytes),
    );
    println!(
        "{}: {:.4} allocs, {:.2} bytes, {:.2} copy bytes per trade over {MEASURED_TRADES} trades ({bars} bars)",
        budget.path, measured.0, measured.1, measured.2
    );
    assert!(
        measured.0 <= budget.allocs
            && measured.1 <= budget.bytes
            && measured.2 <= budget.copy_bytes,
        "{} over budget per trade: {:.4} allocs (budget {}), {:.2} bytes (budget {}), \
         {:.2} copy bytes (budget {}); totals over {MEASURED_TRADES} trades: {work:?}",
        budget.path,
        measured.0,
        budget.allocs,
        measured.1,
        budget.bytes,
        measured.2,
        budget.copy_bytes,
    );
}

#[test]
fn tick_push_holds_to_budget() {
    holds_to_budget(TICK, Box::new(TickBarBuilder::new(100)));
}

#[test]
fn volume_push_holds_to_budget() {
    holds_to_budget(VOLUME, Box::new(VolumeBarBuilder::new(Decimal::from(50))));
}

#[test]
fn dollar_push_holds_to_budget() {
    holds_to_budget(
        DOLLAR,
        Box::new(DollarBarBuilder::new(Decimal::from(10_000_000))),
    );
}

#[test]
fn time_push_holds_to_budget() {
    holds_to_budget(TIME, Box::new(TimeBarBuilder::new(1_000)));
}

#[test]
fn imbalance_push_holds_to_budget() {
    holds_to_budget(IMBALANCE_TRADES, Box::new(ImbalanceBarBuilder::new(100)));
    holds_to_budget(
        IMBALANCE_VOLUME,
        Box::new(ImbalanceBarBuilder::with_unit(100, ImbalanceUnit::Volume)),
    );
    holds_to_budget(
        IMBALANCE_DOLLAR,
        Box::new(ImbalanceBarBuilder::with_unit(100, ImbalanceUnit::Dollar)),
    );
}

#[test]
fn renko_push_holds_to_budget() {
    holds_to_budget(RENKO, Box::new(RenkoBarBuilder::new(4)));
}

#[test]
fn deals_push_holds_to_budget() {
    holds_to_budget(DEALS, Box::new(DealBarBuilder::new(500)));
}
