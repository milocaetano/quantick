//! Bounded diagnostic facts selected by protocol owners; only the driver logs.
use crate::{depth::BookStats, latency::LatencySample, map::MapStats, protocol, rates::RateStats};
use tracing::{debug, info, warn};

pub(super) enum Diagnostic {
    Hello(protocol::Hello),
    Capabilities {
        bridge: String,
        version: String,
        paging: bool,
        depth: bool,
    },
    Schema {
        schema: u32,
        bridge: String,
        version: String,
    },
    Symbol(String),
    Timeout {
        hello: bool,
        seconds: u64,
    },
    Socket {
        hello: bool,
        error: String,
    },
    Eof {
        hello: bool,
    },
    Oversized {
        hello: bool,
    },
    Utf8 {
        hello: bool,
        len: usize,
        total: u64,
    },
    Undecodable {
        hello: bool,
        error: protocol::ParseError,
        snippet: String,
        total: u64,
    },
    FirstMessage(protocol::BridgeMsg),
    Violation {
        message: &'static str,
        action: &'static str,
        bars: usize,
    },
    Bye(String),
    Heartbeat {
        seq_last: u64,
        ticks_sent: u64,
    },
    BackfillStart(Option<u64>),
    BackfillEnd(usize),
    BackfillDiscard,
    HistoryStart {
        count_hint: Option<u64>,
        unsolicited: bool,
    },
    HistoryTruncated(u64),
    Opening {
        trades: usize,
        remaining: Option<u64>,
    },
    HistoryDiscard(usize),
    HistoryEnd {
        trades: usize,
        exhausted: bool,
        scanned_to_ms: Option<i64>,
    },
    HistoryUnanswered(usize),
    RequestUnsupported(u64),
    Request {
        count: u64,
        before_ms: i64,
        before_utc_ms: i64,
    },
    RatesStart {
        interval_ms: i64,
        count_hint: Option<u64>,
    },
    RatesTruncated,
    RatesPartial {
        bars: usize,
        bridge: bool,
        clipped: bool,
    },
    RatesDiscard(usize),
    RatesSummary {
        stats: RateStats,
        interval_ms: i64,
    },
    MapSummary(MapStats),
    BookSummary(BookStats),
    Book(crate::depth::BookDiagnostic),
    MissingDepth,
    Lag {
        late: bool,
        sample: LatencySample,
    },
}

