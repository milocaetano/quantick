//! Hyperliquid backend for the provider-neutral app feed bridge.
//!
//! A reconnecting WebSocket starts with the venue's recovery batch and then
//! carries factual aggressor-side prints. Preserving each venue batch through
//! the app bridge avoids per-trade channel overhead. A separately cancellable
//! `l2Book` connection publishes the visible 20-level book through the same
//! neutral [`DepthEvent`] channel Binance and MetaTrader use.

use std::ops::ControlFlow;

use tokio::{sync::mpsc, task::JoinHandle};

pub(crate) mod output;
use output::Output;
pub(crate) use output::{LegacyOutput, ObservedOutput};
use tracing::{info, warn};

use quantick_engine::Trade;
use quantick_feed_hyperliquid::{
    Backoff, CANDLE_INTERVAL_1M, HYPERLIQUID_WS_URL, ONE_MINUTE_MS, TradeMapper, TradeStreamEvent,
    depth::{DepthEvent, HYPERLIQUID_LEVELS_PER_SIDE, run_depth_with_reconnect},
    fetch_candle_history, run_trade_events_with_reconnect,
};

use super::{FeedCommand, FeedEvent, FeedHandle, FeedNotice, connection_notice};
use crate::config::ProviderKind;
use crate::venue_loop::{CommandPlan, Slot, ohlcv_reply_frees_slot, plan_command, send_or_break};

const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;
const FEED_EVENT_CHANNEL_CAPACITY: usize = 4_096;
const TRADE_BATCH_CHANNEL_CAPACITY: usize = 1_024;
const COMMAND_CHANNEL_CAPACITY: usize = 16;
const NOTICE_CHANNEL_CAPACITY: usize = 32;
const FEED_RUNTIME_WORKERS: usize = 2;
const STARTUP_RECOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const TRADE_RECONNECT_SEED: u64 = 0x4859_5045_525F_5452;
const DEPTH_RECONNECT_SEED: u64 = 0x4859_5045_525F_4C32;

pub(crate) struct HyperliquidSource {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) local_fixture: bool,
    pub(crate) url: String,
    pub(crate) backoff: Backoff,
}

/// Start the selected Hyperliquid perpetual on a background runtime.
#[must_use]
pub fn spawn(symbol: &str) -> FeedHandle {
    let (tx, rx) = mpsc::channel(FEED_EVENT_CHANNEL_CAPACITY);
    let (book_tx, book_rx) = mpsc::channel(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    start(symbol, LegacyOutput(tx), book_tx, notice_tx, cmd_rx);

    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: notice_rx,
        capabilities: super::fixed_capabilities(ProviderKind::Hyperliquid.capabilities()),
        // A web-socket venue stamps a trade and quantick reads it: there is no
        // hop between those two to attribute a delay to.
        latency: super::unsplit_latency(),
        commands: cmd_tx,
        replay: None,
    }
}

/// Start the ordered observation port; legacy callers retain their old handle.
#[must_use]
pub fn spawn_observed(symbol: &str) -> crate::ObservedFeedHandle {
    let (tx, rx) = mpsc::channel(FEED_EVENT_CHANNEL_CAPACITY);
    let (book_tx, book_rx) = mpsc::channel(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    start(symbol, ObservedOutput(tx), book_tx, notice_tx, cmd_rx);
    crate::ObservedFeedHandle {
        events: rx.into(),
        book_events: book_rx,
        notices: notice_rx,
        capabilities: super::fixed_capabilities(ProviderKind::Hyperliquid.capabilities()),
        latency: super::unsplit_latency(),
        commands: cmd_tx,
        replay: None,
    }
}

fn start<O: Output>(
    symbol: &str,
    tx: O,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    let symbol = symbol.to_owned();
    let source = HyperliquidSource {
        #[cfg(any(test, feature = "test-support"))]
        local_fixture: false,
        url: HYPERLIQUID_WS_URL.to_owned(),
        backoff: Backoff::for_feed(TRADE_RECONNECT_SEED),
    };
    std::thread::Builder::new()
        .name("quantick-hyperliquid-feed".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(FEED_RUNTIME_WORKERS)
                .enable_all()
                .build()
                .expect("build Hyperliquid feed runtime");
            runtime.block_on(feed_task_with(
                symbol, tx, book_tx, notice_tx, cmd_rx, source,
            ));
        })
        .expect("spawn Hyperliquid feed thread");
}

