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

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use quantick_engine::{Bar, Trade};
use quantick_orderbook::{DepthEvent, DepthResyncReason, DepthStatus};

use crate::depth::BookMapper;
use crate::map::{MapOutcome, PriceContext, SideMode, TickMapper};
use crate::protocol::{self, BridgeMsg, FeedMsg, SCHEMA_VERSION, TapeKind};
use crate::rates::RateMapper;
use crate::session::SeqTracker;

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

/// Runtime switch controlling whether DOM images are published.
///
/// MT5's book shares one socket with ticks and MQL5 gives us no back-channel
/// to ask the terminal to stop sending it, so "capture off" is a decision made
/// here: images are decoded and dropped rather than published. That costs a
/// JSON parse per image and nothing downstream — no book state, no history, no
/// projection. The alternative (tearing down the bridge session) would take the
/// trade stream down with it.
///
/// Cloning shares the switch; the consumer flips it from any thread.
#[derive(Debug, Clone, Default)]
pub struct BookCaptureSwitch(Arc<BookCaptureState>);

#[derive(Debug, Default)]
struct BookCaptureState {
    enabled: AtomicBool,
    /// Base generation chosen by the consumer. Each bridge session adds its own
    /// offset on top, so a reconnect never reuses a generation.
    base_generation: AtomicU64,
}

impl BookCaptureSwitch {
    /// A switch that starts disabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish depth from `base_generation` onwards.
    ///
    /// A base above the previous one discards whatever the old generation
    /// published; consumers use that to keep stale in-flight events from an
    /// earlier capture out of fresh history.
    pub fn enable(&self, base_generation: u64) {
        self.0
            .base_generation
            .store(base_generation, Ordering::Relaxed);
        self.0.enabled.store(true, Ordering::Release);
    }

    /// Stop publishing depth. The bridge session and the trade stream continue.
    pub fn disable(&self) {
        self.0.enabled.store(false, Ordering::Release);
    }

    /// Whether depth is currently published.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.0.enabled.load(Ordering::Acquire)
    }

    fn state(&self) -> (bool, u64) {
        // Acquire on `enabled` pairs with the release in `enable`, so a true
        // read never sees a stale base generation.
        let enabled = self.0.enabled.load(Ordering::Acquire);
        (enabled, self.0.base_generation.load(Ordering::Relaxed))
    }
}

/// The consumer's one way to ask the terminal for ticks it has not sent — the
/// back-channel behind the chart's "load older" button.
///
/// Shaped like [`BookCaptureSwitch`]: a shared handle the consumer holds and
/// the running session reads, rather than a channel threaded through the
/// server's signature. The two ports differ in what they carry — a switch has
/// a state, a pager has a request — but not in how they dock, and a second
/// shared-state port that invented its own plumbing would be the harder one to
/// find.
///
/// **One request in flight per session.** A trader leaning on the button would
/// otherwise queue pages the terminal answers minutes later, each landing in
/// front of bars the earlier ones already drew. A request arriving while one is
/// outstanding is dropped and counted, exactly as the UI already drops a click
/// whose command channel is full.
#[derive(Debug, Clone, Default)]
pub struct HistoryPager(Arc<HistoryPagerState>);

#[derive(Debug, Default)]
struct HistoryPagerState {
    /// Queue and gate under **one** lock.
    ///
    /// Not a mutex for the request and an atomic for the gate: the consumer
    /// task and the session task run concurrently, and taking a request is
    /// "clear the queue *and* raise the gate" — one decision. Split across two
    /// primitives there is a window between them where a click reads a lowered
    /// gate, queues, and gets sent while the previous page is still coming.
    /// Locked once per click and once per session wake-up, never per tick.
    request: Mutex<PagerRequest>,
    /// Wakes the session loop, which is otherwise parked on the socket.
    wake: Notify,
}

/// What the pager is holding: at most one queued request, and whether a fetch
/// is already running.
#[derive(Debug, Default)]
struct PagerRequest {
    /// The queued request, if any: `(count, before_utc_ms)`.
    queued: Option<(u64, i64)>,
    /// Raised while the session is fetching, so a second click is dropped
    /// rather than queued behind the first.
    in_flight: bool,
}

impl HistoryPager {
    /// A pager with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask for up to `count` ticks from before `before_utc_ms`.
    ///
    /// The cursor is **UTC** — the timestamp of the oldest [`Trade`] the
    /// consumer holds, in the units it already has. The session converts it to
    /// the terminal's clock on the way out, where the live offset lives.
    ///
    /// Returns `false` when the request was dropped because one is already in
    /// flight — the caller's cue to leave its loading indicator alone rather
    /// than start a second one.
    pub fn request(&self, count: u64, before_utc_ms: i64) -> bool {
        {
            let mut held = self.0.request.lock().expect("history pager mutex");
            // Queued counts as busy, not just in flight. The session task runs
            // on another thread and may not have woken yet, so a click that
            // only checked the gate would quietly overwrite the previous
            // click's request — two asks, one answer, and a consumer counting
            // replies left one short forever.
            if held.in_flight || held.queued.is_some() {
                return false;
            }
            held.queued = Some((count, before_utc_ms));
        }
        self.0.wake.notify_one();
        true
    }

    /// Whether a request is outstanding.
    #[must_use]
    pub fn is_in_flight(&self) -> bool {
        self.0
            .request
            .lock()
            .expect("history pager mutex")
            .in_flight
    }

