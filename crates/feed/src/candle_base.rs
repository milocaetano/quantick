//! Which venue-candle base a chart folds from, and which one it is after.
//!
//! A chart holds one base of venue candles per market — minutes, or days for
//! panes cut at a day or longer — and folds every pane's prefix from it. The
//! decisions about that base are plain values and live here, below the
//! application: which base the panes want, whether a reply's candles may join
//! the base held, and what one request asks the provider for. The tab keeps
//! one [`CandleBaseInterval`] and carries out what it says.

use crate::{
    OHLCV_BASE_INTERVAL_MS, OHLCV_DAILY_INTERVAL_MS, OHLCV_SLICE_SPAN_MS, candles::is_foldable,
    ohlcv_base_interval_for, ohlcv_span_for,
};

/// The interval of the candles held, and of the candles wanted.
///
/// The two differ when a provider without daily candles answers a daily
/// request in minutes: the minutes are held and folded, the days stay
/// wanted, and only a change of what is *wanted* is a reason to fetch again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandleBaseInterval {
    held_ms: i64,
    wanted_ms: i64,
}

impl Default for CandleBaseInterval {
    fn default() -> Self {
        Self {
            held_ms: OHLCV_BASE_INTERVAL_MS,
            wanted_ms: OHLCV_BASE_INTERVAL_MS,
        }
    }
}

/// What one candle request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandleAsk {
    /// The base interval asked for.
    pub interval_ms: i64,
    /// How far back one request reaches at that interval.
    pub span_ms: i64,
    /// How much of the span one reply should cover, if sliced at all.
    pub slice_ms: Option<i64>,
}

impl CandleBaseInterval {
    /// The base the panes want: days when every pane that wants venue
    /// candles is cut at a day or longer, minutes as soon as one is not.
    ///
    /// `panes` yields each pane's time interval, `None` for a pane cut by
    /// trades, which wants the venue's minutes only under a `lead_in`. A pane
    /// whose interval no minute folds to (sub-minute, 90s) wants nothing. One
    /// base per chart, so a split showing 5m beside 1d reads both from
    /// minutes, over the minute reach.
    #[must_use]
    pub fn wanted_for(panes: impl IntoIterator<Item = Option<i64>>, lead_in: bool) -> i64 {
        panes
            .into_iter()
            .filter_map(|interval| match interval {
                Some(ms) if is_foldable(OHLCV_BASE_INTERVAL_MS, ms) => {
                    Some(ohlcv_base_interval_for(ms))
                }
                Some(_) => None,
                None => lead_in.then_some(OHLCV_BASE_INTERVAL_MS),
            })
            .min()
            .unwrap_or(OHLCV_BASE_INTERVAL_MS)
    }

    /// The interval the candles held were served at.
    #[must_use]
    pub fn held_ms(self) -> i64 {
        self.held_ms
    }

    /// The interval the chart is after.
    #[must_use]
    pub fn wanted_ms(self) -> i64 {
        self.wanted_ms
    }

    /// Record what the panes want now; the previous wish when it changed.
    pub fn want(&mut self, wanted_ms: i64) -> Option<i64> {
        let was = self.wanted_ms;
        self.wanted_ms = wanted_ms;
        (was != wanted_ms).then_some(was)
    }

    /// Whether a reply's candles at `interval_ms` may join the base, and the
    /// interval the base is held at when they may not.
    ///
    /// The first candles held set the base's interval — as served, which for
    /// a provider without daily candles is minutes under a daily request.
    /// After that every slice has to match: one base, one interval, or a fold
    /// would read some bars at the wrong size. A reply with no bars changes
    /// nothing and is always admitted.
    ///
    /// # Errors
    ///
    /// The interval the base expects, when these candles are at another.
    pub fn admit(
        &mut self,
        interval_ms: i64,
        base_is_empty: bool,
        has_bars: bool,
    ) -> Result<(), i64> {
        if !has_bars {
            return Ok(());
        }
        let expected = if base_is_empty {
            interval_ms
        } else {
            self.held_ms
        };
        if interval_ms <= 0 || interval_ms != expected {
            return Err(expected);
        }
        self.held_ms = interval_ms;
        Ok(())
    }

