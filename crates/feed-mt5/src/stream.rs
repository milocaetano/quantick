//! The bridge server: a local TCP listener the QuantickBridge EA connects to.
//!
//! MQL5 sockets are client-only, so the roles are inverted versus a normal
//! exchange feed: *we* listen, the terminal dials out. One bridge connection
//! is served at a time; when it drops, the server goes back to waiting — the
//! UI hears about every transition through [`Mt5Event::Status`], so "nothing
//! is charting" always has a visible, logged reason.
//!
//! One port carries one symbol. Charting several MetaTrader symbols at once
//! means several of these servers, each on its own port with its own EA — so a
//! connection arriving while a session is being served is a setup mistake, and
//! [`refuse_busy`] answers it promptly instead of letting it sit in the accept
//! backlog where neither side can see it.
//!
//! Every noteworthy transition emits a structured `tracing` event with an
//! `event_code` (see the diagnosis table in the crate docs, `lib.rs`): an AI
//! or operator can reconstruct a session from logs alone.
//!
//! This file is the listener and the refusal path. The rest of a session is
//! owned one module at a time beside it: [`ports`] the two handles the
//! consumer holds, [`events`] what the server says, [`connection`] the
//! session loop, [`blocks`] the two state machines that outlive a line,
//! [`publish`] every send out, and [`reader`] the bounded line reader.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::map::SideMode;
use crate::protocol::{self, BridgeMsg};

mod blocks;
mod connection;
mod events;
mod ports;
mod publish;
mod reader;

pub use events::{Mt5Error, Mt5Event, Mt5Status};
pub use ports::{BookCaptureSwitch, HistoryPager};
pub use reader::{BoundedLine, BoundedLineReader};

use connection::serve_connection;
use events::ConnEnd;

/// Default address the feed listens on for the bridge.
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:9100";

/// Longest line the server will buffer. Protocol lines are a few hundred
/// bytes; anything larger is not the bridge, and an unbounded buffer would let
/// any local process exhaust memory by streaming bytes without a newline.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// How long a connection arriving mid-session may take to name itself before
/// it is closed.
///
/// A bridge sends its hello the moment it connects, so this only has to cover
/// a loopback round trip. It is deliberately far shorter than
/// [`ServerConfig::hello_timeout`]: that one waits on a bridge we intend to
/// serve, this one only buys the log a line saying *which* EA dialed the wrong
/// port. The refusal happens either way.
const BUSY_REFUSAL_WINDOW: Duration = Duration::from_millis(250);

/// Tape delay at or beyond this is reported once, and its return to health is
/// reported once more.
///
/// A second is already far outside what a local socket carrying a local
/// terminal's ticks should ever cost, and well inside the "delays of seconds
/// cannot happen" the chart exists to honour. It is an edge-triggered report,
/// not a per-sample one: a tape that stays late says so once and then stops
/// filling the log with the same sentence.
///
/// **Deliberately lower than the chart's own threshold.** quantick's status bar
/// colours the cell and names the hop at `metrics::HIGH_LAG_MS` (five seconds),
/// because a readout a trader glances at mid-session must not cry wolf. A log
/// nobody watches until something is wrong can afford to start earlier, and a
/// one-to-five-second spell is exactly the kind that is over before anyone
/// looks. The two are meant to differ; if either moves, read the other first.
const LAG_REPORT_MS: i64 = 1_000;

/// How the bridge server behaves for one symbol.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to listen on (the EA dials this).
    pub listen_addr: String,
    /// The symbol this feed expects (hello mismatches are refused).
    pub symbol: String,
    /// Aggressor-side policy for the mapper.
    pub side_mode: SideMode,
    /// How long a fresh connection may take to say hello.
    pub hello_timeout: Duration,
    /// Max silence (no ticks, no heartbeats) before the bridge is presumed
    /// dead. The bridge heartbeats every ~5 s; 30 s means six missed beats.
    pub read_timeout: Duration,
    /// Runtime switch for Depth of Market publication.
    pub book_capture: BookCaptureSwitch,
    /// The consumer's handle for asking a session for older ticks.
    pub history_pager: HistoryPager,
}

impl ServerConfig {
    /// Sensible defaults for `symbol` on [`DEFAULT_LISTEN_ADDR`], with depth
    /// capture off (it costs nothing until a consumer asks for it).
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            listen_addr: DEFAULT_LISTEN_ADDR.to_string(),
            symbol: symbol.into(),
            side_mode: SideMode::TickRule,
            hello_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            book_capture: BookCaptureSwitch::new(),
            history_pager: HistoryPager::new(),
        }
    }
}