    /// Park until a request arrives, then take it.
    ///
    /// Cancel-safe: [`Notify::notified`] and the take are separate steps, and a
    /// notification that arrives before the wait does is remembered, so losing
    /// this future to a `select!` cannot lose a click. Marks the request in
    /// flight before returning it — the session is the only caller, and it is
    /// about to write to the socket.
    async fn take_request(&self) -> (u64, i64) {
        loop {
            {
                let mut held = self.0.request.lock().expect("history pager mutex");
                // Take and raise together. A click landing between the two
                // would otherwise see a lowered gate and queue behind a page
                // already on the wire.
                if let Some(request) = held.queued.take() {
                    held.in_flight = true;
                    return request;
                }
            }
            self.0.wake.notified().await;
        }
    }

    /// The fetch is over (delivered, refused, or abandoned). Clears the gate so
    /// the next click is heard, and reports whether a fetch was actually
    /// running.
    ///
    /// The report is what tells a caller whether it still owes a reply. Callers
    /// that already know they do can ignore it.
    fn settle_owed(&self) -> bool {
        let mut held = self.0.request.lock().expect("history pager mutex");
        let owed = held.in_flight;
        held.in_flight = false;
        owed
    }

    /// Clear the pager at the end of a session, and say whether anything was
    /// owed an answer.
    ///
    /// Both halves matter. The return value keeps the promise of one reply per
    /// request — a request still *queued* when the socket died was never taken,
    /// so the gate alone would miss it and leave a spinner running. And the
    /// clearing is what keeps that promise from being kept twice: the caller
    /// answers whatever this reports, so a request left behind would be served
    /// again by the next session and arrive as a second reply to a click the
    /// consumer already saw resolved.
    fn abandon(&self) -> bool {
        let mut held = self.0.request.lock().expect("history pager mutex");
        let owed = held.queued.take().is_some() || held.in_flight;
        held.in_flight = false;
        owed
    }
}

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
enum ConnEnd {
    /// The consumer dropped the event channel: shut the server down.
    UiGone,
    /// The bridge went away (reason for the status event); keep listening.
    BridgeGone(String),
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

/// Trades one paged block may hold before it stops growing.
///
/// The bridge is the side this crate cannot vouch for — the same reasoning
/// behind [`MAX_BARS_PER_BLOCK`] for candles, and more pressing here: a candle
/// block arrives once per session, a page can be opened again on every click.
/// A peer that opens `history_start` and never stops would otherwise grow this
/// vector until the feed task dies and takes the chart with it.
///
/// Sized well above any honest page: the consumer's own step tops out at 50 000
/// (the toolbar's `DragValue` range) and the Python bridge caps itself at
/// 200 000, so a block reaching this ceiling is a bridge that is not answering
/// the question it was asked.
const MAX_TRADES_PER_PAGE: usize = 250_000;

/// A page of older ticks under construction, and the live tape's tick-rule
/// context waiting for it to finish.
///
/// The context travels with the block rather than in a variable beside it so
/// the two cannot be separated: every path that takes the block back also gets
/// the context to put back, including the ones that discard it.
struct PagedBlock {
    /// The mapped trades collected so far.
    trades: Vec<Trade>,
    /// What the tick rule was reading before the block opened.
    resume: PriceContext,
    /// Trades dropped because the block hit [`MAX_TRADES_PER_PAGE`]. Reported
    /// once at the end rather than per tick, so a runaway bridge cannot turn
    /// the log into the second denial of service.
    over_cap: u64,
}

/// What woke the session loop: something the bridge said, or something the
/// trader asked for.
///
/// The two arrive on opposite halves of the same socket and the loop has to
/// wait on both at once; naming them lets the wait stay one `select!` with one
/// timeout rather than two loops racing over one reader.
enum SessionInput {
    /// A line from the bridge (or the error that ended the read).
    Line(std::io::Result<BoundedLine>),
    /// The consumer wants ticks older than `before_utc_ms`.
    Request {
        /// How many ticks to ask for.
        count: u64,
        /// Oldest UTC millisecond the consumer holds; the request is for
        /// strictly older than this.
        before_utc_ms: i64,
    },
}

/// What happened to one attempt to ask the bridge for older ticks.
enum PageRequestOutcome {
    /// Written to the socket; the block will arrive on the read side.
    Sent,
    /// This bridge does not read its socket, so nothing was written.
    Refused,
    /// The write failed; the session is over.
    WriteFailed(std::io::Error),
}

/// Put one request for older ticks on the wire.
///
/// Refuses rather than writes when the bridge never declared it reads: an
/// unread request would sit in the peer's receive buffer, and on the Expert
/// Advisor a full buffer eventually blocks the terminal thread that sends
/// ticks — so a chart asking for history would stop the chart.
async fn answer_page_request(
    outgoing: &mut tokio::net::tcp::OwnedWriteHalf,
    symbol: &str,
    mapper: &TickMapper,
    can_page: bool,
    count: u64,
    before_utc_ms: i64,
) -> PageRequestOutcome {
    if !can_page {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_LOAD_OLDER_UNSUPPORTED",
            symbol,
            requested = count,
            action = "answer_empty",
            advice = "update the bridge (bridge/mt5/quantick_bridge.py) to page older ticks",
            "this bridge session does not answer requests for older ticks"
        );
        return PageRequestOutcome::Refused;
    }
    // The terminal stamps everything in server time, so the cursor crosses in
    // its clock; the mapper owns that offset and its heartbeat refreshes.
    let before_ms = mapper.to_server_ms(before_utc_ms);
    let line = protocol::encode_line(&FeedMsg::LoadOlder { count, before_ms });
    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "MT5_LOAD_OLDER_REQUESTED",
        symbol,
        requested = count,
        before_ms,
        before_utc_ms,
        "asking the terminal for ticks older than the chart's oldest"
    );
    match outgoing.write_all(line.as_bytes()).await {
        Ok(()) => PageRequestOutcome::Sent,
        Err(e) => PageRequestOutcome::WriteFailed(e),
    }
}

