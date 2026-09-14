//! The chart shows the same bars whether its tape is a `Vec` or chunks.
//!
//! The oracle is what the chart showed with a contiguous tape, reduced to
//! what that tape did: hand each print, in order, to a fresh bar builder and
//! footprint series. Every way a [`ChartState`] takes prints in — a backfill,
//! live prints, both, a page of older history prepended, a second chart
//! seeded from the first's tape, a spec switch, the footprint turned on late —
//! must give the same closed bars, forming bar, backfill boundary, footprint
//! ladders and retained prints, compared by their `Debug` text so a
//! `Decimal`'s scale counts too. The tapes are the engine's golden fixtures,
//! each alone and all of them repeated past three chunk boundaries.

use super::*;
use quantick_engine::fixture::parse_trades;
use quantick_engine::trade_tape::CHUNK_TRADES;

/// Everything a chart shows from its tape, as text.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    bars: String,
    partial: String,
    boundary: Option<usize>,
    footprints: String,
    partial_footprint: String,
    trades: String,
}

impl Shown {
    fn of(state: &ChartState) -> Self {
        Self {
            bars: format!("{:?}", state.bars()),
            partial: format!("{:?}", state.partial()),
            boundary: state.backfill_boundary(),
            footprints: format!("{:?}", state.bar_footprints()),
            partial_footprint: format!("{:?}", state.partial_footprint()),
            trades: format!("{:?}", state.trades().iter().collect::<Vec<_>>()),
        }
    }
}

/// What a contiguous tape showed: `tape` folded in order, the first
/// `backfilled` of it as history.
fn oracle(spec: &BarSpec, tape: &[Trade], backfilled: usize, footprint: bool) -> Shown {
    let mut builder = spec.build();
    let mut footprints = FootprintSeries::new(footprint_series::default_group());
    let mut bars = Vec::new();
    let mut boundary = None;
    for (index, trade) in tape.iter().enumerate() {
        if index == backfilled {
            boundary = Some(bars.len());
        }
        let closed = builder.push(trade);
        if footprint {
            footprints.observe(trade, closed.as_ref());
        }
        bars.extend(closed);
    }
    Shown {
        bars: format!("{bars:?}"),
        partial: format!("{:?}", builder.partial()),
        boundary: boundary.or(Some(bars.len())),
        footprints: format!("{:?}", footprints.closed()),
        partial_footprint: format!("{:?}", footprints.partial()),
        trades: format!("{:?}", tape.iter().collect::<Vec<_>>()),
    }
}

/// The engine's golden trade tapes, each alone, then all of them joined and
/// repeated — ids and stamps moved on each lap — past three chunk boundaries.
fn tapes() -> Vec<Vec<Trade>> {
    let files = [
        include_str!("../../../engine/tests/fixtures/sample_trades.csv"),
        include_str!("../../../engine/tests/fixtures/tick_trades.csv"),
        include_str!("../../../engine/tests/fixtures/time_trades.csv"),
        include_str!("../../../engine/tests/fixtures/volume_trades.csv"),
        include_str!("../../../engine/tests/fixtures/dollar_trades.csv"),
        include_str!("../../../engine/tests/fixtures/imbalance_trades.csv"),
    ];
    let mut tapes: Vec<Vec<Trade>> = files
        .iter()
        .map(|text| parse_trades(text).expect("a golden fixture parses"))
        .collect();
    let joined: Vec<Trade> = tapes.concat();
    let span = joined.iter().map(|t| t.timestamp_ms).max().unwrap_or(0)
        - joined.iter().map(|t| t.timestamp_ms).min().unwrap_or(0)
        + 1;
    let mut long = Vec::new();
    let mut lap = 0_u64;
    while long.len() <= 3 * CHUNK_TRADES {
        long.extend(joined.iter().map(|trade| Trade {
            agg_id: trade.agg_id + lap * 1_000_000,
            timestamp_ms: trade.timestamp_ms + lap as i64 * span,
            ..trade.clone()
        }));
        lap += 1;
    }
    tapes.push(long);
    tapes
}

fn specs() -> Vec<BarSpec> {
    vec![
        BarSpec::Tick(3),
        BarSpec::Tick(50),
        BarSpec::Volume(Decimal::from(5)),
        BarSpec::Dollar(Decimal::from(500)),
        BarSpec::Time(1_000),
        BarSpec::Imbalance(ImbalanceUnit::Trades, 8),
    ]
}

/// A chart on `spec` with the footprint as asked, before any print.
fn chart(spec: &BarSpec, footprint: bool) -> ChartState {
    let mut state = ChartState::new(spec.clone());
    state.set_footprint_enabled(footprint);
    state
}

