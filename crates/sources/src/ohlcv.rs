//! The venue-candle vocabulary: the bases a chart folds from, how far one
//! request reaches, and where one reply sits in the run answering it.
//!
//! Below the feed host because two sides speak it: the host that phrases a
//! `FeedCommand::FetchOhlcv` to a venue and tags each reply, and the tab
//! session that decides what to ask and folds what comes back. The host
//! re-exports every name here at its old path.

/// How far back a time pane asks for candle history in **one** request: seven
/// days.
///
/// Trade backfill and candle history answer different questions and are sized
/// differently on purpose. The tape is a recent window — what is happening now,
/// in full detail. Candles are the context around it.
///
/// This used to be ninety days, on the reasoning that a quarter is the
/// shortest span a weekly chart says anything in. That reasoning was about the
/// *deepest* chart a trader might open, and it was charged to every chart they
/// actually open: a quarter of one-minute candles is over a hundred sequential
/// venue pages, and the trader waits through all of them at every launch to
/// read the last hour. A week is the span a session starts from — a few
/// seconds of fetching, and the intraday context already there.
///
/// The quarter is not gone, it is asked for: each *load older* request reaches
/// another span of this size further back and prepends it (see
/// `Tab::request_older_ohlcv_history`), so the deep chart is thirteen clicks,
/// paid by the trader who wants it rather than by everyone who does not.
pub const TIME_HISTORY_SPAN_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

/// The interval every provider delivers intraday candle history in: one
/// minute.
///
/// A single base series, resampled locally to whatever the pane shows, is what
/// makes this work across four very different transports. The venues disagree
/// about paging, and MetaTrader cannot be *asked* for anything at all (its
/// bridge only pushes), so "every provider answers the same one request" is
/// the only contract all of them can keep. It also makes switching a pane's
/// interval free — no refetch, just a different fold over bars already held.
pub const OHLCV_BASE_INTERVAL_MS: i64 = 60_000;

/// The base a pane cut at a day or longer asks for instead: one UTC day.
///
/// A week of minutes is seven daily bars, and the quarter the deepest minute
/// reach gets to is thirteen weekly ones — a daily, weekly or monthly chart
/// needs years, which only the venue's own daily candles reach in a few
/// requests. Asked for, never assumed: a provider that cannot serve days
/// answers in minutes, tagged as such, and the fold builds the days from them
/// over the shorter reach.
pub const OHLCV_DAILY_INTERVAL_MS: i64 = quantick_engine::time_bucket::DAY_MS;

/// How far back one request for daily candles reaches: five years.
///
/// Two Binance pages of a thousand days, one MetaTrader `CopyRates` call —
/// long enough for a monthly chart to say something, short enough to land in
/// the time a minute request takes. *Load older* reaches another span.
pub const DAILY_HISTORY_SPAN_MS: i64 = 1_826 * OHLCV_DAILY_INTERVAL_MS;

/// The candle base a pane cut at `interval_ms` folds from: days for a day or
/// longer that is a whole number of them (or calendar months), minutes for
/// everything else.
#[must_use]
pub fn ohlcv_base_interval_for(interval_ms: i64) -> i64 {
    if crate::candles::is_foldable(OHLCV_DAILY_INTERVAL_MS, interval_ms) {
        OHLCV_DAILY_INTERVAL_MS
    } else {
        OHLCV_BASE_INTERVAL_MS
    }
}

/// How far one candle request at `base_interval_ms` reaches.
#[must_use]
pub fn ohlcv_span_for(base_interval_ms: i64) -> i64 {
    if base_interval_ms >= OHLCV_DAILY_INTERVAL_MS {
        DAILY_HISTORY_SPAN_MS
    } else {
        TIME_HISTORY_SPAN_MS
    }
}

/// How much of the span one progressive slice covers: two days.
///
/// Chosen against [`TIME_HISTORY_SPAN_MS`], not in isolation — and re-derived
/// when that span became a week. Left at a week it would have been dead
/// policy: a slice as wide as the span is a single window
/// (`ohlcv_plan::plan`), so every request would go straight back to the
/// all-at-once wait progressive loading exists to remove.
///
/// The width is a trade between two costs pulling opposite ways, and a week is
/// short enough that the second one now dominates:
///
/// - **Time to the first readable frame.** Four windows means the newest two
///   days land in roughly a quarter of the venue round trips the span costs.
///   That is the number the trader feels — how long the chart stays empty.
/// - **Refold work per arrival.** Every slice runs `refold_history_prefix`, a
///   fold over the *whole* accumulated base plus an indicator rebuild per
///   pane. That is bounded per request by `ohlcv_plan::MAX_SLICES`, but not
///   across requests: a trader paging back thirteen spans to the old quarter
///   pays it once per slice, against a base growing toward six figures. At one
///   slice a day that is 91 full refolds for the same final chart; at two days
///   it is 52, and at a week it would be 13 with no progressive painting at
///   all.
///
/// Two days keeps the opening week painting in four steps while cutting the
/// deep-chart refold bill by nearly half. Narrower paints marginally sooner
/// and multiplies that bill; wider approaches the all-at-once wait again.
pub const OHLCV_SLICE_SPAN_MS: i64 = 2 * 24 * 60 * 60 * 1_000;

/// Where one `FeedEvent::OhlcvHistory` sits in the run of replies answering
/// a single `FeedCommand::FetchOhlcv`.
///
/// The whole point of the enum is that "how complete is the span?" is a
/// question only the closing reply can answer. An intermediate slice knows
/// nothing about the windows still being fetched, so it is given no field in
/// which to claim otherwise — a `complete` flag on every reply would be a
/// number that means something different depending on which reply carried it,
/// which is exactly the kind of quiet lie the data-honesty rule exists to
/// prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhlcvSlice {
    /// An older slice of the same request follows. The wait is not over and
    /// the loading indicator stays up.
    More,
    /// The request was **refused before it was served**: a fetch for this
    /// market was already in flight and the provider serves one at a time.
    ///
    /// A closing reply, because the caller raised a spinner before the command
    /// left and a silent drop would leave it turning forever. But a distinct
    /// one, because "nobody looked" is not "the venue came up short": answered
    /// as `Last { complete: false }` the tab warns that a venue stopped short
    /// when none did, refolds the whole accumulated base over an empty vector,
    /// and consumes the reach-back measurement for a request that was never
    /// made. This variant ends the wait and changes nothing else.
    Refused,
    /// The last reply for this request — and the only one a provider that
    /// serves the whole span at once ever sends.
    Last {
        /// Whether the run as a whole covered the span that was asked for.
        ///
        /// False means the answer is short and *known* to be short: a venue
        /// that stopped answering partway, a bridge whose paging failed after
        /// some pages had landed, a block clipped to a cap. It does not mean
        /// the same thing as a short series — an instrument younger than the
        /// span genuinely has fewer candles, and that answer is complete.
        ///
        /// Carried rather than inferred from the bar count, which cannot tell
        /// those two apart, and cannot tell either from a quiet market. In a
        /// sliced run it is the conjunction over every window: one window that
        /// came up short makes the whole answer short.
        complete: bool,
    },
}

impl OhlcvSlice {
    /// Whether this reply closes its request.
    #[must_use]
    pub fn is_last(self) -> bool {
        matches!(self, Self::Last { .. } | Self::Refused)
    }
}