/// Serve one bridge connection to completion.
///
/// `generation_offset` is the server-wide capture-generation cursor; this
/// function advances it whenever depth capture needs a fresh generation.
async fn serve_connection(
    stream: TcpStream,
    config: &ServerConfig,
    tx: &mpsc::Sender<Mt5Event>,
    generation_offset: &mut u64,
) -> ConnEnd {
    // Split so the session can write while parked on a read. The write half is
    // used at most once per trader click; the read half carries every tick.
    let (incoming, mut outgoing) = stream.into_split();
    let mut lines = BoundedLineReader::new(incoming);

    // 1. The first message must be a hello that matches what we expect.
    let hello = match tokio::time::timeout(config.hello_timeout, lines.next_line()).await {
        Err(_) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HELLO_TIMEOUT",
                timeout_s = config.hello_timeout.as_secs(),
                "connection said nothing; dropping it"
            );
            return ConnEnd::BridgeGone("hello timeout".to_string());
        }
        Ok(Err(e)) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_SOCKET_ERROR",
                error = %e,
                "socket error before hello; dropping the connection"
            );
            return ConnEnd::BridgeGone(format!("socket error before hello: {e}"));
        }
        Ok(Ok(BoundedLine::Eof)) => {
            info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_EOF",
                "connection closed before hello"
            );
            return ConnEnd::BridgeGone("closed before hello".to_string());
        }
        Ok(Ok(BoundedLine::TooLong)) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_LINE_TOO_LONG",
                max_bytes = MAX_LINE_BYTES as u64,
                "first line exceeded the size cap; dropping the connection"
            );
            return ConnEnd::BridgeGone("oversized hello".to_string());
        }
        Ok(Ok(BoundedLine::NotUtf8 { len })) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_UNDECODABLE_LINE",
                error = "invalid utf-8",
                line_bytes = len as u64,
                "first line was not valid protocol; dropping the connection"
            );
            return ConnEnd::BridgeGone("undecodable hello".to_string());
        }
        Ok(Ok(BoundedLine::Line(line))) => match protocol::parse_line(&line) {
            Ok(BridgeMsg::Hello(h)) => h,
            Ok(other) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    got = ?other,
                    "first message was not a hello; dropping the connection"
                );
                return ConnEnd::BridgeGone("no hello".to_string());
            }
            Err(e) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = %e,
                    snippet = %snippet(&line),
                    "first line was not valid protocol; dropping the connection"
                );
                return ConnEnd::BridgeGone("undecodable hello".to_string());
            }
        },
    };

    if hello.schema != SCHEMA_VERSION {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_SCHEMA_MISMATCH",
            bridge_schema = hello.schema,
            our_schema = SCHEMA_VERSION,
            bridge = %hello.bridge,
            bridge_version = %hello.bridge_version,
            "bridge speaks a different protocol version; refusing"
        );
        return ConnEnd::BridgeGone(format!("schema mismatch (bridge {})", hello.schema));
    }
    if hello.symbol != config.symbol {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_SYMBOL_MISMATCH",
            expected = %config.symbol,
            got = %hello.symbol,
            "bridge streams a different symbol than configured; refusing"
        );
        return ConnEnd::BridgeGone(format!("symbol mismatch ({})", hello.symbol));
    }

    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "MT5_HELLO_OK",
        bridge = %hello.bridge,
        bridge_version = %hello.bridge_version,
        symbol = %hello.symbol,
        broker_symbol = %hello.broker_symbol,
        digits = hello.digits,
        server_utc_offset_s = hello.server_utc_offset_s,
        tape = ?hello.tape,
        "bridge session established"
    );
    // Absent means no: writing to a bridge that never reads would fill its
    // receive buffer and, on the EA, block the terminal thread that sends ticks.
    let can_page = hello.history_paging.unwrap_or(false);
    if tx
        .send(Mt5Event::Status(Mt5Status::Connected {
            symbol: hello.symbol.clone(),
            broker_symbol: hello.broker_symbol.clone(),
            tape: hello.tape,
            book_levels: hello.book_levels,
            rates: hello.rates.unwrap_or(false),
            history_paging: can_page,
        }))
        .await
        .is_err()
    {
        return ConnEnd::UiGone;
    }
    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = if can_page {
            "MT5_HISTORY_PAGING_AVAILABLE"
        } else {
            "MT5_HISTORY_PAGING_UNSUPPORTED"
        },
        symbol = %hello.symbol,
        bridge = %hello.bridge,
        bridge_version = %hello.bridge_version,
        advice = if can_page {
            "-"
        } else {
            "update the bridge (bridge/mt5/quantick_bridge.py) to page older ticks"
        },
        "whether this session can be asked for ticks older than it sent"
    );

    // 2. Stream messages until something ends the session.
    //
    // The bridge declares what its venue prints; the configured `side_mode` is
    // a policy, `tape` is a fact about the instrument, and a fact the feed
    // cannot observe for itself must come from the side that can see it.
    let mut mapper =
        TickMapper::new(config.side_mode, hello.server_utc_offset_s).with_tape(hello.tape);
    let mut tracker = SeqTracker::new();
    let mut backfill: Option<Vec<Trade>> = None;
    let mut candles: Option<RatesBlock> = None;
    let mut undecodable: u64 = 0;
    let mut depth = DepthSession::new(&hello, config.symbol.clone());
    depth.log_capability();
    // The paged block being collected, if the bridge is mid-answer, and the
    // live tape's tick-rule context parked for the duration.
    let mut page: Option<PagedBlock> = None;

    // Parked here across the whole session rather than rebuilt each pass. The
    // read loop polls this once per inbound line, and a future recreated every
    // time would re-register with the `Notify` on every tick — paying a
    // synchronization cost per print for something that fires when a trader
    // clicks. Pinned once, a poll is a load and a return.
    let pending_request = config.history_pager.take_request();
    tokio::pin!(pending_request);

    let end = loop {
        // One wait covers both directions, under the bridge-liveness timeout.
        //
        // The timeout is rebuilt each pass, so a wake from *either* branch
        // restarts it: a click buys a silent bridge one more `read_timeout`
        // before it is declared lost. That is a bounded and rare extension — a
        // click is a human action, and the pager allows one outstanding at a
        // time — not an indefinite one, which is why the timeout stays out here
        // rather than being tracked against the read alone.
        let input = tokio::time::timeout(config.read_timeout, async {
            tokio::select! {
                // Biased so a busy tape cannot starve the click: this branch is
                // ready at most once per trader action, the read branch on
                // nearly every pass.
                biased;
                (count, before_utc_ms) = &mut pending_request => {
                    pending_request.set(config.history_pager.take_request());
                    SessionInput::Request { count, before_utc_ms }
                }
                // `next_line` is cancel-safe, and the reader outlives this
                // `select!` so the state it keeps is still there next pass.
                // Its awaits are all `fill_buf`, and every byte it takes from
                // the `BufReader` is appended to the partial-line buffer in the
                // same synchronous step that consumes it — including the
                // multi-`fill_buf` path a line longer than the buffer takes.
                // So a cancelled poll leaves the two exactly as consistent as
                // an uncancelled one: nothing consumed is unrecorded, and
                // nothing recorded is unconsumed.
                line = lines.next_line() => SessionInput::Line(line),
            }
        })
        .await;

        let line = match input {
            Err(_) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_SILENT",
                    timeout_s = config.read_timeout.as_secs(),
                    "no ticks or heartbeats within the timeout; presuming the bridge dead"
                );
                break ConnEnd::BridgeGone("silent".to_string());
            }
            Ok(SessionInput::Request {
                count,
                before_utc_ms,
            }) => {
                match answer_page_request(
                    &mut outgoing,
                    &config.symbol,
                    &mapper,
                    can_page,
                    count,
                    before_utc_ms,
                )
                .await
                {
                    // Asked. The answer arrives as an ordinary block of lines
                    // on the read side, like every other thing the bridge says.
                    PageRequestOutcome::Sent => continue,
                    // Nothing to ask: this bridge cannot page. The consumer is
                    // still owed the one reply every request gets, or its
                    // spinner runs forever.
                    PageRequestOutcome::Refused => {
                        config.history_pager.settle_owed();
                        if tx
                            .send(Mt5Event::HistoryPage {
                                trades: Vec::new(),
                                exhausted: false,
                                scanned_to_utc_ms: None,
                            })
                            .await
                            .is_err()
                        {
                            break ConnEnd::UiGone;
                        }
                        continue;
                    }
                    // A socket that cannot be written to cannot be read from
                    // either; end the session and let the reconnect handle it.
                    // The owed reply is sent by the tail below, which covers
                    // every way a session can end while a page is outstanding.
                    PageRequestOutcome::WriteFailed(e) => {
                        break ConnEnd::BridgeGone(format!("socket error on write: {e}"));
                    }
                }
            }
            Ok(SessionInput::Line(Err(e))) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_SOCKET_ERROR",
                    error = %e,
                    "socket error; dropping the session"
                );
                break ConnEnd::BridgeGone(format!("socket error: {e}"));
            }
            Ok(SessionInput::Line(Ok(BoundedLine::Eof))) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_EOF",
                    "bridge closed the socket"
                );
                break ConnEnd::BridgeGone("eof".to_string());
            }
            Ok(SessionInput::Line(Ok(BoundedLine::TooLong))) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_LINE_TOO_LONG",
                    max_bytes = MAX_LINE_BYTES as u64,
                    "peer streamed an oversized line; dropping the session"
                );
                break ConnEnd::BridgeGone("oversized line".to_string());
            }
            Ok(SessionInput::Line(Ok(BoundedLine::NotUtf8 { len }))) => {
                undecodable += 1;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = "invalid utf-8",
                    line_bytes = len as u64,
                    total_undecodable = undecodable,
                    "skipping an undecodable line"
                );
                continue;
            }
            Ok(SessionInput::Line(Ok(BoundedLine::Line(line)))) => line,
        };
        if line.trim().is_empty() {
            continue;
        }

        match protocol::parse_line(&line) {
            Err(e) => {
                undecodable += 1;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = %e,
                    snippet = %snippet(&line),
                    total_undecodable = undecodable,
                    "skipping an undecodable line"
                );
            }
            Ok(BridgeMsg::Tick(tick)) => {
                let _ = tracker.observe(tick.seq);
                if let MapOutcome::Trade { trade, .. } = mapper.map(&tick) {
                    // A tick belongs to whichever block is open around it. The
                    // paged block is checked first because it is the one that
                    // can open mid-session: everything else is live by default,
                    // and a paged tick delivered as live would append history to
                    // the front of the tape.
                    match (page.as_mut(), backfill.as_mut()) {
                        (Some(block), _) => {
                            if block.trades.len() < MAX_TRADES_PER_PAGE {
                                block.trades.push(trade);
                            } else {
                                block.over_cap = block.over_cap.saturating_add(1);
                            }
                        }
                        (None, Some(buf)) => buf.push(trade),
                        (None, None) => {
                            if tx.send(Mt5Event::Live(trade)).await.is_err() {
                                break ConnEnd::UiGone;
                            }
                        }
                    }
                }
            }
            Ok(BridgeMsg::Book(image)) => {
                match depth
                    .observe(image, &config.book_capture, generation_offset, tx)
                    .await
                {
                    Ok(()) => {}
                    Err(()) => break ConnEnd::UiGone,
                }
            }
            Ok(BridgeMsg::Heartbeat(hb)) => {
                if let Some(offset) = hb.server_utc_offset_s {
                    mapper.set_server_utc_offset_s(offset);
                    depth.set_server_utc_offset_s(offset);
                }
                // A heartbeat is the natural moment to notice that a consumer
                // is waiting for depth this bridge cannot send.
                if depth
                    .report_missing_capability(&config.book_capture, tx)
                    .await
                    .is_err()
                {
                    break ConnEnd::UiGone;
                }
                debug!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_HEARTBEAT",
                    seq_last = hb.seq_last,
                    ticks_sent = hb.ticks_sent,
                    "bridge heartbeat"
                );
            }
            Ok(BridgeMsg::BackfillStart { count_hint }) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BACKFILL_START",
                    count_hint = ?count_hint,
                    "bridge is sending history"
                );
                backfill = Some(Vec::new());
            }
            Ok(BridgeMsg::BackfillEnd {}) => {
                let batch = backfill.take().unwrap_or_default();
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BACKFILL_END",
                    trades = batch.len(),
                    "history block complete"
                );
                if tx.send(Mt5Event::Backfilled(batch)).await.is_err() {
                    break ConnEnd::UiGone;
                }
            }
            Ok(BridgeMsg::HistoryStart { count_hint }) => {
                if !config.history_pager.is_in_flight() {
                    // Nobody asked. Collect it anyway rather than letting its
                    // ticks fall through as live prints — the block is history,
                    // and charting it at the front of the tape is the one
                    // outcome worse than dropping it. Logged under the same
                    // code as the matching end: one condition, one thing to
                    // grep for, both halves of the event findable.
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_HISTORY_PAGE_UNSOLICITED",
                        action = "collect_and_discard",
                        "history_start with no request outstanding"
                    );
                }
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_HISTORY_PAGE_START",
                    count_hint = ?count_hint,
                    "bridge is sending a page of older ticks"
                );
                // A block already open means the bridge restarted mid-send.
                // Put the live context back before parking it again, or the
                // second start would park an already-emptied one and the tape
                // would never get its price back.
                if let Some(stale) = page.take() {
                    mapper.restore_price_context(stale.resume);
                }
                // The page's ticks are hours older than the live tape; the tick
                // rule must read them against each other, not against the
                // newest price on the chart.
                page = Some(PagedBlock {
                    trades: Vec::new(),
                    resume: mapper.take_price_context(),
                    over_cap: 0,
                });
            }
            Ok(BridgeMsg::HistoryEnd {
                exhausted,
                scanned_to_ms,
            }) => {
                let Some(PagedBlock {
                    trades,
                    resume,
                    over_cap,
                }) = page.take()
                else {
                    // The start went missing — an undecodable line ahead of it
                    // is enough. The block's ticks have already been charted as
                    // live, which cannot be undone here; what must not also
                    // happen is the pager staying latched. Without settling,
                    // `request` refuses every later click for the rest of the
                    // session and the button silently stops working.
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_PROTOCOL_VIOLATION",
                        action = "settle_and_answer_empty",
                        "history_end without a history_start; its ticks went out as live"
                    );
                    if config.history_pager.settle_owed()
                        && tx
                            .send(Mt5Event::HistoryPage {
                                trades: Vec::new(),
                                exhausted: false,
                                scanned_to_utc_ms: None,
                            })
                            .await
                            .is_err()
                    {
                        break ConnEnd::UiGone;
                    }
                    continue;
                };
                // The live tape resumes from the price it left off at, never
                // from wherever the page ended.
                mapper.restore_price_context(resume);
                if over_cap > 0 {
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_HISTORY_PAGE_TRUNCATED",
                        cap = MAX_TRADES_PER_PAGE as u64,
                        dropped = over_cap,
                        "a page exceeded the per-block cap; the surplus was dropped"
                    );
                }
                if !config.history_pager.settle_owed() {
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_HISTORY_PAGE_UNSOLICITED",
                        trades = trades.len(),
                        action = "discard",
                        "a page nobody asked for; discarding it rather than prepending"
                    );
                    continue;
                }
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_HISTORY_PAGE_END",
                    trades = trades.len(),
                    exhausted,
                    scanned_to_ms = ?scanned_to_ms,
                    "page of older ticks complete"
                );
                // Converted here, where the offset lives, for the same reason
                // the outbound cursor is: the consumer speaks UTC and this is
                // the one place that tracks what the terminal's clock is doing.
                let scanned_to_utc_ms = scanned_to_ms.map(|ms| mapper.to_utc_ms(ms));
                if tx
                    .send(Mt5Event::HistoryPage {
                        trades,
                        exhausted,
                        scanned_to_utc_ms,
                    })
                    .await
                    .is_err()
                {
                    break ConnEnd::UiGone;
                }
            }
            Ok(BridgeMsg::RatesStart {
                interval_ms,
                count_hint,
            }) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_RATES_START",
                    interval_ms,
                    count_hint = ?count_hint,
                    "bridge is sending historical candles"
                );
                if let Some(open) = candles.take() {
                    // A block already running: the bridge restarted mid-send,
                    // or two are interleaved. Either way what was collected
                    // belongs to a window this new header does not describe.
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_PROTOCOL_VIOLATION",
                        bars = open.len(),
                        action = "discard_open_block",
                        "a second rates_start arrived inside an open block; discarding the first"
                    );
                }
                candles = Some(RatesBlock::new(interval_ms, hello.server_utc_offset_s));
            }
            Ok(BridgeMsg::Rate(chunk)) => match candles.as_mut() {
                Some(block) => block.absorb(&chunk),
                // A batch outside a block has no interval to be measured in,
                // and guessing one would misdate every bar in it.
                None => warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    bars = chunk.bars.len(),
                    action = "drop_chunk",
                    "candles arrived outside a rates block; dropping them"
                ),
            },
            Ok(BridgeMsg::RatesEnd { partial }) => match candles.take() {
                Some(block) => {
                    let (interval_ms, bars, clipped) = block.finish(&config.symbol);
                    // Either side may know the block is short: the bridge from
                    // its paging, this decoder from its own cap. Kept apart in
                    // the log — they point at different things to go fix.
                    let bridge_said_partial = partial;
                    let partial = bridge_said_partial || clipped;
                    if partial {
                        warn!(
                            target: "quantick::feed",
                            schema_version = 1_u8,
                            event_code = "MT5_RATES_PARTIAL",
                            symbol = %config.symbol,
                            bars = bars.len(),
                            bridge_said_partial,
                            clipped_here = clipped,
                            "the candle block is short of what was asked for"
                        );
                    }
                    if tx
                        .send(Mt5Event::Rates {
                            interval_ms,
                            bars,
                            partial,
                        })
                        .await
                        .is_err()
                    {
                        break ConnEnd::UiGone;
                    }
                }
                None => warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    action = "ignore",
                    "rates_end without a rates_start; ignoring it"
                ),
            },
            Ok(BridgeMsg::Bye { reason }) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_BYE",
                    reason = %reason,
                    "bridge said goodbye"
                );
                break ConnEnd::BridgeGone(format!("bye: {reason}"));
            }
            Ok(BridgeMsg::Hello(_)) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    "second hello mid-session; ignoring it"
                );
            }
        }
    };

    if backfill.is_some() {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_PARTIAL_BACKFILL_DISCARDED",
            "session ended mid-backfill; discarding the incomplete block"
        );
    }
    // Every request gets exactly one reply, including the ones this session
    // died holding — whether it had been sent to the bridge or was still
    // queued. The alternative is a spinner that outlives the connection it was
    // waiting on. `abandon` also clears the request itself: it belongs to the
    // connection that is ending, and replaying it against the next session
    // would page from a cursor that one never sent.
    if config.history_pager.abandon() {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_HISTORY_PAGE_UNANSWERED",
            partial_trades = page.as_ref().map_or(0, |block| block.trades.len()),
            action = "answer_empty",
            "session ended with a page outstanding; answering it empty"
        );
        let _ = tx
            .send(Mt5Event::HistoryPage {
                trades: Vec::new(),
                exhausted: false,
                scanned_to_utc_ms: None,
            })
            .await;
    }
    if let Some(block) = candles {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_PARTIAL_RATES_DISCARDED",
            bars = block.len(),
            "session ended mid-candle-block; discarding it (the next session re-sends)"
        );
    }
    mapper.stats.log_summary(&config.symbol);
    // A consumer that was capturing depth must hear that this generation ended,
    // so it renders the discontinuity instead of connecting liquidity across it.
    depth.close(tx).await;
    end
}

