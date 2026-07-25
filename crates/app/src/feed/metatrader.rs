//! MetaTrader 5 backend: the local QuantickBridge listener → [`FeedEvent`]s.
//!
//! MT5 has no public REST/WebSocket API, so the roles invert versus Binance:
//! `quantick-feed-mt5` listens on a local TCP port and the QuantickBridge EA
//! (running inside the logged-in terminal, see `bridge/mt5/README.md`) dials
//! out and streams ticks. No credentials exist anywhere in this path.
//!
//! Translation to the UI contract, honestly:
//!
//! - An **empty [`FeedEvent::Backfilled`] is sent immediately** — MT5 has no
//!   fetch-on-demand history, so the "initial backfill" resolves at once and
//!   the chart opens honestly empty ("connecting to WIN$N …") until the
//!   bridge connects.
//! - The bridge's backfill block (recent `CopyTicks` history) arrives as
//!   [`FeedEvent::HistoryPrepended`] — but only while no trade has been
//!   forwarded yet; after that, recovered history is forwarded as live to
//!   keep the retained stream ordered (logged as such, never silent).
//! - **Reconnect overlap is dropped, not double-counted**: the bridge
//!   re-sends its recent-history window on every session, and synthetic ids
//!   restart, so the only stable cross-session key is time. Recovered trades
//!   are forwarded only when strictly newer than the last forwarded trade;
//!   the dropped overlap count is logged. (Trades sharing the last forwarded
//!   millisecond are dropped too — losing a same-ms tick to a reconnect is
//!   honest; silently inflating bars is not.)
//! - **"Load older" is unsupported**: every request is answered with an empty
//!   reply plus a structured warning, so the UI's loader always resolves.
//! - **Order-book capture is supported** when the bridge declares a Depth of
//!   Market. The terminal republishes a complete DOM image on every change and
//!   `quantick-feed-mt5` diffs those into the same snapshot-plus-delta stream
//!   Binance produces, so the heatmap runs on one shared pipeline. Capture is
//!   a switch on the running session rather than a task to start and stop: the
//!   book shares one socket with ticks, and MQL5 offers no back-channel to ask
//!   the terminal to stop sending it.
//!
//! Synthetic ids caveat: `agg_id` restarts at 1 on every bridge session, so
//! ids may repeat across reconnects within one chart lifetime. Bars are built
//! from trade order, not ids, so the chart is unaffected; anything keying on
//! `agg_id` across sessions must not, and this is the place that documents it.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use quantick_feed_mt5::{
    BookCaptureSwitch, Mt5Event, Mt5Status, ServerConfig, SideMode, run_bridge_server,
};

use crate::config::{MetaTraderSettings, Mt5SideSource};

use super::{DepthEvent, FeedCommand, FeedEvent, FeedHandle};

/// Depth events are independent from the established trade channel. Sized like
/// the Binance backend's: a B3 book republishes far faster than the UI drains,
/// and this absorbs bursts without either dropping deltas or stalling trades.
const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;

/// How long the autostart waits for a bridge that is already running before
/// launching its own. Long enough for an attached Expert Advisor's reconnect
/// cycle to notice the port, short enough that a cold start feels immediate.
const BRIDGE_AUTOSTART_GRACE: Duration = Duration::from_secs(3);

/// Gap between autostart attempts. The common failure is a terminal still
/// starting up, which resolves in seconds.
const BRIDGE_AUTOSTART_RETRY: Duration = Duration::from_secs(5);

/// How many times the autostart relaunches an exiting bridge before leaving it
/// alone. Every remaining failure (terminal closed, unknown symbol, unknown
/// server offset) needs a human, and retrying it forever only buries the log
/// line that says so.
const BRIDGE_AUTOSTART_ATTEMPTS: u32 = 5;

