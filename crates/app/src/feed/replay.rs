//! Playing a recorded session back through the live-feed channel.
//!
//! Market replay is not a second chart mode: it is a *source*. The session's
//! trades are released on a playback clock and pushed down the very same
//! [`FeedEvent`] channel a live venue uses, so bar building, rendering,
//! navigation and metrics run the code path they always run and cannot drift
//! from live behaviour.
//!
//! Three things keep it smooth at 50×:
//!
//! - trades that fall due together travel as one [`FeedEvent::LiveBatch`], not
//!   one channel message each;
//! - the transport reads playback state from atomics, so drawing the bar costs
//!   no lock and cannot block the worker;
//! - a stalled UI (a dragged window, a slow frame) clamps the clock delta
//!   instead of releasing a minute of market in one jump.
//!
//! The clock itself lives in [`quantick_replay::clock`], is deterministic, and
//! never reads a wall clock — this module measures the time and tells it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use quantick_replay::{PlaybackConfig, Playhead, Session};

use super::{FeedCommand, FeedEvent, FeedHandle};
use crate::config::FeedCapabilities;

/// How often the worker wakes while playing. Finer than a 60 fps frame, so the
/// chart never waits on the clock for a print that is already due.
const TICK_PLAYING: Duration = Duration::from_millis(8);

/// How often the worker wakes while paused — often enough to feel instant on
/// the next click, rare enough to be free.
const TICK_PAUSED: Duration = Duration::from_millis(33);

/// Largest real-time step one advance may take, whatever the wall clock says.
///
/// A frame that took 2 s (a dragged window, a stalled GPU) must not release 100
/// seconds of market at 50× in one lurch. The replay falls behind real time by
/// the overrun instead, which is visible and honest.
const MAX_DELTA_MS: f64 = 250.0;

/// Room for a few frames of batches, so a brief UI stall does not stall the
/// clock. Beyond that the worker blocks, which is the intended backpressure.
const EVENT_CHANNEL_CAPACITY: usize = 64;

/// What the UI asks the transport to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReplayControl {
    /// Start playing (restarting first if the session already finished).
    Play,
    /// Stop advancing, keeping the position.
    Pause,
    /// Play if paused, pause if playing.
    TogglePlay,
    /// Change the speed multiplier; the position does not move.
    SetSpeed(f32),
    /// Jump to a fraction of the session, `0.0`..=`1.0`.
    SeekToFraction(f32),
    /// Jump back to the first print.
    Restart,
}

/// How a session is opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayOptions {
    /// Speed to start at.
    pub speed: f32,
    /// Whether to start playing immediately.
    ///
    /// Off by default. A replay is a rehearsal, and it begins when the trader
    /// says so: a session that runs while they are still reading the chart
    /// costs them the open, and the only way back is a seek that rebuilds
    /// every bar. A scripted run that wants a moving tape asks for it outright.
    pub autoplay: bool,
    /// Market time that may pass with no prints before the clock skips ahead.
    pub skip_idle_over_ms: Option<i64>,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            speed: 1.0,
            autoplay: false,
            skip_idle_over_ms: Some(quantick_replay::clock::DEFAULT_SKIP_IDLE_MS),
        }
    }
}

/// Everything needed to start playing a session that is already loaded.
#[derive(Debug, Clone)]
pub struct ReplayRequest {
    /// The loaded session. Shared, so the UI can read its metadata without
    /// copying a few hundred thousand trades.
    pub session: Arc<Session>,
    /// How to open it.
    pub options: ReplayOptions,
}

/// Playback state, published by the worker and read by the UI every frame.
///
/// Atomics rather than a mutex: the transport bar reads six numbers per frame
/// and must never wait on the thread that is releasing trades.
#[derive(Debug)]
pub struct ReplayStatus {
    position_ms: AtomicI64,
    start_ms: AtomicI64,
    end_ms: AtomicI64,
    cursor: AtomicUsize,
    total: AtomicUsize,
    speed_bits: AtomicU32,
    playing: AtomicBool,
    finished: AtomicBool,
    /// How many restarts and seeks the worker applied, and where the last
    /// one landed: a reader that samples the position once per frame can
    /// tell a rerun that already advanced past its last sample from plain
    /// forward play, and knows where the rerun began.
    rewinds: AtomicU64,
    rewind_target_ms: AtomicI64,
}

impl ReplayStatus {
    fn new(playhead: &Playhead) -> Self {
        let status = Self {
            rewinds: AtomicU64::new(0),
            rewind_target_ms: AtomicI64::new(playhead.position_ms()),
            position_ms: AtomicI64::new(playhead.position_ms()),
            start_ms: AtomicI64::new(playhead.start_ms()),
            end_ms: AtomicI64::new(playhead.end_ms()),
            cursor: AtomicUsize::new(playhead.cursor()),
            total: AtomicUsize::new(playhead.total()),
            speed_bits: AtomicU32::new(playhead.speed().to_bits()),
            playing: AtomicBool::new(playhead.is_playing()),
            finished: AtomicBool::new(playhead.is_finished()),
        };
        status.publish(playhead);
        status
    }

    fn publish(&self, playhead: &Playhead) {
        self.position_ms
            .store(playhead.position_ms(), Ordering::Relaxed);
        self.cursor.store(playhead.cursor(), Ordering::Relaxed);
        self.speed_bits
            .store(playhead.speed().to_bits(), Ordering::Relaxed);
        self.playing.store(playhead.is_playing(), Ordering::Relaxed);
        self.finished
            .store(playhead.is_finished(), Ordering::Relaxed);
    }

    /// Market time reached, in epoch milliseconds.
    #[must_use]
    pub fn position_ms(&self) -> i64 {
        self.position_ms.load(Ordering::Relaxed)
    }

    /// Timestamp of the session's first print.
    #[must_use]
    pub fn start_ms(&self) -> i64 {
        self.start_ms.load(Ordering::Relaxed)
    }

    /// Timestamp of the session's last print.
    #[must_use]
    pub fn end_ms(&self) -> i64 {
        self.end_ms.load(Ordering::Relaxed)
    }

    /// How many prints have been released.
    #[must_use]
    pub fn played(&self) -> usize {
        self.cursor.load(Ordering::Relaxed)
    }