/// Most candles one block may deliver.
///
/// The bridge caps what it sends (`--rates-max-bars`) and logs the shortfall,
/// but the bridge is the side this one cannot vouch for: a misconfigured or
/// hostile one could stream candles until the feed runs out of memory. Ninety
/// days of one-minute buckets is ~130 000, so this is comfortably above any
/// legitimate block while still being a bound.
const MAX_BARS_PER_BLOCK: usize = 1_000_000;

/// The historical candle block being received, between `rates_start` and
/// `rates_end`.
///
/// Bars land in a [`BTreeMap`] keyed by `open_time` rather than a `Vec`: the
/// terminal can repeat a bucket across chunk boundaries, and a map both settles
/// that (last write wins — a repeat is a correction) and hands back one
/// ascending series without a sort whose tie-breaking would be an unstated
/// rule. Same block in, same series out, whatever order the chunks arrived in.
struct RatesBlock {
    interval_ms: i64,
    mapper: RateMapper,
    bars: BTreeMap<i64, Bar>,
    /// Whether the cap has been hit, so it is reported once rather than per row.
    truncated: bool,
}

impl RatesBlock {
    fn new(interval_ms: i64, server_utc_offset_s: i64) -> Self {
        Self {
            interval_ms,
            mapper: RateMapper::new(interval_ms, server_utc_offset_s),
            bars: BTreeMap::new(),
            truncated: false,
        }
    }

