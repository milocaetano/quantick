//! Binance backend for the feed bridge.
//!
//! A background thread runs a tokio runtime that first backfills recent history
//! over REST, then streams live trades (with reconnect + gap detection). Both
//! flow to the UI as [`FeedEvent`]s; the UI's [`FeedCommand`]s (e.g. "load older
//! history") are serviced between live trades.

use std::ops::ControlFlow;

use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tracing::{error, info, warn};

use quantick_engine::Trade;
use quantick_feed_binance::{
    BINANCE_WS_BASE, Backoff, BinanceHttp, BinanceKlineHttp, KLINE_INTERVAL_1M, ONE_MINUTE_MS,
    agg_trade_url, backfill, backfill_before,
    depth::{
        BinanceDepthHttp, DepthEvent, DepthSessionConfig, MAX_DEPTH_LIMIT, run_depth_with_reconnect,
    },
    fetch_history, run_with_reconnect,
};

use super::{
    FeedCommand, FeedEvent, FeedHandle, FeedNotice, connection_notice, initial_backfill_target,
};
use crate::config::ProviderKind;
use crate::venue_loop::{CommandPlan, Slot, ohlcv_reply_frees_slot, plan_command, send_or_break};

/// Default number of REST depth levels requested per side.
const DEFAULT_BOOK_DEPTH: u16 = 1_000;

/// Depth events are independent from the established trade channel.
const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;
const NOTICE_CHANNEL_CAPACITY: usize = 32;

/// Transport inputs kept separate so socket fixtures exercise the real host.
pub(crate) struct BinanceSource {
    pub http: BinanceHttp,
    pub url: String,
    pub backoff: Backoff,
}

/// Start the Binance feed for `symbol` on a background thread.
#[must_use]
pub fn spawn(symbol: &str) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
    let (book_tx, book_rx) = mpsc::channel(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let symbol = symbol.to_string();
    let source = BinanceSource {
        http: BinanceHttp::new(),
        url: agg_trade_url(BINANCE_WS_BASE, &symbol),
        // Fixed seed keeps a single desktop client's retry reproducible.
        backoff: Backoff::for_feed(0x9E37_79B9_7F4A_7C15),
    };
    std::thread::Builder::new()
        .name("quantick-feed".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build feed runtime");
            runtime.block_on(feed_task(symbol, tx, book_tx, notice_tx, cmd_rx, source));
        })
        .expect("spawn feed thread");
    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: notice_rx,
        // Every aggTrade is an execution carrying its real size, and the venue
        // answers the same for every symbol it lists — nothing to narrow later.
        capabilities: super::fixed_capabilities(ProviderKind::Binance.capabilities()),
        // A web-socket venue stamps a trade and quantick reads it: there is no
        // hop between those two to attribute a delay to.
        latency: super::unsplit_latency(),
        commands: cmd_tx,
        replay: None,
    }
}