    /// How many prints the session holds.
    #[must_use]
    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    /// The current speed multiplier.
    #[must_use]
    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed_bits.load(Ordering::Relaxed))
    }

    /// Whether playback is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Whether the session has played out.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// Logical replay time: milliseconds of session time since the first
    /// trade, in the recording's own clock. This is the control trace's time
    /// axis (contract §11); wall-clock time never enters it.
    #[must_use]
    pub fn elapsed_ms(&self) -> i64 {
        self.position_ms().saturating_sub(self.start_ms())
    }

    /// Restarts and seeks applied so far; changes when the playhead jumped.
    #[must_use]
    pub fn rewinds(&self) -> u64 {
        self.rewinds.load(Ordering::Relaxed)
    }

    /// Where the last restart or seek landed, as session-elapsed
    /// milliseconds — the position the rerun started from.
    #[must_use]
    pub fn rewind_target_elapsed_ms(&self) -> i64 {
        self.rewind_target_ms
            .load(Ordering::Relaxed)
            .saturating_sub(self.start_ms())
    }

    /// The worker applied a restart or a seek that landed at `target_ms`.
    pub(crate) fn note_rewind(&self, target_ms: i64) {
        self.rewind_target_ms.store(target_ms, Ordering::Relaxed);
        self.rewinds.fetch_add(1, Ordering::Relaxed);
    }

    /// Move the published playhead, as a worker's forward play would.
    #[cfg(test)]
    pub(crate) fn set_position_ms_for_test(&self, position_ms: i64) {
        self.position_ms.store(position_ms, Ordering::Relaxed);
    }

    /// How far through the session playback is, `0.0`..=`1.0`.
    #[must_use]
    pub fn progress(&self) -> f32 {
        let (start, end) = (self.start_ms(), self.end_ms());
        let span = end.saturating_sub(start);
        if span <= 0 {
            return if self.is_finished() { 1.0 } else { 0.0 };
        }
        let elapsed = self.position_ms().saturating_sub(start) as f64;
        (elapsed / span as f64).clamp(0.0, 1.0) as f32
    }
}

/// The UI's window onto a playing session: what it is, and where it is.
#[derive(Debug, Clone)]
pub struct ReplayLink {
    /// The session being played, for its symbol, day and timezone.
    pub session: Arc<Session>,
    /// Live playback state.
    pub status: Arc<ReplayStatus>,
}

impl ReplayLink {
    /// Instrument and day, e.g. `WINJ26 · 2026-03-16`.
    #[must_use]
    pub fn label(&self) -> String {
        self.session.label()
    }

    /// The instrument being replayed.
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.session.symbol
    }

    /// Prints of the session's *own day* released so far, and how many it
    /// holds.
    ///
    /// The pair the transport bar draws and the control plane publishes, from
    /// one function because they are one answer. A day joined in front of the
    /// recording is context the chart was handed, not part of the session
    /// being rehearsed, so it is counted out of both — and a screen that says
    /// `0 / 1 446 989` beside a snapshot that says `1 504 020 / 2 951 009` is
    /// two surfaces disagreeing about the same session, which is a bug of its
    /// own class here rather than a rounding difference.
    #[must_use]
    pub fn day_prints(&self) -> (usize, usize) {
        let joined = self.session.day_before_prints();
        (
            self.status.played().saturating_sub(joined),
            self.status.total().saturating_sub(joined),
        )
    }
}

#[cfg(test)]
impl ReplayLink {
    /// A link with no worker behind it: the status a fresh playhead over
    /// `session` publishes, and nothing releasing trades.
    ///
    /// For tests that need a tab to believe a recording is its source —
    /// `replay.is_some()` is the one flag the rest of the UI reads — without
    /// spawning playback and waiting on a thread.
    pub(crate) fn for_test(session: Session) -> Self {
        let playhead = Playhead::new(&session.trades, PlaybackConfig::default());
        Self {
            status: Arc::new(ReplayStatus::new(&playhead)),
            session: Arc::new(session),
        }
    }
}

/// Start playing `request` on a background thread.
///
/// The returned handle carries the same channels a live feed does, plus a
/// [`ReplayLink`]. Dropping it stops the worker.
#[must_use]
pub fn spawn(request: ReplayRequest) -> FeedHandle {
    let (tx, rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
    // Replay streams no depth. The receiver exists so the UI drains one shape
    // of handle; it simply never yields an event.
    let (_book_tx, book_rx) = mpsc::channel(1);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);

    let config = PlaybackConfig {
        speed: request.options.speed,
        skip_idle_over_ms: request.options.skip_idle_over_ms,
        ..PlaybackConfig::default()
    };
    // Opening past the day joined in front of the recording, when there is
    // one: those prints are context the chart is handed as history, not part
    // of the session being rehearsed. `day_before_prints` is 0 without a join,
    // which is exactly the playhead that was always built here.
    let mut playhead = Playhead::opening_at(
        &request.session.trades,
        config,
        request.session.day_before_prints(),
    );
    if request.options.autoplay {
        playhead.play();
    }
    let status = Arc::new(ReplayStatus::new(&playhead));
    let link = ReplayLink {
        session: Arc::clone(&request.session),
        status: Arc::clone(&status),
    };

    let session = Arc::clone(&request.session);
    std::thread::Builder::new()
        .name("quantick-replay".into())
        .spawn(move || play(session, playhead, status, tx, cmd_rx))
        .expect("spawn replay thread");

    FeedHandle {
        events: rx,
        book_events: book_rx,
        // A file on disk needs nothing started, and a session that failed to
        // parse never reaches this point: the browser that opened it reports.
        notices: super::silent_notices(),
        // A recording carries no depth and no venue to page trades from. It
        // does carry candles when a context file was downloaded beside it —
        // and only then. Synthesizing months of candles around a recorded hour
        // would present data the recording never held, so the capability
        // follows the file on disk rather than the fact that this is a replay.
        // The sizes in it are the sizes that were captured, so anything
        // measuring volume still works.
        capabilities: super::fixed_capabilities(FeedCapabilities {
            book_capture: false,
            // A recording pages *candles*, never trades: the tape in the
            // file is the whole day, and there is no venue behind it to ask
            // for more prints. So this is flatly false, and the trade half of
            // the history control goes with it.
            //
            // It was once `context.is_some()` — a fact about a *candle* file
            // answering the *trade* capability — and the cost was the bug this
            // line is the fix for: a recording offered the "previous session"
            // reach, the trader pressed it, every request was answered with an
            // empty block, and the run gave up in silence. A capability is a
            // promise about what a request will return, and this source
            // returns no older print under any reach.
            history_paging: false,
            traded_volume: true,
            // The candles are the record it *can* page, and only when one was
            // downloaded beside it — which the chart marks as broker candles
            // wherever it draws them, so the trader reads what they got.
            ohlcv_history: request.session.context.is_some(),
            ohlcv_generation: 0,
        }),
        commands: cmd_tx,
        // A recording has no chain to attribute: its prints are as old as the
        // day they were captured, and the playback clock decides when they
        // appear. There is no delay here to blame anything for.
        latency: super::unsplit_latency(),
        replay: Some(link),
    }
}