    /// Map and absorb one chunk. Unreadable rows are counted, not fatal: one
    /// corrupt candle in ninety days is a gap, not a reason to lose the block.
    fn absorb(&mut self, chunk: &protocol::RateChunk) {
        for row in &chunk.bars {
            if self.bars.len() >= MAX_BARS_PER_BLOCK {
                if !self.truncated {
                    self.truncated = true;
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_RATES_TRUNCATED",
                        max_bars = MAX_BARS_PER_BLOCK as u64,
                        action = "keep_oldest_stop_absorbing",
                        "the candle block exceeded the cap; ignoring the rest of it"
                    );
                }
                return;
            }
            if let Some(bar) = self.mapper.map(row) {
                self.bars.insert(bar.open_time, bar);
            }
        }
    }

    fn len(&self) -> usize {
        self.bars.len()
    }

    /// Close the block: log what it cost, and hand back the ascending series.
    fn finish(self, symbol: &str) -> (i64, Vec<Bar>, bool) {
        self.mapper.stats.log_summary(symbol, self.interval_ms);
        // Clipping here is the same kind of shortfall the bridge reports with
        // its own `partial`: bars that exist and were not delivered.
        let clipped = self.truncated;
        (self.interval_ms, self.bars.into_values().collect(), clipped)
    }
}