pub(crate) async fn feed_task(
    symbol: String,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
    source: BinanceSource,
) {
    let BinanceSource { http, url, backoff } = source;
    // 1. Backfill recent history so the chart opens populated. Remember the
    //    earliest agg_id so we can page further back on demand.
    let ControlFlow::Continue(earliest_id) = initial_backfill(&http, &symbol, &tx).await else {
        return; // UI gone
    };

    // 2. Stream live trades on top, reconnecting as needed. run_with_reconnect
    //    speaks Trade; the loop below tags each as a live FeedEvent and, in the
    //    same select, services UI commands between trades.
    let (live_tx, mut live_rx) = mpsc::channel::<Trade>(4096);
    let (connected_tx, mut connected_rx) = watch::channel(false);
    let reconnect = tokio::spawn(async move {
        run_with_reconnect(&url, &live_tx, &connected_tx, backoff).await;
    });
    // Candle history runs off this loop, not inside it. A week is ~11
    // sequential pages and a trader paging back through a quarter asks for
    // thirteen such runs — seconds on a good day, far longer against a venue
    // that is throttling — and awaiting that in a command arm stops `live_rx`
    // from being polled for the duration: the trade channel fills, the
    // websocket read loop behind it stalls, and pongs stop going out. The task
    // sends its result back here and the loop keeps turning meanwhile.
    let (ohlcv_tx, mut ohlcv_rx) = mpsc::channel::<OhlcvReply>(1);
    let mut feed = BinanceLoop {
        // Candles come off a different endpoint with a different paging rule,
        // so they get their own client rather than overloading the aggTrade one.
        klines: BinanceKlineHttp::new(),
        http,
        symbol,
        tx,
        book_tx,
        notice_tx,
        earliest_id,
        snapshot_limit: initial_book_depth(),
        book_capture: None,
        ohlcv_tx,
        ohlcv_task: None,
        continuity: crate::continuity::BinanceContinuity::default(),
        ever_connected: false,
    };

    loop {
        let flow = tokio::select! {
            Some((bars, slice)) = ohlcv_rx.recv() => feed.on_ohlcv_reply(bars, slice).await,
            maybe_trade = live_rx.recv() => match maybe_trade {
                Some(trade) => feed.on_trade(trade).await,
                None => ControlFlow::Break(()), // stream ended
            },
            changed = connected_rx.changed() => match changed {
                Ok(()) => {
                    // Copied out first: the watch guard is not `Send` and must
                    // not be held across the notice send.
                    let connected = *connected_rx.borrow_and_update();
                    feed.on_link(connected).await
                }
                Err(_) => ControlFlow::Break(()),
            },
            maybe_cmd = cmd_rx.recv() => feed.on_command(maybe_cmd).await,
        };
        if flow.is_break() {
            break;
        }
    }
    reconnect.abort();
    let _ = reconnect.await;
    feed.shutdown().await;
}

/// One reply from a candle fetch: the bars of one window and where it sits.
type OhlcvReply = (Vec<quantick_engine::Bar>, crate::OhlcvSlice);

/// Send the opening history, returning the earliest agg_id to page back from,
/// or `Break` when the UI is gone.
async fn initial_backfill(
    http: &BinanceHttp,
    symbol: &str,
    tx: &mpsc::Sender<FeedEvent>,
) -> ControlFlow<(), Option<u64>> {
    let target = initial_backfill_target();
    match backfill(http, symbol, target).await {
        Ok(trades) => {
            let earliest_id = trades.first().map(|t| t.agg_id);
            info!(target: "quantick::app", symbol, count = trades.len(), target, "backfill ready");
            if tx.send(FeedEvent::Backfilled(trades)).await.is_err() {
                return ControlFlow::Break(()); // UI gone
            }
            ControlFlow::Continue(earliest_id)
        }
        Err(e) => {
            error!(target: "quantick::app", symbol, %e, "backfill failed; continuing to live only");
            // Still mark an empty boundary so the UI knows backfill is done.
            if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(None)
        }
    }
}

/// The streaming loop's driver: the side-task handles, the history cursor and
/// the channels the plan's effects go out on. What each command *means* is
/// [`plan_command`]'s decision; this only carries it out.
struct BinanceLoop {
    symbol: String,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    http: BinanceHttp,
    klines: BinanceKlineHttp,
    /// Earliest agg_id held, the cursor *load older* pages back from.
    earliest_id: Option<u64>,
    snapshot_limit: u16,
    book_capture: Option<BookCaptureTask>,
    ohlcv_tx: mpsc::Sender<OhlcvReply>,
    ohlcv_task: Option<JoinHandle<()>>,
    continuity: crate::continuity::BinanceContinuity,
    ever_connected: bool,
}

impl BinanceLoop {
    async fn on_ohlcv_reply(
        &mut self,
        bars: Vec<quantick_engine::Bar>,
        slice: crate::OhlcvSlice,
    ) -> ControlFlow<()> {
        if ohlcv_reply_frees_slot(slice) {
            self.ohlcv_task = None;
        }
        let event = FeedEvent::OhlcvHistory {
            interval_ms: ONE_MINUTE_MS,
            bars,
            slice,
        };
        send_or_break(&self.tx, event).await // UI gone
    }