/// Start the MetaTrader feed for `symbol`: listen for the bridge on the
/// configured address and translate its stream into [`FeedEvent`]s.
#[must_use]
pub fn spawn(symbol: &str, settings: &MetaTraderSettings) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
    let (book_tx, book_rx) = mpsc::channel::<DepthEvent>(BOOK_EVENT_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let symbol = symbol.to_string();
    let settings = settings.clone();
    std::thread::Builder::new()
        .name("quantick-feed-mt5".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build feed runtime");
            runtime.block_on(feed_task(symbol, settings, tx, book_tx, cmd_rx));
        })
        .expect("spawn mt5 feed thread");
    FeedHandle {
        events: rx,
        book_events: book_rx,
        commands: cmd_tx,
        replay: None,
    }
}

async fn feed_task(
    symbol: String,
    settings: MetaTraderSettings,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    // Resolve the UI's initial history load immediately: there is no
    // fetch-on-demand history on MT5. Bridge history arrives as a prepend.
    if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
        return; // UI gone
    }

    let mut server_cfg = ServerConfig::new(symbol.clone());
    server_cfg.listen_addr = settings.listen_addr.clone();
    server_cfg.side_mode = match settings.side_source {
        Mt5SideSource::TickRule => SideMode::TickRule,
        Mt5SideSource::Flags => SideMode::Flags,
    };

    // Selecting a MetaTrader feed is one action; starting a bridge by hand
    // afterwards was the second one this removes. The supervisor holds off
    // until it is sure nobody else is already feeding us.
    let bridge_connected = Arc::new(AtomicBool::new(false));
    let autostart = settings.bridge_autostart.then(|| {
        tokio::spawn(supervise_bridge(
            symbol.clone(),
            settings.clone(),
            Arc::clone(&bridge_connected),
        ))
    });

    // The switch lives on this side of the server task so UI commands can flip
    // depth capture without disturbing the bridge session or the trade stream.
    let book_capture = BookCaptureSwitch::new();
    server_cfg.book_capture = book_capture.clone();

    let (mt5_tx, mut mt5_rx) = mpsc::channel::<Mt5Event>(4096);
    let server = tokio::spawn(run_bridge_server(server_cfg, mt5_tx));

    // Whether any trade reached the UI yet: the first non-empty history block
    // may be prepended only into an empty chart (see module docs).
    let mut forwarded_any = false;
    // Newest trade timestamp forwarded to the UI. Reconnect history overlaps
    // what was already streamed live; only strictly-newer trades pass.
    let mut last_forwarded_ms = i64::MIN;

    loop {
        tokio::select! {
            maybe_event = mt5_rx.recv() => {
                match maybe_event {
                    Some(Mt5Event::Status(status)) => {
                        if matches!(status, Mt5Status::Connected { .. }) {
                            bridge_connected.store(true, Ordering::Relaxed);
                        }
                        log_status(&symbol, &status);
                    }
                    Some(Mt5Event::Backfilled(batch)) => {
                        if batch.is_empty() {
                            continue;
                        }
                        if forwarded_any {
                            // Reconnect history: forward only what the UI has
                            // not already seen. Labelled, not hidden.
                            let resent = batch.len();
                            let fresh: Vec<_> = batch
                                .into_iter()
                                .filter(|t| t.timestamp_ms > last_forwarded_ms)
                                .collect();
                            info!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_RECOVERED_HISTORY_AS_LIVE",
                                symbol = %symbol,
                                count = fresh.len(),
                                overlap_dropped = resent - fresh.len(),
                                "bridge re-sent history after a reconnect; forwarding the unseen tail as live"
                            );
                            for trade in fresh {
                                last_forwarded_ms = last_forwarded_ms.max(trade.timestamp_ms);
                                if tx.send(FeedEvent::Live(trade)).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            forwarded_any = true;
                            last_forwarded_ms = batch
                                .iter()
                                .map(|t| t.timestamp_ms)
                                .max()
                                .unwrap_or(last_forwarded_ms);
                            info!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_HISTORY_READY",
                                symbol = %symbol,
                                count = batch.len(),
                                "bridge history ready"
                            );
                            if tx.send(FeedEvent::HistoryPrepended(batch)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Mt5Event::Depth(event)) => {
                        // Backpressure rather than dropping: after the opening
                        // snapshot every event is an absolute delta, and a
                        // dropped one desynchronizes the book until the next
                        // generation. A full buffer means the UI is stalled,
                        // and the trade stream is buffered too.
                        if book_tx.send(event).await.is_err() {
                            break; // UI gone
                        }
                    }
                    Some(Mt5Event::Live(trade)) => {
                        forwarded_any = true;
                        last_forwarded_ms = last_forwarded_ms.max(trade.timestamp_ms);
                        if tx.send(FeedEvent::Live(trade)).await.is_err() {
                            break; // UI gone
                        }
                    }
                    None => {
                        // The server ended: either a fatal error (log it) or
                        // we are shutting down. Keep serving UI commands so
                        // the loader can never hang on a dead feed.
                        match server.await {
                            Ok(Err(e)) => error!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_BIND_FAILED",
                                symbol = %symbol,
                                %e,
                                "MT5 bridge listener failed; feed is idle (is another quantick running?)"
                            ),
                            Ok(Ok(())) => {}
                            Err(e) => error!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_SERVER_PANIC",
                                symbol = %symbol,
                                %e,
                                "MT5 bridge listener crashed"
                            ),
                        }
                        idle_serve_commands(&symbol, &tx, &mut cmd_rx, &book_capture).await;
                        return;
                    }
                }
                if tx.is_closed() {
                    break;
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(cmd) => {
                        if !answer_command(&symbol, cmd, &tx, &book_capture).await {
                            break; // UI gone
                        }
                    }
                    None => break, // UI dropped the command sender: it's gone
                }
            }
        }
    }
    server.abort();
    // Dropping the supervisor's task drops the child handle, and the child was
    // spawned with kill-on-drop: a bridge quantick started never outlives the
    // feed that wanted it.
    if let Some(autostart) = autostart {
        autostart.abort();
    }
}

