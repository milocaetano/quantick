//! What the server says to its consumer, and how a session ends.
//!
//! The vocabulary of the feed: where it stands ([`Mt5Status`]), what it has to
//! report ([`Mt5Event`]), what can kill it ([`Mt5Error`]) and why one bridge
//! connection stopped ([`ConnEnd`]).

use quantick_engine::{Bar, Trade};
use quantick_orderbook::DepthEvent;

use crate::latency::LatencySample;
use crate::protocol::TapeKind;

/// Where the feed currently stands, for honest labelling in UI and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mt5Status {
    /// Listening; no bridge connected. The chart should say so, not pretend.
    Waiting {
        /// The actual bound address (resolves `:0` in tests).
        addr: String,
    },
    /// A bridge said hello and is streaming (or about to).
    Connected {
        /// Symbol as configured.
        symbol: String,
        /// The front-month contract the terminal actually streams.
        broker_symbol: String,
        /// What this venue prints for the symbol, as the hello declared it.
        ///
        /// Only a live session knows: the same terminal streams an exchange
        /// contract with a real tape and a broker CFD with none, and the
        /// difference decides what the chart may honestly offer.
        tape: TapeKind,
        /// Levels per side this session can publish, or `None` when it sends no
        /// depth at all (the terminal refused the DOM, or the symbol has none).
        book_levels: Option<u32>,
        /// Whether this session sends a historical candle block.
        ///
        /// Per-session like the two above: the Expert Advisor sends none, and
        /// so does any bridge older than the feature. A consumer waiting on
        /// candles needs to hear that now rather than after a timeout.
        rates: bool,
        /// Whether this session answers requests for older ticks.
        ///
        /// The one capability on this list the consumer can *act* on rather
        /// than merely display: the chart's "load older" button is enabled by
        /// this and by nothing else. Per-session for the same reason as the
        /// rest — the same quantick build talks to a bridge that pages and to
        /// one that does not, and the provider's name cannot tell them apart.
        history_paging: bool,
    },
    /// The bridge went away; the server is looping back to waiting.
    Lost {
        /// Why, e.g. `"bye: deinit"`, `"silent"`, `"eof"`.
        reason: String,
    },
}

