//! The QuantickBridge wire protocol: newline-delimited JSON over a local TCP
//! socket.
//!
//! The stream is bridge → feed for everything the terminal *volunteers*, and
//! feed → bridge for the one thing it has to be *asked* for: ticks older than
//! what the chart already holds ([`FeedMsg::LoadOlder`]). A bridge that never
//! reads its socket is unaffected — it simply does not declare
//! [`Hello::history_paging`], and the feed never writes.
//!
//! This module is the pure, deterministic codec layer: one text line in, one
//! [`BridgeMsg`] out; one [`FeedMsg`] in, one text line out. No I/O, no clocks,
//! no state — fully unit-testable. The full protocol contract (message order,
//! field semantics, versioning) lives in `bridge/mt5/PROTOCOL.md`; this file is
//! its executable counterpart.
//!
//! Prices travel as strings (exact decimal digits, formatted by the bridge with
//! the symbol's `digits`), mirroring how Binance ships prices — no float
//! round-trips between the terminal and the engine.

use serde::{Deserialize, Serialize};

/// The protocol schema version this decoder understands. A bridge announcing a
/// different `schema` in its hello is refused — never half-parsed.
///
/// Depth of Market arrived as *optional* fields inside this same version
/// rather than as a version bump: a bridge that predates it simply declares no
/// [`book_levels`](Hello::book_levels), keeps streaming ticks, and the chart
/// says the book is unavailable. Forcing every existing terminal to recompile
/// its EA to keep charting trades would be a worse trade than that.
pub const SCHEMA_VERSION: u32 = 1;

/// MQL5 `MqlTick.flags` bits (shared vocabulary with the bridge). Real feeds
/// set bits beyond these (bit 1024 is routinely seen on B3); unknown bits are
/// ignored, never an error.
pub mod flags {
    /// Bid price changed.
    pub const BID: u32 = 2;
    /// Ask price changed.
    pub const ASK: u32 = 4;
    /// Last (trade) price changed.
    pub const LAST: u32 = 8;
    /// Volume changed.
    pub const VOLUME: u32 = 16;
    /// The trade's aggressor was a buyer.
    pub const BUY: u32 = 32;
    /// The trade's aggressor was a seller.
    pub const SELL: u32 = 64;
}

/// What the aggressor-side flag bits actually said, before any policy is
/// applied. [`Ambiguous`](AggressorFlag::Ambiguous) (both bits) and
/// [`Absent`](AggressorFlag::Absent) (neither) are real-world cases — some B3
/// brokers stamp every tick `BUY`, which is why the side policy is
/// configurable (see [`crate::map::SideMode`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggressorFlag {
    /// Only the BUY bit is set.
    Buy,
    /// Only the SELL bit is set.
    Sell,
    /// Both BUY and SELL bits are set.
    Ambiguous,
    /// Neither bit is set.
    Absent,
}

/// Decode the aggressor bits of a raw `flags` word.
#[must_use]
pub fn aggressor_from_flags(raw: u32) -> AggressorFlag {
    match (raw & flags::BUY != 0, raw & flags::SELL != 0) {
        (true, false) => AggressorFlag::Buy,
        (false, true) => AggressorFlag::Sell,
        (true, true) => AggressorFlag::Ambiguous,
        (false, false) => AggressorFlag::Absent,
    }
}

