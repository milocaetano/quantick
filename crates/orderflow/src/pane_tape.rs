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
use crate::tape_clock::TapeClock;

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
    pending: PendingTape,
    /// The overlay frame last completed, while unpublished prints are drawn.
    frame: Option<Arc<VisibleOrderflow>>,
}

impl PaneTape {
    /// The lane's market clock. Only the native tape advances between market
    /// events; ordinary lanes keep their event-anchored clock, BTC's
    /// included.
    #[must_use]
    pub fn lane_now_ms(&self, native_tape: bool) -> Option<i64> {
        if native_tape {
            self.clock.now_ms()
        } else {
            None
        }
    }

    /// Advance a live tape by the host's monotonic instant; a pane without
    /// a native tape keeps no clock.
    pub fn follow_live(&mut self, native_tape: bool, applied_ms: Option<i64>, monotonic_ms: u64) {
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
        if !config.immediate_tape() || self.pending.is_empty() {
            self.frame = None;
            return published.cloned();
        }
        let frame = Arc::new(VisibleOrderflow::with_pending_overlay(
            &self.pending,
            config,
            request,
            published.map(Arc::as_ref),
        )?);
        self.frame = Some(Arc::clone(&frame));
        Some(frame)
    }
}
