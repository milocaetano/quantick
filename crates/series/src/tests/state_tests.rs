use super::*;
use quantick_engine::Side;
use std::str::FromStr as _;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn trade(agg_id: u64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: 1000 + agg_id as i64 * 100,
        price: dec("100"),
        quantity: dec("1.0"),
        side: Side::Buy,
    }
}

/// A print at `price`, with everything else fixed — these tests are about
/// the price grid and nothing else.
fn print_at(agg_id: u64, price: &str) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: 1000 + agg_id as i64 * 100,
        price: dec(price),
        quantity: dec("1.0"),
        side: Side::Buy,
    }
}

#[test]
fn a_chart_names_the_grid_its_own_tape_prints_on() {
    // B3's mini index moves in five-point steps. Nothing states that here:
    // the chart is told only the prints, exactly as a market replay tells
    // it, and the grid is what it works out.
    let mut chart = ChartState::new(BarSpec::Tick(1000));
    for (i, price) in [
        "174565", "174570", "174560", "174570", "174585", "174580", "174570", "174575", "174590",
        "174585",
    ]
    .iter()
    .enumerate()
    {
        chart.ingest_live(&print_at(i as u64, price));
    }
    assert_eq!(chart.tape_price_step(), Some(dec("5")));
}

#[test]
fn a_chart_names_the_price_its_tape_trades_at_and_then_holds_still() {
    // The other half of sizing a row. A tick is a true fact about an
    // instrument and still a useless row where the price dwarfs it, so the
    // rule needs the magnitude too — and it has to hold still, because
    // every change to it re-buckets and re-folds every retained ladder.
    let mut chart = ChartState::new(BarSpec::Tick(1000));
    assert_eq!(chart.tape_reference_price(), None, "nothing has printed");

    chart.ingest_live(&print_at(0, "80000"));
    assert_eq!(chart.tape_reference_price(), Some(dec("80000")));

    // A session that runs to a very different price is still the same
    // market at the same magnitude, and re-grouping it mid-session would
    // be a refold that says almost nothing new.
    for (i, price) in ["81000", "84000", "88000"].iter().enumerate() {
        chart.ingest_live(&print_at(i as u64 + 1, price));
    }
    assert_eq!(
        chart.tape_reference_price(),
        Some(dec("80000")),
        "the magnitude drifted with the tape instead of holding at the first print",
    );
}

#[test]
fn backfilled_history_names_the_price_as_well_as_the_grid() {
    // History arrives as one batch before the first live print, and it is
    // what a chart's very first screen is drawn from. A magnitude learned
    // only from live prints would size that screen from nothing.
    let mut chart = ChartState::new(BarSpec::Tick(1000));
    chart.ingest_backfill(&[print_at(0, "140000"), print_at(1, "140005")]);
    assert_eq!(chart.tape_reference_price(), Some(dec("140000")));
}

#[test]
fn a_chart_that_has_not_been_told_enough_names_nothing() {
    // Silence is not an answer of "fine" — a caller sizing rows from this
    // has to keep what it had, and that only works if it can tell the two
    // apart.
    let mut chart = ChartState::new(BarSpec::Tick(1000));
    assert_eq!(chart.tape_price_step(), None);
    for i in 0..20 {
        chart.ingest_live(&print_at(i, "174565"));
    }
    assert_eq!(
        chart.tape_price_step(),
        None,
        "a run of prints at one price shows no distance and so no grid",
    );
}

#[test]
fn backfilled_history_names_the_grid_as_well_as_live_prints_do() {
    // History arrives as one batch before the first live print. A chart
    // that only learned the grid from live prints would draw its first
    // screen — the whole backfill — on the wrong rows.
    let mut chart = ChartState::new(BarSpec::Tick(1000));
    let history: Vec<Trade> = [
        "5216.0", "5216.5", "5217.0", "5216.5", "5215.5", "5217.5", "5218.0", "5217.0", "5216.0",
        "5215.5",
    ]
    .iter()
    .enumerate()
    .map(|(i, price)| print_at(i as u64, price))
    .collect();
    chart.ingest_backfill(&history);
    assert_eq!(chart.tape_price_step(), Some(dec("0.5")));
}