/// Re-parse the context file only when it is not the one already in hand.
///
/// `seen` carries the modified time of the copy `context` was built from —
/// `None` before any read, which is also the state a session opened with a
/// context file already parsed is in, so the first request pays one parse and
/// no later one does unless the file really moved. A file the platform will
/// not stat is read: a missing timestamp is not evidence that nothing changed,
/// and the honest cost of not knowing is the parse.
///
/// Only replaced on success, like the reload it wraps: a download that
/// produced a malformed file must not blank out context already being read.
fn reload_changed_context(
    session: &Session,
    seen: &mut Option<std::time::SystemTime>,
) -> Option<quantick_replay::ContextSeries> {
    let stamp = std::fs::metadata(quantick_replay::context_path(&session.path))
        .and_then(|meta| meta.modified())
        .ok();
    if stamp.is_some() && stamp == *seen {
        return None;
    }
    let fresh = reload_context(session)?;
    *seen = stamp;
    Some(fresh)
}

/// The candles to answer one `FetchOhlcv` with, from the session's context.
///
/// The span is measured back from `until_ms`, and `until_ms` defaults to the
/// recording's first print rather than the wall clock: a session recorded in
/// March is being replayed today, and "the last week" has to mean a week of
/// *the market's* time or the whole context would fall outside it. A *load
/// older* passes the instant before the oldest candle already held, and gets
/// another span of the run-up in front of it — as far as the context file
/// reaches, which is the whole of a recording's past.
///
/// A recording with no context file answers empty and complete — that is the
/// whole truth about it, not a fetch that came up short.
///
/// Always one reply, whatever slicing the request asked for: the context is a
/// file already read into memory, so there is no venue round trip for slices
/// to hide behind, and cutting it up would buy the trader nothing but extra
/// rebuilds.
fn context_reply(
    context: Option<&quantick_replay::ContextSeries>,
    first_print_ms: i64,
    span_ms: i64,
    before_ms: Option<i64>,
) -> FeedEvent {
    let until_ms = before_ms.unwrap_or(first_print_ms);
    let Some(context) = context else {
        return FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: Vec::new(),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        };
    };
    let earliest = until_ms.saturating_sub(span_ms);
    let bars: Vec<_> = context
        .bars
        .iter()
        .filter(|bar| bar.open_time >= earliest && bar.open_time <= until_ms)
        .cloned()
        .collect();
    // Short means "there is more market *before* the first bar returned" —
    // which is a claim about the old end of the window only. Counting against
    // the whole file answered that correctly while the filter had one bound;
    // with an upper bound it does not, because a context that also covers the
    // recording's own window (one downloaded after the session) has its
    // trailing bars cut by the filter, and reporting that as short would say
    // the run-up is missing when it is all there. So the count to beat is the
    // bars the file holds *at or before* this window's newest edge.
    let reachable = context
        .bars
        .iter()
        .filter(|bar| bar.open_time <= until_ms)
        .count();
    FeedEvent::OhlcvHistory {
        interval_ms: context.interval_ms,
        // Short for either of two reasons, and both are the same answer to the
        // caller: the download itself was clipped, or this span does not reach
        // the whole of what the file has before it.
        slice: crate::feed::OhlcvSlice::Last {
            complete: context.complete && bars.len() == reachable,
        },
        bars,
    }
}

/// Re-read the context file beside a recording.
///
/// Returns `None` when there is none, or when the one on disk will not parse.
/// Either way the chart keeps whatever it was showing and the reason reaches
/// the log: a download that produced a malformed file must not blank out
/// context the trader was already reading.
fn reload_context(session: &Session) -> Option<quantick_replay::ContextSeries> {
    let path = quantick_replay::context_path(&session.path);
    let text = std::fs::read_to_string(&path).ok()?;
    match quantick_replay::parse_context(&text) {
        Ok(series) => {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_CONTEXT_RELOADED",
                file = %path.display(),
                candles = series.bars.len(),
                complete = series.complete,
                "picked up context from disk"
            );
            Some(series)
        }
        Err(e) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_CONTEXT_UNREADABLE",
                file = %path.display(),
                detail = %e,
                advice = e.advice(),
                "the context file beside this recording will not parse"
            );
            None
        }
    }
}