/// Launch a bridge when nothing else is feeding us, and keep it alive.
///
/// Deliberately passive at the start: a bridge that is already running, or an
/// Expert Advisor attached to a chart, gets the grace period to dial in. Only
/// silence triggers a launch.
async fn supervise_bridge(
    symbol: String,
    settings: MetaTraderSettings,
    connected: Arc<AtomicBool>,
) {
    let Some((host, port)) = settings.bridge_endpoint() else {
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_AUTOSTART_SKIPPED",
            symbol = %symbol,
            listen_addr = %settings.listen_addr,
            action = "wait_for_manual_bridge",
            "cannot derive a dial address from listen_addr; not starting a bridge"
        );
        return;
    };
    let Some((program, extra)) = settings.bridge_command.split_first() else {
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_AUTOSTART_SKIPPED",
            symbol = %symbol,
            action = "wait_for_manual_bridge",
            "bridge_command is empty; not starting a bridge"
        );
        return;
    };

    tokio::time::sleep(BRIDGE_AUTOSTART_GRACE).await;
    for attempt in 1..=BRIDGE_AUTOSTART_ATTEMPTS {
        if connected.load(Ordering::Relaxed) {
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_AUTOSTART_NOT_NEEDED",
                symbol = %symbol,
                action = "leave_running_bridge_alone",
                "a bridge is already connected; quantick started none of its own"
            );
            return;
        }

        let mut command = Command::new(program);
        command
            .args(extra)
            .arg("--symbol")
            .arg(&symbol)
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port)
            // The bridge speaks the same structured-log vocabulary, so letting
            // it write to our streams keeps one story in one place.
            .stdin(Stdio::null())
            .kill_on_drop(true);

        match command.spawn() {
            Ok(mut child) => {
                info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_SPAWNED",
                    symbol = %symbol,
                    program = %program,
                    pid = child.id(),
                    attempt,
                    host,
                    port,
                    "started a bridge for this feed"
                );
                let status = child.wait().await;
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_EXITED",
                    symbol = %symbol,
                    attempt,
                    max_attempts = BRIDGE_AUTOSTART_ATTEMPTS,
                    status = ?status.map(|s| s.code()),
                    action = "retry_after_backoff",
                    "the bridge quantick started has exited"
                );
            }
            Err(error) => {
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_SPAWN_FAILED",
                    symbol = %symbol,
                    program = %program,
                    attempt,
                    %error,
                    action = "retry_after_backoff",
                    "could not start the bridge (is it on PATH? is the working directory the repo?)"
                );
            }
        }
        tokio::time::sleep(BRIDGE_AUTOSTART_RETRY).await;
    }

    warn!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MT5_BRIDGE_AUTOSTART_GAVE_UP",
        symbol = %symbol,
        attempts = BRIDGE_AUTOSTART_ATTEMPTS,
        action = "wait_for_manual_bridge",
        "the bridge would not stay up; read its own log lines above for the reason"
    );
}