#[test]
fn every_way_in_shows_what_a_contiguous_tape_showed() {
    for tape in tapes() {
        let long = tape.len() > CHUNK_TRADES;
        // The long tape crosses chunks; one spec with the ladder on and one
        // without is enough there, and keeps the test quick.
        let specs = if long {
            vec![BarSpec::Tick(50), BarSpec::Time(1_000)]
        } else {
            specs()
        };
        for spec in &specs {
            for footprint in [false, true] {
                let what = format!("{spec:?}, footprint {footprint}, {} prints", tape.len());
                let (older, rest) = tape.split_at(tape.len() * 2 / 5);
                let (middle, newer) = rest.split_at(rest.len() / 2);

                let mut backfilled = chart(spec, footprint);
                backfilled.ingest_backfill(&tape);
                assert_eq!(
                    Shown::of(&backfilled),
                    oracle(spec, &tape, tape.len(), footprint),
                    "backfill: {what}"
                );

                let mut live = chart(spec, footprint);
                for trade in &tape {
                    live.ingest_live(trade);
                }
                let mut shown = oracle(spec, &tape, 0, footprint);
                shown.boundary = None;
                assert_eq!(Shown::of(&live), shown, "live: {what}");

                let mut mixed = chart(spec, footprint);
                mixed.ingest_backfill(older);
                for trade in rest {
                    mixed.ingest_live(trade);
                }
                assert_eq!(
                    Shown::of(&mixed),
                    oracle(spec, &tape, older.len(), footprint),
                    "backfill then live: {what}"
                );

                let mut paged = chart(spec, footprint);
                paged.ingest_backfill(middle);
                paged.prepend_history(older);
                for trade in newer {
                    paged.ingest_live(trade);
                }
                assert_eq!(
                    Shown::of(&paged),
                    oracle(spec, &tape, older.len() + middle.len(), footprint),
                    "older history prepended: {what}"
                );

                // A second view seeded from the first's tape, as a split does.
                let mut seeded = chart(spec, footprint);
                let split = mixed.backfill_trade_count();
                seeded.ingest_backfill(mixed.trades().range(..split));
                for trade in mixed.trades().since(split) {
                    seeded.ingest_live(trade);
                }
                assert_eq!(
                    Shown::of(&seeded),
                    Shown::of(&mixed),
                    "seeded from another chart's tape: {what}"
                );

                if long {
                    // Rebuilt from the tape: a spec switch, then the footprint
                    // turned on after the fact.
                    let mut switched = chart(&BarSpec::Tick(7), false);
                    switched.ingest_backfill(older);
                    for trade in rest {
                        switched.ingest_live(trade);
                    }
                    switched.set_spec(spec.clone());
                    switched.set_footprint_enabled(footprint);
                    assert_eq!(
                        Shown::of(&switched),
                        oracle(spec, &tape, older.len(), footprint),
                        "switched spec, then refolded: {what}"
                    );
                }
            }
        }
    }
}

/// A synthetic tape past three chunk boundaries, one print every 10 ms, with
/// a deal-counter reading ahead of every 50th print — the venue's counter
/// moving about three deals a print — as a MetaTrader B3 feed sends them.
fn deal_tape() -> (Vec<Trade>, Vec<DealSample>) {
    let count = 3 * CHUNK_TRADES + 1_234;
    let tape: Vec<Trade> = (0..count)
        .map(|index| {
            let index = index as u64;
            Trade {
                agg_id: index + 1,
                timestamp_ms: 1_000 + index as i64 * 10,
                price: Decimal::from(100 + (index * 7) % 13),
                quantity: Decimal::from(1 + index % 3),
                side: if index.is_multiple_of(2) {
                    quantick_engine::Side::Buy
                } else {
                    quantick_engine::Side::Sell
                },
            }
        })
        .collect();
    let readings = tape
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 50 == 0)
        .map(|(index, trade)| DealSample {
            time_ms: trade.timestamp_ms - 1,
            session_deals: 5 + 3 * index as u64 + (index as u64 % 7),
        })
        .collect();
    (tape, readings)
}

