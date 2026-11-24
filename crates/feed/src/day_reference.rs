//! The daily open a crypto venue supplies, kept current: read off the venue's
//! own daily candle when a session starts and again after each 00:00 UTC, and
//! sent as a [`FeedEvent::DayReference`]. The venue's candle is the
//! reference; a day whose candle the venue does not serve sends nothing.

use std::future::Future;
use std::time::Duration;

use quantick_engine::Bar;
use quantick_sources::day_reference::{DayReference, UTC_DAY_MS, utc_day_start};
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::FeedEvent;

/// How long after 00:00 UTC the next day's candle is asked for: the venue
/// opens it on its first print, not on the stroke of midnight.
pub const DAY_ROLL_GRACE_MS: i64 = 5_000;
/// How long a failed or empty read waits before it asks again.
pub const DAY_OPEN_RETRY: Duration = Duration::from_secs(60);

/// The open of the daily candle that starts at `day_start_ms`, among the
/// venue's daily `bars`; `None` when the venue served no candle for that day.
#[must_use]
pub fn open_of_day(bars: &[Bar], day_start_ms: i64) -> Option<Decimal> {
    todo!("{} {day_start_ms}", bars.len())
}

/// When the next day's open is asked for, at market time `now_ms`.
#[must_use]
pub fn next_read_ms(now_ms: i64) -> i64 {
    todo!("{now_ms} {UTC_DAY_MS}")
}

/// Keep the session's daily open current on a task of its own, reading the
/// day's candles through `fetch(day_start_ms, now_ms)`. Ends when the
/// session's consumer is gone.
pub fn spawn_daily_open<F, Fut>(tx: mpsc::Sender<FeedEvent>, fetch: F) -> JoinHandle<()>
where
    F: Fn(i64, i64) -> Fut + Send + 'static,
    Fut: Future<Output = Vec<Bar>> + Send,
{
    tokio::spawn(async move {
        loop {
            let now_ms = crate::clock::wall_clock_ms();
            let day_start = utc_day_start(now_ms);
            let wait = match open_of_day(&fetch(day_start, now_ms).await, day_start) {
                Some(open) => {
                    let reference = DayReference::daily_open(day_start, open);
                    if tx.send(FeedEvent::DayReference(reference)).await.is_err() {
                        return;
                    }
                    let until = next_read_ms(crate::clock::wall_clock_ms());
                    Duration::from_millis(until.saturating_sub(now_ms).max(0) as u64)
                }
                None if tx.is_closed() => return,
                None => DAY_OPEN_RETRY,
            };
            tokio::time::sleep(wait).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 20_400 * UTC_DAY_MS;

    fn daily(open_time: i64, open: &str) -> Bar {
        let price: Decimal = open.parse().expect("a decimal");
        Bar {
            open_time,
            close_time: open_time + UTC_DAY_MS - 1,
            open: price,
            high: price,
            low: price,
            close: price,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn the_open_is_the_candle_that_starts_the_day() {
        let bars = [daily(DAY - UTC_DAY_MS, "75100.5"), daily(DAY, "76490.01")];
        assert_eq!(open_of_day(&bars, DAY), "76490.01".parse().ok());
        assert_eq!(
            open_of_day(&bars[..1], DAY),
            None,
            "yesterday's is no reference"
        );
        assert_eq!(open_of_day(&[], DAY), None);
    }

    #[test]
    fn the_next_open_is_read_just_after_the_next_utc_midnight() {
        assert_eq!(next_read_ms(DAY + 10), DAY + UTC_DAY_MS + DAY_ROLL_GRACE_MS);
        assert_eq!(
            next_read_ms(DAY + UTC_DAY_MS - 1),
            DAY + UTC_DAY_MS + DAY_ROLL_GRACE_MS
        );
    }
}
