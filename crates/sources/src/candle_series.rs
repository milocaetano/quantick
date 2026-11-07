//! One series of venue candles per interval, for a venue that serves its own
//! candles at every interval a chart is cut at.
//!
//! A venue with that capability is asked for the pane's interval itself,
//! a page of [`CANDLE_PAGE_BARS`] at a time: the opening page from the live
//! edge, and one more page behind the oldest candle held whenever the pane
//! scrolls within [`REACH_BACK_MARGIN_BARS`] of its oldest bar. A series is
//! kept per interval, so a trip to another interval and back costs nothing,
//! and two panes at different intervals each read their own.
//!
//! Plain bookkeeping and no I/O: the tab session decides when to ask, and
//! this says what to ask for and what a reply did to the series.

use quantick_engine::Bar;
use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS};

use crate::candle_base::CandleAsk;
use crate::candles::is_foldable;
use crate::ohlcv::OhlcvSlice;

/// How many candles one request asks the venue for: one Binance page.
pub const CANDLE_PAGE_BARS: usize = 1_000;

/// How close to its oldest bar a pane may scroll before the page behind it
/// is asked for: far enough ahead that a steady drag never reaches an empty
/// edge, near enough that a chart opened and left alone never asks.
pub const REACH_BACK_MARGIN_BARS: usize = 200;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// The intervals a series is asked for at: the ones every venue with the
/// capability serves. A pane at any other interval reads the coarsest of
/// these that folds into it — `6h` from `2h`, `10m` from `5m`.
pub const NATIVE_CANDLE_INTERVALS_MS: [i64; 14] = [
    MINUTE_MS,
    3 * MINUTE_MS,
    5 * MINUTE_MS,
    15 * MINUTE_MS,
    30 * MINUTE_MS,
    HOUR_MS,
    2 * HOUR_MS,
    4 * HOUR_MS,
    8 * HOUR_MS,
    12 * HOUR_MS,
    DAY_MS,
    3 * DAY_MS,
    WEEK_MS,
    CALENDAR_MONTH_MS,
];

/// The series a pane cut at `interval_ms` reads: the interval itself when the
/// venue serves it, else the coarsest served interval that folds into it,
/// else none (a sub-minute or `90s` pane).
#[must_use]
pub fn native_series_for(interval_ms: i64) -> Option<i64> {
    if NATIVE_CANDLE_INTERVALS_MS.contains(&interval_ms) {
        return Some(interval_ms);
    }
    NATIVE_CANDLE_INTERVALS_MS
        .iter()
        .rev()
        .copied()
        .find(|&native| is_foldable(native, interval_ms))
}

/// Where a series stands, as the control plane reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesState {
    /// Nothing out; scrolling near the oldest bar would ask for more.
    Idle,
    /// A request is out.
    Loading,
    /// The venue's record starts at the oldest candle held.
    Exhausted,
}

impl SeriesState {
    /// The stable word for the wire and the log.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Exhausted => "exhausted",
        }
    }
}

/// One pane's candle series, as a reader sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesStatus {
    /// Where it stands.
    pub state: SeriesState,
    /// The interval its candles were asked for at.
    pub interval_ms: i64,
    /// How many candles it holds.
    pub candles_held: usize,
    /// When its oldest candle opened.
    pub oldest_open_ms: Option<i64>,
}

/// What a closing reply did to a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settled {
    /// Candles the series gained.
    pub added: usize,
    /// Whether the venue's record now starts at the oldest candle held.
    pub exhausted: bool,
}

/// The venue's candles at one interval, ascending, and where asking for more
/// stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandleSeries {
    bars: Vec<Bar>,
    /// Whether a closing reply has landed, even an empty one: what keeps an
    /// unanswered series apart from one the venue has nothing for.
    answered: bool,
    exhausted: bool,
    /// The request out, one at a time.
    pending: Option<CandleAsk>,
}

impl CandleSeries {
    /// The opening request at `interval_ms`: one page back from the live edge.
    #[must_use]
    pub fn opening_ask(interval_ms: i64) -> CandleAsk {
        let _ = interval_ms;
        todo!("opening ask")
    }

    /// The next request this series wants, if any: the opening one until a
    /// reply lands, then the page behind the oldest candle while the pane's
    /// first visible slot is within [`REACH_BACK_MARGIN_BARS`] of it. Never a
    /// second one while one is out, and never past the record's start.
    #[must_use]
    pub fn next_ask(&self, interval_ms: i64, first_visible: Option<usize>) -> Option<CandleAsk> {
        let _ = (interval_ms, first_visible);
        todo!("next ask")
    }

    /// The request went out.
    pub fn sent(&mut self, ask: CandleAsk) {
        let _ = ask;
        todo!("sent")
    }

    /// The channel the request went out on is gone: nothing will answer it.
    pub fn abandon(&mut self) {
        todo!("abandon")
    }

    /// Take one reply to the request out. `None` while the run goes on, and
    /// for a refusal, which says nothing about the venue's record.
    pub fn take_reply(&mut self, bars: Vec<Bar>, slice: OhlcvSlice) -> Option<Settled> {
        let _ = (bars, slice);
        todo!("take reply")
    }

    /// The candles, ascending.
    #[must_use]
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// When the oldest candle held opened.
    #[must_use]
    pub fn oldest_ms(&self) -> Option<i64> {
        self.bars.first().map(|bar| bar.open_time)
    }

    /// Whether a closing reply has landed.
    #[must_use]
    pub const fn is_answered(&self) -> bool {
        self.answered
    }

    /// Whether a request is out.
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether the venue's record starts at the oldest candle held.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Where the series stands, at `interval_ms`.
    #[must_use]
    pub fn status(&self, interval_ms: i64) -> SeriesStatus {
        let _ = interval_ms;
        todo!("status")
    }
}

#[cfg(test)]
#[path = "candle_series_tests.rs"]
mod candle_series_tests;