pub(super) fn log(diagnostic: Diagnostic, symbol: &str) {
    match diagnostic {
        Diagnostic::Hello(h) => {
            info!(target:"quantick::feed", schema_version=1_u8, event_code="MT5_HELLO_OK", bridge=%h.bridge, bridge_version=%h.bridge_version, symbol=%h.symbol, broker_symbol=%h.broker_symbol, digits=h.digits, server_utc_offset_s=h.server_utc_offset_s, tape=?h.tape, "bridge session established")
        }
        Diagnostic::Capabilities {
            bridge,
            version,
            paging,
            depth,
        } => {
            info!(target:"quantick::feed", schema_version=1_u8, event_code=if paging {"MT5_HISTORY_PAGING_AVAILABLE"} else {"MT5_HISTORY_PAGING_UNSUPPORTED"}, symbol, bridge, bridge_version=version, advice=if paging {"-"} else {"update the bridge (bridge/mt5/quantick_bridge.py) to page older ticks"}, "whether this session can be asked for ticks older than it sent");
            if depth {
                info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BOOK_AVAILABLE",symbol,"bridge declares Depth of Market support");
            } else {
                info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BOOK_UNSUPPORTED_BY_BRIDGE",symbol,action="trades_only","bridge declares no Depth of Market; the heatmap will stay empty (recompile bridge/mt5/QuantickBridge.mq5, or the terminal refused the DOM)");
            }
        }
        Diagnostic::Schema {
            schema,
            bridge,
            version,
        } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_SCHEMA_MISMATCH",bridge_schema=schema,our_schema=protocol::SCHEMA_VERSION,bridge,bridge_version=version,"bridge speaks a different protocol version; refusing")
        }
        Diagnostic::Symbol(got) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_SYMBOL_MISMATCH",expected=symbol,got,"bridge streams a different symbol than configured; refusing")
        }
        Diagnostic::Timeout { hello, seconds } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code=if hello {"MT5_HELLO_TIMEOUT"} else {"MT5_BRIDGE_SILENT"},timeout_s=seconds,"bridge read timed out")
        }
        Diagnostic::Socket { hello, error } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_SOCKET_ERROR",error, before_hello=hello,"socket error; dropping the session")
        }
        Diagnostic::Eof { hello } => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BRIDGE_EOF",before_hello=hello,"bridge closed the socket")
        }
        Diagnostic::Oversized { hello } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_LINE_TOO_LONG",max_bytes=super::super::MAX_LINE_BYTES as u64,before_hello=hello,"peer streamed an oversized line; dropping the session")
        }
        Diagnostic::Utf8 { hello, len, total } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_UNDECODABLE_LINE",error="invalid utf-8",line_bytes=len as u64,total_undecodable=total,before_hello=hello,"skipping an undecodable line")
        }
        Diagnostic::Undecodable {
            hello,
            error,
            snippet,
            total,
        } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_UNDECODABLE_LINE",error=%error,snippet,total_undecodable=total,before_hello=hello,"skipping an undecodable line")
        }
        Diagnostic::FirstMessage(got) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_PROTOCOL_VIOLATION",got=?got,"first message was not a hello; dropping the connection")
        }
        Diagnostic::Violation {
            message,
            action,
            bars,
        } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_PROTOCOL_VIOLATION",action,bars,"{message}")
        }
        Diagnostic::Bye(reason) => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BRIDGE_BYE",reason,"bridge said goodbye")
        }
        Diagnostic::Heartbeat {
            seq_last,
            ticks_sent,
        } => {
            debug!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HEARTBEAT",seq_last,ticks_sent,"bridge heartbeat")
        }
        Diagnostic::BackfillStart(count_hint) => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BACKFILL_START",count_hint=?count_hint,"bridge is sending history")
        }
        Diagnostic::BackfillEnd(trades) => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BACKFILL_END",trades,"history block complete")
        }
        Diagnostic::BackfillDiscard => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_PARTIAL_BACKFILL_DISCARDED","session ended mid-backfill; discarding the incomplete block")
        }
        Diagnostic::HistoryStart {
            count_hint,
            unsolicited,
        } => {
            if unsolicited {
                warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HISTORY_PAGE_UNSOLICITED",action="collect_and_discard","history_start with no request outstanding");
            }
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HISTORY_PAGE_START",count_hint=?count_hint,"bridge is sending a page of older ticks");
        }
        Diagnostic::HistoryTruncated(dropped) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HISTORY_PAGE_TRUNCATED",cap=super::history::MAX_TRADES_PER_PAGE as u64,dropped,"a page exceeded the per-block cap; the surplus was dropped")
        }
        Diagnostic::Opening { trades, remaining } => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_OPENING_PAGE",trades,remaining=?remaining,"a slice of the opening session is ready to prepend")
        }
        Diagnostic::HistoryDiscard(trades) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HISTORY_PAGE_UNSOLICITED",trades,action="discard","a page nobody asked for; discarding it rather than prepending")
        }
        Diagnostic::HistoryEnd {
            trades,
            exhausted,
            scanned_to_ms,
        } => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HISTORY_PAGE_END",trades,exhausted,scanned_to_ms=?scanned_to_ms,"page of older ticks complete")
        }
        Diagnostic::HistoryUnanswered(partial_trades) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_HISTORY_PAGE_UNANSWERED",partial_trades,action="answer_empty","session ended with a page outstanding; answering it empty")
        }
        Diagnostic::RequestUnsupported(requested) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_LOAD_OLDER_UNSUPPORTED",symbol,requested,action="answer_empty",advice="update the bridge (bridge/mt5/quantick_bridge.py) to page older ticks","this bridge session does not answer requests for older ticks")
        }
        Diagnostic::Request {
            count,
            before_ms,
            before_utc_ms,
        } => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_LOAD_OLDER_REQUESTED",symbol,requested=count,before_ms,before_utc_ms,"asking the terminal for ticks older than the chart's oldest")
        }
        Diagnostic::RatesStart {
            interval_ms,
            count_hint,
        } => {
            info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_RATES_START",interval_ms,count_hint=?count_hint,"bridge is sending historical candles")
        }
        Diagnostic::RatesTruncated => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_RATES_TRUNCATED",max_bars=super::super::blocks::MAX_BARS_PER_BLOCK as u64,action="keep_oldest_stop_absorbing","the candle block exceeded the cap; ignoring the rest of it")
        }
        Diagnostic::RatesPartial {
            bars,
            bridge,
            clipped,
        } => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_RATES_PARTIAL",symbol,bars,bridge_said_partial=bridge,clipped_here=clipped,"the candle block is short of what was asked for")
        }
        Diagnostic::RatesDiscard(bars) => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_PARTIAL_RATES_DISCARDED",bars,"session ended mid-candle-block; discarding it (the next session re-sends)")
        }
        Diagnostic::RatesSummary { stats, interval_ms } => stats.log_summary(symbol, interval_ms),
        Diagnostic::MapSummary(stats) => stats.log_summary(symbol),
        Diagnostic::BookSummary(stats) => stats.log_summary(symbol),
        Diagnostic::Book(fact) => fact.log(symbol),
        Diagnostic::MissingDepth => {
            warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_BOOK_UNSUPPORTED_BY_BRIDGE",symbol,action="report_disconnected","depth capture is on but this bridge sends no Depth of Market")
        }
        Diagnostic::Lag { late, sample } => {
            if late {
                warn!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_TAPE_LATE",symbol,arrival_lag_ms=sample.arrival_lag_ms,terminal_lag_ms=sample.terminal_lag_ms,terminal_lag_peak_ms=sample.terminal_lag_peak_ms,transport_lag_ms=sample.transport_lag_ms,prints=sample.prints,hop=sample.dominant().map(crate::latency::LatencyHop::label),"the tape is running behind; the hop field says where the time went");
            } else {
                info!(target:"quantick::feed",schema_version=1_u8,event_code="MT5_TAPE_CAUGHT_UP",symbol,arrival_lag_ms=sample.arrival_lag_ms,"the tape is current again");
            }
        }
    }
}