/// The playback loop. Returns as soon as the UI drops its end of either channel.
fn play(
    session: Arc<Session>,
    mut playhead: Playhead,
    status: Arc<ReplayStatus>,
    tx: mpsc::Sender<FeedEvent>,
    mut commands: mpsc::Receiver<FeedCommand>,
) {
    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "REPLAY_STARTED",
        symbol = session.symbol.as_str(),
        session = %session.label(),
        file = %session.path.display(),
        trades = session.trades.len(),
        span_ms = session.span_ms(),
        speed = playhead.speed(),
        playing = playhead.is_playing(),
        day_before = session.day_before_label().unwrap_or_default().as_str(),
        day_before_prints = session.day_before_prints(),
        "replaying a recorded session"
    );

    // The chart opens on whatever the playhead has already passed: nothing at
    // all for a lone day, and the whole of a joined one in front of a session
    // that has the day before it. Either way the backfill resolves the UI's
    // initial "loading history" state right away.
    let opening = session.trades[..playhead.cursor()].to_vec();
    if tx.blocking_send(FeedEvent::Backfilled(opening)).is_err() {
        return;
    }

    let trades = &session.trades[..];
    // Held apart from the session so "load older" can replace it without
    // touching the playhead or the trades being released.
    let mut context = session.context.clone();
    // When the context file this copy came from was last written, so a request
    // that finds it unchanged skips the parse. `None` until the first request
    // asks — see `reload_changed_context`.
    let mut context_seen: Option<std::time::SystemTime> = None;
    let mut last = Instant::now();
    let mut reported_finished = false;

    let mut drained = Vec::new();
    loop {
        // 1. Controls first, so a click is honoured before the next batch.
        drained.clear();
        loop {
            match commands.try_recv() {
                Ok(FeedCommand::Replay(control)) => drained.push(control),
                // Answered from the context file beside the recording, or
                // answered empty — but always answered exactly once, or the
                // pane waits for a reply that never comes.
                Ok(FeedCommand::FetchOhlcv {
                    span_ms, before_ms, ..
                }) => {
                    // Pick up a context file that has *changed* since it was
                    // parsed — the trader downloads more of the run-up from
                    // inside the replay, and the copy taken when the session
                    // opened predates it. Re-read rather than reopened, so the
                    // playhead never moves: asking for more history mid-replay
                    // must not cost the trader their place.
                    //
                    // Gated on the file's own timestamp, not done every time.
                    // A quarter of 1-minute candles is well over a hundred
                    // thousand rows, and this runs on the playback thread,
                    // whose next step measures the wall time that passed — so
                    // an unconditional parse on the *opening* request would be
                    // charged to the playhead and released as a burst of
                    // market. A `metadata` call is not.
                    //
                    // This lived on `LoadOlder` until the capability above
                    // stopped claiming a recording could page trades. It is
                    // the same act, now on the request that actually answers
                    // with candles.
                    if let Some(fresh) = reload_changed_context(&session, &mut context_seen) {
                        context = Some(fresh);
                    }
                    let reply =
                        context_reply(context.as_ref(), session.start_ms(), span_ms, before_ms);
                    if tx.blocking_send(reply).is_err() {
                        return; // UI gone
                    }
                }
                // "Load older" on a recording cannot page a venue — there is
                // none — and since `history_paging` says so the UI no longer
                // offers the gesture. Still answered, and answered empty: a
                // request queued before a source switch, or sent by an
                // operator through the `QUANTICK_LOAD_OLDER` hook, has raised
                // a wait that only a reply can lower, and a spinner left
                // turning over a record that will never grow is the same lie
                // in a smaller place. The run-up lives on `FetchOhlcv` above.
                Ok(FeedCommand::LoadOlder { .. }) => {
                    if tx
                        .blocking_send(FeedEvent::HistoryPrepended(Vec::new()))
                        .is_err()
                    {
                        return; // UI gone
                    }
                }
                // Other live-feed commands have no meaning for a recording, and
                // none of them is something the UI waits on. Ignored rather
                // than refused: the UI gates them by capability, and a stray
                // one must not kill playback.
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }
        if !drained.is_empty() {
            let (immediate, position) = coalesce(&drained);
            // `Playhead::play` on a finished session parks the cursor back on
            // the open, so pressing play at the end *is* a restart — and every
            // restart is a position change. `coalesce` is pure and cannot ask
            // the playhead whether it is finished, so it is recognised here,
            // before the controls are applied and the answer changes. Without
            // it the session is released a second time onto a chart that
            // already holds it — every bar of the day drawn twice — and a
            // control-trace walk never sees that the playhead moved back.
            let play_restarts = playhead.is_finished()
                && immediate.iter().any(|control| {
                    matches!(control, ReplayControl::Play | ReplayControl::TogglePlay)
                });
            for control in immediate {
                if !apply(control, &mut playhead, trades, &tx, &session) {
                    return; // UI gone
                }
            }
            // Unless an explicit seek or restart came in the same batch: that
            // one rebuilds below, from the position that actually wins.
            if play_restarts && position.is_none() {
                if !rebuild_from_cursor(&playhead, trades, &tx, &session, "play_at_end") {
                    return; // UI gone
                }
                status.publish(&playhead);
                status.note_rewind(playhead.position_ms());
            }
            if let Some(control) = position {
                if !apply(control, &mut playhead, trades, &tx, &session) {
                    return;
                }
                // The new position first, the rewind count after it. A reader
                // that samples between the two must never see the count move
                // while the position is still the pre-seek one: it would
                // rewind its walk and then replay everything up to where the
                // playhead *used* to be, all in one frame. This order can
                // only be seen the other way round, which the reader's own
                // "the position went backwards" check already handles.
                status.publish(&playhead);
                status.note_rewind(playhead.position_ms());
            }
            reported_finished = false;
            last = Instant::now();
        }

        // 2. Release whatever the clock says is due.
        let now = Instant::now();
        let delta = (now.saturating_duration_since(last).as_secs_f64() * 1000.0).min(MAX_DELTA_MS);
        last = now;

        let batch = playhead.advance(trades, delta);
        if !batch.is_empty() {
            let due = trades[batch.range()].to_vec();
            if tx.blocking_send(FeedEvent::LiveBatch(due)).is_err() {
                return;
            }
        }
        status.publish(&playhead);

        if playhead.is_finished() && !reported_finished {
            reported_finished = true;
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_FINISHED",
                symbol = session.symbol.as_str(),
                session = %session.label(),
                trades = playhead.total(),
                action = "await_transport_command",
                "recorded session played out"
            );
        }

        std::thread::sleep(if playhead.is_playing() && !playhead.is_finished() {
            TICK_PLAYING
        } else {
            TICK_PAUSED
        });
    }
}

/// Split a drained batch of controls into the ones applied in order and the one
/// position command that survives.
///
/// Moving the playhead rebuilds the chart from the session's history — work
/// proportional to the whole recording — so applying every queued seek would
/// throw away all but the last rebuild. Only the newest position command
/// (`Restart` or `SeekToFraction`) is kept, and it is applied after the others
/// so the last thing asked for is where playback lands. Play, pause and speed
/// are cheap and orthogonal, so they keep their order.
fn coalesce(controls: &[ReplayControl]) -> (Vec<ReplayControl>, Option<ReplayControl>) {
    let mut immediate = Vec::new();
    let mut position = None;
    for control in controls {
        match control {
            ReplayControl::Restart | ReplayControl::SeekToFraction(_) => position = Some(*control),
            other => immediate.push(*other),
        }
    }
    (immediate, position)
}

/// Apply one transport command. Returns `false` when the UI has gone away.
fn apply(
    control: ReplayControl,
    playhead: &mut Playhead,
    trades: &[quantick_engine::Trade],
    tx: &mpsc::Sender<FeedEvent>,
    session: &Session,
) -> bool {
    match control {
        ReplayControl::Play => playhead.play(),
        ReplayControl::Pause => playhead.pause(),
        ReplayControl::TogglePlay => playhead.toggle(),
        ReplayControl::SetSpeed(speed) => {
            playhead.set_speed(speed);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_SPEED_CHANGED",
                symbol = session.symbol.as_str(),
                speed = playhead.speed(),
                "replay speed changed"
            );
        }
        ReplayControl::Restart => {
            playhead.restart();
            return rebuild_from_cursor(playhead, trades, tx, session, "restart");
        }
        ReplayControl::SeekToFraction(fraction) => {
            playhead.seek_to_fraction(trades, fraction);
            return rebuild_from_cursor(playhead, trades, tx, session, "seek");
        }
    }
    true
}