/// One message from the bridge. `type` tags the variant on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMsg {
    /// First message of every session: who is connected and how to interpret
    /// what follows.
    Hello(Hello),
    /// One MT5 tick (quote and/or trade — the flags say which).
    Tick(Tick),
    /// One complete image of the Depth of Market.
    Book(Book),
    /// Periodic liveness signal; also refreshes the server-time offset.
    Heartbeat(Heartbeat),
    /// The ticks that follow are history (from `CopyTicks`), not live.
    BackfillStart {
        /// How many ticks the bridge intends to send, if it knows.
        #[serde(default)]
        count_hint: Option<u64>,
    },
    /// End of the historical block; everything after is live.
    BackfillEnd {},
    /// A block of historical candles follows, one interval per bar.
    ///
    /// Sent after [`BackfillEnd`](Self::BackfillEnd) by a bridge whose hello
    /// declared [`rates`](Hello::rates). MetaTrader's transport is push-only —
    /// there is no back-channel to request anything — so the block arrives
    /// unasked and the feed holds it for whoever asks later.
    RatesStart {
        /// Milliseconds each candle covers (60 000 for the M1 series).
        interval_ms: i64,
        /// How many candles the bridge intends to send, if it knows.
        #[serde(default)]
        count_hint: Option<u64>,
    },
    /// One **batch** of candles, not one candle.
    ///
    /// Ninety days of M1 is tens of thousands of bars, and one line each would
    /// spend the whole block in newline framing. One line each is also not an
    /// option in the other direction: the whole block on a single line would
    /// blow the 64 KiB line cap and take the session down with it. So a `rate`
    /// message carries a bounded batch — see `bridge/mt5/PROTOCOL.md` for the
    /// size arithmetic behind the bound.
    Rate(RateChunk),
    /// End of the candle block.
    RatesEnd {
        /// Whether the bridge knows this block is short of what was asked.
        ///
        /// Set when paging failed after some pages had landed, or when the
        /// block was clipped to the bridge's own cap. **Absent means complete**
        /// — as in every bridge that predates the field — so the flag is
        /// additive within schema 1 like the rest.
        ///
        /// "Complete" is a claim about the *fetch*, not about the span: a
        /// contract younger than the request has fewer candles and is still
        /// complete. The two are different facts and the chart needs both.
        #[serde(default)]
        partial: bool,
    },
    /// A block of ticks *older* than the chart's oldest follows — the answer
    /// to a [`FeedMsg::LoadOlder`], and the one block the feed asked for.
    ///
    /// Deliberately not [`BackfillStart`](Self::BackfillStart). That block is
    /// the session's opening window and arrives unasked exactly once; this one
    /// arrives on demand, repeatedly, and lands *before* everything already
    /// charted rather than after it. A consumer that folded the two would
    /// prepend the opening window on a reconnect and append a paged block to
    /// the live tape — both wrong, and both invisible once the bars are drawn.
    HistoryStart {
        /// How many ticks the bridge intends to send, if it knows.
        #[serde(default)]
        count_hint: Option<u64>,
    },
    /// End of the paged block; the ticks between the markers are complete.
    HistoryEnd {
        /// Whether the terminal has nothing older left to serve.
        ///
        /// The honest end of the tape: set when the bridge searched back as far
        /// as the terminal keeps ticks and found none. **Absent means "there may
        /// be more"**, so a bridge predating the field simply never claims the
        /// end — the trader keeps a live button and gets empty blocks, which is
        /// the behaviour without this flag at all.
        ///
        /// Note what it is *not*: an empty block is not by itself the end. A
        /// terminal that failed a page, or one that was still downloading
        /// history, returns nothing and has plenty older — telling the trader
        /// "that is all there is" on the strength of one empty answer would
        /// retire the button over a transient.
        #[serde(default)]
        exhausted: bool,
    },
    /// The bridge is going away on purpose (EA removed, terminal closing).
    Bye {
        /// Why, e.g. `"deinit"`.
        reason: String,
    },
}

/// One message from the feed *to* the bridge — the whole of the back-channel.
///
/// Kept a separate enum from [`BridgeMsg`] rather than a shared one because
/// the two directions are not symmetric and never will be: the terminal
/// volunteers ticks, books, candles and heartbeats, and the feed asks for
/// exactly one thing. A shared enum would invite a consumer to match on
/// variants that can only ever arrive from the other side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedMsg {
    /// "Send me up to `count` ticks from before `before_ms`."
    ///
    /// Only ever written to a session whose hello declared
    /// [`Hello::history_paging`]; see that field for why silence is not
    /// permission.
    LoadOlder {
        /// How many ticks the chart wants. A bound, not a promise — the bridge
        /// sends what the terminal has and says so with
        /// [`BridgeMsg::HistoryEnd`].
        count: u64,
        /// Exclusive upper bound, in **server time** epoch milliseconds — the
        /// timestamp of the oldest tick the chart holds.
        ///
        /// Server time because every other timestamp in this protocol is, and
        /// a single message in the opposite direction is a poor place to
        /// introduce a second clock. The consumer names the point in UTC and
        /// [`crate::TickMapper::to_server_ms`] converts it, so the offset is
        /// read in the one place that tracks its heartbeat refreshes.
        before_ms: i64,
    },
}