/// A recorded day loaded behind the live tape is older than the live
/// readings: it lands in order and the rebuild cuts from the whole
/// series, while a reading delivered twice is held once.
/// A reading older than the newest held is kept for the next rebuild,
/// not cut in at once — the live edge is never re-cut per reading —
/// and is held once however many readings share its millisecond.
/// A reading at the newest held millisecond with a *lower* count is a
/// regression the sampler forwards — a late poll. The builder reads a
/// small dip as exactly that, live and rebuilt alike: no bar ends.
#[test]
fn a_lower_reading_at_the_same_millisecond_is_a_late_poll_live_and_rebuilt() {
    let mut s = ChartState::new(BarSpec::Trades(2));
    let at = |time_ms: i64, session_deals: u64| DealSample {
        time_ms,
        session_deals,
    };
    s.observe_deals(at(1_099, 5));
    s.observe_deals(at(1_099, 4));
    s.ingest_live(&trade(1));
    assert!(s.bars().is_empty(), "no rollover bar: {:?}", s.bars());
    assert_eq!(
        s.deal_samples().len(),
        2,
        "held, for the file and the rebuild"
    );
    s.rebuild_bars();
    assert!(s.bars().is_empty(), "the rebuild agrees: {:?}", s.bars());
}

/// A rollover that ends the bar on a print the builder leaves uncounted
/// — last night's bar, this morning's first print — closes the ladder
/// on what it held and folds the print nowhere, so ladders and bars
/// keep the same indices.
#[test]
fn a_rollover_on_an_uncounted_print_keeps_the_ladders_aligned() {
    let mut s = ChartState::new(BarSpec::Trades(1_000));
    s.set_footprint_enabled(true);
    // Two readings around the first print give the rate (100 per
    // contract); the second print is credited it and forms a bar.
    s.observe_deals(DealSample {
        time_ms: 999,
        session_deals: 5_000_300,
    });
    s.ingest_live(&trade(1));
    s.observe_deals(DealSample {
        time_ms: 1_199,
        session_deals: 5_000_400,
    });
    s.ingest_live(&trade(2));
    assert_eq!(
        s.partial_footprint().map(|_| ()),
        Some(()),
        "a ladder is forming"
    );
    // The session restarts, and the next print is far behind the
    // reading that ended it — beyond what a reading holds for.
    s.observe_deals(DealSample {
        time_ms: 1_299,
        session_deals: 3,
    });
    s.ingest_live(&Trade {
        agg_id: 3,
        timestamp_ms: 1_300 + 11 * 60_000,
        price: dec("100"),
        quantity: dec("1.0"),
        side: Side::Buy,
    });
    assert_eq!(s.bars().len(), 1, "last night's bar ended");
    assert_eq!(s.bar_footprints().len(), 1, "and its ladder with it");
    assert_eq!(s.uncounted_trades(), 2, "the first print, and this one");
    assert!(s.partial_footprint().is_none(), "nothing opened the next");
}