/// Listen for the bridge and stream events until the consumer goes away.
///
/// Runs forever (accept → serve → back to waiting), returning `Ok(())` only
/// when the event receiver is dropped.
///
/// # Errors
///
/// Returns [`Mt5Error::Bind`] if the listen address cannot be bound.
pub async fn run_bridge_server(
    config: ServerConfig,
    tx: mpsc::Sender<Mt5Event>,
) -> Result<(), Mt5Error> {
    let listener = TcpListener::bind(&config.listen_addr).await.map_err(|e| {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BIND_FAILED",
            addr = %config.listen_addr,
            error = %e,
            "cannot bind the bridge listen address"
        );
        Mt5Error::Bind {
            addr: config.listen_addr.clone(),
            message: e.to_string(),
        }
    })?;
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| config.listen_addr.clone());
    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "MT5_LISTENING",
        addr = %bound,
        symbol = %config.symbol,
        "listening for the MT5 bridge"
    );

    // Every capture generation this server ever opens gets its own offset on
    // top of the consumer's base, so no reconnect and no mid-session resync
    // can reuse a generation a consumer already retired.
    let mut generation_offset: u64 = 0;
    // The single refusal slot (see `refuse_busy`): one at a time, so an EA
    // that reconnects in a loop cannot grow this server's task count.
    let mut refusal: Option<JoinHandle<()>> = None;

    loop {
        if tx
            .send(Mt5Event::Status(Mt5Status::Waiting {
                addr: bound.clone(),
            }))
            .await
            .is_err()
        {
            // Consumer gone before anyone connected.
            return shut_down(refusal.take());
        }

        let stream = match listener.accept().await {
            Ok((stream, peer)) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_CONNECTED",
                    peer = %peer,
                    "bridge socket connected; waiting for hello"
                );
                stream
            }
            Err(e) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_ACCEPT_FAILED",
                    error = %e,
                    "accept failed; continuing to listen"
                );
                continue;
            }
        };

        // Serve the session and keep accepting alongside it. Accepting is not
        // a second session — it is how anyone else who dials this port gets an
        // answer instead of a silence. Refusing runs on its own task, so the
        // served connection never waits on a stranger's read; `select!` may
        // pick the accept branch first on any given wakeup, and the served
        // future is pinned, so it is simply polled again on the next pass
        // rather than cancelled.
        let served = serve_connection(stream, &config, &tx, &mut generation_offset);
        tokio::pin!(served);
        let end = loop {
            tokio::select! {
                end = &mut served => break end,
                accepted = listener.accept() => match accepted {
                    Ok((extra, peer)) => refuse_busy(extra, peer, &config.symbol, &bound, &tx, &mut refusal),
                    Err(e) => warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_ACCEPT_FAILED",
                        error = %e,
                        "accept failed while serving a session; continuing to listen"
                    ),
                },
            }
        };

        match end {
            ConnEnd::UiGone => return shut_down(refusal.take()),
            ConnEnd::BridgeGone(reason) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_LOST",
                    reason = %reason,
                    "bridge session over; back to waiting"
                );
                if tx
                    .send(Mt5Event::Status(Mt5Status::Lost { reason }))
                    .await
                    .is_err()
                {
                    return shut_down(refusal.take());
                }
            }
        }
    }
}

/// End the server, taking any in-flight refusal down with it.
///
/// Closing a tab drops the consumer, and this now happens routinely rather
/// than only at exit. A refusal outliving the server it belongs to would hold
/// a socket open for the rest of its window, against a listener that is gone.
fn shut_down(refusal: Option<JoinHandle<()>>) -> Result<(), Mt5Error> {
    if let Some(task) = refusal {
        task.abort();
    }
    Ok(())
}

