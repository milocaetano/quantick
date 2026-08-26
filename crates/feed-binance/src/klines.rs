//! Venue-native candle history over Binance's public `klines` endpoint.
//!
//! The tape backfill next door answers "what just happened"; this answers "what
//! has been happening", and a quarter of one-minute candles is a different
//! shape of problem: ~130 sequential pages rather than a handful, against an
//! endpoint that will start refusing if pushed. So this module owns the two
//! things [`crate::backfill`] never needed — a page budget and a retry policy —
//! and gives back a **partial** result rather than nothing when it runs out of
//! either. A chart showing six weeks and saying so beats a chart showing
//! nothing because week seven timed out.
//!
//! The HTTP layer sits behind [`KlineSource`] so the paging, dedup and mapping
//! are unit-testable without a network, exactly as [`crate::backfill`] does it.
//!
//! # What a candle can and cannot say
//!
//! Binance is the one venue here that publishes an aggressor split: field 9 of
//! a kline is the taker-buy base volume, so `buy_volume` is the venue's own
//! number and `sell_volume` is the rest of the candle. Both are exact, and
//! [`Bar::delta`] over these bars means what it says — unlike the other
//! providers, where the split does not exist and each side carries half.

use std::collections::BTreeMap;
use std::time::Duration;

use quantick_engine::Bar;
use rust_decimal::Decimal;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::backfill::{BINANCE_REST_BASE, FeedError, MAX_PAGE_LIMIT};

/// The one interval quantick asks for; everything longer is folded locally.
pub const KLINE_INTERVAL_1M: &str = "1m";

/// Milliseconds in the [`KLINE_INTERVAL_1M`] bucket.
pub const ONE_MINUTE_MS: i64 = 60_000;

/// Pause between pages.
///
/// Not a rate limit so much as a manner. A week of one-minute candles is
/// about 11 pages at weight 2 apiece, and a trader paging back to a quarter
/// asks for thirteen such runs — still a small fraction of the 6 000 weight
/// per minute Binance allows, so the budget is never the binding constraint.
/// What this avoids is the burst: requests fired back to back look like
/// something worth throttling, and being throttled costs far more than the
/// second apiece this spends.
const PAGE_DELAY: Duration = Duration::from_millis(25);

/// How many times one page may be retried after a rate-limit refusal.
///
/// Small on purpose. A 429 that survives three honoured `Retry-After` waits is
/// not a blip, and the useful response then is to hand back what was collected
/// and say how far it reached — not to keep a chart waiting on a venue that
/// has already said no three times.
const MAX_PAGE_RETRIES: u32 = 3;

/// Longest a `Retry-After` will be honoured before it is treated as a refusal.
///
/// Binance answers an IP ban (418) with waits measured in minutes. Sleeping
/// through one would look identical to a hung chart from the outside.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(10);

/// Fallback wait when a rate-limit response carries no `Retry-After`.
const DEFAULT_RETRY_WAIT: Duration = Duration::from_secs(1);

/// Pages one history fetch may request.
///
/// The span the app asks for is a week (`app`'s `TIME_HISTORY_SPAN_MS`),
/// which at one-minute buckets and 1 000 rows a page is ~11 pages — and a
/// *load older* asks for another such span rather than a longer one, so no
/// single request grows past that. This bound is far above it, so reaching it is
/// never a sign that the request was ambitious — it means the venue is
/// answering in a way that does not advance the cursor, and the loop stops
/// rather than paging forever against it. The `complete` flag carries that out
/// to the caller instead of a silently short series.
const MAX_PAGES: u32 = 512;

