//! The playback clock: how much of a recorded session has been reached.
//!
//! Deterministic and clock-free — [`Playhead::advance`] is told how much real
//! time passed, it never reads one. The same sequence of deltas always emits the
//! same trades in the same batches, so playback is unit-tested without sleeping
//! and a replay-driven backtest replays identically to the chart.
//!
//! The playhead owns no trades. Callers pass the session's trade slice into each
//! method, which keeps this struct copyable, lifetime-free, and cheap to hand to
//! a worker thread alongside an `Arc<[Trade]>`.

use quantick_engine::Trade;

/// The speeds the transport offers, in market-seconds per real second.
pub const SPEEDS: [f32; 7] = [1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 50.0];

/// Slowest and fastest speed the clock accepts, so a stray value cannot stall
/// playback or run a session out in one frame.
pub const MIN_SPEED: f32 = 0.1;
/// See [`MIN_SPEED`].
pub const MAX_SPEED: f32 = 1000.0;

/// Market time that may pass with no prints before the clock jumps ahead.
///
/// Recorded sessions have holes — the lunch lull, a halt, the gap between the
/// close and the next morning. Waiting through them in real time is not replay,
/// it is staring at a still chart, so every platform that ships a replay skips
/// them. Two seconds is short enough that the rhythm of live trading survives.
pub const DEFAULT_SKIP_IDLE_MS: i64 = 2_000;

/// Most trades one [`Playhead::advance`] hands back.
///
/// A cap, not a filter: prints beyond it are delivered by the next call, and the
/// clock stays with them instead of skipping ahead. Sized well above a real B3
/// burst at 50×, so it only ever engages on pathological data.
pub const DEFAULT_MAX_BATCH: usize = 20_000;

/// How a session is played back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackConfig {
    /// Market seconds per real second.
    pub speed: f32,
    /// Jump forward when the next print is further away than this. `None` plays
    /// every idle gap in full.
    pub skip_idle_over_ms: Option<i64>,
    /// Upper bound on trades returned per advance.
    pub max_batch: usize,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            skip_idle_over_ms: Some(DEFAULT_SKIP_IDLE_MS),
            max_batch: DEFAULT_MAX_BATCH,
        }
    }
}

/// A range of trades to emit, as indices into the session's trade slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Batch {
    /// First trade index, inclusive.
    pub start: usize,
    /// Last trade index, exclusive.
    pub end: usize,
    /// Whether the clock jumped over an idle gap to reach this batch.
    pub skipped_gap: bool,
}

impl Batch {
    /// An empty batch at `at`.
    #[must_use]
    pub fn empty(at: usize) -> Self {
        Self {
            start: at,
            end: at,
            skipped_gap: false,
        }
    }

    /// How many trades the batch covers.
    #[must_use]
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether there is nothing to emit.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// The batch as a slice range.
    #[must_use]
    pub fn range(self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// Where playback has reached in a session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    position_ms: i64,
    start_ms: i64,
    end_ms: i64,
    cursor: usize,
    first: usize,
    total: usize,
    playing: bool,
    config: PlaybackConfig,
}

impl Playhead {
    /// A playhead parked on the first trade of `trades`, paused.
    ///
    /// An empty session yields a playhead that is immediately finished.
    #[must_use]
    pub fn new(trades: &[Trade], config: PlaybackConfig) -> Self {
        Self::opening_at(trades, config, 0)
    }