/// A lower reading at the same millisecond, batched in behind a live
/// series that already saw the higher one, is replayed in arrival order:
/// the rebuild ignores the dip as the live edge did.
#[test]
fn a_same_millisecond_dip_batched_in_cuts_as_the_live_edge_did() {
    let at = |time_ms: i64, session_deals: u64| DealSample {
        time_ms,
        session_deals,
    };
    // Prints at 1100..1400; a reading between each pair, and at 1 299
    // the higher reading first, the dip after it.
    let mut live = ChartState::new(BarSpec::Trades(1_000));
    live.observe_deals(at(999, 1_000));
    live.ingest_live(&trade(1));
    live.observe_deals(at(1_199, 1_500));
    live.ingest_live(&trade(2));
    live.observe_deals(at(1_299, 2_600));
    live.observe_deals(at(1_299, 1_700));
    live.ingest_live(&trade(3));
    live.ingest_live(&trade(4));
    assert_eq!(live.bars().len(), 3, "{:?}", live.bars());

    let mut rebuilt = ChartState::new(BarSpec::Trades(1_000));
    rebuilt.observe_deals_batch(live.deal_samples());
    rebuilt.ingest_backfill(&[trade(1), trade(2), trade(3), trade(4)]);
    rebuilt.rebuild_bars();
    assert_eq!(rebuilt.bars(), live.bars());
    assert_eq!(rebuilt.uncounted_trades(), live.uncounted_trades());
}

/// A reading held for the next rebuild reaches the ladders only with
/// the bars: a refold meanwhile leaves it out, and the ladders close
/// where the bars are.
#[test]
fn a_refold_with_a_held_reading_rebuilds_the_bars_too() {
    let mut s = ChartState::new(BarSpec::Trades(2));
    s.set_footprint_enabled(true);
    let at = |time_ms: i64, session_deals: u64| DealSample {
        time_ms,
        session_deals,
    };
    s.observe_deals(at(1_099, 1));
    s.observe_deals(at(1_299, 6));
    for id in 1..=3 {
        s.ingest_live(&trade(id));
    }
    s.observe_deals(at(1_199, 4)); // held
    s.set_footprint_group(dec("2"));
    assert_eq!(s.bar_footprints().len(), s.bars().len());
}

#[test]
fn an_out_of_order_reading_is_held_for_the_next_rebuild() {
    let mut s = ChartState::new(BarSpec::Trades(2));
    let at = |time_ms: i64, session_deals: u64| DealSample {
        time_ms,
        session_deals,
    };
    s.observe_deals(at(1_099, 1));
    s.observe_deals(at(1_299, 6));
    for id in 1..=3 {
        s.ingest_live(&trade(id));
    }
    assert_eq!(s.bars().len(), 1, "the print at 1 300 sees 6 and closes 2");
    // The reading the bridge had missed: 1 199, at 4. Held, not cut in.
    s.observe_deals(at(1_199, 4));
    assert_eq!(s.bars().len(), 1, "the live edge is not re-cut");
    assert_eq!(s.deal_samples().len(), 3);
    // A reading re-delivered is held once, whatever else shares its
    // millisecond.
    s.observe_deals(at(1_099, 1));
    s.observe_deals(at(1_099, 1));
    assert_eq!(s.deal_samples().len(), 3);
    // The next rebuild: the print at 1 200 crosses 4 and closes 2, the
    // one at 1 300 crosses 6.
    s.rebuild_bars();
    assert_eq!(s.bars().len(), 2, "{:?}", s.bars());
}