/// Positions inside one kline array. Binance documents this as a fixed-order
/// array of mixed types; naming the slots keeps the mapping readable and makes
/// an added trailing field a non-event.
mod field {
    /// Bucket start, epoch ms.
    pub const OPEN_TIME: usize = 0;
    /// Open price, decimal string.
    pub const OPEN: usize = 1;
    /// High price, decimal string.
    pub const HIGH: usize = 2;
    /// Low price, decimal string.
    pub const LOW: usize = 3;
    /// Close price, decimal string.
    pub const CLOSE: usize = 4;
    /// Base-asset volume over the bucket, decimal string.
    pub const VOLUME: usize = 5;
    /// Bucket end, epoch ms (one millisecond before the next bucket).
    pub const CLOSE_TIME: usize = 6;
    /// Number of trades in the bucket.
    pub const TRADES: usize = 8;
    /// Base-asset volume bought by takers — the aggressor split.
    pub const TAKER_BUY_VOLUME: usize = 9;
    /// Slots this module reads, so a short row is rejected before indexing.
    pub const REQUIRED_LEN: usize = TAKER_BUY_VOLUME + 1;
}

/// One kline, as Binance sends it.
///
/// Prices and volumes stay decimal **strings** here and become [`Decimal`] in
/// [`Kline::to_bar`], so a malformed number is a mapping decision with a place
/// to be counted rather than an opaque deserialisation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kline {
    /// Bucket start, epoch ms.
    pub open_time: i64,
    /// Bucket end as the venue reports it, epoch ms.
    pub close_time: i64,
    /// Open price.
    pub open: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Close price.
    pub close: String,
    /// Base-asset volume over the bucket.
    pub volume: String,
    /// Base-asset volume bought by takers.
    pub taker_buy_volume: String,
    /// Trades aggregated into the bucket.
    pub trades: u64,
}

impl Kline {
    /// Map to an engine [`Bar`] covering `interval_ms`, or `None` when the row
    /// is empty or incoherent.
    ///
    /// `close_time` is computed from the bucket rather than copied from the
    /// venue, so every provider's candles carry the same rule — the fixture
    /// test in this module pins that Binance's own value agrees.
    #[must_use]
    pub fn to_bar(&self, interval_ms: i64) -> Option<Bar> {
        let open = decimal(&self.open)?;
        let high = decimal(&self.high)?;
        let low = decimal(&self.low)?;
        let close = decimal(&self.close)?;
        let volume = Decimal::from_str_exact(&self.volume).ok()?;
        let taker_buy = Decimal::from_str_exact(&self.taker_buy_volume).ok()?;

        // An interval Binance reports with no volume is dropped, following the
        // engine's empty-interval rule: the gap is the honest record that
        // nothing traded, where a carried-forward price would be an invention.
        if volume <= Decimal::ZERO {
            return None;
        }
        // More bought by takers than traded in total is not a candle; charting
        // it would put a negative sell volume on the screen.
        if taker_buy < Decimal::ZERO || taker_buy > volume {
            return None;
        }
        if high < low || open > high || open < low || close > high || close < low {
            return None;
        }

        Some(Bar {
            open_time: self.open_time,
            close_time: self.open_time.saturating_add(interval_ms.saturating_sub(1)),
            open,
            high,
            low,
            close,
            // The venue's own aggressor split: exact on both sides, and the
            // reason delta means something here and nowhere else.
            buy_volume: taker_buy,
            sell_volume: volume.saturating_sub(taker_buy),
            trade_count: self.trades,
        })
    }
}

/// Parse a positive decimal, rejecting the prices no instrument trades at.
fn decimal(text: &str) -> Option<Decimal> {
    let value = Decimal::from_str_exact(text).ok()?;
    (value > Decimal::ZERO).then_some(value)
}

/// Read one kline row out of the venue's mixed-type array.
fn row_to_kline(row: &Value) -> Option<Kline> {
    let row = row.as_array()?;
    if row.len() < field::REQUIRED_LEN {
        return None;
    }
    let text = |index: usize| row.get(index)?.as_str().map(str::to_owned);
    Some(Kline {
        open_time: row.get(field::OPEN_TIME)?.as_i64()?,
        close_time: row.get(field::CLOSE_TIME)?.as_i64()?,
        open: text(field::OPEN)?,
        high: text(field::HIGH)?,
        low: text(field::LOW)?,
        close: text(field::CLOSE)?,
        volume: text(field::VOLUME)?,
        taker_buy_volume: text(field::TAKER_BUY_VOLUME)?,
        trades: row.get(field::TRADES)?.as_u64()?,
    })
}

