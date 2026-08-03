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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use quantick_feed_mt5::{
    BookCaptureSwitch, Mt5Error, Mt5Event, Mt5Status, ServerConfig, SideMode, TapeKind,
    run_bridge_server,
};

use crate::config::{FeedCapabilities, MetaTraderSettings, Mt5SideSource, ProviderKind};

use super::mt5_bridge::{Supervision, supervise};
use super::{DepthEvent, FeedCommand, FeedEvent, FeedHandle, FeedNotice};

/// Depth events are independent from the established trade channel. Sized like
/// the Binance backend's: a B3 book republishes far faster than the UI drains,
/// and this absorbs bursts without either dropping deltas or stalling trades.
const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;

/// Capacity of the notice channel. Notices are rare (one per connection
/// transition) and the UI drains every frame; this only has to outlast a
/// burst of bridge output arriving while the chart is busy.
const NOTICE_CHANNEL_CAPACITY: usize = 32;

/// Start the MetaTrader feed for `symbol`: listen for the bridge on the
/// configured address and translate its stream into [`FeedEvent`]s.
#[must_use]
pub fn spawn(symbol: &str, settings: &MetaTraderSettings) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
    let (book_tx, book_rx) = mpsc::channel::<DepthEvent>(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel::<FeedNotice>(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    // Until a bridge says hello, the provider's own answer stands: this
    // terminal usually streams an exchange contract with a book and a tape,
    // and the session narrows that the moment it knows better.
    let (caps_tx, caps_rx) = watch::channel(ProviderKind::MetaTrader.capabilities());
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
            runtime.block_on(feed_task(
                symbol, settings, tx, book_tx, notice_tx, caps_tx, cmd_rx,
            ));
        })
        .expect("spawn mt5 feed thread");
    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: notice_rx,
        capabilities: caps_rx,
        commands: cmd_tx,
        replay: None,
    }
}

/// What a session that just said hello can really offer.
///
/// The two facts the chart cannot observe for itself: whether this symbol has
/// a book, and whether it has a tape. Both come from the bridge, and both are
/// per-symbol — the same terminal serves a B3 contract with real depth and real
/// prints, and a broker CFD with neither.
fn session_capabilities(tape: TapeKind, book_levels: Option<u32>) -> FeedCapabilities {
    FeedCapabilities {
        book_capture: book_levels.is_some_and(|levels| levels > 0),
        // MT5 has no fetch-on-demand history: the bridge decides what it sends.
        history_paging: false,
        traded_volume: tape == TapeKind::Trades,
    }
}

#[allow(clippy::too_many_arguments)]
async fn feed_task(
    symbol: String,
    settings: MetaTraderSettings,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    caps_tx: watch::Sender<FeedCapabilities>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    // Resolve the UI's initial history load immediately: there is no
    // fetch-on-demand history on MT5. Bridge history arrives as a prepend.
    if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
        return; // UI gone
    }

    // One port carries one symbol, so where this feed listens is a question
    // about *this* symbol. Resolved once here and handed to both the listener
    // and the autostarted bridge, which is what keeps the two agreeing.
    let endpoint = settings.endpoint_for(&symbol);
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MT5_ENDPOINT_RESOLVED",
        symbol = %symbol,
        listen_addr = %endpoint.listen_addr,
        from_ports_map = endpoint.from_ports_map,
        "resolved this symbol's bridge port"
    );

    let mut server_cfg = ServerConfig::new(symbol.clone());
    server_cfg.listen_addr = endpoint.listen_addr.clone();
    server_cfg.side_mode = match settings.side_source {
        Mt5SideSource::TickRule => SideMode::TickRule,
        Mt5SideSource::Flags => SideMode::Flags,
    };

    // Selecting a MetaTrader feed is one action; starting a bridge by hand
    // afterwards was the second one this removes. The supervisor holds off
    // until it is sure nobody else is already feeding us.
    let bridge_connected = Arc::new(AtomicBool::new(false));
    let autostart = settings.bridge_autostart.then(|| {
        tokio::spawn(supervise(
            settings.clone(),
            Supervision {
                symbol: symbol.clone(),
                endpoint: endpoint.clone(),
                connected: Arc::clone(&bridge_connected),
                notices: notice_tx.clone(),
            },
        ))
    });
    if !settings.bridge_autostart {
        // Nothing is coming unless the user brings it: say so rather than
        // leaving an empty chart to be interpreted.
        let _ = notice_tx
            .send(FeedNotice::working(format!(
                "waiting for a MetaTrader bridge on {}",
                endpoint.listen_addr
            )))
            .await;
    }

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
                        // The connection's own story, told where the user is
                        // looking. A connected bridge clears whatever the
                        // startup reported; a lost one replaces it.
                        let notice = match &status {
                            Mt5Status::Connected { tape, book_levels, .. } => {
                                bridge_connected.store(true, Ordering::Relaxed);
                                // What this symbol really offers is known only
                                // now. Publishing it withdraws the affordances
                                // it cannot back — before the user clicks one.
                                let _ = caps_tx.send(session_capabilities(*tape, *book_levels));
                                FeedNotice::Connected
                            }
                            Mt5Status::Waiting { .. } => FeedNotice::working(
                                "waiting for the MetaTrader bridge to connect",
                            ),
                            // Losing the bridge clears the flag as well as
                            // reporting it. The supervisor reads the flag as
                            // "is one feeding us *now*", so a terminal that
                            // restarts mid-session gets picked back up —
                            // without this, "reconnecting" is a promise
                            // nobody keeps.
                            Mt5Status::Lost { .. } => {
                                bridge_connected.store(false, Ordering::Relaxed);
                                FeedNotice::reconnecting(
                                    "the MetaTrader bridge disconnected — reconnecting",
                                )
                            }
                        };
                        let _ = notice_tx.send(notice).await;
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
                            Ok(Err(e)) => {
                                error!(
                                    target: "quantick::app",
                                    schema_version = 1_u8,
                                    event_code = "MT5_BIND_FAILED",
                                    symbol = %symbol,
                                    listen_addr = %endpoint.listen_addr,
                                    from_ports_map = endpoint.from_ports_map,
                                    %e,
                                    "MT5 bridge listener failed; feed is idle (another quantick, \
                                     or another symbol already listening on this port?)"
                                );
                                // A port already taken is the ordinary failure
                                // once several symbols stream at once, and a
                                // log line is not where the user is looking.
                                let _ = notice_tx.send(bind_failure_notice(&symbol, &e)).await;
                            }
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