pub(crate) async fn feed_task_with<O: Output>(
    symbol: String,
    tx: O,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
    source: HyperliquidSource,
) {
    #[cfg(any(test, feature = "test-support"))]
    let local_fixture = source.local_fixture;
    let symbol = symbol.to_uppercase();
    let mapper = TradeMapper::new(&symbol);
    // Connection transitions and mapped batches share one ordered, bounded
    // channel: a watch would coalesce a drop-and-reconnect into nothing, and a
    // handoff the host never saw cannot be reported as unknown continuity.
    let (stream_tx, mut stream_rx) =
        mpsc::channel::<TradeStreamEvent>(TRADE_BATCH_CHANNEL_CAPACITY);
    let stream_symbol = symbol.clone();
    let reconnect = tokio::spawn(async move {
        run_trade_events_with_reconnect(
            &source.url,
            &stream_symbol,
            &stream_tx,
            mapper,
            source.backoff,
        )
        .await;
    });
    // Candle history runs off this loop: see `spawn_ohlcv` for what awaiting it
    // in a command arm used to cost the live trade stream.
    let (ohlcv_tx, mut ohlcv_rx) = mpsc::channel::<OhlcvReply>(1);
    let mut feed = HyperliquidLoop {
        symbol,
        tx,
        book_tx,
        notice_tx,
        book_capture: None,
        ohlcv_tx,
        ohlcv_task: None,
        ever_connected: false,
        recovery_pending: true,
        #[cfg(any(test, feature = "test-support"))]
        local_fixture,
    };
    let recovery_timeout = tokio::time::sleep(STARTUP_RECOVERY_TIMEOUT);
    tokio::pin!(recovery_timeout);

    loop {
        let flow = tokio::select! {
            Some((bars, slice)) = ohlcv_rx.recv() => feed.on_ohlcv_reply(bars, slice).await,
            maybe_stream_event = stream_rx.recv() => match maybe_stream_event {
                Some(TradeStreamEvent::Connected) => feed.on_link(true).await,
                Some(TradeStreamEvent::Disconnected) => feed.on_disconnect().await,
                Some(TradeStreamEvent::Batch(batch)) => feed.on_batch(batch).await,
                None => ControlFlow::Break(()),
            },
            () = &mut recovery_timeout, if feed.recovery_pending => feed.on_recovery_timeout().await,
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

/// The streaming loop's driver: the side-task handles, whether the startup
/// recovery batch is still owed, and the channels the plan's effects go out
/// on. What each command *means* is [`plan_command`]'s decision.
struct HyperliquidLoop<O: Output> {
    symbol: String,
    tx: O,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    book_capture: Option<BookCaptureTask>,
    ohlcv_tx: mpsc::Sender<OhlcvReply>,
    ohlcv_task: Option<JoinHandle<()>>,
    ever_connected: bool,
    /// The first batch is the venue's recovery batch and resolves the UI's
    /// backfill; cleared by that batch or by the startup timeout.
    recovery_pending: bool,
    /// A local synthetic source has no candle or depth transport; its host
    /// refuses those requests instead of dialling the real venue.
    #[cfg(any(test, feature = "test-support"))]
    local_fixture: bool,
}

impl<O: Output> HyperliquidLoop<O> {
    /// Deliver one event in order, or `Break` when the consumer is gone.
    async fn emit(&self, event: FeedEvent) -> ControlFlow<()> {
        if self.tx.send(event).await.is_err() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    async fn on_ohlcv_reply(
        &mut self,
        bars: Vec<quantick_engine::Bar>,
        slice: crate::OhlcvSlice,
    ) -> ControlFlow<()> {
        // Only the closing slice frees the slot: a run still walking backwards
        // through the span is one fetch, however many replies it makes.
        if ohlcv_reply_frees_slot(slice) {
            self.ohlcv_task = None;
        }
        let event = FeedEvent::OhlcvHistory {
            interval_ms: ONE_MINUTE_MS,
            bars,
            slice,
        };
        self.emit(event).await // UI gone
    }

    /// Received rows the venue sent but the mapper excluded are reported apart
    /// from source continuity: they are not missing source IDs.
    async fn on_batch(&mut self, batch: quantick_feed_hyperliquid::MappedBatch) -> ControlFlow<()> {
        let malformed = u64::try_from(batch.errors.len()).unwrap_or(u64::MAX);
        let stale = u64::try_from(batch.stale).unwrap_or(u64::MAX);
        for (reason, count) in [
            (crate::ExclusionReason::MalformedRow, malformed),
            (crate::ExclusionReason::StaleTimestamp, stale),
        ] {
            if let Some(rows) = std::num::NonZeroU64::new(count)
                && self
                    .tx
                    .exclude(crate::FeedExclusion { reason, rows })
                    .await
                    .is_err()
            {
                return ControlFlow::Break(());
            }
        }
        let trades = batch.trades;
        if trades.is_empty() && !self.recovery_pending {
            return ControlFlow::Continue(());
        }
        let is_recovery = self.recovery_pending;
        let count = trades.len();
        let event = classify_trade_batch(trades, &mut self.recovery_pending);
        if is_recovery {
            info!(
                target: "quantick::app",
                provider = "hyperliquid",
                symbol = self.symbol,
                count,
                "initial Hyperliquid websocket recovery ready"
            );
        }
        self.emit(event).await
    }

    /// A dropped socket makes the handoff unknown: whatever the venue printed
    /// until the next connection is neither delivered nor countable.
    async fn on_disconnect(&mut self) -> ControlFlow<()> {
        let continuity = crate::FeedContinuity {
            gap: None,
            missing_messages: None,
            non_monotonic: false,
        };
        self.emit(FeedEvent::Continuity(continuity)).await?;
        self.on_link(false).await
    }

    async fn on_link(&mut self, connected: bool) -> ControlFlow<()> {
        let notice = connection_notice(connected, &mut self.ever_connected, "Hyperliquid");
        send_or_break(&self.notice_tx, notice).await
    }

    async fn on_recovery_timeout(&mut self) -> ControlFlow<()> {
        self.recovery_pending = false;
        warn!(
            target: "quantick::app",
            provider = "hyperliquid",
            symbol = self.symbol,
            timeout_ms = STARTUP_RECOVERY_TIMEOUT.as_millis() as u64,
            action = "continue_live",
            "initial Hyperliquid websocket recovery timed out"
        );
        self.emit(FeedEvent::Backfilled(Vec::new())).await
    }

    /// The local synthetic fixture refuses candle and depth requests; `None`
    /// hands every other command to the shared plan.
    #[cfg(any(test, feature = "test-support"))]
    async fn on_fixture_command(&mut self, cmd: &Option<FeedCommand>) -> Option<ControlFlow<()>> {
        if !self.local_fixture {
            return None;
        }
        match cmd {
            Some(FeedCommand::FetchOhlcv { .. }) => {
                warn!("local synthetic fixture has no candle history; request refused");
                Some(
                    self.emit(FeedEvent::OhlcvHistory {
                        interval_ms: ONE_MINUTE_MS,
                        bars: Vec::new(),
                        slice: crate::OhlcvSlice::Refused,
                    })
                    .await,
                )
            }
            Some(FeedCommand::SetBookCapture { .. } | FeedCommand::RestartBookCapture { .. }) => {
                warn!("local synthetic fixture has no depth transport; request refused");
                Some(ControlFlow::Continue(()))
            }
            _ => None,
        }
    }

    async fn on_command(&mut self, cmd: Option<FeedCommand>) -> ControlFlow<()> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(flow) = self.on_fixture_command(&cmd).await {
            return flow;
        }
        let book = Slot::observe(self.book_capture.as_ref().map(|t| t.handle.is_finished()));
        let ohlcv = Slot::observe(self.ohlcv_task.as_ref().map(JoinHandle::is_finished));
        match plan_command(cmd, book, ohlcv) {
            CommandPlan::RefuseOhlcv { span_ms, before_ms } => {
                // One fetch at a time; the in-flight one answers.
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HYPERLIQUID_OHLCV_ALREADY_RUNNING",
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
                    self.symbol.clone(),
                    span_ms,
                    slice_ms,
                    before_ms,
                    self.ohlcv_tx.clone(),
                ));
            }
            CommandPlan::LoadOlder { .. } => {
                // `recentTrades` is not pageable. Always acknowledge the
                // request so a stale UI command cannot leave a spinner.
                warn!(
                    target: "quantick::app",
                    provider = "hyperliquid",
                    symbol = self.symbol,
                    action = "report_no_history_paging",
                    "older Hyperliquid public trades are unavailable"
                );
                self.emit(FeedEvent::HistoryPrepended(Vec::new())).await?;
            }
            CommandPlan::KeepBook { initial_generation } => info!(
                target: "quantick::app",
                provider = "hyperliquid",
                symbol = self.symbol,
                initial_generation,
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
                    &self.book_tx,
                ));
            }
            CommandPlan::StopBook { reason } => {
                stop_book_capture(&mut self.book_capture, &self.symbol, reason).await;
            }
            CommandPlan::Ignore => {}
            CommandPlan::Shutdown => return ControlFlow::Break(()),
        }
        ControlFlow::Continue(())
    }

    async fn shutdown(mut self) {
        // The candle socket is its own connection; close it with the feed
        // rather than leaving it to time out against a consumer that is gone.
        if let Some(task) = self.ohlcv_task.take() {
            task.abort();
        }
        stop_book_capture(&mut self.book_capture, &self.symbol, "feed_dropped").await;
    }
}