    async fn on_trade(&mut self, trade: Trade) -> ControlFlow<()> {
        if let Some(event) = self.continuity.observe(&trade) {
            send_or_break(&self.tx, FeedEvent::Continuity(event)).await?;
        }
        send_or_break(&self.tx, FeedEvent::Live(trade)).await // UI gone
    }

    async fn on_link(&mut self, connected: bool) -> ControlFlow<()> {
        let notice = connection_notice(connected, &mut self.ever_connected, "Binance");
        send_or_break(&self.notice_tx, notice).await
    }

    async fn on_command(&mut self, cmd: Option<FeedCommand>) -> ControlFlow<()> {
        let book = Slot::observe(self.book_capture.as_ref().map(|t| t.handle.is_finished()));
        let ohlcv = Slot::observe(self.ohlcv_task.as_ref().map(JoinHandle::is_finished));
        match plan_command(cmd, book, ohlcv) {
            CommandPlan::LoadOlder { count } => {
                self.earliest_id =
                    load_older(&self.http, &self.symbol, self.earliest_id, count, &self.tx).await;
                if self.tx.is_closed() {
                    return ControlFlow::Break(()); // UI gone
                }
            }
            CommandPlan::RefuseOhlcv { span_ms, before_ms } => {
                // One fetch at a time: the venue's rate budget is shared, and
                // the in-flight one already answers.
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BINANCE_OHLCV_ALREADY_RUNNING",
                    symbol = self.symbol,
                    requested_span_ms = span_ms,
                    requested_before_ms = before_ms.unwrap_or(0),
                    action = "answer_empty_and_let_the_running_one_finish",
                    "a candle fetch is already in flight; this one is refused, not queued"
                );
                // Refused, but *answered*. The caller marked itself pending and
                // put its spinner up before this command left, so a silent drop
                // leaves that spinner turning for the rest of the session and
                // the reach-back button disabled behind it — the same reason
                // `load_older` never returns silence either. `Refused` rather
                // than a short answer: nothing was fetched because nobody
                // looked, which is not a statement about the venue's record.
                let refused = (Vec::new(), crate::OhlcvSlice::Refused);
                send_or_break(&self.ohlcv_tx, refused).await?; // UI gone
            }
            CommandPlan::StartOhlcv {
                span_ms,
                slice_ms,
                before_ms,
            } => {
                self.ohlcv_task = Some(spawn_ohlcv(
                    self.klines.clone(),
                    self.symbol.clone(),
                    span_ms,
                    slice_ms,
                    before_ms,
                    self.ohlcv_tx.clone(),
                ));
            }
            CommandPlan::KeepBook { initial_generation } => info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "book_capture_enable_ignored",
                provider = "binance",
                symbol = self.symbol.as_str(),
                initial_generation,
                snapshot_limit = self.snapshot_limit,
                action = "keep_running",
                "book capture is already running"
            ),
            CommandPlan::ReplaceBook {
                initial_generation,
                stop_reason,
            } => {
                stop_book_capture(&mut self.book_capture, &self.symbol, stop_reason).await;
                self.book_capture = Some(start_book_capture(
                    &self.symbol,
                    initial_generation,
                    self.snapshot_limit,
                    &self.book_tx,
                ));
            }
            CommandPlan::StopBook { reason } => {
                stop_book_capture(&mut self.book_capture, &self.symbol, reason).await;
            }
            CommandPlan::Ignore => {}
            // UI dropped the command sender: it's gone.
            CommandPlan::Shutdown => return ControlFlow::Break(()),
        }
        ControlFlow::Continue(())
    }

    async fn shutdown(mut self) {
        // A candle fetch can be 130 requests deep when the feed goes away;
        // nobody is left to read its answer.
        if let Some(task) = self.ohlcv_task.take() {
            task.abort();
        }
        stop_book_capture(&mut self.book_capture, &self.symbol, "feed_dropped").await;
    }
}

/// A running depth capture plus the epoch assigned by its controller.
struct BookCaptureTask {
    initial_generation: u64,
    handle: JoinHandle<()>,
}