/// Depth capture state for one bridge connection.
///
/// Split out because it is the only stateful thing in the message loop besides
/// tick mapping, and it must stay correct across three independent events: the
/// consumer toggling capture, the terminal losing images, and the session
/// ending.
struct DepthSession {
    symbol: String,
    /// `None` when the bridge declared no Depth of Market support.
    mapper: Option<BookMapper>,
    /// Whether the consumer has been told a generation is open.
    publishing: bool,
    last_seq: Option<u64>,
    missing_capability_reported: bool,
}

impl DepthSession {
    fn new(hello: &protocol::Hello, symbol: String) -> Self {
        Self {
            mapper: hello.book_levels.map(|levels| {
                BookMapper::new(
                    symbol.clone(),
                    0,
                    Some(levels),
                    hello.tick_size.as_deref(),
                    hello.server_utc_offset_s,
                )
            }),
            symbol,
            publishing: false,
            last_seq: None,
            missing_capability_reported: false,
        }
    }

    fn log_capability(&self) {
        match &self.mapper {
            Some(_) => info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_AVAILABLE",
                symbol = %self.symbol,
                "bridge declares Depth of Market support"
            ),
            None => info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_UNSUPPORTED_BY_BRIDGE",
                symbol = %self.symbol,
                action = "trades_only",
                "bridge declares no Depth of Market; the heatmap will stay empty \
                 (recompile bridge/mt5/QuantickBridge.mq5, or the terminal refused the DOM)"
            ),
        }
    }

    fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        if let Some(mapper) = self.mapper.as_mut() {
            mapper.set_server_utc_offset_s(offset_s);
        }
    }

    /// Handle one image. `Err(())` means the consumer is gone.
    async fn observe(
        &mut self,
        image: protocol::Book,
        capture: &BookCaptureSwitch,
        generation_offset: &mut u64,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        if self.mapper.is_none() {
            // A bridge sending images it never declared is a version skew, not
            // data to trust silently.
            return Ok(());
        }
        let (enabled, base_generation) = capture.state();
        let lost_images = self.images_lost(image.seq);
        let mapper = self.mapper.as_mut().expect("checked above");
        if !enabled {
            if self.publishing {
                let generation = mapper.generation();
                self.publishing = false;
                self.last_seq = None;
                mapper.restart(generation); // next capture starts from a snapshot
                send_depth_status(tx, &self.symbol, generation, DepthStatus::Stopped).await?;
            }
            return Ok(());
        }

        // Open a generation when capture starts, when the consumer moves its
        // base, or when images were lost and the diff would silently bridge a
        // moment we never observed.
        let wanted = base_generation.saturating_add(*generation_offset);
        if !self.publishing || mapper.generation() != wanted || lost_images {
            if lost_images {
                send_depth_status(
                    tx,
                    &self.symbol,
                    mapper.generation(),
                    DepthStatus::Resyncing {
                        reason: DepthResyncReason::SourceRestarted {
                            cause: "book_images_lost",
                        },
                    },
                )
                .await?;
            }
            *generation_offset = generation_offset.saturating_add(1);
            let generation = base_generation.saturating_add(*generation_offset);
            mapper.restart(generation);
            self.publishing = true;
            send_depth_status(tx, &self.symbol, generation, DepthStatus::Connecting).await?;
        }
        self.last_seq = Some(image.seq);

        let Some(event) = mapper.map(&image) else {
            return Ok(());
        };
        let synchronized =
            matches!(event, DepthEvent::Snapshot { .. }).then(|| mapper.synchronized_status());
        let generation = mapper.generation();
        if tx.send(Mt5Event::Depth(event)).await.is_err() {
            return Err(());
        }
        if let Some(status) = synchronized {
            send_depth_status(tx, &self.symbol, generation, status).await?;
        }
        Ok(())
    }

    /// Whether images went missing (or the bridge restarted its counter)
    /// between the last one and `seq`.
    fn images_lost(&self, seq: u64) -> bool {
        match self.last_seq {
            Some(last) => seq != last.saturating_add(1),
            None => false,
        }
    }

    /// Tell a waiting consumer, once, that this bridge cannot supply depth.
    async fn report_missing_capability(
        &mut self,
        capture: &BookCaptureSwitch,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        let (enabled, base_generation) = capture.state();
        if self.mapper.is_some() || self.missing_capability_reported || !enabled {
            return Ok(());
        }
        self.missing_capability_reported = true;
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BOOK_UNSUPPORTED_BY_BRIDGE",
            symbol = %self.symbol,
            action = "report_disconnected",
            "depth capture is on but this bridge sends no Depth of Market"
        );
        // Tagged with the consumer's own base generation: a status below the
        // generation floor it is watching would be discarded as stale, and the
        // chart would keep waiting for a book that is never coming.
        send_depth_status(
            tx,
            &self.symbol,
            base_generation,
            DepthStatus::Disconnected {
                error_class: "bridge_without_depth",
            },
        )
        .await
    }

    /// End the generation when the bridge session ends.
    async fn close(&mut self, tx: &mpsc::Sender<Mt5Event>) {
        if let Some(mapper) = self.mapper.as_ref() {
            mapper.stats.log_summary(&self.symbol);
            if self.publishing {
                let _ = send_depth_status(
                    tx,
                    &self.symbol,
                    mapper.generation(),
                    DepthStatus::Disconnected {
                        error_class: "bridge_lost",
                    },
                )
                .await;
            }
        }
    }
}

