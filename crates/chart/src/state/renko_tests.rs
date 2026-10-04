//! Renko through the chart: the bricks every consumer cuts from WIN's tape,
//! whether the prints arrive as history or one at a time; the step read off
//! the builder's own prints, never off the chart's grid; and every way a chart
//! takes prints in showing what one contiguous tape showed, the prints held
//! while the step is read included.

use super::tape_identity_tests::{Shown, oracle};
use super::*;
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_engine::{RenkoBarBuilder, Side, fixture, golden};

const WIN_TAPE: &str = include_str!("../../../engine/tests/fixtures/renko_win_50r_trades.csv");
const WIN_EXPECTED: &str =
    include_str!("../../../engine/tests/fixtures/renko_win_50r_expected.csv");

fn spec(text: &str) -> BarConfiguration {
    BUILTIN_BARS.parse(text).unwrap()
}

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: 1_000 + agg_id as i64 * 100,
        price: Decimal::from(price),
        quantity: Decimal::from(1 + agg_id % 3),
        side: if agg_id.is_multiple_of(2) {
            Side::Buy
        } else {
            Side::Sell
        },
    }
}

fn prints(prices: &[i64]) -> Vec<Trade> {
    prices
        .iter()
        .enumerate()
        .map(|(i, price)| print(i as u64, *price))
        .collect()
}

/// Every ladder holds exactly its own brick's prints, the forming one too.
fn ladders_hold_their_bricks(chart: &ChartState) {
    let ladder_prints =
        |ladder: &BarFootprint| -> u64 { ladder.levels().values().map(|l| l.trade_count).sum() };
    assert_eq!(chart.bar_footprints().len(), chart.bars().len());
    for (bar, ladder) in chart.bars().iter().zip(chart.bar_footprints()) {
        assert_eq!(ladder_prints(ladder), bar.trade_count);
        let buy: Decimal = ladder.levels().values().map(|level| level.buy).sum();
        assert_eq!(buy, bar.buy_volume);
    }
    assert_eq!(
        chart.partial_footprint().map(ladder_prints),
        chart.partial().map(|bar| bar.trade_count)
    );
}

/// The chart's half of the three-consumer proof: the bricks the engine pins
/// for WIN's tape, by backfill and by live print. The backtest cuts the same
/// file (`crates/backtest/tests/harness.rs`); no crate links both consumers,
/// so the file is what they agree through.
#[test]
fn the_chart_cuts_the_bricks_every_consumer_pins_for_win() {
    let trades = fixture::parse_trades(WIN_TAPE).unwrap();
    let expected = fixture::parse_bars(WIN_EXPECTED).unwrap();

    let mut backfilled = ChartState::new(spec("renko:50"));
    backfilled.ingest_backfill(&trades);
    if let Some(report) = golden::diff_bars(&expected, backfilled.bars()) {
        panic!("backfilled: {report}");
    }

    let mut live = ChartState::new(spec("renko:50"));
    live.set_footprint_enabled(true);
    for trade in &trades {
        live.ingest_live(trade);
    }
    if let Some(report) = golden::diff_bars(&expected, live.bars()) {
        panic!("live: {report}");
    }
    assert_eq!(live.partial(), backfilled.partial());
    ladders_hold_their_bricks(&live);
}

#[test]
fn the_prints_held_while_the_step_is_read_land_as_appended_bricks() {
    // A climb of one point a print: renko:3 holds sixty-four prints while
    // they show the step, and the sixty-fifth cuts every brick they closed.
    // Live ingest still only appends, so the series identity never moves.
    let climb = prints(&(100..=164).collect::<Vec<_>>());
    let mut chart = ChartState::new(spec("renko:3"));
    chart.set_footprint_enabled(true);
    let revision = chart.series_revision();
    for trade in &climb[..64] {
        chart.ingest_live(trade);
    }
    assert!(chart.bars().is_empty(), "the step is not read yet");
    assert_eq!(chart.partial().map(|bar| bar.trade_count), Some(64));
    ladders_hold_their_bricks(&chart);

    chart.ingest_live(&climb[64]);
    assert_eq!(chart.bars().len(), 31);
    assert_eq!(
        chart.series_revision(),
        revision,
        "the bricks were appended"
    );
    ladders_hold_their_bricks(&chart);
}