    /// A playhead whose session opens at `first`, paused on that print.
    ///
    /// The prints before `first` are not part of the session being played:
    /// they are the day joined in front of it, and the caller has already put
    /// them on the chart as history. So the clock treats `first` as the open —
    /// [`Playhead::restart`] returns there, a seek stays at or after it, and
    /// [`Playhead::progress`] measures the session, not the joined stream.
    /// They are still *in* the slice, because index and timestamp have to keep
    /// meaning the same thing to every caller.
    ///
    /// `first` past the end parks a finished playhead: a joined day with no
    /// session behind it has played out before it began.
    #[must_use]
    pub fn opening_at(trades: &[Trade], config: PlaybackConfig, first: usize) -> Self {
        let first = first.min(trades.len());
        // The session's own first print. Past the end there is none, and the
        // last print of what was joined is the only honest reading left.
        let start_ms = trades
            .get(first)
            .or_else(|| trades.last())
            .map_or(0, |t| t.timestamp_ms);
        Self {
            position_ms: start_ms,
            start_ms,
            end_ms: trades.last().map_or(0, |t| t.timestamp_ms),
            cursor: first,
            first,
            total: trades.len(),
            playing: false,
            config: PlaybackConfig {
                speed: config.speed.clamp(MIN_SPEED, MAX_SPEED),
                // A cap of zero would hand back an empty batch forever, leaving
                // the cursor where it is: playback would stall in silence, with
                // nothing to see and nothing to report. One print per advance is
                // slow, but it always finishes.
                max_batch: config.max_batch.max(1),
                ..config
            },
        }
    }

    /// Advance by `wall_delta_ms` of real time and return the trades that fall
    /// due, as indices into the same slice the playhead was built from.
    ///
    /// Returns an empty batch when paused, finished, or when no print is due
    /// yet. Negative or non-finite deltas are treated as zero: a clock that
    /// jumps backwards must never rewind the market.
    pub fn advance(&mut self, trades: &[Trade], wall_delta_ms: f64) -> Batch {
        if !self.playing || self.is_finished() {
            return Batch::empty(self.cursor);
        }
        let delta = if wall_delta_ms.is_finite() && wall_delta_ms > 0.0 {
            (wall_delta_ms * f64::from(self.config.speed)).round() as i64
        } else {
            0
        };
        let mut target = self.position_ms.saturating_add(delta);

        // Nothing prints for a while: jump to the next print rather than
        // playing an empty chart in real time.
        let mut skipped_gap = false;
        if let Some(limit) = self.config.skip_idle_over_ms
            && let Some(next) = trades.get(self.cursor)
            && next.timestamp_ms.saturating_sub(target) > limit
        {
            target = next.timestamp_ms;
            skipped_gap = true;
        }

        let due = self.cursor + trades[self.cursor..].partition_point(|t| t.timestamp_ms <= target);
        let end = due.min(self.cursor.saturating_add(self.config.max_batch));

        if end < due {
            // Capped: stay with the prints that were not delivered instead of
            // stepping over them. The clock falls behind, honestly.
            self.position_ms = trades[end.saturating_sub(1)].timestamp_ms;
        } else {
            self.position_ms = target.min(self.end_ms);
        }

        let batch = Batch {
            start: self.cursor,
            end,
            skipped_gap,
        };
        self.cursor = end;
        batch
    }

    /// Move to `target_ms` of market time, returning the trades between the old
    /// and new position when seeking forward.
    ///
    /// Seeking backwards returns an empty batch: the caller has to rebuild its
    /// chart from the new cursor, because bars that were already closed cannot
    /// be un-closed. [`Playhead::cursor`] is where to restart from.
    pub fn seek_to(&mut self, trades: &[Trade], target_ms: i64) -> Batch {
        let target = target_ms.clamp(self.start_ms, self.end_ms);
        let index = trades.partition_point(|t| t.timestamp_ms <= target);
        let batch = if index >= self.cursor {
            Batch {
                start: self.cursor,
                end: index,
                skipped_gap: true,
            }
        } else {
            Batch::empty(index)
        };
        self.position_ms = target;
        self.cursor = index;
        batch
    }

    /// Seek to a fraction of the session, `0.0` being the open and `1.0` the
    /// close. See [`Playhead::seek_to`] for the backwards case.
    pub fn seek_to_fraction(&mut self, trades: &[Trade], fraction: f32) -> Batch {
        let span = self.end_ms.saturating_sub(self.start_ms) as f64;
        let offset = (span * f64::from(fraction.clamp(0.0, 1.0))).round() as i64;
        self.seek_to(trades, self.start_ms.saturating_add(offset))
    }

