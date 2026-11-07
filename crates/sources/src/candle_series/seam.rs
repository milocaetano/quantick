//! The minutes a seam lead reads on the per-interval path.
//!
//! A pane's own series stops before the bucket of its first trade, so the
//! stretch of that bucket before the first trade — the venue's record of
//! what traded there before the chart was watching — comes from minutes
//! asked for just that stretch, unsliced: a day of them at most. A pane cut
//! longer than a day reads the whole days before the seam day from the
//! daily series instead, and minutes only for the seam day.

use quantick_engine::Bar;

use crate::candle_base::{CandleAsk, SEAM_MINUTES_ASKS};
use crate::candles::{Minutes, MinutesAnswer};
use crate::ohlcv::{OHLCV_BASE_INTERVAL_MS, OhlcvSlice};

/// The one stretch, `[from, to)`, covering every pane's lack: `lacks` yields
/// where each pane's lead lacks minutes from, and its first trade.
#[must_use]
pub fn seam_stretch(lacks: impl IntoIterator<Item = (i64, i64)>) -> Option<(i64, i64)> {
    lacks
        .into_iter()
        .filter(|&(from, to)| from < to)
        .reduce(|(from, to), (other_from, other_to)| (from.min(other_from), to.max(other_to)))
}

/// The seam's minutes: held, asked for, and what their answer vouches for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeamMinutes {
    bars: Vec<Bar>,
    /// The stretch last asked for, and how many answers it has had.
    asked: Option<(i64, i64)>,
    answers: u8,
    /// What the answer vouches for; `None` until one lands.
    answer: Option<MinutesAnswer>,
    out: bool,
}

impl SeamMinutes {
    /// The request for `stretch`, unless one is out, or the stretch asked
    /// covers it and was answered whole — or answered short
    /// [`SEAM_MINUTES_ASKS`] times, after which the seam bar stays labelled
    /// partial.
    #[must_use]
    pub fn ask(&self, stretch: Option<(i64, i64)>) -> Option<CandleAsk> {
        let (from, to) = stretch?;
        let covered = self
            .asked
            .is_some_and(|(asked_from, asked_to)| asked_from <= from && asked_to >= to);
        let settled = matches!(self.answer, Some(MinutesAnswer::Covers { .. }))
            || self.answers >= SEAM_MINUTES_ASKS;
        (!self.out && !(covered && settled)).then_some(CandleAsk {
            interval_ms: OHLCV_BASE_INTERVAL_MS,
            span_ms: to - from,
            slice_ms: None,
            before_ms: Some(to - 1),
        })
    }

    /// The request went out. A new stretch lets go of the minutes outside
    /// it: what a seam that moved back no longer reads is not held.
    pub fn sent(&mut self, ask: CandleAsk) {
        let to = ask.before_ms.map_or(0, |before| before + 1);
        let stretch = (to - ask.span_ms, to);
        if self.asked != Some(stretch) {
            let (from, to) = stretch;
            self.bars
                .retain(|bar| bar.open_time >= from && bar.open_time < to);
            self.answer = None;
            self.answers = 0;
        }
        self.asked = Some(stretch);
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
                self.answers = self.answers.saturating_add(1);
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

    /// The minutes held and what their answer vouches for; `None` before an
    /// answer lands.
    #[must_use]
    pub fn minutes(&self) -> Option<Minutes<'_>> {
        self.answer.map(|answer| Minutes {
            candles: &self.bars,
            answer,
        })
    }
}

#[cfg(test)]
mod seam_tests {
    use super::*;
    use quantick_engine::time_bucket::DAY_MS;
    use rust_decimal::Decimal;

    const MINUTE: i64 = OHLCV_BASE_INTERVAL_MS;
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

    fn minutes(from: i64, count: i64) -> Vec<Bar> {
        (0..count)
            .map(|index| minute(from + index * MINUTE))
            .collect()
    }

    #[test]
    fn the_stretch_runs_from_the_earliest_lack_to_the_latest_first_trade() {
        let first_trade = DAY + 10 * 60 * MINUTE + 7 * MINUTE;
        assert_eq!(
            seam_stretch([(DAY + 10 * 60 * MINUTE, first_trade), (DAY, first_trade)]),
            Some((DAY, first_trade))
        );
        assert_eq!(seam_stretch([(DAY, DAY)]), None, "nothing before it");
    }

    #[test]
    fn the_stretch_is_asked_once_and_a_whole_answer_settles_it() {
        let first_trade = DAY + 7 * MINUTE;
        let stretch = Some((DAY, first_trade));
        let mut seam = SeamMinutes::default();
        let ask = seam.ask(stretch).expect("asked");
        assert_eq!(ask.before_ms, Some(first_trade - 1));
        assert_eq!(ask.span_ms, 7 * MINUTE);
        seam.sent(ask);
        assert_eq!(seam.ask(stretch), None, "one out");
        assert!(seam.take_reply(minutes(DAY, 7), OhlcvSlice::Last { complete: true }));
        assert_eq!(seam.ask(stretch), None, "covered");
        let held = seam.minutes().expect("an answer");
        assert_eq!(held.candles.len(), 7);
        assert_eq!(
            held.answer,
            MinutesAnswer::Covers {
                from_ms: DAY,
                until_ms: first_trade
            }
        );
    }

    #[test]
    fn a_short_answer_is_asked_again_once() {
        let stretch = Some((DAY, DAY + 7 * MINUTE));
        let short = OhlcvSlice::Last { complete: false };
        let mut seam = SeamMinutes::default();
        for _ in 0..SEAM_MINUTES_ASKS {
            let ask = seam.ask(stretch).expect("asked");
            seam.sent(ask);
            seam.take_reply(minutes(DAY, 3), short);
        }
        assert_eq!(seam.ask(stretch), None, "the bar stays labelled partial");
        assert_eq!(
            seam.minutes().map(|held| held.answer),
            Some(MinutesAnswer::Short)
        );
    }

    #[test]
    fn a_seam_moved_back_lets_go_of_the_minutes_it_no_longer_reads() {
        let complete = OhlcvSlice::Last { complete: true };
        let mut seam = SeamMinutes::default();
        let today = seam.ask(Some((DAY, DAY + 7 * MINUTE))).expect("asked");
        seam.sent(today);
        seam.take_reply(minutes(DAY, 7), complete);
        let yesterday = (DAY - DAY_MS, DAY - DAY_MS + 5 * MINUTE);
        let ask = seam.ask(Some(yesterday)).expect("a new stretch");
        seam.sent(ask);
        assert!(seam.minutes().is_none(), "nothing vouched for yet");
        seam.take_reply(minutes(yesterday.0, 5), complete);
        let held = seam.minutes().expect("an answer");
        assert_eq!(held.candles.len(), 5, "today's minutes went");
    }
}