/// Start one independently cancellable depth reconnect loop.
fn start_book_capture(
    symbol: &str,
    initial_generation: u64,
    snapshot_limit: u16,
    events: &mpsc::Sender<DepthEvent>,
) -> BookCaptureTask {
    let symbol = symbol.to_string();
    let events = events.clone();
    let source = BinanceDepthHttp::new();
    let config = DepthSessionConfig {
        snapshot_limit,
        initial_generation,
        ..DepthSessionConfig::default()
    };
    // Keep retry jitter reproducible while giving replacement generations a
    // different deterministic sequence.
    let backoff = Backoff::for_feed(0xD1B5_4A32_D192_ED03 ^ initial_generation);
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "book_capture_started",
        provider = "binance",
        symbol = symbol.as_str(),
        initial_generation,
        snapshot_limit,
        action = "start",
        "starting synchronized Binance book capture"
    );
    let handle = tokio::spawn(async move {
        run_depth_with_reconnect(BINANCE_WS_BASE, &symbol, &source, &events, config, backoff).await;
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "book_capture_task_finished",
            provider = "binance",
            symbol = symbol.as_str(),
            initial_generation,
            snapshot_limit,
            action = "stop",
            "Binance book capture task finished"
        );
    });
    BookCaptureTask {
        initial_generation,
        handle,
    }
}

/// Abort and reap the active depth task, if any.
async fn stop_book_capture(task: &mut Option<BookCaptureTask>, symbol: &str, reason: &'static str) {
    let Some(task) = task.take() else {
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "book_capture_stop_ignored",
            provider = "binance",
            symbol,
            reason,
            action = "already_stopped",
            "book capture stop requested with no active task"
        );
        return;
    };
    let initial_generation = task.initial_generation;
    task.handle.abort();
    let join_result = task.handle.await;
    let outcome = if join_result
        .as_ref()
        .is_err_and(tokio::task::JoinError::is_cancelled)
    {
        "cancelled"
    } else if join_result.is_ok() {
        "finished"
    } else {
        "join_error"
    };
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "book_capture_stopped",
        provider = "binance",
        symbol,
        initial_generation,
        reason,
        outcome,
        action = "stop",
        "Binance book capture stopped"
    );
}

/// Initial REST depth level count, configurable through
/// `QUANTICK_BOOK_DEPTH`.
fn initial_book_depth() -> u16 {
    parse_book_depth(std::env::var("QUANTICK_BOOK_DEPTH").ok().as_deref())
}

fn parse_book_depth(raw: Option<&str>) -> u16 {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.clamp(1, usize::from(MAX_DEPTH_LIMIT)) as u16)
        .unwrap_or(DEFAULT_BOOK_DEPTH)
}

