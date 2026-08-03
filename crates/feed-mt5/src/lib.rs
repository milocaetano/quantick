//! quantick-feed-mt5 — MetaTrader 5 tick feed over the local QuantickBridge
//! socket.
//!
//! MT5 offers no public REST/WebSocket API: market data lives inside the
//! terminal. A *bridge* dials out to this crate's TCP listener with
//! newline-delimited JSON; this crate decodes, validates and maps what arrives
//! into engine [`quantick_engine::Trade`]s and [`quantick_orderbook::DepthEvent`]s.
//! No credentials exist anywhere in this path — the terminal is already
//! logged in, and the socket never leaves localhost.
//!
//! Two bridges ship, and this crate cannot tell them apart — the protocol is
//! the contract, not the implementation:
//!
//! - `bridge/mt5/quantick_bridge.py` — attaches to the running terminal
//!   through the official Python package and polls it. One command, no setup.
//! - `bridge/mt5/QuantickBridge.mq5` — MQL5, runs on a chart inside the
//!   terminal, so `OnBookEvent` pushes every book change at the instant it
//!   happens. Costs a compile-and-attach; buys event-accurate depth.
//!
//! ```text
//! MT5 terminal ── QuantickBridge.mq5 ──▶ TCP 127.0.0.1 ──▶ [stream] ──▶ [protocol] ─┬─▶ [map]   ──▶ Trade
//!                                                                                   └─▶ [depth] ──▶ DepthEvent
//! ```
//!
//! # What MT5 does not provide, and what this crate does about it
//!
//! | Missing | Policy | Where |
//! |---|---|---|
//! | Exchange trade id | synthetic per-session `seq` (gap-detectable) | [`map`], [`session`] |
//! | UTC timestamps | bridge declares `server_utc_offset_s`; converted + refreshed | [`protocol`], [`map`] |
//! | Reliable aggressor side | configurable [`map::SideMode`]; tick rule by default; drops counted | [`map`] |
//! | Incremental order-book updates | terminal republishes whole DOM images; diffed into snapshot + deltas | [`depth`] |
//! | Book update ids | synthetic, monotonic, generation-scoped | [`depth`], `quantick_orderbook::SnapshotDiffer` |
//! | Full book depth | coverage always labelled `Limited`, never `Full` | [`depth`] |
//! | Older-history paging | unsupported; requests answered empty + logged | app layer |
//! | A tape at all (broker CFDs print none) | bridge declares [`protocol::TapeKind`]; quotes chart as one-unit synthetic prints, counted apart | [`map`] |
//!
//! # AI-first diagnosis
//!
//! Every transition logs a structured event with an `event_code`. Reading the
//! JSON logs (`QUANTICK_LOG_FORMAT=json`) is the supported way — for humans
//! and AIs alike — to answer "why is nothing charting?":
//!
//! | event_code | Meaning | Likely cause / fix |
//! |---|---|---|
//! | `MT5_LISTENING` | feed is up, waiting | attach the EA to a chart |
//! | `MT5_BIND_FAILED` | can't open the port | another quantick, or another symbol on the same port; see `listen_addr` / `[metatrader.ports]` |
//! | `MT5_ACCEPT_FAILED` | one accept failed; still listening | transient OS error |
//! | `MT5_BRIDGE_CONNECTED` | socket open, no hello yet | — |
//! | `MT5_SESSION_BUSY` | a second bridge dialed a port already serving one | read `diagnosis`: `same_symbol` = two EAs streaming one symbol, remove a chart; `other_symbol` = give `peer_symbol` its own port; `unidentified` = nothing named itself, start from `peer` |
//! | `MT5_HELLO_TIMEOUT` | connected but silent | wrong client dialed the port |
//! | `MT5_SCHEMA_MISMATCH` | bridge too old/new | recompile the EA from this repo |
//! | `MT5_SYMBOL_MISMATCH` | EA runs on another symbol's chart | attach it to the configured symbol |
//! | `MT5_HELLO_OK` | session established | — |
//! | `MT5_BACKFILL_START`/`_END` | history block | — |
//! | `MT5_PARTIAL_BACKFILL_DISCARDED` | session died mid-history | next connection re-sends it |
//! | `MT5_HEARTBEAT` | liveness + fresh offset (debug level) | — |
//! | `MT5_SEQ_GAP` | ticks lost in transport | terminal overloaded? check EA logs |
//! | `MT5_SEQ_NOT_MONOTONIC` | seq went backwards/repeated | EA bug or duplicated stream |
//! | `MT5_BRIDGE_SILENT` | no data within timeout | terminal closed / market halted / EA removed |
//! | `MT5_BRIDGE_EOF` | peer closed the socket | terminal closing or EA reload |
//! | `MT5_BRIDGE_BYE` | clean shutdown | EA detached or terminal closing |
//! | `MT5_BRIDGE_LOST` | session over; back to waiting | see the paired reason field |
//! | `MT5_SOCKET_ERROR` | read failed mid-session | network stack; session restarts |
//! | `MT5_PROTOCOL_VIOLATION` | message out of place | bridge/feed version skew |
//! | `MT5_UNDECODABLE_LINE` | garbage on the wire (skipped, counted) | bridge/feed version skew |
//! | `MT5_LINE_TOO_LONG` | line over 64 KiB; session dropped | not the bridge talking to us |
//! | `MT5_MAP_SUMMARY` | per-session mapping ledger | audit drops & side sources here |
//! | `MT5_BOOK_AVAILABLE` | bridge declares a DOM | — |
//! | `MT5_BOOK_UNSUPPORTED_BY_BRIDGE` | no DOM on this session | recompile the EA, or the symbol has no book |
//! | `MT5_BOOK_SYNCHRONIZED` | first DOM image accepted; heatmap is live | — |
//! | `MT5_BOOK_CROSSED` | crossed image rejected (first occurrence) | normal during B3's pre-open auction |
//! | `MT5_BOOK_MALFORMED` | unreadable level; whole image rejected | bridge/feed version skew |
//! | `MT5_BOOK_TIME_BACKWARDS` | image timestamp went back; held | terminal clock jumped |
//! | `MT5_BOOK_SUMMARY` | per-session book ledger | audit image/skip counts here |
//!
//! A `MT5_MAP_SUMMARY` where `side_from_flag` is ~100% buys would reveal the
//! broken-flags pathology this crate's tick-rule default exists for (observed
//! empirically on B3 `WIN$N`, 2026-07-23: every tick flagged BUY, `flags =
//! 1080`). Re-verify any broker with `tools/mt5/record_ticks.py`.

pub mod bridge_log;
pub mod depth;
pub mod map;
pub mod protocol;
pub mod session;
pub mod stream;

pub use bridge_log::{BridgeReport, BridgeSeverity, report_for_line};
pub use depth::{BookMapper, BookStats};
pub use map::{DropReason, MapOutcome, MapStats, SideMode, SideSource, TickMapper};
pub use protocol::{BridgeMsg, Hello, ParseError, SCHEMA_VERSION, TapeKind, Tick, parse_line};
pub use session::{SeqAnomaly, SeqTracker};
pub use stream::{
    BookCaptureSwitch, BoundedLine, BoundedLineReader, DEFAULT_LISTEN_ADDR, MAX_LINE_BYTES,
    Mt5Error, Mt5Event, Mt5Status, ServerConfig, run_bridge_server,
};