/// Parse a `GET /api/v3/klines` body.
///
/// Rows that do not have the documented shape are skipped rather than failing
/// the page: one unreadable candle in a quarter is a gap, and losing the other
/// 129 999 to it would be the worse trade. Only a body that is not a JSON array
/// at all is an error.
///
/// # Errors
///
/// Returns [`FeedError::Decode`] when the body is not a JSON array.
pub fn parse_klines(json: &str) -> Result<Vec<Kline>, FeedError> {
    let rows: Value = serde_json::from_str(json).map_err(|e| FeedError::Decode(e.to_string()))?;
    let rows = rows
        .as_array()
        .ok_or_else(|| FeedError::Decode("klines body is not a JSON array".to_string()))?;
    Ok(rows.iter().filter_map(row_to_kline).collect())
}

/// A source of kline pages, following Binance's `klines` semantics.
///
/// `fetch(symbol, interval, start_ms, end_ms, limit)` returns up to `limit`
/// candles whose open time falls in `[start_ms, end_ms]`, ascending.
///
/// As with [`crate::backfill::AggTradeSource`], the `async fn` is only ever
/// used through static dispatch, so `Send`-ness is decided at the concrete call
/// site.
#[allow(async_fn_in_trait)]
pub trait KlineSource {
    /// Fetch one page of klines. See the [trait docs](KlineSource).
    ///
    /// # Errors
    ///
    /// Returns [`FeedError::RateLimited`] when the venue asks us to back off,
    /// and other [`FeedError`] variants on transport, status or decode failure.
    async fn fetch(
        &self,
        symbol: &str,
        interval: &str,
        start_ms: i64,
        end_ms: i64,
        limit: u32,
    ) -> Result<Vec<Kline>, FeedError>;
}

/// The real Binance klines source.
#[derive(Debug, Clone)]
pub struct BinanceKlineHttp {
    base_url: String,
    client: reqwest::Client,
}

impl BinanceKlineHttp {
    /// A source pointing at the public Binance REST base URL.
    #[must_use]
    pub fn new() -> Self {
        Self::with_base_url(BINANCE_REST_BASE)
    }

    /// A source pointing at a custom base URL (for a proxy or a test server).
    #[must_use]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl Default for BinanceKlineHttp {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP statuses Binance uses to say "stop": 429 asks, 418 has stopped asking.
const STATUS_TOO_MANY_REQUESTS: u16 = 429;
const STATUS_IM_A_TEAPOT: u16 = 418;

impl KlineSource for BinanceKlineHttp {
    async fn fetch(
        &self,
        symbol: &str,
        interval: &str,
        start_ms: i64,
        end_ms: i64,
        limit: u32,
    ) -> Result<Vec<Kline>, FeedError> {
        let limit = limit.min(MAX_PAGE_LIMIT);
        let url = format!("{}/api/v3/klines", self.base_url);
        let query: Vec<(&str, String)> = vec![
            ("symbol", symbol.to_string()),
            ("interval", interval.to_string()),
            ("startTime", start_ms.to_string()),
            ("endTime", end_ms.to_string()),
            ("limit", limit.to_string()),
        ];
        debug!(target: "quantick::feed", symbol, interval, start_ms, end_ms, limit, "fetching klines page");

        let resp = self
            .client
            .get(&url)
            .query(&query)
            .send()
            .await
            .map_err(|e| FeedError::Http(e.to_string()))?;
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = resp
            .text()
            .await
            .map_err(|e| FeedError::Http(e.to_string()))?;

        if status.as_u16() == STATUS_TOO_MANY_REQUESTS || status.as_u16() == STATUS_IM_A_TEAPOT {
            return Err(FeedError::RateLimited { retry_after });
        }
        if !status.is_success() {
            warn!(target: "quantick::feed", %status, symbol, "klines request returned non-success");
            return Err(FeedError::Http(format!("HTTP {status}: {body}")));
        }
        parse_klines(&body)
    }
}

/// What a history fetch came back with, and whether it is the whole span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KlineHistory {
    /// Candles, ascending by `open_time`, deduplicated.
    pub bars: Vec<Bar>,
    /// Whether the requested span was covered end to end.
    ///
    /// False means the venue stopped answering partway and these bars are what
    /// was collected before that. Kept as a field rather than inferred from the
    /// bar count, which cannot tell a short answer from a quiet market.
    pub complete: bool,
}