fn classify_trade_batch(trades: Vec<Trade>, recovery_pending: &mut bool) -> FeedEvent {
    if std::mem::take(recovery_pending) {
        FeedEvent::Backfilled(trades)
    } else {
        FeedEvent::LiveBatch(trades)
    }
}

struct BookCaptureTask {
    initial_generation: u64,
    handle: JoinHandle<()>,
}

fn start_book_capture(
    symbol: &str,
    initial_generation: u64,
    events: &mpsc::Sender<DepthEvent>,
) -> BookCaptureTask {
    let symbol = symbol.to_owned();
    let events = events.clone();
    let backoff = Backoff::for_feed(DEPTH_RECONNECT_SEED ^ initial_generation);
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "book_capture_started",
        provider = "hyperliquid",
        symbol,
        initial_generation,
        coverage_levels_per_side = HYPERLIQUID_LEVELS_PER_SIDE,
        action = "start",
        "starting Hyperliquid L2 capture"
    );
    let task_symbol = symbol.clone();
    let handle = tokio::spawn(async move {
        run_depth_with_reconnect(
            HYPERLIQUID_WS_URL,
            &task_symbol,
            &events,
            initial_generation,
            backoff,
        )
        .await;
    });
    BookCaptureTask {
        initial_generation,
        handle,
    }
}

