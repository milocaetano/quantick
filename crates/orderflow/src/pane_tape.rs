//! A pane's own tape, apart from its worker: the market clock the host
//! supplies, and the prints the pane draws on the frame they arrive, before
//! the worker publishes them. Heavy book history stays on the worker.
//!
//! Told the time, never reads it: the host hands in its monotonic instant or
//! the replay's position ([`PaneTape::follow_live`], [`PaneTape::follow_replay`]).
//! What the worker must hear is returned, never sent from here.

use std::sync::Arc;

use quantick_engine::Trade;

use crate::config::HeatmapConfig;
use crate::engine::{ProjectionRequest, VisibleOrderflow};
use crate::projection::PendingTape;
use crate::tape_clock::{TapeClock, TapeHold};

#[path = "pane_depth.rs"]
mod depth;

#[cfg(test)]
#[path = "pane_depth_tests.rs"]
mod depth_tests;

/// What recording one print asks of the worker that publishes the tape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingPrint {
    /// The epoch this print opens: the worker hears the config and this
    /// epoch before the print.
    pub opens: Option<u64>,
    /// The print's ordinal, which the worker's receipt acknowledges through.
    pub ordinal: u64,
}

/// The pane's clock, its unpublished prints and the frame drawn from them.
#[derive(Default)]
pub struct PaneTape {
    clock: TapeClock,
    /// Preview freshness follows source time independently of lane geometry.
    depth_clock: TapeClock,
    /// Nothing is coming: the window stops short of the clock, which runs on.
    hold: Option<TapeHold>,
    pending: PendingTape,
    /// The overlay frame last completed, while unpublished prints are drawn.
    frame: Option<Arc<VisibleOrderflow>>,
    depth: depth::CachedDepthPreview,
}

impl PaneTape {
    /// The lane's market clock. Only the native tape advances between market
    /// events; ordinary lanes keep their event-anchored clock, BTC's
    /// included.
    #[must_use]
    pub fn lane_now_ms(&self, native_tape: bool) -> Option<i64> {
        if native_tape {
            let now = self.clock.now_ms()?;
            Some(self.hold.map_or(now, |hold| hold.window_end_ms(now)))
        } else {
            None
        }
    }

    /// The market's clock whatever the pane draws: live, the last applied
    /// print advanced by the host's monotonic time; replayed, the source's
    /// playhead, stopped at the next print the chart has not applied. Never
    /// held: a window that stops says nothing about the time it is. `None`
    /// before the first print.
    #[must_use]
    pub fn market_now_ms(&self) -> Option<i64> {
        self.depth_clock.now_ms()
    }

    /// Say whether the window is held: the host's call each frame, from its link
    /// and its last print. `None` leaves the clock exactly as it was.
    pub fn set_hold(&mut self, hold: Option<TapeHold>) {
        self.hold = hold;
    }

    /// The latest instant a held window may end at, for the worker's own edge.
    #[must_use]
    pub fn window_cap_ms(&self) -> Option<i64> {
        self.hold.map(TapeHold::cap_ms)
    }

    /// Advance a live tape by the host's monotonic instant; a pane without
    /// a native tape keeps no clock.
    pub fn follow_live(&mut self, native_tape: bool, applied_ms: Option<i64>, monotonic_ms: u64) {
        self.depth_clock.live_at(applied_ms, monotonic_ms);
        if native_tape {
            self.clock.live_at(applied_ms, monotonic_ms);
        } else {
            self.clock.reset();
        }
    }

    /// Place a replayed tape at the replay's position, stopping at the next
    /// print the chart has not applied; a pane without a native tape keeps
    /// no clock.
    pub fn follow_replay(
        &mut self,
        native_tape: bool,
        position_ms: i64,
        applied_ms: Option<i64>,
        next_unapplied_ms: Option<i64>,
    ) {
        self.depth_clock
            .replay_at(position_ms, applied_ms, next_unapplied_ms);
        if native_tape {
            self.clock
                .replay_at(position_ms, applied_ms, next_unapplied_ms);
        } else {
            self.clock.reset();
        }
    }

    /// Another market: the clock starts over without an old anchor.
    pub fn reset_clock(&mut self) {
        self.clock.reset();
        self.depth_clock.reset();
        self.hold = None;
    }

    /// The prints the worker has not published yet.
    #[must_use]
    pub fn pending(&self) -> &PendingTape {
        &self.pending
    }

    /// The same, to acknowledge a receipt or observe the source's facts.
    pub fn pending_mut(&mut self) -> &mut PendingTape {
        &mut self.pending
    }

    /// The overlay frame last completed, while unpublished prints are drawn.
    #[must_use]
    pub fn frame(&self) -> Option<&Arc<VisibleOrderflow>> {
        self.frame.as_ref()
    }