/// Publish one depth status. `Err(())` means the consumer is gone.
async fn send_depth_status(
    tx: &mpsc::Sender<Mt5Event>,
    symbol: &str,
    generation: u64,
    status: DepthStatus,
) -> Result<(), ()> {
    tx.send(Mt5Event::Depth(DepthEvent::Status {
        symbol: symbol.to_string(),
        generation,
        status,
    }))
    .await
    .map_err(|_| ())
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
mod bounded_reader_tests {
    use super::{BoundedLine, BoundedLineReader, MAX_LINE_BYTES};

    /// The reader is generic so the app can point it at a launched bridge's
    /// stderr, not just a socket. A cursor stands in for both.
    #[tokio::test]
    async fn it_reads_lines_from_any_source_not_just_a_socket() {
        let source = std::io::Cursor::new(b"first\nsecond\r\n".to_vec());
        let mut reader = BoundedLineReader::new(source);
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("first".to_owned())
        );
        // A CRLF terminator loses the carriage return, like the socket path.
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("second".to_owned())
        );
        assert_eq!(reader.next_line().await.unwrap(), BoundedLine::Eof);
    }

    #[tokio::test]
    async fn an_endless_line_is_capped_and_reading_continues() {
        // The failure this bound exists for: output with no newline in it.
        // The oversized line is dropped, and the line after it still arrives.
        let mut bytes = vec![b'x'; MAX_LINE_BYTES + 10];
        bytes.push(b'\n');
        bytes.extend_from_slice(b"survivor\n");
        let mut reader = BoundedLineReader::new(std::io::Cursor::new(bytes));
        assert_eq!(reader.next_line().await.unwrap(), BoundedLine::TooLong);
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("survivor".to_owned())
        );
    }

    #[tokio::test]
    async fn invalid_utf8_is_reported_and_skipped_not_fatal() {
        // A Windows-encoded path inside a Python traceback is exactly this.
        let mut bytes = b"before\n".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, b'\n']);
        bytes.extend_from_slice(b"after\n");
        let mut reader = BoundedLineReader::new(std::io::Cursor::new(bytes));
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("before".to_owned())
        );
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::NotUtf8 { len: 2 }
        );
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("after".to_owned()),
            "one unreadable line must not end the stream"
        );
    }
}