/// What a contiguous tape showed under a deal rule: every reading handed to
/// a fresh builder — interleaved ahead of the prints it precedes when
/// `interleaved`, all first otherwise, as a rebuild does — then the prints
/// folded in order, the first `backfilled` of them as history.
fn deal_oracle(
    spec: &BarSpec,
    tape: &[Trade],
    readings: &[DealSample],
    interleaved: bool,
    backfilled: usize,
    footprint: bool,
) -> Shown {
    let mut builder = spec.build();
    let mut footprints = FootprintSeries::new(footprint_series::default_group());
    if !interleaved {
        seed_deal_counter(&mut *builder, readings);
    }
    let mut pending = readings.iter().peekable();
    let mut bars = Vec::new();
    let mut boundary = None;
    for (index, trade) in tape.iter().enumerate() {
        if index == backfilled {
            boundary = Some(bars.len());
        }
        while interleaved
            && pending
                .peek()
                .is_some_and(|r| r.time_ms < trade.timestamp_ms)
        {
            let reading = *pending.next().expect("peeked");
            seed_deal_counter(&mut *builder, &[reading]);
        }
        bars.extend(fold_print(&mut *builder, &mut footprints, footprint, trade));
    }
    Shown {
        bars: format!("{bars:?}"),
        partial: format!("{:?}", builder.partial()),
        boundary: boundary.or(Some(bars.len())),
        footprints: format!("{:?}", footprints.closed()),
        partial_footprint: format!("{:?}", footprints.partial()),
        trades: format!("{:?}", tape.iter().collect::<Vec<_>>()),
    }
}

/// #306's deal bars over #431's chunked tape (synchronization X2): the
/// retained readings and every path that replays them — a backfill, live
/// readings interleaved with live prints, a page of older history, a second
/// view seeded from the first's tape and readings, a switch into the deal
/// rule, a footprint refold, and a series reset that keeps the readings —
/// cut the same bars a contiguous tape cut, across three chunk boundaries.
#[test]
fn deal_bars_on_a_chunked_tape_show_what_a_contiguous_tape_showed() {
    let (tape, readings) = deal_tape();
    let spec = BarSpec::Trades(500);
    for footprint in [false, true] {
        let what = format!("footprint {footprint}, {} prints", tape.len());
        let rebuilt = deal_oracle(&spec, &tape, &readings, false, tape.len(), footprint);
        assert!(
            rebuilt.bars.matches("Bar {").count() > 2,
            "the rule cuts bars: {what}"
        );

        let mut backfilled = chart(&spec, footprint);
        backfilled.observe_deals_batch(&readings);
        backfilled.ingest_backfill(&tape);
        // A batch of readings reaches the bars through the rebuild its caller
        // runs straight after (`Tab::retain_deal_samples`).
        backfilled.rebuild_bars();
        assert_eq!(Shown::of(&backfilled), rebuilt, "backfill: {what}");

        let mut live = chart(&spec, footprint);
        let mut pending = readings.iter().peekable();
        for trade in &tape {
            while let Some(reading) = pending.next_if(|r| r.time_ms < trade.timestamp_ms) {
                live.observe_deals(*reading);
            }
            live.ingest_live(trade);
        }
        let mut shown = deal_oracle(&spec, &tape, &readings, true, 0, footprint);
        shown.boundary = None;
        assert_eq!(
            Shown::of(&live),
            shown,
            "live, readings interleaved: {what}"
        );

        let (older, rest) = tape.split_at(tape.len() * 2 / 5);
        let mut paged = chart(&spec, footprint);
        paged.observe_deals_batch(&readings);
        paged.ingest_backfill(rest);
        paged.prepend_history(older);
        assert_eq!(
            Shown::of(&paged),
            rebuilt,
            "older history prepended: {what}"
        );

        let mut seeded = chart(&spec, footprint);
        seeded.observe_deals_batch(backfilled.deal_samples());
        seeded.rebuild_bars();
        let split = backfilled.backfill_trade_count();
        seeded.ingest_backfill(backfilled.trades().range(..split));
        for trade in backfilled.trades().since(split) {
            seeded.ingest_live(trade);
        }
        assert_eq!(
            Shown::of(&seeded),
            Shown::of(&backfilled),
            "seeded from another chart's tape and readings: {what}"
        );

        let mut switched = chart(&BarSpec::Tick(7), false);
        switched.observe_deals_batch(&readings);
        switched.ingest_backfill(&tape);
        switched.set_spec(spec.clone());
        switched.set_footprint_enabled(footprint);
        assert_eq!(
            Shown::of(&switched),
            rebuilt,
            "switched into the deal rule: {what}"
        );

        backfilled.reset_series(spec.clone());
        assert_eq!(backfilled.deal_samples(), readings.as_slice());
        // A reset is a fresh chart, the footprint off, as `ChartState::new`
        // is; the pane turns it back on as it does after any reset.
        backfilled.set_footprint_enabled(footprint);
        backfilled.ingest_backfill(&tape);
        assert_eq!(
            Shown::of(&backfilled),
            rebuilt,
            "reset, readings kept: {what}"
        );
    }
}
