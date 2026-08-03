//! The bot-readiness proof: a backtester consumes indicators with **zero UI
//! involvement**, through exactly the API a chart uses.
//!
//! This test *is* the future backtest/bot access path, written today so no
//! later change can silently couple the indicator runtime to the app: build
//! a host, push engine bars, read committed plot columns, derive trading
//! signals from them. If this test compiles and passes, "one engine, three
//! consumers" holds for indicators.

use quantick_engine::{TickBarBuilder, fixture, golden as engine_golden};
use quantick_indicators::{
    IndicatorHost, PlotId, SourceId,
    native::{Cvd, Ema},
};

const TRADES: &str = include_str!("fixtures/trades_ramp.csv");

#[test]
fn a_headless_consumer_reads_indicators_like_a_backtester() {
    // 1. Bars from raw trades — the same engine path chart and bot share.
    let trades = fixture::parse_trades(TRADES).expect("fixture parses");
    let bars = engine_golden::replay(&mut TickBarBuilder::new(1), &trades);

    // 2. A host with a fast and a slow EMA plus CVD. No worker, no egui,
    //    no channel — this is the whole setup a bot needs.
    let mut host = IndicatorHost::new();
    let fast = host.add(Box::new(Ema::new(2, SourceId::Close)));
    let slow = host.add(Box::new(Ema::new(3, SourceId::Close)));
    let flow = host.add(Box::new(Cvd::new()));
    for bar in &bars {
        host.push_closed_bar(bar);
    }

    // 3. Committed columns come out as plain slices a strategy iterates.
    let plot = PlotId::new(0);
    let fast_col = host.plots(fast).unwrap().column(plot);
    let slow_col = host.plots(slow).unwrap().column(plot);
    let cvd_col = host.plots(flow).unwrap().column(plot);
    assert_eq!(fast_col.len(), bars.len());
    assert_eq!(slow_col.len(), bars.len());
    assert_eq!(cvd_col.len(), bars.len());

    // 4. A toy strategy over indicator data: fast/slow EMA crosses,
    //    confirmed by the sign of cumulative delta — indicator values and
    //    order-flow data consumed side by side, bar by bar.
    let mut crosses = Vec::new();
    for i in 1..bars.len() {
        let over = fast_col[i] > slow_col[i] && fast_col[i - 1] <= slow_col[i - 1];
        let under = fast_col[i] < slow_col[i] && fast_col[i - 1] >= slow_col[i - 1];
        if over || under {
            crosses.push((i, over, cvd_col[i] > 0.0));
        }
    }

    // The ramp fixture rises 3..18 then falls back: both EMAs warm up on
    // the rise already fast-over-slow, and the downturn produces exactly one
    // cross-under, at bar 7 (hand-computed: fast 13.1667 < slow 13.5 with
    // fast 15.5 >= slow 15.0 the bar before). The tail tick-up to 6 is too
    // small to recross. Deterministic, so exact.
    assert_eq!(crosses.len(), 1, "crosses found: {crosses:?}");
    let (bar, is_over, flow_confirms) = crosses[0];
    assert_eq!(bar, 7);
    assert!(!is_over, "the downturn is a cross-under");
    // Alternating unit buys/sells leave cvd at 0.0 on odd bars — the toy
    // flow filter reads exactly what the CVD pane would show.
    assert!(!flow_confirms, "cvd is 0.0 at bar 7, not positive");

    // 5. Descriptor metadata is available headlessly too (a bot config UI
    //    or report generator reads the same source of truth as the chart).
    assert!(host.descriptor(fast).unwrap().overlay);
    assert!(!host.descriptor(flow).unwrap().overlay);
}