    /// The frame this pane last drew for `published`
    /// ([`complete_frame`](Self::complete_frame)): the overlay while
    /// unpublished prints are drawn, else `published` with the verdict on its
    /// current book the painter used. The cursor reads this one, so it never
    /// reports a strip the painter no longer draws.
    #[must_use]
    pub fn drawn<'a>(
        &'a self,
        published: Option<&'a Arc<VisibleOrderflow>>,
    ) -> Option<&'a Arc<VisibleOrderflow>> {
        let published = published?;
        Some(
            self.frame
                .as_ref()
                .unwrap_or_else(|| self.depth.drawn(published)),
        )
    }

    /// Every opening burst the source recorded, with those `published`
    /// already folded.
    #[must_use]
    pub fn opening_bursts(&self, published: Option<&VisibleOrderflow>) -> Vec<i64> {
        self.pending
            .opening_bursts(published.map(|frame| frame.projection.as_ref()))
    }

    /// Hold `trade` for the pane's next frame. The first print of an epoch
    /// says so ([`PendingPrint::opens`]).
    pub fn record(&mut self, trade: &Trade, config: &HeatmapConfig) -> PendingPrint {
        let opens = (!self.pending.started()).then(|| self.pending.epoch());
        let ordinal = self.pending.record(trade, config);
        PendingPrint { opens, ordinal }
    }

    /// Begin another epoch: the unpublished prints and the frame drawn from
    /// them go; the source's opening facts stay. Returns the epoch the
    /// worker must hear.
    pub fn reset(&mut self) -> u64 {
        let epoch = self.pending.reset();
        self.frame = None;
        self.depth = depth::CachedDepthPreview::default();
        epoch
    }

    /// The frame to draw now: `published`, or — while a native tape of
    /// volume dots holds prints the worker has not published — that frame
    /// with them overlaid. `None` when the overlay cannot be placed.
    pub fn complete_frame(
        &mut self,
        config: &HeatmapConfig,
        request: &ProjectionRequest,
        published: Option<&Arc<VisibleOrderflow>>,
    ) -> Option<Arc<VisibleOrderflow>> {
        // A held window's end says nothing about how old the book is.
        let book_now = request
            .lane_now_ms
            .filter(|_| self.hold.is_none())
            .or_else(|| self.depth_clock.now_ms());
        let published = self.depth.consume(published, book_now);
        if !config.immediate_tape() || self.pending.is_empty() {
            self.frame = None;
            return published;
        }
        let frame = Arc::new(VisibleOrderflow::with_pending_overlay(
            &self.pending,
            config,
            request,
            published.as_deref(),
        )?);
        self.frame = Some(Arc::clone(&frame));
        Some(frame)
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;

    const HOLD: TapeHold = TapeHold {
        last_print_ms: 100_000,
        window_ms: 15_000,
    };

    /// With no hold the clock is what it always was; a hold stops the window and
    /// not the clock, so its release lands on the time it really is.
    #[test]
    fn a_hold_stops_the_window_and_its_release_finds_the_clock_where_it_ran() {
        let mut tape = PaneTape::default();
        tape.follow_live(true, Some(100_000), 0);
        tape.follow_live(true, Some(100_000), 60_000);
        assert_eq!(tape.lane_now_ms(true), Some(160_000));
        assert_eq!(tape.window_cap_ms(), None);

        tape.set_hold(Some(HOLD));
        assert_eq!(tape.lane_now_ms(true), Some(106_750));
        assert_eq!(tape.window_cap_ms(), Some(106_750));
        tape.follow_live(true, Some(100_000), 120_000);
        assert_eq!(tape.lane_now_ms(true), Some(106_750), "it stays held");

        tape.set_hold(None);
        assert_eq!(tape.lane_now_ms(true), Some(220_000));
    }

    /// A print moves the hold with it: the next frame's window is the clock's again.
    #[test]
    fn the_first_print_releases_the_window() {
        let mut tape = PaneTape::default();
        tape.follow_live(true, Some(100_000), 0);
        tape.set_hold(Some(HOLD));
        tape.follow_live(true, Some(160_000), 60_000);
        tape.set_hold(Some(TapeHold {
            last_print_ms: 160_000,
            ..HOLD
        }));
        assert_eq!(tape.lane_now_ms(true), Some(160_000));
    }

    /// The market clock runs for every pane, native tape or not, held or not,
    /// and a replay puts it at the playhead short of the next unapplied print.
    #[test]
    fn the_market_clock_runs_without_a_native_tape_and_through_a_hold() {
        let mut tape = PaneTape::default();
        assert_eq!(tape.market_now_ms(), None, "no print, no clock");
        tape.follow_live(false, Some(100_000), 0);
        tape.set_hold(Some(HOLD));
        tape.follow_live(false, Some(100_000), 30_000);
        assert_eq!(tape.market_now_ms(), Some(130_000));
        tape.follow_replay(false, 125_000, Some(100_000), Some(120_000));
        assert_eq!(
            tape.market_now_ms(),
            Some(120_000),
            "short of the next print"
        );
    }

    #[test]
    fn a_pane_without_a_native_tape_still_has_no_clock() {
        let mut tape = PaneTape::default();
        tape.follow_live(false, Some(100_000), 0);
        tape.set_hold(Some(HOLD));
        assert_eq!(tape.lane_now_ms(false), None);
        assert_eq!(
            tape.window_cap_ms(),
            Some(106_750),
            "its worker's edge holds"
        );
    }

    #[test]
    fn another_market_forgets_the_hold() {
        let mut tape = PaneTape::default();
        tape.set_hold(Some(HOLD));
        tape.reset_clock();
        assert_eq!(tape.window_cap_ms(), None);
    }
}