#[test]
fn older_readings_land_in_order_and_duplicates_are_held_once() {
    let mut s = ChartState::new(BarSpec::Trades(2));
    let at = |time_ms: i64, session_deals: u64| DealSample {
        time_ms,
        session_deals,
    };
    s.observe_deals(at(1_500, 10));
    s.observe_deals(at(1_500, 10));
    s.observe_deals(at(1_100, 3));
    s.observe_deals(at(1_300, 6));
    s.observe_deals(at(1_100, 3));
    let times: Vec<i64> = s.deal_samples().iter().map(|d| d.time_ms).collect();
    assert_eq!(times, [1_100, 1_300, 1_500]);
    // A whole file behind the live readings: one sort, the same series.
    s.observe_deals_batch(&[at(1_300, 6), at(1_000, 1), at(1_200, 4)]);
    let times: Vec<i64> = s.deal_samples().iter().map(|d| d.time_ms).collect();
    assert_eq!(times, [1_000, 1_100, 1_200, 1_300, 1_500]);
    // Two readings at one millisecond, held live and batched again from
    // the file: each pair folds onto itself, in reading order, so the
    // rebuild never sees the lower one return.
    s.observe_deals(at(1_600, 12));
    s.observe_deals(at(1_600, 14));
    s.observe_deals_batch(&[at(1_600, 12), at(1_600, 14)]);
    let tail: Vec<(i64, u64)> = s
        .deal_samples()
        .iter()
        .skip(5)
        .map(|d| (d.time_ms, d.session_deals))
        .collect();
    assert_eq!(tail, [(1_600, 12), (1_600, 14)]);
    // Prints at 1100..1500 (see `trade`): after a rebuild the first —
    // under the first window, no rate yet — is uncounted, and the
    // readings cut three bars from the rest.
    for id in 1..=5 {
        s.ingest_live(&trade(id));
    }
    s.rebuild_bars();
    assert_eq!(s.uncounted_trades(), 1);
    assert_eq!(s.bars().len(), 3, "{:?}", s.bars());
}

/// Recording belongs to the asset: the readings a tab retains survive
/// every switch of the bar rule, so `tick → trades → tick → trades`
/// cuts the same deal bars each time, and the prints before the first
/// reading are counted rather than folded into a bar nobody cut.
/// A series reset — a feed reload, a replay seek — keeps the readings:
/// the prints replayed afterwards join to them as the first pass did.
/// A print a deal bar leaves uncounted belongs to no bar, so it belongs
/// to no ladder either: the footprint series stays aligned with the bars
/// instead of drifting — and asserting — on the first print before a
/// reading. Found by the trader's own workspace, footprint on.
#[test]
fn uncounted_prints_form_no_footprint_ladder() {
    let mut s = ChartState::new(BarSpec::Trades(2_000));
    s.set_footprint_enabled(true);
    s.ingest_backfill(&[trade(1), trade(2)]);
    assert_eq!(s.uncounted_trades(), 2);
    assert!(
        s.partial_footprint().is_none(),
        "nothing folds before a reading"
    );
    s.observe_deals(DealSample {
        time_ms: 1_299,
        session_deals: 3_990,
    });
    s.ingest_live(&trade(3));
    // The window completes at 9 deals over one contract: the next
    // print is credited 9 and crosses 4 000.
    s.observe_deals(DealSample {
        time_ms: 1_399,
        session_deals: 3_999,
    });
    s.ingest_live(&trade(4));
    assert_eq!(s.bars().len(), 1);
    assert_eq!(s.bars()[0].trade_count, 1);
    assert_eq!(
        s.uncounted_trades(),
        3,
        "two before any reading, one before a rate"
    );
    assert_eq!(s.bar_footprints().len(), 1);
    // A refold and a rebuild keep the alignment too.
    s.set_footprint_group(dec("2"));
    assert_eq!(s.bar_footprints().len(), 1);
    s.set_spec(BarSpec::Tick(2));
    s.set_spec(BarSpec::Trades(2_000));
    assert_eq!(s.bar_footprints().len(), s.bars().len());
}

#[test]
fn a_series_reset_keeps_the_readings() {
    let mut s = ChartState::new(BarSpec::Trades(2_000));
    s.observe_deals(DealSample {
        time_ms: 1_000,
        session_deals: 3_990,
    });
    s.observe_deals(DealSample {
        time_ms: 1_150,
        session_deals: 3_999,
    });
    s.ingest_live(&trade(1));
    s.ingest_live(&trade(2));
    assert_eq!(
        s.bars().len(),
        1,
        "the print at 1 200 is credited the window's 9 and crosses 4 000"
    );

    s.reset_series(BarSpec::Trades(2_000));
    assert!(s.bars().is_empty(), "the series is gone");
    assert_eq!(s.deal_samples().len(), 2, "the readings are not");
    s.ingest_backfill(&[trade(1), trade(2)]);
    assert_eq!(s.bars().len(), 1, "the replayed prints cut the same bar");
}