/// Fetch `span_ms` of one-minute candles on a task of its own, delivering the
/// bars back to the feed loop through `reply`.
///
/// Off the loop deliberately. The fetch already opens its own socket, so it
/// never competed with the trade stream for bandwidth — but awaiting it in a
/// command arm stopped the loop from polling `live_rx`, which against a stalled
/// venue meant minutes of unread trades, a full channel, and a websocket read
/// loop stalled behind it.
///
/// Every outcome answers, including the ones that failed: the pane's loading
/// indicator keys on the reply, and a venue that refused is a reason to show an
/// empty pane, not a reason to leave one spinning. What went wrong is in the
/// log, where it can carry the detail a chart cannot.
fn spawn_ohlcv(
    symbol: String,
    span_ms: i64,
    slice_ms: Option<i64>,
    before_ms: Option<i64>,
    reply: mpsc::Sender<(Vec<quantick_engine::Bar>, crate::OhlcvSlice)>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let symbol = symbol.as_str();
        // See the Binance twin: the live edge, or the instant a *load older*
        // wants the reply to end at.
        let now_ms = before_ms.unwrap_or_else(crate::clock::wall_clock_ms);
        let windows = crate::ohlcv_plan::plan(now_ms, span_ms, slice_ms);
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HYPERLIQUID_OHLCV_PLAN",
            symbol,
            requested_span_ms = span_ms,
            slice_ms = slice_ms.unwrap_or(0),
            windows = windows.len(),
            "candle history planned"
        );
        // Short or failed windows accumulate into the answer's completeness:
        // the trader is told the span is short if *any* part of it came up
        // short, never just the last window fetched.
        let mut complete = true;
        for window in windows {
            let bars = match fetch_candle_history(
                HYPERLIQUID_WS_URL,
                symbol,
                CANDLE_INTERVAL_1M,
                ONE_MINUTE_MS,
                window.from_ms,
                window.to_ms,
            )
            .await
            {
                Ok(history) => {
                    if !history.complete {
                        warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "HYPERLIQUID_OHLCV_PARTIAL",
                            symbol,
                            requested_span_ms = span_ms,
                            from_ms = window.from_ms,
                            to_ms = window.to_ms,
                            bars = history.bars.len(),
                            action = "answer_partial",
                            "candle history is short of the requested window"
                        );
                    }
                    complete &= history.complete;
                    history.bars
                }
                Err(error) => {
                    warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "HYPERLIQUID_OHLCV_FAILED",
                        symbol,
                        requested_span_ms = span_ms,
                        from_ms = window.from_ms,
                        to_ms = window.to_ms,
                        %error,
                        action = "answer_empty",
                        "could not fetch candle history"
                    );
                    // A failed window is the clearest "not the whole span"
                    // there is — and a reason to keep going, not to stop: the
                    // windows are independent, and one refused socket says
                    // nothing about the next.
                    complete = false;
                    Vec::new()
                }
            };
            let slice = if window.last {
                crate::OhlcvSlice::Last { complete }
            } else {
                crate::OhlcvSlice::More
            };
            // A closed channel means the feed loop is gone, which is not this
            // task's problem to report: it is already being reported there.
            // Stop fetching, though — nothing is listening for the rest.
            if reply.send((bars, slice)).await.is_err() {
                return;
            }
        }
    })
}