/// Encode one [`FeedMsg`] as the NDJSON line to write to the socket, newline
/// included.
///
/// # Panics
///
/// Never in practice: the enum holds only integers and unit-like tags, so
/// serialization has no failure mode. The `expect` documents that rather than
/// pushing a `Result` no caller could act on.
#[must_use]
pub fn encode_line(msg: &FeedMsg) -> String {
    let mut line = serde_json::to_string(msg).expect("a FeedMsg is plain integers and a tag");
    line.push('\n');
    line
}

/// What a venue actually prints for a symbol — the honest answer to "is there
/// a tape here at all?".
///
/// Exchange-fed instruments print executed trades; broker-quoted CFDs do not.
/// The Tickmill index CFD `US500`, probed on 2026-07-29, sent 100 000
/// consecutive ticks with no LAST bit, no non-zero volume, and
/// `COPY_TICKS_TRADE` returned nothing at all: the broker quotes a price, it
/// does not redistribute the exchange's tape. Charting such a symbol as if it
/// had trades leaves the chart empty; inventing volume for it would be worse.
/// So the bridge declares which world the symbol lives in, and the mapper
/// follows ([`crate::map::TickMapper`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TapeKind {
    /// Executed trades are printed: ticks carry LAST and a real traded volume.
    #[default]
    Trades,
    /// Quotes only: bid and ask move, nothing is ever printed as traded.
    Quotes,
}

/// Session preamble. `server_utc_offset_s` is the key honesty field: MT5
/// stamps ticks in *server wall time encoded as epoch*, so true UTC requires
/// subtracting this offset. The bridge computes it live from
/// `TimeTradeServer() - TimeGMT()`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Hello {
    /// Protocol schema version; must equal [`SCHEMA_VERSION`].
    pub schema: u32,
    /// Bridge implementation name (e.g. `"quantick-mt5-bridge"`).
    pub bridge: String,
    /// Bridge implementation version.
    pub bridge_version: String,
    /// The symbol as configured on our side (e.g. `"WIN$N"`).
    pub symbol: String,
    /// The symbol the terminal actually streams (front-month contract).
    pub broker_symbol: String,
    /// Price decimal places; prices on the wire carry exactly this many.
    pub digits: u32,
    /// `server_time - utc`, in seconds (B3 brokers: −10800).
    pub server_utc_offset_s: i64,
    /// Smallest price increment the instrument trades in
    /// (`SYMBOL_TRADE_TICK_SIZE`), as exact decimal digits. B3's mini index
    /// moves in 5-point steps, so a consumer that renders liquidity on a finer
    /// grid would leave every other row permanently empty.
    ///
    /// Absent from bridges older than the Depth of Market support.
    #[serde(default)]
    pub tick_size: Option<String>,
    /// Maximum number of price levels per side this bridge can publish
    /// (`SYMBOL_TICKS_BOOKDEPTH`), when it publishes depth at all.
    ///
    /// `None` means this bridge streams no book: either it predates the
    /// feature or the terminal refused the DOM subscription. It is the honest
    /// signal for "the heatmap has no data here", never an assumed zero.
    #[serde(default)]
    pub book_levels: Option<u32>,
    /// What the venue prints for this symbol.
    ///
    /// Absent — as in every bridge that predates the field, and every fixture
    /// recorded before it — means [`TapeKind::Trades`]: the behaviour every
    /// exchange-fed session has always had, unchanged.
    #[serde(default)]
    pub tape: TapeKind,
    /// Whether this session sends a block of historical candles after its tick
    /// backfill (see [`BridgeMsg::RatesStart`]).
    ///
    /// `None` or `Some(false)` means no candles at all — an older bridge, the
    /// Expert Advisor (which does not implement them), or one told not to send
    /// them. Like [`book_levels`](Self::book_levels) this is the honest signal
    /// for "the time pane has no history here"; the feed reports the absence
    /// instead of waiting for a block that is never coming.
    #[serde(default)]
    pub rates: Option<bool>,
    /// Whether this session reads its socket and answers
    /// [`FeedMsg::LoadOlder`] with a [`BridgeMsg::HistoryStart`] block.
    ///
    /// `None` or `Some(false)` means the transport is push-only for this
    /// session — an older bridge, or one whose terminal cannot serve the
    /// request — and the feed must not write to the socket: a bridge that
    /// never reads would fill its receive buffer and eventually stall the
    /// terminal thread that writes ticks. Absent is therefore not "probably
    /// fine to try", it is a hard "do not".
    #[serde(default)]
    pub history_paging: Option<bool>,
}