/// Turn away a connection that arrived while a session is being served.
///
/// One connection is one session (PROTOCOL.md), and this server serves one
/// symbol. A second EA dialing the same port is therefore always a setup
/// mistake — two charts pointed at one `InpPort` — and the only useful thing
/// to do with it is say so on both sides: the log names the intruder, and the
/// socket closes so its own reconnect logic reports the disconnect rather than
/// blocking on a send into a backlog nobody is reading.
///
/// `in_flight` bounds the work to **one refusal at a time**. The refusal is a
/// short read and a close, so a peer reconnecting on a five-second timer never
/// overlaps itself; a peer reconnecting faster than that gets closed unread
/// instead of being allowed to spawn tasks without limit.
fn refuse_busy(
    stream: TcpStream,
    peer: SocketAddr,
    serving: &str,
    addr: &str,
    tx: &mpsc::Sender<Mt5Event>,
    in_flight: &mut Option<JoinHandle<()>>,
) {
    if in_flight.as_ref().is_some_and(|task| !task.is_finished()) {
        let unread = PeerIdentity::UNREAD.diagnose(serving);
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_SESSION_BUSY",
            peer = %peer,
            addr = %addr,
            symbol = %serving,
            peer_said = PeerIdentity::UNREAD.said,
            diagnosis = unread.code,
            advice = unread.advice,
            action = "closed_unread",
            "another connection arrived while a refusal was still in flight; closing it unread"
        );
        return; // dropping `stream` closes it
    }
    let serving = serving.to_string();
    let addr = addr.to_string();
    let tx = tx.clone();
    *in_flight = Some(tokio::spawn(async move {
        let identity = identify(stream).await;
        let diagnosis = identity.diagnose(&serving);
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_SESSION_BUSY",
            peer = %peer,
            addr = %addr,
            symbol = %serving,
            peer_said = identity.said,
            peer_symbol = %identity.symbol.as_deref().unwrap_or("-"),
            diagnosis = diagnosis.code,
            advice = diagnosis.advice,
            window_ms = BUSY_REFUSAL_WINDOW.as_millis() as u64,
            action = "closed",
            "a second bridge dialed a port that is already serving a session; refusing it"
        );
        // And once where someone can see it. The chart otherwise says "waiting
        // for the bridge" for as long as the mistake lasts, while the reason
        // sits in a log file nobody has open.
        let _ = tx
            .send(Mt5Event::SessionBusy {
                peer: peer.to_string(),
                peer_symbol: identity.symbol,
                diagnosis: diagnosis.code,
                advice: diagnosis.advice,
            })
            .await;
    }));
}

/// What a refused connection managed to say about itself before being closed.
struct PeerIdentity {
    /// How its first line read: `hello`, `other_message`, `undecodable`,
    /// `nothing` (the window expired), `closed` (it hung up first), or
    /// `not_read` (a refusal was already in flight, so it was never given a
    /// window at all).
    said: &'static str,
    /// The symbol its hello declared, when it sent one. This is the field that
    /// turns "something dialed 9100" into "the XAUUSD chart's EA did".
    symbol: Option<String>,
}

/// What a refusal means for whoever has to fix it.
///
/// "Two bridges on one port" is not one mistake but three, and they need
/// opposite answers — telling someone to map a port when the real problem is a
/// duplicate chart of the same symbol sends them to edit a file that cannot
/// help. The distinguishing fact is whether the intruder streams the symbol
/// this server already serves.
struct BusyDiagnosis {
    /// Stable classification, for log queries.
    code: &'static str,
    /// What to actually do about it, in words.
    advice: &'static str,
}

impl PeerIdentity {
    /// A connection closed without being given a window, because one refusal
    /// was already in flight.
    const UNREAD: Self = Self {
        said: "not_read",
        symbol: None,
    };

    fn diagnose(&self, serving: &str) -> BusyDiagnosis {
        match self.symbol.as_deref() {
            Some(symbol) if symbol == serving => BusyDiagnosis {
                code: "same_symbol",
                advice: "a second EA is streaming this symbol from another chart; \
                         remove one of them",
            },
            Some(_) => BusyDiagnosis {
                code: "other_symbol",
                advice: "that symbol needs a port of its own; map it and set the \
                         matching InpPort on its chart",
            },
            // Nothing identified itself, so neither claim above is supported.
            // Saying only what is known beats guessing which fix applies.
            None => BusyDiagnosis {
                code: "unidentified",
                advice: "the peer never said what it streams; check which EA is \
                         pointed at this port",
            },
        }
    }
}

