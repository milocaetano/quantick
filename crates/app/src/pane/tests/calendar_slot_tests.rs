//! Empty future slots on a calendar chart are timed by the engine's bucket law.

use quantick_engine::Bar;
use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS};
use rust_decimal::Decimal;

use crate::pane::ChartPane;

const JAN_2024_MS: i64 = 19_723 * DAY_MS;
const FEB_2024_MS: i64 = 19_754 * DAY_MS;
const MAR_2024_MS: i64 = 19_783 * DAY_MS;
const APR_2024_MS: i64 = 19_814 * DAY_MS;

fn bar(open_time: i64) -> Bar {
    Bar {
        open_time,
        close_time: open_time + DAY_MS - 1,
        open: Decimal::ONE,
        high: Decimal::ONE,
        low: Decimal::ONE,
        close: Decimal::ONE,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 1,
    }
}

/// A monthly chart ending on February 2024: the first empty slot is 1 March
/// and the second 1 April, and an instant in March maps back into slot 2.
#[test]
fn a_monthly_future_slot_opens_on_the_next_calendar_month() {
    let mut pane = ChartPane::time(1, CALENDAR_MONTH_MS);
    // The February bar opened on its first trade, the 5th.
    pane.install_history_prefix(vec![bar(JAN_2024_MS), bar(FEB_2024_MS + 4 * DAY_MS)]);
    let series = pane.series_read();
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
    let mut pane = ChartPane::time(1, WEEK_MS);
    pane.install_history_prefix(vec![bar(monday + 2 * DAY_MS)]);
    let series = pane.series_read();
    assert_eq!(series.anchor_time(1.5), Some(monday + WEEK_MS));
    assert_eq!(series.future_slot_at_time(monday + WEEK_MS), Some(1.0));
}