/// After a fatal listener error, keep answering UI commands honestly (empty
/// replies) so no loader ever spins forever on a dead feed.
async fn idle_serve_commands(
    symbol: &str,
    tx: &mpsc::Sender<FeedEvent>,
    cmd_rx: &mut mpsc::Receiver<FeedCommand>,
    book_capture: &BookCaptureSwitch,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        if !answer_command(symbol, cmd, tx, book_capture).await {
            return;
        }
    }
}

/// Answer one UI command. Returns false when the UI is gone.
async fn answer_command(
    symbol: &str,
    cmd: FeedCommand,
    tx: &mpsc::Sender<FeedEvent>,
    book_capture: &BookCaptureSwitch,
) -> bool {
    match cmd {
        FeedCommand::LoadOlder { count } => {
            warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_LOAD_OLDER_UNSUPPORTED",
                symbol,
                requested = count,
                action = "answer_empty",
                "MT5 cannot page older history; the bridge only streams forward"
            );
            tx.send(FeedEvent::HistoryPrepended(Vec::new()))
                .await
                .is_ok()
        }
        FeedCommand::SetBookCapture {
            enabled,
            initial_generation,
        } => {
            if enabled {
                book_capture.enable(initial_generation);
            } else {
                book_capture.disable();
            }
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_CAPTURE_SET",
                symbol,
                enabled,
                initial_generation,
                action = if enabled { "publish_depth" } else { "stop_publishing_depth" },
                "book capture switched on the running bridge session"
            );
            true
        }
        FeedCommand::RestartBookCapture { initial_generation } => {
            // A fresh base generation retires whatever the previous one
            // published; the next image opens a new snapshot.
            book_capture.enable(initial_generation);
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_CAPTURE_RESTARTED",
                symbol,
                initial_generation,
                action = "resnapshot_on_next_image",
                "book capture restarted at a fresh generation"
            );
            true
        }
        // Transport commands belong to a recorded session; a live bridge has
        // no playhead to move. Ignored rather than refused — the UI only shows
        // the transport while a replay is the source.
        FeedCommand::Replay(_) => true,
    }
}

