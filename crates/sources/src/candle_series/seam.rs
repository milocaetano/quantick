//! The minutes a seam lead reads on the per-interval path.
//!
//! A pane's own series stops before the bucket of its first trade, so the
//! stretch of that bucket before the first trade — the venue's record of
//! what traded there before the chart was watching — comes from minutes
//! asked for just that stretch, unsliced: a day of them at most.

use quantick_engine::Bar;
use quantick_engine::time_bucket::{DAY_MS, time_bucket_start};

use crate::candle_base::CandleAsk;
use crate::candles::{Minutes, MinutesAnswer, SeamLead, seam_lead};
use crate::ohlcv::{OHLCV_BASE_INTERVAL_MS, OhlcvSlice};

/// The stretch every time pane's seam lead needs, `[from, to)`: from the
/// earliest seam bucket's start to the latest first trade. `seams` yields
/// each pane's first trade and interval; panes cut longer than a day read no
/// minutes (their lead would be weeks of them) and stay labelled partial.
#[must_use]
pub fn seam_stretch(seams: impl IntoIterator<Item = (i64, i64)>) -> Option<(i64, i64)> {
    seams
        .into_iter()
        .filter(|&(_, interval_ms)| reads_seam_minutes(interval_ms))
        .map(|(first_trade_ms, interval_ms)| {
            (
                time_bucket_start(first_trade_ms, interval_ms),
                first_trade_ms,
            )
        })
        .filter(|&(from, to)| from < to)
        .reduce(|(from, to), (other_from, other_to)| (from.min(other_from), to.max(other_to)))
}

/// Whether a pane cut at `interval_ms` reads its seam lead from minutes: a
/// day or shorter. Longer, the lead would be weeks of minutes, so the seam
/// bar stays labelled partial instead.
#[must_use]
pub const fn reads_seam_minutes(interval_ms: i64) -> bool {
    interval_ms > 0 && interval_ms <= DAY_MS
}

/// The seam's minutes: held, asked for, and what their answer vouches for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeamMinutes {
    bars: Vec<Bar>,
    /// The stretch last asked for.
    asked: Option<(i64, i64)>,
    /// What the answer vouches for; `None` until one lands.
    answer: Option<MinutesAnswer>,
    out: bool,
}

impl SeamMinutes {
    /// The request for `stretch`, unless one is out or the stretch asked
    /// already covers it.
    #[must_use]
    pub fn ask(&self, stretch: Option<(i64, i64)>) -> Option<CandleAsk> {
        let (from, to) = stretch?;
        let covered = self
            .asked
            .is_some_and(|(asked_from, asked_to)| asked_from <= from && asked_to >= to);
        (!self.out && !covered).then_some(CandleAsk {
            interval_ms: OHLCV_BASE_INTERVAL_MS,
            span_ms: to - from,
            slice_ms: None,
            before_ms: Some(to - 1),
        })
    }

    /// The request went out.
    pub fn sent(&mut self, ask: CandleAsk) {
        let to = ask.before_ms.map_or(0, |before| before + 1);
        self.asked = Some((to - ask.span_ms, to));
        self.out = true;
    }

    /// The channel it went out on is gone; it may go out again.
    pub fn abandon(&mut self) {
        if self.out {
            self.out = false;
            self.asked = None;
        }
    }

    /// Take a reply; whether it closed the request.
    pub fn take_reply(&mut self, bars: Vec<Bar>, slice: OhlcvSlice) -> bool {
        match slice {
            OhlcvSlice::More => {
                self.bars.extend(bars);
                false
            }
            OhlcvSlice::Refused => {
                self.abandon();
                true
            }
            OhlcvSlice::Last { complete } => {
                self.bars.extend(bars);
                self.bars.sort_by_key(|bar| bar.open_time);
                self.bars.dedup_by_key(|bar| bar.open_time);
                self.answer = Some(match (complete, self.asked) {
                    (true, Some((from_ms, until_ms))) => {
                        MinutesAnswer::Covers { from_ms, until_ms }
                    }
                    _ => MinutesAnswer::Short,
                });
                self.out = false;
                true
            }
        }
    }

    /// The lead of a pane cut at `interval_ms` whose first trade is
    /// `first_trade_ms`, from the minutes held.
    #[must_use]
    pub fn lead(&self, first_trade_ms: i64, interval_ms: i64) -> SeamLead {
        let minutes = self.answer.map(|answer| Minutes {
            candles: &self.bars,
            answer,
        });
        seam_lead(None, minutes, first_trade_ms, interval_ms)
    }
}

#[cfg(test)]
mod seam_tests {
    use super::*;
    use rust_decimal::Decimal;

    const MINUTE: i64 = OHLCV_BASE_INTERVAL_MS;
    const QUARTER: i64 = 15 * MINUTE;
    const DAY: i64 = 20_000 * DAY_MS;

    fn minute(open_time: i64) -> Bar {
        Bar {
            open_time,
            close_time: open_time + MINUTE - 1,
            open: Decimal::ONE,
            high: Decimal::TWO,
            low: Decimal::ONE,
            close: Decimal::ONE,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn the_stretch_runs_from_the_earliest_seam_bucket_to_the_latest_first_trade() {
        let first_trade = DAY + 10 * 60 * MINUTE + 7 * MINUTE;
        assert_eq!(
            seam_stretch([(first_trade, QUARTER), (first_trade, DAY_MS)]),
            Some((DAY, first_trade))
        );
        assert_eq!(
            seam_stretch([(first_trade, 7 * DAY_MS)]),
            None,
            "a week of minutes is not asked for"
        );
        assert_eq!(seam_stretch([(DAY, DAY_MS)]), None, "nothing before it");
    }

    #[test]
    fn the_stretch_is_asked_once_and_its_minutes_lead_the_seam_bar() {
        let first_trade = DAY + 7 * MINUTE;
        let stretch = seam_stretch([(first_trade, QUARTER)]);
        let mut seam = SeamMinutes::default();
        let ask = seam.ask(stretch).expect("asked");
        assert_eq!(ask.before_ms, Some(first_trade - 1));
        assert_eq!(ask.span_ms, 7 * MINUTE);
        seam.sent(ask);
        assert_eq!(seam.ask(stretch), None, "one out");
        let bars = (0..7).map(|index| minute(DAY + index * MINUTE)).collect();
        assert!(seam.take_reply(bars, OhlcvSlice::Last { complete: true }));
        assert_eq!(seam.ask(stretch), None, "covered");
        let lead = seam.lead(first_trade, QUARTER);
        assert!(lead.covers_seam(), "{lead:?}");
        let bar = lead.into_bar().expect("a lead");
        assert_eq!(bar.open_time, DAY);
        assert_eq!(bar.trade_count, 7);
    }

    #[test]
    fn without_minutes_the_lead_says_so() {
        let seam = SeamMinutes::default();
        assert!(!seam.lead(DAY + 7 * MINUTE, QUARTER).covers_seam());
    }
}