/// One tick. `time_ms` is **server-time** epoch milliseconds (see [`Hello`]).
/// `seq` is assigned by the bridge, monotonically from 1 per session — it is a
/// *synthetic* id (MT5 has no exchange trade id), useful for transport-gap
/// detection but not stable across sessions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tick {
    /// Bridge-assigned session sequence number, from 1.
    pub seq: u64,
    /// Server-time epoch milliseconds (`MqlTick.time_msc`).
    pub time_ms: i64,
    /// Bid price, or `"0"` when the feed carries none (common on B3 history).
    pub bid: String,
    /// Ask price, or `"0"` when the feed carries none.
    pub ask: String,
    /// Last trade price, or `"0"` on quote-only ticks.
    pub last: String,
    /// Trade volume in contracts/lots; 0 on quote-only ticks.
    pub volume: u64,
    /// Raw `MqlTick.flags` word (see [`flags`]).
    pub flags: u32,
}

/// One price level on the wire: `["price", "quantity"]`.
///
/// A two-element array rather than an object because a book image repeats it
/// dozens of times per message, many times a second — the field names would be
/// most of the bytes. Both values are exact decimal strings, like tick prices.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WireLevel(
    /// Price.
    pub String,
    /// Total resting quantity at that price.
    pub String,
);

/// One complete image of the Depth of Market.
///
/// MT5 has no incremental book protocol: `OnBookEvent` fires and
/// `MarketBookGet` returns the whole visible DOM, with no update ids of any
/// kind. The bridge therefore sends images and the feed diffs them (see
/// [`crate::depth`]); `seq` exists only to detect images lost in transport,
/// and is numbered per session independently of tick `seq`.
///
/// Levels are the *limit* book only. MT5 also reports market-order rows
/// (`BOOK_TYPE_*_MARKET`), which carry no meaningful price and are excluded by
/// the bridge — they are not resting liquidity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Book {
    /// Bridge-assigned session image number, from 1.
    pub seq: u64,
    /// Server-time epoch milliseconds when the image was taken (see [`Hello`]).
    pub time_ms: i64,
    /// Resting buy levels, in whatever order the terminal returned them.
    pub bids: Vec<WireLevel>,
    /// Resting sell levels, in whatever order the terminal returned them.
    pub asks: Vec<WireLevel>,
}

/// Most candles one [`BridgeMsg::Rate`] line may carry.
///
/// Chosen against the 64 KiB line cap ([`crate::stream::MAX_LINE_BYTES`]),
/// which a longer line does not merely truncate — it ends the session. A row is
/// `[time,"o","h","l","c","v"]`: a 20-character timestamp plus five quoted
/// decimals of at most 22 characters each, with separators, is under 150 bytes
/// even for an instrument quoting eight integer and eight fractional digits.
/// Three hundred of those is ~44 KiB, leaving a third of the cap as headroom
/// for anything this arithmetic did not foresee. `rates_line_stays_under_the_
/// cap` in this module pins it.
pub const MAX_BARS_PER_RATE_LINE: usize = 300;

/// One batch of historical candles.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RateChunk {
    /// The candles in this batch, ascending by time.
    pub bars: Vec<RateRow>,
}

/// One candle on the wire: `[time_ms, open, high, low, close, volume]`.
///
/// A positional array rather than an object because this is the one message
/// that repeats tens of thousands of times in a session — field names would be
/// most of the bytes. Prices and volume travel as decimal **strings**, like
/// every other price in this protocol, so nothing round-trips through a float
/// between the terminal and the engine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RateRow(
    /// Bucket start, in **server time** epoch milliseconds (see [`Hello`]).
    pub i64,
    /// Open price.
    pub String,
    /// High price.
    pub String,
    /// Low price.
    pub String,
    /// Close price.
    pub String,
    /// Volume over the candle. What that counts depends on the venue: an
    /// exchange tape reports traded size, and a broker-quoted symbol — which
    /// prints nothing at all — reports its tick count, matching the one
    /// synthetic unit per tick the live mapper charts for the same instrument
    /// (see [`TapeKind`]).
    pub String,
);