/// Fetch `span_ms` of one-minute candles on a task of its own, delivering the
/// bars back to the feed loop through `reply` — in one message, or in a run of
/// them from the newest window backwards when `slice_ms` asks for slices.
///
/// Off the loop deliberately: this is the one command whose work is measured in
/// seconds rather than milliseconds, and the loop it was called from is the one
/// draining live trades. See where it is spawned for what awaiting it inline
/// used to cost.
///
/// A short answer is still an answer: the pane's loading indicator keys on the
/// closing reply, so a fetch that ran out of rate budget resolves it with what
/// it collected rather than leaving the pane waiting on a venue that has
/// stopped talking. That is also why a failed window does not abandon the run:
/// the remaining windows are still attempted and the closing slice reports the
/// whole answer as short. How much arrived is in the `BINANCE_OHLCV_*` log
/// events.
fn spawn_ohlcv(
    klines: BinanceKlineHttp,
    symbol: String,
    span_ms: i64,
    slice_ms: Option<i64>,
    before_ms: Option<i64>,
    reply: mpsc::Sender<(Vec<quantick_engine::Bar>, crate::OhlcvSlice)>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // The right-hand edge of everything this request covers: the live edge
        // for the opening fetch, and the millisecond before the oldest candle
        // held for a *load older*. The plan needs no other change to reach
        // further back — it always cut a span ending at a caller-supplied
        // instant, and the wall clock was only ever the instant that mattered.
        let now_ms = before_ms.unwrap_or_else(crate::clock::wall_clock_ms);
        let windows = crate::ohlcv_plan::plan(now_ms, span_ms, slice_ms);
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BINANCE_OHLCV_PLAN",
            symbol,
            requested_span_ms = span_ms,
            slice_ms = slice_ms.unwrap_or(0),
            windows = windows.len(),
            "candle history planned"
        );
        // Short windows accumulate into the answer's completeness: the trader
        // is told the span is short if *any* part of it came up short, never
        // just the last one fetched.
        let mut complete = true;
        for window in windows {
            let history = fetch_history(
                &klines,
                &symbol,
                KLINE_INTERVAL_1M,
                ONE_MINUTE_MS,
                window.from_ms,
                window.to_ms,
            )
            .await;
            complete &= history.complete;
            if !history.complete {
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BINANCE_OHLCV_PARTIAL",
                    symbol,
                    requested_span_ms = span_ms,
                    from_ms = window.from_ms,
                    to_ms = window.to_ms,
                    bars = history.bars.len(),
                    action = "answer_partial",
                    "candle history is short of the requested window"
                );
            }
            let slice = if window.last {
                crate::OhlcvSlice::Last { complete }
            } else {
                crate::OhlcvSlice::More
            };
            // A closed channel means the feed loop is gone, which is not this
            // task's problem to report: it is already being reported there.
            // Stop fetching, though — nothing is listening for the rest.
            if reply.send((history.bars, slice)).await.is_err() {
                return;
            }
        }
    })
}

/// Fetch `count` trades older than `earliest`, send them to the UI, and return
/// the new earliest agg_id (unchanged if nothing older was available or a fetch
/// failed). `None` earliest means there is no history to page back from.
///
/// Every call answers the UI with exactly one [`FeedEvent::HistoryPrepended`] —
/// empty when nothing older exists or the fetch failed — mirroring how a failed
/// initial backfill still sends an empty [`FeedEvent::Backfilled`]. The UI keys
/// its loading indicator on that reply, so a silent no-answer would leave a
/// spinner running forever.
async fn load_older(
    http: &BinanceHttp,
    symbol: &str,
    earliest: Option<u64>,
    count: usize,
    tx: &mpsc::Sender<FeedEvent>,
) -> Option<u64> {
    let Some(before) = earliest else {
        warn!(target: "quantick::app", "load older ignored: no history to page back from");
        let _ = tx.send(FeedEvent::HistoryPrepended(Vec::new())).await;
        return None;
    };
    match backfill_before(http, symbol, before, count).await {
        Ok(older) if !older.is_empty() => {
            let new_earliest = older.first().map(|t| t.agg_id);
            info!(target: "quantick::app", symbol, count = older.len(), "older history ready");
            if tx.send(FeedEvent::HistoryPrepended(older)).await.is_err() {
                return earliest; // UI gone; caller notices via tx.is_closed()
            }
            new_earliest
        }
        Ok(_) => {
            info!(target: "quantick::app", symbol, "no older history available");
            let _ = tx.send(FeedEvent::HistoryPrepended(Vec::new())).await;
            earliest
        }
        Err(e) => {
            error!(target: "quantick::app", symbol, %e, "load older failed");
            let _ = tx.send(FeedEvent::HistoryPrepended(Vec::new())).await;
            earliest
        }
    }
}

crate::hooks::declare_hooks!["QUANTICK_BOOK_DEPTH"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_depth_defaults_and_clamps_the_environment_value() {
        assert_eq!(parse_book_depth(None), DEFAULT_BOOK_DEPTH);
        assert_eq!(parse_book_depth(Some("")), DEFAULT_BOOK_DEPTH);
        assert_eq!(parse_book_depth(Some("invalid")), DEFAULT_BOOK_DEPTH);
        assert_eq!(parse_book_depth(Some("0")), 1);
        assert_eq!(parse_book_depth(Some("2500")), 2_500);
        assert_eq!(parse_book_depth(Some("999999")), MAX_DEPTH_LIMIT);
    }
}