#[test]
fn deal_readings_survive_a_switch_of_the_bar_rule() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    // Prints at 1100, 1200, ... (see `trade`); readings just ahead of
    // the prints at 1300 and 1500, as the feed sends them.
    s.ingest_backfill(&[trade(1), trade(2)]);
    s.observe_deals(DealSample {
        time_ms: 1299,
        session_deals: 3_990,
    });
    s.ingest_live(&trade(3));
    s.ingest_live(&trade(4));
    s.observe_deals(DealSample {
        time_ms: 1499,
        session_deals: 4_003,
    });
    s.ingest_live(&trade(5));
    assert_eq!(s.bars().len(), 2, "two tick bars of two prints");
    assert_eq!(
        s.uncounted_trades(),
        0,
        "a tick rule counts nothing as uncounted"
    );

    s.set_spec(BarSpec::Trades(2_000));
    assert!(
        s.bars().is_empty(),
        "4 003 plus one print's estimate reaches no multiple"
    );
    assert_eq!(
        s.partial().map(|bar| bar.trade_count),
        Some(1),
        "the print at 1500, credited the first window's rate"
    );
    assert_eq!(
        s.uncounted_trades(),
        4,
        "two before the first reading, two before the first rate"
    );
    assert_eq!(s.deal_samples().len(), 2);

    s.set_spec(BarSpec::Tick(2));
    assert_eq!(s.bars().len(), 2);
    s.set_spec(BarSpec::Trades(2_000));
    assert_eq!(
        s.uncounted_trades(),
        4,
        "the readings were retained across both switches"
    );
    // Prints with a rate advance the countdown in estimated deals, not
    // prints: the window's 13 deals over two contracts credit the print
    // at 1500 six and a half, past the re-anchored 4 003.
    let (progress, unit) = s.progress().expect("a deal bar runs toward a fixed count");
    assert_eq!(unit, "deals");
    // From where the forming bar opened — the re-anchored 4 003 — not
    // from the previous multiple.
    assert_eq!(progress.done, dec("6.5"));
}

#[test]
fn backfill_and_live_go_through_the_same_builder() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    s.ingest_backfill(&[trade(1), trade(2), trade(3)]);
    assert_eq!(s.bars().len(), 1);
    assert_eq!(s.backfill_boundary(), Some(1));

    s.ingest_live(&trade(4));
    assert_eq!(s.bars().len(), 2);
    assert_eq!(s.backfill_boundary(), Some(1), "boundary does not move");
}

#[test]
fn switching_bar_type_rebuilds_from_retained_trades() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    let trades: Vec<Trade> = (1..=6).map(trade).collect();
    s.ingest_backfill(&trades); // tick(2): 6 trades -> 3 bars
    assert_eq!(s.bars().len(), 3);

    s.set_spec(BarSpec::Tick(3)); // rebuild: 6 trades -> 2 bars
    assert_eq!(s.bars().len(), 2);
    assert_eq!(
        s.backfill_boundary(),
        Some(2),
        "all six are backfill -> boundary at the end"
    );
}

#[test]
fn timeline_revision_tracks_ingest_prepend_and_rebuilds() {
    let mut state = ChartState::new(BarSpec::Tick(2));
    assert_eq!(state.timeline_revision(), 0);

    state.ingest_backfill(&(5..=8).map(trade).collect::<Vec<_>>());
    assert_eq!(state.timeline_revision(), 1);
    state.ingest_live(&trade(9));
    assert_eq!(state.timeline_revision(), 2);
    state.prepend_history(&(1..=4).map(trade).collect::<Vec<_>>());
    assert_eq!(state.timeline_revision(), 3);

    state.set_spec(BarSpec::Tick(2));
    assert_eq!(state.timeline_revision(), 3, "an unchanged spec is a no-op");
    state.set_spec(BarSpec::Tick(3));
    assert_eq!(state.timeline_revision(), 4);
}

