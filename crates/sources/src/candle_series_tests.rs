//! The series bookkeeping, against fixed pages: what is asked, when scrolling
//! asks again, and when the venue's record is known to start.

use super::*;
use rust_decimal::Decimal;

const MINUTE: i64 = 60_000;
const QUARTER: i64 = 15 * MINUTE;

fn candle(open_time: i64, interval_ms: i64) -> Bar {
    Bar {
        open_time,
        close_time: open_time + interval_ms - 1,
        open: Decimal::ONE,
        high: Decimal::ONE,
        low: Decimal::ONE,
        close: Decimal::ONE,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ZERO,
        trade_count: 1,
    }
}

/// `count` candles at `interval_ms`, the newest opening just before `end_ms`.
fn page(end_ms: i64, count: usize, interval_ms: i64) -> Vec<Bar> {
    let count = count as i64;
    (0..count)
        .map(|index| candle(end_ms - (count - index) * interval_ms, interval_ms))
        .collect()
}

const LAST: OhlcvSlice = OhlcvSlice::Last { complete: true };

/// A series that took a full opening page ending at `end_ms`.
fn opened(end_ms: i64) -> CandleSeries {
    let mut series = CandleSeries::default();
    series.sent(CandleSeries::opening_ask(QUARTER));
    series.take_reply(page(end_ms, CANDLE_PAGE_BARS, QUARTER), LAST);
    series
}

const END: i64 = 2_000_000 * QUARTER;

#[test]
fn the_opening_ask_is_one_page_of_the_pane_interval_from_the_live_edge() {
    assert_eq!(
        CandleSeries::opening_ask(QUARTER),
        CandleAsk {
            interval_ms: QUARTER,
            span_ms: 1_000 * QUARTER,
            slice_ms: None,
            before_ms: None,
        }
    );
    let fresh = CandleSeries::default();
    assert_eq!(
        fresh.next_ask(QUARTER, None),
        Some(CandleSeries::opening_ask(QUARTER))
    );
}

#[test]
fn only_one_request_is_out_at_a_time() {
    let mut series = CandleSeries::default();
    series.sent(CandleSeries::opening_ask(QUARTER));
    assert!(series.is_pending());
    assert_eq!(series.next_ask(QUARTER, Some(0)), None);
    assert_eq!(series.status(QUARTER).state, SeriesState::Loading);
}

#[test]
fn scrolling_within_the_margin_asks_for_the_page_behind_the_oldest_candle() {
    let series = opened(END);
    let oldest = series.oldest_ms().expect("a page landed");
    assert_eq!(series.next_ask(QUARTER, Some(REACH_BACK_MARGIN_BARS)), None);
    assert_eq!(series.next_ask(QUARTER, None), None, "no layout, no reach");
    assert_eq!(
        series.next_ask(QUARTER, Some(REACH_BACK_MARGIN_BARS - 1)),
        Some(CandleAsk {
            interval_ms: QUARTER,
            span_ms: 1_000 * QUARTER,
            slice_ms: None,
            before_ms: Some(oldest - 1),
        })
    );
}

#[test]
fn an_older_page_prepends_and_reports_what_it_added() {
    let mut series = opened(END);
    let oldest = series.oldest_ms().expect("held");
    let ask = series.next_ask(QUARTER, Some(0)).expect("an older ask");
    series.sent(ask);
    let settled = series.take_reply(page(oldest, CANDLE_PAGE_BARS, QUARTER), LAST);
    assert_eq!(
        settled,
        Some(Settled {
            added: CANDLE_PAGE_BARS,
            exhausted: false
        })
    );
    assert_eq!(series.bars().len(), 2 * CANDLE_PAGE_BARS);
    assert_eq!(
        series.oldest_ms(),
        Some(oldest - CANDLE_PAGE_BARS as i64 * QUARTER)
    );
    assert!(
        series
            .bars()
            .windows(2)
            .all(|w| w[0].open_time < w[1].open_time)
    );
    assert_eq!(series.status(QUARTER).state, SeriesState::Idle);
}

