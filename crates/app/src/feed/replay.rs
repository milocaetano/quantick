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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use quantick_replay::{PlaybackConfig, Playhead, Session};

use super::{FeedCommand, FeedEvent, FeedHandle};

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
    pub autoplay: bool,
    /// Market time that may pass with no prints before the clock skips ahead.
    pub skip_idle_over_ms: Option<i64>,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            speed: 1.0,
            autoplay: true,
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
}

impl ReplayStatus {
    fn new(playhead: &Playhead) -> Self {
        let status = Self {
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
    let mut playhead = Playhead::new(&request.session.trades, config);
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
        commands: cmd_tx,
        replay: Some(link),
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
        "replaying a recorded session"
    );

    // The chart opens empty and fills as the session plays; an empty backfill
    // resolves the UI's initial "loading history" state right away.
    if tx.blocking_send(FeedEvent::Backfilled(Vec::new())).is_err() {
        return;
    }

    let trades = &session.trades[..];
    let mut last = Instant::now();
    let mut reported_finished = false;

    let mut drained = Vec::new();
    loop {
        // 1. Controls first, so a click is honoured before the next batch.
        drained.clear();
        loop {
            match commands.try_recv() {
                Ok(FeedCommand::Replay(control)) => drained.push(control),
                // Live-feed commands have no meaning for a recording. Ignored
                // rather than refused: the UI gates them by capability, and a
                // stray one must not kill playback.
                Ok(_) => {}
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }
        if !drained.is_empty() {
            let (immediate, position) = coalesce(&drained);
            for control in immediate {
                if !apply(control, &mut playhead, trades, &tx, &session) {
                    return; // UI gone
                }
            }
            if let Some(control) = position
                && !apply(control, &mut playhead, trades, &tx, &session)
            {
                return;
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
}
