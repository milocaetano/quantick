//! Venue-native candle history over Hyperliquid's WebSocket `post` method.
//!
//! Hyperliquid serves `candleSnapshot` through its info API, which is reachable
//! two ways: an HTTP endpoint, or the `post` method on a WebSocket. This crate
//! takes the WebSocket, deliberately — it has no HTTP client and gains none
//! here, so the whole venue keeps talking over one transport.
//!
//! The request runs on **its own short-lived connection**: connect, post, read
//! the reply, close. That makes correlation trivial (one socket carries exactly
//! one outstanding request, so an `id` mismatch is the only check needed) and,
//! more importantly, keeps the live trade session untouched. Candle history is
//! fetched when a pane opens; the trade stream is the thing that must not
//! stutter, and threading a request-reply state machine through its hot loop
//! would put a once-per-pane convenience in the path of every print.
//!
//! # What a Hyperliquid candle says
//!
//! It reports its own trade count (`n`) but no aggressor split, so `buy_volume`
//! and `sell_volume` each carry half the volume: [`Bar::volume`] stays exact and
//! [`Bar::delta`] is identically zero, meaning *not measured*. Only Binance
//! publishes the split, and only there does delta over candles mean anything.

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use quantick_engine::Bar;
use rust_decimal::Decimal;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

/// The interval quantick asks for; longer ones are folded locally.
pub const CANDLE_INTERVAL_1M: &str = "1m";

/// Milliseconds in the [`CANDLE_INTERVAL_1M`] bucket.
pub const ONE_MINUTE_MS: i64 = 60_000;

/// Candles requested per page.
///
/// Hyperliquid caps a `candleSnapshot` at 5 000 rows and answers a wider window
/// by truncating it, so asking for more would silently lose the tail. Ninety
/// days of one-minute candles is ~26 pages at this size.
pub const MAX_CANDLES_PER_REQUEST: i64 = 5_000;

/// How long one request-reply exchange may take before it is abandoned.
///
/// Generous: a 5 000-candle page is a large message and the venue is
/// occasionally slow. Bounded all the same, because a pane waiting on a socket
/// that will never answer is the failure this exists to prevent.
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the polite close frame may take before the socket is simply
/// dropped. The bars are already collected by then; nothing is riding on it.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Pages that may be fetched for one history request.
///
/// A week needs ~3, and no single request asks for more — a *load older*
/// repeats the span rather than extending it. The bound is what stops a venue
/// that keeps answering
/// with the same bucket from paging forever; it is deliberately far above any
/// legitimate need, so hitting it means something is wrong, not that the span
/// was ambitious.
const MAX_PAGES: u32 = 128;

/// Something went wrong fetching candles. Transport-shaped, like the trade
/// session's error next door.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandleError {
    /// Connect, read, or write failure.
    Transport(String),
    /// The venue answered, but not with a candle snapshot.
    Protocol(String),
    /// No reply arrived within [`REPLY_TIMEOUT`].
    Timeout,
}

impl std::fmt::Display for CandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "candle websocket error: {error}"),
            Self::Protocol(error) => write!(f, "candle protocol error: {error}"),
            Self::Timeout => f.write_str("candle request timed out"),
        }
    }
}

impl std::error::Error for CandleError {}

/// One candle as Hyperliquid sends it.
///
/// Prices and volume stay decimal **strings** until [`Candle::to_bar`], so a
/// malformed number is a mapping decision rather than an opaque deserialisation
/// failure — the same shape the Binance kline mapper uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candle {
    /// Bucket start, epoch ms (`t`).
    pub open_time: i64,
    /// Bucket end as the venue reports it, epoch ms (`T`).
    pub close_time: i64,
    /// Open price (`o`).
    pub open: String,
    /// High price (`h`).
    pub high: String,
    /// Low price (`l`).
    pub low: String,
    /// Close price (`c`).
    pub close: String,
    /// Volume over the bucket (`v`).
    pub volume: String,
    /// Trades in the bucket (`n`) — the venue's own count.
    pub trades: u64,
}

