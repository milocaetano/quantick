//! Which venue-candle base a chart folds from, and which one it is after.
//!
//! A chart folds every pane's prefix from one base of venue candles per
//! market — minutes, or days for panes cut at a day or longer — and keeps the
//! other one parked while it is not wanted. The decisions about those bases
//! are plain values and live here, below the application: which base the
//! panes want, whether a reply's candles may join the base held, what one
//! request asks the provider for, which parked base comes back, and whether
//! the provider's answer changed. The tab keeps one [`CandleBaseInterval`]
//! and carries out what it says.

use quantick_engine::Bar;

use crate::config::FeedCapabilities;
use crate::candles::{SeamLead, is_foldable, merge_older_candles, seam_lead};
use crate::{
    OHLCV_BASE_INTERVAL_MS, OHLCV_DAILY_INTERVAL_MS, OHLCV_SLICE_SPAN_MS, OhlcvSlice,
    ohlcv_base_interval_for, ohlcv_span_for,
};

/// The interval of the candles held and of the candles wanted, the base the
/// panes stopped wanting, and the provider generations already acted on.
///
/// Held and wanted differ when a provider without daily candles answers a
/// daily request in minutes: the minutes are held and folded, the days stay
/// wanted, and only a change of what is *wanted* is a reason to fetch again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandleBaseInterval {
    held_ms: i64,
    wanted_ms: i64,
    /// A settled base kept while the panes want the other one, so a trip from
    /// 5m to 1d and back neither refetches the minutes nor loses what *load
    /// older* paged into them.
    parked: Option<ParkedBase>,
    /// The candle generations already acted on, minutes then days.
    seen_generations: (u64, u64),
    /// Where the one request for the seam lead's minutes stands.
    seam_minutes: SeamMinutes,
}

/// The minutes a daily base needs for its seam lead (see
/// [`crate::candles::seam_lead`]): the seam day's own candle overlaps the
/// chart's trades, so the stretch of it before the first trade comes from
/// minutes, asked for once beside the days and kept parked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SeamMinutes {
    /// Not asked for this base.
    #[default]
    NotAsked,
    /// The request is out; its reply goes to the parked slot.
    Out,
    /// Answered; asked again only once the minutes are gone.
    Answered,
}

/// A settled base the panes stopped wanting.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParkedBase {
    /// The interval it was wanted at — the one that brings it back.
    wanted_ms: i64,
    /// The interval its candles were served at.
    held_ms: i64,
    bars: Vec<Bar>,
    /// Whether a *load older* through it had already found the record's start.
    older_exhausted: bool,
}

/// A base handed back by [`CandleBaseInterval::swap_parked`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredBase {
    /// The candles, as they were when parked.
    pub bars: Vec<Bar>,
    /// Whether *load older* had already found the record's start.
    pub older_exhausted: bool,
}