#[test]
fn the_charts_own_grid_narrowing_never_recuts_a_renko_series() {
    // Two-point moves freeze the builder's step at two. The one-point print
    // after it narrows the chart's grid to one; the bricks stay cut on two.
    let mut prices: Vec<i64> = (0..65).map(|i| 100 + 2 * (i % 2)).collect();
    prices.extend([105, 110, 101]);
    let trades = prints(&prices);
    let mut chart = ChartState::new(spec("renko:3"));
    let mut revisions = Vec::new();
    for trade in &trades {
        chart.ingest_live(trade);
        revisions.push(chart.series_revision());
    }
    assert_eq!(chart.tape_price_step(), Some(Decimal::ONE));
    assert!(
        revisions.iter().all(|revision| *revision == revisions[0]),
        "live ingest only appended"
    );
    let told = golden::replay(&mut RenkoBarBuilder::with_step(3, Decimal::TWO), &trades);
    assert_eq!(chart.bars(), told.as_slice());
}

/// A walk on a five-point grid: a climb through its first seventy prints, so
/// the prints held while the step is read close bricks of their own, then a
/// zig-zag with a spike every so often that clears several levels at once.
fn walk() -> Vec<Trade> {
    let ticks = (0..700_i64).map(|i| {
        let zigzag = if i < 70 {
            i
        } else {
            70 + ((i % 60) - 30).abs()
        };
        let spike = if i % 97 == 0 { 9 } else { 0 };
        5 * (20_000 + zigzag + spike)
    });
    prints(&ticks.collect::<Vec<_>>())
}

/// Every way a chart takes prints in shows what one contiguous tape showed,
/// with the step read off the first sixty-four distances wherever they
/// arrived: in history, live, or across the seam between the two.
#[test]
fn every_way_in_shows_what_a_contiguous_renko_tape_showed() {
    let tape = walk();
    let renko = spec("renko:3");
    for footprint in [false, true] {
        let what = format!("footprint {footprint}");
        let fresh = || {
            let mut chart = ChartState::new(renko);
            chart.set_footprint_enabled(footprint);
            chart
        };
        let (older, rest) = tape.split_at(tape.len() * 2 / 5);
        let (middle, newer) = rest.split_at(rest.len() / 2);

        let mut backfilled = fresh();
        backfilled.ingest_backfill(&tape);
        assert!(backfilled.bars().len() > 60, "{what}: it cuts");
        assert_eq!(
            Shown::of(&backfilled),
            oracle(renko, &tape, tape.len(), footprint),
            "backfill: {what}"
        );

        // Ten prints of history show too little of the grid: the print that
        // reads the step arrives live and cuts history and live alike.
        let mut seam = fresh();
        seam.ingest_backfill(&tape[..10]);
        for trade in &tape[10..] {
            seam.ingest_live(trade);
        }
        assert_eq!(
            Shown::of(&seam),
            oracle(renko, &tape, 10, footprint),
            "the step read across the seam: {what}"
        );

        let mut paged = fresh();
        paged.ingest_backfill(middle);
        paged.prepend_history(older);
        for trade in newer {
            paged.ingest_live(trade);
        }
        assert_eq!(
            Shown::of(&paged),
            oracle(renko, &tape, older.len() + middle.len(), footprint),
            "older history prepended: {what}"
        );

        let mut seeded = fresh();
        let split = paged.backfill_trade_count();
        seeded.ingest_backfill(paged.trades().range(..split));
        for trade in paged.trades().since(split) {
            seeded.ingest_live(trade);
        }
        assert_eq!(Shown::of(&seeded), Shown::of(&paged), "seeded: {what}");

        let mut switched = ChartState::new(BarSpec::Tick(7));
        switched.ingest_backfill(older);
        for trade in rest {
            switched.ingest_live(trade);
        }
        switched.set_spec(renko);
        switched.set_footprint_enabled(footprint);
        assert_eq!(
            Shown::of(&switched),
            oracle(renko, &tape, older.len(), footprint),
            "switched, then refolded: {what}"
        );
    }
}