impl Candle {
    /// Map to an engine [`Bar`] covering `interval_ms`, or `None` when the row
    /// is empty or incoherent.
    ///
    /// `close_time` is computed from the bucket rather than copied, so every
    /// provider's candles carry one rule; the fixture test pins that
    /// Hyperliquid's own `T` agrees.
    #[must_use]
    pub fn to_bar(&self, interval_ms: i64) -> Option<Bar> {
        let open = positive(&self.open)?;
        let high = positive(&self.high)?;
        let low = positive(&self.low)?;
        let close = positive(&self.close)?;
        let volume = Decimal::from_str_exact(&self.volume).ok()?;

        // Empty buckets are dropped, per the engine's empty-interval rule.
        if volume <= Decimal::ZERO {
            return None;
        }
        if high < low || open > high || open < low || close > high || close < low {
            return None;
        }

        // No aggressor split exists here: half each side keeps the volume the
        // venue reported exact and leaves delta at zero, which reads as "not
        // measured" rather than "measured and balanced".
        let half = volume / Decimal::TWO;
        Some(Bar {
            open_time: self.open_time,
            close_time: self.open_time.saturating_add(interval_ms.saturating_sub(1)),
            open,
            high,
            low,
            close,
            buy_volume: half,
            sell_volume: half,
            trade_count: self.trades,
        })
    }
}

fn positive(text: &str) -> Option<Decimal> {
    let value = Decimal::from_str_exact(text).ok()?;
    (value > Decimal::ZERO).then_some(value)
}

