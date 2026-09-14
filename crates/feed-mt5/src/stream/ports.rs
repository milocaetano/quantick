//! The two shared-state ports the consumer holds onto.
//!
//! Both are handles the consumer keeps and the running session reads, rather
//! than channels threaded through the server's signature. They live together
//! because they dock the same way: clone the handle, hold it, and the session
//! picks the change up on its next pass.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

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

    pub(super) fn state(&self) -> (bool, u64) {
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
    pub(super) async fn take_request(&self) -> (u64, i64) {
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
    pub(super) fn settle_owed(&self) -> bool {
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
    pub(super) fn abandon(&self) -> bool {
        let mut held = self.0.request.lock().expect("history pager mutex");
        let owed = held.queued.take().is_some() || held.in_flight;
        held.in_flight = false;
        owed
    }
}
