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
    bars.iter()
        .find(|bar| bar.open_time == day_start_ms)
        .map(|bar| bar.open)
}

/// When the next day's open is asked for, at market time `now_ms`.
#[must_use]
pub fn next_read_ms(now_ms: i64) -> i64 {
    utc_day_start(now_ms) + UTC_DAY_MS + DAY_ROLL_GRACE_MS
}

/// What one read of the day's candles decides: the open to send, if the
/// read found the day it asked for, and how long to wait before the next.
///
/// The read asked at `asked_ms` for the day starting `day_start_ms` and came
/// back at `done_ms`. A read that straddled 00:00 UTC found yesterday's day:
/// it sends nothing and reads the new day at once, rather than sending a
/// stale open and sleeping a day on it.
#[must_use]
pub fn after_read(bars: &[Bar], asked_ms: i64, done_ms: i64) -> (Option<DayReference>, Duration) {
    let day_start = utc_day_start(asked_ms);
    if utc_day_start(done_ms) != day_start {
        return (None, Duration::ZERO);
    }
    match open_of_day(bars, day_start) {
        Some(open) => {
            let until = next_read_ms(asked_ms);
            let wait = until.saturating_sub(done_ms).max(0);
            let wait = Duration::from_millis(u64::try_from(wait).unwrap_or_default());
            (Some(DayReference::daily_open(day_start, open)), wait)
        }
        None => (None, DAY_OPEN_RETRY),
    }
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
            let bars = fetch(utc_day_start(now_ms), now_ms).await;
            let (open, wait) = after_read(&bars, now_ms, crate::clock::wall_clock_ms());
            if let Some(open) = open {
                if tx.send(FeedEvent::DayReference(Some(open))).await.is_err() {
                    return;
                }
            } else if tx.is_closed() {
                return;
            }
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
    fn a_read_sends_the_days_open_and_waits_for_the_next_midnight() {
        let bars = [daily(DAY, "76490.01")];
        let (open, wait) = after_read(&bars, DAY + 1_000, DAY + 3_000);
        assert_eq!(
            open,
            Some(DayReference::daily_open(DAY, "76490.01".parse().unwrap()))
        );
        let until = DAY + UTC_DAY_MS + DAY_ROLL_GRACE_MS;
        assert_eq!(wait, Duration::from_millis((until - DAY - 3_000) as u64));
        assert_eq!(
            after_read(&[], DAY + 1_000, DAY + 3_000),
            (None, DAY_OPEN_RETRY)
        );
    }

    /// Asked at 23:59:59.900 UTC, answered at 00:00:00.400: the answer is
    /// yesterday's open, and a day's sleep on it would leave the new day
    /// without one.
    #[test]
    fn a_read_straddling_utc_midnight_sends_nothing_and_reads_again_at_once() {
        let bars = [daily(DAY - UTC_DAY_MS, "75100.5"), daily(DAY, "76490.01")];
        assert_eq!(
            after_read(&bars, DAY - 100, DAY + 400),
            (None, Duration::ZERO)
        );
        let (open, wait) = after_read(&bars, DAY + 401, DAY + 900);
        assert_eq!(open.map(|day| day.from_ms), Some(DAY), "the new day's open");
        assert!(wait > Duration::from_secs(86_000));
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