/// The series identity is the *narrow* question — "were the bars and
/// ladders a fold reads rebuilt?" — and it has to stay narrow. Live
/// ingest only appends, so neither a print nor a bar closing on the right
/// edge changes a bar that a range already folded; a consumer keeping that
/// fold must not be told otherwise, or a long range restarts on every tick
/// and never finishes.
#[test]
fn series_revision_ignores_prints_and_closes_and_tracks_rebuilds() {
    let mut state = ChartState::new(BarSpec::Tick(2));
    assert_eq!(state.series_revision(), 0);

    // Two trades: the first opens the forming bar, the second closes it.
    // Neither rewrites a bar that was already there.
    state.ingest_live(&trade(1));
    state.ingest_live(&trade(2));
    assert_eq!(state.bars().len(), 1, "a bar did close");
    assert_eq!(
        state.series_revision(),
        0,
        "appending never rewrites what was already folded"
    );
    assert_eq!(
        state.timeline_revision(),
        2,
        "the timeline moved on both prints all the same"
    );

    // Every way the series is genuinely rebuilt does move it.
    state.ingest_backfill(&(5..=8).map(trade).collect::<Vec<_>>());
    assert_eq!(state.series_revision(), 1, "the backfill rebuilt the bars");
    state.prepend_history(&(3..=4).map(trade).collect::<Vec<_>>());
    assert_eq!(state.series_revision(), 2, "so does a prepended page");
    state.set_spec(BarSpec::Tick(3));
    assert_eq!(state.series_revision(), 3, "and a new bar rule");
    state.set_spec(BarSpec::Tick(3));
    assert_eq!(state.series_revision(), 3, "an unchanged spec is a no-op");
    state.set_footprint_enabled(true);
    assert_eq!(
        state.series_revision(),
        4,
        "switching the ladders on rebuilds them, which is a fold's inputs"
    );
    state.set_footprint_group(dec("2"));
    assert_eq!(state.series_revision(), 5, "and so does a refold");
}

#[test]
fn boundary_is_recomputed_across_a_switch() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    s.ingest_backfill(&[trade(1), trade(2), trade(3)]); // 1 bar + partial
    s.ingest_live(&trade(4)); // closes bar 2 (backfill 3 + live 4)
    assert_eq!(s.bars().len(), 2);
    assert_eq!(s.backfill_boundary(), Some(1));

    // tick(4): 4 trades -> 1 bar. The first 3 (backfill) close 0 bars.
    s.set_spec(BarSpec::Tick(4));
    assert_eq!(s.bars().len(), 1);
    assert_eq!(s.backfill_boundary(), Some(0));
}

#[test]
fn prepend_history_adds_older_bars_and_keeps_boundary() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    s.ingest_backfill(&[trade(5), trade(6), trade(7), trade(8)]); // 2 bars
    s.ingest_live(&trade(9)); // opens a partial, still 2 closed bars
    assert_eq!(s.bars().len(), 2);
    assert_eq!(s.backfill_boundary(), Some(2));

    // Pull in the four older trades 1..=4.
    let added = s.prepend_history(&[trade(1), trade(2), trade(3), trade(4)]);
    // tick(2) over 1..=8 backfill = 4 closed bars; trade 9 is the partial.
    assert_eq!(s.bars().len(), 4);
    assert_eq!(added, 2, "two net bars were prepended");
    assert_eq!(
        s.backfill_boundary(),
        Some(4),
        "all eight retained backfill trades are history"
    );
}

#[test]
fn prepend_empty_history_is_a_noop() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    s.ingest_backfill(&[trade(1), trade(2)]);
    let before = s.bars().len();
    let added = s.prepend_history(&[]);
    assert_eq!(added, 0);
    assert_eq!(s.bars().len(), before);
}