    /// The request for the base wanted. Progressive slicing paints a week of
    /// minutes in steps; five years of days is two venue pages, which slicing
    /// would only cut into more replies.
    #[must_use]
    pub fn ask(self, progressive: bool) -> CandleAsk {
        CandleAsk {
            interval_ms: self.wanted_ms,
            span_ms: ohlcv_span_for(self.wanted_ms),
            slice_ms: (progressive && self.wanted_ms < OHLCV_DAILY_INTERVAL_MS)
                .then_some(OHLCV_SLICE_SPAN_MS),
        }
    }
}

#[cfg(test)]
mod candle_base_tests {
    use super::*;
    use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS};

    #[test]
    fn days_are_wanted_only_when_every_pane_is_cut_at_a_day_or_longer() {
        let minute = OHLCV_BASE_INTERVAL_MS;
        let day = OHLCV_DAILY_INTERVAL_MS;
        let wanted = |panes: &[Option<i64>], lead_in: bool| {
            CandleBaseInterval::wanted_for(panes.iter().copied(), lead_in)
        };
        assert_eq!(wanted(&[Some(DAY_MS)], false), day);
        assert_eq!(
            wanted(&[Some(WEEK_MS), Some(CALENDAR_MONTH_MS)], false),
            day
        );
        assert_eq!(wanted(&[Some(DAY_MS), Some(5 * minute)], false), minute);
        assert_eq!(
            wanted(&[Some(36 * 3_600_000)], false),
            minute,
            "36h is no whole day"
        );
        assert_eq!(
            wanted(&[None, Some(DAY_MS)], false),
            day,
            "a tick pane wants nothing"
        );
        assert_eq!(
            wanted(&[None, Some(DAY_MS)], true),
            minute,
            "unless it leads in"
        );
        assert_eq!(
            wanted(&[Some(500), Some(DAY_MS)], false),
            day,
            "nor a sub-minute pane"
        );
        assert_eq!(wanted(&[], false), minute);
    }

    #[test]
    fn the_first_candles_set_the_base_and_later_ones_must_match() {
        let mut base = CandleBaseInterval::default();
        assert_eq!(
            base.want(OHLCV_DAILY_INTERVAL_MS),
            Some(OHLCV_BASE_INTERVAL_MS)
        );
        assert_eq!(
            base.want(OHLCV_DAILY_INTERVAL_MS),
            None,
            "no change, no news"
        );
        // A provider without days answers in minutes: held, and days still wanted.
        assert_eq!(base.admit(OHLCV_BASE_INTERVAL_MS, true, true), Ok(()));
        assert_eq!(base.held_ms(), OHLCV_BASE_INTERVAL_MS);
        assert_eq!(base.wanted_ms(), OHLCV_DAILY_INTERVAL_MS);
        assert_eq!(
            base.admit(OHLCV_DAILY_INTERVAL_MS, false, true),
            Err(OHLCV_BASE_INTERVAL_MS),
            "a day slice cannot join a base of minutes"
        );
        assert_eq!(base.admit(OHLCV_DAILY_INTERVAL_MS, false, false), Ok(()));
        assert_eq!(base.admit(0, true, true), Err(0));
    }

    #[test]
    fn a_daily_request_reaches_years_in_one_reply() {
        let mut base = CandleBaseInterval::default();
        assert_eq!(
            base.ask(true),
            CandleAsk {
                interval_ms: OHLCV_BASE_INTERVAL_MS,
                span_ms: crate::TIME_HISTORY_SPAN_MS,
                slice_ms: Some(OHLCV_SLICE_SPAN_MS),
            }
        );
        base.want(OHLCV_DAILY_INTERVAL_MS);
        assert_eq!(
            base.ask(true),
            CandleAsk {
                interval_ms: OHLCV_DAILY_INTERVAL_MS,
                span_ms: crate::DAILY_HISTORY_SPAN_MS,
                slice_ms: None,
            }
        );
    }
}