/// One read from a [`BoundedLineReader`].
#[derive(Debug, PartialEq, Eq)]
pub enum BoundedLine {
    /// A complete UTF-8 line (terminator stripped).
    Line(String),
    /// A complete line that was not valid UTF-8; skippable, per PROTOCOL.md.
    NotUtf8 {
        /// Length of the rejected line, in bytes.
        len: usize,
    },
    /// The peer streamed more than [`MAX_LINE_BYTES`] without a newline.
    TooLong,
    /// The peer closed the connection.
    Eof,
}

/// What one `fill_buf` round decided (split out so the borrow of the reader's
/// internal buffer ends before `consume`).
enum ReadStep {
    Eof,
    Line(usize),
    TooLong(usize),
    More(usize),
}

/// A newline-delimited reader that never buffers more than
/// [`MAX_LINE_BYTES`], unlike `AsyncBufReadExt::lines`. Cancel-safe: the only
/// await is `fill_buf`, and bytes move out of the source buffer and into the
/// line buffer within a single poll.
///
/// Generic over the source because the bridge speaks the same line-delimited
/// shape over two transports: a socket, and the stderr of a bridge quantick
/// launched itself. Both need the same bound — an unterminated line is a
/// memory leak wherever it comes from — and one implementation is what keeps
/// the two honest about it.
pub struct BoundedLineReader<R> {
    reader: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: tokio::io::AsyncRead + Unpin> BoundedLineReader<R> {
    /// Wrap `source`, reading at most [`MAX_LINE_BYTES`] per line.
    pub fn new(source: R) -> Self {
        Self {
            reader: BufReader::new(source),
            buf: Vec::new(),
        }
    }

    /// The next line, or what went wrong with it.
    pub async fn next_line(&mut self) -> std::io::Result<BoundedLine> {
        loop {
            let step = {
                let available = self.reader.fill_buf().await?;
                if available.is_empty() {
                    ReadStep::Eof
                } else if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                    if self.buf.len() + pos > MAX_LINE_BYTES {
                        ReadStep::TooLong(pos + 1)
                    } else {
                        self.buf.extend_from_slice(&available[..pos]);
                        ReadStep::Line(pos + 1)
                    }
                } else if self.buf.len() + available.len() > MAX_LINE_BYTES {
                    ReadStep::TooLong(available.len())
                } else {
                    self.buf.extend_from_slice(available);
                    ReadStep::More(available.len())
                }
            };
            match step {
                ReadStep::Eof => {
                    // A trailing unterminated line still counts, like
                    // `lines()` behaves.
                    if self.buf.is_empty() {
                        return Ok(BoundedLine::Eof);
                    }
                    return Ok(Self::finish(std::mem::take(&mut self.buf)));
                }
                ReadStep::Line(consume) => {
                    self.reader.consume(consume);
                    return Ok(Self::finish(std::mem::take(&mut self.buf)));
                }
                ReadStep::TooLong(consume) => {
                    self.reader.consume(consume);
                    self.buf.clear();
                    return Ok(BoundedLine::TooLong);
                }
                ReadStep::More(consume) => self.reader.consume(consume),
            }
        }
    }

    fn finish(mut line: Vec<u8>) -> BoundedLine {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        match String::from_utf8(line) {
            Ok(text) => BoundedLine::Line(text),
            Err(e) => BoundedLine::NotUtf8 {
                len: e.as_bytes().len(),
            },
        }
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
