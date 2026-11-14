//! The composed slot space's lookups, asked of the series itself: which bar
//! covers an instant, where the clamping lookup parks one, which candle the
//! pointer names, and where an empty calendar slot opens.

use super::*;
use crate::state::BarSpec;
use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS};
use quantick_engine::{Bar, Side, Trade};
use rust_decimal::Decimal;

/// The fixture tape's first print.
const TAPE_START_MS: i64 = 1_700_000_000_000;

/// One print of that tape: a second apart, flat at a hundred.
fn print_at(index: u64) -> Trade {
    Trade {
        agg_id: index,
        timestamp_ms: TAPE_START_MS + index as i64 * 1_000,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// `count` of those prints, `per_bar` of them to a bar.
fn tape(count: u64, per_bar: u64) -> ChartState {
    let mut state = ChartState::new(BarSpec::Tick(per_bar));
    let trades: Vec<_> = (0..count).map(print_at).collect();
    state.ingest_backfill(&trades);
    state
}

/// A flat one-unit bar from `open_time` to `close_time`.
fn bar(open_time: i64, close_time: i64) -> Bar {
    Bar {
        open_time,
        close_time,
        open: Decimal::ONE,
        high: Decimal::ONE,
        low: Decimal::ONE,
        close: Decimal::ONE,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ZERO,
        trade_count: 1,
    }
}

fn read<'a>(
    prefix: &'a [Bar],
    state: &'a ChartState,
    spec: &'a SpecSelector,
) -> PaneSeriesRead<'a> {
    PaneSeriesRead {
        history_prefix: prefix,
        state,
        spec,
    }
}

/// The lookup the trade marks are painted through answers only inside the
/// tape: the first print to the last, edges included. Ten prints, four to a
/// bar: two closed bars and a forming one holding the last two.
#[test]
fn a_covering_slot_answers_only_inside_the_tape() {
    let state = tape(10, 4);
    let spec = SpecSelector::new(BarSpec::Tick(4));
    let series = read(&[], &state, &spec);
    let newest = TAPE_START_MS + 9_000;
    for (ms, slot, what) in [
        (
            TAPE_START_MS - 1,
            None,
            "a millisecond before the first print",
        ),
        (TAPE_START_MS, Some(0), "the first print itself"),
        (
            TAPE_START_MS + 2_500,
            Some(0),
            "between two prints of a bar",
        ),
        (
            TAPE_START_MS + 4_000,
            Some(1),
            "the second bar's first print",
        ),
        (newest, Some(2), "the forming bar's last print"),
        (newest + 1, None, "a millisecond past it"),
        (newest + 3_600_000, None, "an hour past it"),
    ] {
        assert_eq!(series.covering_slot_at_time(ms), slot, "{what}");
    }

    // The other lookup clamps, on purpose: a drawing anchor being re-hung
    // wants the closest bar. Pinned beside it so the two cannot converge.
    assert_eq!(series.slot_at_time(newest + 3_600_000), Some(2));
    assert_eq!(series.slot_at_time(TAPE_START_MS - 3_600_000), Some(0));

    let empty = tape(0, 4);
    assert_eq!(
        read(&[], &empty, &spec).covering_slot_at_time(TAPE_START_MS),
        None,
        "an empty series covers no instant at all"
    );
}

/// Venue candles alone: the window is the prefix's own, so an instant inside
/// it still finds its candle, and one past the newest candle does not.
#[test]
fn a_venue_prefix_covers_its_own_instants() {
    let state = tape(0, 4);
    let spec = SpecSelector::new(BarSpec::Tick(4));
    let prefix: Vec<_> = (0..2)
        .map(|index: i64| {
            bar(
                TAPE_START_MS - 120_000 + index * 60_000,
                TAPE_START_MS - 61_000 + index * 60_000,
            )
        })
        .collect();
    let series = read(&prefix, &state, &spec);
    assert_eq!(series.covering_slot_at_time(TAPE_START_MS - 121_000), None);
    assert_eq!(
        series.covering_slot_at_time(TAPE_START_MS - 90_000),
        Some(0)
    );
    assert_eq!(
        series.covering_slot_at_time(TAPE_START_MS - 30_000),
        Some(1)
    );
    assert_eq!(
        series.covering_slot_at_time(TAPE_START_MS),
        None,
        "past the newest candle, where the tape has not started yet"
    );
}

/// The replay seek's shape: a fill the rebuilt tape has not reached gets no
/// bar until the tape does, instead of parking on the edge one.
#[test]
fn a_fill_ahead_of_the_tape_waits_for_it() {
    let fill = TAPE_START_MS + 6_000;
    let spec = SpecSelector::new(BarSpec::Tick(4));
    let mut state = tape(4, 4);
    assert_eq!(read(&[], &state, &spec).covering_slot_at_time(fill), None);
    assert_eq!(
        read(&[], &state, &spec).slot_at_time(fill),
        Some(0),
        "which is where the clamping lookup would have parked it"
    );
    for index in 4..8 {
        state.ingest_live(&print_at(index));
    }
    assert_eq!(
        read(&[], &state, &spec).covering_slot_at_time(fill),
        Some(1)
    );
}