/// Liveness + offset refresh. A silent bridge (no ticks, no heartbeats) is
/// treated as lost after a timeout.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Heartbeat {
    /// The last tick `seq` sent so far (0 if none yet).
    pub seq_last: u64,
    /// Server-time epoch milliseconds at send.
    pub time_ms: i64,
    /// Total ticks sent this session.
    pub ticks_sent: u64,
    /// Refreshed `server_time - utc` seconds, if the bridge recomputed it.
    #[serde(default)]
    pub server_utc_offset_s: Option<i64>,
}

/// Why a line could not become a [`BridgeMsg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not valid JSON, or JSON that doesn't match any message shape.
    Malformed(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Malformed(m) => write!(f, "malformed bridge message: {m}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Decode one NDJSON line into a [`BridgeMsg`].
///
/// # Errors
///
/// Returns [`ParseError::Malformed`] when the line is not valid JSON or does
/// not match any protocol message. Unknown *fields* inside a known message are
/// ignored (forward compatibility); an unknown `type` is an error.
pub fn parse_line(line: &str) -> Result<BridgeMsg, ParseError> {
    serde_json::from_str(line).map_err(|e| ParseError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paged_history_block_parses_from_its_two_markers() {
        // Verbatim from PROTOCOL.md. The markers bracket ordinary `tick`
        // messages — the block is distinguished by what encloses it, not by a
        // different tick shape, so the mapper stays one code path.
        let BridgeMsg::HistoryStart { count_hint } =
            parse_line(r#"{"type":"history_start","count_hint":2000}"#).expect("start parses")
        else {
            panic!("expected history_start");
        };
        assert_eq!(count_hint, Some(2000));

        let BridgeMsg::HistoryEnd { exhausted } =
            parse_line(r#"{"type":"history_end","exhausted":true}"#).expect("end parses")
        else {
            panic!("expected history_end");
        };
        assert!(exhausted, "the bridge said the terminal has nothing older");
    }

    #[test]
    fn a_paged_block_that_claims_nothing_is_neither_exhausted_nor_counted() {
        // Both fields are optional, so a bridge that knows neither how many it
        // will send nor whether it reached the end still frames a valid block.
        // The defaults are the cautious ones: no count promised, and the tape
        // is *not* declared finished — retiring the button on a bridge that
        // never said "that is all" would strand history the terminal has.
        let BridgeMsg::HistoryStart { count_hint } =
            parse_line(r#"{"type":"history_start"}"#).expect("bare start parses")
        else {
            panic!("expected history_start");
        };
        assert_eq!(count_hint, None);

        let BridgeMsg::HistoryEnd { exhausted } =
            parse_line(r#"{"type":"history_end"}"#).expect("bare end parses")
        else {
            panic!("expected history_end");
        };
        assert!(!exhausted, "absent must not read as 'nothing older exists'");
    }

    #[test]
    fn a_hello_without_history_paging_does_not_claim_it() {
        // The compatibility case that matters most: every bridge shipped before
        // the back-channel existed. It must still establish a session, and it
        // must not be written to — a bridge that never reads its socket would
        // have the feed's request sit in a receive buffer forever.
        let BridgeMsg::Hello(hello) = parse_line(
            r#"{"type":"hello","schema":1,"bridge":"quantick-mt5-bridge","bridge_version":"0.2.0","symbol":"WIN$N","broker_symbol":"WINQ26","digits":0,"server_utc_offset_s":-10800}"#,
        )
        .expect("a pre-paging hello parses")
        else {
            panic!("expected hello");
        };
        assert_eq!(hello.history_paging, None);

        let BridgeMsg::Hello(pager) = parse_line(
            r#"{"type":"hello","schema":1,"bridge":"quantick-mt5-bridge","bridge_version":"0.3.0","symbol":"WIN$N","broker_symbol":"WINQ26","digits":0,"server_utc_offset_s":-10800,"history_paging":true}"#,
        )
        .expect("a paging hello parses")
        else {
            panic!("expected hello");
        };
        assert_eq!(pager.history_paging, Some(true));
    }

    #[test]
    fn the_load_older_line_is_one_line_and_says_what_it_wants() {
        let line = encode_line(&FeedMsg::LoadOlder {
            count: 2000,
            before_ms: 1_784_824_300_802,
        });
        assert_eq!(
            line, "{\"type\":\"load_older\",\"count\":2000,\"before_ms\":1784824300802}\n",
            "this exact line is what PROTOCOL.md documents and the bridge parses"
        );
        // NDJSON framing: exactly one newline, and it terminates the line. A
        // second one would frame an empty message the bridge has to skip.
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn the_back_channel_type_tag_matches_the_python_bridge() {
        // Same treatment as the batch bound below: the wire name lives on both
        // sides, and a rename on one side would look like a bridge that ignores
        // the trader's clicks rather than like a protocol break.
        let bridge = include_str!("../../../bridge/mt5/quantick_bridge.py");
        let declared = bridge
            .lines()
            .find_map(|line| line.strip_prefix("LOAD_OLDER_TYPE = "))
            .expect("the bridge declares the message type at module level")
            .trim()
            .trim_matches('"');
        let line = encode_line(&FeedMsg::LoadOlder {
            count: 1,
            before_ms: 0,
        });
        assert!(
            line.contains(&format!("\"type\":\"{declared}\"")),
            "the bridge answers `{declared}`; this feed sends {line}"
        );
    }

    #[test]
    fn the_batch_bound_matches_the_python_bridge() {
        // The bound exists on both sides of the wire: Rust drops a session whose
        // line exceeds the cap, and the bridge is what decides how long a line
        // gets. A bridge that drifted upward would take down every session it
        // opened, and the failure would look like a network problem. So the two
        // constants are asserted equal, not assumed — the same treatment the
        // tracked-vs-embedded presets file gets in `bubble_presets.rs`.
        let bridge = include_str!("../../../bridge/mt5/quantick_bridge.py");
        let declared = bridge
            .lines()
            .find_map(|line| line.strip_prefix("MAX_BARS_PER_RATE_LINE = "))
            .expect("the bridge declares the bound at module level")
            .trim()
            .parse::<usize>()
            .expect("the bound is a plain integer");
        assert_eq!(
            declared, MAX_BARS_PER_RATE_LINE,
            "the bridge batches {declared} candles per line; this decoder is sized for              {MAX_BARS_PER_RATE_LINE}. A line over the cap ends the session it arrives on."
        );
    }

    #[test]
    fn rates_line_stays_under_the_cap() {
        // The bound that keeps a candle block from killing the session it
        // arrives on: a line over MAX_LINE_BYTES is not truncated, it ends the
        // connection. Built here at a width no real instrument reaches — eight
        // integer digits, eight fractional, on all five decimal fields, with a
        // timestamp far past any date a terminal will report.
        let wide = format!("\"{}\"", "9".repeat(8) + "." + &"9".repeat(8));
        let row = format!("[{},{wide},{wide},{wide},{wide},{wide}]", i64::MAX);
        let line = format!(
            "{{\"type\":\"rate\",\"bars\":[{}]}}\n",
            std::iter::repeat_n(row.as_str(), MAX_BARS_PER_RATE_LINE)
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(
            line.len() < crate::stream::MAX_LINE_BYTES,
            "a full batch must fit the line cap with room to spare: {} bytes of {}",
            line.len(),
            crate::stream::MAX_LINE_BYTES
        );
        // And it really is one line: a newline anywhere inside would reframe
        // the batch into fragments that parse as nothing.
        assert_eq!(line.matches('\n').count(), 1);

        // The shape the size was computed for is the shape that parses.
        let BridgeMsg::Rate(chunk) = parse_line(line.trim_end()).expect("a full batch parses")
        else {
            panic!("expected a rate batch");
        };
        assert_eq!(chunk.bars.len(), MAX_BARS_PER_RATE_LINE);
    }

    #[test]
    fn a_rates_block_parses_from_its_three_message_types() {
        let BridgeMsg::RatesStart {
            interval_ms,
            count_hint,
        } = parse_line(r#"{"type":"rates_start","interval_ms":60000,"count_hint":43200}"#).unwrap()
        else {
            panic!("expected rates_start");
        };
        assert_eq!(interval_ms, 60_000);
        assert_eq!(count_hint, Some(43_200));

        let BridgeMsg::Rate(chunk) = parse_line(
            r#"{"type":"rate","bars":[[1784824260000,"177790","177850","177780","177800","1234"]]}"#,
        )
        .unwrap() else {
            panic!("expected a rate batch");
        };
        assert_eq!(chunk.bars.len(), 1);
        assert_eq!(chunk.bars[0].0, 1_784_824_260_000);
        assert_eq!(chunk.bars[0].5, "1234");

        // Absent means complete, so a bridge predating the flag still parses.
        assert!(matches!(
            parse_line(r#"{"type":"rates_end"}"#).unwrap(),
            BridgeMsg::RatesEnd { partial: false }
        ));
        assert!(matches!(
            parse_line(r#"{"type":"rates_end","partial":true}"#).unwrap(),
            BridgeMsg::RatesEnd { partial: true }
        ));

        // A count hint is optional, like the backfill's.
        let BridgeMsg::RatesStart { count_hint, .. } =
            parse_line(r#"{"type":"rates_start","interval_ms":60000}"#).unwrap()
        else {
            panic!("expected rates_start");
        };
        assert_eq!(count_hint, None);
    }

    #[test]
    fn a_hello_without_rates_declares_none() {
        // Every bridge that predates candles, and the Expert Advisor, which
        // does not implement them. Absence must read as absence, not as a
        // block that is still coming.
        let line = r#"{"type":"hello","schema":1,"bridge":"b","bridge_version":"0","symbol":"WIN$N","broker_symbol":"WINQ26","digits":0,"server_utc_offset_s":-10800}"#;
        let BridgeMsg::Hello(h) = parse_line(line).unwrap() else {
            panic!("expected hello");
        };
        assert_eq!(h.rates, None);

        let with = line.replace("\"digits\":0", "\"rates\":true,\"digits\":0");
        let BridgeMsg::Hello(h) = parse_line(&with).unwrap() else {
            panic!("expected hello");
        };
        assert_eq!(h.rates, Some(true));
    }

    #[test]
    fn parses_a_real_recorded_hello() {
        // Verbatim first line of tests/fixtures/win_ticks.ndjson (real WIN$N
        // recording, 2026-07-23).
        let line = r#"{"type": "hello", "schema": 1, "bridge": "record_ticks.py", "bridge_version": "0.1.0", "symbol": "WIN$N", "broker_symbol": "WIN$N", "digits": 0, "server_utc_offset_s": -10800}"#;
        let BridgeMsg::Hello(h) = parse_line(line).unwrap() else {
            panic!("expected hello");
        };
        assert_eq!(h.schema, SCHEMA_VERSION);
        assert_eq!(h.symbol, "WIN$N");
        assert_eq!(h.server_utc_offset_s, -10_800);
    }

    #[test]
    fn parses_a_real_recorded_tick() {
        // Verbatim from the same recording: B3 history ticks carry bid/ask "0"
        // and the undocumented 1024 flag bit.
        let line = r#"{"type": "tick", "seq": 1, "time_ms": 1784824300802, "bid": "0", "ask": "0", "last": "177795", "volume": 3, "flags": 1080}"#;
        let BridgeMsg::Tick(t) = parse_line(line).unwrap() else {
            panic!("expected tick");
        };
        assert_eq!(t.seq, 1);
        assert_eq!(t.last, "177795");
        assert_eq!(t.volume, 3);
        assert_eq!(t.flags & flags::LAST, flags::LAST);
    }

    #[test]
    fn hello_depth_fields_are_optional_so_older_bridges_still_connect() {
        // A bridge predating Depth of Market: ticks keep flowing, the book is
        // honestly absent rather than assumed empty.
        let old = r#"{"type": "hello", "schema": 1, "bridge": "b", "bridge_version": "0",
            "symbol": "WIN$N", "broker_symbol": "WINQ26", "digits": 0,
            "server_utc_offset_s": -10800}"#;
        let BridgeMsg::Hello(h) = parse_line(old).unwrap() else {
            panic!("expected hello");
        };
        assert_eq!(h.tick_size, None);
        assert_eq!(h.book_levels, None);

        let with_book = r#"{"type": "hello", "schema": 1, "bridge": "b", "bridge_version": "0",
            "symbol": "WIN$N", "broker_symbol": "WINQ26", "digits": 0,
            "server_utc_offset_s": -10800, "tick_size": "5", "book_levels": 20}"#;
        let BridgeMsg::Hello(h) = parse_line(with_book).unwrap() else {
            panic!("expected hello");
        };
        assert_eq!(h.tick_size.as_deref(), Some("5"));
        assert_eq!(h.book_levels, Some(20));
    }

    #[test]
    fn a_book_image_parses_with_paired_level_arrays() {
        let line = r#"{"type":"book","seq":7,"time_ms":1784824300802,
            "bids":[["177795","3"],["177790","12"]],"asks":[["177800","5"]]}"#;
        let BridgeMsg::Book(book) = parse_line(line).unwrap() else {
            panic!("expected a book image");
        };
        assert_eq!(book.seq, 7);
        assert_eq!(book.time_ms, 1_784_824_300_802);
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.bids[0], WireLevel("177795".into(), "3".into()));
        assert_eq!(book.asks, vec![WireLevel("177800".into(), "5".into())]);

        // An empty side is legitimate (auction, halted book), not an error.
        let empty = r#"{"type":"book","seq":1,"time_ms":1,"bids":[],"asks":[]}"#;
        let BridgeMsg::Book(book) = parse_line(empty).unwrap() else {
            panic!("expected a book image");
        };
        assert!(book.bids.is_empty() && book.asks.is_empty());
    }

    #[test]
    fn heartbeat_offset_is_optional() {
        let with = r#"{"type":"heartbeat","seq_last":42,"time_ms":1,"ticks_sent":42,"server_utc_offset_s":-10800}"#;
        let without = r#"{"type":"heartbeat","seq_last":42,"time_ms":1,"ticks_sent":42}"#;
        let BridgeMsg::Heartbeat(a) = parse_line(with).unwrap() else {
            panic!()
        };
        let BridgeMsg::Heartbeat(b) = parse_line(without).unwrap() else {
            panic!()
        };
        assert_eq!(a.server_utc_offset_s, Some(-10_800));
        assert_eq!(b.server_utc_offset_s, None);
    }

    #[test]
    fn backfill_markers_and_bye_parse() {
        assert!(matches!(
            parse_line(r#"{"type":"backfill_start","count_hint":500}"#).unwrap(),
            BridgeMsg::BackfillStart {
                count_hint: Some(500)
            }
        ));
        assert!(matches!(
            parse_line(r#"{"type":"backfill_start"}"#).unwrap(),
            BridgeMsg::BackfillStart { count_hint: None }
        ));
        assert!(matches!(
            parse_line(r#"{"type":"backfill_end"}"#).unwrap(),
            BridgeMsg::BackfillEnd {}
        ));
        assert!(matches!(
            parse_line(r#"{"type":"bye","reason":"deinit"}"#).unwrap(),
            BridgeMsg::Bye { reason } if reason == "deinit"
        ));
    }

    #[test]
    fn unknown_fields_are_tolerated_unknown_type_is_not() {
        // Forward compatibility: a newer bridge may add fields.
        let extra = r#"{"type":"bye","reason":"x","new_field":123}"#;
        assert!(parse_line(extra).is_ok());
        // But a whole unknown message type is an error, not a silent skip —
        // the *caller* decides to skip (and count) it.
        assert!(parse_line(r#"{"type":"depth","levels":[]}"#).is_err());
        assert!(parse_line("{not json").is_err());
    }

    #[test]
    fn aggressor_flag_decoding_covers_all_cases() {
        assert_eq!(aggressor_from_flags(flags::BUY), AggressorFlag::Buy);
        assert_eq!(aggressor_from_flags(flags::SELL), AggressorFlag::Sell);
        assert_eq!(
            aggressor_from_flags(flags::BUY | flags::SELL),
            AggressorFlag::Ambiguous
        );
        assert_eq!(aggressor_from_flags(0), AggressorFlag::Absent);
        // The real-world 1080 word: LAST|VOLUME|BUY plus undocumented 1024.
        assert_eq!(aggressor_from_flags(1080), AggressorFlag::Buy);
    }
}
