use super::*;
use crate::state::{BarSpec, ChartState};
use quantick_engine::{ImbalanceUnit, Side, Trade};
use std::str::FromStr as _;

fn dec(text: &str) -> Decimal {
    Decimal::from_str(text).unwrap()
}

fn trade(agg_id: u64, timestamp_ms: i64, price: &str, quantity: &str, side: Side) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: dec(price),
        quantity: dec(quantity),
        side,
    }
}

fn chart_after(spec: BarSpec, trades: &[Trade]) -> ChartState {
    let mut chart = ChartState::new(spec);
    for trade in trades {
        chart.ingest_live(trade);
    }
    chart
}

#[test]
fn count_rules_print_done_over_target() {
    assert_eq!(count_text(dec("522"), dec("2000")), "522/2000");
    assert_eq!(count_text(dec("312"), dec("500")), "312/500");
    assert_eq!(
        count_text(dec("0.125"), dec("5")),
        "0.12/5",
        "cut, never rounded up"
    );
    assert_eq!(count_text(dec("99999"), dec("99999")), "99999/99999");
}

#[test]
fn large_targets_print_in_the_targets_unit() {
    assert_eq!(count_text(dec("1250000"), dec("2000000")), "1.2M/2.0M");
    assert_eq!(
        count_text(dec("1999999"), dec("2000000")),
        "1.9M/2.0M",
        "1.99M has not reached 2.0M"
    );
    assert_eq!(count_text(dec("31200"), dec("500000")), "31.2K/500.0K");
    assert_eq!(count_text(dec("700000000"), dec("1500000000")), "0.7B/1.5B");
}

#[test]
fn a_time_bar_counts_down_like_tradingview() {
    assert_eq!(countdown_text(52_000), "00:52");
    assert_eq!(countdown_text(52_999), "00:52");
    assert_eq!(countdown_text(60_000), "01:00");
    assert_eq!(countdown_text(3_599_999), "59:59");
    assert_eq!(countdown_text(3_852_000), "1:04:12");
    assert_eq!(countdown_text(-5), "00:00");
}

/// A countdown asks for the frame its text changes on: `00:52` turns
/// `00:51` once 52 300 ms has run down past 52 000.
#[test]
fn a_countdown_repaints_on_its_next_whole_second_and_a_count_never() {
    let left = |remaining_ms| BarProgressReading::Remaining {
        length_ms: 60_000,
        remaining_ms,
    };
    assert_eq!(left(52_300).repaint_after_ms(), Some(301));
    assert_eq!(left(52_000).repaint_after_ms(), Some(1));
    assert_eq!(countdown_text(52_000 - 1), "00:51");
    assert_eq!(
        left(0).repaint_after_ms(),
        None,
        "a closed bucket waits for a print"
    );
    let count = BarProgressReading::Count {
        done: dec("5"),
        target: dec("10"),
        unit: "ticks",
    };
    assert_eq!(count.repaint_after_ms(), None);
}

#[test]
fn an_imbalance_bar_prints_its_signed_running_total() {
    assert_eq!(imbalance_text(dec("340")), "+340");
    assert_eq!(imbalance_text(dec("-120")), "-120");
    assert_eq!(imbalance_text(Decimal::ZERO), "0");
    assert_eq!(imbalance_text(dec("-2500000")), "-2.5M");
}

#[test]
fn a_tick_chart_reads_its_count() {
    let trades: Vec<Trade> = (0..3)
        .map(|i| trade(i, 1_000 + i as i64, "100", "1", Side::Buy))
        .collect();
    let reading = chart_after(BarSpec::Tick(2000), &trades)
        .bar_progress_at(Some(99_999))
        .expect("a tick bar counts");
    assert_eq!(reading.tag_text(), "3/2000");
    assert_eq!(
        (reading.done(), reading.target(), reading.unit()),
        (dec("3"), Some(dec("2000")), "ticks")
    );
}

#[test]
fn a_volume_and_a_dollar_chart_read_in_their_measure() {
    let trades = [trade(1, 1_000, "100", "2.5", Side::Sell)];
    let volume = chart_after(BarSpec::Volume(dec("500")), &trades)
        .bar_progress_at(None)
        .unwrap();
    assert_eq!(volume.tag_text(), "2.5/500");
    assert_eq!(volume.unit(), "vol");
    let dollar = chart_after(BarSpec::Dollar(dec("2000000")), &trades)
        .bar_progress_at(None)
        .unwrap();
    assert_eq!(dollar.tag_text(), "0.0M/2.0M");
    assert_eq!(dollar.done(), dec("250.0"));
}

#[test]
fn a_time_chart_counts_down_from_its_own_bucket_on_the_market_clock() {
    let chart = chart_after(
        BarSpec::Time(60_000),
        &[trade(1, 125_000, "100", "1", Side::Buy)],
    );
    let at = |clock| chart.bar_progress_at(clock).unwrap();
    assert_eq!(
        at(Some(128_000)).tag_text(),
        "00:52",
        "bucket 120000..180000"
    );
    assert_eq!(at(Some(179_400)).tag_text(), "00:00");
    assert_eq!(
        at(Some(200_000)).tag_text(),
        "00:00",
        "never negative past the end"
    );
    assert_eq!(at(Some(128_000)).done(), dec("8000"));
    assert_eq!(at(Some(128_000)).target(), Some(dec("60000")));
    assert_eq!(at(Some(128_000)).unit(), TIME_UNIT);
    // No clock: the trades' own time, which a quiet stretch freezes.
    assert_eq!(at(None).tag_text(), "00:55");
}

#[test]
fn an_hour_bar_reads_hours() {
    let chart = chart_after(
        BarSpec::Time(4 * 3_600_000),
        &[trade(1, 1_000, "100", "1", Side::Buy)],
    );
    let reading = chart.bar_progress_at(Some(10_549_000)).unwrap();
    assert_eq!(reading.tag_text(), "1:04:11");
}

#[test]
fn an_imbalance_chart_reads_its_running_total_without_a_target() {
    let trades = [
        trade(1, 1_000, "100", "1", Side::Buy),
        trade(2, 1_001, "100", "1", Side::Buy),
        trade(3, 1_002, "100", "1", Side::Sell),
    ];
    let reading = chart_after(BarSpec::Imbalance(ImbalanceUnit::Trades, 50), &trades)
        .bar_progress_at(Some(5_000))
        .unwrap();
    assert_eq!(reading.tag_text(), "+1");
    assert_eq!(reading.target(), None);
    assert_eq!(reading.unit(), "trades");
    assert_eq!(
        reading.fixed_progress(),
        None,
        "no share of a bar without a target"
    );
}

#[test]
fn a_fixed_progress_keeps_the_trades_time_for_a_time_rule() {
    let chart = chart_after(
        BarSpec::Time(60_000),
        &[trade(1, 125_000, "100", "1", Side::Buy)],
    );
    let progress = chart
        .bar_progress_at(None)
        .unwrap()
        .fixed_progress()
        .unwrap();
    assert_eq!(
        (progress.done, progress.target),
        (dec("5000"), dec("60000"))
    );
}