/// What to tell the user when this symbol's port could not be opened.
///
/// The cause worth naming is the one that arrived with multi-symbol charting:
/// two feeds asking for the same port. Whoever bound it first keeps streaming,
/// and the second chart would otherwise just sit there empty.
fn bind_failure_notice(symbol: &str, error: &Mt5Error) -> FeedNotice {
    let Mt5Error::Bind { addr, .. } = error;
    FeedNotice::attention(
        format!("quantick could not open the MetaTrader port for {symbol}"),
        format!(
            "Nothing can listen on {addr} — another quantick has it, or another symbol in \
             this one does. One port carries one symbol: give {symbol} its own under \
             [metatrader.ports] in your configuration, and set the matching InpPort on its \
             EA chart."
        ),
    )
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
            tape,
            book_levels,
        } => info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_STREAMING",
            symbol,
            hello_symbol = %hello_symbol,
            broker_symbol = %broker_symbol,
            // The two facts that decide what the chart may offer — right where
            // "why is the volume-bar option greyed out?" gets answered.
            tape = ?tape,
            book_levels = book_levels.unwrap_or(0),
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

    async fn notice_matching(
        notices: &mut mpsc::Receiver<FeedNotice>,
        predicate: impl Fn(&FeedNotice) -> bool,
    ) -> FeedNotice {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notice = notices.recv().await.expect("notice channel closed");
                if predicate(&notice) {
                    return notice;
                }
            }
        })
        .await
        .expect("timed out waiting for feed notice")
    }

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
    async fn a_mapped_symbol_listens_on_its_own_port_not_the_shared_one() {
        // The whole multi-symbol feature in one assertion: the port the map
        // names is the port a bridge finds. `listen_addr` deliberately points
        // somewhere else, so nothing here can pass by falling through to it.
        let mut ports = std::collections::BTreeMap::new();
        ports.insert("XAUUSD".to_string(), 19176_u16);
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19177".to_string(),
            ports,
            side_source: Mt5SideSource::TickRule,
            // This test is the bridge; a second one racing it would make the
            // assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("XAUUSD", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19176").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("nothing is listening on this symbol's mapped port");

        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"XAUUSD\",\"broker_symbol\":\"XAUUSD\",\"digits\":2,",
            "\"server_utc_offset_s\":10800}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"4000.00\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"4001.00\",\"volume\":1,\"flags\":1080}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // seq 1 has no tick-rule context; seq 2 is an uptick.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for the trade")
            .expect("feed closed");
        let FeedEvent::Live(trade) = event else {
            panic!("expected a live trade off the mapped port");
        };
        assert_eq!(trade.agg_id, 2);
        assert_eq!(trade.side, quantick_engine::Side::Buy);

        // And the shared default was never bound for this symbol.
        assert!(
            tokio::net::TcpStream::connect("127.0.0.1:19177")
                .await
                .is_err(),
            "a mapped symbol must not also occupy the shared listen_addr"
        );
    }

    #[test]
    fn a_port_that_will_not_open_names_the_port_and_the_way_out() {
        // The failure multi-symbol charting made ordinary: two feeds asking
        // for one port. It used to reach a log line only, leaving the second
        // chart blank with nothing on screen to act on.
        let FeedNotice::Attention {
            headline,
            next_step,
        } = bind_failure_notice(
            "US500",
            &Mt5Error::Bind {
                addr: "127.0.0.1:9102".to_string(),
                message: "address already in use".to_string(),
            },
        )
        else {
            panic!("a dead listener must ask for attention");
        };
        assert!(headline.contains("US500"), "headline: {headline}");
        assert!(next_step.contains("127.0.0.1:9102"), "step: {next_step}");
        assert!(
            next_step.contains("[metatrader.ports]") && next_step.contains("InpPort"),
            "the step names both halves of the pairing: {next_step}"
        );
    }

    #[test]
    fn a_session_reports_exactly_what_its_symbol_offers() {
        // An exchange contract: prints trades, publishes a book.
        let exchange = session_capabilities(TapeKind::Trades, Some(10));
        assert!(exchange.traded_volume);
        assert!(exchange.book_capture);

        // A broker-quoted CFD: neither. Both facts come from the same hello,
        // and neither is inferable from the provider being MetaTrader.
        let cfd = session_capabilities(TapeKind::Quotes, None);
        assert!(!cfd.traded_volume);
        assert!(!cfd.book_capture);

        // A bridge that subscribed to a DOM with no levels in it has no book
        // either — "declared" is not "has".
        assert!(!session_capabilities(TapeKind::Trades, Some(0)).book_capture);

        // Paging is never available on MT5, whatever the symbol is.
        assert!(!exchange.history_paging && !cfd.history_paging);
    }

    #[tokio::test]
    async fn a_quote_only_venue_withdraws_what_it_cannot_back() {
        // The Tickmill US500 case end to end: the bridge declares a venue that
        // prints nothing, and the chart's volume-based affordances have to go
        // dark before the user clicks one — while quotes still chart.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19173".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("US500", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };
        // Until a bridge says otherwise, the provider's optimistic answer holds.
        assert!(
            feed.capabilities.borrow().traded_volume,
            "the terminal usually streams a real tape; only this session says otherwise"
        );

        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19173").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("could not reach the feed listener");

        // Real US500 shape: quotes only, last "0.00", volume 0, flags BID|ASK,
        // and no book_levels at all.
        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"US500\",\"broker_symbol\":\"US500\",\"digits\":2,",
            "\"server_utc_offset_s\":10800,\"tape\":\"quotes\"}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"7447.81\",\"ask\":\"7448.11\",\"last\":\"0.00\",\"volume\":0,\"flags\":6}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"7447.82\",\"ask\":\"7448.12\",\"last\":\"0.00\",\"volume\":0,\"flags\":6}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // The quote charts: seq 1 has no tick-rule context, seq 2 is a buy at
        // the mid, one unit.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for the live print")
            .expect("feed closed");
        let FeedEvent::Live(trade) = event else {
            panic!("expected a live synthetic print");
        };
        assert_eq!(trade.agg_id, 2);
        assert_eq!(
            trade.price,
            rust_decimal::Decimal::from_str_exact("7447.97").unwrap()
        );
        assert_eq!(trade.quantity, rust_decimal::Decimal::ONE);

        // And the capability narrowed, so the toolbar's volume/dollar bars,
        // bubbles and heatmap disable themselves this frame.
        let caps = *feed.capabilities.borrow();
        assert!(!caps.traded_volume, "nothing here was ever traded");
        assert!(!caps.book_capture, "this symbol publishes no book");
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
        let connected = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Connected)
        })
        .await;
        assert_eq!(connected, FeedNotice::Connected);

        // Session 2 (reconnect): the re-sent window overlaps everything the
        // UI already has, plus one genuinely new tick at 1003.
        drop(sock);
        let reconnecting = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Reconnecting { headline } if headline.contains("disconnected"))
        })
        .await;
        assert!(matches!(reconnecting, FeedNotice::Reconnecting { .. }));
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
        let connected_again = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Connected)
        })
        .await;
        assert_eq!(connected_again, FeedNotice::Connected);
    }
}