/// When the fixture's first venue candle opened.
const BAR_ZERO_MS: i64 = 1_700_000_000_000;

/// `count` venue candles, each opening one minute after the last.
fn minute_candles(count: usize) -> Vec<Bar> {
    (0..count)
        .map(|index| {
            let open = BAR_ZERO_MS + index as i64 * 60_000;
            bar(open, open + 59_999)
        })
        .collect()
}

/// Point at a candle, learn *its* time — on every pixel of it, the left half
/// included, which is where this used to answer with the bar before it.
#[test]
fn the_pointer_names_the_candle_under_it() {
    let prefix = minute_candles(200);
    let state = tape(0, 50);
    let spec = SpecSelector::new(BarSpec::Tick(50));
    let series = read(&prefix, &state, &spec);
    let viewport = Viewport::new();
    let (right, total) = (1_000.0, series.slots());
    let width = viewport.candle_width();
    for slot in [120_usize, 199] {
        let centre = viewport.x_center(slot, right, total);
        for x in [
            centre - width / 2.0 + 0.01,
            centre,
            centre + width / 2.0 - 0.01,
        ] {
            assert_eq!(
                series.pointer_bar(&viewport, x, right, total),
                Some(PointerBar {
                    slot,
                    open_time_unix_ms: BAR_ZERO_MS + slot as i64 * 60_000,
                    interval_ms: None,
                }),
                "every pixel of candle {slot} reads candle {slot}'s own instant"
            );
        }
    }
}

/// Data honesty: the canvas past the newest bar, before the oldest, and the
/// tape's own band hold no bar, and neither does a chart with none.
#[test]
fn the_pointer_names_no_bar_where_there_is_none() {
    let prefix = minute_candles(200);
    let state = tape(0, 50);
    let spec = SpecSelector::new(BarSpec::Tick(50));
    let series = read(&prefix, &state, &spec);
    let viewport = Viewport::new();
    let (right, total) = (1_000.0, series.slots());
    assert_eq!(
        series.pointer_bar(&viewport, right + 60.0, right, total),
        None
    );
    let oldest = viewport.x_center(0, right, total);
    assert_eq!(
        series.pointer_bar(&viewport, oldest - 60.0, right, total),
        None
    );
    assert_eq!(
        series.pointer_bar(&viewport, right - 40.0, right - 150.0, total),
        None,
        "past the history's right edge is the lane, not a bar"
    );
    assert_eq!(
        read(&[], &state, &spec).pointer_bar(&viewport, 500.0, right, 0),
        None
    );
}

const JAN_2024_MS: i64 = 19_723 * DAY_MS;
const FEB_2024_MS: i64 = 19_754 * DAY_MS;
const MAR_2024_MS: i64 = 19_783 * DAY_MS;
const APR_2024_MS: i64 = 19_814 * DAY_MS;

/// A monthly chart ending on February 2024: the first empty slot is 1 March
/// and the second 1 April, and an instant in March maps back into slot 2.
#[test]
fn a_monthly_future_slot_opens_on_the_next_calendar_month() {
    let state = ChartState::new(BarSpec::Time(CALENDAR_MONTH_MS));
    let spec = SpecSelector::new(BarSpec::Time(CALENDAR_MONTH_MS));
    // The February bar opened on its first trade, the 5th.
    let february = FEB_2024_MS + 4 * DAY_MS;
    let prefix = [
        bar(JAN_2024_MS, JAN_2024_MS + DAY_MS - 1),
        bar(february, february + DAY_MS - 1),
    ];
    let series = read(&prefix, &state, &spec);
    assert_eq!(series.slots(), 2);
    assert_eq!(series.anchor_time(2.5), Some(MAR_2024_MS));
    assert_eq!(series.anchor_time(3.5), Some(APR_2024_MS));
    assert_eq!(series.future_slot_at_time(MAR_2024_MS), Some(2.0));
    assert_eq!(series.future_slot_at_time(APR_2024_MS), Some(3.0));
    let mid_march = series.future_slot_at_time(MAR_2024_MS + 15 * DAY_MS + DAY_MS / 2);
    assert_eq!(mid_march, Some(2.5), "half of March's 31 days");
    assert_eq!(series.future_slot_at_time(FEB_2024_MS + 20 * DAY_MS), None);
    assert_eq!(series.slot_of_time(MAR_2024_MS), Some(2.5));
}

/// A weekly chart steps to the next Monday, whatever day its last bar opened.
#[test]
fn a_weekly_future_slot_opens_on_the_next_monday() {
    let monday = JAN_2024_MS; // 1 January 2024 was a Monday.
    let state = ChartState::new(BarSpec::Time(WEEK_MS));
    let spec = SpecSelector::new(BarSpec::Time(WEEK_MS));
    let opened = monday + 2 * DAY_MS;
    let prefix = [bar(opened, opened + DAY_MS - 1)];
    let series = read(&prefix, &state, &spec);
    assert_eq!(series.anchor_time(1.5), Some(monday + WEEK_MS));
    assert_eq!(series.future_slot_at_time(monday + WEEK_MS), Some(1.0));
}