impl Default for CandleBaseInterval {
    fn default() -> Self {
        Self {
            held_ms: OHLCV_BASE_INTERVAL_MS,
            wanted_ms: OHLCV_BASE_INTERVAL_MS,
            parked: None,
            seen_generations: (0, 0),
            seam_minutes: SeamMinutes::NotAsked,
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
    pub fn held_ms(&self) -> i64 {
        self.held_ms
    }

    /// The interval the chart is after.
    #[must_use]
    pub fn wanted_ms(&self) -> i64 {
        self.wanted_ms
    }

    /// After a change of wish ([`Self::want`] returned `was_ms`): park the
    /// settled base the panes stopped wanting, and hand back the one parked
    /// for the base they want now, if there is one.
    ///
    /// `settled` is `None` while a request is out — a base still filling is
    /// not worth keeping, and its remaining slices are dropped as stale. A
    /// base handed back restores the interval its candles were served at.
    pub fn swap_parked(
        &mut self,
        was_ms: i64,
        settled: Option<Vec<Bar>>,
        older_exhausted: bool,
    ) -> Option<RestoredBase> {
        let wanted_ms = self.wanted_ms;
        let restored = self.parked.take_if(|parked| parked.wanted_ms == wanted_ms);
        if let Some(bars) = settled {
            self.parked = Some(ParkedBase {
                wanted_ms: was_ms,
                held_ms: self.held_ms,
                bars,
                older_exhausted,
            });
        }
        // A new base, a new question: an answer still out is stale.
        self.seam_minutes = SeamMinutes::NotAsked;
        restored.map(|parked| {
            self.held_ms = parked.held_ms;
            RestoredBase {
                bars: parked.bars,
                older_exhausted: parked.older_exhausted,
            }
        })
    }

    /// Forget the parked base: it described another market.
    pub fn drop_parked(&mut self) {
        self.parked = None;
        self.seam_minutes = SeamMinutes::NotAsked;
    }

    /// Take the provider's generations; whether the answer to the base
    /// wanted changed since the last call. A parked base whose own answer
    /// changed is dropped — it is the old answer, and wanting it again asks.
    pub fn observe(&mut self, capabilities: &FeedCapabilities) -> bool {
        let now = (
            capabilities.ohlcv_generation,
            capabilities.ohlcv_daily_generation,
        );
        let seen = std::mem::replace(&mut self.seen_generations, now);
        let moved = |interval_ms: i64| {
            if interval_ms >= OHLCV_DAILY_INTERVAL_MS {
                seen.1 != now.1
            } else {
                seen.0 != now.0
            }
        };
        if self
            .parked
            .as_ref()
            .is_some_and(|parked| moved(parked.wanted_ms))
        {
            self.parked = None;
            self.seam_minutes = SeamMinutes::NotAsked;
        }
        moved(self.wanted_ms)
    }

    /// Make the next [`Self::observe`] see a change, whatever is published.
    pub fn forget_generations(&mut self) {
        self.seen_generations = (u64::MAX, u64::MAX);
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

    /// The minutes parked beside a daily base, if any.
    #[must_use]
    pub fn parked_minutes(&self) -> Option<&[Bar]> {
        self.parked
            .as_ref()
            .filter(|parked| parked.held_ms < OHLCV_DAILY_INTERVAL_MS)
            .map(|parked| parked.bars.as_slice())
    }

    /// The seam lead of a pane cut at `interval_ms` whose first trade is
    /// `first_trade_ms`: from `held`, the base held, and — under a daily
    /// base — the minutes parked beside it.
    #[must_use]
    pub fn seam_lead(&self, held: &[Bar], first_trade_ms: i64, interval_ms: i64) -> SeamLead {
        if self.held_ms >= OHLCV_DAILY_INTERVAL_MS {
            seam_lead(
                Some(held),
                self.parked_minutes(),
                first_trade_ms,
                interval_ms,
            )
        } else {
            seam_lead(None, Some(held), first_trade_ms, interval_ms)
        }
    }

    /// The one request for the minutes a daily base's seam leads lack, when
    /// a pane's lead lacks them: `seams` yields each time pane's first trade
    /// and interval. Asked once per base, never while the minutes are parked
    /// or a request for them is out; unsliced, at the minute base's own span,
    /// so the answer is also the base a trip back to an intraday pane folds.
    #[must_use]
    pub fn seam_minutes_ask(
        &self,
        held: &[Bar],
        seams: impl IntoIterator<Item = (i64, i64)>,
    ) -> Option<CandleAsk> {
        let on_days = self.held_ms >= OHLCV_DAILY_INTERVAL_MS
            && self.wanted_ms >= OHLCV_DAILY_INTERVAL_MS;
        if !on_days
            || self.seam_minutes != SeamMinutes::NotAsked
            || self.parked_minutes().is_some()
        {
            return None;
        }
        seams
            .into_iter()
            .any(|(first_trade_ms, interval_ms)| {
                matches!(
                    self.seam_lead(held, first_trade_ms, interval_ms),
                    SeamLead::MinutesNotHeld { .. }
                )
            })
            .then_some(CandleAsk {
                interval_ms: OHLCV_BASE_INTERVAL_MS,
                span_ms: ohlcv_span_for(OHLCV_BASE_INTERVAL_MS),
                slice_ms: None,
            })
    }

    /// The seam-minutes request went out: its replies go to
    /// [`Self::take_seam_minutes`].
    pub fn seam_minutes_sent(&mut self) {
        self.seam_minutes = SeamMinutes::Out;
    }

    /// Whether the seam-minutes request is out.
    #[must_use]
    pub fn seam_minutes_out(&self) -> bool {
        self.seam_minutes == SeamMinutes::Out
    }

    /// A reply to the seam-minutes request: its minutes join the parked
    /// slot. A refusal was not served, so the request may go out again; a
    /// reply at another interval, or with nothing in it, parks nothing.
    pub fn take_seam_minutes(&mut self, interval_ms: i64, bars: Vec<Bar>, slice: OhlcvSlice) {
        self.seam_minutes = match slice {
            OhlcvSlice::Refused => SeamMinutes::NotAsked,
            OhlcvSlice::More => SeamMinutes::Out,
            OhlcvSlice::Last { .. } => SeamMinutes::Answered,
        };
        if interval_ms != OHLCV_BASE_INTERVAL_MS || bars.is_empty() {
            return;
        }
        match &mut self.parked {
            Some(parked) if parked.held_ms == OHLCV_BASE_INTERVAL_MS => {
                merge_older_candles(&mut parked.bars, bars);
            }
            _ => {
                self.parked = Some(ParkedBase {
                    wanted_ms: OHLCV_BASE_INTERVAL_MS,
                    held_ms: OHLCV_BASE_INTERVAL_MS,
                    bars,
                    older_exhausted: false,
                });
            }
        }
    }

    /// The seam-minutes reply was dropped as stale: ask again if needed.
    pub fn forget_seam_minutes(&mut self) {
        if self.seam_minutes == SeamMinutes::Out {
            self.seam_minutes = SeamMinutes::NotAsked;
        }
    }

    /// The request for the base wanted. Progressive slicing paints a week of
    /// minutes in steps; five years of days is two venue pages, which slicing
    /// would only cut into more replies.
    #[must_use]
    pub fn ask(&self, progressive: bool) -> CandleAsk {
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

    fn candle(open_time: i64) -> Bar {
        Bar {
            open_time,
            close_time: open_time + OHLCV_BASE_INTERVAL_MS - 1,
            open: rust_decimal::Decimal::ONE,
            high: rust_decimal::Decimal::ONE,
            low: rust_decimal::Decimal::ONE,
            close: rust_decimal::Decimal::ONE,
            buy_volume: rust_decimal::Decimal::ONE,
            sell_volume: rust_decimal::Decimal::ZERO,
            trade_count: 1,
        }
    }

    /// 5m to 1d and back: the minute base, with what *load older* learned
    /// about it, comes back as it was parked, and the days wait their turn.
    #[test]
    fn a_settled_base_is_parked_and_comes_back_for_its_own_wish() {
        let mut base = CandleBaseInterval::default();
        let minutes = vec![candle(0), candle(60_000)];
        let was = base.want(OHLCV_DAILY_INTERVAL_MS).unwrap();
        assert_eq!(
            base.swap_parked(was, Some(minutes.clone()), true),
            None,
            "nothing parked for days yet"
        );
        assert_eq!(base.admit(OHLCV_DAILY_INTERVAL_MS, true, true), Ok(()));
        let was = base.want(OHLCV_BASE_INTERVAL_MS).unwrap();
        let days = vec![candle(0)];
        assert_eq!(
            base.swap_parked(was, Some(days.clone()), false),
            Some(RestoredBase {
                bars: minutes,
                older_exhausted: true
            })
        );
        assert_eq!(base.held_ms(), OHLCV_BASE_INTERVAL_MS, "held as served");
        let was = base.want(OHLCV_DAILY_INTERVAL_MS).unwrap();
        assert_eq!(
            base.swap_parked(was, None, false),
            Some(RestoredBase {
                bars: days,
                older_exhausted: false
            })
        );
        assert_eq!(base.held_ms(), OHLCV_DAILY_INTERVAL_MS);
    }

    /// Each base reads its own generation: days arriving leave the minutes'
    /// answer standing, and a parked base whose answer moved is dropped.
    #[test]
    fn each_base_reads_its_own_generation() {
        let mut caps = FeedCapabilities::none();
        let mut base = CandleBaseInterval::default();
        assert!(!base.observe(&caps));
        caps.ohlcv_daily_generation = 1;
        assert!(!base.observe(&caps), "a daily block: minutes unchanged");
        caps.ohlcv_generation = 1;
        assert!(base.observe(&caps));

        let was = base.want(OHLCV_DAILY_INTERVAL_MS).unwrap();
        base.swap_parked(was, Some(vec![candle(0)]), false);
        caps.ohlcv_daily_generation = 2;
        assert!(base.observe(&caps), "the days answer moved");
        let was = base.want(OHLCV_BASE_INTERVAL_MS).unwrap();
        assert!(
            base.swap_parked(was, None, false).is_some(),
            "the parked minutes' answer did not move"
        );
        let was = base.want(OHLCV_DAILY_INTERVAL_MS).unwrap();
        base.swap_parked(was, Some(vec![candle(0)]), false);
        caps.ohlcv_generation = 2;
        assert!(!base.observe(&caps));
        let was = base.want(OHLCV_BASE_INTERVAL_MS).unwrap();
        assert_eq!(
            base.swap_parked(was, None, false),
            None,
            "the parked minutes went stale"
        );
        base.forget_generations();
        assert!(base.observe(&caps));
    }

    /// A daily base's seam lead lacks the seam day's minutes: they are asked
    /// for once, unsliced, parked beside the days, and read back for the
    /// lead — and a trip to an intraday pane gets them back as its base.
    #[test]
    fn the_seam_lead_s_minutes_are_asked_once_and_parked_beside_the_days() {
        let day = OHLCV_DAILY_INTERVAL_MS;
        let minute = OHLCV_BASE_INTERVAL_MS;
        let mut base = CandleBaseInterval::default();
        base.want(day);
        base.swap_parked(minute, None, false);
        assert_eq!(base.admit(day, true, true), Ok(()));
        // Ten whole days, the seam day's among them, and a first trade six
        // hours into it.
        let days: Vec<Bar> = (0..10)
            .map(|index| Bar {
                close_time: index * DAY_MS + DAY_MS - 1,
                ..candle(index * DAY_MS)
            })
            .collect();
        let first_trade = 9 * DAY_MS + 6 * 3_600_000 + 5_000;
        let seams = [(first_trade, DAY_MS), (first_trade, WEEK_MS)];
        assert!(matches!(
            base.seam_lead(&days, first_trade, DAY_MS),
            SeamLead::MinutesNotHeld { .. }
        ));
        let ask = base.seam_minutes_ask(&days, seams).expect("asked");
        assert_eq!(
            ask,
            CandleAsk {
                interval_ms: minute,
                span_ms: crate::TIME_HISTORY_SPAN_MS,
                slice_ms: None,
            }
        );
        base.seam_minutes_sent();
        assert!(base.seam_minutes_out());
        assert_eq!(base.seam_minutes_ask(&days, seams), None, "one at a time");

        // Refused: nobody looked, so it may go out again.
        base.take_seam_minutes(minute, Vec::new(), OhlcvSlice::Refused);
        assert!(base.seam_minutes_ask(&days, seams).is_some());
        base.seam_minutes_sent();
        let minutes: Vec<Bar> = (0..(2 * 1_440))
            .map(|index| candle(8 * DAY_MS + index * minute))
            .collect();
        base.take_seam_minutes(
            minute,
            minutes.clone(),
            OhlcvSlice::Last { complete: true },
        );
        assert!(!base.seam_minutes_out());
        assert_eq!(base.parked_minutes(), Some(minutes.as_slice()));
        assert_eq!(base.held_ms(), day, "the days are still the base");
        let lead = base
            .seam_lead(&days, first_trade, DAY_MS)
            .into_bar()
            .expect("a lead");
        assert_eq!(lead.open_time, 9 * DAY_MS);
        assert_eq!(lead.trade_count, 6 * 60, "00:00 to 06:00, no more");
        assert_eq!(base.seam_minutes_ask(&days, seams), None, "parked: not again");

        let was = base.want(minute).unwrap();
        assert_eq!(
            base.swap_parked(was, Some(days), false),
            Some(RestoredBase {
                bars: minutes,
                older_exhausted: false
            }),
            "the minutes are the intraday base"
        );
        assert_eq!(base.held_ms(), minute);
    }

    /// A stale reply is forgotten, so the request may go out again; an empty
    /// answer is an answer, and is not asked again.
    #[test]
    fn a_dropped_or_empty_seam_minutes_reply_settles_the_ask() {
        let day = OHLCV_DAILY_INTERVAL_MS;
        let mut base = CandleBaseInterval::default();
        base.want(day);
        assert_eq!(base.admit(day, true, true), Ok(()));
        let days: Vec<Bar> = (0..3)
            .map(|index| Bar {
                close_time: index * DAY_MS + DAY_MS - 1,
                ..candle(index * DAY_MS)
            })
            .collect();
        let seams = [(2 * DAY_MS + 60_000_000, DAY_MS)];
        base.seam_minutes_sent();
        base.forget_seam_minutes();
        assert!(base.seam_minutes_ask(&days, seams).is_some());
        base.seam_minutes_sent();
        base.take_seam_minutes(
            OHLCV_BASE_INTERVAL_MS,
            Vec::new(),
            OhlcvSlice::Last { complete: true },
        );
        assert_eq!(base.parked_minutes(), None);
        assert_eq!(base.seam_minutes_ask(&days, seams), None);
        // An intraday pane on a minute base has its minutes held: no ask.
        let minutes = CandleBaseInterval::default();
        assert_eq!(minutes.seam_minutes_ask(&days, seams), None);
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