    /// Park back on the session's first trade, keeping speed and play state.
    ///
    /// Back to *the session's* open, never into a day joined in front of it: a
    /// trader restarting a rehearsal is asking for the day again, not for its
    /// run-up to replay.
    pub fn restart(&mut self) {
        self.position_ms = self.start_ms;
        self.cursor = self.first;
    }

    /// Start playing. A finished session restarts from the open, which is what
    /// pressing play at the end of a replay is asking for.
    pub fn play(&mut self) {
        if self.is_finished() {
            self.restart();
        }
        self.playing = true;
    }

    /// Stop advancing; the position is kept.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Toggle between playing and paused.
    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Change the speed, clamped to [`MIN_SPEED`]..=[`MAX_SPEED`]. The position
    /// is untouched, so speed can be changed mid-playback without a jump.
    pub fn set_speed(&mut self, speed: f32) {
        self.config.speed = speed.clamp(MIN_SPEED, MAX_SPEED);
    }

    /// The current speed multiplier.
    #[must_use]
    pub fn speed(&self) -> f32 {
        self.config.speed
    }

    /// Whether playback is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Whether every trade has been emitted.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.cursor >= self.total
    }

    /// Index of the next trade to emit.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Market time reached, in epoch milliseconds.
    #[must_use]
    pub fn position_ms(&self) -> i64 {
        self.position_ms
    }

    /// Timestamp of the session's first trade.
    #[must_use]
    pub fn start_ms(&self) -> i64 {
        self.start_ms
    }

    /// Timestamp of the session's last trade.
    #[must_use]
    pub fn end_ms(&self) -> i64 {
        self.end_ms
    }

    /// How far through the session playback is, `0.0`..=`1.0`.
    #[must_use]
    pub fn progress(&self) -> f32 {
        let span = self.end_ms.saturating_sub(self.start_ms);
        if span <= 0 {
            return if self.is_finished() { 1.0 } else { 0.0 };
        }
        let elapsed = self.position_ms.saturating_sub(self.start_ms) as f64;
        (elapsed / span as f64).clamp(0.0, 1.0) as f32
    }

    /// How many trades the session holds, the joined day included.
    #[must_use]
    pub fn total(&self) -> usize {
        self.total
    }