/// Bar indices mean a different thing per spec; market time does not. This
/// is the lookup that carries the user's position across a rebuild.
#[test]
fn a_slot_is_found_by_the_market_time_it_shows() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    // trade(n) is stamped at 1000 + n*100, so tick(2) bars open at 1100,
    // 1300, 1500 and the partial (trade 7) at 1700.
    let trades: Vec<Trade> = (1..=7).map(trade).collect();
    s.ingest_backfill(&trades);
    assert_eq!(s.bars().len(), 3);
    assert!(s.partial().is_some());

    assert_eq!(s.slot_open_time(0), Some(1100));
    assert_eq!(s.slot_open_time(3), Some(1700), "the forming bar's slot");
    assert_eq!(s.slot_open_time(4), None, "no such slot");

    assert_eq!(s.slot_at_time(1300), Some(1), "exactly on an open");
    assert_eq!(s.slot_at_time(1400), Some(1), "inside a bar");
    assert_eq!(s.slot_at_time(9_999), Some(3), "past the end: the newest");
    assert_eq!(s.slot_at_time(0), Some(0), "before the start: the oldest");
}

#[test]
fn an_empty_series_has_no_slot_to_point_at() {
    let s = ChartState::new(BarSpec::Tick(2));
    assert_eq!(s.slot_at_time(1_000), None);
    assert_eq!(s.slot_open_time(0), None);
}

/// The rebuild is what makes the lookup necessary: the same market time
/// lands on a different index once the bars are re-cut.
#[test]
fn the_slot_of_a_time_moves_when_the_spec_changes() {
    let mut s = ChartState::new(BarSpec::Tick(1));
    let trades: Vec<Trade> = (1..=8).map(trade).collect();
    s.ingest_backfill(&trades);
    let slot = s.slot_at_time(1500).expect("a slot for trade 5");
    assert_eq!(slot, 4, "tick(1): one bar per trade");

    s.set_spec(BarSpec::Tick(4));
    assert_eq!(s.slot_at_time(1500), Some(1), "tick(4): the second bar");
}

#[test]
fn setting_the_same_spec_is_a_noop() {
    let mut s = ChartState::new(BarSpec::Tick(2));
    s.ingest_backfill(&[trade(1), trade(2)]);
    let before = s.bars().len();
    s.set_spec(BarSpec::Tick(2));
    assert_eq!(s.bars().len(), before);
}

/// The first live print after a backfill must not copy the backfill.
///
/// Filled exactly, a contiguous tape made the next push reallocate the
/// whole loaded session on the UI thread — about 95 MB for a recovered
/// 1.7 M-print day, one dropped frame the moment the chart went live. The
/// chunked tape never moves a print it holds, so that push copies
/// nothing, and it holds at most one chunk it does not use.
#[test]
fn the_first_live_print_after_a_backfill_copies_nothing() {
    let history: Vec<Trade> = (5_001..=15_000).map(trade).collect();
    let older: Vec<Trade> = (1..=5_000).map(trade).collect();
    let mut s = ChartState::new(BarSpec::Tick(50));
    let first_live = |s: &mut ChartState, id: u64| {
        let before = crate::work_meter::tally();
        s.ingest_live(&trade(id));
        let copied = crate::work_meter::tally().since(before).realloc_copy_bytes;
        let tape_bytes = (s.trades.len() * std::mem::size_of::<Trade>()) as u64;
        assert!(
            copied < tape_bytes / 2,
            "the first live print copied {copied} bytes of a {tape_bytes}-byte tape"
        );
        assert!(
            s.trades.capacity() - s.trades.len() < quantick_engine::trade_tape::CHUNK_TRADES,
            "no more than one chunk is held unused"
        );
    };
    s.ingest_backfill(&history);
    first_live(&mut s, 15_001);
    // Paging older history in joins a new tape: chunked the same way.
    s.prepend_history(&older);
    first_live(&mut s, 15_002);
}