/// Read one candle object.
fn value_to_candle(row: &Value) -> Option<Candle> {
    let text = |key: &str| row.get(key)?.as_str().map(str::to_owned);
    Some(Candle {
        open_time: row.get("t")?.as_i64()?,
        close_time: row.get("T")?.as_i64()?,
        open: text("o")?,
        high: text("h")?,
        low: text("l")?,
        close: text("c")?,
        volume: text("v")?,
        // Absent rather than fatal: the count is the one field a bar can carry
        // as "not reported" without misleading anyone.
        trades: row.get("n").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// What one frame off the candle socket turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandleReply {
    /// A `candleSnapshot` answer, with the id it answers.
    Snapshot {
        /// The request id this answers.
        id: u64,
        /// The candles, in whatever order the venue returned them.
        candles: Vec<Candle>,
    },
    /// The venue reported an error for a request.
    Failed {
        /// What it said.
        message: String,
    },
    /// Anything else on the socket — a pong, an unrelated channel. Ignored.
    Other,
}

/// Decode one WebSocket text frame from the candle socket.
///
/// Rows that are not candle objects are skipped rather than failing the frame,
/// matching how the rest of this crate treats a partially usable batch.
///
/// # Errors
///
/// Returns [`CandleError::Protocol`] when the frame is not JSON at all.
pub fn parse_candle_reply(text: &str) -> Result<CandleReply, CandleError> {
    let frame: Value =
        serde_json::from_str(text).map_err(|error| CandleError::Protocol(error.to_string()))?;
    match frame.get("channel").and_then(Value::as_str) {
        Some("post") => {}
        Some("error") => {
            return Ok(CandleReply::Failed {
                message: frame
                    .get("data")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified")
                    .to_owned(),
            });
        }
        _ => return Ok(CandleReply::Other),
    }
    let Some(data) = frame.get("data") else {
        return Ok(CandleReply::Other);
    };
    let Some(id) = data.get("id").and_then(Value::as_u64) else {
        return Ok(CandleReply::Other);
    };
    let response = data.get("response");
    // An info request that failed comes back as an error response rather than
    // on the error channel.
    if response.and_then(|r| r.get("type")).and_then(Value::as_str) == Some("error") {
        return Ok(CandleReply::Failed {
            message: response
                .and_then(|r| r.get("payload"))
                .map_or_else(|| "unspecified".to_owned(), ToString::to_string),
        });
    }
    let rows = response
        .and_then(|r| r.get("payload"))
        .and_then(|p| p.get("data"))
        .and_then(Value::as_array);
    let Some(rows) = rows else {
        return Ok(CandleReply::Other);
    };
    Ok(CandleReply::Snapshot {
        id,
        candles: rows.iter().filter_map(value_to_candle).collect(),
    })
}

/// Build the `post` frame asking for one page of candles.
#[must_use]
pub fn candle_request(id: u64, coin: &str, interval: &str, start_ms: i64, end_ms: i64) -> String {
    serde_json::json!({
        "method": "post",
        "id": id,
        "request": {
            "type": "info",
            "payload": {
                "type": "candleSnapshot",
                "req": {
                    "coin": coin,
                    "interval": interval,
                    "startTime": start_ms,
                    "endTime": end_ms,
                }
            }
        }
    })
    .to_string()
}

/// What a history fetch came back with, and whether it covers the whole span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandleHistory {
    /// Candles, ascending by `open_time`, deduplicated.
    pub bars: Vec<Bar>,
    /// Whether the requested span was covered end to end. False means the
    /// venue stopped answering partway and these are what arrived first.
    pub complete: bool,
}

/// Fetch one-minute candles covering `[from_ms, to_ms]` over a dedicated
/// short-lived WebSocket.
///
/// Pages forward, merging through a [`BTreeMap`] keyed by open time so repeated
/// buckets across page boundaries settle deterministically and no sort is
/// needed. Returns what it collected — labelled incomplete — when the venue
/// stops answering, rather than failing the whole span.
///
/// # Errors
///
/// Returns [`CandleError`] only when the connection could not be established at
/// all; a failure partway through comes back as an incomplete [`CandleHistory`].
pub async fn fetch_history(
    url: &str,
    coin: &str,
    interval: &str,
    interval_ms: i64,
    from_ms: i64,
    to_ms: i64,
) -> Result<CandleHistory, CandleError> {
    let coin = coin.to_uppercase();
    debug!(target: "quantick::feed", coin, url, from_ms, to_ms, "opening a candle socket");
    let (mut socket, _response) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|error| CandleError::Transport(error.to_string()))?;

    let mut bars: BTreeMap<i64, Bar> = BTreeMap::new();
    let mut cursor = from_ms;
    let mut pages = 0_u32;
    let mut dropped = 0_u64;
    let mut complete = true;

    while cursor <= to_ms && pages < MAX_PAGES {
        let id = u64::from(pages).saturating_add(1);
        // One page is at most MAX_CANDLES_PER_REQUEST buckets wide: a wider
        // window comes back truncated, and the tail would be lost in silence.
        let page_end = cursor
            .saturating_add(interval_ms.saturating_mul(MAX_CANDLES_PER_REQUEST))
            .min(to_ms);
        let candles = match request_page(&mut socket, id, &coin, interval, cursor, page_end).await {
            Ok(candles) => candles,
            Err(error) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "HYPERLIQUID_OHLCV_INCOMPLETE",
                    coin,
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
        if candles.is_empty() {
            break; // nothing further in this span
        }

        let mut newest = cursor;
        for candle in &candles {
            newest = newest.max(candle.open_time);
            match candle.to_bar(interval_ms) {
                Some(bar) => {
                    bars.insert(bar.open_time, bar);
                }
                None => dropped = dropped.saturating_add(1),
            }
        }
        // Step past the newest bucket seen; a venue returning nothing newer
        // would otherwise re-request the same window forever.
        let next = newest.saturating_add(interval_ms);
        if next <= cursor {
            break;
        }
        cursor = next;
    }
    if pages >= MAX_PAGES && cursor <= to_ms {
        complete = false;
    }

    // Best-effort and bounded: the answer is already in hand, and a venue that
    // will not take a close frame must cost the caller neither its history nor
    // an unbounded wait. The connection is dropped either way.
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, socket.close(None)).await;

    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "HYPERLIQUID_OHLCV_READY",
        coin,
        interval,
        pages,
        bars = bars.len(),
        dropped,
        complete,
        "candle history fetched"
    );
    Ok(CandleHistory {
        bars: bars.into_values().collect(),
        complete,
    })
}