/// Read one line from `source` within [`BUSY_REFUSAL_WINDOW`], for the log.
/// The source is dropped when this returns — for a socket, that closes it.
///
/// Generic over the source for the same reason [`BoundedLineReader`] is: it
/// lets the classification be tested against bytes rather than against a
/// listener, which is the only way the "which EA is this?" decision gets
/// covered without a live socket in a unit test.
async fn identify<R: tokio::io::AsyncRead + Unpin>(source: R) -> PeerIdentity {
    let plain = |said| PeerIdentity { said, symbol: None };
    let mut lines = BoundedLineReader::new(source);
    match tokio::time::timeout(BUSY_REFUSAL_WINDOW, lines.next_line()).await {
        Err(_) => plain("nothing"),
        Ok(Err(_)) => plain("undecodable"),
        Ok(Ok(BoundedLine::Eof)) => plain("closed"),
        Ok(Ok(BoundedLine::TooLong | BoundedLine::NotUtf8 { .. })) => plain("undecodable"),
        Ok(Ok(BoundedLine::Line(line))) => match protocol::parse_line(&line) {
            Ok(BridgeMsg::Hello(hello)) => PeerIdentity {
                said: "hello",
                symbol: Some(hello.symbol),
            },
            Ok(_) => plain("other_message"),
            Err(_) => plain("undecodable"),
        },
    }
}

/// First 120 chars of a line, for log context without flooding. Truncates on
/// a char boundary: a byte-index slice would panic mid-codepoint.
fn snippet(line: &str) -> &str {
    match line.char_indices().nth(120) {
        Some((i, _)) => &line[..i],
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::{PeerIdentity, identify, snippet};

    /// Run `identify` over a fixed byte script, as the refusal path does over
    /// a socket.
    async fn identify_bytes(script: &str) -> PeerIdentity {
        identify(std::io::Cursor::new(script.as_bytes().to_vec())).await
    }

    fn hello_for(symbol: &str) -> String {
        format!(
            "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\
             \"symbol\":\"{symbol}\",\"broker_symbol\":\"{symbol}\",\"digits\":2,\
             \"server_utc_offset_s\":0}}\n"
        )
    }

    #[tokio::test]
    async fn a_refusal_advises_by_what_the_intruder_actually_streams() {
        // Two bridges on one port is three different mistakes, and the advice
        // has to follow the evidence. The same symbol twice is a duplicate
        // chart: telling that user to map a port would send them to edit a
        // file that cannot help them.
        let same = identify_bytes(&hello_for("XAUUSD")).await;
        assert_eq!(same.said, "hello");
        assert_eq!(same.symbol.as_deref(), Some("XAUUSD"));
        let same = same.diagnose("XAUUSD");
        assert_eq!(same.code, "same_symbol");
        assert!(same.advice.contains("another chart"), "{}", same.advice);
        assert!(
            !same.advice.contains("InpPort"),
            "there is no port to map here: {}",
            same.advice
        );

        // A different symbol is the port-mapping case, and the only one where
        // naming InpPort is the right instruction.
        let other = identify_bytes(&hello_for("US500")).await.diagnose("XAUUSD");
        assert_eq!(other.code, "other_symbol");
        assert!(other.advice.contains("InpPort"), "{}", other.advice);

        // Nothing identified itself: neither fix above is supported, so the
        // advice claims neither.
        let mute = identify_bytes("{\"type\":\"bye\",\"reason\":\"x\"}\n").await;
        assert_eq!(mute.said, "other_message");
        let garbage = identify_bytes("not json at all\n").await;
        assert_eq!(garbage.said, "undecodable");
        let hung_up = identify_bytes("").await;
        assert_eq!(hung_up.said, "closed");
        for unknown in [mute, garbage, hung_up, PeerIdentity::UNREAD] {
            let unknown = unknown.diagnose("XAUUSD");
            assert_eq!(unknown.code, "unidentified");
            assert!(unknown.advice.contains("never said"), "{}", unknown.advice);
        }
        // The one classification the reader never produces on its own: a peer
        // closed before it was given a window at all.
        assert_eq!(PeerIdentity::UNREAD.said, "not_read");
    }

    #[test]
    fn snippet_truncates_on_char_boundaries() {
        // 1 ASCII byte then two-byte chars: byte 120 falls mid-codepoint,
        // which the old byte slice panicked on.
        let line = format!("x{}", "é".repeat(200));
        assert_eq!(snippet(&line).chars().count(), 120);

        let short = "short line";
        assert_eq!(snippet(short), short);

        let exact: String = "a".repeat(120);
        assert_eq!(snippet(&exact), exact);
    }
}
