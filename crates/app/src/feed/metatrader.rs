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
//! - Order-book capture is unsupported (the UI already disables the heatmap
//!   for this provider); the depth channel stays open but forever empty.
//!
//! Synthetic ids caveat: `agg_id` restarts at 1 on every bridge session, so
//! ids may repeat across reconnects within one chart lifetime. Bars are built
//! from trade order, not ids, so the chart is unaffected; anything keying on
//! `agg_id` across sessions must not, and this is the place that documents it.

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, SideMode, run_bridge_server};

use crate::config::{MetaTraderSettings, Mt5SideSource};

use super::{DepthEvent, FeedCommand, FeedEvent, FeedHandle};

/// Start the MetaTrader feed for `symbol`: listen for the bridge on the
/// configured address and translate its stream into [`FeedEvent`]s.
#[must_use]
pub fn spawn(symbol: &str, settings: &MetaTraderSettings) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
    let (book_tx, book_rx) = mpsc::channel::<DepthEvent>(1);
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
    }
}

async fn feed_task(
    symbol: String,
    settings: MetaTraderSettings,
    tx: mpsc::Sender<FeedEvent>,
    _book_tx: mpsc::Sender<DepthEvent>, // held open, forever empty: no depth from MT5
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
                    Some(Mt5Event::Status(status)) => log_status(&symbol, &status),
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
                        idle_serve_commands(&symbol, &tx, &mut cmd_rx).await;
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
                        if !answer_command(&symbol, cmd, &tx).await {
                            break; // UI gone
                        }
                    }
                    None => break, // UI dropped the command sender: it's gone
                }
            }
        }
    }
    server.abort();
}

/// After a fatal listener error, keep answering UI commands honestly (empty
/// replies) so no loader ever spins forever on a dead feed.
async fn idle_serve_commands(
    symbol: &str,
    tx: &mpsc::Sender<FeedEvent>,
    cmd_rx: &mut mpsc::Receiver<FeedCommand>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        if !answer_command(symbol, cmd, tx).await {
            return;
        }
    }
}

/// Answer one UI command. Returns false when the UI is gone.
async fn answer_command(symbol: &str, cmd: FeedCommand, tx: &mpsc::Sender<FeedEvent>) -> bool {
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
            warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_CAPTURE_UNSUPPORTED",
                symbol,
                enabled,
                initial_generation,
                action = "ignore",
                "MT5 order-book capture is not implemented"
            );
            true
        }
        FeedCommand::RestartBookCapture { initial_generation } => {
            warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_CAPTURE_UNSUPPORTED",
                symbol,
                initial_generation,
                action = "ignore",
                "MT5 order-book capture is not implemented"
            );
            true
        }
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