/// Fetch `interval_ms` candles covering `[from_ms, to_ms]`, ascending.
///
/// Pages forward from `from_ms`, merging through a [`BTreeMap`] keyed by open
/// time so repeated buckets across page boundaries settle deterministically and
/// the result needs no sort. Stops early — with `complete: false` — when the
/// venue refuses past [`MAX_PAGE_RETRIES`], and logs how far it reached.
pub async fn fetch_history<S: KlineSource>(
    source: &S,
    symbol: &str,
    interval: &str,
    interval_ms: i64,
    from_ms: i64,
    to_ms: i64,
) -> KlineHistory {
    let mut bars: BTreeMap<i64, Bar> = BTreeMap::new();
    let mut cursor = from_ms;
    let mut pages = 0_u32;
    let mut dropped = 0_u64;
    let mut complete = true;

    while cursor <= to_ms && pages < MAX_PAGES {
        let page = match fetch_page_with_retries(source, symbol, interval, cursor, to_ms).await {
            Ok(page) => page,
            Err(error) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "BINANCE_OHLCV_INCOMPLETE",
                    symbol,
                    interval,
                    pages,
                    collected = bars.len(),
                    reached_ms = cursor,
                    requested_to_ms = to_ms,
                    %error,
                    action = "return_partial",
                    "candle history stopped short; returning what was collected"
                );
                complete = false;
                break;
            }
        };
        pages = pages.saturating_add(1);
        if page.is_empty() {
            break; // the venue has nothing further in this span
        }

        let mut newest = cursor;
        for kline in &page {
            newest = newest.max(kline.open_time);
            match kline.to_bar(interval_ms) {
                Some(bar) => {
                    bars.insert(bar.open_time, bar);
                }
                None => dropped = dropped.saturating_add(1),
            }
        }
        // Step past the newest bucket seen. A venue that returned nothing newer
        // than the cursor would otherwise re-request the same page forever.
        let next = newest.saturating_add(interval_ms);
        if next <= cursor {
            break;
        }
        cursor = next;
        if cursor <= to_ms {
            tokio::time::sleep(PAGE_DELAY).await;
        }
    }

    if pages >= MAX_PAGES && cursor <= to_ms {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "BINANCE_OHLCV_PAGE_BUDGET_SPENT",
            symbol,
            interval,
            max_pages = MAX_PAGES,
            collected = bars.len(),
            reached_ms = cursor,
            requested_to_ms = to_ms,
            action = "return_partial",
            "the page budget ran out before the span did; the venue is not advancing"
        );
        complete = false;
    }

    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "BINANCE_OHLCV_READY",
        symbol,
        interval,
        pages,
        bars = bars.len(),
        // Empty or incoherent buckets, counted rather than quietly absent.
        dropped,
        complete,
        "candle history fetched"
    );
    KlineHistory {
        bars: bars.into_values().collect(),
        complete,
    }
}