async fn stop_book_capture(task: &mut Option<BookCaptureTask>, symbol: &str, reason: &'static str) {
    let Some(task) = task.take() else {
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
        provider = "hyperliquid",
        symbol,
        initial_generation,
        reason,
        outcome,
        action = "stop",
        "Hyperliquid book capture stopped"
    );
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt as _, StreamExt as _};
    use rust_decimal::Decimal;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;

    fn trade(id: u64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: i64::try_from(id).expect("test id fits in i64"),
            price: Decimal::ONE,
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        }
    }

    #[test]
    fn first_websocket_batch_is_recovery_then_batches_stay_live() {
        let mut recovery_pending = true;
        let first = classify_trade_batch(vec![trade(1), trade(2)], &mut recovery_pending);
        assert!(matches!(first, FeedEvent::Backfilled(trades) if trades.len() == 2));
        assert!(!recovery_pending);

        let second = classify_trade_batch(vec![trade(3)], &mut recovery_pending);
        assert!(matches!(second, FeedEvent::LiveBatch(trades) if trades.len() == 1));
    }

    #[test]
    fn batch_after_recovery_timeout_is_live() {
        let mut recovery_pending = false;
        let event = classify_trade_batch(vec![trade(1)], &mut recovery_pending);
        assert!(matches!(event, FeedEvent::LiveBatch(trades) if trades.len() == 1));
    }

    async fn accept_trade_subscription(
        listener: &TcpListener,
    ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let subscription = socket.next().await.unwrap().unwrap();
        assert!(
            matches!(subscription, Message::Text(text) if text.contains("\"type\":\"trades\""))
        );
        socket
    }

    #[tokio::test]
    async fn real_stream_integrity_reaches_the_provider_neutral_host_in_order() {
        const ACK: &str = r#"{"channel":"subscriptionResponse","data":{}}"#;
        const EMPTY: &str = r#"{"channel":"trades","data":[]}"#;
        const MIXED: &str = r#"{"channel":"trades","data":[
            {"coin":"BTC","side":"B","px":"1","sz":"1","time":200,"tid":7},
            {"coin":"BTC","side":"X","px":"1","sz":"1","time":201,"tid":8}
        ]}"#;
        const OVERLAP: &str = r#"{"channel":"trades","data":[
            {"coin":"BTC","side":"B","px":"1","sz":"1","time":200,"tid":7}
        ]}"#;
        const STALE: &str = r#"{"channel":"trades","data":[
            {"coin":"BTC","side":"A","px":"1","sz":"1","time":199,"tid":9}
        ]}"#;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut socket = accept_trade_subscription(&listener).await;
            socket.send(Message::Text(ACK.into())).await.unwrap();
            socket.close(None).await.unwrap();

            let mut socket = accept_trade_subscription(&listener).await;
            socket.send(Message::Text(ACK.into())).await.unwrap();
            socket.send(Message::Text(EMPTY.into())).await.unwrap();
            socket.send(Message::Text(MIXED.into())).await.unwrap();
            socket.close(None).await.unwrap();

            let mut socket = accept_trade_subscription(&listener).await;
            socket.send(Message::Text(ACK.into())).await.unwrap();
            socket.send(Message::Text(OVERLAP.into())).await.unwrap();
            socket.send(Message::Text(STALE.into())).await.unwrap();
            socket.close(None).await.unwrap();
        });

        let (tx, mut rx) = mpsc::channel(16);
        let (book_tx, _book_rx) = mpsc::channel(8);
        let (notice_tx, _notice_rx) = mpsc::channel(16);
        let (cmd_tx, cmd_rx) = mpsc::channel(8);
        let host = tokio::spawn(feed_task_with(
            "BTC".into(),
            ObservedOutput(tx),
            book_tx,
            notice_tx,
            cmd_rx,
            HyperliquidSource {
                local_fixture: false,
                url: format!("ws://{address}"),
                backoff: Backoff::new(
                    std::time::Duration::from_millis(1),
                    std::time::Duration::from_millis(1),
                    7,
                ),
            },
        ));

        let events = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut events = Vec::new();
            for _ in 0..7 {
                events.push(rx.recv().await.unwrap());
            }
            events
        })
        .await
        .unwrap();
        assert!(matches!(
            events[0],
            crate::ObservedFeedEvent::Feed(FeedEvent::Continuity(crate::FeedContinuity {
                gap: None,
                missing_messages: None,
                non_monotonic: false,
            }))
        ));
        assert!(
            matches!(&events[1], crate::ObservedFeedEvent::Feed(FeedEvent::Backfilled(trades)) if trades.is_empty())
        );
        assert!(
            matches!(events[2], crate::ObservedFeedEvent::Excluded(crate::FeedExclusion { reason: crate::ExclusionReason::MalformedRow, rows }) if rows.get() == 1)
        );
        assert!(
            matches!(&events[3], crate::ObservedFeedEvent::Feed(FeedEvent::LiveBatch(trades)) if trades.len() == 1 && trades[0].timestamp_ms == 200)
        );
        assert!(matches!(
            events[4],
            crate::ObservedFeedEvent::Feed(FeedEvent::Continuity(crate::FeedContinuity {
                missing_messages: None,
                ..
            }))
        ));
        assert!(
            matches!(events[5], crate::ObservedFeedEvent::Excluded(crate::FeedExclusion { reason: crate::ExclusionReason::StaleTimestamp, rows }) if rows.get() == 1)
        );
        assert!(matches!(
            events[6],
            crate::ObservedFeedEvent::Feed(FeedEvent::Continuity(crate::FeedContinuity {
                missing_messages: None,
                ..
            }))
        ));
        assert!(
            rx.try_recv().is_err(),
            "overlap batches must not invent loss"
        );

        drop(cmd_tx);
        tokio::time::timeout(std::time::Duration::from_secs(2), host)
            .await
            .expect("host did not stop")
            .unwrap();
        server.await.unwrap();
    }
}