/// Rebuild the chart so it matches the playhead exactly.
///
/// Seeking either direction resends the whole session up to the new position as
/// history. Bars already closed cannot be reopened, and a forward-only patch
/// would leave the chart showing a series the position no longer explains — so
/// both directions take the same honest path.
fn rebuild_from_cursor(
    playhead: &Playhead,
    trades: &[quantick_engine::Trade],
    tx: &mpsc::Sender<FeedEvent>,
    session: &Session,
    reason: &'static str,
) -> bool {
    let cursor = playhead.cursor();
    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "REPLAY_SEEK",
        symbol = session.symbol.as_str(),
        reason,
        position_ms = playhead.position_ms(),
        played = cursor,
        total = playhead.total(),
        action = "rebuild_chart_from_history",
        "replay position moved"
    );
    if tx.blocking_send(FeedEvent::Reset).is_err() {
        return false;
    }
    tx.blocking_send(FeedEvent::Backfilled(trades[..cursor].to_vec()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use quantick_engine::{Side, Trade};
    use quantick_replay::ParseOptions;
    use rust_decimal::Decimal;

    /// `count` prints 10 ms apart from 10:00:00, so a session of any length
    /// still moves strictly forward in time — which is what the format demands.
    fn session(count: usize) -> Arc<Session> {
        let mut text = String::from("# symbol=TEST\nDate,Time,Price,Volume,Side\n");
        for i in 0..count {
            let ms = i as i64 * 10;
            text.push_str(&format!(
                "2026-03-16,{:02}:{:02}:{:02}.{:03},{},1,B\n",
                10 + ms / 3_600_000,
                (ms / 60_000) % 60,
                (ms / 1_000) % 60,
                ms % 1_000,
                100 + i
            ));
        }
        Arc::new(
            Session::from_text(
                &PathBuf::from("replay/TEST/20260316.csv"),
                &text,
                ParseOptions::default(),
            )
            .expect("session"),
        )
    }

    /// The same session, with `minutes` of broker candles downloaded beside it.
    ///
    /// The candles sit in the hour before the tape starts, which is where a
    /// real context file sits: the market's past, never overlapping the
    /// recording.
    fn session_with_context(count: usize, minutes: i64) -> Arc<Session> {
        let mut session = Session::clone(&session(count));
        let first = session.start_ms();
        let mut text = String::from("# interval_ms=60000\nDate,Time,Open,High,Low,Close,Volume\n");
        for i in 0..minutes {
            // Counting backwards from the print before the tape opens.
            let stamp = first - (minutes - i) * 60_000;
            let (date, time) = quantick_replay::format::format_datetime(
                stamp,
                quantick_replay::format::UtcOffset::UTC,
            );
            text.push_str(&format!("{date},{time},100,110,90,105,7\n"));
        }
        session.context = Some(quantick_replay::parse_context(&text).expect("context fixture"));
        Arc::new(session)
    }

    /// Drain the next candle reply, or fail the test rather than hang.
    fn next_candles(handle: &mut FeedHandle) -> (i64, Vec<quantick_engine::Bar>, bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "no reply: a pane would hang here"
            );
            match handle.events.try_recv() {
                Ok(FeedEvent::OhlcvHistory {
                    interval_ms,
                    bars,
                    slice,
                }) => {
                    // A recording always answers once and for all; the helper
                    // would be hiding a second reply if one ever appeared.
                    let crate::feed::OhlcvSlice::Last { complete } = slice else {
                        panic!("a recording must answer with a single closing slice");
                    };
                    break (interval_ms, bars, complete);
                }
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PAUSED),
                Err(mpsc::error::TryRecvError::Disconnected) => panic!("worker gone"),
            }
        }
    }

    /// A recording must not claim it can page *trades*.
    ///
    /// The tape in the file is the whole of it and there is no venue behind it
    /// to ask for more prints — the comment above `spawn`'s capability block
    /// has always said so. Declaring `history_paging` from the presence of a
    /// *candle* file put the trade reach, the page-size box and a live
    /// "+ older" button in front of the trader over a source that answers
    /// every one of them with an empty block. That is the facade this test
    /// exists to keep shut.
    #[test]
    fn a_recording_never_claims_to_page_trades() {
        // Every shape a recording comes in, the joined day included: that one
        // hands the trader the day before through the *opening backfill*, once,
        // and never through a request — so it is more tape behind the chart and
        // still nothing this source could page if asked.
        for session in [
            session(4),
            session_with_context(4, 90),
            session_with_day_before(4, 2),
        ] {
            let handle = spawn(ReplayRequest {
                session: Arc::clone(&session),
                options: ReplayOptions {
                    speed: 1.0,
                    autoplay: false,
                    ..ReplayOptions::default()
                },
            });
            let capabilities = *handle.capabilities.borrow();
            assert!(
                !capabilities.history_paging,
                "a recording has no older trades to serve — not with a context                  file beside it, and not with the day before joined in front"
            );
            assert_eq!(
                capabilities.ohlcv_history,
                session.context.is_some(),
                "candles are the record a recording *can* page, and only when \
                 one was downloaded beside it"
            );
        }
    }

    /// The behaviour that used to ride on `LoadOlder`, on the request that
    /// actually answers with candles.
    ///
    /// The reachable half of it, which is narrower than it first looks.
    /// `ohlcv_history` is fixed at spawn from `session.context.is_some()` and
    /// the channel is never updated, so a recording opened with *no* context
    /// file has no candle capability for the rest of the session and the app
    /// will not send this request at all — the old `LoadOlder` path could not
    /// reach that case either, since `history_paging` was the same expression.
    /// What both could always do, and what this proves, is pick up a file that
    /// **grew**: the trader downloads more of the run-up from inside the
    /// replay, and the copy parsed when the session opened is short.
    #[test]
    fn the_candle_reach_picks_up_a_run_up_that_grew_since_the_session_opened() {
        let dir = scratch_dir("grew");
        let path = dir.join("20260316.csv");
        let mut tape = String::from("# symbol=TEST\nDate,Time,Price,Volume,Side\n");
        for i in 0..4_i64 {
            tape.push_str(&format!("2026-03-16,10:00:0{i}.000,{},1,B\n", 100 + i));
        }
        std::fs::write(&path, &tape).expect("tape on disk");
        let context_path = quantick_replay::context_path(&path);

        // Opened with a short run-up: two candles, so the capability is on.
        std::fs::write(&context_path, context_text(&path, &tape, 2)).expect("short context");
        // `load`, not `from_text`: the sidecar is attached by the loader that
        // touches the disk, which is the path the session browser takes.
        let session = Arc::new(Session::load(&path, ParseOptions::default()).expect("session"));
        let short = session
            .context
            .as_ref()
            .expect("the file beside the tape was parsed at open");
        assert_eq!(short.bars.len(), 2, "what the session opened holding");

        let mut handle = spawn(ReplayRequest {
            session: Arc::clone(&session),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                ..ReplayOptions::default()
            },
        });
        assert!(
            handle.capabilities.borrow().ohlcv_history,
            "a recording with a run-up beside it serves candles"
        );

        // The trader downloads more of it while the replay is open.
        std::fs::write(&context_path, context_text(&path, &tape, 5)).expect("grown context");

        handle
            .commands
            .blocking_send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                before_ms: None,
                slice_ms: None,
            })
            .expect("worker alive");

        let (interval_ms, bars, _complete) = next_candles(&mut handle);
        assert_eq!(interval_ms, crate::feed::OHLCV_BASE_INTERVAL_MS);
        assert_eq!(
            bars.len(),
            5,
            "the run-up as it stands on disk now, not the copy taken at open"
        );
    }

    /// A scratch directory of this test's own, emptied before it is used.
    ///
    /// Cleared up front rather than only at the end: an assertion that fails
    /// leaves the old one behind, Windows recycles pids freely, and a later
    /// run inheriting a populated directory is the stale-temp-dir flake this
    /// repo has already been bitten by elsewhere.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quantick-replay-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// `minutes` of broker candles in the hour before a tape starts — where a
    /// real context file sits: the market's past, never overlapping the
    /// recording.
    fn context_text(path: &std::path::Path, tape: &str, minutes: i64) -> String {
        let first = Session::from_text(path, tape, ParseOptions::default())
            .expect("session")
            .start_ms();
        let mut text = String::from("# interval_ms=60000\nDate,Time,Open,High,Low,Close,Volume\n");
        for i in 0..minutes {
            let (date, time) = quantick_replay::format::format_datetime(
                first - (minutes - i) * 60_000,
                quantick_replay::format::UtcOffset::UTC,
            );
            text.push_str(&format!("{date},{time},100,110,90,105,7\n"));
        }
        text
    }

    #[test]
    fn a_downloaded_context_answers_the_candle_request() {
        let mut handle = spawn(ReplayRequest {
            session: session_with_context(4, 90),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                skip_idle_over_ms: None,
            },
        });
        assert!(
            handle.capabilities.borrow().ohlcv_history,
            "the capability follows the file on disk, not the fact this is a replay"
        );

        handle
            .commands
            .blocking_send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                slice_ms: None,
                before_ms: None,
            })
            .expect("the worker is listening");

        let (interval_ms, bars, complete) = next_candles(&mut handle);
        assert_eq!(
            interval_ms, 60_000,
            "the interval is the file's, not assumed"
        );
        assert_eq!(bars.len(), 90);
        assert!(
            complete,
            "the whole downloaded context fits inside the span asked for"
        );
        assert!(
            bars.windows(2).all(|w| w[0].open_time < w[1].open_time),
            "candles run forwards"
        );
        // Broker candles have no aggressor split, all the way to the chart.
        assert!(bars.iter().all(|b| b.delta() == Decimal::ZERO));
    }

    /// Asking for the run-up mid-replay must not cost the trader their place.
    ///
    /// This test used to drive `LoadOlder`, because a recording used to claim
    /// it could page trades. It cannot, and never could — the tape in the file
    /// is the whole of it. The invariant it was really about is the playhead,
    /// and that belongs on the request a recording actually serves.
    #[test]
    fn the_candle_reach_never_moves_the_playhead() {
        let session = session_with_context(400, 90);
        let mut handle = spawn(ReplayRequest {
            session: Arc::clone(&session),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: true,
                skip_idle_over_ms: None,
            },
        });
        let link = handle.replay.clone().expect("replay link");
        assert!(
            handle.capabilities.borrow().ohlcv_history,
            "there is context on disk to pick up, so the candle reach is offered"
        );

        // Let playback get under way, then ask for more history mid-replay.
        let deadline = Instant::now() + Duration::from_secs(5);
        while link.status.played() == 0 && Instant::now() < deadline {
            std::thread::sleep(TICK_PLAYING);
        }
        let before = link.status.played();
        assert!(before > 0, "playback started");

        handle
            .commands
            .blocking_send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                before_ms: None,
                slice_ms: None,
            })
            .expect("the worker is listening");

        let (_, bars, _) = next_candles(&mut handle);
        assert!(!bars.is_empty(), "the reply carries candles, not trades");
        assert!(
            link.status.is_playing(),
            "asking for history never pauses the tape"
        );
        assert!(
            link.status.played() >= before,
            "and never rewinds it: the trader keeps their place"
        );
    }

    /// A `LoadOlder` that reaches a recording anyway — queued before a source
    /// switch, or sent by an operator through `QUANTICK_LOAD_OLDER` — is still
    /// answered, and answered empty.
    ///
    /// The UI raises a wait on every one of these and only a reply lowers it.
    /// A spinner left turning over a record that will never grow is the same
    /// dishonesty as the button that used to offer the gesture, in a smaller
    /// place.
    #[test]
    fn a_load_older_that_arrives_anyway_is_answered_rather_than_dropped() {
        let mut handle = spawn(ReplayRequest {
            session: session_with_context(4, 90),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                skip_idle_over_ms: None,
            },
        });
        handle
            .commands
            .blocking_send(FeedCommand::LoadOlder { count: 2_000 })
            .expect("the worker is listening");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "no reply: the loading indicator would turn forever"
            );
            match handle.events.try_recv() {
                Ok(FeedEvent::HistoryPrepended(trades)) => {
                    assert!(
                        trades.is_empty(),
                        "a recording has no older print to hand back"
                    );
                    break;
                }
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PAUSED),
                Err(mpsc::error::TryRecvError::Disconnected) => panic!("worker gone"),
            }
        }
    }

    #[test]
    fn a_span_shorter_than_the_context_says_there_is_more_before_it() {
        // The span is measured back from the recording's own first print, not
        // from the wall clock: a session recorded in March is replayed today,
        // and "the last hour" has to mean the market's hour.
        let mut handle = spawn(ReplayRequest {
            session: session_with_context(4, 90),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                skip_idle_over_ms: None,
            },
        });
        handle
            .commands
            .blocking_send(FeedCommand::FetchOhlcv {
                span_ms: 30 * 60_000,
                slice_ms: None,
                before_ms: None,
            })
            .expect("the worker is listening");

        let (_, bars, complete) = next_candles(&mut handle);
        assert_eq!(bars.len(), 30, "only the candles inside the span");
        assert!(
            !complete,
            "there is more market before the first bar, and the pane must be told"
        );
    }

    /// "Load older" on a recording reaches further back into the same context
    /// file. Nothing in the run-up is a venue call, so this is purely about
    /// where the window is placed: the first request takes the span ending at
    /// the recording's first print, and the second takes the span ending just
    /// before what that returned — meeting it exactly, never overlapping it.
    #[test]
    fn asking_for_older_context_reaches_past_the_span_already_answered() {
        let session = session_with_context(4, 90);
        let first_print = session.start_ms();
        let context = session.context.clone();
        let span = 30 * 60_000;

        let newest = match context_reply(context.as_ref(), first_print, span, None) {
            FeedEvent::OhlcvHistory { bars, .. } => bars,
            _ => panic!("a candle request is answered with candles"),
        };
        assert_eq!(newest.len(), 30, "the span ending at the first print");
        let oldest_held = newest.first().expect("30 bars").open_time;

        let older = match context_reply(context.as_ref(), first_print, span, Some(oldest_held - 1))
        {
            FeedEvent::OhlcvHistory { bars, .. } => bars,
            _ => panic!("a candle request is answered with candles"),
        };
        assert_eq!(older.len(), 30, "another span of the run-up");
        assert!(
            older.iter().all(|bar| bar.open_time < oldest_held),
            "and every bar of it is older than what was already held"
        );
    }

    /// The same session, with its first `joined` prints declared as the day
    /// before it — the shape `Session::load_with_day_before` produces.
    fn session_with_day_before(count: usize, joined: usize) -> Arc<Session> {
        let mut session = Session::clone(&session(count));
        session.day_before = Some(quantick_replay::JoinedDay {
            path: PathBuf::from("replay/TEST/20260313.csv"),
            date: quantick_replay::SessionDate::parse("20260313"),
            prints: joined,
        });
        Arc::new(session)
    }

    /// Drain up to the next backfill, or fail the test rather than hang.
    fn next_backfill(handle: &mut FeedHandle) -> Vec<Trade> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "no backfill: the chart would wait"
            );
            match handle.events.try_recv() {
                Ok(FeedEvent::Backfilled(batch)) => break batch,
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PAUSED),
                Err(mpsc::error::TryRecvError::Disconnected) => panic!("worker gone"),
            }
        }
    }

    /// What the trader who asked for this feature gets: yesterday's order flow
    /// already on the chart, the playhead parked on today's first print, and
    /// nothing moving until they say so.
    #[test]
    fn a_joined_day_opens_on_the_chart_with_the_tape_waiting() {
        let session = session_with_day_before(40, 12);
        let expected_open = session.trades[12].timestamp_ms;
        let mut handle = spawn(ReplayRequest {
            session,
            options: ReplayOptions::default(),
        });
        let link = handle.replay.clone().expect("replay link");

        let opening = next_backfill(&mut handle);
        assert_eq!(
            opening.len(),
            12,
            "the day before is handed over as history, whole"
        );
        assert!(
            !link.status.is_playing(),
            "and the default is to wait for the trader"
        );
        assert_eq!(link.status.played(), 12, "parked past what is on the chart");
        assert_eq!(
            link.day_prints(),
            (0, 28),
            "and the session's own day has not started — the one pair the \
             transport bar and the control plane both read"
        );
        assert_eq!(
            link.status.start_ms(),
            expected_open,
            "the session opens on its own day, not on the joined one"
        );
        assert_eq!(link.status.progress(), 0.0, "at the open, not half way");
    }

    #[test]
    fn a_session_with_nothing_joined_still_opens_on_an_empty_chart() {
        let mut handle = spawn(ReplayRequest {
            session: session(20),
            options: ReplayOptions::default(),
        });
        assert!(
            next_backfill(&mut handle).is_empty(),
            "today's behaviour, unchanged, when there is no day before"
        );
        let link = handle.replay.clone().expect("replay link");
        assert_eq!(link.status.played(), 0);
    }

    #[test]
    fn playing_a_joined_session_releases_only_the_day_that_was_chosen() {
        let mut handle = spawn(ReplayRequest {
            session: session_with_day_before(40, 12),
            options: ReplayOptions {
                speed: 50.0,
                autoplay: true,
                skip_idle_over_ms: Some(2_000),
            },
        });
        // The opening backfill is the joined day; everything after it is the
        // session playing. 40 prints, 12 of them already on the chart.
        let opening = next_backfill(&mut handle);
        assert_eq!(opening.len(), 12);
        let played = collect(&mut handle, 28);
        assert_eq!(played.len(), 28, "the day, and never the day before again");
        assert_eq!(
            played[0].price,
            Decimal::from(112),
            "play begins on the day's own first print"
        );
    }

    #[test]
    fn restarting_a_joined_session_puts_the_day_before_back_and_no_more() {
        let mut handle = spawn(ReplayRequest {
            session: session_with_day_before(40, 12),
            options: ReplayOptions {
                speed: 50.0,
                autoplay: true,
                skip_idle_over_ms: Some(2_000),
            },
        });
        let link = handle.replay.clone().expect("replay link");
        let deadline = Instant::now() + Duration::from_secs(5);
        while link.status.played() <= 12 && Instant::now() < deadline {
            std::thread::sleep(TICK_PLAYING);
        }
        assert!(link.status.played() > 12, "playback started");

        handle
            .commands
            .blocking_send(FeedCommand::Replay(ReplayControl::Restart))
            .expect("the worker is listening");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_reset = false;
        let mut history = Vec::new();
        while Instant::now() < deadline {
            match handle.events.try_recv() {
                Ok(FeedEvent::Reset) => saw_reset = true,
                Ok(FeedEvent::Backfilled(batch)) if saw_reset => {
                    history = batch;
                    break;
                }
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PLAYING),
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        assert!(saw_reset, "a restart rebuilds the chart");
        assert_eq!(
            history.len(),
            12,
            "back to the day's open, with yesterday still behind it"
        );
    }

    /// Drain events until `want` trades have arrived or the deadline passes.
    fn collect(handle: &mut FeedHandle, want: usize) -> Vec<Trade> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut trades = Vec::new();
        while trades.len() < want && Instant::now() < deadline {
            match handle.events.try_recv() {
                Ok(FeedEvent::LiveBatch(batch)) => trades.extend(batch),
                Ok(FeedEvent::Live(trade)) => trades.push(trade),
                Ok(FeedEvent::Backfilled(batch)) => trades.extend(batch),
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PLAYING),
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        trades
    }

    #[test]
    fn a_session_plays_out_through_the_live_channel() {
        let session = session(30);
        let mut handle = spawn(ReplayRequest {
            session,
            options: ReplayOptions {
                // 30 s of market time; at 50× that is well under a second.
                speed: 50.0,
                autoplay: true,
                skip_idle_over_ms: Some(2_000),
            },
        });
        let trades = collect(&mut handle, 30);
        assert_eq!(trades.len(), 30);
        assert_eq!(trades[0].price, Decimal::from(100));
        assert_eq!(trades[29].price, Decimal::from(129));
        assert!(
            trades
                .windows(2)
                .all(|w| w[0].timestamp_ms <= w[1].timestamp_ms)
        );

        let link = handle.replay.as_ref().expect("replay link");
        assert_eq!(link.symbol(), "TEST");
        assert_eq!(link.status.total(), 30);
    }

    #[test]
    fn a_paused_session_releases_nothing_until_play() {
        let mut handle = spawn(ReplayRequest {
            session: session(20),
            options: ReplayOptions {
                speed: 50.0,
                autoplay: false,
                ..Default::default()
            },
        });
        let link = handle.replay.clone().expect("replay link");
        assert!(!link.status.is_playing());

        std::thread::sleep(Duration::from_millis(60));
        assert!(collect(&mut handle, 1).is_empty(), "paused feed is silent");

        handle
            .commands
            .blocking_send(FeedCommand::Replay(ReplayControl::Play))
            .unwrap();
        assert_eq!(collect(&mut handle, 20).len(), 20);
        assert!(link.status.is_finished());
        assert_eq!(link.status.progress(), 1.0);
    }

    #[test]
    fn seeking_resets_the_chart_and_resends_history() {
        let mut handle = spawn(ReplayRequest {
            session: session(60),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                ..Default::default()
            },
        });
        handle
            .commands
            .blocking_send(FeedCommand::Replay(ReplayControl::SeekToFraction(0.5)))
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_reset = false;
        let mut history = Vec::new();
        while Instant::now() < deadline {
            match handle.events.try_recv() {
                Ok(FeedEvent::Reset) => saw_reset = true,
                Ok(FeedEvent::Backfilled(batch)) if saw_reset => {
                    history = batch;
                    break;
                }
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PLAYING),
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        assert!(saw_reset, "a seek resets the chart");
        // 60 prints one second apart: half way is 30 of them.
        assert_eq!(history.len(), 30);
        assert_eq!(history[0].side, Side::Buy);
    }

    #[test]
    fn a_burst_of_seeks_rebuilds_the_chart_once() {
        // A dragged seek handle can queue a command per frame; each rebuild
        // re-sends the session's history, so only the last position may be
        // applied. Play/pause/speed are orthogonal and keep their order.
        let controls = [
            ReplayControl::SeekToFraction(0.1),
            ReplayControl::SetSpeed(10.0),
            ReplayControl::SeekToFraction(0.4),
            ReplayControl::Restart,
            ReplayControl::Pause,
            ReplayControl::SeekToFraction(0.9),
        ];
        let (immediate, position) = coalesce(&controls);
        assert_eq!(
            immediate,
            vec![ReplayControl::SetSpeed(10.0), ReplayControl::Pause]
        );
        assert_eq!(position, Some(ReplayControl::SeekToFraction(0.9)));
    }

    #[test]
    fn a_restart_after_a_seek_is_the_position_that_wins() {
        let (immediate, position) =
            coalesce(&[ReplayControl::SeekToFraction(0.8), ReplayControl::Restart]);
        assert!(immediate.is_empty());
        assert_eq!(position, Some(ReplayControl::Restart));
    }

    #[test]
    fn speed_changes_reach_the_playhead() {
        let handle = spawn(ReplayRequest {
            session: session(10),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                ..Default::default()
            },
        });
        let link = handle.replay.clone().expect("replay link");
        handle
            .commands
            .blocking_send(FeedCommand::Replay(ReplayControl::SetSpeed(20.0)))
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while link.status.speed() != 20.0 && Instant::now() < deadline {
            std::thread::sleep(TICK_PLAYING);
        }
        assert_eq!(link.status.speed(), 20.0);
    }

    #[test]
    fn dropping_the_handle_stops_the_worker() {
        let handle = spawn(ReplayRequest {
            session: session(20_000),
            options: ReplayOptions::default(),
        });
        let link = handle.replay.clone().expect("replay link");
        drop(handle);
        // The worker holds the session Arc; once it exits, only the link's
        // clone remains.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&link.session) > 1 && Instant::now() < deadline {
            std::thread::sleep(TICK_PAUSED);
        }
        assert_eq!(Arc::strong_count(&link.session), 1, "worker exited");
    }

    #[test]
    fn a_recording_answers_a_candle_request_with_an_honest_nothing() {
        // The exactly-once contract, at the one provider that can never
        // satisfy the request. A recording holds ticks; the months of context
        // a time pane wants were never captured, and synthesizing them would
        // present data the file does not contain. So the answer is empty — but
        // it *is* an answer, because the pane's loading indicator keys on the
        // reply and silence would spin it forever.
        let mut handle = spawn(ReplayRequest {
            session: session(4),
            options: ReplayOptions {
                speed: 1.0,
                autoplay: false,
                skip_idle_over_ms: None,
            },
        });
        assert!(
            !handle.capabilities.borrow().ohlcv_history,
            "a file has no candles, and says so before anything is asked"
        );

        handle
            .commands
            .blocking_send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                slice_ms: None,
                before_ms: None,
            })
            .expect("the worker is listening");

        let deadline = Instant::now() + Duration::from_secs(5);
        let reply = loop {
            assert!(
                Instant::now() < deadline,
                "no reply: a pane would hang here"
            );
            match handle.events.try_recv() {
                Ok(FeedEvent::OhlcvHistory {
                    interval_ms,
                    bars,
                    slice,
                }) => {
                    // A recording always answers once and for all; the helper
                    // would be hiding a second reply if one ever appeared.
                    let crate::feed::OhlcvSlice::Last { complete } = slice else {
                        panic!("a recording must answer with a single closing slice");
                    };
                    break (interval_ms, bars, complete);
                }
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(TICK_PAUSED),
                Err(mpsc::error::TryRecvError::Disconnected) => panic!("worker gone"),
            }
        };
        let (interval_ms, bars, complete) = reply;
        assert!(
            complete,
            "a recording holds no candles; that answer is whole, not short"
        );
        assert!(
            bars.is_empty(),
            "nothing was recorded, so nothing is claimed"
        );
        assert_eq!(
            interval_ms,
            crate::feed::OHLCV_BASE_INTERVAL_MS,
            "the interval is tagged even on an empty answer, so a consumer never              has to guess what it would have been resampling"
        );
    }
}