#[test]
fn an_empty_or_short_reply_marks_the_record_start() {
    let mut empty = opened(END);
    empty.sent(empty.next_ask(QUARTER, Some(0)).expect("ask"));
    let settled = empty.take_reply(Vec::new(), LAST).expect("closed");
    assert!(settled.exhausted);
    assert_eq!(empty.next_ask(QUARTER, Some(0)), None);
    assert_eq!(empty.status(QUARTER).state, SeriesState::Exhausted);

    // The record starts 300 candles into the window.
    let mut short = opened(END);
    let oldest = short.oldest_ms().expect("held");
    short.sent(short.next_ask(QUARTER, Some(0)).expect("ask"));
    let settled = short
        .take_reply(page(oldest, 700, QUARTER), LAST)
        .expect("closed");
    assert_eq!(settled.added, 700);
    assert!(settled.exhausted);
}

#[test]
fn a_short_page_with_a_hole_inside_it_is_not_the_record_start() {
    let mut series = opened(END);
    let oldest = series.oldest_ms().expect("held");
    series.sent(series.next_ask(QUARTER, Some(0)).expect("ask"));
    // A maintenance hole of eight candles in the middle of the window.
    let mut holed = page(oldest, CANDLE_PAGE_BARS, QUARTER);
    holed.drain(400..408);
    let settled = series.take_reply(holed, LAST).expect("closed");
    assert!(!settled.exhausted);
}

#[test]
fn a_young_market_answers_its_opening_short_and_is_exhausted_at_once() {
    let mut series = CandleSeries::default();
    series.sent(CandleSeries::opening_ask(DAY_MS));
    let settled = series
        .take_reply(page(END, 120, DAY_MS), LAST)
        .expect("closed");
    assert!(settled.exhausted);
    assert_eq!(series.next_ask(DAY_MS, Some(0)), None);
}

#[test]
fn a_reply_that_brings_nothing_older_ends_the_reach() {
    let mut series = opened(END);
    series.sent(series.next_ask(QUARTER, Some(0)).expect("ask"));
    // The venue re-sends what is held.
    let settled = series
        .take_reply(page(END, CANDLE_PAGE_BARS, QUARTER), LAST)
        .expect("closed");
    assert_eq!(settled.added, 0);
    assert!(settled.exhausted);
}

#[test]
fn a_refusal_closes_the_request_and_teaches_nothing() {
    let mut series = opened(END);
    series.sent(series.next_ask(QUARTER, Some(0)).expect("ask"));
    assert_eq!(series.take_reply(Vec::new(), OhlcvSlice::Refused), None);
    assert!(!series.is_pending());
    assert!(!series.is_exhausted());
    assert!(series.next_ask(QUARTER, Some(0)).is_some(), "asks again");
}

#[test]
fn an_abandoned_request_may_go_out_again() {
    let mut series = CandleSeries::default();
    series.sent(CandleSeries::opening_ask(QUARTER));
    series.abandon();
    assert_eq!(
        series.next_ask(QUARTER, None),
        Some(CandleSeries::opening_ask(QUARTER))
    );
}

#[test]
fn the_status_names_the_interval_the_count_and_the_oldest_candle() {
    let series = opened(END);
    assert_eq!(
        series.status(QUARTER),
        SeriesStatus {
            state: SeriesState::Idle,
            interval_ms: QUARTER,
            candles_held: CANDLE_PAGE_BARS,
            oldest_open_ms: Some(END - CANDLE_PAGE_BARS as i64 * QUARTER),
            last_reply_short: false,
        }
    );
}

const SHORT: OhlcvSlice = OhlcvSlice::Last { complete: false };

fn tick(series: &mut CandleSeries, polls: u32) {
    (0..polls).for_each(|_| series.tick());
}