/// One page, honouring `Retry-After` up to [`MAX_PAGE_RETRIES`] times.
async fn fetch_page_with_retries<S: KlineSource>(
    source: &S,
    symbol: &str,
    interval: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<Kline>, FeedError> {
    let mut attempt = 0_u32;
    loop {
        let error = match source
            .fetch(symbol, interval, start_ms, end_ms, MAX_PAGE_LIMIT)
            .await
        {
            Ok(page) => return Ok(page),
            Err(error) => error,
        };
        let FeedError::RateLimited { retry_after } = error else {
            return Err(error);
        };
        attempt = attempt.saturating_add(1);
        if attempt > MAX_PAGE_RETRIES {
            return Err(FeedError::RateLimited { retry_after });
        }
        // A wait longer than the cap is a ban, not a queue: honouring it would
        // be indistinguishable from a hung chart.
        let wait = retry_after.unwrap_or(DEFAULT_RETRY_WAIT);
        if wait > MAX_RETRY_WAIT {
            return Err(FeedError::RateLimited { retry_after });
        }
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "BINANCE_RATE_LIMITED",
            symbol,
            interval,
            start_ms,
            attempt,
            max_attempts = MAX_PAGE_RETRIES,
            wait_ms = wait.as_millis() as u64,
            action = "wait_and_retry",
            "the venue asked us to slow down"
        );
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// One verbatim row from Binance's `klines` documentation, retyped for a
    /// one-minute BTCUSDT bucket. The mixed types are the point: two integers,
    /// a trade count, and eight decimal strings, in a fixed order.
    const PAGE: &str = r#"[
      [1499040000000,"0.01634790","0.80000000","0.01575800","0.01577100","148976.11427815",
       1499040059999,"2434.19055334",308,"1756.87402397","28.46694368","0"]
    ]"#;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str_exact(s).unwrap()
    }

    #[test]
    fn a_documented_row_maps_with_the_venues_own_aggressor_split() {
        let klines = parse_klines(PAGE).expect("the documented shape parses");
        assert_eq!(klines.len(), 1);
        let bar = klines[0].to_bar(ONE_MINUTE_MS).expect("maps");

        assert_eq!(bar.open_time, 1_499_040_000_000);
        // The bucket rule this crate applies agrees with Binance's own close
        // time, which is what lets every provider carry the same rule.
        assert_eq!(bar.close_time, klines[0].close_time);
        assert_eq!(bar.close_time, bar.open_time + ONE_MINUTE_MS - 1);

        assert_eq!(bar.open, dec("0.01634790"));
        assert_eq!(bar.high, dec("0.80000000"));
        assert_eq!(bar.low, dec("0.01575800"));
        assert_eq!(bar.close, dec("0.01577100"));
        assert_eq!(bar.trade_count, 308);

        // Field 9 is the taker-buy base volume: buys are the venue's number,
        // sells are the remainder, and both are exact.
        assert_eq!(bar.buy_volume, dec("1756.87402397"));
        assert_eq!(
            bar.sell_volume,
            dec("148976.11427815") - dec("1756.87402397")
        );
        assert_eq!(bar.volume(), dec("148976.11427815"));
        assert!(
            bar.delta() < Decimal::ZERO,
            "sellers dominated this bucket, and here that is a measurement"
        );
    }

    #[test]
    fn an_empty_or_incoherent_bucket_is_dropped() {
        let zero_volume = Kline {
            open_time: 0,
            close_time: 59_999,
            open: "10".into(),
            high: "10".into(),
            low: "10".into(),
            close: "10".into(),
            volume: "0".into(),
            taker_buy_volume: "0".into(),
            trades: 0,
        };
        assert!(zero_volume.to_bar(ONE_MINUTE_MS).is_none());

        // More bought by takers than traded at all: a negative sell volume.
        let over_bought = Kline {
            volume: "5".into(),
            taker_buy_volume: "6".into(),
            ..zero_volume.clone()
        };
        assert!(over_bought.to_bar(ONE_MINUTE_MS).is_none());

        // High below low.
        let inverted = Kline {
            volume: "5".into(),
            taker_buy_volume: "1".into(),
            high: "9".into(),
            low: "11".into(),
            ..zero_volume.clone()
        };
        assert!(inverted.to_bar(ONE_MINUTE_MS).is_none());

        // A price no instrument trades at.
        let free = Kline {
            volume: "5".into(),
            taker_buy_volume: "1".into(),
            open: "0".into(),
            ..zero_volume
        };
        assert!(free.to_bar(ONE_MINUTE_MS).is_none());
    }

    #[test]
    fn a_short_row_is_skipped_and_the_page_survives() {
        // Forward compatibility in both directions: a row missing the fields
        // this reads is skipped, and a row with extra trailing ones is fine.
        let mixed = r#"[
          [1,"1","2","0.5","1.5","10",60000,"15",3,"4"],
          [2,"1","2"],
          [3,"1","2","0.5","1.5","10",120000,"15",3,"4","5","0","future-field"]
        ]"#;
        let klines = parse_klines(mixed).expect("parses");
        assert_eq!(klines.len(), 2, "only the truncated row is skipped");
        assert_eq!(klines[0].open_time, 1);
        assert_eq!(klines[1].open_time, 3);

        // A body that is not an array at all is a real error.
        assert!(matches!(
            parse_klines(r#"{"code":-1121,"msg":"Invalid symbol."}"#),
            Err(FeedError::Decode(_))
        ));
    }

    /// A scripted source: each `fetch` pops the next queued outcome.
    struct ScriptedSource {
        pages: RefCell<Vec<Result<Vec<Kline>, FeedError>>>,
        calls: RefCell<Vec<i64>>,
    }

    impl ScriptedSource {
        fn new(pages: Vec<Result<Vec<Kline>, FeedError>>) -> Self {
            Self {
                pages: RefCell::new(pages),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl KlineSource for ScriptedSource {
        async fn fetch(
            &self,
            _symbol: &str,
            _interval: &str,
            start_ms: i64,
            _end_ms: i64,
            _limit: u32,
        ) -> Result<Vec<Kline>, FeedError> {
            self.calls.borrow_mut().push(start_ms);
            let mut pages = self.pages.borrow_mut();
            if pages.is_empty() {
                return Ok(Vec::new());
            }
            pages.remove(0)
        }
    }

    /// A coherent one-minute candle: `close` must sit inside the 9..12 range
    /// the helper fixes, or the mapper rightly refuses the row.
    fn kline(open_time: i64, close: &str) -> Kline {
        Kline {
            open_time,
            close_time: open_time + ONE_MINUTE_MS - 1,
            open: "10".into(),
            high: "12".into(),
            low: "9".into(),
            close: close.into(),
            volume: "10".into(),
            taker_buy_volume: "4".into(),
            trades: 7,
        }
    }

    #[tokio::test]
    async fn paging_walks_forward_and_merges_overlaps_deterministically() {
        // Pages overlap at their seam, which is ordinary: the second page
        // starts at the bucket after the newest one seen, and a venue may
        // repeat it. The later value wins and the series stays ascending.
        let source = ScriptedSource::new(vec![
            Ok(vec![kline(0, "9.5"), kline(ONE_MINUTE_MS, "10.5")]),
            Ok(vec![
                kline(ONE_MINUTE_MS, "11.5"),
                kline(2 * ONE_MINUTE_MS, "10.75"),
            ]),
            Ok(Vec::new()),
        ]);
        let history = fetch_history(
            &source,
            "BTCUSDT",
            KLINE_INTERVAL_1M,
            ONE_MINUTE_MS,
            0,
            10 * ONE_MINUTE_MS,
        )
        .await;

        assert!(history.complete);
        assert_eq!(history.bars.len(), 3, "the repeated bucket merged");
        assert_eq!(
            history.bars.iter().map(|b| b.open_time).collect::<Vec<_>>(),
            [0, ONE_MINUTE_MS, 2 * ONE_MINUTE_MS]
        );
        assert_eq!(
            history.bars[1].close,
            dec("11.5"),
            "a repeated bucket is a correction: the later value wins"
        );
    }

    #[tokio::test]
    async fn a_venue_that_keeps_refusing_yields_a_labelled_partial() {
        // The case that decides between "six weeks, and we say so" and "an
        // empty chart because week seven timed out".
        let source = ScriptedSource::new(vec![
            Ok(vec![kline(0, "10.5")]),
            Err(FeedError::RateLimited {
                retry_after: Some(Duration::from_secs(0)),
            }),
            Err(FeedError::RateLimited {
                retry_after: Some(Duration::from_secs(0)),
            }),
            Err(FeedError::RateLimited {
                retry_after: Some(Duration::from_secs(0)),
            }),
            Err(FeedError::RateLimited {
                retry_after: Some(Duration::from_secs(0)),
            }),
        ]);
        let history = fetch_history(
            &source,
            "BTCUSDT",
            KLINE_INTERVAL_1M,
            ONE_MINUTE_MS,
            0,
            100 * ONE_MINUTE_MS,
        )
        .await;

        assert!(!history.complete, "the shortfall is labelled, not hidden");
        assert_eq!(history.bars.len(), 1, "what was collected is kept");
        // One initial attempt plus MAX_PAGE_RETRIES on the failing page.
        assert_eq!(
            source.calls.borrow().len(),
            1 + 1 + MAX_PAGE_RETRIES as usize
        );
    }

    #[tokio::test]
    async fn a_ban_length_wait_is_refused_rather_than_slept_through() {
        let source = ScriptedSource::new(vec![Err(FeedError::RateLimited {
            retry_after: Some(MAX_RETRY_WAIT + Duration::from_secs(1)),
        })]);
        let history = fetch_history(
            &source,
            "BTCUSDT",
            KLINE_INTERVAL_1M,
            ONE_MINUTE_MS,
            0,
            10 * ONE_MINUTE_MS,
        )
        .await;

        assert!(!history.complete);
        assert!(history.bars.is_empty());
        assert_eq!(
            source.calls.borrow().len(),
            1,
            "a ban is not retried; it is reported"
        );
    }

    // Paused clock: the budget is 512 pages and each pays PAGE_DELAY, which is
    // thirteen real seconds of nothing. The scripted source does no I/O, so
    // auto-advancing the timer changes what is measured not at all.
    #[tokio::test(start_paused = true)]
    async fn the_page_budget_bounds_a_venue_that_answers_one_candle_at_a_time() {
        // A venue that advances by exactly one bucket per page would need
        // one request per bucket. The budget is what turns that from
        // an unbounded loop into a labelled partial.
        struct Dribble;
        impl KlineSource for Dribble {
            async fn fetch(
                &self,
                _symbol: &str,
                _interval: &str,
                start_ms: i64,
                _end_ms: i64,
                _limit: u32,
            ) -> Result<Vec<Kline>, FeedError> {
                Ok(vec![kline(start_ms, "10.5")])
            }
        }
        let history = fetch_history(
            &Dribble,
            "BTCUSDT",
            KLINE_INTERVAL_1M,
            ONE_MINUTE_MS,
            0,
            // A span far wider than the budget can walk one bucket at a time.
            ONE_MINUTE_MS * 100_000,
        )
        .await;
        assert!(!history.complete, "the shortfall is labelled, not hidden");
        assert_eq!(
            history.bars.len(),
            MAX_PAGES as usize,
            "one bar per page, and the budget is what stopped it"
        );
    }

    #[tokio::test]
    async fn a_venue_stuck_on_one_bucket_cannot_loop_forever() {
        // A source that always answers with the same bucket would re-request
        // the same page indefinitely without the no-progress guard.
        let source = ScriptedSource::new(vec![
            Ok(vec![kline(0, "10.5")]),
            Ok(vec![kline(0, "10.5")]),
            Ok(vec![kline(0, "10.5")]),
        ]);
        let history = fetch_history(
            &source,
            "BTCUSDT",
            KLINE_INTERVAL_1M,
            // An interval of zero cannot advance the cursor.
            0,
            0,
            10 * ONE_MINUTE_MS,
        )
        .await;
        assert_eq!(
            source.calls.borrow().len(),
            1,
            "it stopped instead of spinning"
        );
        assert!(history.bars.len() <= 1);
    }
}
