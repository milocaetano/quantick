//! Renko through the chart: the bricks the engine pins, whether the prints
//! arrive as history or one at a time, cut on the grid the chart's own tape
//! shows — and cut again when that grid turns out finer.

use super::*;
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_engine::{Side, fixture, golden};

const TRADES: &str = include_str!("../../../engine/tests/fixtures/renko_trades.csv");
const EXPECTED: &str = include_str!("../../../engine/tests/fixtures/renko_n3_expected.csv");
const WIN_TAPE: &str = include_str!("../../../engine/tests/fixtures/renko_win_50r_trades.csv");

fn spec(text: &str) -> BarConfiguration {
    BUILTIN_BARS.parse(text).unwrap()
}

/// Every brick `trades` close for `text` on a step the caller states — the
/// engine's own answer, for the chart to agree with.
fn engine_bricks(text: &str, step: Decimal, trades: &[Trade]) -> Vec<Bar> {
    let mut builder = spec(text).build_for(InstrumentFacts {
        price_step: Some(step),
    });
    let mut closed = Vec::new();
    for trade in trades {
        builder.push_into(trade, &mut closed);
    }
    closed
}

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: 1_000 + agg_id as i64 * 100,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

#[test]
fn the_chart_cuts_the_renko_golden_by_backfill_and_by_live_print() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let expected = fixture::parse_bars(EXPECTED).unwrap();

    let mut backfilled = ChartState::new(spec("renko:3"));
    backfilled.ingest_backfill(&trades);
    if let Some(report) = golden::diff_bars(&expected, backfilled.bars()) {
        panic!("backfilled: {report}");
    }

    let mut live = ChartState::new(spec("renko:3"));
    live.set_footprint_enabled(true);
    for trade in &trades {
        live.ingest_live(trade);
    }
    if let Some(report) = golden::diff_bars(&expected, live.bars()) {
        panic!("live: {report}");
    }
    assert_eq!(live.partial(), backfilled.partial());

    // Every brick keeps its ladder; one a print cleared on its way past
    // keeps an empty one.
    assert_eq!(live.bar_footprints().len(), live.bars().len());
    for (bar, ladder) in live.bars().iter().zip(live.bar_footprints()) {
        let prints: u64 = ladder
            .levels()
            .values()
            .map(|level| level.trade_count)
            .sum();
        assert_eq!(prints, bar.trade_count);
    }
}

#[test]
fn no_brick_is_cut_before_the_tape_has_shown_its_grid() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let mut chart = ChartState::new(spec("renko:3"));
    for trade in &trades[..8] {
        chart.ingest_live(trade);
    }
    assert_eq!(
        chart.tape_price_step(),
        None,
        "seven distances are not enough"
    );
    assert!(chart.bars().is_empty());
    assert_eq!(chart.partial().map(|bar| bar.trade_count), Some(8));

    let revision = chart.series_revision();
    chart.ingest_live(&trades[8]);
    assert_eq!(chart.tape_price_step(), Some(Decimal::ONE));
    assert_ne!(
        chart.series_revision(),
        revision,
        "the grid re-cut the series"
    );
    assert_eq!(chart.bars().len(), 6);
}

#[test]
fn a_grid_that_turns_out_finer_cuts_every_brick_again() {
    // Eight two-point moves name a two-point grid; the next print is one
    // point away and halves it — and with it every brick's height.
    let mut trades: Vec<Trade> = [100, 102, 100, 102, 104, 102, 104, 106, 104]
        .iter()
        .enumerate()
        .map(|(i, price)| print(i as u64, *price))
        .collect();
    let mut chart = ChartState::new(spec("renko:3"));
    for trade in &trades {
        chart.ingest_live(trade);
    }
    assert_eq!(chart.tape_price_step(), Some(Decimal::TWO));
    assert_eq!(
        chart.bars(),
        engine_bricks("renko:3", Decimal::TWO, &trades).as_slice()
    );

    trades.push(print(9, 105));
    let revision = chart.series_revision();
    chart.ingest_live(&trades[9]);
    assert_eq!(chart.tape_price_step(), Some(Decimal::ONE));
    assert_ne!(chart.series_revision(), revision);
    let finer = engine_bricks("renko:3", Decimal::ONE, &trades);
    assert_ne!(finer.len(), 1, "the finer grid has to cut differently");
    assert_eq!(chart.bars(), finer.as_slice());
}

#[test]
fn a_rule_that_does_not_measure_in_the_grid_is_never_recut_by_it() {
    let mut chart = ChartState::new(BarSpec::Tick(2));
    let mut revisions = Vec::new();
    for (i, price) in [100, 102, 100, 102, 104, 102, 104, 106, 104, 105]
        .iter()
        .enumerate()
    {
        chart.ingest_live(&print(i as u64, *price));
        revisions.push(chart.series_revision());
    }
    assert_eq!(chart.tape_price_step(), Some(Decimal::ONE));
    assert!(
        revisions.iter().all(|revision| *revision == revisions[0]),
        "live ingest only appended"
    );
}

#[test]
fn fifty_r_on_the_win_tape_cuts_the_bricks_the_engine_pins() {
    let trades = fixture::parse_trades(WIN_TAPE).unwrap();
    let expected = engine_bricks("renko:50", Decimal::from(5), &trades);
    assert_eq!(expected.len(), 18, "the reference and the brick before it");

    let mut backfilled = ChartState::new(spec("renko:50"));
    backfilled.ingest_backfill(&trades);
    assert_eq!(backfilled.tape_price_step(), Some(Decimal::from(5)));
    assert_eq!(backfilled.bars(), expected.as_slice());

    let mut live = ChartState::new(spec("renko:50"));
    for trade in &trades {
        live.ingest_live(trade);
    }
    assert_eq!(live.bars(), expected.as_slice());
}