    /// Index of the session's own first trade: how many prints in front of it
    /// came from a joined day, and `0` when none did.
    #[must_use]
    pub fn first(&self) -> usize {
        self.first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quantick_engine::Side;
    use rust_decimal::Decimal;

    /// Trades one second apart, starting at epoch 10_000 ms.
    fn ticks(count: usize) -> Vec<Trade> {
        (0..count)
            .map(|i| Trade {
                agg_id: i as u64 + 1,
                timestamp_ms: 10_000 + i as i64 * 1_000,
                price: Decimal::from(100),
                quantity: Decimal::ONE,
                side: Side::Buy,
            })
            .collect()
    }

    fn playing(trades: &[Trade], speed: f32) -> Playhead {
        let mut head = Playhead::new(
            trades,
            PlaybackConfig {
                speed,
                ..Default::default()
            },
        );
        head.play();
        head
    }

    #[test]
    fn a_paused_playhead_emits_nothing() {
        let trades = ticks(5);
        let mut head = Playhead::new(&trades, PlaybackConfig::default());
        assert!(head.advance(&trades, 5_000.0).is_empty());
        assert_eq!(head.cursor(), 0);
        assert!(!head.is_playing());
    }

    #[test]
    fn one_times_speed_follows_market_time() {
        let trades = ticks(5);
        let mut head = playing(&trades, 1.0);
        // The playhead starts on the first trade's timestamp, so it is due at once.
        assert_eq!(head.advance(&trades, 0.0).len(), 1);
        assert_eq!(head.advance(&trades, 999.0).len(), 0, "not due yet");
        assert_eq!(
            head.advance(&trades, 1.0).len(),
            1,
            "the next second's print"
        );
        assert_eq!(head.advance(&trades, 2_000.0).len(), 2);
        assert!(!head.is_finished(), "one print still to come");
        assert_eq!(head.advance(&trades, 1_000.0).len(), 1);
        assert!(head.is_finished());
    }

    #[test]
    fn speed_multiplies_market_time_per_real_second() {
        let trades = ticks(51);
        let mut head = playing(&trades, 50.0);
        // One real second at 50× covers 50 s of market time: 51 prints.
        let batch = head.advance(&trades, 1_000.0);
        assert_eq!(batch.len(), 51);
        assert!(head.is_finished());
    }

    #[test]
    fn every_offered_speed_plays_a_session_out() {
        for speed in SPEEDS {
            let trades = ticks(120);
            let mut head = playing(&trades, speed);
            let mut emitted = 0;
            // 120 s of market time, in 16 ms frames, at the speed under test.
            let frames = (120_000.0 / (16.0 * f64::from(speed))).ceil() as usize + 2;
            for _ in 0..frames {
                emitted += head.advance(&trades, 16.0).len();
            }
            assert_eq!(emitted, trades.len(), "speed {speed}×");
            assert!(head.is_finished(), "speed {speed}×");
        }
    }

    #[test]
    fn playback_is_deterministic_for_the_same_deltas() {
        let trades = ticks(200);
        let run = || {
            let mut head = playing(&trades, 10.0);
            let mut batches = Vec::new();
            for step in 0..40 {
                batches.push(head.advance(&trades, f64::from(step % 7) * 8.0));
            }
            (batches, head)
        };
        let (first_batches, first) = run();
        let (second_batches, second) = run();
        assert_eq!(first_batches, second_batches);
        assert_eq!(first, second);
    }

    #[test]
    fn idle_gaps_are_skipped_to_the_next_print() {
        let mut trades = ticks(2);
        // A one-hour hole before the third print.
        trades.push(Trade {
            timestamp_ms: trades[1].timestamp_ms + 3_600_000,
            ..trades[1].clone()
        });
        let mut head = playing(&trades, 1.0);
        head.advance(&trades, 0.0); // first print
        head.advance(&trades, 1_000.0); // second print
        let batch = head.advance(&trades, 16.0);
        assert_eq!(batch.len(), 1, "the print after the gap arrives at once");
        assert!(batch.skipped_gap);
        assert_eq!(head.position_ms(), trades[2].timestamp_ms);
    }

    #[test]
    fn gap_skipping_can_be_turned_off() {
        let mut trades = ticks(1);
        trades.push(Trade {
            timestamp_ms: trades[0].timestamp_ms + 3_600_000,
            ..trades[0].clone()
        });
        let mut head = Playhead::new(
            &trades,
            PlaybackConfig {
                speed: 1.0,
                skip_idle_over_ms: None,
                ..Default::default()
            },
        );
        head.play();
        head.advance(&trades, 0.0);
        assert!(
            head.advance(&trades, 16.0).is_empty(),
            "the gap is played out"
        );
    }

    #[test]
    fn a_burst_larger_than_the_cap_is_delivered_across_calls() {
        // 100 prints in the same millisecond.
        let trades: Vec<Trade> = (0..100)
            .map(|i| Trade {
                agg_id: i + 1,
                timestamp_ms: 10_000,
                price: Decimal::from(100),
                quantity: Decimal::ONE,
                side: Side::Buy,
            })
            .collect();
        let mut head = Playhead::new(
            &trades,
            PlaybackConfig {
                speed: 1.0,
                max_batch: 30,
                ..Default::default()
            },
        );
        head.play();
        let mut emitted = 0;
        for _ in 0..4 {
            emitted += head.advance(&trades, 16.0).len();
        }
        assert_eq!(emitted, 100, "no print is skipped by the cap");
        assert!(head.is_finished());
    }

    #[test]
    fn a_zero_batch_cap_cannot_stall_playback() {
        let trades = ticks(5);
        let mut head = Playhead::new(
            &trades,
            PlaybackConfig {
                speed: 1.0,
                max_batch: 0,
                ..Default::default()
            },
        );
        head.play();
        let mut emitted = 0;
        for _ in 0..trades.len() {
            emitted += head.advance(&trades, 1_000.0).len();
        }
        assert_eq!(emitted, trades.len(), "every print is still delivered");
        assert!(head.is_finished());
    }

    #[test]
    fn seeking_forward_hands_back_everything_in_between() {
        let trades = ticks(100);
        let mut head = playing(&trades, 1.0);
        head.advance(&trades, 0.0);
        let batch = head.seek_to(&trades, trades[49].timestamp_ms);
        assert_eq!(batch.start, 1);
        assert_eq!(batch.end, 50);
        assert_eq!(head.cursor(), 50);
    }

    #[test]
    fn seeking_backwards_rewinds_the_cursor_and_emits_nothing() {
        let trades = ticks(100);
        let mut head = playing(&trades, 1.0);
        head.advance(&trades, 60_000.0);
        assert!(head.cursor() > 50);
        let batch = head.seek_to(&trades, trades[10].timestamp_ms);
        assert!(batch.is_empty());
        assert_eq!(head.cursor(), 11, "restart from here");
    }

    #[test]
    fn seeking_by_fraction_lands_inside_the_session() {
        let trades = ticks(101);
        let mut head = playing(&trades, 1.0);
        head.seek_to_fraction(&trades, 0.5);
        assert_eq!(head.position_ms(), trades[50].timestamp_ms);
        assert!((head.progress() - 0.5).abs() < 0.001);
        head.seek_to_fraction(&trades, 2.0);
        assert_eq!(head.position_ms(), head.end_ms());
        head.seek_to_fraction(&trades, -1.0);
        assert_eq!(head.position_ms(), head.start_ms());
    }

    #[test]
    fn play_at_the_end_starts_the_session_over() {
        let trades = ticks(3);
        let mut head = playing(&trades, 1.0);
        head.advance(&trades, 10_000.0);
        assert!(head.is_finished());
        head.play();
        assert!(!head.is_finished());
        assert_eq!(head.cursor(), 0);
        assert_eq!(head.position_ms(), head.start_ms());
    }

    #[test]
    fn speed_changes_do_not_move_the_playhead() {
        let trades = ticks(50);
        let mut head = playing(&trades, 1.0);
        head.advance(&trades, 5_000.0);
        let before = (head.position_ms(), head.cursor());
        head.set_speed(20.0);
        assert_eq!((head.position_ms(), head.cursor()), before);
        assert_eq!(head.speed(), 20.0);
    }

    #[test]
    fn speed_is_clamped_to_a_sane_band() {
        let trades = ticks(2);
        let mut head = playing(&trades, 1.0);
        head.set_speed(0.0);
        assert_eq!(head.speed(), MIN_SPEED);
        head.set_speed(f32::INFINITY);
        assert_eq!(head.speed(), MAX_SPEED);
    }

    #[test]
    fn a_backwards_or_broken_delta_never_rewinds() {
        let trades = ticks(10);
        let mut head = playing(&trades, 1.0);
        head.advance(&trades, 0.0);
        let before = head.position_ms();
        for delta in [-1_000.0, f64::NAN, f64::INFINITY] {
            head.advance(&trades, delta);
        }
        assert_eq!(head.position_ms(), before);
        assert_eq!(head.cursor(), 1);
    }

    #[test]
    fn an_empty_session_is_finished_immediately() {
        let trades: Vec<Trade> = Vec::new();
        let mut head = playing(&trades, 1.0);
        assert!(head.is_finished());
        assert!(head.advance(&trades, 1_000.0).is_empty());
        assert_eq!(head.progress(), 1.0);
    }

    #[test]
    fn progress_runs_from_open_to_close() {
        let trades = ticks(11);
        let mut head = playing(&trades, 1.0);
        assert_eq!(head.progress(), 0.0);
        head.advance(&trades, 5_000.0);
        assert!((head.progress() - 0.5).abs() < 0.001);
        head.advance(&trades, 60_000.0);
        assert_eq!(head.progress(), 1.0);
    }

    /// A session with the day before joined in front of it: 4 prints of
    /// yesterday, then 11 of the day being rehearsed.
    fn joined() -> (Vec<Trade>, usize) {
        (ticks(15), 4)
    }

    #[test]
    fn a_session_opening_past_a_joined_day_parks_on_its_own_first_print() {
        let (trades, first) = joined();
        let head = Playhead::opening_at(&trades, PlaybackConfig::default(), first);
        assert_eq!(
            head.cursor(),
            first,
            "the joined day is already on the chart"
        );
        assert_eq!(head.first(), first);
        assert_eq!(head.position_ms(), trades[first].timestamp_ms);
        assert_eq!(head.start_ms(), trades[first].timestamp_ms);
        assert_eq!(
            head.total(),
            trades.len(),
            "the whole stream is still there"
        );
        assert!(!head.is_finished());
        assert_eq!(head.progress(), 0.0, "at the open of the day, not half way");
    }

    #[test]
    fn playing_a_joined_session_never_replays_the_day_before() {
        let (trades, first) = joined();
        let mut head = Playhead::opening_at(
            &trades,
            PlaybackConfig {
                speed: 1.0,
                ..Default::default()
            },
            first,
        );
        head.play();
        let batch = head.advance(&trades, 0.0);
        assert_eq!(
            batch.range().start,
            first,
            "the first print released is the day's own"
        );
        let mut emitted = batch.len();
        for _ in 0..12 {
            emitted += head.advance(&trades, 1_000.0).len();
        }
        assert_eq!(emitted, trades.len() - first, "the day, and only the day");
        assert!(head.is_finished());
    }

    #[test]
    fn restarting_a_joined_session_returns_to_the_days_open() {
        let (trades, first) = joined();
        let mut head = Playhead::opening_at(&trades, PlaybackConfig::default(), first);
        head.play();
        head.advance(&trades, 5_000.0);
        head.restart();
        assert_eq!(head.cursor(), first, "not back into yesterday");
        assert_eq!(head.position_ms(), trades[first].timestamp_ms);
        assert_eq!(head.progress(), 0.0);
    }

    #[test]
    fn a_seek_on_a_joined_session_stays_inside_the_day() {
        let (trades, first) = joined();
        let mut head = Playhead::opening_at(&trades, PlaybackConfig::default(), first);
        head.play();
        head.advance(&trades, 60_000.0);
        // Fully back: the earliest a seek may land is the day's own open.
        head.seek_to_fraction(&trades, 0.0);
        assert!(
            head.cursor() > first,
            "the day's first print has been served"
        );
        assert_eq!(head.position_ms(), trades[first].timestamp_ms);
        // And below it, clamped rather than reaching into the joined day.
        head.seek_to(&trades, trades[0].timestamp_ms);
        assert_eq!(head.position_ms(), trades[first].timestamp_ms);
        assert!(head.cursor() > first);
    }

    #[test]
    fn a_joined_day_with_no_session_behind_it_is_finished() {
        let trades = ticks(4);
        let head = Playhead::opening_at(&trades, PlaybackConfig::default(), trades.len());
        assert!(head.is_finished(), "there is no day left to play");
        assert_eq!(head.progress(), 1.0);
    }

    #[test]
    fn a_playhead_with_nothing_joined_is_the_one_that_was_always_there() {
        let trades = ticks(6);
        assert_eq!(
            Playhead::opening_at(&trades, PlaybackConfig::default(), 0),
            Playhead::new(&trades, PlaybackConfig::default())
        );
    }
}