/// Surface bridge-connection transitions in the app's log stream.
fn log_status(symbol: &str, status: &Mt5Status) {
    match status {
        Mt5Status::Waiting { addr } => info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_WAITING_FOR_BRIDGE",
            symbol,
            addr = %addr,
            "waiting for the QuantickBridge EA (see bridge/mt5/README.md)"
        ),
        Mt5Status::Connected {
            symbol: hello_symbol,
            broker_symbol,
        } => info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_STREAMING",
            symbol,
            hello_symbol = %hello_symbol,
            broker_symbol = %broker_symbol,
            "bridge connected and streaming"
        ),
        Mt5Status::Lost { reason } => warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_LOST",
            symbol,
            reason = %reason,
            "bridge session ended; feed keeps listening"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt as _;

    /// Spawn the feed on an ephemeral port and return its handle.
    fn test_feed(symbol: &str) -> FeedHandle {
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:0".to_string(),
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridge; a second one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        spawn(symbol, &settings)
    }

    #[test]
    fn resolves_the_initial_backfill_immediately_and_answers_commands() {
        let mut feed = test_feed("WIN$N");

        // The UI contract: exactly one Backfilled reply, straight away.
        assert!(matches!(
            feed.events.blocking_recv(),
            Some(FeedEvent::Backfilled(trades)) if trades.is_empty()
        ));

        // Unsupported commands are answered, never left hanging.
        feed.commands
            .blocking_send(FeedCommand::LoadOlder { count: 100 })
            .unwrap();
        assert!(matches!(
            feed.events.blocking_recv(),
            Some(FeedEvent::HistoryPrepended(trades)) if trades.is_empty()
        ));
        feed.commands
            .blocking_send(FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation: 1,
            })
            .unwrap();
        feed.commands
            .blocking_send(FeedCommand::RestartBookCapture {
                initial_generation: 2,
            })
            .unwrap();
        // Book channel stays open and empty.
        assert!(matches!(
            feed.book_events.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn a_bridge_session_flows_history_then_live_into_feed_events() {
        // End-to-end inside the app layer: fake bridge over real TCP. A fixed
        // high port (no other test binds one) because the bound address stays
        // internal to the feed thread.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19171".to_string(),
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridge; a second one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(empty)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };
        assert!(empty.is_empty());

        // Give the listener a moment to bind, then connect as the bridge.
        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19171").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("could not reach the feed listener");

        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":-10800}\n",
            "{\"type\":\"backfill_start\",\"count_hint\":3}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"100\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"101\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"backfill_end\"}\n",
            "{\"type\":\"tick\",\"seq\":3,\"time_ms\":1002,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"100\",\"volume\":2,\"flags\":1080}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // History block: seq 1 honestly dropped (no context), seq 2 = buy.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for history")
            .expect("feed closed");
        let FeedEvent::HistoryPrepended(history) = event else {
            panic!("expected the bridge history as a prepend");
        };
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].agg_id, 2);

        // Live tick: downtick = sell.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for the live trade")
            .expect("feed closed");
        let FeedEvent::Live(trade) = event else {
            panic!("expected a live trade");
        };
        assert_eq!(trade.agg_id, 3);
        assert_eq!(trade.side, quantick_engine::Side::Sell);
    }

    #[tokio::test]
    async fn reconnect_history_overlap_is_dropped_not_double_counted() {
        // The bridge re-sends its recent-history window on every session; the
        // overlap with trades already forwarded must not inflate the bars.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19172".to_string(),
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridge; a second one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        async fn connect() -> tokio::net::TcpStream {
            for _ in 0..50 {
                match tokio::net::TcpStream::connect("127.0.0.1:19172").await {
                    Ok(s) => return s,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
            panic!("could not reach the feed listener");
        }

        fn tick(seq: u64, time_ms: i64, last: &str) -> String {
            format!(
                "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"bid\":\"0\",\
                 \"ask\":\"0\",\"last\":\"{last}\",\"volume\":1,\"flags\":1080}}\n"
            )
        }
        const HELLO: &str = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":-10800}\n",
        );

        // Session 1: history (1000, 1001) then a live tick at 1002.
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\",\"count_hint\":2}\n");
        script.push_str(&tick(1, 1000, "100"));
        script.push_str(&tick(2, 1001, "101"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        script.push_str(&tick(3, 1002, "100"));
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        let Some(FeedEvent::HistoryPrepended(history)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for history")
        else {
            panic!("expected the bridge history as a prepend");
        };
        assert_eq!(history.len(), 1);
        let Some(FeedEvent::Live(live)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the live trade")
        else {
            panic!("expected the live trade");
        };
        assert_eq!(live.timestamp_ms, 1002 + 10_800_000);

        // Session 2 (reconnect): the re-sent window overlaps everything the
        // UI already has, plus one genuinely new tick at 1003.
        drop(sock);
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\",\"count_hint\":4}\n");
        script.push_str(&tick(1, 1000, "100"));
        script.push_str(&tick(2, 1001, "101"));
        script.push_str(&tick(3, 1002, "100"));
        script.push_str(&tick(4, 1003, "102"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // Only the unseen tail arrives; the overlap is dropped, not replayed.
        let Some(FeedEvent::Live(fresh)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the post-reconnect trade")
        else {
            panic!("expected the unseen tail as a live trade");
        };
        assert_eq!(fresh.timestamp_ms, 1003 + 10_800_000);
        assert_eq!(fresh.agg_id, 4);
    }
}