/// One message from the bridge server to its consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mt5Event {
    /// A connection-state transition.
    Status(Mt5Status),
    /// One complete historical block (may be empty), already mapped. Sent
    /// exactly once per `backfill_start`/`backfill_end` pair.
    Backfilled(Vec<Trade>),
    /// One live trade.
    Live(Trade),
    /// Where the tape's delay is being spent, measured at the socket.
    ///
    /// Sent at a bounded rate — at most once every
    /// [`SAMPLE_EVERY_PRINTS`](crate::latency::SAMPLE_EVERY_PRINTS) prints, and
    /// once per heartbeat so a thin tape still reports. Never once per print:
    /// the reading costs a system clock read, and a per-print clock read on a
    /// busy tape is the kind of cost this whole change exists to remove.
    ///
    /// The figures are taken where the line is read off the socket, so they
    /// account for the chain up to this crate and no further. What a consumer
    /// then spends queueing and drawing is its own to measure — and the
    /// difference between its arrival figure and this one is exactly that.
    Latency(LatencySample),
    /// One order-book event, in the provider-neutral depth vocabulary.
    ///
    /// Only produced while [`BookCaptureSwitch`] is enabled and the bridge
    /// declares depth support.
    Depth(DepthEvent),
    /// Another bridge dialed this port while a session was being served, and
    /// was refused.
    ///
    /// The log has said this since the refusal existed; the chart has not, and
    /// the chart is where someone is looking at a window that says "waiting for
    /// the bridge" while the answer sits in a file. Carries the same diagnosis
    /// the log gets, so the consumer can put it in front of a person.
    SessionBusy {
        /// Address of the connection that was turned away.
        peer: String,
        /// The symbol its hello declared, when it sent one.
        peer_symbol: Option<String>,
        /// Stable classification: `same_symbol`, `other_symbol`, or
        /// `unidentified`.
        diagnosis: &'static str,
        /// What to do about it, in words.
        advice: &'static str,
    },
    /// The session's historical candle block, complete and already mapped.
    ///
    /// Sent exactly once per `rates_start`/`rates_end` pair, ascending by
    /// `open_time` and deduplicated. A block that never finished is discarded
    /// rather than half-delivered — a candle series with a hole in the middle
    /// reads as a market that stopped trading.
    Rates {
        /// Milliseconds each bar covers, as the block declared.
        interval_ms: i64,
        /// The candles, ascending by `open_time`.
        bars: Vec<Bar>,
        /// Whether the block is known to be short of what was asked for —
        /// the bridge said so, or this decoder clipped it. See
        /// [`protocol::BridgeMsg::RatesEnd`].
        partial: bool,
    },
    /// The answer to one [`HistoryPager::request`]: ticks older than the cursor
    /// the consumer asked from, already mapped and ascending by time.
    ///
    /// **Exactly one per request, always** — including when the terminal had
    /// nothing, when the bridge cannot page, and when the session died before
    /// answering. A consumer shows a spinner while it waits, and a request that
    /// can go unanswered is a spinner that never stops.
    HistoryPage {
        /// The older trades, ascending. Empty is a legitimate answer.
        trades: Vec<Trade>,
        /// Whether the terminal reports nothing older left — the end of the
        /// tape, not merely the end of this block. See
        /// [`protocol::BridgeMsg::HistoryEnd`] for why an empty block alone
        /// does not mean this.
        exhausted: bool,
        /// How far back the search actually reached, in **UTC** milliseconds,
        /// when the bridge said.
        ///
        /// Distinct from the oldest trade in `trades`, and the difference is
        /// what keeps paging moving over stretches that map to nothing — a
        /// pre-open session of quote-only ticks, or a window that held none at
        /// all. See [`protocol::BridgeMsg::HistoryEnd::scanned_to_ms`].
        scanned_to_utc_ms: Option<i64>,
    },
    /// A slice of the *opening* history: older than everything sent so far, and
    /// asked for by nobody.
    ///
    /// The bridge opens a chart on the whole trading session, which on a liquid
    /// contract is far too much to arrive in one block. The newest slice goes
    /// out as [`Mt5Event::Backfilled`] so there is something to look at within
    /// a second, and the rest of the session follows in these, newest-first,
    /// while the live tape keeps flowing between them.
    ///
    /// Deliberately **not** a [`Mt5Event::HistoryPage`]. That event's contract
    /// is one reply per request and a consumer's spinner depends on it; these
    /// answer no request and must settle no debt, or a click made during the
    /// fill would be answered by a slice that has nothing to do with it.
    OpeningPage {
        /// The older trades, ascending. Empty is legitimate: a slice can map to
        /// no trades at all.
        trades: Vec<Trade>,
        /// Slices still to come after this one, when the bridge said. For
        /// showing progress; never a promise, since a session can end mid-fill.
        remaining: Option<u64>,
    },
}

/// A fatal server error (the non-fatal ones are events/logs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mt5Error {
    /// Could not bind the listen address (typically: port already in use by
    /// another quantick instance).
    Bind {
        /// The address we tried.
        addr: String,
        /// The OS error text.
        message: String,
    },
}

impl std::fmt::Display for Mt5Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mt5Error::Bind { addr, message } => {
                write!(f, "cannot listen on {addr} for the MT5 bridge: {message}")
            }
        }
    }
}

impl std::error::Error for Mt5Error {}

/// Why one bridge connection ended.
pub(super) enum ConnEnd {
    /// The consumer dropped the event channel: shut the server down.
    UiGone,
    /// The bridge went away (reason for the status event); keep listening.
    BridgeGone(String),
}