/// Post one request and read frames until its reply arrives.
async fn request_page<S>(
    socket: &mut S,
    id: u64,
    coin: &str,
    interval: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<Candle>, CandleError>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
        + futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    socket
        .send(Message::Text(candle_request(
            id, coin, interval, start_ms, end_ms,
        )))
        .await
        .map_err(|error| CandleError::Transport(error.to_string()))?;

    let deadline = tokio::time::Instant::now() + REPLY_TIMEOUT;
    loop {
        let frame = tokio::time::timeout_at(deadline, socket.next())
            .await
            .map_err(|_| CandleError::Timeout)?;
        let Some(frame) = frame else {
            return Err(CandleError::Transport("socket closed".to_owned()));
        };
        match frame.map_err(|error| CandleError::Transport(error.to_string()))? {
            Message::Text(text) => match parse_candle_reply(text.as_str())? {
                // The socket carries one request at a time, so a reply for a
                // different id is a venue bug rather than a race — ignored
                // rather than mistaken for this page's answer.
                CandleReply::Snapshot {
                    id: replied,
                    candles,
                } if replied == id => {
                    return Ok(candles);
                }
                CandleReply::Snapshot { id: replied, .. } => {
                    debug!(target: "quantick::feed", coin, expected = id, replied, "ignoring a reply for another request");
                }
                CandleReply::Failed { message } => return Err(CandleError::Protocol(message)),
                CandleReply::Other => {}
            },
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| CandleError::Transport(error.to_string()))?;
            }
            Message::Close(_) => {
                return Err(CandleError::Transport("server closed".to_owned()));
            }
            Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A verbatim `candleSnapshot` reply shape, with one BTC one-minute candle.
    const REPLY: &str = r#"{"channel":"post","data":{"id":1,"response":{"type":"info","payload":{
        "type":"candleSnapshot","data":[
          {"t":1681923600000,"T":1681923659999,"s":"BTC","i":"1m",
           "o":"29258.0","c":"29309.0","h":"29320.0","l":"29250.0","v":"0.98639","n":15}
        ]}}}}"#;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str_exact(s).unwrap()
    }

    #[test]
    fn a_snapshot_reply_maps_to_bars() {
        let CandleReply::Snapshot { id, candles } =
            parse_candle_reply(REPLY).expect("the documented shape parses")
        else {
            panic!("expected a snapshot");
        };
        assert_eq!(id, 1);
        assert_eq!(candles.len(), 1);

        let bar = candles[0].to_bar(ONE_MINUTE_MS).expect("maps");
        assert_eq!(bar.open_time, 1_681_923_600_000);
        // The venue's own close time agrees with the bucket rule this crate
        // applies, which is what lets every provider carry the same one.
        assert_eq!(bar.close_time, candles[0].close_time);
        assert_eq!(bar.close_time, bar.open_time + ONE_MINUTE_MS - 1);
        assert_eq!(bar.open, dec("29258.0"));
        assert_eq!(bar.high, dec("29320.0"));
        assert_eq!(bar.low, dec("29250.0"));
        assert_eq!(bar.close, dec("29309.0"));
        assert_eq!(bar.trade_count, 15, "the venue counts its own trades");

        // No aggressor split exists here.
        assert_eq!(bar.volume(), dec("0.98639"), "the reported volume is exact");
        assert_eq!(
            bar.delta(),
            Decimal::ZERO,
            "delta is not measured, not zero"
        );
    }

    #[test]
    fn the_request_names_the_documented_fields() {
        let request = candle_request(7, "BTC", CANDLE_INTERVAL_1M, 1_000, 2_000);
        let value: Value = serde_json::from_str(&request).unwrap();
        assert_eq!(value["method"], "post");
        assert_eq!(value["id"], 7);
        assert_eq!(value["request"]["type"], "info");
        let req = &value["request"]["payload"]["req"];
        assert_eq!(value["request"]["payload"]["type"], "candleSnapshot");
        assert_eq!(req["coin"], "BTC");
        assert_eq!(req["interval"], "1m");
        assert_eq!(req["startTime"], 1_000);
        assert_eq!(req["endTime"], 2_000);
    }

    #[test]
    fn unrelated_frames_and_failures_are_told_apart() {
        // The trade channel's own traffic, a pong, and a subscription ack all
        // arrive on a socket like this one and must not look like an answer.
        for noise in [
            r#"{"channel":"pong"}"#,
            r#"{"channel":"subscriptionResponse","data":{}}"#,
            r#"{"channel":"trades","data":[]}"#,
        ] {
            assert_eq!(parse_candle_reply(noise).unwrap(), CandleReply::Other);
        }

        // A venue-level error, on either of the two shapes it takes.
        let CandleReply::Failed { message } =
            parse_candle_reply(r#"{"channel":"error","data":"bad request"}"#).unwrap()
        else {
            panic!("expected a failure");
        };
        assert_eq!(message, "bad request");
        assert!(matches!(
            parse_candle_reply(
                r#"{"channel":"post","data":{"id":1,"response":{"type":"error","payload":"nope"}}}"#
            )
            .unwrap(),
            CandleReply::Failed { .. }
        ));

        // Not JSON at all is the one thing that is an error to the caller.
        assert!(matches!(
            parse_candle_reply("not json"),
            Err(CandleError::Protocol(_))
        ));
    }

    #[test]
    fn an_empty_or_incoherent_candle_is_dropped() {
        let base = Candle {
            open_time: 0,
            close_time: 59_999,
            open: "10".into(),
            high: "12".into(),
            low: "9".into(),
            close: "11".into(),
            volume: "0".into(),
            trades: 0,
        };
        assert!(base.to_bar(ONE_MINUTE_MS).is_none(), "no volume");
        assert!(
            Candle {
                volume: "5".into(),
                high: "8".into(),
                ..base.clone()
            }
            .to_bar(ONE_MINUTE_MS)
            .is_none(),
            "high below the close it claims"
        );
        assert!(
            Candle {
                volume: "5".into(),
                low: "0".into(),
                ..base.clone()
            }
            .to_bar(ONE_MINUTE_MS)
            .is_none(),
            "a price no instrument trades at"
        );
        // A row missing the count still maps: absent is reported as 0.
        let row: Value =
            serde_json::from_str(r#"{"t":0,"T":59999,"o":"10","c":"11","h":"12","l":"9","v":"5"}"#)
                .unwrap();
        let candle = value_to_candle(&row).expect("maps without n");
        assert_eq!(candle.trades, 0);
        assert!(candle.to_bar(ONE_MINUTE_MS).is_some());
    }

    #[test]
    fn a_page_is_never_wider_than_the_venues_row_cap() {
        // Asking for more than the cap comes back truncated, and the tail would
        // vanish without a word. The window is sized to the cap instead.
        let widest = ONE_MINUTE_MS * MAX_CANDLES_PER_REQUEST;
        assert_eq!(
            widest / ONE_MINUTE_MS,
            MAX_CANDLES_PER_REQUEST,
            "one page covers exactly the cap in buckets"
        );
        // And a deep reach still fits inside the page budget with room over.
        let span = 90 * 24 * 60 * 60 * 1_000_i64;
        let needed = span / widest + 1;
        assert!(
            needed < i64::from(MAX_PAGES),
            "a ninety-day reach needs {needed} pages of {MAX_PAGES}"
        );
    }
}
