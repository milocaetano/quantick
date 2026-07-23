//! quantick-feed-mt5 — MetaTrader 5 tick feed over the local QuantickBridge
//! socket.
//!
//! MT5 offers no public REST/WebSocket API: market data lives inside the
//! terminal. The bridge (`bridge/mt5/QuantickBridge.mq5`, MQL5 — the
//! terminal's C++-family language) runs on a chart and dials out to this
//! crate's TCP listener with newline-delimited JSON; this crate decodes,
//! validates and maps those ticks into engine [`quantick_engine::Trade`]s.
//! No credentials exist anywhere in this path — the terminal is already
//! logged in, and the socket never leaves localhost.
//!
//! ```text
//! MT5 terminal ── QuantickBridge.mq5 ──▶ TCP 127.0.0.1 ──▶ [stream] ──▶ [protocol] ──▶ [map] ──▶ Trade
//! ```
//!
//! # What MT5 does not provide, and what this crate does about it
//!
//! | Missing | Policy | Where |
//! |---|---|---|
//! | Exchange trade id | synthetic per-session `seq` (gap-detectable) | [`map`], [`session`] |
//! | UTC timestamps | bridge declares `server_utc_offset_s`; converted + refreshed | [`protocol`], [`map`] |
//! | Reliable aggressor side | configurable [`map::SideMode`]; tick rule by default; drops counted | [`map`] |
//! | Order-book depth | not implemented; the app never enables the heatmap for MT5 | app layer |
//! | Older-history paging | unsupported; requests answered empty + logged | app layer |
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
//! | `MT5_BIND_FAILED` | can't open the port | another quantick running; change `listen_addr` |
//! | `MT5_BRIDGE_CONNECTED` | socket open, no hello yet | — |
//! | `MT5_HELLO_TIMEOUT` | connected but silent | wrong client dialed the port |
//! | `MT5_SCHEMA_MISMATCH` | bridge too old/new | recompile the EA from this repo |
//! | `MT5_SYMBOL_MISMATCH` | EA runs on another symbol's chart | attach it to the configured symbol |
//! | `MT5_HELLO_OK` | session established | — |
//! | `MT5_BACKFILL_START`/`_END` | history block | — |
//! | `MT5_SEQ_GAP` | ticks lost in transport | terminal overloaded? check EA logs |
//! | `MT5_BRIDGE_SILENT` | no data within timeout | terminal closed / market halted / EA removed |
//! | `MT5_BRIDGE_BYE` | clean shutdown | EA detached or terminal closing |
//! | `MT5_UNDECODABLE_LINE` | garbage on the wire | bridge/feed version skew |
//! | `MT5_MAP_SUMMARY` | per-session mapping ledger | audit drops & side sources here |
//!
//! A `MT5_MAP_SUMMARY` where `side_from_flag` is ~100% buys would reveal the
//! broken-flags pathology this crate's tick-rule default exists for (observed
//! empirically on B3 `WIN$N`, 2026-07-23: every tick flagged BUY, `flags =
//! 1080`). Re-verify any broker with `tools/mt5/record_ticks.py`.

pub mod map;
pub mod protocol;
pub mod session;
pub mod stream;

pub use map::{DropReason, MapOutcome, MapStats, SideMode, SideSource, TickMapper};
pub use protocol::{BridgeMsg, Hello, ParseError, SCHEMA_VERSION, Tick, parse_line};
pub use session::{SeqAnomaly, SeqTracker};
pub use stream::{
    DEFAULT_LISTEN_ADDR, Mt5Error, Mt5Event, Mt5Status, ServerConfig, run_bridge_server,
};