#[test]
fn an_answer_cut_short_is_not_the_record_start_and_is_asked_again_after_a_backoff() {
    let mut series = opened(END);
    let oldest = series.oldest_ms().expect("held");
    let ask = series.next_ask(QUARTER, Some(0)).expect("ask");
    series.sent(ask);
    let settled = series.take_reply(Vec::new(), SHORT).expect("closed");
    assert!(
        !settled.exhausted,
        "a failed fetch says nothing of the record"
    );
    assert!(!series.is_exhausted());
    let status = series.status(QUARTER);
    assert_eq!(status.state, SeriesState::Idle);
    assert!(status.last_reply_short, "said, not hidden");
    assert_eq!(series.next_ask(QUARTER, Some(0)), None, "backing off");
    tick(&mut series, SHORT_REPLY_RETRY_POLLS);
    assert_eq!(series.next_ask(QUARTER, Some(0)), Some(ask), "asked again");

    // A second short answer waits twice as long.
    series.sent(ask);
    series.take_reply(Vec::new(), SHORT);
    tick(&mut series, SHORT_REPLY_RETRY_POLLS);
    assert_eq!(series.next_ask(QUARTER, Some(0)), None);
    tick(&mut series, SHORT_REPLY_RETRY_POLLS);
    assert_eq!(series.next_ask(QUARTER, Some(0)), Some(ask));

    // A complete page clears the flag.
    series.sent(ask);
    series.take_reply(page(oldest, CANDLE_PAGE_BARS, QUARTER), LAST);
    assert!(!series.status(QUARTER).last_reply_short);
}

#[test]
fn the_backoff_stops_growing() {
    let mut series = opened(END);
    let ask = series.next_ask(QUARTER, Some(0)).expect("ask");
    for _ in 0..20 {
        series.sent(ask);
        series.take_reply(Vec::new(), SHORT);
        tick(&mut series, SHORT_REPLY_RETRY_MAX_POLLS);
        assert_eq!(series.next_ask(QUARTER, Some(0)), Some(ask));
    }
}

#[test]
fn an_opening_cut_short_and_empty_asks_the_opening_again() {
    let mut series = CandleSeries::default();
    series.sent(CandleSeries::opening_ask(QUARTER));
    series.take_reply(Vec::new(), SHORT);
    assert!(!series.is_answered(), "nothing arrived");
    assert!(!series.is_exhausted());
    assert_eq!(series.next_ask(QUARTER, None), None, "backing off");
    tick(&mut series, SHORT_REPLY_RETRY_POLLS);
    assert_eq!(
        series.next_ask(QUARTER, None),
        Some(CandleSeries::opening_ask(QUARTER))
    );
}

#[test]
fn a_press_asks_the_older_page_through_the_backoff() {
    let mut series = opened(END);
    let ask = series.next_ask(QUARTER, Some(0)).expect("ask");
    series.sent(ask);
    series.take_reply(Vec::new(), SHORT);
    assert_eq!(series.next_ask(QUARTER, Some(0)), None);
    assert_eq!(series.older_ask(QUARTER), Some(ask));
}

#[test]
fn a_pane_reads_its_own_interval_or_the_coarsest_one_that_folds_into_it() {
    assert_eq!(native_series_for(QUARTER), Some(QUARTER));
    assert_eq!(native_series_for(DAY_MS), Some(DAY_MS));
    assert_eq!(native_series_for(WEEK_MS), Some(WEEK_MS));
    assert_eq!(
        native_series_for(CALENDAR_MONTH_MS),
        Some(CALENDAR_MONTH_MS)
    );
    assert_eq!(native_series_for(6 * HOUR_MS), Some(2 * HOUR_MS));
    assert_eq!(native_series_for(10 * MINUTE), Some(5 * MINUTE));
    assert_eq!(native_series_for(2 * DAY_MS), Some(DAY_MS));
    assert_eq!(native_series_for(3 * CALENDAR_MONTH_MS), Some(DAY_MS));
    assert_eq!(native_series_for(90_000), None);
    assert_eq!(native_series_for(500), None);
}
